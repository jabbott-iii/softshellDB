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

//! Base64 with the standard alphabet and padding (RFC 4648 §4), as
//! extended JSON writes `Bytes`.
//!
//! [`decode`] is strict, so every byte string has exactly one text: the
//! length is a multiple of 4, `=` appears only as the last one or two
//! characters, nothing else (no line breaks, no whitespace) is allowed, and
//! the unused low bits before the padding must be zero (RFC 4648 §3.5).
//!
//! ```
//! use softshell_codec::base64;
//!
//! assert_eq!(base64::encode(b"foobar"), "Zm9vYmFy");
//! assert_eq!(base64::decode("Zm9vYg==").unwrap(), b"foob");
//! assert!(base64::decode("Zm9vYh==").is_err()); // non-zero unused bits
//! ```

#![deny(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use std::error::Error;
use std::fmt;

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Marks a byte that is not in the alphabet.
const INVALID: u8 = 0xFF;

/// Each byte's 6-bit value, or [`INVALID`].
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "evaluated at compile time, where a bad index fails the build"
)]
const DECODE: [u8; 256] = {
    let mut table = [INVALID; 256];
    let mut i = 0;
    while i < 64 {
        table[ALPHABET[i] as usize] = i as u8;
        i += 1;
    }
    table
};

/// The base64 text of `bytes`, padded with `=` to a multiple of 4.
#[must_use]
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3).saturating_mul(4));
    let (groups, rest) = bytes.as_chunks::<3>();
    for &group in groups {
        push_group(&mut out, group, 4);
    }
    match *rest {
        [a] => {
            push_group(&mut out, [a, 0, 0], 2);
            out.push_str("==");
        }
        [a, b] => {
            push_group(&mut out, [a, b, 0], 3);
            out.push('=');
        }
        _ => {}
    }
    out
}

/// Appends the first `symbols` characters encoding `group`.
fn push_group(out: &mut String, group: [u8; 3], symbols: usize) {
    let bits = u32::from(group[0]) << 16 | u32::from(group[1]) << 8 | u32::from(group[2]);
    for shift in [18, 12, 6, 0].into_iter().take(symbols) {
        // A 6-bit index is always inside the 64-entry alphabet.
        let index = (bits >> shift & 0x3F) as usize;
        out.push(char::from(ALPHABET.get(index).copied().unwrap_or(b'A')));
    }
}

/// The bytes that `text` encodes.
///
/// # Errors
///
/// If `text` is not the canonical padded base64 of some byte string (see
/// the module documentation).
pub fn decode(text: &str) -> Result<Vec<u8>, Base64Error> {
    let input = text.as_bytes();
    let (groups, rest) = input.as_chunks::<4>();
    if !rest.is_empty() {
        return Err(Base64Error::Length { len: input.len() });
    }
    // Three bytes per four symbols at most: bounded by the input.
    let mut out = Vec::with_capacity(groups.len().saturating_mul(3));
    for (index, group) in groups.iter().enumerate() {
        let start = index.saturating_mul(4);
        let last = index.saturating_add(1) == groups.len();
        let mut values = [0u8; 4];
        let mut padding: usize = 0;
        for (i, (&byte, value)) in group.iter().zip(&mut values).enumerate() {
            let offset = start.saturating_add(i);
            if byte == b'=' {
                // Padding is the last one or two symbols of the last group.
                if !last || i < 2 || (i == 2 && group.get(3) != Some(&b'=')) {
                    return Err(Base64Error::Padding { offset });
                }
                padding = padding.saturating_add(1);
                continue;
            }
            if padding > 0 {
                return Err(Base64Error::Padding { offset });
            }
            *value = match DECODE.get(usize::from(byte)) {
                Some(&v) if v != INVALID => v,
                _ => return Err(invalid_char(text, offset)),
            };
        }
        let bits = u32::from(values[0]) << 18
            | u32::from(values[1]) << 12
            | u32::from(values[2]) << 6
            | u32::from(values[3]);
        let [_, a, b, c] = bits.to_be_bytes();
        match padding {
            0 => out.extend_from_slice(&[a, b, c]),
            1 if c == 0 => out.extend_from_slice(&[a, b]),
            2 if b == 0 && c == 0 => out.push(a),
            // The last symbol before the padding has bits no byte uses.
            _ => {
                let symbol = 3usize.saturating_sub(padding);
                return Err(Base64Error::NonCanonical {
                    offset: start.saturating_add(symbol),
                });
            }
        }
    }
    Ok(out)
}

fn invalid_char(text: &str, offset: usize) -> Base64Error {
    let found = text
        .get(offset..)
        .and_then(|rest| rest.chars().next())
        .unwrap_or(char::REPLACEMENT_CHARACTER);
    Base64Error::InvalidChar { offset, found }
}

