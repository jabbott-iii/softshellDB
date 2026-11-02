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

//! The public API as later decoders will use it, from outside the crate
//! and under the decoder lints: an SDOC string, an SST data block and a
//! WAL batch, each with the error pattern its crate will follow.

#![deny(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use std::error::Error;
use std::fmt;
use std::num::NonZeroUsize;

use softshell_core::bytes::{ByteReader, ByteWriter};
use softshell_core::error::{Chain, Context, CoreError, ErrorContext};
use softshell_core::limits::Limits;
use softshell_core::varint;

// ----- codec: an SDOC string -----

/// A codec error: each decode variant names its format.
#[derive(Debug, PartialEq, Eq)]
enum CodecError {
    Malformed {
        format: &'static str,
        error: Context<CoreError>,
    },
    InvalidUtf8 {
        format: &'static str,
        offset: u64,
    },
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed { format, .. } => write!(f, "malformed {format}"),
            Self::InvalidUtf8 { format, offset } => {
                write!(f, "invalid UTF-8 in {format} at offset {offset}")
            }
        }
    }
}

impl Error for CodecError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Malformed { error, .. } => Some(error),
            Self::InvalidUtf8 { .. } => None,
        }
    }
}

/// What the SDOC module defines once: the step and the format, in one
/// adapter per call.
fn sdoc(doing: &'static str) -> impl FnOnce(CoreError) -> CodecError {
    move |error| CodecError::Malformed {
        format: "SDOC",
        error: Context::new(doing, error),
    }
}

/// An SDOC String payload; the tag is already read.
fn decode_string(r: &mut ByteReader<'_>, limits: &Limits) -> Result<String, CodecError> {
    let len =
        varint::read_len(r, limits.max_string_bytes).map_err(sdoc("reading a string length"))?;
    let start = r.offset();
    let bytes = r.read_bytes(len).map_err(sdoc("reading a string"))?;
    let text = std::str::from_utf8(bytes).map_err(|e| CodecError::InvalidUtf8 {
        format: "SDOC",
        offset: start.saturating_add(u64::try_from(e.valid_up_to()).unwrap_or(u64::MAX)),
    })?;
    // The only allocation, after both checks.
    Ok(text.to_owned())
}

#[test]
fn sdoc_string_checks_the_limit_before_allocating() {
    // Tag 05, then 2^32 declared in a 10-byte input.
    let input = [0x05, 0x80, 0x80, 0x80, 0x80, 0x10, 0, 0, 0, 0];
    let mut r = ByteReader::new(&input);
    assert_eq!(r.read_u8(), Ok(0x05));
    let err = decode_string(&mut r, &Limits::default()).unwrap_err();
    assert_eq!(
        err,
        CodecError::Malformed {
            format: "SDOC",
            error: Context::new(
                "reading a string length",
                CoreError::LimitExceeded {
                    offset: 1,
                    max: 16_777_216,
                    found: 4_294_967_296,
                },
            ),
        }
    );
    assert_eq!(
        Chain::new(&err).to_string(),
        "malformed SDOC: while reading a string length: \
         size 4294967296 declared at offset 1 is over the limit of 16777216"
    );
    assert_eq!(r.position(), 1, "the failed read consumed nothing");
}

#[test]
fn sdoc_string_round_trips_and_reports_bad_utf8() {
    let mut w = ByteWriter::new();
    varint::write_len(&mut w, 5);
    w.write_bytes(b"hello");
    varint::write_len(&mut w, 3);
    w.write_bytes(&[b'o', 0xFF, b'k']);
    let bytes = w.into_vec();

    // A limit set from another crate, on the #[non_exhaustive] struct.
    let mut limits = Limits::default();
    limits.max_string_bytes = 5;
    let mut r = ByteReader::with_base(&bytes, 100);
    assert_eq!(decode_string(&mut r, &limits).as_deref(), Ok("hello"));
    assert_eq!(
        decode_string(&mut r, &limits),
        Err(CodecError::InvalidUtf8 {
            format: "SDOC",
            offset: 108,
        })
    );
}

// ----- storage: an SST data block -----

/// An SST block decoder's error: a core error with context, or a rule of
/// this format, in a variant of its own.
#[derive(Debug, PartialEq, Eq)]
enum BlockError {
    Malformed(Context<CoreError>),
    SharedPrefixTooLong {
        offset: u64,
        shared: u64,
        previous: usize,
    },
}

impl From<Context<CoreError>> for BlockError {
    fn from(error: Context<CoreError>) -> Self {
        Self::Malformed(error)
    }
}

impl fmt::Display for BlockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(_) => f.write_str("malformed SST data block"),
            Self::SharedPrefixTooLong {
                offset,
                shared,
                previous,
            } => write!(
                f,
                "SST entry at offset {offset} shares {shared} bytes with a {previous}-byte key"
            ),
        }
    }
}

