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

//! [`Id`], a 128-bit time-ordered identifier, and [`IdGenerator`], which
//! issues them in increasing order.
//!
//! An id is 48 bits of Unix milliseconds followed by 80 random bits, the
//! layout of a ULID. Its big-endian bytes, its numeric value and its text
//! all sort in the same order, which is creation order for ids from one
//! generator. The text form is 26 characters of Crockford base32.
//!
//! The randomness comes from an [`Entropy`] source: the operating system in
//! the facade, a seeded [`Pcg32`] in tests. Ids are ordered per generator;
//! the facade keeps one per open database.
//!
//! ```
//! use softshell_core::clock::{ManualClock, Timestamp};
//! use softshell_core::id::{Id, IdGenerator};
//! use softshell_core::rng::Pcg32;
//!
//! let clock = ManualClock::new(Timestamp::from_nanos(1_469_922_850_259_000_000));
//! let mut ids = IdGenerator::new(&clock, Pcg32::new(1, 1));
//! let first = ids.next_id()?;
//! let second = ids.next_id()?;
//! assert!(second > first);
//! assert_eq!(first.millis(), 1_469_922_850_259);
//! assert_eq!(first.to_string().parse::<Id>(), Ok(first));
//! # Ok::<(), softshell_core::id::EntropyError>(())
//! ```

#![deny(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use std::error::Error;
use std::fmt;
use std::str::FromStr;

use crate::clock::{Clock, Timestamp};
use crate::rng::Pcg32;

/// Crockford's base32 digits: no I, L, O or U.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
/// Characters in the text form: 26 × 5 = 130 bits, so the first character
/// carries only 3.
const TEXT_LEN: usize = 26;
const RANDOM_BITS: u32 = 80;
const RANDOM_MASK: u128 = (1 << RANDOM_BITS) - 1;
/// The largest millisecond an id can hold, in the year 10889.
const MAX_MILLIS: u64 = (1 << 48) - 1;

/// A 128-bit id: Unix milliseconds in the top 48 bits, random bits in the
/// low 80.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(u128);

impl Id {
    /// The id with this numeric value.
    #[must_use]
    pub const fn from_u128(value: u128) -> Self {
        Self(value)
    }

    /// The numeric value.
    #[must_use]
    pub const fn as_u128(self) -> u128 {
        self.0
    }

    /// The id with these big-endian bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(u128::from_be_bytes(bytes))
    }

    /// The id as big-endian bytes, which sort in id order.
    #[must_use]
    pub const fn to_bytes(self) -> [u8; 16] {
        self.0.to_be_bytes()
    }

    /// The Unix milliseconds in the top 48 bits.
    #[must_use]
    pub const fn millis(self) -> u64 {
        // The shift leaves 48 bits.
        (self.0 >> RANDOM_BITS) as u64
    }

    /// The milliseconds as a [`Timestamp`], or `None` when they are past
    /// [`Timestamp::MAX`]'s: an id made from other bits, or, in theory, one
    /// a generator carried past the last millisecond of
    /// 2262-04-11T23:47:16.854Z.
    #[must_use]
    pub const fn timestamp(self) -> Option<Timestamp> {
        // At most 2^48, so the conversion is exact.
        Timestamp::from_millis(self.millis() as i64)
    }

    /// The 80 random bits.
    #[must_use]
    pub const fn random_bits(self) -> u128 {
        self.0 & RANDOM_MASK
    }
}

/// The 26-character text form, in upper case.
impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut text = [0u8; TEXT_LEN];
        let mut rest = self.0;
        for slot in text.iter_mut().rev() {
            // The mask keeps 5 bits, a valid index.
            *slot = ALPHABET.get((rest & 31) as usize).copied().unwrap_or(b'0');
            rest >>= 5;
        }
        // Every byte comes from `ALPHABET`, which is ASCII.
        f.write_str(std::str::from_utf8(&text).unwrap_or_default())
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Id({self})")
    }
}

/// Reads the 26-character text form. Letters may be in either case; I, L,
/// O and U are not Crockford digits and are rejected, as is a first
/// character above 7, which would need more than 128 bits.
impl FromStr for Id {
    type Err = ParseIdError;

