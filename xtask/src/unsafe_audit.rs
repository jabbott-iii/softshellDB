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

//! `cargo xtask unsafe-audit`: `unsafe` lives in `softshell-sys` and
//! `softshell-platform` only, and there every `unsafe` block says why it
//! is sound (ADR-0005).
//!
//! Every Rust file Git lists is scanned with [`lexer`], so
//! text in comments and literals is not code. A file belongs to the member
//! whose directory holds it most closely. Outside the two crates:
//!
//! - any `unsafe` keyword is a finding: blocks, functions, impls, traits,
//!   extern blocks, `#[unsafe(...)]` attributes, and `unsafe` in the Rust
//!   examples of doc comments and `#[doc = "…"]` attributes, which
//!   `cargo test` compiles and runs; so is a `doc` attribute that is not a
//!   plain string literal (`include_str!`), whose text the audit cannot read;
//! - so are `global_asm!`, `naked_asm!` and `asm!`, unsafe without the
//!   keyword;
//! - so is naming the `unsafe_code` lint anywhere but directly in a `deny`
//!   or `forbid` list of an attribute (`allow`, `expect`, `warn`,
//!   `cfg_attr`, raw `r#unsafe_code`, or passed to a macro).
//!
//! Inside them, an `unsafe` block or `unsafe impl` needs a comment that
//! starts with `SAFETY:` and says something, between the statement before
//! it (the last `;`, `{` or `}`) and the `unsafe`; an exported macro may
//! not contain `unsafe`, since it would run in the crates that call it;
//! and extern blocks, raw OS declarations, belong in `softshell-sys` only.
//!
//! Everywhere, what the compiler reads must be what this audit reads:
//! `#[path = …]` and `include!(…)` may name only a relative `.rs` file
//! inside the crate, as a plain string literal (and `include` may not be
//! renamed or passed to a macro); a macro may not give a module its
//! attributes from a fragment, or call a macro it is given; no source is a
//! symbolic link; every target's root file is a `.rs` file inside its
//! package; every member and target is on edition 2024, where
//! `#[no_mangle]`, `#[export_name]`, `#[link_section]` and extern blocks
//! need the `unsafe` keyword; and every member takes the workspace's lints
//! (`[lints] workspace = true`), whose `unsafe_code` stays `deny`.
//!
//! Not scanned: the format fixtures and fuzz corpora, which are data.

use crate::check::Finding;
use crate::error::XtaskError;
use crate::lexer::{self, Comment, Kind, Lexed, Token};
use crate::metadata::{Metadata, Resolve};
use crate::toml_keys;
use crate::workspace::Workspace;

/// Directories of data, not code, which no source check reads.
pub(crate) const DATA_DIRS: [&str; 2] = ["tests/fixtures/", "fuzz/corpus/"];

/// The edition every member and target must be on.
const EDITION: &str = "2024";

/// Which rules a file is under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Zone {
    /// Any crate but the two: no `unsafe` at all.
    Safe,
    /// `softshell-sys`: `unsafe` with `SAFETY:` comments, and the raw OS
    /// declarations.
    Sys,
    /// `softshell-platform`: `unsafe` with `SAFETY:` comments, but no raw
    /// declarations.
    Platform,
}

/// Runs the audit over the workspace.
///
/// # Errors
///
/// [`XtaskError`] if the files cannot be listed or read, or cargo fails.
pub(crate) fn run(workspace: &Workspace) -> Result<Vec<Finding>, XtaskError> {
    let metadata = Metadata::load(workspace, Resolve::No)?;
    let mut findings = check_targets(&metadata);
    findings.extend(check_lints(workspace, &metadata)?);
    let mut members = Vec::new();
    for package in metadata.members() {
        let dir = metadata
            .relative(&package.dir)
            .ok_or_else(|| XtaskError::Shape {
                what: format!("package `{}`", package.name),
                detail: format!(
                    "its directory {} is outside the workspace root {}",
                    package.dir.display(),
                    metadata.root.display()
                ),
            })?;
        members.push((dir, package.name.as_str()));
    }
    let files = workspace.files()?;
    for link in &files.links {
        findings.push(Finding::new(
            link,
            "is a symbolic link: the compiler follows links and the source checks do not, so sources are regular files",
        ));
    }
    for path in files.files {
        if !path.ends_with(".rs") || DATA_DIRS.iter().any(|dir| path.starts_with(dir)) {
            continue;
        }
        let source = workspace.read_text(&path)?;
        let zone = match owner(&members, &path) {
            Some("softshell-sys") => Zone::Sys,
            Some("softshell-platform") => Zone::Platform,
            _ => Zone::Safe,
        };
        findings.extend(audit(&path, &source, zone));
    }
    Ok(findings)
}

/// The member that holds `path`: the one with the longest directory that
/// contains it, or `None` for a file in no member.
fn owner<'a>(members: &[(String, &'a str)], path: &str) -> Option<&'a str> {
    members
        .iter()
        .filter(|(dir, _)| dir.is_empty() || path.starts_with(&format!("{dir}/")))
        .max_by_key(|(dir, _)| dir.len())
        .map(|(_, name)| *name)
}

