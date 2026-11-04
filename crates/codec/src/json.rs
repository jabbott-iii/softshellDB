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

//! JSON text (RFC 8259) to and from [`Value`], with extended JSON for the
//! types JSON lacks. [`jsonc`](crate::jsonc) reads the same text with
//! comments and trailing commas.
//!
//! **Numbers:** one without a fraction or exponent that fits an `i64` is an
//! `Int`; any other is a `Float`, correctly rounded. One outside the `f64`
//! range is an error; one too small for it rounds to zero.
//!
//! **Extended JSON**, in both directions:
//!
//! | Value | JSON form |
//! | :- | :- |
//! | `Id` | `{"$id": "01ARZ3NDEKTSV4RRFFQ69G5FAV"}` (26 characters, either case on input) |
//! | `Timestamp` | `{"$ts": "2026-11-01T12:00:00.000000001Z"}` (RFC 3339; any offset and 0 to 9 fraction digits on input) |
//! | `Bytes` | `{"$bytes": "AQID"}` (base64, RFC 4648, padded) |
//! | `F32Vec` | `{"$f32vec": [0.1, 0.2]}` (a non-finite element as a `$float` object) |
//! | Non-finite `Float` | `{"$float": "NaN"}`, `"Infinity"` or `"-Infinity"` |
//!
//! An object whose first key is one of these five must have no other key,
//! and one of them anywhere else in an object is an error. Other keys that
//! start with `$`, such as query operators, are ordinary keys. So an
//! [`Object`](crate::value::Object) that itself has one of the five keys
//! has no JSON form: the writers write it, but it does not read back.
//!
//! **Writers** give one text per value, which reads back as an
//! [identical](Value::identical) value (NaN's sign and payload aside):
//! floats in the shortest form that rounds back to the same bits, always
//! with a `.` or an exponent (`5.0`, `0.0001`, `1e16`, `1.5e-7`); strings
//! with `"`, `\` and control characters escaped (`\n`, `\u001f`) and
//! everything else as UTF-8.
//!
//! ```
//! use softshell_codec::json;
//! use softshell_codec::value::Value;
//! use softshell_core::limits::Limits;
//!
//! let v = json::parse(r#"{"n": 1, "x": 1.0, "id": {"$id": "01ARZ3NDEKTSV4RRFFQ69G5FAV"}}"#, Limits::default())
//!     .unwrap();
//! let object = v.as_object().unwrap();
//! assert!(object.get("n").unwrap().identical(&Value::Int(1)));
//! assert!(object.get("x").unwrap().identical(&Value::Float(1.0)));
//! assert_eq!(
//!     json::to_string_compact(&v),
//!     r#"{"n":1,"x":1.0,"id":{"$id":"01ARZ3NDEKTSV4RRFFQ69G5FAV"}}"#
//! );
//!
//! let err = json::parse("[1,\n 2,]", Limits::default()).unwrap_err();
//! assert_eq!(err.to_string(), "JSON at line 2, column 3: trailing commas are allowed only in JSONC");
//! ```

mod parse;
#[cfg(test)]
mod tests;
mod write;

use std::error::Error;
use std::fmt;

use softshell_core::civil::CivilError;
use softshell_core::id::ParseIdError;
use softshell_core::limits::Limits;

use crate::base64::Base64Error;
use crate::value::Value;

pub(crate) use parse::Dialect;

/// Parses `text` as one JSON value (RFC 8259), with extended JSON.
///
/// Every size is checked against `limits` before it is allocated: the
/// text against `max_document_bytes`, each string and key once unescaped
/// against `max_string_bytes`, each array and object against
/// `max_entries`, each `$f32vec` against `max_vector_dims`, and the
/// nesting of arrays and objects against `max_depth` (extended JSON forms
/// are scalars and add none).
///
/// - **Stack:** the parser, and every operation that walks a value
///   (`Clone`, `Drop`, `Ord`, `Hash`, `Debug`, the writers), recurses once
///   per level. A `max_depth` up to 256 is tested on a 2 MiB thread stack
///   in a debug build; the default is 64.
/// - **Memory:** a value takes more memory than its text: at most about
///   25 times as much at peak (398 MiB, with glibc, for 16 MiB of nested
///   one-element arrays such as `[[[[[[[[0]]]]]]]]`).
/// - The text limit applies to the text. The same document as SDOC can be
///   several times smaller, so importing documents stored at the SDOC
///   limit needs a larger `max_document_bytes` here.
///
/// # Errors
///
/// A [`JsonError`] with the line and column of a problem. Problems are
/// found in text order, with two exceptions. A repeated key is found when
/// its object closes, so an error later inside that object, or a repeat
/// inside an object nested in it, is reported first. And an object too
/// deep is found once its first key is read (that key tells an extended
/// JSON form, which adds no depth, from an object), so a malformed first
/// key is reported instead.
pub fn parse(text: &str, limits: Limits) -> Result<Value, JsonError> {
    parse::parse(text, limits, Dialect::Json)
}

