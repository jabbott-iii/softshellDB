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

//! The JSON and JSONC parser: recursive descent over the text's bytes.
//!
//! Text needs peeking and character boundaries, so this reads through its
//! own cursor rather than `ByteReader`: every byte is read with a checked
//! `get`, and every slice of the text with `str::get`. Positions only ever
//! sit after an ASCII byte or at the start, so they are character
//! boundaries.
//!
//! The elements of open arrays and the entries of open objects wait on two
//! stacks shared by the whole parse, and each container is moved into a
//! vector of exactly its length when it closes, so memory stays near the
//! size of the values themselves.

#![deny(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use softshell_core::civil;
use softshell_core::id::Id;
use softshell_core::limits::Limits;

use super::{JsonError, JsonErrorKind};
use crate::base64;
use crate::value::{self, Object, Value};

/// JSON, or JSON with comments and trailing commas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Dialect {
    Json,
    Jsonc,
}

impl Dialect {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Json => "JSON",
            Self::Jsonc => "JSONC",
        }
    }
}

/// The extended JSON keys.
#[derive(Clone, Copy)]
enum Extended {
    Id,
    Ts,
    Bytes,
    F32Vec,
    Float,
}

impl Extended {
    fn from_key(key: &str) -> Option<Self> {
        match key {
            "$id" => Some(Self::Id),
            "$ts" => Some(Self::Ts),
            "$bytes" => Some(Self::Bytes),
            "$f32vec" => Some(Self::F32Vec),
            "$float" => Some(Self::Float),
            _ => None,
        }
    }

    const fn key(self) -> &'static str {
        match self {
            Self::Id => "$id",
            Self::Ts => "$ts",
            Self::Bytes => "$bytes",
            Self::F32Vec => "$f32vec",
            Self::Float => "$float",
        }
    }
}

/// What a `$float` string means, if it is one of the three names.
fn float_named(name: &str) -> Option<f64> {
    match name {
        "NaN" => Some(f64::NAN),
        "Infinity" => Some(f64::INFINITY),
        "-Infinity" => Some(f64::NEG_INFINITY),
        _ => None,
    }
}

const FLOAT_NAMES: &str = "\"NaN\", \"Infinity\" or \"-Infinity\"";

/// Number tokens up to this long go to `from_str` as they are. A longer
/// one is first rewritten with its exponent folded in: `from_str` stops
/// adding an explicit exponent's digits once it reaches 65,536, so it
/// miscounts an exponent of 655,360 or more, which enough digits can
/// offset into range. A short token cannot offset it that far.
const SHORT_NUMBER: usize = 64;

/// Significant digits kept when rewriting a long number; a dropped
/// non-zero digit is remembered as a final `1`. A halfway point between
/// two `f64`s has at most 768 significant digits, so rounding is the same.
const KEPT_DIGITS: usize = 800;

pub(crate) fn parse(text: &str, limits: Limits, dialect: Dialect) -> Result<Value, JsonError> {
    let mut parser = Parser {
        text,
        pos: 0,
        limits,
        dialect,
        values: Vec::new(),
        entries: Vec::new(),
        key_offsets: Vec::new(),
    };
    if text.len() > limits.max_document_bytes {
        let kind = JsonErrorKind::TooLarge {
            max: limits.max_document_bytes,
            found: text.len(),
        };
        return Err(parser.error(0, kind));
    }
    if text.starts_with('\u{FEFF}') {
        return Err(parser.error(0, JsonErrorKind::ByteOrderMark));
    }
    parser.skip_space()?;
    let value = parser.value(0)?;
    parser.skip_space()?;
    if parser.pos < text.len() {
        return Err(parser.unexpected("the end of the text"));
    }
    Ok(value)
}

