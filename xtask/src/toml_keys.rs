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

//! Just enough TOML to know every key a file sets: its full dotted path,
//! whichever way it is spelled (table headers, arrays of tables, dotted
//! keys, quoted and escaped keys, inline tables), with the line it is on
//! and its value's text. The checks use it to find what cargo would read
//! in `Cargo.toml` and `.cargo/config.toml` (`[source]`, `[patch]`,
//! `rustflags`, `[lints]`), so that a spelling cannot hide a table.
//!
//! It is not a TOML parser: values are kept as text, and the file is
//! assumed valid, since cargo has already read it.

/// One key the file sets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Key {
    /// Its full path: the table's keys, then its own.
    pub(crate) path: Vec<String>,
    /// The line it is on, from 1.
    pub(crate) line: usize,
    /// Its value as written, trimmed; for a table header, empty.
    pub(crate) value: String,
}

/// Why a file could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TomlError {
    /// The line, from 1.
    pub(crate) line: usize,
    /// What is wrong.
    pub(crate) what: &'static str,
}

/// Every key `text` sets, table headers included, in order.
///
/// # Errors
///
/// [`TomlError`] for a key, string or bracket that never ends.
pub(crate) fn keys(text: &str) -> Result<Vec<Key>, TomlError> {
    // Cargo reads a file that starts with a byte-order mark.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut scanner = Scanner {
        chars: text.chars().collect(),
        at: 0,
        line: 1,
        out: Vec::new(),
    };
    scanner.document()?;
    Ok(scanner.out)
}

struct Scanner {
    chars: Vec<char>,
    at: usize,
    line: usize,
    out: Vec<Key>,
}

impl Scanner {
    fn peek(&self, ahead: usize) -> Option<char> {
        self.chars.get(self.at + ahead).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek(0)?;
        self.at += 1;
        if c == '\n' {
            self.line += 1;
        }
        Some(c)
    }

    fn error(&self, what: &'static str) -> TomlError {
        TomlError {
            line: self.line,
            what,
        }
    }

    /// Spaces and tabs on this line.
    fn spaces(&mut self) {
        while matches!(self.peek(0), Some(' ' | '\t')) {
            self.bump();
        }
    }

    /// Whitespace, newlines and comments.
    fn blank(&mut self) {
        loop {
            match self.peek(0) {
                Some(' ' | '\t' | '\r' | '\n') => {
                    self.bump();
                }
                Some('#') => {
                    while self.peek(0).is_some_and(|c| c != '\n') {
                        self.bump();
                    }
                }
                _ => return,
            }
        }
    }

    fn document(&mut self) -> Result<(), TomlError> {
        let mut table: Vec<String> = Vec::new();
        loop {
            self.blank();
            let Some(c) = self.peek(0) else {
                return Ok(());
            };
            let line = self.line;
            if c == '[' {
                self.bump();
                let array = self.peek(0) == Some('[');
                if array {
                    self.bump();
                }
                table = self.key_path(']')?;
                for _ in 0..=usize::from(array) {
                    if self.bump() != Some(']') {
                        return Err(self.error("table header"));
                    }
                }
                self.out.push(Key {
                    path: table.clone(),
                    line,
                    value: String::new(),
                });
            } else {
                self.key_value(&table)?;
            }
        }
    }

    /// `key.path = value`, under `prefix`.
    fn key_value(&mut self, prefix: &[String]) -> Result<(), TomlError> {
        let line = self.line;
        let mut path = prefix.to_vec();
        path.extend(self.key_path('=')?);
        if self.bump() != Some('=') {
            return Err(self.error("key"));
        }
        self.spaces();
        let start = self.at;
        let inline = self.peek(0) == Some('{');
        if inline {
            self.bump();
            loop {
                self.blank();
                match self.peek(0) {
                    Some('}') => {
                        self.bump();
                        break;
                    }
                    Some(',') => {
                        self.bump();
                    }
                    Some(_) => self.key_value(&path)?,
                    None => return Err(self.error("inline table")),
                }
            }
        } else {
            self.value()?;
        }
        let value: String = self
            .chars
            .get(start..self.at)
            .unwrap_or(&[])
            .iter()
            .collect();
        self.out.push(Key {
            path,
            line,
            value: value.trim().to_owned(),
        });
        Ok(())
    }

    /// A value that is not an inline table: up to the end of the line, or
    /// across lines inside a string or an array.
    fn value(&mut self) -> Result<(), TomlError> {
        let mut depth = 0usize;
        loop {
            match self.peek(0) {
                None => {
                    return if depth == 0 {
                        Ok(())
                    } else {
                        Err(self.error("array"))
                    };
                }
                Some('\n' | '#' | ',' | '}') if depth == 0 => return Ok(()),
                Some('"' | '\'') => {
                    self.string()?;
                }
                Some('#') => {
                    while self.peek(0).is_some_and(|c| c != '\n') {
                        self.bump();
                    }
                }
                Some('[' | '{') => {
                    depth += 1;
                    self.bump();
                }
                Some(']' | '}') => {
                    depth = depth.saturating_sub(1);
                    self.bump();
                }
                Some(_) => {
                    self.bump();
                }
            }
        }
    }

    /// A key path up to `end` (not consumed): bare or quoted parts
    /// separated by dots, with spaces around them.
    fn key_path(&mut self, end: char) -> Result<Vec<String>, TomlError> {
        let mut path = Vec::new();
        loop {
            self.spaces();
            let part = match self.peek(0) {
                Some('"' | '\'') => self.string()?,
                Some(c) if c.is_ascii_alphanumeric() || c == '_' || c == '-' => {
                    let mut part = String::new();
                    while let Some(c) = self
                        .peek(0)
                        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                    {
                        part.push(c);
                        self.bump();
                    }
                    part
                }
                _ => return Err(self.error("key")),
            };
            path.push(part);
            self.spaces();
            match self.peek(0) {
                Some('.') => {
                    self.bump();
                }
                Some(c) if c == end => return Ok(path),
                _ => return Err(self.error("key")),
            }
        }
    }

