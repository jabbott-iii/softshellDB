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

//! `cargo xtask deps`: nothing from outside the workspace is a dependency
//! unless the user approved it (ADR-0002, `docs/02-dependency-policy.md`).
//!
//! The approvals are [`ALLOWLIST`]'s lines, `name version ADR-NNNN`, each
//! citing an ADR that `docs/decisions.md` records as `Accepted` (a
//! consistency check: the review of the change is what keeps an approval
//! honest). The check reports:
//!
//! - in the members' manifests (`cargo metadata --no-deps`), every declared
//!   dependency, of every kind and platform, that is not a path dependency
//!   on a member, or an approved crate from crates.io required at exactly an
//!   approved version (`= x.y.z`);
//! - in `Cargo.lock` (`cargo metadata --all-features`, which lists optional
//!   dependencies too), every package that is not a member, or an approved
//!   name and version from crates.io: so a path package outside the
//!   workspace (vendored code, a `[patch]` to a local copy) and an approved
//!   name from git or another registry are findings too. A package already
//!   reported in a manifest is not reported again;
//! - a member that is not a crate of the dependency matrix: third-party
//!   code made a workspace member;
//! - a Cargo manifest that is not the root's or a member's (a separate
//!   workspace, or a package the root excludes), whose dependencies nothing
//!   else checks, and any `Cargo.lock` but the root's;
//! - submodules, which hold code from elsewhere;
//! - in the root `Cargo.toml`, `patch` and `replace`, and in
//!   `.cargo/config.toml`, `source`, `patch`, `registries`, `paths` and
//!   `include`, however they are spelled (headers, dotted keys, inline
//!   tables), since they change where a crate's code comes from without
//!   changing its name. These are not allowed at all: there is no approval
//!   for them.

use std::collections::BTreeSet;

use softshell_core::error::Chain;

use crate::check::Finding;
use crate::error::XtaskError;
use crate::layers::MATRIX;
use crate::metadata::{Dependency, Metadata, Package, Resolve};
use crate::toml_keys;
use crate::unsafe_audit::DATA_DIRS;
use crate::workspace::{Files, Workspace};

/// The approved third-party packages, one `name version ADR-NNNN` per line.
pub(crate) const ALLOWLIST: &str = "xtask/deps-allowlist.txt";

/// The decisions file, where an approval's ADR must be `Accepted`.
const DECISIONS: &str = "docs/decisions.md";

/// crates.io, as cargo writes its source, git index and sparse index.
const CRATES_IO: [&str; 2] = [
    "registry+https://github.com/rust-lang/crates.io-index",
    "sparse+https://index.crates.io/",
];

/// The cargo configuration files that can redirect where code comes from,
/// and the tables that do it.
const REDIRECTS: [(&str, &[&str]); 3] = [
    ("Cargo.toml", &["patch", "replace"]),
    (
        ".cargo/config.toml",
        &["source", "patch", "registries", "paths", "include"],
    ),
    (
        ".cargo/config",
        &["source", "patch", "registries", "paths", "include"],
    ),
];

/// One approved package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    /// The package name.
    pub(crate) name: String,
    /// Its exact version.
    pub(crate) version: String,
    /// The line of the allowlist, from 1.
    pub(crate) line: usize,
}