/// Findings for the members' editions and target root files.
pub(crate) fn check_targets(metadata: &Metadata) -> Vec<Finding> {
    let mut findings = Vec::new();
    for package in metadata.members() {
        let manifest = metadata.display(&package.manifest);
        let name = &package.name;
        if package.edition != EDITION {
            findings.push(Finding::new(
                &manifest,
                format!(
                    "`{name}` is on edition {}; every crate is on {EDITION}, where unsafe attributes and extern blocks need the `unsafe` keyword",
                    package.edition
                ),
            ));
        }
        let dir = metadata.relative(&package.dir);
        for target in &package.targets {
            let src = metadata.display(&target.src_path);
            if target.edition != EDITION {
                findings.push(Finding::new(
                    &manifest,
                    format!(
                        "the target `{src}` is on edition {}; every target is on {EDITION}",
                        target.edition
                    ),
                ));
            }
            let inside = match (&dir, metadata.relative(&target.src_path)) {
                (Some(dir), Some(rel)) => dir.is_empty() || rel.starts_with(&format!("{dir}/")),
                _ => false,
            };
            if !src.ends_with(".rs") || !inside {
                findings.push(Finding::new(
                    &manifest,
                    format!(
                        "the target root `{src}` must be a `.rs` file inside `{name}`'s directory, where the source checks read it"
                    ),
                ));
            }
        }
    }
    findings
}

/// Findings for lints: every member takes the workspace's, and the
/// workspace's `unsafe_code` is `deny` or `forbid`.
fn check_lints(workspace: &Workspace, metadata: &Metadata) -> Result<Vec<Finding>, XtaskError> {
    let mut findings = Vec::new();
    let root = workspace.read_text("Cargo.toml")?;
    findings.extend(check_workspace_lints("Cargo.toml", &root));
    for package in metadata.members() {
        let Some(path) = metadata.relative(&package.manifest) else {
            continue;
        };
        findings.extend(check_member_lints(&path, &workspace.read_text(&path)?));
    }
    Ok(findings)
}

/// A finding unless the workspace manifest `text` keeps `unsafe_code` at
/// `deny` or `forbid`.
pub(crate) fn check_workspace_lints(path: &str, text: &str) -> Option<Finding> {
    let keys = match toml_keys::keys(text) {
        Ok(keys) => keys,
        Err(err) => return Some(unreadable(path, &err)),
    };
    let level = keys
        .iter()
        .find(|k| k.path == ["workspace", "lints", "rust", "unsafe_code"]);
    let denied =
        level.is_some_and(|k| k.value.contains("\"deny\"") || k.value.contains("\"forbid\""));
    (!denied).then(|| {
        Finding::new(
            path,
            "`[workspace.lints.rust] unsafe_code` must stay `deny` (ADR-0005)",
        )
    })
}

/// Findings unless the member manifest `text` has `[lints] workspace = true`
/// and no lints of its own.
pub(crate) fn check_member_lints(path: &str, text: &str) -> Vec<Finding> {
    let keys = match toml_keys::keys(text) {
        Ok(keys) => keys,
        Err(err) => return vec![unreadable(path, &err)],
    };
    let mut findings = Vec::new();
    let mut takes_workspace = false;
    for key in &keys {
        match key.path.as_slice() {
            [lints] if lints == "lints" => {}
            [lints, workspace] if lints == "lints" && workspace == "workspace" => {
                takes_workspace = key.value == "true";
            }
            [lints, ..] if lints == "lints" => findings.push(Finding::at_line(
                path,
                key.line,
                "a member takes the workspace's lints only (`[lints] workspace = true`), so that `unsafe_code` stays `deny` (ADR-0005)",
            )),
            _ => {}
        }
    }
    if !takes_workspace {
        findings.push(Finding::new(
            path,
            "needs `[lints] workspace = true`, so that the workspace's `unsafe_code = \"deny\"` applies (ADR-0005)",
        ));
    }
    findings
}

fn unreadable(path: &str, err: &toml_keys::TomlError) -> Finding {
    Finding::at_line(
        path,
        err.line,
        format!("cannot read this TOML: a {} never ends", err.what),
    )
}

/// The findings in one file, `source`, at `path`, under `zone`'s rules.
pub(crate) fn audit(path: &str, source: &str, zone: Zone) -> Vec<Finding> {
    let lexed = match lexer::lex(source) {
        Ok(lexed) => lexed,
        Err(err) => {
            return vec![Finding::at_line(
                path,
                err.line,
                format!("cannot scan for `unsafe`: this {} never ends", err.what),
            )];
        }
    };
    let safe = zone == Zone::Safe;
    let tokens = &lexed.tokens;
    let attributes = attribute_mask(tokens);
    let mut findings = Vec::new();
    let mut found =
        |line: usize, message: &str| findings.push(Finding::at_line(path, line, message));
    for (i, token) in tokens.iter().enumerate() {
        let next = |n: usize| tokens.get(i + n);
        let in_attribute = attributes.get(i).copied().unwrap_or(false);
        if token.kind == Kind::Ident && token.text == "unsafe" {
            if safe {
                found(
                    token.line,
                    "`unsafe` outside softshell-sys and softshell-platform (ADR-0005)",
                );
            } else {
                let block_or_impl = next(1).is_some_and(|t| is_punct(t, "{") || name(t) == "impl");
                if block_or_impl && !has_safety_comment(&lexed, i) {
                    found(
                        token.line,
                        "`unsafe` without a `// SAFETY:` comment saying why it is sound, before it in its statement",
                    );
                }
            }
            continue;
        }
        if !matches!(token.kind, Kind::Ident | Kind::RawIdent) {
            continue;
        }
        match name(token) {
            "unsafe_code" if safe && !(in_attribute && in_deny_list(tokens, i)) => found(
                token.line,
                "the `unsafe_code` lint may be named only in a `deny` or `forbid` attribute outside softshell-sys and softshell-platform (ADR-0005)",
            ),
            "global_asm" | "naked_asm" | "asm" if safe && names_builtin(tokens, i) => found(
                token.line,
                "inline assembly is unsafe code, which belongs in softshell-sys and softshell-platform (ADR-0005)",
            ),
            "path" if in_attribute && next(1).is_some_and(|t| is_punct(t, "=")) => {
                if !next(2).is_some_and(is_local_rs_literal) {
                    found(token.line, PATH_RULE);
                }
            }
            "include" if names_builtin(tokens, i) => {
                let call = next(1).is_some_and(|t| is_punct(t, "!"))
                    && next(2).is_some_and(|t| matches!(t.text, "(" | "[" | "{"))
                    && next(3).is_some_and(is_local_rs_literal)
                    && next(4).is_some_and(|t| matches!(t.text, ")" | "]" | "}"));
                if !call {
                    found(token.line, PATH_RULE);
                }
            }
            "macro_rules" if next(1).is_some_and(|t| is_punct(t, "!")) => {
                for (line, message) in macro_findings(tokens, &attributes, i, zone) {
                    found(line, message);
                }
            }
            "extern" if zone == Zone::Platform && is_extern_block(tokens, i) => found(
                token.line,
                "extern blocks, raw OS declarations, belong in softshell-sys; softshell-platform calls them",
            ),
            _ => {}
        }
    }
    if safe {
        findings.extend(doc_examples(path, &lexed, &attributes));
    }
    findings
}

