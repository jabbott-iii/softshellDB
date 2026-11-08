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

//! `cargo xtask headers`: every source file starts with the Apache-2.0
//! header (ADR-0004), exactly, in its comment syntax.
//!
//! Source files are the files Git lists that are Rust (`*.rs`), scripts
//! (`*.sh`, `*.ps1`, `*.py`), GitHub workflows (`.github/workflows/*.yml`
//! and `*.yaml`) or the server's sample `softshell.jsonc`. Everything else
//! is exempt: Markdown, TOML, other JSON and JSONC, other `.github` files,
//! `LICENSE`, `NOTICE`, and the data in the format fixtures and fuzz
//! corpora.
//!
//! The header is the first thing in the file, or follows a shebang line.
//! Lines may end in `\n` or `\r\n` (a Windows checkout).

use crate::check::Finding;
use crate::error::XtaskError;
use crate::lexer;
use crate::unsafe_audit::DATA_DIRS;
use crate::workspace::Workspace;

/// The header's lines, without comment markers: `LICENSE`'s appendix
/// boilerplate with its placeholder filled in (ADR-0004).
pub(crate) const HEADER: [&str; 13] = [
    "Copyright 2026 Joseph Anthony Abbott III",
    "",
    "Licensed under the Apache License, Version 2.0 (the \"License\");",
    "you may not use this file except in compliance with the License.",
    "You may obtain a copy of the License at",
    "",
    "    http://www.apache.org/licenses/LICENSE-2.0",
    "",
    "Unless required by applicable law or agreed to in writing, software",
    "distributed under the License is distributed on an \"AS IS\" BASIS,",
    "WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.",
    "See the License for the specific language governing permissions and",
    "limitations under the License.",
];

/// A file format's line-comment marker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Style {
    /// `//`: Rust, JSONC.
    Slashes,
    /// `#`: shell, PowerShell, Python, YAML.
    Hash,
}

impl Style {
    const fn marker(self) -> &'static str {
        match self {
            Self::Slashes => "//",
            Self::Hash => "#",
        }
    }

    /// Header line `text` as this style writes it: blank lines carry the
    /// marker alone, with no trailing space.
    fn line(self, text: &str) -> String {
        if text.is_empty() {
            self.marker().to_owned()
        } else {
            format!("{} {text}", self.marker())
        }
    }
}

/// Runs the check over the workspace.
///
/// # Errors
///
/// [`XtaskError`] if the files cannot be listed or read.
pub(crate) fn run(workspace: &Workspace) -> Result<Vec<Finding>, XtaskError> {
    let mut findings = Vec::new();
    for path in workspace.files()?.files {
        let Some(style) = style_of(&path) else {
            continue;
        };
        let bytes = std::fs::read(workspace.path(&path)).map_err(|source| XtaskError::Io {
            doing: format!("reading {path}"),
            source,
        })?;
        let finding = match std::str::from_utf8(&bytes) {
            Ok(text) => check(&path, text, style),
            Err(_) => Some(Finding::new(
                &path,
                "is not UTF-8, so its header cannot be checked",
            )),
        };
        findings.extend(finding);
    }
    Ok(findings)
}