/// Runs the check over the workspace.
///
/// # Errors
///
/// [`XtaskError`] if the allowlist, the decisions or the files cannot be
/// read, or the manifests cannot be listed. A failure to read `Cargo.lock`'s
/// resolution is a finding.
pub(crate) fn run(workspace: &Workspace) -> Result<Vec<Finding>, XtaskError> {
    let allowlist = workspace.read_text(ALLOWLIST)?;
    let decisions = workspace.read_text(DECISIONS)?;
    let (entries, mut findings) = parse_allowlist(&allowlist, &decisions);
    let manifests = Metadata::load(workspace, Resolve::No)?;
    findings.extend(check_members(&manifests));
    let in_manifests = check_manifests(&manifests, &entries);
    let reported: Vec<String> = in_manifests.iter().map(|(name, _)| name.clone()).collect();
    findings.extend(in_manifests.into_iter().map(|(_, finding)| finding));
    match Metadata::load(workspace, Resolve::Yes) {
        Ok(resolved) => findings.extend(check_resolution(&resolved, &entries, &reported)),
        Err(err) => findings.push(Finding::new(
            "Cargo.lock",
            format!(
                "cannot read the resolved dependency graph: {}",
                Chain::new(&err)
            ),
        )),
    }
    match workspace.files() {
        Ok(files) => findings.extend(check_files(&files, &manifests)),
        Err(err) => findings.push(Finding::new(
            "",
            format!(
                "cannot look for submodules and other manifests: {}",
                Chain::new(&err)
            ),
        )),
    }
    for (path, tables) in REDIRECTS {
        if let Some(text) = workspace.read_text_if_present(path)? {
            findings.extend(check_redirects(path, &text, tables));
        }
    }
    Ok(findings)
}

/// Findings for files that bring in code nothing else checks: submodules,
/// manifests that are not the root's or a member's, and other lock files.
pub(crate) fn check_files(files: &Files, metadata: &Metadata) -> Vec<Finding> {
    let mut findings = Vec::new();
    for gitlink in &files.gitlinks {
        findings.push(Finding::new(
            gitlink,
            "is a Git submodule: code from elsewhere, which is not allowed (docs/02-dependency-policy.md)",
        ));
    }
    let manifests: Vec<String> = metadata
        .members()
        .filter_map(|p| metadata.relative(&p.manifest))
        .collect();
    for path in &files.files {
        if DATA_DIRS.iter().any(|dir| path.starts_with(dir)) {
            continue;
        }
        let name = path.rsplit('/').next().unwrap_or(path);
        let message = match name {
            ".gitmodules" => {
                "declares Git submodules: code from elsewhere, which is not allowed (docs/02-dependency-policy.md)"
            }
            "Cargo.toml" if path != "Cargo.toml" && !manifests.contains(path) => {
                "is a Cargo manifest outside the workspace's members, whose dependencies no check reads; make it a member or remove it (docs/02-dependency-policy.md)"
            }
            "Cargo.lock" if path != "Cargo.lock" => {
                "is a lock file outside the workspace's own, whose packages no check reads (docs/02-dependency-policy.md)"
            }
            _ => continue,
        };
        findings.push(Finding::new(path, message));
    }
    findings
}

/// The valid entries of the allowlist `text`, and a finding for each line
/// that is not one. `decisions` is the text of `docs/decisions.md`.
pub(crate) fn parse_allowlist(text: &str, decisions: &str) -> (Vec<Entry>, Vec<Finding>) {
    let accepted = accepted_adrs(decisions);
    let mut entries: Vec<Entry> = Vec::new();
    let mut findings = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let content = raw.split('#').next().unwrap_or("").trim();
        if content.is_empty() {
            continue;
        }
        let problem = |message: String| Finding::at_line(ALLOWLIST, line, message);
        let fields: Vec<&str> = content.split_whitespace().collect();
        let [name, version, adr] = fields[..] else {
            findings.push(problem("expected `name version ADR-NNNN`".to_owned()));
            continue;
        };
        if !is_package_name(name) {
            findings.push(problem(format!("`{name}` is not a package name")));
        } else if !is_exact_version(version) {
            findings.push(problem(format!(
                "`{version}` is not an exact version such as `1.2.3`"
            )));
        } else if !is_adr(adr) {
            findings.push(problem(format!(
                "`{adr}` is not an ADR number such as `ADR-0023`"
            )));
        } else if !accepted.contains(adr) {
            findings.push(problem(format!(
                "{adr} is not `Accepted` in {DECISIONS}; only the user approves a dependency"
            )));
        } else if entries
            .iter()
            .any(|e| e.name == name && e.version == version)
        {
            findings.push(problem(format!("`{name} {version}` is listed twice")));
        } else {
            entries.push(Entry {
                name: name.to_owned(),
                version: version.to_owned(),
                line,
            });
        }
    }
    (entries, findings)
}