impl Error for BlockError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Malformed(error) => Some(error),
            Self::SharedPrefixTooLong { .. } => None,
        }
    }
}

/// Splits a data block (its trailer already removed) into its entries and
/// its restart offsets. Offsets in errors are file offsets.
fn split_block(
    block: &[u8],
    file_offset: u64,
) -> Result<(ByteReader<'_>, ByteReader<'_>), BlockError> {
    let mut entries = ByteReader::with_base(block, file_offset);
    let count = entries
        .split_tail(4)
        .and_then(|mut tail| tail.read_u32_le())
        .context("reading the restart count")?;
    let len = usize::try_from(count)
        .ok()
        .and_then(|c| c.checked_mul(4))
        .unwrap_or(usize::MAX);
    let restarts = entries
        .split_tail(len)
        .context("reading the restart offsets")?;
    Ok((entries, restarts))
}

/// Reads one entry into `key` and returns its value.
fn read_entry<'a>(
    r: &mut ByteReader<'a>,
    key: &mut Vec<u8>,
    limits: &Limits,
) -> Result<&'a [u8], BlockError> {
    let at = r.offset();
    let shared = varint::read_u64(r).context("reading a shared prefix length")?;
    let shared = match usize::try_from(shared) {
        Ok(n) if n <= key.len() => n,
        _ => {
            return Err(BlockError::SharedPrefixTooLong {
                offset: at,
                shared,
                previous: key.len(),
            });
        }
    };
    // shared + unshared <= max_key_bytes, without an addition. The key and
    // the value follow both lengths, so the lengths are checked against
    // their limits only (read_at_most, not read_len), and read_bytes checks
    // the input where each one starts.
    let unshared = varint::read_at_most(r, limits.max_key_bytes.saturating_sub(shared))
        .context("reading a key length")?;
    let value_len = varint::read_at_most(r, usize::MAX).context("reading a value length")?;
    let suffix = r.read_bytes(unshared).context("reading a key")?;
    let value = r.read_bytes(value_len).context("reading a value")?;
    key.truncate(shared);
    key.extend_from_slice(suffix);
    Ok(value)
}

fn block(entries: &[(u64, &[u8], &[u8])], restarts: &[u32]) -> Vec<u8> {
    let mut w = ByteWriter::new();
    for &(shared, suffix, value) in entries {
        varint::write_u64(&mut w, shared);
        varint::write_len(&mut w, suffix.len());
        varint::write_len(&mut w, value.len());
        w.write_bytes(suffix);
        w.write_bytes(value);
    }
    for &offset in restarts {
        w.write_u32_le(offset);
    }
    w.write_u32_le(u32::try_from(restarts.len()).unwrap_or(u32::MAX));
    w.into_vec()
}

#[test]
fn sst_block_decodes_entries_with_file_offsets() {
    let limits = Limits::default();
    let bytes = block(&[(0, b"abc", b"v1"), (2, b"d", b"w")], &[0]);
    let (mut entries, mut restarts) = split_block(&bytes, 4096).unwrap();
    // Entries are 8 and 5 bytes; the restart array follows them.
    assert_eq!(entries.offset(), 4096);
    assert_eq!(restarts.offset(), 4109);
    assert_eq!(restarts.read_u32_le(), Ok(0));
    assert_eq!(restarts.finish(), Ok(()));
    let mut key = Vec::new();
    assert_eq!(read_entry(&mut entries, &mut key, &limits), Ok(&b"v1"[..]));
    assert_eq!(key, b"abc");
    assert_eq!(entries.offset(), 4104);
    assert_eq!(read_entry(&mut entries, &mut key, &limits), Ok(&b"w"[..]));
    assert_eq!(key, b"abd");
    assert_eq!(entries.offset(), 4109);
    assert_eq!(entries.finish(), Ok(()));
}

#[test]
fn sst_block_errors_name_file_offsets() {
    let limits = Limits::default();
    // A value that runs past the entries: named where the value starts
    // (after the key at 4099), with only its own bytes counted.
    let mut bytes = vec![0x00, 0x01, 0x09, b'k', b'v', b'v'];
    bytes.extend_from_slice(&0u32.to_le_bytes());
    let (mut entries, _) = split_block(&bytes, 4096).unwrap();
    let err = read_entry(&mut entries, &mut Vec::new(), &limits).unwrap_err();
    assert_eq!(
        Chain::new(&err).to_string(),
        "malformed SST data block: while reading a value: \
         unexpected end of input at offset 4100: needed 9 bytes, 2 left"
    );

    // A key over the limit: named at its length, before anything is read.
    let mut small = Limits::default();
    small.max_key_bytes = 2;
    let bytes = block(&[(0, b"abc", b"")], &[]);
    let (mut entries, _) = split_block(&bytes, 4096).unwrap();
    let err = read_entry(&mut entries, &mut Vec::new(), &small).unwrap_err();
    assert_eq!(
        Chain::new(&err).to_string(),
        "malformed SST data block: while reading a key length: \
         size 3 declared at offset 4097 is over the limit of 2"
    );

    // A shared prefix longer than the previous key: this format's own error.
    let bytes = block(&[(5, b"x", b"")], &[]);
    let (mut entries, _) = split_block(&bytes, 0).unwrap();
    assert_eq!(
        read_entry(&mut entries, &mut Vec::new(), &limits),
        Err(BlockError::SharedPrefixTooLong {
            offset: 0,
            shared: 5,
            previous: 0
        })
    );

    // A restart count of u32::MAX in an 8-byte block.
    let bytes = [0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF];
    let err = split_block(&bytes, 4096).unwrap_err();
    assert_eq!(
        Chain::new(&err).to_string(),
        "malformed SST data block: while reading the restart offsets: \
         unexpected end of input at offset 4096: needed 17179869180 bytes, 4 left"
    );
}