    /// A string at the cursor, of any of TOML's four kinds; its value.
    fn string(&mut self) -> Result<String, TomlError> {
        let quote = self.bump().ok_or_else(|| self.error("string"))?;
        let multiline = self.peek(0) == Some(quote) && self.peek(1) == Some(quote);
        if multiline {
            self.bump();
            self.bump();
        }
        let mut text = String::new();
        loop {
            let c = self.bump().ok_or_else(|| self.error("string"))?;
            if c == quote {
                if !multiline {
                    return Ok(text);
                }
                if self.peek(0) == Some(quote) && self.peek(1) == Some(quote) {
                    self.bump();
                    self.bump();
                    // Up to two more quotes may end the content.
                    while self.peek(0) == Some(quote) {
                        text.push(quote);
                        self.bump();
                    }
                    return Ok(text);
                }
                text.push(c);
            } else if c == '\n' && !multiline {
                return Err(self.error("string"));
            } else if c == '\\' && quote == '"' {
                self.escape(&mut text, multiline)?;
            } else {
                text.push(c);
            }
        }
    }

    /// The rest of an escape in a basic string, after `\`.
    fn escape(&mut self, text: &mut String, multiline: bool) -> Result<(), TomlError> {
        let c = self.bump().ok_or_else(|| self.error("string"))?;
        let simple = match c {
            'b' => Some('\u{8}'),
            't' => Some('\t'),
            'n' => Some('\n'),
            'f' => Some('\u{c}'),
            'r' => Some('\r'),
            'e' => Some('\u{1b}'),
            '"' => Some('"'),
            '\\' => Some('\\'),
            _ => None,
        };
        if let Some(simple) = simple {
            text.push(simple);
            return Ok(());
        }
        let digits = match c {
            'x' => 2,
            'u' => 4,
            'U' => 8,
            c if multiline && c.is_whitespace() => {
                // A line-ending backslash trims the whitespace after it.
                while self.peek(0).is_some_and(char::is_whitespace) {
                    self.bump();
                }
                return Ok(());
            }
            _ => return Err(self.error("string escape")),
        };
        let mut code = 0u32;
        for _ in 0..digits {
            let digit = self
                .bump()
                .and_then(|d| d.to_digit(16))
                .ok_or_else(|| self.error("string escape"))?;
            code = code * 16 + digit;
        }
        text.push(char::from_u32(code).ok_or_else(|| self.error("string escape"))?);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(text: &str) -> Vec<(String, usize)> {
        keys(text)
            .unwrap()
            .into_iter()
            .map(|k| (k.path.join("."), k.line))
            .collect()
    }

    #[test]
    fn every_spelling_of_a_key() {
        let text = r#"
top = 1
source.crates-io.replace-with = "v"   # a comment
"source" . 'x' . "\u0064irectory" = "vendor"
[patch.crates-io]
foo = { path = "x", features = ["a", "b"] }
[ "target" . 'x86_64-unknown-linux-gnu' ]
rustflags = [
  "--cfg", # inside an array
  "feature=\"dev\"",
]
[[bin]]
name = """
multi = line
[not.a.header]
"""
[lints]
workspace = true
"#;
        assert_eq!(
            paths(text),
            [
                ("top".to_owned(), 2),
                ("source.crates-io.replace-with".to_owned(), 3),
                ("source.x.directory".to_owned(), 4),
                ("patch.crates-io".to_owned(), 5),
                ("patch.crates-io.foo.path".to_owned(), 6),
                ("patch.crates-io.foo.features".to_owned(), 6),
                ("patch.crates-io.foo".to_owned(), 6),
                ("target.x86_64-unknown-linux-gnu".to_owned(), 7),
                ("target.x86_64-unknown-linux-gnu.rustflags".to_owned(), 8),
                ("bin".to_owned(), 12),
                ("bin.name".to_owned(), 13),
                ("lints".to_owned(), 17),
                ("lints.workspace".to_owned(), 18),
            ]
        );
        let found = keys(text).unwrap();
        assert_eq!(found.last().unwrap().value, "true");
        let crlf = text.replace('\n', "\r\n");
        assert_eq!(paths(&crlf), paths(text));
    }

    #[test]
    fn the_real_manifests_read() {
        for text in [
            include_str!("../../Cargo.toml"),
            include_str!("../../.cargo/config.toml"),
            include_str!("../Cargo.toml"),
        ] {
            assert!(keys(text).is_ok());
        }
        let root = paths(include_str!("../../Cargo.toml"));
        assert!(
            root.iter()
                .any(|(p, _)| p == "workspace.lints.rust.unsafe_code")
        );
    }

    #[test]
    fn a_byte_order_mark_is_skipped() {
        assert_eq!(
            paths("\u{feff}[lints]\nworkspace = true\n"),
            [("lints".to_owned(), 1), ("lints.workspace".to_owned(), 2)]
        );
    }

    #[test]
    fn unterminated_text_is_an_error() {
        for text in [
            "a = \"x",
            "[a",
            "a = [1, 2",
            "a = { b = 1",
            "\"a = 1",
            "a = \"\"\"x",
            "a = \"\\q\"",
        ] {
            assert!(keys(text).is_err(), "{text}");
        }
    }
}