/// The ADRs `docs/decisions.md` records as `Accepted`.
fn accepted_adrs(decisions: &str) -> BTreeSet<String> {
    let mut accepted = BTreeSet::new();
    let mut current: Option<&str> = None;
    for line in decisions.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            current = rest.split_whitespace().next().filter(|id| is_adr(id));
        } else if let Some(status) = line.strip_prefix("- **Status:** ")
            && let Some(id) = current.take()
            && status.starts_with("Accepted")
        {
            accepted.insert(id.to_owned());
        }
    }
    accepted
}

fn is_adr(text: &str) -> bool {
    text.strip_prefix("ADR-")
        .is_some_and(|n| n.len() == 4 && n.bytes().all(|b| b.is_ascii_digit()))
}

fn is_package_name(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// `MAJOR.MINOR.PATCH`, with an optional `-pre-release` and `+build`.
fn is_exact_version(text: &str) -> bool {
    let (core, rest) = match text.find(['-', '+']) {
        Some(at) => text.split_at(at),
        None => (text, ""),
    };
    let numbers: Vec<&str> = core.split('.').collect();
    let numeric = |n: &&str| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit());
    numbers.len() == 3
        && numbers.iter().all(numeric)
        && rest.get(1..).is_none_or(|r| {
            !r.is_empty()
                && r.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".-+".contains(&b))
        })
}

/// Findings for members that are not crates of the dependency matrix.
pub(crate) fn check_members(metadata: &Metadata) -> Vec<Finding> {
    metadata
        .members()
        .filter(|p| !MATRIX.iter().any(|(name, _)| *name == p.name))
        .map(|p| {
            Finding::new(
                metadata.display(&p.manifest),
                format!(
                    "`{}` is a workspace member but not a crate of the dependency matrix (docs/01-architecture.md); third-party code made a member is a dependency too (docs/02-dependency-policy.md)",
                    p.name
                ),
            )
        })
        .collect()
}

/// The manifest pass: for every member's declared dependency that is not
/// allowed, the dependency's name and the finding.
pub(crate) fn check_manifests(metadata: &Metadata, entries: &[Entry]) -> Vec<(String, Finding)> {
    let mut findings = Vec::new();
    for package in metadata.members() {
        let manifest = metadata.display(&package.manifest);
        for dependency in &package.dependencies {
            if let Some(message) = manifest_problem(metadata, dependency, entries) {
                findings.push((
                    dependency.name.clone(),
                    Finding::new(&manifest, format!("{}: {message}", dependency.table())),
                ));
            }
        }
    }
    findings
}

fn manifest_problem(
    metadata: &Metadata,
    dependency: &Dependency,
    entries: &[Entry],
) -> Option<String> {
    let name = &dependency.name;
    if let Some(dir) = &dependency.path
        && metadata.member_at(dir).is_some()
    {
        return None;
    }
    let from = match (&dependency.path, &dependency.source, &dependency.registry) {
        (Some(dir), _, _) => format!(
            "is a path dependency outside the workspace ({})",
            metadata.display(dir)
        ),
        // crates.io, however the manifest names it (`registry = "crates-io"`
        // fills in `registry` too).
        (None, Some(source), _) if CRATES_IO.contains(&source.as_str()) => {
            let approved: Vec<&str> = entries
                .iter()
                .filter(|e| e.name == *name)
                .map(|e| e.version.as_str())
                .collect();
            if approved.is_empty() {
                "comes from crates.io".to_owned()
            } else if dependency.req.strip_prefix('=').is_some_and(|v| {
                // Cargo drops build metadata from a requirement.
                approved
                    .iter()
                    .any(|a| a.split('+').next() == Some(v.trim()))
            }) {
                return None;
            } else {
                return Some(format!(
                    "`{name}` is approved only at an exact version: write `= {}`, not `{}`",
                    approved.join("` or `= "),
                    dependency.req
                ));
            }
        }
        (None, Some(source), _) if source.starts_with("git+") => {
            format!("comes from git ({source})")
        }
        (None, _, Some(registry)) => format!("comes from the registry `{registry}`"),
        (None, Some(source), None) => format!("comes from {source}"),
        (None, None, None) => "is not a workspace crate".to_owned(),
    };
    Some(format!(
        "`{name}` {from} and is not an approved crate from crates.io in {ALLOWLIST}; a third-party crate needs the user's approval (docs/02-dependency-policy.md)"
    ))
}