// ----- storage: a WAL batch payload -----

#[derive(Debug, PartialEq, Eq)]
enum Op<'a> {
    Put(&'a [u8], &'a [u8]),
    Delete(&'a [u8]),
}

/// A WAL batch payload: `sequence u64` LE, `count u32` LE, then per
/// operation `kind u8`, varint key length, key, and for puts varint value
/// length, value. (A real decoder rejects an unknown kind with a variant
/// of its own.)
fn decode_batch<'a>(
    r: &mut ByteReader<'a>,
    limits: &Limits,
) -> Result<(u64, Vec<Op<'a>>), Context<CoreError>> {
    // The smallest operation: a kind byte and an empty key's length.
    const MIN_OP_BYTES: NonZeroUsize = NonZeroUsize::new(2).expect("2 is not zero");
    let sequence = r.read_u64_le().context("reading the sequence")?;
    let count = r.read_u32_le().context("reading the count")?;
    let count = r
        .check_count(count.into(), MIN_OP_BYTES)
        .context("checking the count")?;
    let mut ops = Vec::with_capacity(count);
    for _ in 0..count {
        let kind = r.read_u8().context("reading an operation kind")?;
        let key_len = varint::read_len(r, limits.max_key_bytes).context("reading a key length")?;
        let key = r.read_bytes(key_len).context("reading a key")?;
        if kind == 0 {
            ops.push(Op::Delete(key));
        } else {
            let len = varint::read_len(r, usize::MAX).context("reading a value length")?;
            ops.push(Op::Put(key, r.read_bytes(len).context("reading a value")?));
        }
    }
    r.finish().context("ending the batch")?;
    Ok((sequence, ops))
}

#[test]
fn wal_batch_checks_its_count_before_allocating() {
    let limits = Limits::default();
    let mut w = ByteWriter::new();
    w.write_u64_le(42);
    w.write_u32_le(2);
    w.write_u8(1);
    varint::write_len(&mut w, 1);
    w.write_bytes(b"k");
    varint::write_len(&mut w, 2);
    w.write_bytes(b"v1");
    w.write_u8(0);
    varint::write_len(&mut w, 1);
    w.write_bytes(b"j");
    let bytes = w.into_vec();
    let mut r = ByteReader::new(&bytes);
    assert_eq!(
        decode_batch(&mut r, &limits),
        Ok((42, vec![Op::Put(b"k", b"v1"), Op::Delete(b"j")]))
    );

    // u32::MAX operations declared, 2 bytes present: no allocation.
    let mut w = ByteWriter::new();
    w.write_u64_le(1);
    w.write_u32_le(u32::MAX);
    w.write_bytes(&[0, 0]);
    let bytes = w.into_vec();
    let err = decode_batch(&mut ByteReader::new(&bytes), &limits).unwrap_err();
    assert_eq!(
        err.error(),
        &CoreError::UnexpectedEnd {
            offset: 12,
            needed: 8_589_934_590,
            available: 2
        }
    );

    // The kind under the context says what went wrong, not why: a short
    // batch and a corrupt count both give UnexpectedEnd. Telling a torn tail
    // from corruption is the record-framing layer's job (from position and
    // checksum); a batch decoder reports every failure as corruption.
    let short = bytes.get(..10).unwrap_or_default();
    let err = decode_batch(&mut ByteReader::new(short), &limits).unwrap_err();
    assert_eq!(err.doing(), "reading the count");
    assert!(matches!(err.error(), CoreError::UnexpectedEnd { .. }));

    // Bytes after the last operation.
    let mut w = ByteWriter::new();
    w.write_u64_le(1);
    w.write_u32_le(1);
    w.write_bytes(&[0, 0, 0, 0, 0]);
    let long = w.into_vec();
    let err = decode_batch(&mut ByteReader::new(&long), &limits).unwrap_err();
    assert_eq!(
        err,
        Context::new(
            "ending the batch",
            CoreError::TrailingBytes {
                offset: 14,
                count: 3
            }
        )
    );
}