const PATH_RULE: &str = "`#[path]` and `include!` may name only a relative `.rs` file inside the crate, as a plain string literal, so that the source checks read what the compiler reads";

fn is_punct(token: &Token<'_>, text: &str) -> bool {
    token.kind == Kind::Punct && token.text == text
}

/// An identifier's name: `r#path` is `path`, as rustc reads it.
fn name<'a>(token: &Token<'a>) -> &'a str {
    match token.kind {
        Kind::RawIdent => token.text.strip_prefix("r#").unwrap_or(token.text),
        Kind::Ident => token.text,
        _ => "",
    }
}

/// Whether `token` is a plain string literal naming a relative `.rs` path
/// that does not leave its directory.
fn is_local_rs_literal(token: &Token<'_>) -> bool {
    let Some(path) = token
        .text
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
    else {
        return false;
    };
    token.kind == Kind::Literal
        && path.ends_with(".rs")
        && !path.starts_with('/')
        && !path.contains(['\\', ':'])
        && path.split('/').all(|part| part != ".." && !part.is_empty())
}

/// Whether the identifier at `at` can name a built-in macro: it is
/// called (`include!`), reached by a path (`core::include`), or renamed
/// (`include as inc`). A variable of that name is none of these.
fn names_builtin(tokens: &[Token<'_>], at: usize) -> bool {
    let punct = |i: usize, text: &str| tokens.get(i).is_some_and(|t| is_punct(t, text));
    punct(at + 1, "!")
        || (at >= 2 && punct(at - 1, ":") && punct(at - 2, ":"))
        || tokens.get(at + 1).is_some_and(|t| name(t) == "as")
}

/// Whether `tokens[at]` sits directly in the list of a `deny(…)` or
/// `forbid(…)`.
fn in_deny_list(tokens: &[Token<'_>], at: usize) -> bool {
    let mut depth = 0usize;
    let mut i = at;
    while i > 0 {
        i -= 1;
        let Some(token) = tokens.get(i) else {
            return false;
        };
        if is_punct(token, ")") {
            depth += 1;
        } else if is_punct(token, "(") {
            if depth == 0 {
                return i
                    .checked_sub(1)
                    .and_then(|before| tokens.get(before))
                    .is_some_and(|t| matches!(name(t), "deny" | "forbid"));
            }
            depth -= 1;
        } else if !(matches!(token.kind, Kind::Ident | Kind::RawIdent)
            || is_punct(token, ",")
            || is_punct(token, ":"))
        {
            // Lint lists hold only names (`clippy::x`) and commas.
            return false;
        }
    }
    false
}

/// Whether the `extern` at `at` opens an extern block: `extern {` or
/// `extern "abi" {`.
fn is_extern_block(tokens: &[Token<'_>], at: usize) -> bool {
    let after = |n: usize| tokens.get(at + n);
    let abi = after(1).is_some_and(|t| t.kind == Kind::Literal);
    after(if abi { 2 } else { 1 }).is_some_and(|t| is_punct(t, "{"))
}

/// For each token, whether it is part of an attribute (`#[…]`, `#![…]`).
fn attribute_mask(tokens: &[Token<'_>]) -> Vec<bool> {
    let mut mask = vec![false; tokens.len()];
    let mut i = 0;
    while i < tokens.len() {
        match attribute_end(tokens, i) {
            Some(end) => {
                for flag in mask.get_mut(i..=end).unwrap_or(&mut []) {
                    *flag = true;
                }
                i = end + 1;
            }
            None => i += 1,
        }
    }
    mask
}

/// If `tokens[at]` starts an attribute (`#[` or `#![`), the index of its
/// closing `]`.
fn attribute_end(tokens: &[Token<'_>], at: usize) -> Option<usize> {
    let punct = |i: usize, text: &str| tokens.get(i).is_some_and(|t| is_punct(t, text));
    if !punct(at, "#") {
        return None;
    }
    let open = if punct(at + 1, "[") {
        at + 1
    } else if punct(at + 1, "!") && punct(at + 2, "[") {
        at + 2
    } else {
        return None;
    };
    closing(tokens, open)
}

/// The index of the bracket that closes the one at `open`.
fn closing(tokens: &[Token<'_>], open: usize) -> Option<usize> {
    let (opener, closer) = match tokens.get(open)?.text {
        "[" => ("[", "]"),
        "(" => ("(", ")"),
        "{" => ("{", "}"),
        _ => return None,
    };
    let mut depth = 0usize;
    for (i, token) in tokens.iter().enumerate().skip(open) {
        if is_punct(token, opener) {
            depth += 1;
        } else if is_punct(token, closer) {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

/// Findings for the `macro_rules!` at `at`: an exported one may not hold
/// `unsafe` (in the two crates), and none may give a module attributes
/// from a fragment or call a macro it is given.
fn macro_findings(
    tokens: &[Token<'_>],
    attributes: &[bool],
    at: usize,
    zone: Zone,
) -> Vec<(usize, &'static str)> {
    let mut findings = Vec::new();
    let line = tokens.get(at).map_or(0, |t| t.line);
    // The body is the first bracket after `macro_rules!` and its name
    // (which a macro that writes macros gives as `$name`).
    let body = (at + 2..tokens.len().min(at + 5))
        .find(|&i| {
            tokens
                .get(i)
                .is_some_and(|t| matches!(t.text, "{" | "(" | "[") && t.kind == Kind::Punct)
        })
        .and_then(|open| Some((open, closing(tokens, open)?)));
    let Some((open, close)) = body else {
        findings.push((
            line,
            "cannot find this macro's body, so the audit cannot read it",
        ));
        return findings;
    };
    let body = tokens.get(open..close).unwrap_or(&[]);
    if zone != Zone::Safe
        && is_exported(tokens, attributes, at)
        && let Some(t) = body
            .iter()
            .find(|t| t.kind == Kind::Ident && t.text == "unsafe")
    {
        findings.push((
            t.line,
            "an exported macro may not contain `unsafe`: it would run in the crates that call it",
        ));
    }
    let has_mod = body.iter().any(|t| name(t) == "mod");
    for (i, t) in body.iter().enumerate() {
        let after = |n: usize| body.get(i + n);
        let fragment_attribute = is_punct(t, "#")
            && after(1).is_some_and(|t| is_punct(t, "["))
            && after(2).is_some_and(|t| is_punct(t, "$"));
        if fragment_attribute && has_mod {
            findings.push((
                t.line,
                "a macro may not give a module attributes from a fragment: a `path` could hide in it",
            ));
        }
        let calls_fragment = is_punct(t, "$")
            && after(1).is_some_and(|t| matches!(t.kind, Kind::Ident | Kind::RawIdent))
            && after(2).is_some_and(|t| is_punct(t, "!"));
        if calls_fragment {
            findings.push((
                t.line,
                "a macro may not call a macro it is given: it could be `include!`",
            ));
        }
    }
    findings
}

/// Whether the attributes right before `tokens[at]` include
/// `macro_export`.
fn is_exported(tokens: &[Token<'_>], attributes: &[bool], at: usize) -> bool {
    let mut i = at;
    while i > 0 && attributes.get(i - 1).copied().unwrap_or(false) {
        i -= 1;
        if tokens.get(i).is_some_and(|t| name(t) == "macro_export") {
            return true;
        }
    }
    false
}

/// Whether a comment saying `SAFETY:` comes after the statement before
/// `tokens[at]` ends and before the `unsafe` itself.
fn has_safety_comment(lexed: &Lexed<'_>, at: usize) -> bool {
    let Some(token) = lexed.tokens.get(at) else {
        return false;
    };
    let start = lexed
        .tokens
        .get(..at)
        .unwrap_or(&[])
        .iter()
        .rev()
        .find(|t| is_punct(t, ";") || is_punct(t, "{") || is_punct(t, "}"))
        .map_or(0, |t| t.offset);
    let between: Vec<&Comment<'_>> = lexed
        .comments
        .iter()
        .filter(|c| c.offset > start && c.offset < token.offset)
        .collect();
    between
        .iter()
        .enumerate()
        .any(|(i, comment)| says_safety(comment, between.get(i + 1).copied()))
}

/// Whether `comment` starts with `SAFETY:` and says something, there or
/// in the `next` comment, on the line below.
fn says_safety(comment: &Comment<'_>, next: Option<&Comment<'_>>) -> bool {
    let Some(rest) = comment_body(comment.text)
        .trim_start()
        .strip_prefix("SAFETY:")
    else {
        return false;
    };
    !rest.trim().is_empty()
        || next.is_some_and(|n| {
            n.line == comment.end_line + 1 && !comment_body(n.text).trim().is_empty()
        })
}

/// A comment's text without its markers (`//`, `///`, `//!`, `/*`, `*/`).
fn comment_body(text: &str) -> &str {
    if let Some(block) = text.strip_prefix("/*") {
        let block = block.strip_suffix("*/").unwrap_or(block);
        block.trim_start_matches(['*', '!'])
    } else {
        let line = text.trim_start_matches('/');
        line.strip_prefix('!').unwrap_or(line)
    }
}

/// A piece of documentation: a doc comment or a `doc` attribute.
struct DocPiece {
    /// Whether it documents the enclosing item (`//!`, `#![doc]`).
    inner: bool,
    /// Where it starts and ends in the source.
    start: usize,
    end: usize,
    /// Its lines of text and their lines in the file.
    lines: Vec<(usize, String)>,
}

/// `unsafe` in the Rust examples of the documentation, which rustdoc
/// compiles as doctests, and `doc` attributes the audit cannot read.
fn doc_examples(path: &str, lexed: &Lexed<'_>, attributes: &[bool]) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut pieces = Vec::new();
    for comment in &lexed.comments {
        if let Some(piece) = comment_piece(comment) {
            pieces.push(piece);
        }
    }
    let tokens = &lexed.tokens;
    for (i, token) in tokens.iter().enumerate() {
        if name(token) != "doc"
            || !attributes.get(i).copied().unwrap_or(false)
            || !tokens.get(i + 1).is_some_and(|t| is_punct(t, "="))
        {
            continue;
        }
        let value = tokens.get(i + 2).filter(|t| t.kind == Kind::Literal);
        let Some(text) = value.and_then(|t| lexer::string_value(t.text)) else {
            findings.push(Finding::at_line(
                path,
                token.line,
                "a `doc` attribute outside softshell-sys and softshell-platform must be a plain string literal, so that the audit can read its examples",
            ));
            continue;
        };
        // The attribute's `#`, and whether it is `#!`.
        let hash = (0..i).rev().find(|&j| {
            tokens.get(j).is_some_and(|t| is_punct(t, "#"))
                && attribute_end(tokens, j).is_some_and(|end| end > i)
        });
        let Some(hash) = hash else {
            continue;
        };
        let end = attribute_end(tokens, hash)
            .and_then(|e| tokens.get(e))
            .map_or(token.offset, |t| t.offset);
        // A literal on one line keeps every line of its text there; one
        // that spans lines spreads them as written.
        let (first, spans) = value.map_or((token.line, false), |t| (t.line, t.text.contains('\n')));
        pieces.push(DocPiece {
            inner: tokens.get(hash + 1).is_some_and(|t| is_punct(t, "!")),
            start: tokens.get(hash).map_or(token.offset, |t| t.offset),
            end,
            lines: text
                .lines()
                .enumerate()
                .map(|(n, l)| (if spans { first + n } else { first }, l.to_owned()))
                .collect(),
        });
    }
    pieces.sort_by_key(|p| p.start);
    // rustdoc joins every piece of an item's documentation, whatever comes
    // between them, except code.
    let mut block: Vec<(usize, String)> = Vec::new();
    let mut previous: Option<&DocPiece> = None;
    for piece in &pieces {
        let joined = previous.is_some_and(|p| {
            p.inner == piece.inner
                && tokens
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| t.offset > p.end && t.offset < piece.start)
                    .all(|(j, _)| attributes.get(j).copied().unwrap_or(false))
        });
        if !joined {
            findings.extend(markdown_examples(path, &block));
            block.clear();
        }
        block.extend(piece.lines.iter().cloned());
        previous = Some(piece);
    }
    findings.extend(markdown_examples(path, &block));
    findings
}

/// The doc piece of `comment`, if it is a doc comment.
fn comment_piece(comment: &Comment<'_>) -> Option<DocPiece> {
    let text = comment.text;
    if !lexer::is_doc_comment(text) {
        return None;
    }
    let inner = text.starts_with("//!") || text.starts_with("/*!");
    let body = text.get(3..).unwrap_or("");
    let lines = if text.starts_with("//") {
        vec![(comment.line, body.trim_end_matches('\r').to_owned())]
    } else {
        let body = body.strip_suffix("*/").unwrap_or(body);
        body.lines()
            .enumerate()
            .map(|(i, line)| {
                let trimmed = line.trim_start();
                let line = trimmed.strip_prefix('*').unwrap_or(line);
                (comment.line + i, line.to_owned())
            })
            .collect()
    };
    Some(DocPiece {
        inner,
        start: comment.offset,
        end: comment.offset + text.len(),
        lines,
    })
}

/// `unsafe` in the Rust code blocks of one item's documentation, `lines`:
/// fenced blocks (of three or more backticks or tildes) whose info string
/// makes them Rust, and indented blocks, as rustdoc reads Markdown.
fn markdown_examples(path: &str, lines: &[(usize, String)]) -> Vec<Finding> {
    // rustdoc removes the indentation every line shares.
    let indent = lines
        .iter()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(_, l)| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    let mut findings = Vec::new();
    let mut code: Vec<(usize, &str)> = Vec::new();
    // An open fence: its character, its length, and whether it is Rust
    // that rustdoc compiles.
    let mut fence: Option<(char, usize, bool)> = None;
    // An open indented block, and the indentation it is measured from.
    let mut indented: Option<usize> = None;
    // The content indentation of the list item being read, if any: code
    // inside it is indented from there.
    let mut list: Option<usize> = None;
    let mut after_blank = true;
    for (line, text) in lines {
        let text = text.get(indent..).unwrap_or_else(|| text.trim_start());
        let trimmed = text.trim_start();
        let leading = text.len() - trimmed.len();
        if let Some((c, len, compiled)) = fence {
            let run = trimmed.chars().take_while(|&x| x == c).count();
            if run >= len && trimmed.get(run..).unwrap_or("").trim().is_empty() {
                if compiled {
                    findings.extend(scan_example(path, &code));
                }
                code.clear();
                fence = None;
            } else if compiled {
                code.push((*line, hide_marker(text)));
            }
            continue;
        }
        if let Some(base) = indented {
            if leading >= base + 4 || trimmed.is_empty() {
                code.push((*line, text.get(base + 4..).unwrap_or("")));
                continue;
            }
            findings.extend(scan_example(path, &code));
            code.clear();
            indented = None;
        }
        let base = list.unwrap_or(0);
        let opener = ['`', '~'].into_iter().find_map(|c| {
            let run = trimmed.chars().take_while(|&x| x == c).count();
            (run >= 3 && leading < base + 4).then_some((c, run))
        });
        if let Some((c, run)) = opener {
            let info = trimmed.get(run..).unwrap_or("");
            if c == '~' || !info.contains('`') {
                fence = Some((c, run, is_rust_info(info) && !is_ignored(info)));
                after_blank = false;
                continue;
            }
        }
        if leading >= base + 4 && after_blank && !trimmed.is_empty() {
            indented = Some(base);
            code.push((*line, text.get(base + 4..).unwrap_or("")));
            continue;
        }
        if !trimmed.is_empty() {
            match list_item(trimmed) {
                Some(marker) => list = Some(leading + marker),
                None if after_blank && leading < base => list = None,
                None => {}
            }
        }
        after_blank = trimmed.is_empty();
    }
    if fence.is_some_and(|(_, _, compiled)| compiled) || indented.is_some() {
        findings.extend(scan_example(path, &code));
    }
    findings
}

/// If `line` starts a list item (`- `, `* `, `+ `, `1. `, `1) `), the
/// width of its marker and the spaces after it.
fn list_item(line: &str) -> Option<usize> {
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    let marker = match line.as_bytes().get(digits) {
        Some(b'.' | b')') if (1..=9).contains(&digits) => digits + 1,
        Some(b'-' | b'*' | b'+') if digits == 0 => 1,
        _ => return None,
    };
    let rest = line.get(marker..)?;
    let spaces = rest.len() - rest.trim_start_matches(' ').len();
    (spaces >= 1 || rest.is_empty()).then_some(marker + spaces.clamp(1, 4))
}

/// Whether a fence's info string marks its block `ignore`, which rustdoc
/// neither compiles nor runs (`ignore-<target>` it does elsewhere).
fn is_ignored(info: &str) -> bool {
    info.split(|c: char| c == ',' || c.is_whitespace())
        .any(|word| word == "ignore")
}

/// Whether a fence's info string makes its block Rust, as rustdoc reads
/// it: empty, naming `rust`, or naming only rustdoc's own attributes.
/// `{…}` groups are classes, not languages.
fn is_rust_info(info: &str) -> bool {
    let mut words = Vec::new();
    let mut depth = 0usize;
    let mut word = String::new();
    for c in info.chars() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            _ if depth > 0 => {}
            c if c == ',' || c.is_whitespace() => words.push(std::mem::take(&mut word)),
            c => word.push(c),
        }
    }
    words.push(word);
    let words: Vec<&String> = words.iter().filter(|w| !w.is_empty()).collect();
    words.iter().any(|w| *w == "rust")
        || words.iter().all(|word| {
            matches!(
                word.as_str(),
                "ignore"
                    | "no_run"
                    | "should_panic"
                    | "compile_fail"
                    | "test_harness"
                    | "standalone_crate"
            ) || word.starts_with("edition")
                || word.starts_with("ignore-")
                || (word.starts_with('E')
                    && word
                        .get(1..)
                        .is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit())))
        })
}

/// A doctest line without rustdoc's hiding `#`: `# use x;` is `use x;`.
fn hide_marker(line: &str) -> &str {
    let trimmed = line.trim_start();
    if trimmed == "#" {
        ""
    } else {
        trimmed.strip_prefix("# ").unwrap_or(line)
    }
}

/// `unsafe` in one doc example: `code` holds its lines and their lines in
/// the file.
fn scan_example(path: &str, code: &[(usize, &str)]) -> Vec<Finding> {
    let text: Vec<&str> = code.iter().map(|(_, line)| *line).collect();
    let file_line = |n: usize| code.get(n.saturating_sub(1)).map_or(0, |(line, _)| *line);
    match lexer::lex(&text.join("\n")) {
        Ok(lexed) => lexed
            .tokens
            .iter()
            .filter(|t| t.kind == Kind::Ident && t.text == "unsafe")
            .map(|t| {
                Finding::at_line(
                    path,
                    file_line(t.line),
                    "`unsafe` in a doc example outside softshell-sys and softshell-platform: `cargo test` runs it (ADR-0005)",
                )
            })
            .collect(),
        Err(err) => vec![Finding::at_line(
            path,
            file_line(err.line),
            format!("cannot scan this doc example for `unsafe`: a {} never ends", err.what),
        )],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::tests::{metadata, package, package_with, target};

    fn lines_of(path: &str, source: &str, zone: Zone) -> Vec<usize> {
        let found = audit(path, source, zone);
        let crlf = audit(path, &source.replace('\n', "\r\n"), zone);
        assert_eq!(found, crlf, "CRLF line endings change nothing");
        let mut lines: Vec<usize> = found
            .iter()
            .map(|f| {
                let line = f.location.rsplit(':').next().unwrap();
                line.parse().unwrap()
            })
            .collect();
        lines.sort_unstable();
        lines
    }

    const TXN: &str = "crates/txn/src/lib.rs";

    #[test]
    fn unsafe_outside_the_two_crates_is_found_wherever_it_is() {
        let source = "\
fn f() {
    // SAFETY: a comment does not make it allowed
    unsafe { g() }
}
unsafe fn h() {}
unsafe impl Send for S {}
unsafe trait T {}
unsafe extern \"C\" {}
#[unsafe(no_mangle)]
fn k() {}
core::arch::global_asm!(\"nop\");
use core::arch::naked_asm as n;
let asm = 1;
";
        assert_eq!(
            lines_of(TXN, source, Zone::Safe),
            [3, 5, 6, 7, 8, 9, 11, 12]
        );
    }

    #[test]
    fn invisible_whitespace_does_not_hide_unsafe() {
        for space in ['\u{200e}', '\u{2028}', '\u{85}'] {
            let source = format!(
                "fn f() {{ {space}unsafe{space}{{ g() }} }}\n#[allow({space}unsafe_code)]\nfn h() {{}}\n"
            );
            assert_eq!(lines_of(TXN, &source, Zone::Safe), [1, 2], "{space:?}");
        }
    }

    #[test]
    fn text_that_is_not_code_is_not_found() {
        let source = "\
//! Splitting needs `unsafe` or an unstable API.
/* unsafe { } */
/**/ const Z: u8 = 0; /***/
const A: &str = \"unsafe { }\";
const B: &str = r#\"unsafe { }\"#;
const C: char = '\"';
const D: u8 = b'\"';
const E: &str = r##\"unsafe \"# unsafe\"##;
const F: &[u8] = br#\"unsafe\"#;
const G: &CStr = cr\"unsafe\";
const H: char = '🦀';
let r#unsafe = 1;
#![deny(unsafe_code)]
#![forbid(clippy::x, unsafe_code)]
#![cfg_attr(test, deny(unsafe_code))]
let s = include_str!(\"../data.json\");
let included = 1;
";
        assert_eq!(audit("crates/core/src/cli.rs", source, Zone::Safe), []);
    }

    #[test]
    fn naming_the_unsafe_code_lint_is_found() {
        let source = "\
#![allow(unsafe_code)]
#![cfg_attr(test, allow(dead_code, unsafe_code))]
#[expect(unsafe_code)]
#[warn(unsafe_code)]
#![allow(r#unsafe_code)]
lint!(unsafe_code);
lint!(deny(unsafe_code) mod m {});
#![deny(unsafe_code)]
#![r#deny(r#unsafe_code)]
#![allow(dead_code)]
// #![allow(unsafe_code)]
";
        assert_eq!(
            lines_of("xtask/src/main.rs", source, Zone::Safe),
            [1, 2, 3, 4, 5, 6, 7]
        );
        assert_eq!(lines_of("crates/sys/src/lib.rs", source, Zone::Sys), []);
    }

    #[test]
    fn files_the_audit_cannot_read_are_found() {
        let source = "\
#[path = \"raw.txt\"]
mod a;
#[cfg_attr(windows, path = \"../../sys/src/raw.rs\")]
mod b;
#[path = \"/tmp/x.rs\"]
mod c;
#[path = \"os/windows.rs\"]
mod d;
include!(\"gen.inc\");
include!(concat!(env!(\"OUT_DIR\"), \"/gen.rs\"));
include!(\"tables.rs\");
let path = \"not an attribute\";
#[r#path = \"raw.txt\"]
mod e;
r#include!(\"raw.txt\");
use core::include as inc;
use std::{include as inc2};
let include = true;
f(include, asm);
";
        let expected = [1, 3, 5, 9, 10, 13, 15, 16, 17];
        assert_eq!(
            lines_of("crates/text/src/lib.rs", source, Zone::Safe),
            expected
        );
        assert_eq!(
            lines_of("crates/platform/src/lib.rs", source, Zone::Platform),
            expected
        );
    }

    #[test]
    fn macros_that_could_hide_a_file() {
        let source = "\
macro_rules! module {
    ($attr:meta, $name:ident) => {
        #[$attr]
        mod $name;
    };
}
macro_rules! call {
    ($m:ident, $a:expr) => {
        $m!($a)
    };
}
macro_rules! forward {
    ($(#[$meta:meta])* $item:item) => {
        $(#[$meta])* $item
    };
}
";
        assert_eq!(lines_of(TXN, source, Zone::Safe), [3, 9]);
    }

    #[test]
    fn blocks_in_the_two_crates_need_a_safety_comment() {
        let source = "\
fn f() {
    // SAFETY: the pointer is valid.
    let a = unsafe { g() };
    // SAFETY: checked above,
    // and the length too.
    let b = unsafe { g() };
    let c = foo(
        // SAFETY: inside an argument list.
        unsafe { g() },
    );
    /* SAFETY: a block comment */ unsafe { g() };
    // SAFETY: attributes may come between.
    #[allow(clippy::x)]
    #[cfg(all(
        unix,
    ))]
    unsafe { g() };
    // SAFETY: rustfmt may break the statement.
    let n =
        unsafe { read_at(fd, buf, off) };
    // SAFETY:
    //   the explanation may start on the next line.
    unsafe { g() };
    let d = unsafe { g() };
    // SAFETY: but not with code between.
    let e = 1;
    unsafe { g() };
    // TODO: write the SAFETY: comment
    unsafe { g() };
    // SAFETY:
    unsafe { g() };
    // SAFETY:
    unsafe { g() } // TODO: an empty SAFETY: is not saved by a later comment
    unsafe { g() } // SAFETY: nor after it.
}
// SAFETY: an impl needs one too.
unsafe impl Send for S {}
unsafe impl Sync for S {}
unsafe fn h() {}
";
        assert_eq!(
            lines_of("crates/sys/src/os.rs", source, Zone::Sys),
            [24, 27, 29, 31, 33, 34, 38]
        );
    }

    #[test]
    fn extern_blocks_belong_in_sys() {
        let source = "\
unsafe extern \"C\" {
    fn f();
}
unsafe extern {
}
pub extern \"C\" fn callback() {}
";
        assert_eq!(
            lines_of("crates/platform/src/os.rs", source, Zone::Platform),
            [1, 4]
        );
        assert_eq!(lines_of("crates/sys/src/os.rs", source, Zone::Sys), []);
    }

    #[test]
    fn exported_macros_in_the_two_crates_may_not_hold_unsafe() {
        let source = "\
#[macro_export]
#[doc(hidden)]
macro_rules! raw_read {
    ($p:expr) => {
        // SAFETY: the caller's pointer.
        unsafe { *$p }
    };
}
macro_rules! internal {
    ($p:expr) => {
        // SAFETY: used only here.
        unsafe { *$p }
    };
}
#[macro_export]
macro_rules! safe {
    () => {};
}
#[r#macro_export]
macro_rules! raw_attribute {
    ($p:expr) => {
        // SAFETY: the caller's pointer.
        unsafe { *$p }
    };
}
macro_rules! make_reader {
    ($name:ident) => {
        #[macro_export]
        macro_rules! $name {
            ($p:expr) => {
                // SAFETY: the caller's pointer.
                unsafe { *$p }
            };
        }
    };
}
";
        assert_eq!(
            lines_of("crates/platform/src/lib.rs", source, Zone::Platform),
            [6, 23, 32]
        );
    }

    #[test]
    fn doc_examples_are_code() {
        let source = "\
//! Module docs.
//!
//! ```
//! # fn main() {
//! let x = unsafe { f() };
//! # }
//! ```
//!
//! ```text
//! unsafe { prose }
//! ```
/// An item.
///
/// ```no_run,edition2024
/// unsafe { g() }
/// ```
/// ~~~
/// // unsafe in a comment of an example
/// let s = \"unsafe\";
/// ~~~
fn item() {}
/**
 * ```
 * unsafe { h() }
 * ```
 */
fn other() {}
/// Prose may say unsafe.
fn third() {}
/// ```
// a plain comment between doc lines
/// unsafe { i() }
#[inline]
/// ```
fn fourth() {}
/// ````rust,foo {.class}
/// unsafe { j() }
/// ````
fn fifth() {}
/// Indented:
///
///     unsafe { k() }
fn sixth() {}
#[doc = \"```\\nunsafe { l() }\\n```\"]
fn seventh() {}
#[doc = include_str!(\"../README.md\")]
fn eighth() {}
/// ```text,rust
/// unsafe { m() }
/// ```
fn ninth() {}
";
        assert_eq!(
            lines_of(TXN, source, Zone::Safe),
            [5, 15, 24, 32, 37, 42, 44, 46, 49]
        );
        assert_eq!(lines_of("crates/sys/src/lib.rs", source, Zone::Sys), []);
    }

    #[test]
    fn doc_examples_as_rustdoc_reads_them() {
        let source = "\
/// Steps:
///
/// 1. Call `prepare` first.
///
///     This paragraph belongs to the item, and mentions unsafe code.
///
/// 2. Then `commit`.
///
///         let x = unsafe { inside_the_list() };
///
/// - A nested item:
///     ```
///     unsafe { fenced_in_the_list() }
///     ```
///
/// Out of the list again:
///
///     unsafe { indented_again() }
///
/// ```ignore
/// unsafe { never_compiled() }
/// ```
///
/// ```ignore-windows
/// unsafe { compiled_elsewhere() }
/// ```
fn item() {}
";
        assert_eq!(lines_of(TXN, source, Zone::Safe), [9, 13, 18, 25]);
        assert_eq!(list_item("1. x"), Some(3));
        assert_eq!(list_item("- x"), Some(2));
        assert_eq!(list_item("10) x"), Some(4));
        assert_eq!(list_item("-x"), None);
        assert_eq!(list_item("1.5 apples"), None);
    }

    #[test]
    fn unterminated_files_are_reported_not_passed() {
        let found = audit(TXN, "fn f() {}\n/* unsafe { }", Zone::Safe);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].location, "crates/txn/src/lib.rs:2");
        assert!(found[0].message.contains("block comment"));
        let found = audit(
            TXN,
            "/// ```\n/// let s = \"x;\n/// ```\nfn f() {}\n",
            Zone::Safe,
        );
        assert_eq!(found.len(), 1);
        assert!(found[0].message.contains("doc example"), "{found:?}");
    }

    #[test]
    fn files_belong_to_the_closest_member() {
        let members = [
            ("crates/platform".to_owned(), "softshell-platform"),
            ("crates/platform/fault".to_owned(), "nested"),
            ("crates/sys".to_owned(), "softshell-sys"),
        ];
        assert_eq!(
            owner(&members, "crates/platform/src/lib.rs"),
            Some("softshell-platform")
        );
        assert_eq!(
            owner(&members, "crates/platform/fault/src/lib.rs"),
            Some("nested")
        );
        assert_eq!(owner(&members, "crates/sysutil/src/lib.rs"), None);
        assert_eq!(owner(&members, "build.rs"), None);
    }

    #[test]
    fn targets_and_editions() {
        let m = metadata(
            &[
                package("good", "crates/good", None, &[]),
                package_with(
                    "bad",
                    "0.1.0",
                    "crates/bad",
                    "null",
                    "2021",
                    &[
                        target("lib", "crates/bad/src/core.txt", "2024"),
                        target("bin", "crates/other/src/main.rs", "2024"),
                        target("test", "crates/bad/tests/t.rs", "2018"),
                    ],
                    &[],
                ),
            ],
            &["good-id", "bad-id"],
        );
        let messages: Vec<_> = check_targets(&m).iter().map(ToString::to_string).collect();
        assert_eq!(messages.len(), 4, "{messages:#?}");
        assert!(messages[0].starts_with("crates/bad/Cargo.toml: `bad` is on edition 2021"));
        assert!(messages[1].contains("`crates/bad/src/core.txt` must be a `.rs` file"));
        assert!(
            messages[2].contains(
                "`crates/other/src/main.rs` must be a `.rs` file inside `bad`'s directory"
            )
        );
        assert!(messages[3].contains("the target `crates/bad/tests/t.rs` is on edition 2018"));
    }

    #[test]
    fn lints() {
        let ok = "[package]\nname = \"a\"\n\n[lints]\nworkspace = true\n";
        assert_eq!(check_member_lints("a/Cargo.toml", ok), []);
        assert_eq!(
            check_member_lints(
                "a/Cargo.toml",
                "lints = { workspace = true }\n[package]\nname = \"a\"\n"
            ),
            []
        );
        let none = "[package]\nname = \"a\"\n";
        assert_eq!(check_member_lints("a/Cargo.toml", none).len(), 1);
        let own = "[lints.rust]\nunsafe_code = \"allow\"\n";
        let found = check_member_lints("a/Cargo.toml", own);
        assert_eq!(found.len(), 3, "{found:?}");
        assert_eq!(
            check_member_lints("a/Cargo.toml", "[lints]\nworkspace = false\n").len(),
            1
        );
        assert_eq!(
            check_workspace_lints("Cargo.toml", include_str!("../../Cargo.toml")),
            None
        );
        let weakened = "[workspace.lints.rust]\nunsafe_code = \"warn\"\n";
        assert!(check_workspace_lints("Cargo.toml", weakened).is_some());
        let table = "[workspace.lints.rust]\nunsafe_code = { level = \"deny\", priority = 1 }\n";
        assert_eq!(check_workspace_lints("Cargo.toml", table), None);
        for manifest in [
            include_str!("../Cargo.toml"),
            include_str!("../../crates/core/Cargo.toml"),
        ] {
            assert_eq!(check_member_lints("x/Cargo.toml", manifest), []);
        }
    }
}