pub(crate) fn parse_dialect(
    text: &str,
    limits: Limits,
    dialect: Dialect,
) -> Result<Value, JsonError> {
    parse::parse(text, limits, dialect)
}

/// `value` as JSON with no whitespace.
#[must_use]
pub fn to_string_compact(value: &Value) -> String {
    write::write(value, false)
}

/// `value` as JSON indented by two spaces per level, with each element,
/// entry and closing bracket of a non-empty array or object on its own
/// line, and no newline at the end. Extended JSON forms stay on one line.
/// Deep nesting makes the text much longer than the compact form: a value
/// nested `d` deep pays about `2 × d` bytes of indentation on each of its
/// lines, so a chain of containers grows with the square of its depth.
#[must_use]
pub fn to_string_pretty(value: &Value) -> String {
    write::write(value, true)
}

impl fmt::Display for Value {
    /// The value as compact JSON, as [`to_string_compact`] writes it.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&to_string_compact(self))
    }
}

/// Why JSON or JSONC text was rejected, and where.
///
/// Its `Display` includes at most one character of the text (what was
/// found where something else was expected), escaped, so it is safe to
/// log. `$id`, `$ts` and `$bytes` errors point at the string's opening
/// quote; their [`source`](Error::source) counts offsets from the start
/// of the unescaped string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonError {
    kind: JsonErrorKind,
    offset: usize,
    line: usize,
    column: usize,
    dialect: Dialect,
}

impl JsonError {
    pub(crate) fn new(text: &str, offset: usize, kind: JsonErrorKind, dialect: Dialect) -> Self {
        let (line, column) = line_and_column(text, offset);
        Self {
            kind,
            offset,
            line,
            column,
            dialect,
        }
    }

    /// What was wrong.
    #[must_use]
    pub const fn kind(&self) -> &JsonErrorKind {
        &self.kind
    }

    /// The byte offset in the text where the problem starts.
    #[must_use]
    pub const fn offset(&self) -> usize {
        self.offset
    }

    /// The line, from 1. `\n`, `\r\n` and a lone `\r` each end a line.
    #[must_use]
    pub const fn line(&self) -> usize {
        self.line
    }

    /// The column, from 1, counted in characters (Unicode scalar values).
    #[must_use]
    pub const fn column(&self) -> usize {
        self.column
    }

    /// The format being read: `"JSON"` or `"JSONC"`.
    #[must_use]
    pub const fn format(&self) -> &'static str {
        self.dialect.name()
    }
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at line {}, column {}: {}",
            self.format(),
            self.line,
            self.column,
            self.kind
        )
    }
}

impl Error for JsonError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            JsonErrorKind::InvalidId(e) => Some(e),
            JsonErrorKind::InvalidTimestamp(e) => Some(e),
            JsonErrorKind::InvalidBytes(e) => Some(e),
            _ => None,
        }
    }
}

/// The 1-based line and column of byte `offset` in `text`.
fn line_and_column(text: &str, offset: usize) -> (usize, usize) {
    let (mut line, mut column) = (1usize, 1usize);
    let mut chars = text
        .char_indices()
        .take_while(|&(i, _)| i < offset)
        .peekable();
    while let Some((_, c)) = chars.next() {
        let ends_line = match c {
            '\n' => true,
            // `\r\n` ends one line, at its `\n`.
            '\r' => !matches!(chars.peek(), Some((_, '\n'))),
            _ => false,
        };
        if ends_line {
            line = line.saturating_add(1);
            column = 1;
        } else {
            column = column.saturating_add(1);
        }
    }
    (line, column)
}

