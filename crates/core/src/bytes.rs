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

//! [`ByteReader`], a cursor over untrusted bytes whose every read is
//! checked, and [`ByteWriter`], its counterpart for encoders.
//!
//! What every `ByteReader` method guarantees, whatever the input:
//!
//! - It never panics. It splits the unread slice instead of indexing it,
//!   so even a length of `usize::MAX` is only an error.
//! - It never allocates: reads borrow from the input.
//! - A call that fails leaves the reader unchanged.
//! - Errors name absolute offsets (see [`CoreError`]).
//!
//! A size read from the input is checked before it sizes anything: against
//! a limit (a [`Limits`](crate::limits::Limits) field) and, when the bytes
//! or items it describes follow in this input, against what remains. For a
//! varint that is [`varint::read_at_most`](crate::varint::read_at_most),
//! [`varint::read_len`](crate::varint::read_len) or
//! [`varint::read_count`](crate::varint::read_count); for a fixed-width
//! count, [`ByteReader::check_count`]. A size passed straight to
//! [`read_bytes`](ByteReader::read_bytes),
//! [`sub_reader`](ByteReader::sub_reader) or
//! [`split_tail`](ByteReader::split_tail) needs no check, because those
//! fail on their own.
//!
//! A checked count bounds how many items the input can hold, not how much
//! a decoder may reserve: each level of a nested format counts against the
//! same bytes, so a recursive decoder caps what it preallocates.
//!
//! ```
//! use softshell_core::bytes::{ByteReader, ByteWriter};
//!
//! let mut w = ByteWriter::new();
//! w.write_u32_le(7);
//! w.write_u16_be(0x0102);
//! let bytes = w.into_vec();
//!
//! let mut r = ByteReader::new(&bytes);
//! assert_eq!(r.read_u32_le()?, 7);
//! assert_eq!(r.read_u16_be()?, 0x0102);
//! r.finish()?;
//! # Ok::<(), softshell_core::error::CoreError>(())
//! ```

#![deny(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use std::fmt;
use std::num::NonZeroUsize;

use crate::error::CoreError;

// Offsets and sizes in errors are `u64`; this makes `usize as u64` lossless.
const _: () = assert!(usize::BITS <= u64::BITS);

/// Widens a size or position to the `u64` that errors carry.
pub(crate) const fn to_u64(n: usize) -> u64 {
    n as u64
}

/// A cursor over untrusted bytes. Every read is checked and returns a
/// `Result`; see the [module documentation](self) for the guarantees.
///
/// Cloning is cheap and is the way to look ahead or to come back to a
/// position. `Debug` shows offsets and sizes, never the bytes, so a reader
/// may be logged.
#[derive(Clone)]
pub struct ByteReader<'a> {
    /// The bytes not yet read.
    rest: &'a [u8],
    /// Absolute offset of this reader's first byte.
    base: u64,
    /// Bytes read so far.
    consumed: usize,
}

