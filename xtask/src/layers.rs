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

//! `cargo xtask layers`: each crate depends only on the workspace crates
//! its row of the dependency matrix allows (`docs/01-architecture.md`,
//! ADR-0005), counting normal, dev and build dependencies on every
//! platform. A test keeps [`MATRIX`] equal to the document's table.
//! Dependencies from outside the workspace are `deps`'s business.

use crate::check::Finding;
use crate::error::XtaskError;
use crate::metadata::{Metadata, Resolve};
use crate::workspace::Workspace;

/// What a crate may depend on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Allowed {
    /// These workspace crates only.
    Only(&'static [&'static str]),
    /// Any workspace crate.
    Any,
}

/// The dependency matrix, by package name.
pub(crate) const MATRIX: &[(&str, Allowed)] = &[
    ("softshell-core", Allowed::Only(&[])),
    ("softshell-sys", Allowed::Only(&[])),
    (
        "softshell-platform",
        Allowed::Only(&["softshell-core", "softshell-sys"]),
    ),
    ("softshell-codec", Allowed::Only(&["softshell-core"])),
    (
        "softshell-storage",
        Allowed::Only(&["softshell-core", "softshell-platform", "softshell-codec"]),
    ),
    (
        "softshell-expr",
        Allowed::Only(&["softshell-core", "softshell-codec"]),
    ),
    (
        "softshell-proto",
        Allowed::Only(&["softshell-core", "softshell-codec"]),
    ),
    (
        "softshell-txn",
        Allowed::Only(&["softshell-core", "softshell-codec", "softshell-storage"]),
    ),
    (
        "softshell-documents",
        Allowed::Only(&[
            "softshell-core",
            "softshell-codec",
            "softshell-expr",
            "softshell-txn",
        ]),
    ),
    (
        "softshell-graph",
        Allowed::Only(&[
            "softshell-core",
            "softshell-codec",
            "softshell-expr",
            "softshell-txn",
        ]),
    ),
    (
        "softshell-series",
        Allowed::Only(&[
            "softshell-core",
            "softshell-codec",
            "softshell-expr",
            "softshell-txn",
        ]),
    ),
    (
        "softshell-text",
        Allowed::Only(&["softshell-core", "softshell-codec", "softshell-txn"]),
    ),
    (
        "softshell-vector",
        Allowed::Only(&["softshell-core", "softshell-codec", "softshell-txn"]),
    ),
    (
        "softshelldb",
        Allowed::Only(&[
            "softshell-core",
            "softshell-platform",
            "softshell-codec",
            "softshell-storage",
            "softshell-expr",
            "softshell-txn",
            "softshell-documents",
            "softshell-graph",
            "softshell-series",
            "softshell-text",
            "softshell-vector",
        ]),
    ),
    (
        "softshell-server",
        Allowed::Only(&[
            "softshell-core",
            "softshell-codec",
            "softshell-proto",
            "softshelldb",
        ]),
    ),
    (
        "softshell-client",
        Allowed::Only(&["softshell-core", "softshell-codec", "softshell-proto"]),
    ),
    (
        "softshell-cli",
        Allowed::Only(&[
            "softshell-core",
            "softshell-codec",
            "softshelldb",
            "softshell-client",
        ]),
    ),
    ("tidepool", Allowed::Only(&["softshelldb"])),
    (
        "xtask",
        Allowed::Only(&["softshell-core", "softshell-codec"]),
    ),
    ("softshell-bench", Allowed::Any),
];

/// Runs the check over the workspace.
///
/// # Errors
///
/// [`XtaskError`] if cargo fails.
pub(crate) fn run(workspace: &Workspace) -> Result<Vec<Finding>, XtaskError> {
    let metadata = Metadata::load(workspace, Resolve::No)?;
    Ok(check(&metadata))
}