/// Why [`decode`] rejected a text. Offsets are byte offsets in the text.
///
/// New variants may be added in any release.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Base64Error {
    /// The length is not a multiple of 4.
    Length {
        /// The text's length in bytes.
        len: usize,
    },
    /// A character outside the alphabet.
    InvalidChar {
        /// Where it is.
        offset: usize,
        /// The character.
        found: char,
    },
    /// `=` somewhere other than the last one or two characters, or a
    /// symbol after it.
    Padding {
        /// Where the misplaced character is.
        offset: usize,
    },
    /// The last symbol before the padding has non-zero bits that no byte
    /// uses, so the text is not the canonical one.
    NonCanonical {
        /// Where that symbol is.
        offset: usize,
    },
}

impl fmt::Display for Base64Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Length { len } => {
                write!(f, "base64 length {len} is not a multiple of 4")
            }
            Self::InvalidChar { offset, found } => {
                write!(f, "{found:?} at byte {offset} is not a base64 character")
            }
            Self::Padding { offset } => write!(
                f,
                "misplaced base64 padding at byte {offset}: `=` may only end the text"
            ),
            Self::NonCanonical { offset } => write!(
                f,
                "base64 symbol at byte {offset} has unused bits set, so the text is not canonical"
            ),
        }
    }
}

/// A leaf error: it has no source.
impl Error for Base64Error {}

#[cfg(test)]
mod tests {
    #![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

    use super::*;

    /// RFC 4648 §10.
    const VECTORS: [(&str, &str); 7] = [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
    ];

    #[test]
    fn rfc_4648_vectors() {
        for (bytes, text) in VECTORS {
            assert_eq!(encode(bytes.as_bytes()), text);
            assert_eq!(decode(text).unwrap(), bytes.as_bytes(), "{text}");
        }
    }

    #[test]
    fn every_byte_value_round_trips() {
        let all: Vec<u8> = (0..=255).collect();
        for len in 0..all.len() {
            let text = encode(&all[..len]);
            assert_eq!(text.len(), len.div_ceil(3) * 4);
            assert_eq!(decode(&text).unwrap(), &all[..len]);
        }
        assert_eq!(
            encode(&all[..63]),
            "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8gISIjJCUmJygpKissLS4vMDEyMzQ1Njc4OTo7PD0+"
        );
        assert_eq!(encode(&[0xFB, 0xFF, 0xBF]), "+/+/");
    }

    #[test]
    fn rejects_what_is_not_canonical() {
        let cases: [(&str, Base64Error); 14] = [
            ("Zg=", Base64Error::Length { len: 3 }),
            ("Z", Base64Error::Length { len: 1 }),
            ("Zg", Base64Error::Length { len: 2 }),
            ("Zm9vY", Base64Error::Length { len: 5 }),
            ("Zm9v\nYg==", Base64Error::Length { len: 9 }),
            (
                "Zm 9",
                Base64Error::InvalidChar {
                    offset: 2,
                    found: ' ',
                },
            ),
            (
                "Zm-9",
                Base64Error::InvalidChar {
                    offset: 2,
                    found: '-',
                },
            ),
            (
                "Zm_9",
                Base64Error::InvalidChar {
                    offset: 2,
                    found: '_',
                },
            ),
            ("Zé==", Base64Error::Length { len: 5 }),
            (
                "Zmé",
                Base64Error::InvalidChar {
                    offset: 2,
                    found: 'é',
                },
            ),
            ("====", Base64Error::Padding { offset: 0 }),
            ("Z===", Base64Error::Padding { offset: 1 }),
            ("Zg==Zm9v", Base64Error::Padding { offset: 2 }),
            ("Zm=v", Base64Error::Padding { offset: 2 }),
        ];
        for (text, expected) in cases {
            assert_eq!(decode(text), Err(expected), "{text:?}");
        }
        // The unused bits must be zero: `Zh==` would also mean "f".
        assert_eq!(decode("Zh=="), Err(Base64Error::NonCanonical { offset: 1 }));
        assert_eq!(decode("Zm9="), Err(Base64Error::NonCanonical { offset: 2 }));
        assert_eq!(decode("Zm8="), Ok(b"fo".to_vec()));
        assert_eq!(decode("Zg==").unwrap(), b"f");
    }

    #[test]
    fn errors_say_what_and_where() {
        assert_eq!(
            Base64Error::Length { len: 3 }.to_string(),
            "base64 length 3 is not a multiple of 4"
        );
        assert_eq!(
            Base64Error::InvalidChar {
                offset: 2,
                found: '\n'
            }
            .to_string(),
            "'\\n' at byte 2 is not a base64 character"
        );
        assert_eq!(
            Base64Error::Padding { offset: 1 }.to_string(),
            "misplaced base64 padding at byte 1: `=` may only end the text"
        );
        assert_eq!(
            Base64Error::NonCanonical { offset: 1 }.to_string(),
            "base64 symbol at byte 1 has unused bits set, so the text is not canonical"
        );
    }
}
