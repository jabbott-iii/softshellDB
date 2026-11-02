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

//! Errors: [`CoreError`] for this crate's checked reads; [`ErrorContext`]
//! and [`Context`], which record what was being done when an error from
//! any crate happened; and [`Chain`], which prints an error with its
//! causes.
//!
//! Display convention (every softshellDB error follows it): an error's
//! `Display` describes its own layer only, and [`Error::source`] returns
//! the cause. Logs and the CLI print the whole chain with [`Chain`]:
//!
//! ```
//! use softshell_core::bytes::ByteReader;
//! use softshell_core::error::{Chain, ErrorContext};
//! use softshell_core::varint;
//!
//! let mut r = ByteReader::new(&[0x80, 0x00]);
//! let err = varint::read_u64(&mut r).context("reading a key length").unwrap_err();
//! assert_eq!(err.to_string(), "while reading a key length");
//! assert_eq!(
//!     Chain::new(&err).to_string(),
//!     "while reading a key length: varint at offset 0 is not minimal: it ends in a zero byte",
//! );
//! ```
//!
//! A crate's own error enum holds a `Context<CoreError>` in a variant that
//! also names the format being decoded (`Malformed { format, error }`, say),
//! so the message carries the format, the offset, what was expected and
//! what was found.

use std::error::Error;
use std::fmt;

/// The most causes [`Chain`] prints before it stops.
const MAX_CAUSES: usize = 32;

/// An error from a [`ByteReader`](crate::bytes::ByteReader) read or a
/// [`varint`](crate::varint) decode.
///
/// Offsets are absolute: a reader made with
/// [`ByteReader::with_base`](crate::bytes::ByteReader::with_base) counts
/// from its base (a block's offset in its file, say), one made with
/// [`ByteReader::new`](crate::bytes::ByteReader::new) from 0, and readers
/// split from either keep their parent's offsets. Offsets and sizes
/// saturate at `u64::MAX`.
///
/// New variants may be added in any release; the fields of an existing
/// variant do not change.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CoreError {
    /// The input ended before a read, or before the data a declared size
    /// promises, was complete. It does not tell truncated input from a
    /// corrupt size: both look the same here.
    UnexpectedEnd {
        /// Where the missing bytes would begin: the read's first byte, a
        /// varint's first byte, the byte after a size checked by
        /// [`read_len`](crate::varint::read_len),
        /// [`read_count`](crate::varint::read_count) or
        /// [`check_count`](crate::bytes::ByteReader::check_count), or, for
        /// [`split_tail`](crate::bytes::ByteReader::split_tail), the
        /// reader's current offset.
        offset: u64,
        /// Bytes needed from `offset`. A lower bound for a truncated varint
        /// (the bytes present plus one) and for a checked count
        /// (`count × min_item_bytes`, saturating).
        needed: u64,
        /// Bytes left at `offset`.
        available: u64,
    },
    /// Bytes were left over where the input should have ended.
    TrailingBytes {
        /// Where the unread bytes begin.
        offset: u64,
        /// How many bytes were left.
        count: u64,
    },
    /// A varint's last allowed byte (the 5th for 32 bits, the 10th for 64)
    /// says more bytes follow.
    VarintOverlong {
        /// Where the varint begins.
        offset: u64,
        /// The width being decoded: 32 or 64.
        bits: u32,
    },
    /// A varint ended in a zero byte after the first, so a shorter encoding
    /// of the same value exists.
    VarintNonMinimal {
        /// Where the varint begins.
        offset: u64,
    },
    /// A varint's last allowed byte held bits beyond its type's width.
    VarintOverflow {
        /// Where the varint begins.
        offset: u64,
        /// The width being decoded: 32 or 64.
        bits: u32,
    },
    /// A declared length or count was larger than its limit.
    LimitExceeded {
        /// Where the declared size begins.
        offset: u64,
        /// The limit the caller passed, usually a
        /// [`Limits`](crate::limits::Limits) field.
        max: u64,
        /// The declared size.
        found: u64,
    },
}

impl CoreError {
    /// The byte offset the error refers to, if it has one.
    #[must_use]
    pub const fn offset(&self) -> Option<u64> {
        match *self {
            Self::UnexpectedEnd { offset, .. }
            | Self::TrailingBytes { offset, .. }
            | Self::VarintOverlong { offset, .. }
            | Self::VarintNonMinimal { offset }
            | Self::VarintOverflow { offset, .. }
            | Self::LimitExceeded { offset, .. } => Some(offset),
        }
    }
}

