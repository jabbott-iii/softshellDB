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

//! UTC civil time: [`DateTime`] converts a [`Timestamp`] to and from a
//! proleptic Gregorian date and time of day, and RFC 3339 text is written
//! by [`format_rfc3339`] and read by [`parse_rfc3339`].
//!
//! Output is always UTC with nine fraction digits, so every timestamp has
//! one text form, 30 characters long, and text order is time order. Input
//! may have 0 to 9 fraction digits and any UTC offset. A time outside the
//! [`Timestamp`] range is an error, never clamped (ADR-0013).
//!
//! ```
//! use softshell_core::civil::{format_rfc3339, parse_rfc3339};
//!
//! let ts = parse_rfc3339("2026-11-01T13:00:00.5+01:00").unwrap();
//! assert_eq!(format_rfc3339(ts), "2026-11-01T12:00:00.500000000Z");
//! assert!(parse_rfc3339("2262-04-11T23:47:16.854775808Z").is_err());
//! ```

#![deny(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use std::error::Error;
use std::fmt;

use crate::clock::Timestamp;

const NANOS_PER_SECOND: i64 = 1_000_000_000;
const SECONDS_PER_DAY: i64 = 86_400;
/// The years a [`Timestamp`] reaches, in part at either end.
const YEARS: std::ops::RangeInclusive<i32> = 1677..=2262;

/// A UTC date and time of day in the proleptic Gregorian calendar, with
/// no leap seconds.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct DateTime {
    /// The year, such as 2026.
    pub year: i32,
    /// 1 to 12.
    pub month: u8,
    /// 1 to the month's length.
    pub day: u8,
    /// 0 to 23.
    pub hour: u8,
    /// 0 to 59.
    pub minute: u8,
    /// 0 to 59.
    pub second: u8,
    /// 0 to 999,999,999.
    pub nanosecond: u32,
}

impl DateTime {
    /// The date and time of `ts`, in UTC.
    #[must_use]
    pub fn from_timestamp(ts: Timestamp) -> Self {
        let nanos = ts.as_nanos();
        let seconds = nanos.div_euclid(NANOS_PER_SECOND);
        let days = seconds.div_euclid(SECONDS_PER_DAY);
        // Each remainder is below its divisor, so the narrowing is exact.
        let of_day = seconds.rem_euclid(SECONDS_PER_DAY) as u32;
        let (year, month, day) = civil_from_days(days);
        Self {
            year,
            month,
            day,
            hour: (of_day / 3600) as u8,
            minute: (of_day / 60 % 60) as u8,
            second: (of_day % 60) as u8,
            nanosecond: nanos.rem_euclid(NANOS_PER_SECOND) as u32,
        }
    }

    /// The timestamp of this date and time.
    ///
    /// # Errors
    ///
    /// [`CivilError::FieldOutOfRange`] for a field outside its range (a
    /// 13th month, April 31st, February 29th in a common year, second 60);
    /// [`CivilError::OutOfRange`] for a valid date and time outside the
    /// [`Timestamp`] range.
    pub fn to_timestamp(&self) -> Result<Timestamp, CivilError> {
        check_field("month", u32::from(self.month), 1..=12)?;
        let length = month_length(self.year, self.month);
        check_field("day", u32::from(self.day), 1..=u32::from(length))?;
        check_field("hour", u32::from(self.hour), 0..=23)?;
        check_field("minute", u32::from(self.minute), 0..=59)?;
        check_field("second", u32::from(self.second), 0..=59)?;
        check_field("nanosecond", self.nanosecond, 0..=999_999_999)?;
        if !YEARS.contains(&self.year) {
            return Err(CivilError::OutOfRange);
        }
        let days = days_from_civil(self.year, self.month, self.day);
        let time = [self.hour, self.minute, self.second].map(u32::from);
        timestamp_from_parts(days, time, self.nanosecond, 0)
    }
}

/// Writes `ts` as `YYYY-MM-DDTHH:MM:SS.nnnnnnnnnZ`.
#[must_use]
pub fn format_rfc3339(ts: Timestamp) -> String {
    ts.to_string()
}