    fn from_str(text: &str) -> Result<Self, ParseIdError> {
        let bytes = text.as_bytes();
        if bytes.len() != TEXT_LEN {
            return Err(ParseIdError::Length { found: bytes.len() });
        }
        let mut value = 0u128;
        for (offset, &b) in bytes.iter().enumerate() {
            let digit = ALPHABET
                .iter()
                .position(|&a| a == b.to_ascii_uppercase())
                .ok_or(ParseIdError::InvalidChar { offset })?;
            if offset == 0 && digit > 7 {
                return Err(ParseIdError::Overflow);
            }
            // 26 digits of 5 bits with a first digit below 8: 128 bits.
            value = (value << 5) | digit as u128;
        }
        Ok(Self(value))
    }
}

/// Why text is not an [`Id`]. The text itself is not included.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParseIdError {
    /// The text is not 26 bytes long.
    Length {
        /// Its length in bytes.
        found: usize,
    },
    /// A byte is not a Crockford base32 digit.
    InvalidChar {
        /// Its offset.
        offset: usize,
    },
    /// The first character is above 7, so the value needs more than 128
    /// bits.
    Overflow,
}

impl fmt::Display for ParseIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Length { found } => {
                write!(f, "an id is 26 characters, not {found} bytes")
            }
            Self::InvalidChar { offset } => {
                write!(f, "byte {offset} of an id is not a Crockford base32 digit")
            }
            Self::Overflow => f.write_str("an id's first character must be 0 to 7"),
        }
    }
}

/// A leaf error: it has no source.
impl Error for ParseIdError {}

/// A source of random bytes for ids.
pub trait Entropy: Send {
    /// Fills `buf` with random bytes.
    ///
    /// # Errors
    ///
    /// When the source fails, such as an operating-system call.
    fn fill(&mut self, buf: &mut [u8]) -> Result<(), EntropyError>;
}

impl<E: Entropy + ?Sized> Entropy for &mut E {
    fn fill(&mut self, buf: &mut [u8]) -> Result<(), EntropyError> {
        (**self).fill(buf)
    }
}

impl<E: Entropy + ?Sized> Entropy for Box<E> {
    fn fill(&mut self, buf: &mut [u8]) -> Result<(), EntropyError> {
        (**self).fill(buf)
    }
}

/// A seeded, predictable source, for tests and reproducible data. Never
/// for ids that must be hard to guess. It never fails.
impl Entropy for Pcg32 {
    fn fill(&mut self, buf: &mut [u8]) -> Result<(), EntropyError> {
        for chunk in buf.chunks_mut(4) {
            let word = self.next_u32().to_le_bytes();
            for (dst, src) in chunk.iter_mut().zip(word) {
                *dst = src;
            }
        }
        Ok(())
    }
}

/// An [`Entropy`] source failed. `source()` gives the cause, such as the
/// operating system's error.
#[derive(Debug)]
pub struct EntropyError {
    cause: Box<dyn Error + Send + Sync>,
}

impl EntropyError {
    /// Wraps the source's own error.
    pub fn new(cause: impl Into<Box<dyn Error + Send + Sync>>) -> Self {
        Self {
            cause: cause.into(),
        }
    }
}

impl fmt::Display for EntropyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the random source failed")
    }
}

impl Error for EntropyError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&*self.cause)
    }
}

/// Issues ids in strictly increasing order, whatever the clock does.
///
/// A new millisecond gets fresh random bits. Within one millisecond, or if
/// the clock has stepped back, the generator keeps the last id's
/// millisecond and adds one to it; when the random bits are all ones, the
/// carry moves the millisecond forward instead. A clock before 1970 counts
/// as millisecond 0.
///
/// Order holds per generator, so a process issues ids from one generator
/// (shared behind a lock) per database.
#[derive(Debug)]
pub struct IdGenerator<C, E> {
    clock: C,
    entropy: E,
    last: Option<Id>,
}

impl<C: Clock, E: Entropy> IdGenerator<C, E> {
    /// A generator reading `clock` and drawing random bits from `entropy`.
    #[must_use]
    pub const fn new(clock: C, entropy: E) -> Self {
        Self {
            clock,
            entropy,
            last: None,
        }
    }