impl<'a> ByteReader<'a> {
    /// Creates a reader over `bytes` whose offsets count from 0.
    #[must_use]
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self::with_base(bytes, 0)
    }

    /// Creates a reader over `bytes` whose first byte is at absolute offset
    /// `base` (a block's offset in its file, say), so errors name file
    /// offsets.
    #[must_use]
    pub const fn with_base(bytes: &'a [u8], base: u64) -> Self {
        Self {
            rest: bytes,
            base,
            consumed: 0,
        }
    }

    /// Bytes this reader has read: the position of the next byte, counted
    /// from this reader's first byte.
    #[must_use]
    pub const fn position(&self) -> usize {
        self.consumed
    }

    /// The absolute offset of the next byte (saturating at `u64::MAX`).
    #[must_use]
    pub const fn offset(&self) -> u64 {
        self.base.saturating_add(to_u64(self.consumed))
    }

    /// Bytes not yet read.
    #[must_use]
    pub const fn remaining(&self) -> usize {
        self.rest.len()
    }

    /// Reads one byte.
    pub fn read_u8(&mut self) -> Result<u8, CoreError> {
        let Some((&byte, rest)) = self.rest.split_first() else {
            return Err(self.end_error(1));
        };
        self.advance(rest, 1);
        Ok(byte)
    }

    /// Reads a little-endian `u16`.
    pub fn read_u16_le(&mut self) -> Result<u16, CoreError> {
        self.read_array().map(u16::from_le_bytes)
    }

    /// Reads a big-endian `u16`.
    pub fn read_u16_be(&mut self) -> Result<u16, CoreError> {
        self.read_array().map(u16::from_be_bytes)
    }

    /// Reads a little-endian `u32`.
    pub fn read_u32_le(&mut self) -> Result<u32, CoreError> {
        self.read_array().map(u32::from_le_bytes)
    }

    /// Reads a big-endian `u32`.
    pub fn read_u32_be(&mut self) -> Result<u32, CoreError> {
        self.read_array().map(u32::from_be_bytes)
    }

    /// Reads a little-endian `u64`.
    pub fn read_u64_le(&mut self) -> Result<u64, CoreError> {
        self.read_array().map(u64::from_le_bytes)
    }

    /// Reads a big-endian `u64`.
    pub fn read_u64_be(&mut self) -> Result<u64, CoreError> {
        self.read_array().map(u64::from_be_bytes)
    }

    /// Reads exactly `N` bytes into an array: a magic number, a 16-byte
    /// `Id`, or any other fixed-width field (with `f64::from_le_bytes`,
    /// `i16::from_be_bytes` and their siblings).
    pub fn read_array<const N: usize>(&mut self) -> Result<[u8; N], CoreError> {
        let Some((&head, rest)) = self.rest.split_first_chunk::<N>() else {
            return Err(self.end_error(N));
        };
        self.advance(rest, N);
        Ok(head)
    }

    /// Reads exactly `len` bytes, borrowed from the input. Discarding the
    /// result skips them.
    pub fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], CoreError> {
        let Some((head, rest)) = self.rest.split_at_checked(len) else {
            return Err(self.end_error(len));
        };
        self.advance(rest, len);
        Ok(head)
    }

    /// Splits off the next `len` bytes as a reader of their own (a
    /// container's body, a record's payload) and moves this reader past
    /// them. The new reader cannot read beyond those bytes, and its offsets
    /// continue this reader's.
    pub fn sub_reader(&mut self, len: usize) -> Result<ByteReader<'a>, CoreError> {
        let base = self.offset();
        let bytes = self.read_bytes(len)?;
        Ok(ByteReader::with_base(bytes, base))
    }

    /// Splits off the last `len` unread bytes as a reader of their own (a
    /// block trailer, a footer, an array of restart offsets). This reader
    /// then ends where they begin; its position does not change.
    pub fn split_tail(&mut self, len: usize) -> Result<ByteReader<'a>, CoreError> {
        let Some((head, tail)) = self
            .rest
            .len()
            .checked_sub(len)
            .and_then(|keep| self.rest.split_at_checked(keep))
        else {
            return Err(self.end_error(len));
        };
        let base = self.offset().saturating_add(to_u64(head.len()));
        self.rest = head;
        Ok(ByteReader::with_base(tail, base))
    }

    /// Checks a count that was read as a fixed-width integer (a WAL batch's
    /// operation count, say): `count` items of at least `min_item_bytes`
    /// each, starting at the current position, must fit in the unread
    /// bytes. Returns the count as a `usize`.
    ///
    /// Otherwise returns [`CoreError::UnexpectedEnd`] at the current offset,
    /// with `needed` the lower bound `count × min_item_bytes`, saturating at
    /// `u64::MAX`. Compare the count with any [`Limits`](crate::limits::Limits)
    /// field first, and see the [module documentation](self) before
    /// preallocating from it.
    pub fn check_count(
        &self,
        count: u64,
        min_item_bytes: NonZeroUsize,
    ) -> Result<usize, CoreError> {
        let needed = count.checked_mul(to_u64(min_item_bytes.get()));
        match (needed, usize::try_from(count)) {
            (Some(needed), Ok(count)) if needed <= to_u64(self.rest.len()) => Ok(count),
            (needed, _) => Err(CoreError::UnexpectedEnd {
                offset: self.offset(),
                needed: needed.unwrap_or(u64::MAX),
                available: to_u64(self.rest.len()),
            }),
        }
    }

    /// Succeeds if every byte was read; otherwise returns
    /// [`CoreError::TrailingBytes`].
    pub fn finish(&self) -> Result<(), CoreError> {
        if self.rest.is_empty() {
            Ok(())
        } else {
            Err(CoreError::TrailingBytes {
                offset: self.offset(),
                count: to_u64(self.rest.len()),
            })
        }
    }

    const fn advance(&mut self, rest: &'a [u8], consumed: usize) {
        self.rest = rest;
        // Never saturates: `consumed` is at most the input's length.
        self.consumed = self.consumed.saturating_add(consumed);
    }

    const fn end_error(&self, needed: usize) -> CoreError {
        CoreError::UnexpectedEnd {
            offset: self.offset(),
            needed: to_u64(needed),
            available: to_u64(self.rest.len()),
        }
    }
}