/// `"s"` unless `n` is 1.
const fn plural(n: u64) -> &'static str {
    if n == 1 { "" } else { "s" }
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::UnexpectedEnd {
                offset,
                needed,
                available,
            } => write!(
                f,
                "unexpected end of input at offset {offset}: needed {needed} byte{}, {available} left",
                plural(needed)
            ),
            Self::TrailingBytes { offset, count } => write!(
                f,
                "{count} unexpected trailing byte{} at offset {offset}",
                plural(count)
            ),
            Self::VarintOverlong { offset, bits } => write!(
                f,
                "varint at offset {offset} is longer than {} bytes, the most a {bits}-bit value takes",
                bits.div_ceil(7)
            ),
            Self::VarintNonMinimal { offset } => write!(
                f,
                "varint at offset {offset} is not minimal: it ends in a zero byte"
            ),
            Self::VarintOverflow { offset, bits } => {
                write!(f, "varint at offset {offset} does not fit in {bits} bits")
            }
            Self::LimitExceeded { offset, max, found } => write!(
                f,
                "size {found} declared at offset {offset} is over the limit of {max}"
            ),
        }
    }
}

/// A leaf error: it has no source.
impl Error for CoreError {}

/// An error together with what was being done when it happened. Made by
/// [`ErrorContext::context`] on a `Result`, or by [`Context::new`] for a
/// bare error.
///
/// `Display` prints `while <doing>`; [`Error::source`] returns the error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context<E> {
    doing: &'static str,
    error: E,
}

impl<E> Context<E> {
    /// Wraps `error`. `doing` completes the phrase "while …", such as
    /// "reading an SDOC string length". It is static text, so it never
    /// carries a user's keys or values into a log.
    #[must_use]
    pub const fn new(doing: &'static str, error: E) -> Self {
        Self { doing, error }
    }

    /// What was being done, as a phrase completing "while …".
    #[must_use]
    pub const fn doing(&self) -> &'static str {
        self.doing
    }

    /// The error that interrupted it. Match on this to classify the
    /// failure (to turn a [`CoreError::LimitExceeded`] into the caller's own
    /// limit error, say).
    #[must_use]
    pub const fn error(&self) -> &E {
        &self.error
    }

    /// Unwraps the error, dropping the context.
    #[must_use]
    pub fn into_error(self) -> E {
        self.error
    }
}

impl<E> fmt::Display for Context<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "while {}", self.doing)
    }
}

impl<E: Error + 'static> Error for Context<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.error)
    }
}

/// Adds "while doing X" context to the error of any `Result`, whatever
/// crate its error type comes from.
///
/// ```
/// use softshell_core::bytes::ByteReader;
/// use softshell_core::error::{CoreError, ErrorContext};
///
/// let mut r = ByteReader::new(&[]);
/// let err = r.read_u8().context("reading the tag").unwrap_err();
/// assert_eq!(err.doing(), "reading the tag");
/// assert!(matches!(err.error(), CoreError::UnexpectedEnd { offset: 0, .. }));
/// ```
pub trait ErrorContext<T, E> {
    /// Wraps the error, if any, in a [`Context`] naming what was being
    /// done. `doing` completes the phrase "while …".
    fn context(self, doing: &'static str) -> Result<T, Context<E>>;
}

impl<T, E> ErrorContext<T, E> for Result<T, E> {
    fn context(self, doing: &'static str) -> Result<T, Context<E>> {
        self.map_err(|error| Context { doing, error })
    }
}

/// Displays an error followed by each of its causes, separated by `": "`:
/// the form logs and the CLI print.
///
/// It prints at most 32 causes, then `": …"`, so even an error whose
/// `source` chain never ends is printed in bounded time.
#[derive(Debug, Clone, Copy)]
pub struct Chain<'a> {
    error: &'a (dyn Error + 'a),
}

impl<'a> Chain<'a> {
    /// Prepares `error` and its causes for display.
    #[must_use]
    pub const fn new(error: &'a (dyn Error + 'a)) -> Self {
        Self { error }
    }
}

