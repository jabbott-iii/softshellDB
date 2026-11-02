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

//! Canonical LEB128 varints: unsigned 32- and 64-bit, and zigzag `i64`.
//!
//! Each byte holds 7 value bits, least significant group first; its high
//! bit (`0x80`) says another byte follows. Decoding accepts exactly the
//! bytes the encoder writes, so every value has one encoding. At the last
//! byte a type allows (the 5th for 32 bits, the 10th for 64), the checks
//! run in this order:
//!
//! 1. **Overlong** ([`CoreError::VarintOverlong`]): its high bit is set.
//!    No further byte is read.
//! 2. **Overflow** ([`CoreError::VarintOverflow`]): it holds bits beyond
//!    the type's width (above `0x0F` for 32 bits, above `0x01` for 64).
//! 3. **Non-minimal** ([`CoreError::VarintNonMinimal`]): it is `0x00`.
//!
//! Before that byte, a final byte of `0x00` after the first byte is
//! non-minimal, as in `80 00`. Input that ends while a high bit says more
//! follows is [`CoreError::UnexpectedEnd`] with `needed` one more than the
//! bytes present. Every decoding error, and every
//! [`CoreError::LimitExceeded`], names the varint's first byte; a failed
//! read leaves the reader unchanged.
//!
//! A declared size is checked before it sizes anything.
//! [`read_at_most`] checks it against a limit. [`read_len`] and
//! [`read_count`] also check it against the input, which is right only when
//! the bytes or items it describes start right after the varint (an SDOC
//! string, say). When other fields sit in between (an SST entry's key and
//! value lengths), or the size describes something not in this input (a
//! decompressed length, a frame body still to arrive), use [`read_at_most`]
//! and let the later read check the input where the bytes really are.
//!
//! The encoding of a value does not depend on its declared width, so
//! [`write_u64`] writes 32-bit values too.
//!
//! ```
//! use softshell_core::bytes::{ByteReader, ByteWriter};
//! use softshell_core::varint;
//!
//! let mut w = ByteWriter::new();
//! varint::write_u64(&mut w, 300);
//! varint::write_i64(&mut w, -3);
//! assert_eq!(w.as_slice(), [0xAC, 0x02, 0x05]);
//!
//! let bytes = w.into_vec();
//! let mut r = ByteReader::new(&bytes);
//! assert_eq!(varint::read_u32(&mut r)?, 300);
//! assert_eq!(varint::read_i64(&mut r)?, -3);
//! # Ok::<(), softshell_core::error::CoreError>(())
//! ```

#![deny(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use std::num::NonZeroUsize;

use crate::bytes::{ByteReader, ByteWriter, to_u64};
use crate::error::CoreError;

/// Reads a canonical unsigned varint of at most 5 bytes into a `u32`.
pub fn read_u32(r: &mut ByteReader<'_>) -> Result<u32, CoreError> {
    let (value, after) = decode(r, u32::BITS)?;
    // `decode` returns only values that fit in 32 bits.
    let value = u32::try_from(value).map_err(|_| CoreError::VarintOverflow {
        offset: r.offset(),
        bits: u32::BITS,
    })?;
    *r = after;
    Ok(value)
}

/// Reads a canonical unsigned varint of at most 10 bytes into a `u64`.
///
/// The value is unchecked: before it sizes anything, compare it with a
/// limit, or read it with [`read_at_most`], [`read_len`] or [`read_count`]
/// instead.
pub fn read_u64(r: &mut ByteReader<'_>) -> Result<u64, CoreError> {
    let (value, after) = decode(r, u64::BITS)?;
    *r = after;
    Ok(value)
}

/// Reads a canonical zigzag varint of at most 10 bytes into an `i64`
/// (`0, -1, 1, -2, …` are encoded as `0, 1, 2, 3, …`).
pub fn read_i64(r: &mut ByteReader<'_>) -> Result<i64, CoreError> {
    read_u64(r).map(zigzag_decode)
}

