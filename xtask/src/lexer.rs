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

//! Just enough of a Rust lexer for `unsafe-audit`: it finds identifiers,
//! punctuation and comments, with their lines, and steps over every kind of
//! literal, so that `unsafe` in a string or a comment is not code.
//!
//! It handles line and nested block comments; string, byte-string and
//! C-string literals with escapes; raw strings with any number of `#`s;
//! char and byte literals, told apart from lifetimes and labels; raw
//! identifiers (`r#unsafe` is an identifier, not the keyword); the
//! non-ASCII characters Rust takes as whitespace (U+0085, U+200E, U+200F,
//! U+2028, U+2029), which end an identifier however invisible they are; a
//! leading byte-order mark; and a shebang line, told from an inner
//! attribute as rustc tells them. It does not check that the code is valid
//! Rust: the compiler does that.

/// What a token is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// An identifier or keyword, such as `unsafe` or `allow`.
    Ident,
    /// A raw identifier, such as `r#unsafe`: never a keyword.
    RawIdent,
    /// A lifetime or label, such as `'a`.
    Lifetime,
    /// A literal: a number, string, char or byte.
    Literal,
    /// One punctuation character, such as `{` or `#`.
    Punct,
}

/// A token, with the line it starts on (from 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Token<'a> {
    /// What it is.
    pub(crate) kind: Kind,
    /// Its text.
    pub(crate) text: &'a str,
    /// The line it starts on, from 1.
    pub(crate) line: usize,
    /// Its byte offset in the source.
    pub(crate) offset: usize,
}

/// A comment, doc comments included, with the lines it covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Comment<'a> {
    /// Its text, from `//` or `/*` to its end.
    pub(crate) text: &'a str,
    /// The line it starts on, from 1.
    pub(crate) line: usize,
    /// The line it ends on.
    pub(crate) end_line: usize,
    /// Its byte offset in the source.
    pub(crate) offset: usize,
}

/// A source file's tokens and comments, in order.
#[derive(Debug, Default)]
pub(crate) struct Lexed<'a> {
    /// The tokens.
    pub(crate) tokens: Vec<Token<'a>>,
    /// The comments.
    pub(crate) comments: Vec<Comment<'a>>,
}

/// Why a file could not be lexed: a literal or comment that never ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LexError {
    /// The line it starts on.
    pub(crate) line: usize,
    /// What never ends, such as "block comment".
    pub(crate) what: &'static str,
}

/// Splits `source` into tokens and comments.
///
/// # Errors
///
/// [`LexError`] for a comment, string or char literal that is not closed.
pub(crate) fn lex(source: &str) -> Result<Lexed<'_>, LexError> {
    // rustc skips a byte-order mark, then a shebang.
    let start = if source.starts_with('\u{feff}') { 3 } else { 0 };
    let rest = source.get(start..).unwrap_or("");
    let start = start + shebang_len(rest).unwrap_or(0);
    let mut lexer = Lexer::new(source, start);
    lexer.run()?;
    Ok(lexer.out)
}

/// The length of the shebang line `source` starts with, if it does: `#!`
/// not followed by `[` (after whitespace and comments that are not doc
/// comments), which would make it an inner attribute. This is rustc's rule.
pub(crate) fn shebang_len(source: &str) -> Option<usize> {
    let tail = source.strip_prefix("#!")?;
    let line = tail.split('\n').next().unwrap_or(tail);
    let mut lexer = Lexer::new(tail, 0);
    if lexer.run().is_err() {
        return Some(2 + line.len());
    }
    let first_token = lexer.out.tokens.first().map(|t| (t.offset, t.text));
    let first_doc = lexer
        .out
        .comments
        .iter()
        .find(|c| is_doc_comment(c.text))
        .map(|c| c.offset);
    let attribute = match (first_token, first_doc) {
        (Some((at, "[")), Some(doc)) => at < doc,
        (Some((_, "[")), None) => true,
        _ => false,
    };
    (!attribute).then_some(2 + line.len())
}

/// Whether `text`, a comment, is a doc comment: `///` (not `////`),
/// `//!`, `/**` (not `/***` or `/**/`) or `/*!`.
pub(crate) fn is_doc_comment(text: &str) -> bool {
    (text.starts_with("///") && !text.starts_with("////"))
        || text.starts_with("//!")
        || (text.starts_with("/**") && !text.starts_with("/***") && text != "/**/")
        || text.starts_with("/*!")
}