/// The findings for `metadata`'s members.
pub(crate) fn check(metadata: &Metadata) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (name, _) in MATRIX {
        if !metadata.members().any(|p| p.name == *name) {
            findings.push(Finding::new(
                "",
                format!(
                    "the dependency matrix has a row for `{name}`, which is not a workspace member"
                ),
            ));
        }
    }
    for package in metadata.members() {
        let manifest = metadata.display(&package.manifest);
        let Some((_, allowed)) = MATRIX.iter().find(|(name, _)| *name == package.name) else {
            findings.push(Finding::new(
                manifest,
                format!(
                    "`{}` has no row in the dependency matrix (docs/01-architecture.md)",
                    package.name
                ),
            ));
            continue;
        };
        for dependency in &package.dependencies {
            let Some(target) = dependency
                .path
                .as_deref()
                .and_then(|dir| metadata.member_at(dir))
            else {
                continue;
            };
            // A crate's dev-dependency on itself (to turn on its own
            // features in its tests) is no edge between crates.
            if target.name == package.name {
                continue;
            }
            let ok = match allowed {
                Allowed::Any => true,
                Allowed::Only(names) => names.contains(&target.name.as_str()),
            };
            if !ok {
                let row = match allowed {
                    Allowed::Only([]) => "none".to_owned(),
                    Allowed::Only(names) => names.join(", "),
                    Allowed::Any => "any".to_owned(),
                };
                findings.push(Finding::new(
                    manifest.clone(),
                    format!(
                        "`{}` may not depend on `{}` (in {}); its row of the dependency matrix allows: {row} (ADR-0005)",
                        package.name,
                        target.name,
                        dependency.table(),
                    ),
                ));
            }
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::tests::{dependency, metadata, package};

    fn member(name: &str, dir: &str, deps: &[String]) -> String {
        package(name, dir, None, deps)
    }

    /// Every matrix row as a member, with `extra` dependencies on `txn`.
    fn with_txn_deps(extra: &[String]) -> Metadata {
        let mut packages = Vec::new();
        let mut ids = Vec::new();
        for (name, _) in MATRIX {
            let deps = if *name == "softshell-txn" { extra } else { &[] };
            packages.push(member(name, name, deps));
            ids.push(format!("{name}-id"));
        }
        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        metadata(&packages, &ids)
    }

    #[test]
    fn allowed_edges_pass() {
        let m = with_txn_deps(&[
            dependency(
                "softshell-storage",
                None,
                Some("softshell-storage"),
                None,
                "^0.1.0",
            ),
            dependency(
                "softshell-core",
                Some("dev"),
                Some("softshell-core"),
                None,
                "^0.1.0",
            ),
            dependency(
                "outside",
                None,
                None,
                Some("registry+https://github.com/rust-lang/crates.io-index"),
                "^1",
            ),
        ]);
        assert_eq!(check(&m), []);
    }

    #[test]
    fn a_dev_dependency_on_itself_is_no_edge() {
        let m = with_txn_deps(&[dependency(
            "softshell-txn",
            Some("dev"),
            Some("softshell-txn"),
            None,
            "^0.1.0",
        )]);
        assert_eq!(check(&m), []);
    }

    #[test]
    fn forbidden_edges_of_every_kind_are_found() {
        let m = with_txn_deps(&[
            dependency(
                "softshell-documents",
                None,
                Some("softshell-documents"),
                None,
                "^0.1.0",
            ),
            dependency(
                "softshell-platform",
                Some("dev"),
                Some("softshell-platform"),
                None,
                "^0.1.0",
            ),
            dependency(
                "softshelldb",
                Some("build"),
                Some("softshelldb"),
                None,
                "^0.1.0",
            ),
        ]);
        let found = check(&m);
        assert_eq!(found.len(), 3, "{found:?}");
        assert_eq!(found[0].location, "softshell-txn/Cargo.toml");
        assert_eq!(
            found[0].message,
            "`softshell-txn` may not depend on `softshell-documents` (in [dependencies]); its row of the dependency matrix allows: softshell-core, softshell-codec, softshell-storage (ADR-0005)"
        );
        assert!(found[1].message.contains("[dev-dependencies]"));
        assert!(found[2].message.contains("[build-dependencies]"));
    }

    #[test]
    fn a_member_without_a_row_and_a_row_without_a_member_are_found() {
        let m = metadata(
            &[
                member("softshell-core", "crates/core", &[]),
                member("softshell-new", "crates/new", &[]),
            ],
            &["softshell-core-id", "softshell-new-id"],
        );
        let found = check(&m);
        assert!(
            found
                .iter()
                .any(|f| f.message.contains("`softshell-new` has no row"))
        );
        assert!(found.iter().any(|f| {
            f.message
                .contains("row for `softshell-sys`, which is not a workspace member")
        }));
    }

    #[test]
    fn the_matrix_is_the_architecture_documents() {
        let doc = include_str!("../../docs/01-architecture.md");
        let section = doc.split("## Dependency matrix").nth(1).unwrap();
        let mut rows = Vec::new();
        for line in section.lines().skip_while(|l| !l.starts_with('|')) {
            if !line.starts_with('|') {
                break;
            }
            let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
            let crate_name = cells[0].split('`').nth(1);
            let Some(crate_name) = crate_name else {
                continue; // The header and the separator.
            };
            let allowed = if cells[1] == "any workspace crate" {
                None
            } else {
                Some(
                    cells[1]
                        .split(',')
                        .filter_map(|c| c.trim().strip_prefix('`')?.strip_suffix('`'))
                        .map(|short| match short {
                            "softshelldb" => short.to_owned(),
                            _ => format!("softshell-{short}"),
                        })
                        .collect::<Vec<_>>(),
                )
            };
            rows.push((crate_name.to_owned(), allowed));
        }
        let ours: Vec<_> = MATRIX
            .iter()
            .map(|(name, allowed)| {
                let allowed = match allowed {
                    Allowed::Any => None,
                    Allowed::Only(names) => Some(names.iter().map(|n| (*n).to_owned()).collect()),
                };
                ((*name).to_owned(), allowed)
            })
            .collect();
        assert_eq!(ours, rows);
    }
}