/// Reads a declared size as a canonical 64-bit varint and checks it
/// against `max` only: a larger size is [`CoreError::LimitExceeded`] at the
/// varint's first byte.
///
/// Use it when the bytes the size describes do not start right after the
/// varint (see the [module documentation](self)); when they do, prefer
/// [`read_len`], which also checks the input.
pub fn read_at_most(r: &mut ByteReader<'_>, max: usize) -> Result<usize, CoreError> {
    let (found, after) = decode(r, u64::BITS)?;
    let size = at_most(r, found, max)?;
    *r = after;
    Ok(size)
}

/// Reads the length of the bytes that follow, as a canonical 64-bit varint,
/// and checks it before anything is allocated: at most `max`, else
/// [`CoreError::LimitExceeded`] at the varint, and at most the bytes
/// remaining after the varint, else [`CoreError::UnexpectedEnd`] at the
/// byte after the varint.
///
/// The bytes must start right after the varint, or that check and its
/// offset are wrong; otherwise use [`read_at_most`]. For a count of items,
/// use [`read_count`].
pub fn read_len(r: &mut ByteReader<'_>, max: usize) -> Result<usize, CoreError> {
    read_count(r, max, NonZeroUsize::MIN)
}

/// Reads a count of the items that follow, as a canonical 64-bit varint,
/// and checks it: at most `max`, else [`CoreError::LimitExceeded`] at the
/// varint, and `count` items of at least `min_item_bytes` each must fit in
/// the bytes remaining after the varint, else [`CoreError::UnexpectedEnd`]
/// at the byte after the varint. The items must start right after the
/// varint, as with [`read_len`].
///
/// The count bounds how many items the input can hold, not how much a
/// decoder may reserve: each level of a nested format counts against the
/// same bytes. A recursive decoder therefore caps what it preallocates (say
/// `Vec::with_capacity(count.min(1024))`) and lets the vector grow as items
/// decode.
pub fn read_count(
    r: &mut ByteReader<'_>,
    max: usize,
    min_item_bytes: NonZeroUsize,
) -> Result<usize, CoreError> {
    let (found, after) = decode(r, u64::BITS)?;
    at_most(r, found, max)?;
    let count = after.check_count(found, min_item_bytes)?;
    *r = after;
    Ok(count)
}

/// Appends the canonical varint of `value` (1 to 10 bytes). Use it for
/// `u32` values too: `write_u64(w, n.into())` writes the bytes
/// [`read_u32`] expects.
pub fn write_u64(w: &mut ByteWriter, mut value: u64) {
    while value >= 0x80 {
        // Truncation to the low 7 bits is the point.
        w.write_u8((value as u8) | 0x80);
        value = value.wrapping_shr(7);
    }
    w.write_u8(value as u8);
}

/// Appends the canonical zigzag varint of `value` (1 to 10 bytes).
pub fn write_i64(w: &mut ByteWriter, value: i64) {
    write_u64(w, zigzag_encode(value));
}

/// Appends a length or count: the counterpart of [`read_len`] and
/// [`read_count`].
pub fn write_len(w: &mut ByteWriter, len: usize) {
    write_u64(w, to_u64(len));
}

/// Checks a size `found` in the varint at `r` against `max`.
fn at_most(r: &ByteReader<'_>, found: u64, max: usize) -> Result<usize, CoreError> {
    match usize::try_from(found) {
        Ok(size) if size <= max => Ok(size),
        _ => Err(CoreError::LimitExceeded {
            offset: r.offset(),
            max: to_u64(max),
            found,
        }),
    }
}

