// Copyright 2026 Joseph Anthony Abbott III
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Wall time: [`Timestamp`], and the [`Clock`] trait that every component
//! needing the time of day takes, so tests can drive time with a
//! [`ManualClock`]. Durations are measured with `std::time::Instant`, not
//! with a `Clock`.
//!
//! ```
//! use std::time::Duration;
//! use softshell_core::clock::{Clock, ManualClock, Timestamp};
//!
//! let clock = ManualClock::new(Timestamp::UNIX_EPOCH);
//! clock.advance(Duration::from_millis(1500));
//! assert_eq!(clock.now().as_millis(), 1500);
//! assert_eq!(clock.now().to_string(), "1970-01-01T00:00:01.500000000Z");
//! ```

use std::fmt;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, SystemTime};

use crate::civil::{self, CivilError};

const NANOS_PER_MILLI: i64 = 1_000_000;

/// A point in time: nanoseconds since the Unix epoch, UTC, as an `i64`. It
/// spans [`MIN`](Self::MIN) (1677-09-21T00:12:43.145224192Z) to
/// [`MAX`](Self::MAX) (2262-04-11T23:47:16.854775807Z) (ADR-0013). Leap
/// seconds are not counted, as in Unix time.
///
/// `Display` and `FromStr` use RFC 3339 (see [`civil`]); `Debug` shows
/// the same text.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(i64);

impl Timestamp {
    /// The earliest timestamp, 1677-09-21T00:12:43.145224192Z.
    pub const MIN: Self = Self(i64::MIN);
    /// The latest timestamp, 2262-04-11T23:47:16.854775807Z.
    pub const MAX: Self = Self(i64::MAX);
    /// 1970-01-01T00:00:00Z.
    pub const UNIX_EPOCH: Self = Self(0);

    /// The timestamp `nanos` nanoseconds after the epoch (before it, if
    /// negative).
    #[must_use]
    pub const fn from_nanos(nanos: i64) -> Self {
        Self(nanos)
    }

    /// Nanoseconds since the epoch.
    #[must_use]
    pub const fn as_nanos(self) -> i64 {
        self.0
    }

    /// The timestamp `millis` milliseconds after the epoch, or `None` if
    /// that is outside the range. `from_millis(t.as_millis())` is `Some` for
    /// every `t` except the first 775,808 ns of the range, whose millisecond
    /// starts before [`MIN`](Self::MIN).
    #[must_use]
    pub const fn from_millis(millis: i64) -> Option<Self> {
        match millis.checked_mul(NANOS_PER_MILLI) {
            Some(nanos) => Some(Self(nanos)),
            None => None,
        }
    }

    /// Whole milliseconds since the epoch, rounded down: one nanosecond
    /// before the epoch is millisecond -1.
    #[must_use]
    pub const fn as_millis(self) -> i64 {
        self.0.div_euclid(NANOS_PER_MILLI)
    }

    /// The timestamp `duration` later, or `None` past [`MAX`](Self::MAX).
    #[must_use]
    pub fn checked_add(self, duration: Duration) -> Option<Self> {
        // Up to `u64::MAX` ns can be added: that is `MIN` to `MAX`.
        let nanos = u64::try_from(duration.as_nanos()).ok()?;
        self.0.checked_add_unsigned(nanos).map(Self)
    }

    /// The timestamp `duration` earlier, or `None` before
    /// [`MIN`](Self::MIN).
    #[must_use]
    pub fn checked_sub(self, duration: Duration) -> Option<Self> {
        // `i64::MIN` nanoseconds is a valid step back from a positive time,
        // but does not fit a positive `i64`.
        let nanos = i128::from(self.0) - i128::try_from(duration.as_nanos()).ok()?;
        i64::try_from(nanos).ok().map(Self)
    }
}

/// RFC 3339 with nine fraction digits, as [`civil::format_rfc3339`] writes
/// it.
impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        civil::write_rfc3339(*self, f)
    }
}

impl fmt::Debug for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Timestamp({self})")
    }
}

/// RFC 3339, as [`civil::parse_rfc3339`] reads it.
impl FromStr for Timestamp {
    type Err = CivilError;

    fn from_str(text: &str) -> Result<Self, CivilError> {
        civil::parse_rfc3339(text)
    }
}

/// A source of wall time. Components that need the time of day (ids, TTL,
/// retention) take one, so tests can use a [`ManualClock`].
pub trait Clock: Send + Sync {
    /// The current time.
    fn now(&self) -> Timestamp;
}