/// The resolution pass: findings for every package in `Cargo.lock` that is
/// not a member or an approved crate from crates.io, except those named in
/// `reported` (already reported in a manifest).
pub(crate) fn check_resolution(
    metadata: &Metadata,
    entries: &[Entry],
    reported: &[String],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for package in &metadata.packages {
        if metadata.is_member(package) || reported.contains(&package.name) {
            continue;
        }
        let Package {
            name,
            version,
            source,
            ..
        } = package;
        let message = match source {
            None => format!(
                "`{name} {version}` is a path package outside the workspace ({}); vendored or copied code is a dependency too (docs/02-dependency-policy.md)",
                metadata.display(&package.dir)
            ),
            Some(source)
                if CRATES_IO.contains(&source.as_str())
                    && entries
                        .iter()
                        .any(|e| e.name == *name && e.version == *version) =>
            {
                continue;
            }
            Some(source) => format!(
                "`{name} {version}` from {source} is not an approved crate from crates.io in {ALLOWLIST}; a third-party crate needs the user's approval (docs/02-dependency-policy.md)"
            ),
        };
        findings.push(Finding::new("Cargo.lock", message));
    }
    findings
}

/// Findings for the keys in `text` (the file at `path`) under `tables`,
/// which redirect where code comes from: `[patch.crates-io]`,
/// `source.x.directory = …`, `source = { … }`.
fn check_redirects(path: &str, text: &str, tables: &[&str]) -> Vec<Finding> {
    let keys = match toml_keys::keys(text) {
        Ok(keys) => keys,
        Err(err) => {
            return vec![Finding::at_line(
                path,
                err.line,
                format!("cannot read this TOML: a {} never ends", err.what),
            )];
        }
    };
    let mut findings = Vec::new();
    let mut seen = Vec::new();
    for key in keys {
        let Some(table) = key.path.first().filter(|t| tables.contains(&t.as_str())) else {
            continue;
        };
        if seen.contains(&key.line) {
            continue;
        }
        seen.push(key.line);
        findings.push(Finding::at_line(
            path,
            key.line,
            format!(
                "`{table}` changes where cargo gets a crate's code or configuration, which is not allowed (docs/02-dependency-policy.md)"
            ),
        ));
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::tests::{
        dependency, dependency_with, metadata, package, package_with, registry_package, target,
    };

    const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";
    const SPARSE: &str = "sparse+https://index.crates.io/";

    const DECISIONS_TEXT: &str = "\
```markdown
## ADR-NNNN — <title>

- **Status:** Proposed | Accepted | Superseded by ADR-NNNN
```

## ADR-0001 — One engine

- **Status:** Accepted (user decision, 2026-11-01)

## ADR-0030 — Use some crate

- **Status:** Proposed

## ADR-0031 — Use another crate

- **Status:** Accepted (user decision, 2026-12-01)
";

    fn entries(text: &str) -> Vec<Entry> {
        let (entries, findings) = parse_allowlist(text, DECISIONS_TEXT);
        assert_eq!(findings, []);
        entries
    }

    #[test]
    fn the_shipped_allowlist_is_valid() {
        // Valid, not empty: an approval the policy's way must not fail a
        // test (`cargo xtask deps` reports a bad line in any case).
        let text = include_str!("../deps-allowlist.txt");
        let decisions = include_str!("../../docs/decisions.md");
        assert_eq!(parse_allowlist(text, decisions).1, []);
        let crlf = text.replace('\n', "\r\n");
        assert_eq!(parse_allowlist(&crlf, decisions).1, []);
    }

    #[test]
    fn accepted_adrs_are_read_from_the_real_decisions() {
        let decisions = include_str!("../../docs/decisions.md");
        for text in [decisions.to_owned(), decisions.replace('\n', "\r\n")] {
            let accepted = accepted_adrs(&text);
            assert!(accepted.contains("ADR-0002"));
            assert!(accepted.contains("ADR-0022"));
            // The template, in a code fence, is no ADR.
            assert!(accepted.iter().all(|id| is_adr(id)));
        }
        let accepted = accepted_adrs(DECISIONS_TEXT);
        assert_eq!(
            accepted.into_iter().collect::<Vec<_>>(),
            ["ADR-0001", "ADR-0031"]
        );
    }

    #[test]
    fn allowlist_lines() {
        let text = "\
# A comment.

foo 1.2.3 ADR-0031  # approved in December
bar 0.4.0-rc.1+build.5 ADR-0031
foo 1.2.3 ADR-0031
baz 1.2 ADR-0031
qux 1.2.3 ADR-0030
quux 1.2.3 ADR-0099
no-adr 1.0.0
bad/name 1.0.0 ADR-0031
x 1.0.0 adr-31
";
        let (entries, findings) = parse_allowlist(text, DECISIONS_TEXT);
        let names: Vec<_> = entries.iter().map(|e| (e.name.as_str(), e.line)).collect();
        assert_eq!(names, [("foo", 3), ("bar", 4)]);
        let lines: Vec<_> = findings.iter().map(|f| f.location.as_str()).collect();
        assert_eq!(
            lines,
            [5, 6, 7, 8, 9, 10, 11].map(|l| format!("{ALLOWLIST}:{l}"))
        );
        assert!(findings[0].message.contains("listed twice"));
        assert!(findings[1].message.contains("not an exact version"));
        assert!(findings[2].message.contains("ADR-0030 is not `Accepted`"));
        assert!(findings[3].message.contains("ADR-0099 is not `Accepted`"));
        assert!(
            findings[4]
                .message
                .contains("expected `name version ADR-NNNN`")
        );
        assert!(findings[5].message.contains("not a package name"));
        assert!(findings[6].message.contains("not an ADR number"));
    }

    #[test]
    fn manifests_may_declare_only_members_and_pinned_approvals_from_crates_io() {
        let m = metadata(
            &[
                package(
                    "softshell-txn",
                    "crates/txn",
                    None,
                    &[
                        dependency("softshell-core", None, Some("crates/core"), None, "^0.1.0"),
                        dependency("serde", None, None, Some(CRATES_IO), "^1"),
                        dependency("tokio", Some("dev"), None, Some(CRATES_IO), "^1"),
                        dependency("cc", Some("build"), None, Some(CRATES_IO), "^1"),
                        dependency("local", None, Some("vendor/local"), None, "*"),
                        dependency("gitdep", None, None, Some("git+https://example.com/x"), "*"),
                        dependency("foo", None, None, Some(CRATES_IO), "=1.2.3"),
                        dependency("foo", Some("dev"), None, Some(SPARSE), "= 1.2.3"),
                        dependency("bar", None, None, Some(CRATES_IO), "^0.4.0"),
                        dependency(
                            "foo",
                            None,
                            None,
                            Some("git+https://example.com/fork"),
                            "=1.2.3",
                        ),
                        dependency_with(
                            "foo",
                            None,
                            None,
                            Some("registry+https://example.com/index"),
                            "=1.2.3",
                            &[],
                            "\"https://example.com/index\"",
                        ),
                    ],
                ),
                package("softshell-core", "crates/core", None, &[]),
            ],
            &["softshell-txn-id", "softshell-core-id"],
        );
        let approved = entries("foo 1.2.3 ADR-0031\nbar 0.4.0 ADR-0031\n");
        let found = check_manifests(&m, &approved);
        let names: Vec<_> = found.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            [
                "serde", "tokio", "cc", "local", "gitdep", "bar", "foo", "foo"
            ]
        );
        let messages: Vec<_> = found.iter().map(|(_, f)| f.to_string()).collect();
        assert_eq!(
            messages[0],
            "crates/txn/Cargo.toml: [dependencies]: `serde` comes from crates.io and is not an approved crate from crates.io in xtask/deps-allowlist.txt; a third-party crate needs the user's approval (docs/02-dependency-policy.md)"
        );
        assert!(messages[1].contains("[dev-dependencies]: `tokio`"));
        assert!(messages[2].contains("[build-dependencies]: `cc`"));
        assert!(
            messages[3]
                .contains("`local` is a path dependency outside the workspace (vendor/local)")
        );
        assert!(messages[4].contains("`gitdep` comes from git"));
        assert!(
            messages[5].contains(
                "`bar` is approved only at an exact version: write `= 0.4.0`, not `^0.4.0`"
            )
        );
        assert!(messages[6].contains("`foo` comes from git (git+https://example.com/fork)"));
        assert!(messages[7].contains("`foo` comes from the registry `https://example.com/index`"));
    }

    #[test]
    fn a_platform_dependency_names_its_platform() {
        let mut dep = dependency("winapi", None, None, Some(CRATES_IO), "^0.3");
        dep = dep.replace(r#""target": null"#, r#""target": "cfg(windows)""#);
        let m = metadata(
            &[package("softshell-sys", "crates/sys", None, &[dep])],
            &["softshell-sys-id"],
        );
        let found = check_manifests(&m, &[]);
        assert!(
            found[0]
                .1
                .message
                .starts_with("[dependencies] for `cfg(windows)`: `winapi`")
        );
    }

    #[test]
    fn the_resolution_may_hold_only_members_and_approvals_from_crates_io() {
        let fork = package_with(
            "foo",
            "1.2.4",
            "git/foo",
            "\"git+https://example.com/fork#abc\"",
            "2021",
            &[target("lib", "git/foo/src/lib.rs", "2021")],
            &[],
        );
        let m = metadata(
            &[
                package("softshell-core", "crates/core", None, &[]),
                registry_package("foo", "1.2.3"),
                registry_package("serde", "1.0.0"),
                registry_package("already", "1.0.0"),
                package("vendored", "vendor/vendored", None, &[]),
                fork,
            ],
            &["softshell-core-id"],
        );
        let approved = entries("foo 1.2.3 ADR-0031\nfoo 1.2.4 ADR-0031\nstale 2.0.0 ADR-0031\n");
        let messages: Vec<_> = check_resolution(&m, &approved, &["already".to_owned()])
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(messages.len(), 3, "{messages:#?}");
        assert!(messages[0].starts_with("Cargo.lock: `serde 1.0.0` from registry+"));
        assert!(messages[1].contains(
            "`vendored 0.1.0` is a path package outside the workspace (vendor/vendored)"
        ));
        assert!(messages[2].starts_with("Cargo.lock: `foo 1.2.4` from git+https://example.com/fork#abc is not an approved crate from crates.io"));
    }

    #[test]
    fn members_outside_the_matrix_are_found() {
        let m = metadata(
            &[
                package("softshell-core", "crates/core", None, &[]),
                package("leftpad", "vendor/leftpad", None, &[]),
            ],
            &["softshell-core-id", "leftpad-id"],
        );
        let found = check_members(&m);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].location, "vendor/leftpad/Cargo.toml");
        assert!(
            found[0].message.contains(
                "`leftpad` is a workspace member but not a crate of the dependency matrix"
            )
        );
    }

    #[test]
    fn redirecting_tables_are_found() {
        let text = "[workspace]\n[patch.crates-io]\nserde = { git = \"x\" }\n[replace]\n  [patch.'https://github.com/x']\n[profile.xtask]\n[workspace.dependencies]\n";
        for text in [text.to_owned(), text.replace('\n', "\r\n")] {
            let lines: Vec<_> = check_redirects("Cargo.toml", &text, &["patch", "replace"])
                .iter()
                .map(|f| f.location.clone())
                .collect();
            assert_eq!(
                lines,
                [
                    "Cargo.toml:2",
                    "Cargo.toml:3",
                    "Cargo.toml:4",
                    "Cargo.toml:5"
                ]
            );
        }
        let config_tables = &["source", "patch", "registries", "paths", "include"];
        let config = "\
source.crates-io.replace-with = \"v\"
\"source\" . 'v' . directory = \"vendor\"
[\"\\u0073ource\".x]
paths = [\"../fork\"]
include = [\"more.toml\"]
source = { y = { directory = \"z\" } }
[alias]
xtask = \"run\"
[registries.x]
";
        let lines: Vec<_> = check_redirects(".cargo/config.toml", config, config_tables)
            .iter()
            .map(|f| f.location.clone())
            .collect();
        assert_eq!(
            lines,
            [1, 2, 3, 4, 5, 6, 9].map(|l| format!(".cargo/config.toml:{l}"))
        );
        // The real files redirect nothing.
        assert_eq!(
            check_redirects(
                "Cargo.toml",
                include_str!("../../Cargo.toml"),
                &["patch", "replace"]
            ),
            []
        );
        assert_eq!(
            check_redirects(
                ".cargo/config.toml",
                include_str!("../../.cargo/config.toml"),
                config_tables
            ),
            []
        );
    }

    #[test]
    fn files_that_bring_in_unchecked_code() {
        let m = metadata(
            &[package("softshell-core", "crates/core", None, &[])],
            &["softshell-core-id"],
        );
        let files = Files {
            files: [
                "Cargo.toml",
                "Cargo.lock",
                "crates/core/Cargo.toml",
                "tools/helper/Cargo.toml",
                "tools/helper/Cargo.lock",
                ".gitmodules",
                "tests/fixtures/x/v1/Cargo.toml",
                "docs/Cargo.toml.md",
            ]
            .map(str::to_owned)
            .to_vec(),
            gitlinks: vec!["vendor/sub".to_owned()],
            links: vec![],
        };
        let locations: Vec<_> = check_files(&files, &m)
            .into_iter()
            .map(|f| f.location)
            .collect();
        assert_eq!(
            locations,
            [
                "vendor/sub",
                "tools/helper/Cargo.toml",
                "tools/helper/Cargo.lock",
                ".gitmodules"
            ]
        );
    }

    #[test]
    fn crates_io_named_as_a_registry_is_crates_io() {
        let m = metadata(
            &[package(
                "softshell-sys",
                "crates/sys",
                None,
                &[
                    dependency_with(
                        "foo",
                        None,
                        None,
                        Some(CRATES_IO),
                        "=1.0.0",
                        &[],
                        "\"https://github.com/rust-lang/crates.io-index\"",
                    ),
                    dependency_with(
                        "bar",
                        None,
                        None,
                        Some(SPARSE),
                        "=1.0.0",
                        &[],
                        "\"sparse+https://index.crates.io/\"",
                    ),
                ],
            )],
            &["softshell-sys-id"],
        );
        let approved = entries("foo 1.0.0 ADR-0031\nbar 1.0.0 ADR-0031\n");
        assert_eq!(check_manifests(&m, &approved), []);
    }

    #[test]
    fn approved_versions_with_build_metadata() {
        let m = metadata(
            &[package(
                "softshell-sys",
                "crates/sys",
                None,
                &[dependency("wasi", None, None, Some(CRATES_IO), "=0.11.0")],
            )],
            &["softshell-sys-id"],
        );
        let approved = entries("wasi 0.11.0+wasi-snapshot-preview1 ADR-0031\n");
        assert_eq!(check_manifests(&m, &approved), []);
    }

    #[test]
    fn exact_versions() {
        for good in [
            "0.0.0",
            "1.2.3",
            "10.20.30",
            "1.0.0-alpha.1",
            "1.0.0+build",
            "1.0.0-rc.1+b.2",
        ] {
            assert!(is_exact_version(good), "{good}");
        }
        for bad in [
            "", "1", "1.2", "1.2.3.4", "^1.2.3", "=1.2.3", "1.x.3", "1.2.3-", "1..3", "v1.2.3",
        ] {
            assert!(!is_exact_version(bad), "{bad}");
        }
    }
}