struct Lexer<'a> {
    source: &'a str,
    bytes: &'a [u8],
    at: usize,
    line: usize,
    out: Lexed<'a>,
}

impl<'a> Lexer<'a> {
    const fn new(source: &'a str, at: usize) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            at,
            line: 1,
            out: Lexed {
                tokens: Vec::new(),
                comments: Vec::new(),
            },
        }
    }

    fn peek(&self, ahead: usize) -> Option<u8> {
        self.bytes.get(self.at + ahead).copied()
    }

    /// Moves past `n` bytes, counting the newlines among them.
    fn advance(&mut self, n: usize) {
        let end = (self.at + n).min(self.bytes.len());
        let newlines = self
            .bytes
            .get(self.at..end)
            .map_or(0, |b| b.iter().filter(|&&c| c == b'\n').count());
        self.line += newlines;
        self.at = end;
    }

    fn text(&self, start: usize) -> &'a str {
        self.source.get(start..self.at).unwrap_or("")
    }

    fn run(&mut self) -> Result<(), LexError> {
        while let Some(b) = self.peek(0) {
            let start = self.at;
            let line = self.line;
            if let Some(len) = self.space_len() {
                self.advance(len);
                continue;
            }
            match b {
                b'/' if self.peek(1) == Some(b'/') => {
                    let len = self.source.get(start..).unwrap_or("").find('\n');
                    self.advance(len.unwrap_or(self.bytes.len() - start));
                    self.comment(start, line);
                }
                b'/' if self.peek(1) == Some(b'*') => self.block_comment(start, line)?,
                b'"' => {
                    self.advance(1);
                    self.quoted(b'"', "string literal", line)?;
                    self.token(Kind::Literal, start, line);
                }
                b'\'' => self.quote(start, line)?,
                b'0'..=b'9' => {
                    self.advance(1);
                    self.word();
                    self.token(Kind::Literal, start, line);
                }
                b if is_word_start(b) => self.word_or_prefixed(start, line)?,
                _ => {
                    let len = self.source.get(start..).unwrap_or("").chars().next();
                    self.advance(len.map_or(1, char::len_utf8));
                    self.token(Kind::Punct, start, line);
                }
            }
        }
        Ok(())
    }

    fn token(&mut self, kind: Kind, start: usize, line: usize) {
        let text = self.text(start);
        self.out.tokens.push(Token {
            kind,
            text,
            line,
            offset: start,
        });
    }

    fn comment(&mut self, start: usize, line: usize) {
        let text = self.text(start);
        self.out.comments.push(Comment {
            text,
            line,
            end_line: self.line,
            offset: start,
        });
    }

    /// A `/* … */` comment; they nest.
    fn block_comment(&mut self, start: usize, line: usize) -> Result<(), LexError> {
        self.advance(2);
        let mut depth = 1usize;
        while depth > 0 {
            match (self.peek(0), self.peek(1)) {
                (None, _) => {
                    return Err(LexError {
                        line,
                        what: "block comment",
                    });
                }
                (Some(b'/'), Some(b'*')) => {
                    depth += 1;
                    self.advance(2);
                }
                (Some(b'*'), Some(b'/')) => {
                    depth -= 1;
                    self.advance(2);
                }
                _ => self.advance(1),
            }
        }
        self.comment(start, line);
        Ok(())
    }

    /// The rest of a quoted literal, after its opening quote: up to the
    /// unescaped `close`.
    fn quoted(&mut self, close: u8, what: &'static str, line: usize) -> Result<(), LexError> {
        loop {
            match self.peek(0) {
                None => return Err(LexError { line, what }),
                Some(b'\\') => self.advance(2),
                Some(b) if b == close => {
                    self.advance(1);
                    return Ok(());
                }
                Some(_) => self.advance(1),
            }
        }
    }

    /// The rest of a raw string, after `r`: `#`s, `"`, the text, `"` and
    /// as many `#`s.
    fn raw_string(&mut self, line: usize) -> Result<(), LexError> {
        let mut hashes = 0;
        while self.peek(0) == Some(b'#') {
            hashes += 1;
            self.advance(1);
        }
        // The caller checked that a `"` follows the `#`s.
        self.advance(1);
        let what = "raw string literal";
        loop {
            match self.peek(0) {
                None => return Err(LexError { line, what }),
                Some(b'"') if (1..=hashes).all(|i| self.peek(i) == Some(b'#')) => {
                    self.advance(1 + hashes);
                    return Ok(());
                }
                Some(_) => self.advance(1),
            }
        }
    }

    /// After `'`: a char literal (`'a'`, `'\n'`, `'é'`) or a lifetime or
    /// label (`'a`, `'static`).
    fn quote(&mut self, start: usize, line: usize) -> Result<(), LexError> {
        self.advance(1);
        if self.peek(0) == Some(b'\\') {
            self.advance(2);
            self.quoted(b'\'', "char literal", line)?;
            self.token(Kind::Literal, start, line);
            return Ok(());
        }
        let next = self.source.get(self.at..).unwrap_or("").chars().next();
        let Some(next) = next else {
            self.token(Kind::Punct, start, line);
            return Ok(());
        };
        if self.peek(next.len_utf8()) == Some(b'\'') {
            self.advance(next.len_utf8() + 1);
            self.token(Kind::Literal, start, line);
        } else if self.peek(0).is_some_and(is_word_start) {
            self.word();
            self.token(Kind::Lifetime, start, line);
        } else {
            self.token(Kind::Punct, start, line);
        }
        Ok(())
    }

    /// Moves past identifier characters.
    fn word(&mut self) {
        while self.peek(0).is_some_and(is_word_char) && self.space_len().is_none() {
            self.advance(1);
        }
    }

    /// If a character Rust takes as whitespace is next, its length.
    fn space_len(&self) -> Option<usize> {
        let c = self.source.get(self.at..)?.chars().next()?;
        matches!(
            c,
            '\t' | '\n'
                | '\u{b}'
                | '\u{c}'
                | '\r'
                | ' '
                | '\u{85}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{2028}'
                | '\u{2029}'
        )
        .then(|| c.len_utf8())
    }

    /// An identifier, or a literal with a prefix: `b"…"`, `c"…"`, `b'…'`,
    /// `r"…"`, `br#"…"#`, `cr"…"`, or the raw identifier `r#name`.
    fn word_or_prefixed(&mut self, start: usize, line: usize) -> Result<(), LexError> {
        self.word();
        let prefix = self.text(start);
        match (prefix, self.peek(0)) {
            ("b" | "c", Some(b'"')) => {
                self.advance(1);
                self.quoted(b'"', "string literal", line)?;
                self.token(Kind::Literal, start, line);
            }
            ("b", Some(b'\'')) => {
                self.advance(1);
                self.quoted(b'\'', "byte literal", line)?;
                self.token(Kind::Literal, start, line);
            }
            ("r" | "br" | "cr", Some(b'"' | b'#')) => {
                let mut hashes = 0;
                while self.peek(hashes) == Some(b'#') {
                    hashes += 1;
                }
                if self.peek(hashes) == Some(b'"') {
                    self.raw_string(line)?;
                    self.token(Kind::Literal, start, line);
                } else if prefix == "r" && hashes == 1 && self.peek(1).is_some_and(is_word_start) {
                    self.advance(1);
                    self.word();
                    self.token(Kind::RawIdent, start, line);
                } else {
                    self.token(Kind::Ident, start, line);
                }
            }
            _ => self.token(Kind::Ident, start, line),
        }
        Ok(())
    }
}