impl<C: Clock + ?Sized> Clock for &C {
    fn now(&self) -> Timestamp {
        (**self).now()
    }
}

impl<C: Clock + ?Sized> Clock for Box<C> {
    fn now(&self) -> Timestamp {
        (**self).now()
    }
}

impl<C: Clock + ?Sized> Clock for Arc<C> {
    fn now(&self) -> Timestamp {
        (**self).now()
    }
}

/// The operating system's wall clock (`std::time::SystemTime`). It can
/// step backwards when the system time is set.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    /// The system time. A system clock set outside the [`Timestamp`] range
    /// gives [`Timestamp::MIN`] or [`Timestamp::MAX`].
    fn now(&self) -> Timestamp {
        from_system_time(SystemTime::now())
    }
}

/// `time` as a [`Timestamp`], saturating at the ends of the range.
fn from_system_time(time: SystemTime) -> Timestamp {
    let nanos = match time.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_nanos()).unwrap_or(i64::MAX),
        // `i64::MIN` is -2^63, one more than `i64::MAX` in size, so it is
        // reached by saturating, not by negating.
        Err(before) => i64::try_from(before.duration().as_nanos()).map_or(i64::MIN, |nanos| -nanos),
    };
    Timestamp(nanos)
}

/// A clock that only moves when told to, for tests. It can be shared
/// between threads (`&ManualClock` and `Arc<ManualClock>` are clocks too).
#[derive(Debug, Default)]
pub struct ManualClock {
    nanos: AtomicI64,
}

impl ManualClock {
    /// A clock reading `start`.
    #[must_use]
    pub const fn new(start: Timestamp) -> Self {
        Self {
            nanos: AtomicI64::new(start.0),
        }
    }

    /// Sets the time, forwards or backwards.
    pub fn set(&self, time: Timestamp) {
        self.nanos.store(time.0, Ordering::SeqCst);
    }