/// Shows offsets and sizes, never the contents: a reader may hold user
/// data, which must not reach logs.
impl fmt::Debug for ByteReader<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ByteReader")
            .field("offset", &self.offset())
            .field("position", &self.consumed)
            .field("remaining", &self.rest.len())
            .finish()
    }
}

/// An append-only byte buffer for encoders, the counterpart of
/// [`ByteReader`]. Writes cannot fail.
#[derive(Clone, Default)]
pub struct ByteWriter {
    buf: Vec<u8>,
}

impl ByteWriter {
    /// Creates an empty writer.
    #[must_use]
    pub const fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// Bytes written so far.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.buf.len()
    }

    /// Whether nothing has been written.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// The bytes written so far.
    #[must_use]
    pub const fn as_slice(&self) -> &[u8] {
        self.buf.as_slice()
    }

    /// Returns the bytes written.
    #[must_use]
    pub fn into_vec(self) -> Vec<u8> {
        self.buf
    }

    /// Appends one byte.
    pub fn write_u8(&mut self, value: u8) {
        self.buf.push(value);
    }

    /// Appends a little-endian `u16`.
    pub fn write_u16_le(&mut self, value: u16) {
        self.write_bytes(&value.to_le_bytes());
    }

    /// Appends a big-endian `u16`.
    pub fn write_u16_be(&mut self, value: u16) {
        self.write_bytes(&value.to_be_bytes());
    }

    /// Appends a little-endian `u32`.
    pub fn write_u32_le(&mut self, value: u32) {
        self.write_bytes(&value.to_le_bytes());
    }

    /// Appends a big-endian `u32`.
    pub fn write_u32_be(&mut self, value: u32) {
        self.write_bytes(&value.to_be_bytes());
    }

    /// Appends a little-endian `u64`.
    pub fn write_u64_le(&mut self, value: u64) {
        self.write_bytes(&value.to_le_bytes());
    }

    /// Appends a big-endian `u64`.
    pub fn write_u64_be(&mut self, value: u64) {
        self.write_bytes(&value.to_be_bytes());
    }

    /// Appends `bytes`.
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }
}