/// The value of a string literal token's text (`"…"` with escapes, or a
/// raw `r#"…"#`; with a `b` or `c` prefix, the bytes read as Latin-1), or
/// `None` if it is not a string literal or an escape is not valid.
pub(crate) fn string_value(text: &str) -> Option<String> {
    let (prefixed, text) = match text.strip_prefix(['b', 'c']) {
        Some(rest) if rest.starts_with(['"', 'r']) => (true, rest),
        _ => (false, text),
    };
    if let Some(raw) = text.strip_prefix('r') {
        let hashes = raw.len() - raw.trim_start_matches('#').len();
        let inner = raw.get(hashes..raw.len().checked_sub(hashes)?)?;
        return inner
            .strip_prefix('"')?
            .strip_suffix('"')
            .map(str::to_owned);
    }
    let inner = text.strip_prefix('"')?.strip_suffix('"')?;
    let mut value = String::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            value.push(c);
            continue;
        }
        match chars.next()? {
            'n' => value.push('\n'),
            'r' => value.push('\r'),
            't' => value.push('\t'),
            '0' => value.push('\0'),
            '\\' => value.push('\\'),
            '\'' => value.push('\''),
            '"' => value.push('"'),
            'x' => {
                let hex: String = [chars.next()?, chars.next()?].iter().collect();
                let byte = u8::from_str_radix(&hex, 16)
                    .ok()
                    .filter(|b| prefixed || *b < 0x80)?;
                value.push(char::from(byte));
            }
            'u' => {
                if chars.next()? != '{' {
                    return None;
                }
                let hex: String = chars
                    .by_ref()
                    .take_while(|c| *c != '}')
                    .filter(|c| *c != '_')
                    .collect();
                value.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
            }
            '\n' | '\r' => {
                // A line continuation: the whitespace after it goes too.
                while chars.peek().is_some_and(|c| c.is_whitespace()) {
                    chars.next();
                }
            }
            _ => return None,
        }
    }
    Some(value)
}