/// Decodes a canonical varint whose value must fit in `bits` (32 or 64)
/// from a copy of `r`. Returns the value and the copy, advanced past the
/// varint; the caller commits the copy once its own checks pass.
fn decode<'a>(r: &ByteReader<'a>, bits: u32) -> Result<(u64, ByteReader<'a>), CoreError> {
    let offset = r.offset();
    let mut probe = r.clone();
    let mut value: u64 = 0;
    let mut shift: u32 = 0;
    loop {
        let Ok(byte) = probe.read_u8() else {
            let available = to_u64(r.remaining());
            return Err(CoreError::UnexpectedEnd {
                offset,
                needed: available.saturating_add(1),
                available,
            });
        };
        let payload = u64::from(byte & 0x7F);
        // The value bits this byte may still hold; at most 7 means it is
        // the last byte the width allows.
        let room = bits.saturating_sub(shift);
        if room <= 7 {
            if byte & 0x80 != 0 {
                return Err(CoreError::VarintOverlong { offset, bits });
            }
            if payload.checked_shr(room).unwrap_or(0) != 0 {
                return Err(CoreError::VarintOverflow { offset, bits });
            }
        }
        // `shift < bits <= 64` here, and the check above keeps every bit.
        value |= payload.wrapping_shl(shift);
        if byte & 0x80 == 0 {
            if byte == 0 && shift > 0 {
                return Err(CoreError::VarintNonMinimal { offset });
            }
            return Ok((value, probe));
        }
        shift = shift.wrapping_add(7);
    }
}

const fn zigzag_encode(value: i64) -> u64 {
    value.cast_unsigned().wrapping_shl(1) ^ value.wrapping_shr(63).cast_unsigned()
}