/// Writes `ts` with milliseconds, truncated: `YYYY-MM-DDTHH:MM:SS.mmmZ`.
/// For log lines; [`format_rfc3339`] is the exact form.
#[must_use]
pub fn format_rfc3339_millis(ts: Timestamp) -> String {
    let mut text = String::with_capacity(24);
    // Writing to a `String` cannot fail.
    let _ = write_rfc3339_millis(ts, &mut text);
    text
}

/// What [`format_rfc3339_millis`] returns, written to `out`.
pub(crate) fn write_rfc3339_millis(ts: Timestamp, out: &mut impl fmt::Write) -> fmt::Result {
    let t = DateTime::from_timestamp(ts);
    write!(
        out,
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        t.year,
        t.month,
        t.day,
        t.hour,
        t.minute,
        t.second,
        t.nanosecond / 1_000_000
    )
}

/// What `Timestamp`'s `Display` writes.
pub(crate) fn write_rfc3339(ts: Timestamp, out: &mut impl fmt::Write) -> fmt::Result {
    let t = DateTime::from_timestamp(ts);
    write!(
        out,
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:09}Z",
        t.year, t.month, t.day, t.hour, t.minute, t.second, t.nanosecond
    )
}

/// Reads an RFC 3339 date-time (RFC 3339 §5.6):
/// `YYYY-MM-DDTHH:MM:SS[.fraction](Z|+HH:MM|-HH:MM)`.
///
/// - `T` and `Z` may be lower case. A space in place of `T` is not
///   accepted.
/// - The fraction has 1 to 9 digits.
/// - The offset is local time minus UTC, so UTC is the local time minus
///   the offset: `13:00+01:00` is `12:00Z`. `-00:00` is UTC.
/// - The range is checked once, on the UTC instant.
///
/// # Errors
///
/// - [`CivilError::Syntax`] for text that does not match the grammar,
///   including anything after the offset.
/// - [`CivilError::FieldOutOfRange`] for an impossible date or time.
/// - [`CivilError::LeapSecond`] for second 60, which a [`Timestamp`] cannot
///   represent.
/// - [`CivilError::TooPrecise`] for more than nine fraction digits.
/// - [`CivilError::OutOfRange`] for a time outside the [`Timestamp`]
///   range, even by one nanosecond.
pub fn parse_rfc3339(text: &str) -> Result<Timestamp, CivilError> {
    let mut p = Parser {
        rest: text.as_bytes(),
        len: text.len(),
    };
    let year = p.digits(4, "a 4-digit year")?;
    p.byte(b"-", "`-`")?;
    let month = p.digits(2, "a 2-digit month")?;
    p.byte(b"-", "`-`")?;
    let day = p.digits(2, "a 2-digit day")?;
    p.byte(b"Tt", "`T`")?;
    let hour = p.digits(2, "a 2-digit hour")?;
    p.byte(b":", "`:`")?;
    let minute = p.digits(2, "a 2-digit minute")?;
    p.byte(b":", "`:`")?;
    let second = p.digits(2, "a 2-digit second")?;
    let nanosecond = if p.peek() == Some(b'.') {
        p.byte(b".", "`.`")?;
        p.fraction()?
    } else {
        0
    };
    let offset_minutes = match p.peek() {
        Some(b'Z' | b'z') => {
            p.byte(b"Zz", "`Z`")?;
            0
        }
        Some(sign @ (b'+' | b'-')) => {
            p.byte(b"+-", "`+` or `-`")?;
            let hours = p.digits(2, "a 2-digit offset hour")?;
            p.byte(b":", "`:`")?;
            let minutes = p.digits(2, "a 2-digit offset minute")?;
            check_field("offset hour", hours, 0..=23)?;
            check_field("offset minute", minutes, 0..=59)?;
            // At most 23 * 60 + 59, so neither step can overflow.
            let total = i64::from(hours.wrapping_mul(60).wrapping_add(minutes));
            if sign == b'-' {
                total.wrapping_neg()
            } else {
                total
            }
        }
        _ => return Err(p.expected("`Z`, `+` or `-`")),
    };
    if !p.rest.is_empty() {
        return Err(p.expected("the end of the timestamp"));
    }

    // The parsed fields fit their types: at most 4 digits, or 2.
    let year = year as i32;
    let month = month as u8;
    let day = day as u8;
    check_field("month", u32::from(month), 1..=12)?;
    check_field(
        "day",
        u32::from(day),
        1..=u32::from(month_length(year, month)),
    )?;
    check_field("hour", hour, 0..=23)?;
    check_field("minute", minute, 0..=59)?;
    if second == 60 {
        return Err(CivilError::LeapSecond);
    }
    check_field("second", second, 0..=59)?;
    // The range check comes last, after the offset.
    let days = days_from_civil(year, month, day);
    timestamp_from_parts(days, [hour, minute, second], nanosecond, offset_minutes)
}

