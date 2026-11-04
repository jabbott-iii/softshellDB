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

//! The JSON writers. They recurse once per level of nesting.

use std::fmt::Write as _;

use softshell_core::civil;

use crate::base64;
use crate::value::{Object, Value};

pub(crate) fn write(value: &Value, pretty: bool) -> String {
    let mut writer = Writer {
        out: String::new(),
        pretty,
        depth: 0,
    };
    writer.value(value);
    writer.out
}

struct Writer {
    out: String,
    pretty: bool,
    /// Containers around the current position, for indentation.
    depth: usize,
}

impl Writer {
    fn value(&mut self, value: &Value) {
        match value {
            Value::Null => self.out.push_str("null"),
            Value::Bool(true) => self.out.push_str("true"),
            Value::Bool(false) => self.out.push_str("false"),
            Value::Int(i) => {
                // Writing to a `String` cannot fail.
                let _ = write!(self.out, "{i}");
            }
            Value::Float(f) => match float_name(*f) {
                None => push_float(&mut self.out, &format!("{f:e}")),
                Some(name) => self.extended("$float", |w| w.string(name)),
            },
            Value::String(s) => self.string(s),
            Value::Bytes(b) => self.extended("$bytes", |w| w.string(&base64::encode(b))),
            Value::Timestamp(t) => {
                self.extended("$ts", |w| w.string(&civil::format_rfc3339(*t)));
            }
            Value::Id(id) => self.extended("$id", |w| w.string(&id.to_string())),
            Value::Array(items) => self.array(items),
            Value::Object(object) => self.object(object),
            Value::F32Vec(elements) => self.extended("$f32vec", |w| w.f32_vec(elements)),
        }
    }

    /// `{"<key>": <body>}`, on one line.
    fn extended(&mut self, key: &str, body: impl FnOnce(&mut Self)) {
        self.out.push('{');
        self.string(key);
        self.out.push_str(if self.pretty { ": " } else { ":" });
        body(self);
        self.out.push('}');
    }

    fn f32_vec(&mut self, elements: &[f32]) {
        self.out.push('[');
        for (i, &x) in elements.iter().enumerate() {
            if i > 0 {
                self.out.push_str(if self.pretty { ", " } else { "," });
            }
            match float_name(f64::from(x)) {
                None => push_float(&mut self.out, &format!("{x:e}")),
                Some(name) => self.extended("$float", |w| w.string(name)),
            }
        }
        self.out.push(']');
    }

    /// In pretty output, a line break and the current indentation.
    fn new_line(&mut self) {
        if self.pretty {
            self.out.push('\n');
            for _ in 0..self.depth {
                self.out.push_str("  ");
            }
        }
    }

    fn array(&mut self, items: &[Value]) {
        if items.is_empty() {
            self.out.push_str("[]");
            return;
        }
        self.out.push('[');
        self.depth = self.depth.saturating_add(1);
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                self.out.push(',');
            }
            self.new_line();
            self.value(item);
        }
        self.depth = self.depth.saturating_sub(1);
        self.new_line();
        self.out.push(']');
    }

    fn object(&mut self, object: &Object) {
        if object.is_empty() {
            self.out.push_str("{}");
            return;
        }
        self.out.push('{');
        self.depth = self.depth.saturating_add(1);
        for (i, (key, value)) in object.iter().enumerate() {
            if i > 0 {
                self.out.push(',');
            }
            self.new_line();
            self.string(key);
            self.out.push_str(if self.pretty { ": " } else { ":" });
            self.value(value);
        }
        self.depth = self.depth.saturating_sub(1);
        self.new_line();
        self.out.push('}');
    }

    /// A string literal: `"`, `\` and control characters escaped, the rest
    /// as it is.
    fn string(&mut self, s: &str) {
        self.out.push('"');
        let mut run = 0;
        for (i, c) in s.char_indices() {
            let escape = match c {
                '"' => "\\\"",
                '\\' => "\\\\",
                '\n' => "\\n",
                '\r' => "\\r",
                '\t' => "\\t",
                '\u{8}' => "\\b",
                '\u{c}' => "\\f",
                '\0'..='\u{1f}' => "",
                _ => continue,
            };
            self.out.push_str(s.get(run..i).unwrap_or(""));
            if escape.is_empty() {
                let _ = write!(self.out, "\\u{:04x}", u32::from(c));
            } else {
                self.out.push_str(escape);
            }
            // Every escaped character is one byte long.
            run = i.saturating_add(1);
        }
        self.out.push_str(s.get(run..).unwrap_or(""));
        self.out.push('"');
    }
}

/// The `$float` name of a non-finite value.
fn float_name(f: f64) -> Option<&'static str> {
    if f.is_nan() {
        Some("NaN")
    } else if f == f64::INFINITY {
        Some("Infinity")
    } else if f == f64::NEG_INFINITY {
        Some("-Infinity")
    } else {
        None
    }
}

/// Writes a finite float given its `{:e}` text, which has the shortest
/// digits that round back to it: as a plain decimal with a `.` when the
/// exponent is -4 to 15 (`0.0001`, `5.0`, `123.25`), otherwise as that text
/// (`1e16`, `-1.5e-7`). Both forms read back as floats.
fn push_float(out: &mut String, exp_text: &str) {
    let (mantissa, exponent) = exp_text.split_once('e').unwrap_or((exp_text, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    if !(-4..16).contains(&exponent) {
        out.push_str(exp_text);
        return;
    }
    let (negative, mantissa) = match mantissa.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, mantissa),
    };
    // One digit before the point, the others after it.
    let (lead, rest) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if negative {
        out.push('-');
    }
    if let Ok(whole_digits) = usize::try_from(exponent) {
        let (whole, fraction) = rest
            .split_at_checked(whole_digits.min(rest.len()))
            .unwrap_or((rest, ""));
        out.push_str(lead);
        out.push_str(whole);
        for _ in whole.len()..whole_digits {
            out.push('0');
        }
        out.push('.');
        out.push_str(if fraction.is_empty() { "0" } else { fraction });
    } else {
        out.push_str("0.");
        for _ in 1..exponent.unsigned_abs() {
            out.push('0');
        }
        out.push_str(lead);
        out.push_str(rest);
    }
}