const fn zigzag_decode(value: u64) -> i64 {
    value.wrapping_shr(1).cast_signed() ^ (value & 1).cast_signed().wrapping_neg()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enc_u64(v: u64) -> Vec<u8> {
        let mut w = ByteWriter::new();
        write_u64(&mut w, v);
        w.into_vec()
    }

    fn enc_i64(v: i64) -> Vec<u8> {
        let mut w = ByteWriter::new();
        write_i64(&mut w, v);
        w.into_vec()
    }

    /// Minimal LEB128 length: one byte per started group of 7 bits.
    fn minimal_len(v: u64) -> usize {
        let bits = u64::BITS.wrapping_sub(v.leading_zeros()).max(1);
        usize::try_from(bits.div_ceil(7)).unwrap()
    }

    /// 0, 1, and 2^k - 1, 2^k, 2^k + 1 for every k, plus the extremes.
    fn edge_u64() -> Vec<u64> {
        let mut v = vec![0, 1, u64::MAX, u64::MAX.wrapping_sub(1)];
        for k in 0..u64::BITS {
            let p = 1u64.wrapping_shl(k);
            v.extend([p.wrapping_sub(1), p, p.wrapping_add(1)]);
        }
        v
    }

    fn edge_i64() -> Vec<i64> {
        let mut v = vec![0, 1, -1, 63, -64, 64, -65, i64::MIN, i64::MAX];
        for u in edge_u64() {
            let i = u.cast_signed();
            v.extend([i, i.wrapping_neg()]);
        }
        v
    }

    #[test]
    fn edge_values_round_trip_at_minimal_length() {
        for v in edge_u64() {
            let bytes = enc_u64(v);
            assert_eq!(bytes.len(), minimal_len(v), "{v}");
            let mut r = ByteReader::new(&bytes);
            assert_eq!(read_u64(&mut r), Ok(v));
            assert_eq!(r.finish(), Ok(()));

            let mut r = ByteReader::new(&bytes);
            match u32::try_from(v) {
                Ok(v32) => assert_eq!(read_u32(&mut r), Ok(v32)),
                Err(_) => {
                    assert!(read_u32(&mut r).is_err(), "{v}");
                    assert_eq!(r.position(), 0);
                }
            }
        }
        for v in edge_i64() {
            let bytes = enc_i64(v);
            assert_eq!(bytes.len(), minimal_len(zigzag_encode(v)), "{v}");
            let mut r = ByteReader::new(&bytes);
            assert_eq!(read_i64(&mut r), Ok(v), "{v}");
            assert_eq!(r.finish(), Ok(()));
        }
    }

    #[test]
    fn known_encodings() {
        assert_eq!(enc_u64(0), [0x00]);
        assert_eq!(enc_u64(127), [0x7F]);
        assert_eq!(enc_u64(128), [0x80, 0x01]);
        assert_eq!(enc_u64(300), [0xAC, 0x02]);
        assert_eq!(enc_u64(u32::MAX.into()), [0xFF, 0xFF, 0xFF, 0xFF, 0x0F]);
        assert_eq!(enc_u64(1 << 32), [0x80, 0x80, 0x80, 0x80, 0x10]);
        assert_eq!(
            enc_u64(1 << 63),
            [0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x01]
        );
        assert_eq!(
            enc_u64(u64::MAX),
            [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01]
        );
        assert_eq!(enc_i64(0), [0x00]);
        assert_eq!(enc_i64(-1), [0x01]);
        assert_eq!(enc_i64(1), [0x02]);
        assert_eq!(enc_i64(-64), [0x7F]);
        assert_eq!(enc_i64(64), [0x80, 0x01]);
        assert_eq!(
            enc_i64(i64::MIN),
            [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01]
        );
        assert_eq!(
            enc_i64(i64::MAX),
            [0xFE, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01]
        );
        let mut w = ByteWriter::new();
        write_len(&mut w, 300);
        assert_eq!(w.as_slice(), [0xAC, 0x02]);
    }

    type Decoder = fn(&mut ByteReader<'_>) -> Result<u64, CoreError>;

    fn u32_as_u64(r: &mut ByteReader<'_>) -> Result<u64, CoreError> {
        read_u32(r).map(u64::from)
    }

    fn i64_as_u64(r: &mut ByteReader<'_>) -> Result<u64, CoreError> {
        read_i64(r).map(zigzag_encode)
    }

    /// Asserts `read` rejects `input` with `expected` (built from the
    /// offset 50, where the varint starts) and does not move.
    fn assert_rejects(read: Decoder, input: &[u8], expected: CoreError) {
        let mut r = ByteReader::with_base(input, 50);
        assert_eq!(read(&mut r), Err(expected), "{input:02X?}");
        assert_eq!(r.position(), 0, "a failed read must not consume");
    }

    const fn end(available: u64) -> CoreError {
        CoreError::UnexpectedEnd {
            offset: 50,
            needed: available.saturating_add(1),
            available,
        }
    }

    #[test]
    fn non_canonical_encodings_are_rejected() {
        use CoreError::{VarintNonMinimal, VarintOverflow, VarintOverlong};
        let all: [Decoder; 3] = [u32_as_u64, read_u64, i64_as_u64];
        let wide: [Decoder; 2] = [read_u64, i64_as_u64];
        let non_minimal = VarintNonMinimal { offset: 50 };
        let overlong32 = VarintOverlong {
            offset: 50,
            bits: 32,
        };
        let overlong64 = VarintOverlong {
            offset: 50,
            bits: 64,
        };
        let overflow32 = VarintOverflow {
            offset: 50,
            bits: 32,
        };
        let overflow64 = VarintOverflow {
            offset: 50,
            bits: 64,
        };
        let ff9 = [0xFF; 9];
        let with_last = |prefix: &[u8], last: &[u8]| [prefix, last].concat();

        for read in all {
            // Non-minimal: a zero final byte after the first.
            for input in [
                &[0x80, 0x00][..],
                &[0xFF, 0x00],
                &[0x81, 0x00],
                &[0xFF, 0x80, 0x00],
                &[0x81, 0x80, 0x00],
                &[0x80, 0x80, 0x80, 0x80, 0x00],
            ] {
                assert_rejects(read, input, non_minimal.clone());
            }
            // Truncated: the input ends while a high bit is set.
            assert_rejects(read, &[], end(0));
            assert_rejects(read, &[0x80], end(1));
            assert_rejects(read, &[0xFF, 0xFF], end(2));
            assert_rejects(read, &[0x80, 0x80, 0x80, 0x80], end(4));
        }
        // 32 bits: the 5th byte is the last allowed. A wider value is an
        // overflow if it ends there (2^34), else overlong (u64::MAX).
        let (five, ten) = (enc_u64(1 << 34), enc_u64(u64::MAX));
        for input in [
            &[0x80, 0x80, 0x80, 0x80, 0x10][..],
            &[0xFF, 0xFF, 0xFF, 0xFF, 0x7F],
            &five,
        ] {
            assert_rejects(u32_as_u64, input, overflow32.clone());
        }
        for input in [
            &[0x80; 5][..],
            &[0x80, 0x80, 0x80, 0x80, 0x80, 0x00],
            &[0xFF, 0xFF, 0xFF, 0xFF, 0x8F, 0x00],
            &ten,
        ] {
            assert_rejects(u32_as_u64, input, overlong32.clone());
        }
        // 64 bits: the 10th byte is the last allowed, and must be 01.
        for read in wide {
            assert_rejects(read, &[0x80; 10], overlong64.clone());
            assert_rejects(read, &with_last(&ff9, &[0x81]), overlong64.clone());
            assert_rejects(read, &with_last(&ff9, &[0x81, 0x00]), overlong64.clone());
            assert_rejects(read, &with_last(&[0x80; 9], &[0x00]), non_minimal.clone());
            assert_rejects(read, &ff9, end(9));
            for last in 0x02..=0x7F {
                assert_rejects(read, &with_last(&ff9, &[last]), overflow64.clone());
            }
        }
    }

    #[test]
    fn errors_report_the_varint_start() {
        let buf = [0x01, 0x02, 0x80, 0x00];
        let mut r = ByteReader::with_base(&buf, 10);
        assert_eq!(read_u64(&mut r), Ok(1));
        assert_eq!(read_u64(&mut r), Ok(2));
        assert_eq!(
            read_u64(&mut r),
            Err(CoreError::VarintNonMinimal { offset: 12 })
        );
        assert_eq!(r.position(), 2);
        let buf = [0x01, 0xFF, 0xFF];
        let mut r = ByteReader::with_base(&buf, 10);
        assert_eq!(read_u32(&mut r), Ok(1));
        assert_eq!(
            read_u32(&mut r),
            Err(CoreError::UnexpectedEnd {
                offset: 11,
                needed: 3,
                available: 2
            })
        );
        assert_eq!(r.position(), 1);
        let buf = [0x01, 0x80, 0x80, 0x80, 0x80, 0x80];
        let mut r = ByteReader::with_base(&buf, 10);
        assert_eq!(read_u32(&mut r), Ok(1));
        assert_eq!(
            read_u32(&mut r),
            Err(CoreError::VarintOverlong {
                offset: 11,
                bits: 32
            })
        );
        assert_eq!(r.position(), 1);
        let buf = [
            0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x02,
        ];
        let mut r = ByteReader::with_base(&buf, 10);
        assert_eq!(read_u64(&mut r), Ok(1));
        assert_eq!(
            read_u64(&mut r),
            Err(CoreError::VarintOverflow {
                offset: 11,
                bits: 64
            })
        );
        assert_eq!(r.position(), 1);
    }

    /// Checks one input against every decoder: each either fails without
    /// consuming, or returns `v` with the encoding of `v` equal to exactly
    /// the bytes it consumed. With the round trips, this means each value
    /// has exactly one accepted encoding.
    fn check_canonical(input: &[u8]) {
        let mut r = ByteReader::new(input);
        match read_u64(&mut r) {
            Ok(v) => assert_eq!(
                Some(enc_u64(v).as_slice()),
                input.get(..r.position()),
                "{input:02X?}"
            ),
            Err(_) => assert_eq!(r.position(), 0),
        }
        let mut r = ByteReader::new(input);
        match read_u32(&mut r) {
            Ok(v) => assert_eq!(
                Some(enc_u64(v.into()).as_slice()),
                input.get(..r.position()),
                "{input:02X?}"
            ),
            Err(_) => assert_eq!(r.position(), 0),
        }
        let mut r = ByteReader::new(input);
        match read_i64(&mut r) {
            Ok(v) => assert_eq!(
                Some(enc_i64(v).as_slice()),
                input.get(..r.position()),
                "{input:02X?}"
            ),
            Err(_) => assert_eq!(r.position(), 0),
        }
    }

    #[test]
    fn every_input_of_up_to_two_bytes_is_canonical() {
        check_canonical(&[]);
        for a in 0..=u8::MAX {
            check_canonical(&[a]);
            for b in 0..=u8::MAX {
                check_canonical(&[a, b]);
            }
        }
    }

    #[test]
    fn long_inputs_are_canonical() {
        // Every length up to 11: each continuation byte is 0x80 (no value
        // bits) or 0xFF (all value bits), in every combination, followed by
        // every possible byte.
        for len in 1..=11u32 {
            let combos = 1usize.wrapping_shl(len.wrapping_sub(1));
            for bits in 0..combos {
                let mut input = Vec::new();
                for i in 1..len {
                    let all = bits.wrapping_shr(i.wrapping_sub(1)) & 1 == 1;
                    input.push(if all { 0xFF } else { 0x80 });
                }
                for last in 0..=u8::MAX {
                    input.push(last);
                    check_canonical(&input);
                    input.pop();
                }
            }
        }
    }

    /// Every value below 2^21 (every 1- to 3-byte encoding) is accepted
    /// from exactly one input of up to 3 bytes, the encoder's.
    #[test]
    fn every_value_below_2_pow_21_has_exactly_one_encoding() {
        let mut seen = vec![0u8; 1 << 21];
        let mut check = |input: &[u8]| {
            let mut r = ByteReader::new(input);
            let Ok(v) = read_u64(&mut r) else {
                return;
            };
            assert_eq!(r.remaining(), 0, "{input:02X?}");
            assert_eq!(enc_u64(v).as_slice(), input, "{input:02X?}");
            let mut r32 = ByteReader::new(input);
            assert_eq!(read_u32(&mut r32).map(u64::from), Ok(v), "{input:02X?}");
            let slot = usize::try_from(v).ok().and_then(|i| seen.get_mut(i));
            let slot = slot.unwrap_or_else(|| panic!("{v} from {input:02X?}"));
            *slot = slot.saturating_add(1);
        };
        for a in 0..=u8::MAX {
            check(&[a]);
            if a & 0x80 == 0 {
                continue;
            }
            for b in 0..=u8::MAX {
                check(&[a, b]);
                if b & 0x80 == 0 {
                    continue;
                }
                for c in 0..=u8::MAX {
                    check(&[a, b, c]);
                }
            }
        }
        assert!(seen.iter().all(|&count| count == 1));
    }

    #[test]
    fn read_len_checks_the_limit_then_the_input() {
        // 2^32 declared in a 10-byte input.
        let mut input = enc_u64(1 << 32);
        input.resize(10, 0);
        let mut r = ByteReader::with_base(&input, 1);
        assert_eq!(
            read_len(&mut r, 16 << 20),
            Err(CoreError::LimitExceeded {
                offset: 1,
                max: 16 << 20,
                found: 1 << 32
            })
        );
        assert_eq!(r.position(), 0);

        let input = [0x05, 1, 2, 3];
        let mut r = ByteReader::new(&input);
        assert_eq!(
            read_len(&mut r, 100),
            Err(CoreError::UnexpectedEnd {
                offset: 1,
                needed: 5,
                available: 3
            })
        );
        assert_eq!(r.position(), 0);

        let input = [0x03, 1, 2, 3];
        let mut r = ByteReader::new(&input);
        assert_eq!(read_len(&mut r, 3), Ok(3));
        assert_eq!(r.read_bytes(3), Ok(&[1, 2, 3][..]));
        let mut r = ByteReader::new(&input);
        assert_eq!(
            read_len(&mut r, 2),
            Err(CoreError::LimitExceeded {
                offset: 0,
                max: 2,
                found: 3
            })
        );
        assert_eq!(
            read_len(&mut r, 0),
            Err(CoreError::LimitExceeded {
                offset: 0,
                max: 0,
                found: 3
            })
        );

        let max = enc_u64(u64::MAX);
        let mut r = ByteReader::new(&max);
        assert_eq!(
            read_len(&mut r, usize::MAX),
            Err(CoreError::UnexpectedEnd {
                offset: 10,
                needed: u64::MAX,
                available: 0
            })
        );
        // A malformed varint is reported as such, before any limit.
        let mut r = ByteReader::new(&[0x80, 0x00]);
        assert_eq!(
            read_len(&mut r, 0),
            Err(CoreError::VarintNonMinimal { offset: 0 })
        );
    }

    #[test]
    fn read_at_most_checks_only_the_limit() {
        // 9 declared with nothing after it: the size describes other bytes.
        let input = [0x01, 0x09];
        let mut r = ByteReader::with_base(&input, 30);
        assert_eq!(read_u64(&mut r), Ok(1));
        assert_eq!(read_at_most(&mut r, 9), Ok(9));
        assert_eq!(r.remaining(), 0);
        let mut r = ByteReader::with_base(&input, 30);
        assert_eq!(read_u64(&mut r), Ok(1));
        assert_eq!(
            read_at_most(&mut r, 8),
            Err(CoreError::LimitExceeded {
                offset: 31,
                max: 8,
                found: 9
            })
        );
        assert_eq!(r.position(), 1);
        let max = enc_u64(u64::MAX);
        assert_eq!(
            read_at_most(&mut ByteReader::new(&max), usize::MAX).map(to_u64),
            Ok(u64::MAX)
        );
        // A malformed varint is reported as such, before any limit.
        assert_eq!(
            read_at_most(&mut ByteReader::new(&[0x80, 0x00]), 0),
            Err(CoreError::VarintNonMinimal { offset: 0 })
        );
    }

    #[test]
    fn read_count_checks_items_against_the_input() {
        let four = NonZeroUsize::new(4).unwrap();
        // 3 items of 4 bytes need 12; only 8 follow the varint.
        let mut input = vec![0x03];
        input.resize(9, 0);
        let mut r = ByteReader::with_base(&input, 20);
        assert_eq!(
            read_count(&mut r, 4_096, four),
            Err(CoreError::UnexpectedEnd {
                offset: 21,
                needed: 12,
                available: 8
            })
        );
        assert_eq!(r.position(), 0);
        assert_eq!(read_count(&mut r, 4_096, NonZeroUsize::MIN), Ok(3));
        assert_eq!(r.position(), 1);

        let mut r = ByteReader::new(&input);
        assert_eq!(
            read_count(&mut r, 2, four),
            Err(CoreError::LimitExceeded {
                offset: 0,
                max: 2,
                found: 3
            })
        );
        // count × size overflows u64: needed saturates.
        let mut big = enc_u64(u64::MAX);
        big.push(0);
        let mut r = ByteReader::new(&big);
        assert_eq!(
            read_count(&mut r, usize::MAX, four),
            Err(CoreError::UnexpectedEnd {
                offset: 10,
                needed: u64::MAX,
                available: 1
            })
        );
        let two = [0x02, 0, 0, 0, 0, 0, 0, 0, 0];
        let mut r = ByteReader::new(&two);
        assert_eq!(read_count(&mut r, 2, four), Ok(2));
        assert_eq!(r.remaining(), 8);
    }
}