/// Why civil-time text or fields were rejected. The input text is never
/// included, so a message is safe to log.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CivilError {
    /// The text does not follow the RFC 3339 grammar.
    Syntax {
        /// The byte offset where the text went wrong.
        offset: usize,
        /// What was expected there, such as "`-`" or "a 2-digit hour".
        expected: &'static str,
    },
    /// A field is outside its range for this date.
    FieldOutOfRange {
        /// The field: "month", "day", "hour", "minute", "second",
        /// "nanosecond", "offset hour" or "offset minute".
        field: &'static str,
        /// The value given.
        value: u32,
    },
    /// Second 60, a leap second, which a [`Timestamp`] cannot represent.
    LeapSecond,
    /// More than nine fraction digits: finer than a nanosecond.
    TooPrecise,
    /// A valid time outside the [`Timestamp`] range (ADR-0013).
    OutOfRange,
}

impl fmt::Display for CivilError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Syntax { offset, expected } => write!(
                f,
                "malformed RFC 3339 timestamp: expected {expected} at byte {offset}"
            ),
            Self::FieldOutOfRange { field, value } => {
                write!(f, "{field} {value} is out of range")
            }
            Self::LeapSecond => f.write_str("leap seconds (second 60) are not supported"),
            Self::TooPrecise => f.write_str("more than 9 fraction digits: finer than a nanosecond"),
            Self::OutOfRange => write!(
                f,
                "time is outside the timestamp range, {} to {}",
                Timestamp::MIN,
                Timestamp::MAX
            ),
        }
    }
}

/// A leaf error: it has no source.
impl Error for CivilError {}

fn check_field(
    field: &'static str,
    value: u32,
    range: std::ops::RangeInclusive<u32>,
) -> Result<(), CivilError> {
    if range.contains(&value) {
        Ok(())
    } else {
        Err(CivilError::FieldOutOfRange { field, value })
    }
}

/// A cursor over the text. `rest` is always a suffix of the input, so the
/// offset of the next byte is `len - rest.len()`.
struct Parser<'a> {
    rest: &'a [u8],
    len: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.rest.first().copied()
    }

    fn expected(&self, expected: &'static str) -> CivilError {
        CivilError::Syntax {
            offset: self.len.saturating_sub(self.rest.len()),
            expected,
        }
    }

    /// Consumes one byte, which must be one of `allowed`.
    fn byte(&mut self, allowed: &[u8], expected: &'static str) -> Result<(), CivilError> {
        match self.rest.split_first() {
            Some((b, rest)) if allowed.contains(b) => {
                self.rest = rest;
                Ok(())
            }
            _ => Err(self.expected(expected)),
        }
    }

    /// Consumes exactly `count` ASCII digits (at most 4) and returns their
    /// value. An error names the field's first byte.
    fn digits(&mut self, count: usize, expected: &'static str) -> Result<u32, CivilError> {
        let start = self.rest;
        let mut value = 0u32;
        for _ in 0..count {
            match self.rest.split_first() {
                Some((b @ b'0'..=b'9', rest)) => {
                    // At most 4 digits, so this cannot wrap.
                    value = value
                        .wrapping_mul(10)
                        .wrapping_add(u32::from(b.wrapping_sub(b'0')));
                    self.rest = rest;
                }
                _ => {
                    self.rest = start;
                    return Err(self.expected(expected));
                }
            }
        }
        Ok(value)
    }

    /// Consumes 1 to 9 fraction digits and returns them as nanoseconds.
    fn fraction(&mut self) -> Result<u32, CivilError> {
        let mut value = 0u32;
        let mut scale = NANOS_PER_SECOND as u32;
        while let Some((b @ b'0'..=b'9', rest)) = self.rest.split_first() {
            if scale == 1 {
                return Err(CivilError::TooPrecise);
            }
            scale /= 10;
            // The digits times their scales stay below 10^9 in total.
            value = value.wrapping_add(u32::from(b.wrapping_sub(b'0')).wrapping_mul(scale));
            self.rest = rest;
        }
        if scale == NANOS_PER_SECOND as u32 {
            return Err(self.expected("a fraction digit"));
        }
        Ok(value)
    }
}