struct Parser<'a> {
    text: &'a str,
    pos: usize,
    limits: Limits,
    dialect: Dialect,
    /// The elements of the open arrays, outermost first.
    values: Vec<Value>,
    /// The entries of the open objects, outermost first.
    entries: Vec<(String, Value)>,
    /// Where each key in `entries` starts.
    key_offsets: Vec<usize>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.pos).copied()
    }

    fn peek_second(&self) -> Option<u8> {
        self.text
            .as_bytes()
            .get(self.pos.saturating_add(1))
            .copied()
    }

    /// Steps over one byte; only after `peek` returned it.
    fn bump(&mut self) {
        self.pos = self.pos.saturating_add(1);
    }

    #[cold]
    #[inline(never)]
    fn error(&self, offset: usize, kind: JsonErrorKind) -> JsonError {
        JsonError::new(self.text, offset, kind, self.dialect)
    }

    /// An error for whatever is at the current position.
    #[cold]
    #[inline(never)]
    fn unexpected(&self, expected: &'static str) -> JsonError {
        let kind = match self
            .text
            .get(self.pos..)
            .and_then(|rest| rest.chars().next())
        {
            None => JsonErrorKind::UnexpectedEnd { expected },
            Some(found) => JsonErrorKind::Unexpected { found, expected },
        };
        self.error(self.pos, kind)
    }

    /// Skips whitespace, and comments in JSONC.
    fn skip_space(&mut self) -> Result<(), JsonError> {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\n' | b'\r') => self.bump(),
                Some(b'/') if matches!(self.peek_second(), Some(b'/' | b'*')) => {
                    if self.dialect == Dialect::Json {
                        return Err(self.error(self.pos, JsonErrorKind::Comment));
                    }
                    self.skip_comment()?;
                }
                _ => return Ok(()),
            }
        }
    }

    /// Skips a `//` or `/*` comment that starts at the current position.
    fn skip_comment(&mut self) -> Result<(), JsonError> {
        let start = self.pos;
        let body = start.saturating_add(2);
        let rest = self.text.get(body..).unwrap_or("");
        let end = if self.peek_second() == Some(b'/') {
            // To the end of the line; the line break is whitespace.
            rest.find(['\n', '\r']).unwrap_or(rest.len())
        } else {
            match rest.find("*/") {
                Some(close) => close.saturating_add(2),
                None => return Err(self.error(start, JsonErrorKind::UnterminatedComment)),
            }
        };
        self.pos = body.saturating_add(end);
        Ok(())
    }

    /// One value; `depth` is how many arrays and objects contain it.
    fn value(&mut self, depth: usize) -> Result<Value, JsonError> {
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => self.string().map(Value::String),
            Some(b't') => self.literal(b"true", "`true`", Value::Bool(true)),
            Some(b'f') => self.literal(b"false", "`false`", Value::Bool(false)),
            Some(b'n') => self.literal(b"null", "`null`", Value::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(self.unexpected("a value")),
        }
    }

    fn literal(
        &mut self,
        word: &[u8],
        name: &'static str,
        value: Value,
    ) -> Result<Value, JsonError> {
        for &byte in word {
            if self.peek() != Some(byte) {
                return Err(self.unexpected(name));
            }
            self.bump();
        }
        Ok(value)
    }

    /// The depth of a container that starts at `at` inside `depth`
    /// others, if the limit allows it.
    fn enter(&self, depth: usize, at: usize) -> Result<usize, JsonError> {
        let inner = depth.saturating_add(1);
        if inner > self.limits.max_depth {
            let max = self.limits.max_depth;
            return Err(self.error(at, JsonErrorKind::TooDeep { max }));
        }
        Ok(inner)
    }

    /// After a `,` in an array or object: whether the closing `close`
    /// follows (a trailing comma, which only JSONC allows).
    fn trailing_comma(&mut self, comma: usize, close: u8) -> Result<bool, JsonError> {
        self.skip_space()?;
        if self.peek() != Some(close) {
            return Ok(false);
        }
        if self.dialect == Dialect::Json {
            return Err(self.error(comma, JsonErrorKind::TrailingComma));
        }
        self.bump();
        Ok(true)
    }

    fn array(&mut self, depth: usize) -> Result<Value, JsonError> {
        let depth = self.enter(depth, self.pos)?;
        self.bump();
        self.skip_space()?;
        if self.peek() == Some(b']') {
            self.bump();
            return Ok(Value::Array(Vec::new()));
        }
        let base = self.values.len();
        loop {
            if self.values.len().saturating_sub(base) >= self.limits.max_entries {
                let max = self.limits.max_entries;
                return Err(self.error(self.pos, JsonErrorKind::TooManyEntries { max }));
            }
            let item = self.value(depth)?;
            self.values.push(item);
            self.skip_space()?;
            match self.peek() {
                Some(b',') => {
                    let comma = self.pos;
                    self.bump();
                    if self.trailing_comma(comma, b']')? {
                        break;
                    }
                }
                Some(b']') => {
                    self.bump();
                    break;
                }
                _ => return Err(self.unexpected("`,` or `]`")),
            }
        }
        // Exactly as long as the array.
        Ok(Value::Array(self.values.drain(base..).collect()))
    }

    /// Skips the `:` after a key, and the space around it.
    fn colon(&mut self) -> Result<(), JsonError> {
        self.skip_space()?;
        if self.peek() != Some(b':') {
            return Err(self.unexpected("`:` after the key"));
        }
        self.bump();
        self.skip_space()
    }

    fn object(&mut self, depth: usize) -> Result<Value, JsonError> {
        let start = self.pos;
        self.bump();
        self.skip_space()?;
        match self.peek() {
            Some(b'}') => {
                self.enter(depth, start)?;
                self.bump();
                return Ok(Value::Object(Object::new()));
            }
            Some(b'"') => {}
            _ => return Err(self.unexpected("a string key or `}`")),
        }
        let mut key_at = self.pos;
        let mut key = self.string()?;
        if let Some(extended) = Extended::from_key(&key) {
            // An extended JSON form is a scalar: it adds no depth.
            return self.extended(extended);
        }
        let depth = self.enter(depth, start)?;
        let base = self.entries.len();
        loop {
            if self.entries.len().saturating_sub(base) >= self.limits.max_entries {
                let max = self.limits.max_entries;
                return Err(self.error(key_at, JsonErrorKind::TooManyEntries { max }));
            }
            self.colon()?;
            let value = self.value(depth)?;
            self.entries.push((key, value));
            self.key_offsets.push(key_at);
            self.skip_space()?;
            match self.peek() {
                Some(b',') => {
                    let comma = self.pos;
                    self.bump();
                    if self.trailing_comma(comma, b'}')? {
                        break;
                    }
                    if self.peek() != Some(b'"') {
                        return Err(self.unexpected("a string key"));
                    }
                }
                Some(b'}') => {
                    self.bump();
                    break;
                }
                _ => return Err(self.unexpected("`,` or `}`")),
            }
            key_at = self.pos;
            key = self.string()?;
            if let Some(extended) = Extended::from_key(&key) {
                let kind = JsonErrorKind::ExtendedJson {
                    key: extended.key(),
                    expected: "to be the only key of its object",
                };
                return Err(self.error(key_at, kind));
            }
        }
        let open = self.entries.get(base..).unwrap_or_default();
        if let Some(index) = value::first_duplicate(open) {
            return Err(self.duplicate_key(base.saturating_add(index)));
        }
        self.key_offsets.truncate(base);
        // Exactly as long as the object.
        let entries = self.entries.drain(base..).collect();
        Ok(Value::Object(Object::from_unique(entries)))
    }

    /// The error for the open object entry at `index`, whose key repeats
    /// an earlier one.
    #[cold]
    #[inline(never)]
    fn duplicate_key(&mut self, index: usize) -> JsonError {
        let at = self.key_offsets.get(index).copied().unwrap_or(0);
        let key = self
            .entries
            .get_mut(index)
            .map(|(k, _)| std::mem::take(k))
            .unwrap_or_default();
        self.error(at, JsonErrorKind::DuplicateKey { key })
    }

    /// The rest of an object whose first key was `extended`, through its
    /// `}`. Each form's value is read by its own reader, never by
    /// [`value`](Self::value), so wrappers cannot nest.
    #[inline(never)]
    fn extended(&mut self, extended: Extended) -> Result<Value, JsonError> {
        self.colon()?;
        let at = self.pos;
        let value = match extended {
            Extended::Id => {
                let text = self.extended_string(extended)?;
                let id = text
                    .parse::<Id>()
                    .map_err(|e| self.error(at, JsonErrorKind::InvalidId(e)))?;
                Value::Id(id)
            }
            Extended::Ts => {
                let text = self.extended_string(extended)?;
                let ts = civil::parse_rfc3339(&text)
                    .map_err(|e| self.error(at, JsonErrorKind::InvalidTimestamp(e)))?;
                Value::Timestamp(ts)
            }
            Extended::Bytes => {
                let text = self.extended_string(extended)?;
                let bytes = base64::decode(&text)
                    .map_err(|e| self.error(at, JsonErrorKind::InvalidBytes(e)))?;
                Value::Bytes(bytes)
            }
            Extended::Float => Value::Float(self.float_name()?),
            Extended::F32Vec => Value::F32Vec(self.f32_vec()?),
        };
        self.close_extended(extended)?;
        Ok(value)
    }

    /// The `}` that ends an extended JSON object, after an optional
    /// trailing comma in JSONC.
    fn close_extended(&mut self, extended: Extended) -> Result<(), JsonError> {
        self.skip_space()?;
        match self.peek() {
            Some(b'}') => {
                self.bump();
                Ok(())
            }
            Some(b',') => {
                let comma = self.pos;
                self.bump();
                if self.trailing_comma(comma, b'}')? {
                    return Ok(());
                }
                if self.peek() != Some(b'"') {
                    return Err(self.unexpected("`}`"));
                }
                let kind = JsonErrorKind::ExtendedJson {
                    key: extended.key(),
                    expected: "to be the only key of its object",
                };
                Err(self.error(self.pos, kind))
            }
            _ => Err(self.unexpected("`}`")),
        }
    }

    /// The string an extended JSON key needs.
    fn extended_string(&mut self, extended: Extended) -> Result<String, JsonError> {
        match self.peek() {
            Some(b'"') => self.string(),
            None => Err(self.unexpected("a string")),
            Some(_) => {
                let kind = JsonErrorKind::ExtendedJson {
                    key: extended.key(),
                    expected: "a string",
                };
                Err(self.error(self.pos, kind))
            }
        }
    }

    /// A `$float` value: one of the three names.
    fn float_name(&mut self) -> Result<f64, JsonError> {
        let at = self.pos;
        let name = self.extended_string(Extended::Float)?;
        float_named(&name).ok_or_else(|| {
            let kind = JsonErrorKind::ExtendedJson {
                key: Extended::Float.key(),
                expected: FLOAT_NAMES,
            };
            self.error(at, kind)
        })
    }

    /// An `$f32vec` array: numbers, and `$float` objects for non-finite
    /// elements.
    fn f32_vec(&mut self) -> Result<Vec<f32>, JsonError> {
        const EXPECTED: &str = "an array of numbers and `$float` objects";
        let not_vector = |parser: &Self| {
            let kind = JsonErrorKind::ExtendedJson {
                key: Extended::F32Vec.key(),
                expected: EXPECTED,
            };
            parser.error(parser.pos, kind)
        };
        match self.peek() {
            Some(b'[') => self.bump(),
            None => return Err(self.unexpected("`[`")),
            Some(_) => return Err(not_vector(self)),
        }
        self.skip_space()?;
        let mut elements = Vec::new();
        if self.peek() == Some(b']') {
            self.bump();
            return Ok(elements);
        }
        loop {
            if elements.len() >= self.limits.max_vector_dims {
                let max = self.limits.max_vector_dims;
                return Err(self.error(self.pos, JsonErrorKind::TooManyDimensions { max }));
            }
            let element = match self.peek() {
                Some(b'-' | b'0'..=b'9') => self.f32_number()?,
                Some(b'{') => self.f32_named()?,
                None => return Err(self.unexpected("a number or a `$float` object")),
                Some(_) => return Err(not_vector(self)),
            };
            elements.push(element);
            self.skip_space()?;
            match self.peek() {
                Some(b',') => {
                    let comma = self.pos;
                    self.bump();
                    if self.trailing_comma(comma, b']')? {
                        break;
                    }
                }
                Some(b']') => {
                    self.bump();
                    break;
                }
                _ => return Err(self.unexpected("`,` or `]`")),
            }
        }
        Ok(elements)
    }

    /// A `{"$float": ...}` element of an `$f32vec`.
    fn f32_named(&mut self) -> Result<f32, JsonError> {
        self.bump();
        self.skip_space()?;
        let key_at = self.pos;
        if self.peek().is_none() {
            return Err(self.unexpected("`\"$float\"`"));
        }
        let is_float = self.peek() == Some(b'"') && self.string()? == Extended::Float.key();
        if !is_float {
            let kind = JsonErrorKind::ExtendedJson {
                key: Extended::F32Vec.key(),
                expected: "numbers, or `$float` objects for non-finite elements",
            };
            return Err(self.error(key_at, kind));
        }
        self.colon()?;
        let value = self.float_name()?;
        self.close_extended(Extended::Float)?;
        // NaN and the infinities convert exactly.
        Ok(value as f32)
    }

    /// Steps over a number's text, checking RFC 8259's grammar. Returns
    /// where it starts and whether it is an integer (no fraction or
    /// exponent).
    fn scan_number(&mut self) -> Result<(usize, bool), JsonError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.bump();
        }
        match self.peek() {
            Some(b'0') => {
                self.bump();
                if matches!(self.peek(), Some(b'0'..=b'9')) {
                    return Err(self.error(start, JsonErrorKind::LeadingZero));
                }
            }
            Some(b'1'..=b'9') => self.digits(),
            _ => return Err(self.unexpected("a digit")),
        }
        let mut integer = true;
        if self.peek() == Some(b'.') {
            self.bump();
            integer = false;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.unexpected("a digit after `.`"));
            }
            self.digits();
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.bump();
            integer = false;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.bump();
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.unexpected("a digit in the exponent"));
            }
            self.digits();
        }
        Ok((start, integer))
    }

    fn digits(&mut self) {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.bump();
        }
    }

    /// The text from `start` to the current position.
    fn token(&self, start: usize) -> &str {
        self.text.get(start..self.pos).unwrap_or("")
    }

    #[inline(never)]
    fn number(&mut self) -> Result<Value, JsonError> {
        let (start, integer) = self.scan_number()?;
        let token = self.token(start);
        if integer && let Ok(i) = token.parse::<i64>() {
            return Ok(Value::Int(i));
        }
        match float_text(token, F64_EXPONENTS) {
            Some(text) => match text.parse::<f64>() {
                Ok(f) if f.is_finite() => Ok(Value::Float(f)),
                _ => Err(self.error(start, JsonErrorKind::NumberOutOfRange { ty: "f64" })),
            },
            None => Err(self.error(start, JsonErrorKind::NumberOutOfRange { ty: "f64" })),
        }
    }

    /// An `$f32vec` element, read straight from its text so that it is
    /// rounded once.
    fn f32_number(&mut self) -> Result<f32, JsonError> {
        let (start, _) = self.scan_number()?;
        match float_text(self.token(start), F32_EXPONENTS).map(|text| text.parse::<f32>()) {
            Some(Ok(f)) if f.is_finite() => Ok(f),
            _ => Err(self.error(start, JsonErrorKind::NumberOutOfRange { ty: "f32" })),
        }
    }

    /// A string, from its opening quote through its closing one.
    #[inline(never)]
    fn string(&mut self) -> Result<String, JsonError> {
        let start = self.pos;
        self.bump();
        let mut out = String::new();
        let mut run = self.pos;
        loop {
            match self.peek() {
                Some(b'"') => {
                    self.push_run(&mut out, run, start)?;
                    self.bump();
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.push_run(&mut out, run, start)?;
                    let c = self.escape()?;
                    if out.len().saturating_add(c.len_utf8()) > self.limits.max_string_bytes {
                        return Err(self.string_too_long(start));
                    }
                    out.push(c);
                    run = self.pos;
                }
                Some(byte @ 0x00..=0x1F) => {
                    let found = char::from(byte);
                    return Err(self.error(self.pos, JsonErrorKind::ControlCharacter { found }));
                }
                // UTF-8 continuation bytes are never `"` or `\`.
                Some(_) => self.bump(),
                None => return Err(self.unexpected("`\"` to end the string")),
            }
        }
    }

    fn string_too_long(&self, start: usize) -> JsonError {
        let max = self.limits.max_string_bytes;
        self.error(start, JsonErrorKind::StringTooLong { max })
    }

    /// Appends the unescaped text from `run` to the current position.
    fn push_run(&self, out: &mut String, run: usize, start: usize) -> Result<(), JsonError> {
        let chunk = self.text.get(run..self.pos).unwrap_or("");
        if out.len().saturating_add(chunk.len()) > self.limits.max_string_bytes {
            return Err(self.string_too_long(start));
        }
        out.push_str(chunk);
        Ok(())
    }

    /// The character a `\` escape at the current position stands for.
    fn escape(&mut self) -> Result<char, JsonError> {
        let at = self.pos;
        self.bump();
        let c = match self.peek() {
            Some(b'"') => '"',
            Some(b'\\') => '\\',
            Some(b'/') => '/',
            Some(b'b') => '\u{8}',
            Some(b'f') => '\u{c}',
            Some(b'n') => '\n',
            Some(b'r') => '\r',
            Some(b't') => '\t',
            Some(b'u') => {
                self.bump();
                return self.unicode_escape(at);
            }
            Some(_) => {
                let found = self
                    .text
                    .get(self.pos..)
                    .and_then(|rest| rest.chars().next())
                    .unwrap_or(char::REPLACEMENT_CHARACTER);
                return Err(self.error(at, JsonErrorKind::InvalidEscape { found }));
            }
            None => return Err(self.unexpected("an escape character")),
        };
        self.bump();
        Ok(c)
    }

    /// The rest of a `\u` escape that starts at `at`, with the low half
    /// of a surrogate pair if it needs one.
    fn unicode_escape(&mut self, at: usize) -> Result<char, JsonError> {
        let code = self.hex4()?;
        let lone = |parser: &Self| parser.error(at, JsonErrorKind::LoneSurrogate { code });
        match code {
            0xD800..=0xDBFF => {
                if self.peek() != Some(b'\\') || self.peek_second() != Some(b'u') {
                    return Err(lone(self));
                }
                self.bump();
                self.bump();
                let low = self.hex4()?;
                match char::decode_utf16([code, low]).next() {
                    Some(Ok(c)) => Ok(c),
                    _ => Err(lone(self)),
                }
            }
            _ => char::from_u32(u32::from(code)).ok_or_else(|| lone(self)),
        }
    }

    fn hex4(&mut self) -> Result<u16, JsonError> {
        let mut code: u16 = 0;
        for _ in 0..4 {
            let digit = self.peek().and_then(|b| char::from(b).to_digit(16));
            let Some(digit) = digit else {
                return Err(self.unexpected("a hex digit"));
            };
            // Four hex digits fill exactly 16 bits.
            code = code << 4 | digit as u16;
            self.bump();
        }
        Ok(code)
    }
}