    /// Moves the time forward by `duration`, stopping at
    /// [`Timestamp::MAX`].
    pub fn advance(&self, duration: Duration) {
        // `u64::MAX` ns reaches `MAX` from anywhere, so the clamp is exact.
        let step = u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX);
        self.nanos
            .update(Ordering::SeqCst, Ordering::SeqCst, |nanos| {
                nanos.saturating_add_unsigned(step)
            });
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Timestamp {
        Timestamp(self.nanos.load(Ordering::SeqCst))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn millis_round_down() {
        assert_eq!(Timestamp::from_nanos(1_999_999).as_millis(), 1);
        assert_eq!(Timestamp::from_nanos(-1).as_millis(), -1);
        assert_eq!(Timestamp::from_nanos(-1_000_000).as_millis(), -1);
        assert_eq!(Timestamp::from_nanos(-1_000_001).as_millis(), -2);
        assert_eq!(Timestamp::MIN.as_millis(), -9_223_372_036_855);
        assert_eq!(Timestamp::MAX.as_millis(), 9_223_372_036_854);
        assert_eq!(Timestamp::from_millis(-3), Some(Timestamp(-3_000_000)));
        assert_eq!(
            Timestamp::from_millis(9_223_372_036_854),
            Some(Timestamp(9_223_372_036_854_000_000))
        );
        assert_eq!(Timestamp::from_millis(9_223_372_036_855), None);
        assert_eq!(Timestamp::from_millis(-9_223_372_036_855), None);
        // The edge where a millisecond starts before MIN.
        let edge = Timestamp::from_nanos(-9_223_372_036_854_000_001);
        assert_eq!(Timestamp::from_millis(edge.as_millis()), None);
        let after = Timestamp::from_nanos(-9_223_372_036_854_000_000);
        assert_eq!(Timestamp::from_millis(after.as_millis()), Some(after));
    }

    #[test]
    fn debug_shows_the_text() {
        assert_eq!(
            format!("{:?}", Timestamp::from_nanos(1)),
            "Timestamp(1970-01-01T00:00:00.000000001Z)"
        );
    }

    #[test]
    fn checked_arithmetic_stops_at_the_ends() {
        let second = Duration::from_secs(1);
        assert_eq!(
            Timestamp(5).checked_add(second),
            Some(Timestamp(1_000_000_005))
        );
        assert_eq!(
            Timestamp(5).checked_sub(second),
            Some(Timestamp(-999_999_995))
        );
        assert_eq!(Timestamp::MAX.checked_add(Duration::from_nanos(1)), None);
        assert_eq!(Timestamp::MIN.checked_sub(Duration::from_nanos(1)), None);
        assert_eq!(
            Timestamp::MAX.checked_add(Duration::ZERO),
            Some(Timestamp::MAX)
        );
        // A step of 2^63 ns: too big to add, exactly the distance from 0 to MIN.
        let big = Duration::from_nanos(1 << 63);
        assert_eq!(Timestamp::UNIX_EPOCH.checked_add(big), None);
        assert_eq!(Timestamp::UNIX_EPOCH.checked_sub(big), Some(Timestamp::MIN));
        assert_eq!(Timestamp::MAX.checked_sub(Duration::MAX), None);
        assert_eq!(Timestamp::MIN.checked_add(Duration::MAX), None);
        // Steps of 2^63 ns or more, from the far end.
        assert_eq!(Timestamp::MIN.checked_add(big), Some(Timestamp::UNIX_EPOCH));
        let whole_range = Duration::from_nanos(u64::MAX);
        assert_eq!(
            Timestamp::MIN.checked_add(whole_range),
            Some(Timestamp::MAX)
        );
        assert_eq!(
            Timestamp::MAX.checked_sub(whole_range),
            Some(Timestamp::MIN)
        );
        assert_eq!(
            Timestamp::from_nanos(-1).checked_add(big),
            Some(Timestamp::MAX)
        );
        assert_eq!(Timestamp(-10).checked_add(whole_range), None);
    }

    #[test]
    fn manual_clock_moves_only_when_told() {
        let clock = ManualClock::new(Timestamp(10));
        assert_eq!(clock.now(), Timestamp(10));
        clock.advance(Duration::from_nanos(5));
        assert_eq!(clock.now(), Timestamp(15));
        clock.set(Timestamp(-7));
        assert_eq!(clock.now(), Timestamp(-7));
        clock.set(Timestamp(i64::MAX - 1));
        clock.advance(Duration::MAX);
        assert_eq!(clock.now(), Timestamp::MAX);
        // Steps of 2^63 ns or more from below the epoch.
        clock.set(Timestamp::MIN);
        clock.advance(Duration::from_nanos(1 << 63));
        assert_eq!(clock.now(), Timestamp::UNIX_EPOCH);
        clock.set(Timestamp(-10));
        clock.advance(Duration::from_nanos((1 << 63) + 5));
        assert_eq!(clock.now(), Timestamp(i64::MAX - 4));
        clock.set(Timestamp::MIN);
        clock.advance(Duration::from_nanos(u64::MAX));
        assert_eq!(clock.now(), Timestamp::MAX);
        clock.set(Timestamp::MIN);
        clock.advance(Duration::MAX);
        assert_eq!(clock.now(), Timestamp::MAX);
        // Shared through a reference and an `Arc`.
        let shared = Arc::new(ManualClock::new(Timestamp(1)));
        let by_ref: &dyn Clock = &*shared;
        let boxed: Box<dyn Clock> = Box::new(Arc::clone(&shared));
        shared.advance(Duration::from_nanos(1));
        assert_eq!(by_ref.now(), Timestamp(2));
        assert_eq!(boxed.now(), Timestamp(2));
    }

    #[test]
    fn system_times_convert_and_saturate() {
        let epoch = SystemTime::UNIX_EPOCH;
        let ns = Duration::from_nanos;
        assert_eq!(from_system_time(epoch), Timestamp::UNIX_EPOCH);
        // Some platforms cannot hold every one of these times (Windows'
        // starts in 1601), so each is checked only where it exists.
        let cases = [
            (epoch.checked_add(ns(1)), Timestamp(1)),
            (epoch.checked_sub(ns(1)), Timestamp(-1)),
            (epoch.checked_add(ns(i64::MAX as u64)), Timestamp::MAX),
            (epoch.checked_add(ns(1 << 63)), Timestamp::MAX),
            (epoch.checked_sub(ns(i64::MAX as u64)), Timestamp(-i64::MAX)),
            (epoch.checked_sub(ns(1 << 63)), Timestamp::MIN),
            (epoch.checked_sub(ns((1 << 63) + 1)), Timestamp::MIN),
        ];
        for (time, expected) in cases {
            if let Some(time) = time {
                assert_eq!(from_system_time(time), expected);
            }
        }
    }

    #[test]
    fn system_clock_is_after_this_code_was_written() {
        // 2026-11-01T00:00:00Z.
        assert!(SystemClock.now() > Timestamp(1_793_491_200_000_000_000));
    }
}