/// Assembles a timestamp from a day number, the hour, minute and second,
/// nanoseconds and an offset east of UTC in minutes, or fails with
/// [`CivilError::OutOfRange`].
fn timestamp_from_parts(
    days: i64,
    [hour, minute, second]: [u32; 3],
    nanosecond: u32,
    offset_minutes: i64,
) -> Result<Timestamp, CivilError> {
    let minutes = i128::from(days)
        .checked_mul(24 * 60)
        .and_then(|m| m.checked_add(i128::from(hour).checked_mul(60)?))
        .and_then(|m| m.checked_add(i128::from(minute)))
        .and_then(|m| m.checked_sub(i128::from(offset_minutes)));
    let nanos = minutes
        .and_then(|m| m.checked_mul(60))
        .and_then(|s| s.checked_add(i128::from(second)))
        .and_then(|s| s.checked_mul(i128::from(NANOS_PER_SECOND)))
        .and_then(|n| n.checked_add(i128::from(nanosecond)));
    nanos
        .and_then(|n| i64::try_from(n).ok())
        .map(Timestamp::from_nanos)
        .ok_or(CivilError::OutOfRange)
}

const fn is_leap(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// The length of `month` (1 to 12) in `year`; 0 for any other month.
const fn month_length(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

// The two conversions below are Howard Hinnant's `days_from_civil` and
// `civil_from_days` ("chrono-Compatible Low-Level Date Algorithms"), with
// days counted from 1970-01-01. They work in 400-year eras of 146,097
// days, with years starting on March 1st so the leap day comes last.

/// The day number of a valid date with a year in 0..=9999.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "callers pass a validated date with a year in 0..=9999, so every intermediate value is below 4 million"
)]
fn days_from_civil(year: i32, month: u8, day: u8) -> i64 {
    let year = i64::from(year) - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let month = i64::from(month);
    let shifted_month = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The date of a day number in the years 0 to 9999, which holds every
/// [`Timestamp`]'s day.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "callers pass a day in the years 0 to 9999, so every intermediate value is below 4 million"
)]
fn civil_from_days(days: i64) -> (i32, u8, u8) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    // The year is 0 to 9999, the month 1 to 12 and the day 1 to 31.
    (year as i32, month as u8, day as u8)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "test code indexes and computes with known-good values"
    )]

    use super::*;
    use crate::hash::fnv1a64;

    fn ts(nanos: i64) -> Timestamp {
        Timestamp::from_nanos(nanos)
    }

    /// Timestamps and their text, computed with Python's `datetime`.
    const KNOWN: [(i64, &str); 16] = [
        (0, "1970-01-01T00:00:00.000000000Z"),
        (-1, "1969-12-31T23:59:59.999999999Z"),
        (1, "1970-01-01T00:00:00.000000001Z"),
        (i64::MIN, "1677-09-21T00:12:43.145224192Z"),
        (i64::MAX, "2262-04-11T23:47:16.854775807Z"),
        (951_782_400_000_000_000, "2000-02-29T00:00:00.000000000Z"),
        (951_868_799_999_999_999, "2000-02-29T23:59:59.999999999Z"),
        (2_147_483_647_000_000_000, "2038-01-19T03:14:07.000000000Z"),
        (2_147_483_648_000_000_000, "2038-01-19T03:14:08.000000000Z"),
        (-2_208_988_800_000_000_000, "1900-01-01T00:00:00.000000000Z"),
        (-5_364_662_400_000_000_001, "1799-12-31T23:59:59.999999999Z"),
        (1_709_164_800_123_456_789, "2024-02-29T00:00:00.123456789Z"),
        (1_704_067_199_000_000_000, "2023-12-31T23:59:59.000000000Z"),
        (-86_400_000_000_000, "1969-12-31T00:00:00.000000000Z"),
        (-86_400_000_000_001, "1969-12-30T23:59:59.999999999Z"),
        (4_107_542_400_000_000_000, "2100-03-01T00:00:00.000000000Z"),
    ];

    #[test]
    fn known_timestamps_format_and_parse() {
        for (nanos, text) in KNOWN {
            assert_eq!(format_rfc3339(ts(nanos)), text, "{nanos}");
            assert_eq!(parse_rfc3339(text), Ok(ts(nanos)), "{text}");
            let t = DateTime::from_timestamp(ts(nanos));
            assert_eq!(t.to_timestamp(), Ok(ts(nanos)), "{t:?}");
        }
        assert_eq!(format_rfc3339_millis(ts(-1)), "1969-12-31T23:59:59.999Z");
        assert_eq!(
            format_rfc3339_millis(ts(1_709_164_800_123_456_789)),
            "2024-02-29T00:00:00.123Z"
        );
        assert_eq!(
            format_rfc3339_millis(Timestamp::MAX),
            "2262-04-11T23:47:16.854Z"
        );
    }

    /// One timestamp on every day of the range, at a time of day that
    /// varies, against an aggregate computed with Python's `datetime`.
    #[test]
    fn every_day_in_the_range_matches_the_reference() {
        const DAY: i128 = 86_400 * 1_000_000_000;
        let (first, last) = (
            i128::from(i64::MIN).div_euclid(DAY),
            i128::from(i64::MAX).div_euclid(DAY),
        );
        let mut lines = Vec::new();
        for d in first..=last {
            let nanos = d * DAY + (d * 1_000_003_141).rem_euclid(DAY);
            let Ok(nanos) = i64::try_from(nanos) else {
                continue;
            };
            let text = format_rfc3339(ts(nanos));
            assert_eq!(parse_rfc3339(&text), Ok(ts(nanos)), "{text}");
            lines.push(text);
        }
        assert_eq!(lines.len(), 213_504);
        assert_eq!(lines[0], "1677-09-21T18:20:47.664691968Z");
        assert_eq!(lines[lines.len() - 1], "2262-04-11T05:39:11.335304891Z");
        assert_eq!(fnv1a64(lines.join("\n").as_bytes()), 0xEF3F_4205_99B2_137C);
    }

    #[test]
    fn days_round_trip_across_whole_eras() {
        // Year 0 to 9999, every day: the parser's full domain.
        let mut expected = days_from_civil(0, 1, 1);
        for year in 0..=9999 {
            for month in 1..=12 {
                for day in 1..=month_length(year, month) {
                    let days = days_from_civil(year, month, day);
                    assert_eq!(days, expected, "{year}-{month}-{day}");
                    assert_eq!(civil_from_days(days), (year, month, day));
                    expected += 1;
                }
            }
        }
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
    }

    #[test]
    fn leap_years() {
        assert!(is_leap(2000) && is_leap(2024) && is_leap(1600));
        assert!(!is_leap(1900) && !is_leap(2100) && !is_leap(2023));
        assert_eq!(
            parse_rfc3339("2024-02-29T00:00:00Z"),
            Ok(ts(1_709_164_800_000_000_000))
        );
        let feb_29 = |year: &str| format!("{year}-02-29T00:00:00Z");
        for year in ["1900", "2023", "2100"] {
            assert_eq!(
                parse_rfc3339(&feb_29(year)),
                Err(CivilError::FieldOutOfRange {
                    field: "day",
                    value: 29
                })
            );
        }
        assert!(parse_rfc3339(&feb_29("2000")).is_ok());
    }

    #[test]
    fn both_ends_of_the_range_parse_and_one_nanosecond_beyond_does_not() {
        let accepted = [
            ("1677-09-21T00:12:43.145224192Z", Timestamp::MIN),
            ("2262-04-11T23:47:16.854775807Z", Timestamp::MAX),
            // The same instants written with offsets.
            ("1677-09-20T23:12:43.145224192-01:00", Timestamp::MIN),
            ("2262-04-12T00:47:16.854775807+01:00", Timestamp::MAX),
            ("2262-04-12T23:46:16.854775807+23:59", Timestamp::MAX),
            // Local times outside the range for instants inside it.
            ("2262-04-12T01:00:00+02:00", ts(9_223_369_200_000_000_000)),
            ("1677-09-20T23:59:59-01:00", ts(-9_223_369_201_000_000_000)),
            ("1970-01-01T01:00:00+01:00", Timestamp::UNIX_EPOCH),
        ];
        for (text, expected) in accepted {
            assert_eq!(parse_rfc3339(text), Ok(expected), "{text}");
        }
        let rejected = [
            "1677-09-21T00:12:43.145224191Z",
            "2262-04-11T23:47:16.854775808Z",
            "1677-09-20T23:12:43.145224191-01:00",
            "2262-04-12T00:47:16.854775808+01:00",
            // Inside the range in local time, outside it in UTC.
            "2262-04-11T23:47:16.854775807-00:01",
            "1677-09-21T00:12:43.145224192+00:01",
            "0000-01-01T00:00:00Z",
            "9999-12-31T23:59:59.999999999Z",
        ];
        for text in rejected {
            assert_eq!(parse_rfc3339(text), Err(CivilError::OutOfRange), "{text}");
        }
        let past_max = DateTime {
            nanosecond: 854_775_808,
            ..DateTime::from_timestamp(Timestamp::MAX)
        };
        assert_eq!(past_max.to_timestamp(), Err(CivilError::OutOfRange));
        let before_min = DateTime {
            nanosecond: 145_224_191,
            ..DateTime::from_timestamp(Timestamp::MIN)
        };
        assert_eq!(before_min.to_timestamp(), Err(CivilError::OutOfRange));
        let far = DateTime {
            year: i32::MAX,
            ..DateTime::from_timestamp(Timestamp::UNIX_EPOCH)
        };
        assert_eq!(far.to_timestamp(), Err(CivilError::OutOfRange));
    }

    #[test]
    fn accepted_variations() {
        let noon = ts(1_793_534_400_000_000_000); // 2026-11-01T12:00:00Z
        for text in [
            "2026-11-01T12:00:00Z",
            "2026-11-01t12:00:00z",
            "2026-11-01T12:00:00.0Z",
            "2026-11-01T12:00:00.000000000Z",
            "2026-11-01T12:00:00-00:00",
            "2026-11-01T12:00:00+00:00",
            "2026-11-01T13:30:00+01:30",
            "2026-11-01T00:01:00-11:59",
            "2026-10-31T12:01:00-23:59",
        ] {
            assert_eq!(parse_rfc3339(text), Ok(noon), "{text}");
        }
        assert_eq!(
            parse_rfc3339("2026-11-01T12:00:00.5Z"),
            Ok(ts(1_793_534_400_500_000_000))
        );
        assert_eq!(
            parse_rfc3339("2026-11-01T12:00:00.000000001Z"),
            Ok(ts(1_793_534_400_000_000_001))
        );
        assert_eq!(
            parse_rfc3339("2026-11-01T12:00:00.987654321Z"),
            Ok(ts(1_793_534_400_987_654_321))
        );
        assert_eq!("2026-11-01T12:00:00Z".parse::<Timestamp>(), Ok(noon));
    }

    #[test]
    fn syntax_errors_name_the_offset() {
        let syntax = |offset, expected| Err(CivilError::Syntax { offset, expected });
        let cases = [
            ("", syntax(0, "a 4-digit year")),
            ("2026", syntax(4, "`-`")),
            ("226-11-01T12:00:00Z", syntax(0, "a 4-digit year")),
            ("+2026-11-01T12:00:00Z", syntax(0, "a 4-digit year")),
            ("2026/11/01T12:00:00Z", syntax(4, "`-`")),
            ("2026-1-01T12:00:00Z", syntax(5, "a 2-digit month")),
            ("2026-11-01 12:00:00Z", syntax(10, "`T`")),
            ("2026-11-01T12:00Z", syntax(16, "`:`")),
            ("2026-11-01T12:00:00", syntax(19, "`Z`, `+` or `-`")),
            ("2026-11-01T12:00:00.Z", syntax(20, "a fraction digit")),
            ("2026-11-01T12:00:00,5Z", syntax(19, "`Z`, `+` or `-`")),
            ("2026-11-01T12:00:00+0100", syntax(22, "`:`")),
            (
                "2026-11-01T12:00:00+1:00",
                syntax(20, "a 2-digit offset hour"),
            ),
            (
                "2026-11-01T12:00:00Z ",
                syntax(20, "the end of the timestamp"),
            ),
            (
                "2026-11-01T12:00:00ZZ",
                syntax(20, "the end of the timestamp"),
            ),
            (
                "2026-11-01T12:00:00+01:00Z",
                syntax(25, "the end of the timestamp"),
            ),
            ("２026-11-01T12:00:00Z", syntax(0, "a 4-digit year")),
            ("2026-11-01T1２:00:00Z", syntax(11, "a 2-digit hour")),
        ];
        for (text, expected) in cases {
            assert_eq!(parse_rfc3339(text), expected, "{text:?}");
        }
    }

    #[test]
    fn field_errors() {
        let field = |field, value| Err(CivilError::FieldOutOfRange { field, value });
        let cases = [
            ("2026-00-01T00:00:00Z", field("month", 0)),
            ("2026-13-01T00:00:00Z", field("month", 13)),
            ("2026-01-00T00:00:00Z", field("day", 0)),
            ("2026-01-32T00:00:00Z", field("day", 32)),
            ("2026-04-31T00:00:00Z", field("day", 31)),
            ("2026-01-01T24:00:00Z", field("hour", 24)),
            ("2026-01-01T00:60:00Z", field("minute", 60)),
            ("2026-01-01T00:00:61Z", field("second", 61)),
            ("2026-01-01T00:00:00+24:00", field("offset hour", 24)),
            ("2026-01-01T00:00:00-00:60", field("offset minute", 60)),
        ];
        for (text, expected) in cases {
            assert_eq!(parse_rfc3339(text), expected, "{text}");
        }
        assert_eq!(
            parse_rfc3339("2016-12-31T23:59:60Z"),
            Err(CivilError::LeapSecond)
        );
        assert_eq!(
            parse_rfc3339("2026-01-01T00:00:00.0000000001Z"),
            Err(CivilError::TooPrecise)
        );
        assert_eq!(
            parse_rfc3339("2026-01-01T00:00:00.1234567890Z"),
            Err(CivilError::TooPrecise)
        );
        // A long run of digits stops at the tenth.
        let long = format!("2026-01-01T00:00:00.{}Z", "0".repeat(100_000));
        assert_eq!(parse_rfc3339(&long), Err(CivilError::TooPrecise));
        let epoch = DateTime::from_timestamp(Timestamp::UNIX_EPOCH);
        assert_eq!(
            DateTime {
                second: 60,
                ..epoch
            }
            .to_timestamp(),
            field("second", 60)
        );
        assert_eq!(
            DateTime {
                nanosecond: 1_000_000_000,
                ..epoch
            }
            .to_timestamp(),
            field("nanosecond", 1_000_000_000)
        );
        assert_eq!(
            DateTime {
                year: 2023,
                month: 2,
                day: 29,
                ..epoch
            }
            .to_timestamp(),
            field("day", 29)
        );
        assert_eq!(
            DateTime { hour: 24, ..epoch }.to_timestamp(),
            field("hour", 24)
        );
        assert_eq!(
            DateTime {
                minute: 60,
                ..epoch
            }
            .to_timestamp(),
            field("minute", 60)
        );
        assert_eq!(
            DateTime { month: 13, ..epoch }.to_timestamp(),
            field("month", 13)
        );
        assert_eq!(
            DateTime { month: 0, ..epoch }.to_timestamp(),
            field("month", 0)
        );
    }

    #[test]
    fn errors_display_without_the_input() {
        assert_eq!(
            parse_rfc3339("2026-11-01T12:00").unwrap_err().to_string(),
            "malformed RFC 3339 timestamp: expected `:` at byte 16"
        );
        assert_eq!(
            CivilError::OutOfRange.to_string(),
            "time is outside the timestamp range, 1677-09-21T00:12:43.145224192Z to 2262-04-11T23:47:16.854775807Z"
        );
        assert_eq!(
            CivilError::FieldOutOfRange {
                field: "day",
                value: 31
            }
            .to_string(),
            "day 31 is out of range"
        );
    }
}