/// Whether `b` can start an identifier. Bytes above ASCII are taken as
/// parts of identifiers (Rust allows Unicode ones); the lexer only needs
/// to recognize ASCII keywords.
const fn is_word_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b >= 0x80
}

const fn is_word_char(b: u8) -> bool {
    is_word_start(b) || b.is_ascii_digit()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idents(source: &str) -> Vec<(&str, usize)> {
        lex(source)
            .unwrap()
            .tokens
            .into_iter()
            .filter(|t| t.kind == Kind::Ident)
            .map(|t| (t.text, t.line))
            .collect()
    }

    #[test]
    fn literals_and_comments_hide_keywords() {
        let source = r####"
            // unsafe
            /* unsafe /* nested unsafe */ still unsafe */
            /// unsafe in a doc comment
            let a = "unsafe \" unsafe";
            let b = b"unsafe";
            let c = c"unsafe";
            let d = r"unsafe";
            let e = r#"unsafe " unsafe"#;
            let f = br##"unsafe "# unsafe"##;
            let g = cr#"unsafe"#;
            let h = '\'';
            let i = b'\\';
            let j = "multi
                line unsafe";
            word
        "####;
        let found = idents(source);
        assert!(found.iter().all(|&(t, _)| t != "unsafe"), "{found:?}");
        assert_eq!(found.last(), Some(&("word", 16)));
        let lexed = lex(source).unwrap();
        assert_eq!(lexed.comments.len(), 3);
        assert_eq!((lexed.comments[1].line, lexed.comments[1].end_line), (3, 3));
    }

    #[test]
    fn keywords_in_code_are_found() {
        let found =
            idents("unsafe { x }\nunsafe fn f() {}\n#[unsafe(no_mangle)]\nunsafe impl S {}");
        let unsafes: Vec<_> = found.iter().filter(|(t, _)| *t == "unsafe").collect();
        assert_eq!(unsafes.len(), 4);
        assert_eq!(unsafes[3].1, 4);
    }

    #[test]
    fn raw_identifiers_are_not_keywords() {
        let lexed = lex("let r#unsafe = 1; r#match").unwrap();
        let raw: Vec<_> = lexed
            .tokens
            .iter()
            .filter(|t| t.kind == Kind::RawIdent)
            .map(|t| t.text)
            .collect();
        assert_eq!(raw, ["r#unsafe", "r#match"]);
        assert!(lexed.tokens.iter().all(|t| t.text != "unsafe"));
    }

    #[test]
    fn chars_and_lifetimes() {
        let lexed =
            lex("fn f<'a>(x: &'a str) -> char { 'outer: loop { break 'outer; } 'é' }").unwrap();
        let kinds: Vec<_> = lexed
            .tokens
            .iter()
            .filter(|t| matches!(t.kind, Kind::Lifetime | Kind::Literal))
            .map(|t| (t.kind, t.text))
            .collect();
        assert_eq!(
            kinds,
            [
                (Kind::Lifetime, "'a"),
                (Kind::Lifetime, "'a"),
                (Kind::Lifetime, "'outer"),
                (Kind::Lifetime, "'outer"),
                (Kind::Literal, "'é'"),
            ]
        );
        // A char literal holding a quote or an escape does not swallow code.
        assert_eq!(
            idents("'\"' unsafe '\\u{1F600}' x"),
            [("unsafe", 1), ("x", 1)]
        );
    }

    #[test]
    fn shebang_but_not_inner_attribute() {
        assert_eq!(
            idents("#!/usr/bin/env run-cargo-script\nunsafe"),
            [("unsafe", 2)]
        );
        assert_eq!(idents("#![allow(x)]"), [("allow", 1), ("x", 1)]);
        assert_eq!(idents("#! [allow(x)]"), [("allow", 1), ("x", 1)]);
        // Comments between `#!` and `[` still make an inner attribute,
        // as rustc reads it; a doc comment does not.
        for source in [
            "#!/**/[allow(x)]",
            "#! /* c */ [allow(x)]",
            "#!// c\n[allow(x)]",
        ] {
            let names: Vec<_> = idents(source).into_iter().map(|(t, _)| t).collect();
            assert_eq!(names, ["allow", "x"], "{source}");
            assert_eq!(shebang_len(source), None, "{source}");
        }
        assert_eq!(shebang_len("#!/usr/bin/env x\nfn"), Some(16));
        assert_eq!(shebang_len("#!//! doc\n[allow(x)]"), Some(9));
        assert_eq!(shebang_len("#!"), Some(2));
        assert_eq!(shebang_len("fn main() {}"), None);
        // A byte-order mark comes before the shebang.
        assert_eq!(idents("\u{feff}#!/bin/x\nunsafe"), [("unsafe", 2)]);
        assert_eq!(idents("\u{feff}unsafe"), [("unsafe", 1)]);
    }

    #[test]
    fn string_values() {
        assert_eq!(string_value(r#""a\nb\"c\\""#).as_deref(), Some("a\nb\"c\\"));
        assert_eq!(
            string_value(r#""\x41\u{1F600}\u{0_041}""#).as_deref(),
            Some("A😀A")
        );
        assert_eq!(
            string_value("\"line \\\n    continued\"").as_deref(),
            Some("line continued")
        );
        assert_eq!(
            string_value(r###"r#"raw "quoted" \n"#"###).as_deref(),
            Some(r#"raw "quoted" \n"#)
        );
        assert_eq!(string_value(r#"r"plain""#).as_deref(), Some("plain"));
        assert_eq!(string_value(r#"b"a\xFF\n""#).as_deref(), Some("a\u{ff}\n"));
        assert_eq!(string_value(r##"br#"x"#"##).as_deref(), Some("x"));
        assert_eq!(string_value(r#"c"x\u{41}""#).as_deref(), Some("xA"));
        assert_eq!(string_value(r#"cr"x""#).as_deref(), Some("x"));
        assert_eq!(string_value("b'x'"), None);
        for bad in [
            "x",
            "'a'",
            r#""\q""#,
            r#""\xFF""#,
            r#""\u{110000}""#,
            r#""unterminated"#,
        ] {
            assert_eq!(string_value(bad), None, "{bad}");
        }
    }

    #[test]
    fn unicode_whitespace_ends_identifiers() {
        for space in [
            '\u{85}', '\u{200e}', '\u{200f}', '\u{2028}', '\u{2029}', '\u{b}',
        ] {
            let before = format!("{space}unsafe{{ x }}");
            let after = format!("unsafe{space}{{ x }}");
            let lint = format!("allow({space}unsafe_code{space})");
            assert_eq!(idents(&before)[0].0, "unsafe", "{space:?}");
            assert_eq!(idents(&after)[0].0, "unsafe", "{space:?}");
            assert_eq!(idents(&lint)[1].0, "unsafe_code", "{space:?}");
        }
        // Other non-ASCII characters are parts of identifiers.
        assert_eq!(idents("unsafeé x"), [("unsafeé", 1), ("x", 1)]);
        let lines = idents("a\u{2028}b");
        assert_eq!(lines, [("a", 1), ("b", 1)]);
    }

    #[test]
    fn line_numbers_count_crlf_and_literal_newlines() {
        assert_eq!(
            idents("a\r\nb\r\n\"x\ny\"\r\nc"),
            [("a", 1), ("b", 2), ("c", 5)]
        );
    }

    #[test]
    fn unterminated_input_is_an_error() {
        for (source, what) in [
            ("/* /* */", "block comment"),
            ("\"abc", "string literal"),
            ("\"abc\\\"", "string literal"),
            ("r#\"abc\"", "raw string literal"),
            ("b'a", "byte literal"),
            ("'\\n", "char literal"),
        ] {
            assert_eq!(lex(source).unwrap_err().what, what, "{source}");
        }
    }
}