/// The comment style of the source file at `path`, or `None` if it is not
/// a source file.
pub(crate) fn style_of(path: &str) -> Option<Style> {
    if DATA_DIRS.iter().any(|dir| path.starts_with(dir)) {
        return None;
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    let extension = name.rsplit_once('.').map(|(_, ext)| ext);
    match extension {
        Some("rs") => Some(Style::Slashes),
        _ if name == "softshell.jsonc" => Some(Style::Slashes),
        Some("sh" | "ps1" | "py") => Some(Style::Hash),
        Some("yml" | "yaml") if path.starts_with(".github/workflows/") => Some(Style::Hash),
        _ => None,
    }
}

/// The problem with `text`'s header, if any.
pub(crate) fn check(path: &str, text: &str, style: Style) -> Option<Finding> {
    if text.starts_with('\u{feff}') {
        return Some(Finding::at_line(
            path,
            1,
            "starts with a byte-order mark; the license header must come first (ADR-0004)",
        ));
    }
    // `lines` drops the empty piece after a final newline.
    let lines: Vec<&str> = text.lines().collect();
    // A Rust file's shebang is what rustc takes as one; a script's is any
    // `#!` line.
    let shebang = match style {
        Style::Slashes => lexer::shebang_len(text).is_some(),
        Style::Hash => text.starts_with("#!"),
    };
    let first = usize::from(shebang);
    let expected: Vec<String> = HEADER.iter().map(|text| style.line(text)).collect();
    let line_at = |i: usize| lines.get(i).copied();
    if line_at(first) != expected.first().map(String::as_str) {
        let found_at = lines
            .iter()
            .position(|line| Some(*line) == expected.first().map(String::as_str));
        let message = match found_at {
            Some(at) => format!(
                "the license header starts on line {}; it must start on line {} (ADR-0004)",
                at + 1,
                first + 1
            ),
            None if line_at(first)
                .is_some_and(|l| l.starts_with(style.marker()) && l.contains("Copyright")) =>
            {
                format!(
                    "the license header's copyright line differs: expected `{}` (ADR-0004)",
                    expected.first().map_or("", String::as_str)
                )
            }
            None => format!(
                "has no license header; it must start with `{}` (ADR-0004)",
                expected.first().map_or("", String::as_str)
            ),
        };
        return Some(Finding::at_line(path, first + 1, message));
    }
    for (i, want) in expected.iter().enumerate().skip(1) {
        let at = first + i;
        match line_at(at) {
            Some(found) if found == want => {}
            Some(found) => {
                return Some(Finding::at_line(
                    path,
                    at + 1,
                    format!(
                        "the license header differs: expected `{want}`, found `{found}` (ADR-0004)"
                    ),
                ));
            }
            None => {
                return Some(Finding::at_line(
                    path,
                    at,
                    format!("the license header ends early: expected `{want}` next (ADR-0004)"),
                ));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(style: Style) -> String {
        HEADER.iter().map(|l| style.line(l) + "\n").collect()
    }

    #[test]
    fn source_files_and_exemptions() {
        for (path, style) in [
            ("crates/core/src/lib.rs", Some(Style::Slashes)),
            ("build.rs", Some(Style::Slashes)),
            ("crates/server/softshell.jsonc", Some(Style::Slashes)),
            ("scripts/release.sh", Some(Style::Hash)),
            ("scripts/sign.ps1", Some(Style::Hash)),
            ("tools/gen.py", Some(Style::Hash)),
            (".github/workflows/ci.yml", Some(Style::Hash)),
            (".github/workflows/codeql.yaml", Some(Style::Hash)),
            (".github/dependabot.yml", None),
            (".github/ISSUE_TEMPLATE/issue.yml", None),
            ("docs/features.jsonc", None),
            ("README.md", None),
            ("Cargo.toml", None),
            ("LICENSE", None),
            ("NOTICE", None),
            ("tests/fixtures/sdoc/v1/a.rs", None),
            ("fuzz/corpus/json/seed.py", None),
            ("crates/core/tests/vectors/sha256/SHA256ShortMsg.rsp", None),
        ] {
            assert_eq!(style_of(path), style, "{path}");
        }
    }

    #[test]
    fn exact_headers_pass() {
        let rust = header(Style::Slashes) + "\n//! Docs.\n";
        assert_eq!(check("a.rs", &rust, Style::Slashes), None);
        let script = format!("#!/bin/sh\n{}\nset -eu\n", header(Style::Hash));
        assert_eq!(check("a.sh", &script, Style::Hash), None);
        let crlf = rust.replace('\n', "\r\n");
        assert_eq!(check("a.rs", &crlf, Style::Slashes), None);
        // Nothing after the header is needed.
        assert_eq!(
            check("a.rs", header(Style::Slashes).trim_end(), Style::Slashes),
            None
        );
    }

    #[test]
    fn wrong_headers_are_found() {
        let good = header(Style::Slashes) + "\nfn main() {}\n";
        let message = |text: &str| {
            check("a.rs", text, Style::Slashes)
                .map(|f| f.to_string())
                .unwrap_or_default()
        };
        assert!(message("fn main() {}\n").starts_with("a.rs:1: has no license header"));
        assert!(message("").starts_with("a.rs:1: has no license header"));
        assert!(
            message(&format!("//! Docs.\n{good}"))
                .contains("starts on line 2; it must start on line 1")
        );
        assert!(message(&format!("\u{feff}{good}")).contains("byte-order mark"));
        assert!(message(&good.replace("2026", "2025")).contains("copyright line differs"));
        assert!(
            message(&good.replace("(the \"License\")", "(the License)"))
                .starts_with("a.rs:3: the license header differs")
        );
        assert!(message(&good.replace("governing ", "governing  ")).starts_with("a.rs:12:"));
        assert!(message(&good.replacen("//\n", "// \n", 1)).starts_with("a.rs:2:"));
        let cut: String = good.lines().take(5).map(|l| format!("{l}\n")).collect();
        assert!(
            message(cut.trim_end()).contains("ends early"),
            "{}",
            message(&cut)
        );
        // The wrong comment style is a missing header.
        let hashed = header(Style::Hash);
        assert!(message(&hashed).contains("has no license header"));
        // `#!` with an attribute after a comment is no shebang to rustc.
        assert!(
            message(&format!("#!/**/[allow(x)]\n{good}"))
                .contains("starts on line 2; it must start on line 1")
        );
        // A shebang only shifts the header in a file that has one.
        assert!(
            message(&format!("#!/bin/sh\n//! x\n{good}"))
                .contains("starts on line 3; it must start on line 2")
        );
    }

    #[test]
    fn the_header_is_licenses_appendix_boilerplate() {
        let license = include_str!("../../LICENSE");
        let start = license
            .find("APPENDIX: How to apply the Apache License to your work.")
            .unwrap();
        let appendix = &license[start..];
        let lines: Vec<&str> = appendix.lines().map(str::trim_end).collect();
        let first = lines
            .iter()
            .position(|l| l.trim_start().starts_with("Copyright "))
            .unwrap();
        let last = lines
            .iter()
            .position(|l| l.trim() == "limitations under the License.")
            .unwrap();
        let boilerplate: Vec<String> = lines[first..=last]
            .iter()
            .map(|l| {
                let l = l.strip_prefix("   ").unwrap_or(l);
                if l.trim_start().starts_with("Copyright [yyyy]") {
                    HEADER[0].to_owned()
                } else {
                    l.to_owned()
                }
            })
            .collect();
        assert_eq!(boilerplate, HEADER);
    }
}