/// What was wrong with JSON or JSONC text.
///
/// New variants may be added in any release.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum JsonErrorKind {
    /// The text ended where `expected` should have been.
    UnexpectedEnd {
        /// What should have come next, such as "a value" or "`,` or `]`".
        expected: &'static str,
    },
    /// `found` is where `expected` should have been.
    Unexpected {
        /// The character found.
        found: char,
        /// What should have come instead.
        expected: &'static str,
    },
    /// The text starts with a byte order mark (U+FEFF), which RFC 8259
    /// does not allow.
    ByteOrderMark,
    /// A number starts with `0` followed by another digit, such as `01`.
    LeadingZero,
    /// A trailing comma in JSON; only JSONC allows them.
    TrailingComma,
    /// A comment in JSON; only JSONC allows them.
    Comment,
    /// A `/*` comment with no `*/`.
    UnterminatedComment,
    /// A control character (U+0000 to U+001F) inside a string, where it
    /// must be escaped.
    ControlCharacter {
        /// The character.
        found: char,
    },
    /// A backslash in a string followed by something that is not an
    /// escape.
    InvalidEscape {
        /// What follows the backslash.
        found: char,
    },
    /// A `\u` escape of half a UTF-16 surrogate pair, without the other
    /// half.
    LoneSurrogate {
        /// The escaped code unit.
        code: u16,
    },
    /// A number outside the range of its type.
    NumberOutOfRange {
        /// `"f64"`, or `"f32"` in an `$f32vec`.
        ty: &'static str,
    },
    /// An object key that an earlier key of the same object already is.
    DuplicateKey {
        /// The key.
        key: String,
    },
    /// Arrays and objects nested deeper than [`Limits::max_depth`].
    TooDeep {
        /// The limit.
        max: usize,
    },
    /// The text is longer than [`Limits::max_document_bytes`].
    TooLarge {
        /// The limit.
        max: usize,
        /// The text's length in bytes.
        found: usize,
    },
    /// A string or key longer than [`Limits::max_string_bytes`] once
    /// unescaped.
    StringTooLong {
        /// The limit.
        max: usize,
    },
    /// An array or object with more than [`Limits::max_entries`] entries.
    TooManyEntries {
        /// The limit.
        max: usize,
    },
    /// An `$f32vec` with more than [`Limits::max_vector_dims`] elements.
    TooManyDimensions {
        /// The limit.
        max: usize,
    },
    /// An extended JSON key with the wrong kind of value, or not alone in
    /// its object.
    ExtendedJson {
        /// The key: `"$id"`, `"$ts"`, `"$bytes"`, `"$f32vec"` or
        /// `"$float"`.
        key: &'static str,
        /// What it needs.
        expected: &'static str,
    },
    /// An `$id` string that is not an id; the error is its source.
    InvalidId(ParseIdError),
    /// A `$ts` string that is not an RFC 3339 time in the `Timestamp`
    /// range; the error is its source.
    InvalidTimestamp(CivilError),
    /// A `$bytes` string that is not canonical base64; the error is its
    /// source.
    InvalidBytes(Base64Error),
}

impl fmt::Display for JsonErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEnd { expected } => {
                write!(f, "expected {expected}, found the end of the text")
            }
            Self::Unexpected { found, expected } => {
                write!(f, "expected {expected}, found {found:?}")
            }
            Self::ByteOrderMark => f.write_str("the text starts with a byte order mark (U+FEFF)"),
            Self::LeadingZero => f.write_str("a number cannot start with 0 followed by a digit"),
            Self::TrailingComma => f.write_str("trailing commas are allowed only in JSONC"),
            Self::Comment => f.write_str("comments are allowed only in JSONC"),
            Self::UnterminatedComment => f.write_str("a `/*` comment is never closed"),
            Self::ControlCharacter { found } => write!(
                f,
                "control character U+{:04X} must be escaped in a string",
                u32::from(*found)
            ),
            Self::InvalidEscape { found } => {
                write!(f, "a backslash followed by {found:?} is not an escape")
            }
            Self::LoneSurrogate { code } => write!(
                f,
                "\\u{code:04X} is half of a UTF-16 surrogate pair, without the other half"
            ),
            Self::NumberOutOfRange { ty } => write!(f, "the number is outside the range of {ty}"),
            Self::DuplicateKey { .. } => {
                f.write_str("a key repeats an earlier key of the same object")
            }
            Self::TooDeep { max } => {
                write!(f, "arrays and objects are nested more than {max} deep")
            }
            Self::TooLarge { max, found } => {
                write!(f, "the text is {found} bytes, over the limit of {max}")
            }
            Self::StringTooLong { max } => {
                write!(f, "a string is longer than the limit of {max} bytes")
            }
            Self::TooManyEntries { max } => {
                write!(f, "an array or object has more than {max} entries")
            }
            Self::TooManyDimensions { max } => {
                write!(f, "an `$f32vec` has more than {max} elements")
            }
            Self::ExtendedJson { key, expected } => write!(f, "`{key}` needs {expected}"),
            Self::InvalidId(_) => f.write_str("invalid `$id`"),
            Self::InvalidTimestamp(_) => f.write_str("invalid `$ts`"),
            Self::InvalidBytes(_) => f.write_str("invalid `$bytes`"),
        }
    }
}