impl fmt::Display for Chain<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.error)?;
        let mut cause = self.error.source();
        for _ in 0..MAX_CAUSES {
            let Some(error) = cause else {
                return Ok(());
            };
            write!(f, ": {error}")?;
            cause = error.source();
        }
        if cause.is_some() {
            f.write_str(": …")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_variant() -> [CoreError; 6] {
        [
            CoreError::UnexpectedEnd {
                offset: 1,
                needed: 2,
                available: 0,
            },
            CoreError::TrailingBytes {
                offset: 1,
                count: 3,
            },
            CoreError::VarintOverlong {
                offset: 1,
                bits: 32,
            },
            CoreError::VarintNonMinimal { offset: 1 },
            CoreError::VarintOverflow {
                offset: 1,
                bits: 64,
            },
            CoreError::LimitExceeded {
                offset: 1,
                max: 8,
                found: 9,
            },
        ]
    }

    #[test]
    fn offset_is_reported_for_every_variant() {
        for e in every_variant() {
            assert_eq!(e.offset(), Some(1));
            assert!(e.to_string().contains("offset 1"), "{e}");
            assert!(e.source().is_none());
        }
    }

    #[test]
    fn display_is_pinned_for_every_variant() {
        let shown: Vec<String> = every_variant().iter().map(ToString::to_string).collect();
        assert_eq!(
            shown,
            [
                "unexpected end of input at offset 1: needed 2 bytes, 0 left",
                "3 unexpected trailing bytes at offset 1",
                "varint at offset 1 is longer than 5 bytes, the most a 32-bit value takes",
                "varint at offset 1 is not minimal: it ends in a zero byte",
                "varint at offset 1 does not fit in 64 bits",
                "size 9 declared at offset 1 is over the limit of 8",
            ]
        );
        let one = CoreError::UnexpectedEnd {
            offset: 7,
            needed: 1,
            available: 0,
        };
        assert_eq!(
            one.to_string(),
            "unexpected end of input at offset 7: needed 1 byte, 0 left"
        );
        let one = CoreError::TrailingBytes {
            offset: 7,
            count: 1,
        };
        assert_eq!(one.to_string(), "1 unexpected trailing byte at offset 7");
        let long = CoreError::VarintOverlong {
            offset: 4,
            bits: 64,
        };
        assert!(long.to_string().contains("longer than 10 bytes"));
    }

    #[test]
    fn context_wraps_and_chains() {
        let r: Result<(), CoreError> = Err(CoreError::VarintNonMinimal { offset: 7 });
        let e = r.context("reading a key length").unwrap_err();
        assert_eq!(e.to_string(), "while reading a key length");
        assert_eq!(e.doing(), "reading a key length");
        assert_eq!(e.error(), &CoreError::VarintNonMinimal { offset: 7 });
        let source = e.source().map(ToString::to_string);
        assert_eq!(
            source.as_deref(),
            Some("varint at offset 7 is not minimal: it ends in a zero byte")
        );
        assert_eq!(
            e,
            Context::new(
                "reading a key length",
                CoreError::VarintNonMinimal { offset: 7 }
            )
        );
        assert_eq!(e.into_error(), CoreError::VarintNonMinimal { offset: 7 });

        let ok: Result<u8, CoreError> = Ok(3);
        assert_eq!(ok.context("unused"), Ok(3));
    }

    #[test]
    fn chain_prints_every_cause() {
        let inner = Context::new(
            "reading a value length",
            CoreError::VarintNonMinimal { offset: 9 },
        );
        let outer = Context::new("reading SST block 3", inner);
        assert_eq!(
            Chain::new(&outer).to_string(),
            "while reading SST block 3: while reading a value length: \
             varint at offset 9 is not minimal: it ends in a zero byte"
        );
        let leaf = CoreError::VarintNonMinimal { offset: 9 };
        assert_eq!(Chain::new(&leaf).to_string(), leaf.to_string());
    }

    /// An error whose source is itself: a chain that never ends.
    #[derive(Debug)]
    struct Loop;

    impl fmt::Display for Loop {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("loop")
        }
    }

    impl Error for Loop {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            Some(&Loop)
        }
    }

    #[test]
    fn chain_is_bounded() {
        let shown = Chain::new(&Loop).to_string();
        assert_eq!(shown.matches("loop").count(), MAX_CAUSES.wrapping_add(1));
        assert!(shown.ends_with(": …"), "{shown}");
    }

    /// An error with a finite chain of causes.
    #[derive(Debug)]
    struct Link(Option<Box<Link>>);

    impl fmt::Display for Link {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("link")
        }
    }

    impl Error for Link {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            self.0.as_deref().map(|l| l as &(dyn Error + 'static))
        }
    }

    /// A `Link` with `causes` causes below it.
    fn links(causes: usize) -> Link {
        (0..causes).fold(Link(None), |inner, _| Link(Some(Box::new(inner))))
    }

    #[test]
    fn chain_marks_only_causes_it_did_not_print() {
        let exact = Chain::new(&links(MAX_CAUSES)).to_string();
        assert_eq!(exact.matches("link").count(), MAX_CAUSES + 1);
        assert!(!exact.contains('…'), "{exact}");
        let over = Chain::new(&links(MAX_CAUSES + 1)).to_string();
        assert_eq!(over.matches("link").count(), MAX_CAUSES + 1);
        assert!(over.ends_with(": …"), "{over}");
    }

    /// `Chain` takes an error borrowed for any lifetime, as log and CLI
    /// helpers receive it.
    fn show(error: &dyn Error) -> String {
        Chain::new(error).to_string()
    }

    #[test]
    fn chain_accepts_a_borrowed_error() {
        let e = Context::new("reading a tag", CoreError::VarintNonMinimal { offset: 2 });
        assert_eq!(
            show(&e),
            "while reading a tag: varint at offset 2 is not minimal: it ends in a zero byte"
        );
    }

    /// A ceiling, not the exact size (32 bytes today): a later variant may
    /// own a `String` and an offset, but every crate error that wraps a
    /// `Context<CoreError>` must stay well under clippy's 128-byte
    /// `result_large_err` threshold.
    #[test]
    fn core_error_stays_small() {
        assert!(size_of::<CoreError>() <= 48, "{}", size_of::<CoreError>());
        assert!(size_of::<Context<CoreError>>() <= 64);
    }
}