/// The decimal exponents (of the first significant digit) beyond which a
/// float type certainly overflows, or certainly rounds to zero.
#[derive(Clone, Copy)]
struct Exponents {
    overflow: i64,
    underflow: i64,
}

const F64_EXPONENTS: Exponents = Exponents {
    overflow: 310,
    underflow: -400,
};

const F32_EXPONENTS: Exponents = Exponents {
    overflow: 40,
    underflow: -100,
};

/// Text that `from_str` reads as the same float as the number `token`
/// (already checked against the grammar), or `None` if it certainly
/// overflows. A short token is returned as it is; a long one is rewritten
/// as `d.ddd…e<exponent>`, with at most [`KEPT_DIGITS`] digits and a small
/// exponent.
fn float_text(token: &str, range: Exponents) -> Option<std::borrow::Cow<'_, str>> {
    use std::borrow::Cow;

    if token.len() <= SHORT_NUMBER {
        return Some(Cow::Borrowed(token));
    }
    let (negative, unsigned) = match token.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, token),
    };
    let (mantissa, exponent) = unsigned.split_once(['e', 'E']).unwrap_or((unsigned, "0"));
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = whole.bytes().chain(fraction.bytes());
    let leading_zeros = digits.clone().take_while(|&d| d == b'0').count();
    let significant: Vec<u8> = digits.skip(leading_zeros).collect();
    let significant = significant
        .iter()
        .rposition(|&d| d != b'0')
        .and_then(|last| significant.get(..=last))
        .unwrap_or_default();
    let sign = if negative { "-" } else { "" };
    if significant.is_empty() {
        return Some(Cow::Owned(format!("{sign}0")));
    }
    // The first significant digit's place: 10^(whole digits - leading
    // zeros - 1 + exponent).
    let place = i64::try_from(whole.len())
        .unwrap_or(i64::MAX)
        .saturating_sub(i64::try_from(leading_zeros).unwrap_or(i64::MAX))
        .saturating_sub(1)
        .saturating_add(saturating_exponent(exponent));
    if place >= range.overflow {
        return None;
    }
    if place <= range.underflow {
        return Some(Cow::Owned(format!("{sign}0")));
    }
    let mut text = String::with_capacity(KEPT_DIGITS.saturating_add(32));
    text.push_str(sign);
    let (first, rest) = significant.split_first().unwrap_or((&b'0', &[]));
    text.push(char::from(*first));
    text.push('.');
    let kept = rest.get(..KEPT_DIGITS).unwrap_or(rest);
    text.extend(kept.iter().map(|&d| char::from(d)));
    // `significant` ends in a non-zero digit, so a cut drops one.
    if kept.len() < rest.len() {
        text.push('1');
    }
    text.push('e');
    text.push_str(&place.to_string());
    Some(Cow::Owned(text))
}

/// An exponent's digits (with an optional sign) as an `i64`, saturating
/// far beyond any float's range.
fn saturating_exponent(text: &str) -> i64 {
    const CAP: i64 = 1 << 40;
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, text.get(1..).unwrap_or("")),
        Some(b'+') => (false, text.get(1..).unwrap_or("")),
        _ => (false, text),
    };
    let magnitude = digits.bytes().fold(0i64, |acc, d| {
        acc.saturating_mul(10)
            .saturating_add(i64::from(d.wrapping_sub(b'0')))
            .min(CAP)
    });
    if negative {
        magnitude.saturating_neg()
    } else {
        magnitude
    }
}