/// Shows the length, never the contents.
impl fmt::Debug for ByteWriter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ByteWriter")
            .field("len", &self.buf.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Read = fn(&mut ByteReader<'_>) -> Result<(), CoreError>;

    /// Every fixed-width read, with the bytes it needs.
    const FIXED: [(&str, usize, Read); 13] = [
        ("read_u8", 1, |r| r.read_u8().map(drop)),
        ("read_u16_le", 2, |r| r.read_u16_le().map(drop)),
        ("read_u16_be", 2, |r| r.read_u16_be().map(drop)),
        ("read_u32_le", 4, |r| r.read_u32_le().map(drop)),
        ("read_u32_be", 4, |r| r.read_u32_be().map(drop)),
        ("read_u64_le", 8, |r| r.read_u64_le().map(drop)),
        ("read_u64_be", 8, |r| r.read_u64_be().map(drop)),
        ("read_array::<1>", 1, |r| r.read_array::<1>().map(drop)),
        ("read_array::<2>", 2, |r| r.read_array::<2>().map(drop)),
        ("read_array::<3>", 3, |r| r.read_array::<3>().map(drop)),
        ("read_array::<4>", 4, |r| r.read_array::<4>().map(drop)),
        ("read_array::<8>", 8, |r| r.read_array::<8>().map(drop)),
        ("read_array::<16>", 16, |r| r.read_array::<16>().map(drop)),
    ];

    const BASES: [u64; 3] = [0, 4096, u64::MAX.wrapping_sub(2)];

    /// A reader over `buf` at absolute offset `base`, advanced by `pos`.
    fn reader_at(buf: &[u8], base: u64, pos: usize) -> ByteReader<'_> {
        let mut r = ByteReader::with_base(buf, base);
        r.read_bytes(pos).unwrap();
        r
    }

    /// Asserts `result` is the exact end-of-input error, and that the
    /// reader did not move.
    fn assert_end<T: fmt::Debug>(
        what: &str,
        result: Result<T, CoreError>,
        r: &ByteReader<'_>,
        (base, pos): (u64, usize),
        needed: u64,
        available: usize,
    ) {
        let offset = base.saturating_add(to_u64(pos));
        assert_eq!(
            result.unwrap_err(),
            CoreError::UnexpectedEnd {
                offset,
                needed,
                available: to_u64(available),
            },
            "{what} at base {base}, position {pos}"
        );
        assert_eq!(r.position(), pos, "{what} moved");
        assert_eq!(r.offset(), offset, "{what} moved");
        assert_eq!(r.remaining(), available, "{what} moved");
    }

    #[test]
    fn every_read_past_the_end_is_an_error() {
        let data: Vec<u8> = (0..=20).collect();
        for len in 0..=data.len() {
            let buf = data.get(..len).unwrap();
            for base in BASES {
                for pos in 0..=len {
                    let at = (base, pos);
                    let left = len.checked_sub(pos).unwrap();
                    for (name, width, read) in FIXED {
                        if left < width {
                            let mut r = reader_at(buf, base, pos);
                            let res = read(&mut r);
                            assert_end(name, res, &r, at, to_u64(width), left);
                        }
                    }
                    for want in [
                        left.checked_add(1).unwrap(),
                        left.checked_add(7).unwrap(),
                        usize::MAX,
                    ] {
                        let mut r = reader_at(buf, base, pos);
                        let res = r.read_bytes(want);
                        assert_end("read_bytes", res, &r, at, to_u64(want), left);
                        let mut r = reader_at(buf, base, pos);
                        let res = r.sub_reader(want);
                        assert_end("sub_reader", res, &r, at, to_u64(want), left);
                        let mut r = reader_at(buf, base, pos);
                        let res = r.split_tail(want);
                        assert_end("split_tail", res, &r, at, to_u64(want), left);
                    }
                    let r = reader_at(buf, base, pos);
                    let count = to_u64(left).checked_add(1).unwrap();
                    let res = r.check_count(count, NonZeroUsize::MIN);
                    assert_end("check_count", res, &r, at, count, left);
                }
            }
        }
    }

    #[test]
    fn reads_with_exactly_enough_bytes_succeed() {
        let data = [0xA5u8; 16];
        for (name, width, read) in FIXED {
            let mut r = ByteReader::new(data.get(..width).unwrap());
            read(&mut r).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(r.finish(), Ok(()), "{name}");
        }
    }

    #[test]
    fn fixed_width_reads_decode_both_byte_orders() {
        let buf = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
        let mut r = ByteReader::new(&buf);
        assert_eq!(r.read_u8(), Ok(1));
        assert_eq!(r.read_u16_be(), Ok(0x0203));
        assert_eq!(r.read_u32_le(), Ok(0x0706_0504));
        assert_eq!(r.read_u64_be(), Ok(0x0809_0A0B_0C0D_0E0F));
        assert_eq!(r.position(), 15);
        let mut r = ByteReader::new(&buf);
        assert_eq!(r.read_u16_le(), Ok(0x0201));
        assert_eq!(r.read_u32_be(), Ok(0x0304_0506));
        assert_eq!(r.read_u64_le(), Ok(0x0E0D_0C0B_0A09_0807));
        assert_eq!(r.read_bytes(0), Ok(&[][..]));
        assert_eq!(r.read_bytes(1), Ok(&[15][..]));
        assert_eq!(r.finish(), Ok(()));
    }

    #[test]
    fn sub_reader_is_bounded_and_keeps_offsets() {
        let buf = [0xAA, 1, 2, 3, 0xBB];
        let mut r = ByteReader::with_base(&buf, 100);
        r.read_u8().unwrap();
        let mut sub = r.sub_reader(3).unwrap();
        assert_eq!((r.position(), r.offset()), (4, 104));
        assert_eq!((sub.position(), sub.offset()), (0, 101));
        assert_eq!(sub.read_bytes(3), Ok(&[1, 2, 3][..]));
        // The parent still has a byte; the sub-reader must not see it.
        assert_eq!(
            sub.read_u8(),
            Err(CoreError::UnexpectedEnd {
                offset: 104,
                needed: 1,
                available: 0
            })
        );
        assert_eq!(sub.finish(), Ok(()));
        assert_eq!(r.read_u8(), Ok(0xBB));
    }

    #[test]
    fn split_tail_takes_the_last_bytes() {
        let buf = [1, 2, 3, 4, 5, 6];
        let mut r = ByteReader::with_base(&buf, 1000);
        r.read_u8().unwrap();
        let mut tail = r.split_tail(2).unwrap();
        assert_eq!((tail.offset(), tail.remaining()), (1004, 2));
        assert_eq!(tail.read_u16_be(), Ok(0x0506));
        // The reader keeps its position and now ends before the tail.
        assert_eq!((r.position(), r.offset(), r.remaining()), (1, 1001, 3));
        assert_eq!(r.read_bytes(3), Ok(&[2, 3, 4][..]));
        assert_eq!(
            r.read_u8(),
            Err(CoreError::UnexpectedEnd {
                offset: 1004,
                needed: 1,
                available: 0
            })
        );
        let mut r = ByteReader::new(&buf);
        let all = r.split_tail(6).unwrap();
        assert_eq!((all.offset(), all.remaining(), r.remaining()), (0, 6, 0));
        assert_eq!(r.split_tail(0).map(|t| t.remaining()), Ok(0));
    }

    #[test]
    fn check_count_bounds_allocations_by_the_input() {
        let buf = [0u8; 12];
        let r = ByteReader::with_base(&buf, 8);
        let four = NonZeroUsize::new(4).unwrap();
        assert_eq!(r.check_count(3, four), Ok(3));
        assert_eq!(r.check_count(0, four), Ok(0));
        assert_eq!(
            r.check_count(4, four),
            Err(CoreError::UnexpectedEnd {
                offset: 8,
                needed: 16,
                available: 12
            })
        );
        // count × size overflows: needed saturates.
        assert_eq!(
            r.check_count(u64::MAX, four),
            Err(CoreError::UnexpectedEnd {
                offset: 8,
                needed: u64::MAX,
                available: 12
            })
        );
        assert_eq!(r.check_count(12, NonZeroUsize::MIN), Ok(12));
        assert_eq!(r.position(), 0);
    }

    #[test]
    fn finish_reports_trailing_bytes() {
        let buf = [1, 2, 3];
        let mut r = ByteReader::with_base(&buf, 40);
        r.read_u8().unwrap();
        assert_eq!(
            r.finish(),
            Err(CoreError::TrailingBytes {
                offset: 41,
                count: 2
            })
        );
        assert_eq!(ByteReader::new(&[]).finish(), Ok(()));
    }

    #[test]
    fn offsets_saturate() {
        let buf = [0u8; 8];
        let mut r = ByteReader::with_base(&buf, u64::MAX.wrapping_sub(2));
        r.read_bytes(5).unwrap();
        assert_eq!((r.offset(), r.position()), (u64::MAX, 5));
        let tail = r.split_tail(1).unwrap();
        assert_eq!(tail.offset(), u64::MAX);
    }

    #[test]
    fn debug_never_shows_contents() {
        let secret = *b"secret";
        let mut w = ByteWriter::new();
        w.write_bytes(&secret);
        let r = ByteReader::new(&secret);
        let shown = format!("{r:?} {w:?}");
        assert!(!shown.contains("115"), "{shown}"); // b's'
        assert_eq!(
            shown,
            "ByteReader { offset: 0, position: 0, remaining: 6 } ByteWriter { len: 6 }"
        );
    }

    #[test]
    fn writer_round_trips_through_reader() {
        let mut w = ByteWriter::new();
        assert!(w.is_empty());
        w.write_u8(0x05);
        w.write_u16_le(0xBEEF);
        w.write_u16_be(0xBEEF);
        w.write_u32_le(0xDEAD_BEEF);
        w.write_u32_be(0xDEAD_BEEF);
        w.write_u64_le(u64::MAX.wrapping_sub(1));
        w.write_u64_be(u64::MAX.wrapping_sub(1));
        w.write_bytes(b"SSST");
        assert_eq!(w.len(), 33);
        assert!(!w.is_empty());
        assert_eq!(w.as_slice().get(1..3), Some(&[0xEF, 0xBE][..]));
        let bytes = w.into_vec();
        let mut r = ByteReader::new(&bytes);
        assert_eq!(r.read_u8(), Ok(0x05));
        assert_eq!(r.read_u16_le(), Ok(0xBEEF));
        assert_eq!(r.read_u16_be(), Ok(0xBEEF));
        assert_eq!(r.read_u32_le(), Ok(0xDEAD_BEEF));
        assert_eq!(r.read_u32_be(), Ok(0xDEAD_BEEF));
        assert_eq!(r.read_u64_le(), Ok(u64::MAX.wrapping_sub(1)));
        assert_eq!(r.read_u64_be(), Ok(u64::MAX.wrapping_sub(1)));
        assert_eq!(r.read_array(), Ok(*b"SSST"));
        assert_eq!(r.finish(), Ok(()));
    }
}