    /// The next id, greater than every id this generator issued before.
    ///
    /// # Errors
    ///
    /// When `entropy` fails, which only a new millisecond asks it for. The
    /// generator is then unchanged.
    pub fn next_id(&mut self) -> Result<Id, EntropyError> {
        // At least 0, and at most 2262's milliseconds, well below 2^48.
        let millis = self.clock.now().as_millis().clamp(0, MAX_MILLIS as i64) as u64;
        let id = match self.last {
            // The last id is far below `u128::MAX`: its milliseconds are at
            // most one past 2262's last, well under 2^48, so adding one
            // cannot overflow.
            Some(last) if millis <= last.millis() => Id(last.0.saturating_add(1)),
            _ => {
                let mut random = [0u8; RANDOM_BITS as usize / 8];
                self.entropy.fill(&mut random)?;
                let random = random
                    .iter()
                    .fold(0, |bits, &b| (bits << 8) | u128::from(b));
                Id((u128::from(millis) << RANDOM_BITS) | random)
            }
        };
        self.last = Some(id);
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "test code indexes and computes with known-good values"
    )]

    use std::time::Duration;

    use super::*;
    use crate::clock::ManualClock;
    use crate::hash::fnv1a64;

    /// Entropy that repeats one byte, or fails, and counts the bytes it
    /// was asked for.
    struct Fixed {
        byte: Option<u8>,
        requested: usize,
    }

    impl Fixed {
        fn new(byte: Option<u8>) -> Self {
            Self { byte, requested: 0 }
        }
    }

    impl Entropy for Fixed {
        fn fill(&mut self, buf: &mut [u8]) -> Result<(), EntropyError> {
            self.requested += buf.len();
            let byte = self.byte.ok_or_else(|| EntropyError::new("no entropy"))?;
            buf.fill(byte);
            Ok(())
        }
    }

    fn at_millis(millis: i64) -> Timestamp {
        Timestamp::from_millis(millis).unwrap()
    }

    #[test]
    fn known_text_forms() {
        // Computed with an independent Python encoder; the first is the
        // example in the ULID specification.
        let known = [
            (
                0x0156_3E3A_B5D3_D676_4C61_EFB9_9302_BD5B,
                "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            ),
            (0, "00000000000000000000000000"),
            (1, "00000000000000000000000001"),
            (u128::MAX, "7ZZZZZZZZZZZZZZZZZZZZZZZZZ"),
            (1 << 80, "00000000010000000000000000"),
            ((1 << 80) - 1, "0000000000ZZZZZZZZZZZZZZZZ"),
            (
                0x0123_4567_89AB_CDEF_0123_4567_89AB_CDEF,
                "014D2PF2DBSQQG28T5CY4TQKFF",
            ),
        ];
        for (value, text) in known {
            let id = Id::from_u128(value);
            assert_eq!(id.to_string(), text);
            assert_eq!(text.parse::<Id>(), Ok(id));
            assert_eq!(text.to_ascii_lowercase().parse::<Id>(), Ok(id));
        }
        let example = Id::from_u128(known[0].0);
        assert_eq!(example.millis(), 1_469_922_850_259);
        assert_eq!(example.random_bits(), 0xD676_4C61_EFB9_9302_BD5B);
        assert_eq!(example.timestamp(), Some(at_millis(1_469_922_850_259)));
        assert_eq!(format!("{example:?}"), "Id(01ARZ3NDEKTSV4RRFFQ69G5FAV)");
        assert_eq!(Id::from_u128(u128::MAX).timestamp(), None);
    }

    #[test]
    fn text_round_trips_and_sorts_like_the_value() {
        // A 128-bit LCG; the aggregate was computed with the Python encoder.
        let mut x: u128 = 0x9E37_79B9_7F4A_7C15_F39C_C060_5CED_C834;
        let mut texts = Vec::new();
        let mut ids = Vec::new();
        for _ in 0..1000 {
            x = x
                .wrapping_mul(0x2360_ED05_1FC6_5DA4_4385_DF64_9FCC_F645)
                .wrapping_add(0x5851_F42D_4C95_7F2D_1405_7B7E_F767_814F);
            let id = Id::from_u128(x);
            let text = id.to_string();
            assert_eq!(text.parse::<Id>(), Ok(id));
            assert_eq!(Id::from_bytes(id.to_bytes()), id);
            texts.push(text);
            ids.push(id);
        }
        assert_eq!(texts[0], "7E1FE2B81B8HNS3VSBX1BN0VTK");
        assert_eq!(texts[999], "2X2ZKB6NAWA4F58TX9VTB24KMW");
        assert_eq!(fnv1a64(texts.join("\n").as_bytes()), 0x8DCC_1D4F_3478_10AA);
        // Text order, byte order and numeric order agree.
        let mut by_text: Vec<_> = ids.iter().zip(&texts).collect();
        by_text.sort_by(|a, b| a.1.cmp(b.1));
        let mut by_value = ids.clone();
        by_value.sort();
        assert!(
            by_text
                .iter()
                .map(|(id, _)| **id)
                .eq(by_value.iter().copied())
        );
        let mut by_bytes = ids.clone();
        by_bytes.sort_by_key(|id| id.to_bytes());
        assert_eq!(by_bytes, by_value);
    }

    #[test]
    fn parse_errors() {
        assert_eq!("".parse::<Id>(), Err(ParseIdError::Length { found: 0 }));
        assert_eq!(
            "01ARZ3NDEKTSV4RRFFQ69G5FA".parse::<Id>(),
            Err(ParseIdError::Length { found: 25 })
        );
        assert_eq!(
            "01ARZ3NDEKTSV4RRFFQ69G5FAVV".parse::<Id>(),
            Err(ParseIdError::Length { found: 27 })
        );
        for (offset, bad) in [(0, 'I'), (5, 'L'), (12, 'o'), (25, 'U'), (3, '-'), (7, ' ')] {
            let mut text: Vec<char> = "01ARZ3NDEKTSV4RRFFQ69G5FAV".chars().collect();
            text[offset] = bad;
            let text: String = text.into_iter().collect();
            assert_eq!(
                text.parse::<Id>(),
                Err(ParseIdError::InvalidChar { offset }),
                "{text}"
            );
        }
        // 26 bytes, but one character is two bytes long.
        assert_eq!(
            "01ARZ3NDEKTSV4RRFFQ69G5Fé".parse::<Id>(),
            Err(ParseIdError::InvalidChar { offset: 24 })
        );
        assert_eq!(
            "80000000000000000000000000".parse::<Id>(),
            Err(ParseIdError::Overflow)
        );
        assert_eq!(
            "ZZZZZZZZZZZZZZZZZZZZZZZZZZ".parse::<Id>(),
            Err(ParseIdError::Overflow)
        );
        assert_eq!(
            ParseIdError::InvalidChar { offset: 3 }.to_string(),
            "byte 3 of an id is not a Crockford base32 digit"
        );
    }

    #[test]
    fn monotonic_when_the_clock_steps_back() {
        let clock = ManualClock::new(at_millis(1_000));
        let mut ids = IdGenerator::new(&clock, Pcg32::new(7, 7));
        let a = ids.next_id().unwrap();
        assert_eq!(a.millis(), 1_000);
        // The clock steps back: the larger millisecond stays, the random
        // part goes up by one.
        clock.set(at_millis(900));
        let b = ids.next_id().unwrap();
        assert_eq!(b, Id::from_u128(a.as_u128() + 1));
        let c = ids.next_id().unwrap();
        assert_eq!(c, Id::from_u128(b.as_u128() + 1));
        // Back to the same millisecond: still incrementing.
        clock.set(at_millis(1_000));
        let d = ids.next_id().unwrap();
        assert_eq!(d, Id::from_u128(c.as_u128() + 1));
        // A new millisecond draws fresh random bits.
        clock.advance(Duration::from_millis(1));
        let e = ids.next_id().unwrap();
        assert_eq!(e.millis(), 1_001);
        assert!(e > d);
        // Far back, before 1970: still increasing.
        clock.set(Timestamp::MIN);
        let f = ids.next_id().unwrap();
        assert_eq!(f, Id::from_u128(e.as_u128() + 1));
    }

    #[test]
    fn strictly_increasing_under_a_wandering_clock() {
        let clock = ManualClock::new(at_millis(5_000));
        let mut rng = Pcg32::new(11, 12);
        let mut ids = IdGenerator::new(&clock, Pcg32::new(13, 14));
        let mut last = ids.next_id().unwrap();
        let mut high_water = 5_000;
        for _ in 0..100_000 {
            // Small steps either way.
            let step = rng.range(0..7) as i64 - 3;
            let now = clock.now().as_millis() + step;
            clock.set(at_millis(now));
            let id = ids.next_id().unwrap();
            assert!(id > last);
            high_water = high_water.max(now);
            assert_eq!(id.millis(), high_water as u64);
            last = id;
        }
    }

    #[test]
    fn entropy_is_drawn_only_for_a_new_millisecond() {
        let clock = ManualClock::new(at_millis(10));
        let mut entropy = Fixed::new(Some(0x5A));
        {
            let mut ids = IdGenerator::new(&clock, &mut entropy);
            let first = ids.next_id().unwrap();
            assert_eq!(first.random_bits(), 0x5A5A_5A5A_5A5A_5A5A_5A5A);
            ids.next_id().unwrap(); // the same millisecond
            clock.set(at_millis(9));
            ids.next_id().unwrap(); // the clock stepped back
            clock.set(at_millis(11));
            ids.next_id().unwrap(); // a new millisecond
        }
        assert_eq!(entropy.requested, 20);
    }

    #[test]
    fn a_clock_before_1970_counts_as_millisecond_zero() {
        let clock = ManualClock::new(Timestamp::from_nanos(-1));
        let mut ids = IdGenerator::new(&clock, Fixed::new(Some(0)));
        let a = ids.next_id().unwrap();
        assert_eq!(a, Id::from_u128(0));
        clock.set(Timestamp::MIN);
        assert_eq!(ids.next_id().unwrap(), Id::from_u128(1));
        clock.set(Timestamp::UNIX_EPOCH);
        assert_eq!(ids.next_id().unwrap(), Id::from_u128(2));
    }

    #[test]
    fn a_full_random_part_carries_into_the_millisecond() {
        let clock = ManualClock::new(at_millis(42));
        let mut ids = IdGenerator::new(&clock, Fixed::new(Some(0xFF)));
        let first = ids.next_id().unwrap();
        assert_eq!((first.millis(), first.random_bits()), (42, RANDOM_MASK));
        let second = ids.next_id().unwrap();
        assert_eq!((second.millis(), second.random_bits()), (43, 0));
        assert!(second > first);
        // At 43 now, the clock still at 42: increment, no new random bits.
        let third = ids.next_id().unwrap();
        assert_eq!((third.millis(), third.random_bits()), (43, 1));
    }

    #[test]
    fn an_entropy_failure_leaves_the_generator_unchanged() {
        let clock = ManualClock::new(at_millis(1));
        let mut ids = IdGenerator::new(&clock, Fixed::new(None));
        let err = ids.next_id().unwrap_err();
        assert_eq!(err.to_string(), "the random source failed");
        assert_eq!(
            err.source().map(ToString::to_string).as_deref(),
            Some("no entropy")
        );
        assert_eq!(ids.last, None);
        ids.entropy.byte = Some(0);
        let a = ids.next_id().unwrap();
        assert_eq!(a, Id::from_u128(1 << 80));
        // The same millisecond needs no entropy.
        ids.entropy.byte = None;
        assert_eq!(ids.next_id().unwrap(), Id::from_u128((1 << 80) + 1));
        clock.advance(Duration::from_millis(1));
        assert!(ids.next_id().is_err());
        assert_eq!(ids.last, Some(Id::from_u128((1 << 80) + 1)));
        // The next success is still greater.
        ids.entropy.byte = Some(0);
        assert_eq!(ids.next_id().unwrap(), Id::from_u128(2 << 80));
    }

    #[test]
    fn generators_can_be_boxed() {
        let clock: std::sync::Arc<dyn Clock> = std::sync::Arc::new(ManualClock::new(at_millis(3)));
        let entropy: Box<dyn Entropy> = Box::new(Pcg32::new(1, 2));
        let mut ids = IdGenerator::new(clock, entropy);
        let boxed = ids.next_id().unwrap();
        assert_eq!(boxed.millis(), 3);
        // The box passes the bytes through.
        let bare = IdGenerator::new(ManualClock::new(at_millis(3)), Pcg32::new(1, 2))
            .next_id()
            .unwrap();
        assert_eq!(boxed, bare);
    }

    #[test]
    fn pcg32_fills_any_length() {
        let mut a = Pcg32::new(42, 54);
        let mut buf = [0u8; 10];
        a.fill(&mut buf).unwrap();
        // The pcg32-demo words A15C02B7 7B47F409 BA1D3330, little-endian.
        assert_eq!(
            buf,
            [0xB7, 0x02, 0x5C, 0xA1, 0x09, 0xF4, 0x47, 0x7B, 0x30, 0x33]
        );
    }
}
