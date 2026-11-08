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

//! `cargo xtask release-no-dev`: a release build of `softshelldb` or
//! `softshell-cli` turns on neither `dev` nor `fault-injection` in any
//! workspace crate (`docs/01-architecture.md`, "Feature flags").
//!
//! - It asks cargo which features a build of each package alone resolves:
//!   `cargo tree -p <package> -e normal,build --target all`. Leaving out dev
//!   edges makes cargo resolve features without dev-dependencies, as a
//!   release build does, and `--target all` covers every platform's
//!   dependencies. Release builds are of one package each (`-p`): building
//!   several at once would unify their features.
//! - Every member, not only those two, turns a feature on only from its
//!   `[features]` table or a dev-dependency: the same `cargo tree`, with
//!   `--no-default-features` so that the member's own features are off,
//!   must show no forbidden feature on any workspace crate. That covers a
//!   normal or build dependency asking for the feature, for a feature that
//!   turns it on, or for a crate whose `default` does.
//! - A feature can also be turned on behind cargo's back, as a `cfg`. In
//!   the members a build script can run (those with a build script or a
//!   library), a string that emits `rustc-cfg` with anything but a plain
//!   name of our own is a finding: `feature="dev"`, `test`, `doc`, a
//!   `target_` name, or text filled in. Strings of every kind count, so do
//!   `concat!` of strings and files read with `include_str!` or
//!   `include_bytes!`; a directive put together at run time is left to
//!   review.
//! - The repository's cargo configuration holds only the `xtask` alias,
//!   exactly as written: anything else (`rustflags`, `[env]`, a `runner`,
//!   `build.rustc`, a `rustc-cfg` override) acts on the builds, and on
//!   `cargo xtask` itself, behind cargo's back. A `.cargo/config` file,
//!   which cargo would read instead of `.cargo/config.toml`, is a finding.
//!   The tables that redirect where code comes from are `deps`'s business.

use crate::check::Finding;
use crate::error::XtaskError;
use crate::lexer::{self, Kind};
use crate::metadata::{Metadata, Resolve};
use crate::toml_keys;
use crate::workspace::Workspace;
use softshell_core::error::Chain;

/// The packages whose release builds are checked.
pub(crate) const RELEASED: [&str; 2] = ["softshelldb", "softshell-cli"];

/// Features no release build may turn on.
pub(crate) const FORBIDDEN: [&str; 2] = ["dev", "fault-injection"];

/// The repository's cargo configuration.
const CONFIG: &str = ".cargo/config.toml";

/// The `xtask` alias, the one setting the configuration may hold.
const ALIAS: &str = "\"run --profile xtask --package xtask --\"";

/// The tables of the configuration `deps` reports.
const REDIRECTS: [&str; 5] = ["source", "patch", "registries", "paths", "include"];

/// Cfg names that rustc or the tools give a meaning; a build script may
/// not set them. Names starting `target_` or `sanitize` too.
const RESERVED_CFGS: [&str; 18] = [
    "feature",
    "test",
    "doc",
    "doctest",
    "miri",
    "rustfmt",
    "clippy",
    "rustdoc",
    "debug_assertions",
    "overflow_checks",
    "ub_checks",
    "contract_checks",
    "unix",
    "windows",
    "proc_macro",
    "panic",
    "relocation_model",
    "fmt_debug",
];

/// The arguments of the `cargo tree` that resolves a build's features.
pub(crate) const TREE: [&str; 11] = [
    "tree",
    "--offline",
    "--locked",
    "--edges",
    "normal,build",
    "--target",
    "all",
    "--prefix",
    "none",
    "--format",
    "{p}\t{f}",
];

/// How a package's tree is resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Build {
    /// A release build: default features.
    Release,
    /// The member's own features off, to see what its dependencies turn on.
    Dependencies,
}

/// Runs the check.
///
/// # Errors
///
/// [`XtaskError`] if cargo fails or a file cannot be read.
pub(crate) fn run(workspace: &Workspace) -> Result<Vec<Finding>, XtaskError> {
    let metadata = Metadata::load(workspace, Resolve::No)?;
    let members: Vec<&str> = metadata.members().map(|p| p.name.as_str()).collect();
    let mut findings = Vec::new();
    let mut builds: Vec<(&str, Build, String)> = Vec::new();
    for package in RELEASED {
        builds.push((package, Build::Release, String::new()));
    }
    for package in metadata.members() {
        let manifest = metadata.display(&package.manifest);
        builds.push((package.name.as_str(), Build::Dependencies, manifest));
    }
    for (package, build, location) in builds {
        let mut command = workspace.cargo();
        command.args(TREE).args(["--package", package]);
        if build == Build::Dependencies {
            command.arg("--no-default-features");
        }
        let tree = workspace.output(command)?;
        findings.extend(check_tree(package, build, &location, &tree, &members)?);
    }
    // The members a build script can run: those with one, and libraries,
    // which another member's build script can call.
    let scanned: Vec<String> = metadata
        .members()
        .filter(|p| {
            p.targets.iter().any(|t| {
                t.kinds
                    .iter()
                    .any(|k| k == "custom-build" || k.contains("lib") || k == "proc-macro")
            })
        })
        .filter_map(|p| metadata.relative(&p.dir))
        .collect();
    match workspace.files() {
        Ok(files) => {
            for path in files.files {
                let in_member = scanned
                    .iter()
                    .any(|dir| path.starts_with(&format!("{dir}/")));
                if in_member && path.ends_with(".rs") {
                    let text = workspace.read_text(&path)?;
                    let include = |relative: &str| workspace.read_text(relative).ok();
                    findings.extend(check_build_source(&path, &text, include));
                }
            }
        }
        Err(err) => findings.push(Finding::new(
            "",
            format!(
                "cannot list the files build scripts can run: {}",
                Chain::new(&err)
            ),
        )),
    }
    if let Some(text) = workspace.read_text_if_present(CONFIG)? {
        findings.extend(check_config(CONFIG, &text));
    }
    if workspace.path(".cargo/config").exists() {
        findings.push(Finding::new(
            ".cargo/config",
            "cargo reads this file instead of .cargo/config.toml; the configuration lives in .cargo/config.toml only",
        ));
    }
    Ok(findings)
}

/// The findings in `tree`, the output of [`TREE`] for `package` built as
/// `build`; `members` names the workspace's crates, the only ones whose
/// features count, and `location` is where a [`Build::Dependencies`]
/// finding points.
///
/// # Errors
///
/// [`XtaskError::Shape`] if `tree` does not start with `package`, or has a
/// line that is not `<package>\t<features>`.
pub(crate) fn check_tree(
    package: &str,
    build: Build,
    location: &str,
    tree: &str,
    members: &[&str],
) -> Result<Vec<Finding>, XtaskError> {
    let shape = |detail: String| XtaskError::Shape {
        what: format!("the output of cargo tree for {package}"),
        detail,
    };
    let starts_right = tree
        .lines()
        .next()
        .is_some_and(|line| line.starts_with(&format!("{package} v")));
    if !starts_right {
        return Err(shape(format!("it does not start with `{package} v…`")));
    }
    let mut findings = Vec::new();
    for line in tree.lines().filter(|l| !l.trim().is_empty()) {
        let (spec, features) = line
            .split_once('\t')
            .ok_or_else(|| shape(format!("the line `{line}` has no tab")))?;
        let name = spec.split_whitespace().next().unwrap_or(spec);
        if !members.contains(&name) {
            continue;
        }
        // A package printed before is marked ` (*)`.
        let features = features.trim_end().trim_end_matches("(*)").trim_end();
        for feature in features.split(',').map(str::trim) {
            if !FORBIDDEN.contains(&feature) {
                continue;
            }
            let finding = match build {
                Build::Release => Finding::new(
                    "",
                    format!(
                        "a release build of `{package}` turns on feature `{feature}` of `{name}`; \
                         `cargo tree -p {package} -e features,no-dev --target all -i {name}` shows why"
                    ),
                ),
                Build::Dependencies => Finding::new(
                    location,
                    format!(
                        "the normal and build dependencies of `{package}` turn on feature `{feature}` of `{name}`; \
                         turn it on only from [features] or [dev-dependencies] \
                         (`cargo tree -p {package} --no-default-features -e features,no-dev --target all -i {name}` shows how)"
                    ),
                ),
            };
            if !findings.contains(&finding) {
                findings.push(finding);
            }
        }
    }
    Ok(findings)
}

/// Findings for `rustc-cfg` directives that set more than a plain name of
/// our own, in `text` (the file at `path`): in its string literals of every
/// kind, in `concat!` of string literals, and in the files it reads with
/// `include_str!` or `include_bytes!` (`include` returns a file's text,
/// given its path relative to the workspace root).
pub(crate) fn check_build_source(
    path: &str,
    text: &str,
    include: impl Fn(&str) -> Option<String>,
) -> Vec<Finding> {
    let Ok(lexed) = lexer::lex(text) else {
        return Vec::new(); // unsafe-audit reports a file it cannot scan.
    };
    let tokens = &lexed.tokens;
    let mut findings = Vec::new();
    let mut flag = |line: usize, message: &str| {
        let finding = Finding::at_line(path, line, message);
        if !findings.contains(&finding) {
            findings.push(finding);
        }
    };
    let directory = path.rsplit_once('/').map_or("", |(dir, _)| dir);
    for (i, token) in tokens.iter().enumerate() {
        let macro_args = || {
            let is = |n: usize, text: &str| tokens.get(i + n).is_some_and(|t| t.text == text);
            (is(1, "!") && is(2, "(")).then(|| {
                tokens
                    .iter()
                    .skip(i + 3)
                    .take_while(|t| t.text != ")")
                    .filter(|t| t.kind == Kind::Literal)
                    .filter_map(|t| lexer::string_value(t.text))
                    .collect::<Vec<_>>()
            })
        };
        if token.kind == Kind::Literal {
            if lexer::string_value(token.text).is_some_and(|v| sets_a_cfg_of_its_own(&v)) {
                flag(token.line, DIRECTIVE_RULE);
            }
            continue;
        }
        match token.text {
            "concat" => {
                if macro_args().is_some_and(|parts| sets_a_cfg_of_its_own(&parts.concat())) {
                    flag(token.line, DIRECTIVE_RULE);
                }
            }
            "include_str" | "include_bytes" => {
                let Some(file) = macro_args().and_then(|args| args.into_iter().next()) else {
                    continue;
                };
                let relative = if directory.is_empty() {
                    file
                } else {
                    format!("{directory}/{file}")
                };
                if include(&relative).is_some_and(|included| sets_a_cfg_of_its_own(&included)) {
                    flag(
                        token.line,
                        "the file this includes has a `rustc-cfg` directive that sets more than a plain name of our own, a cfg cargo does not see",
                    );
                }
            }
            _ => {}
        }
    }
    findings
}

const DIRECTIVE_RULE: &str = "a build script may emit `rustc-cfg` only with a plain name of our own: a feature, a name rustc gives a meaning (`test`, `doc`, `target_…`), or text filled in turns on a cfg cargo does not see";

/// Whether `text` has a `rustc-cfg` directive, on any of its lines, that
/// sets anything but a plain name of our own.
fn sets_a_cfg_of_its_own(text: &str) -> bool {
    text.lines().any(|line| {
        let Some((_, rest)) = line.split_once("rustc-cfg") else {
            return false;
        };
        let cfg = rest.strip_prefix('=').unwrap_or(rest).trim_end();
        let plain = cfg.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && cfg.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        let reserved = RESERVED_CFGS.contains(&cfg)
            || cfg.starts_with("target_")
            || cfg.starts_with("sanitize");
        !plain || reserved
    })
}

/// Findings for the cargo configuration `text` (at `path`): it holds only
/// `alias.xtask`, exactly [`ALIAS`]; the tables `deps` reports are left to
/// it.
pub(crate) fn check_config(path: &str, text: &str) -> Vec<Finding> {
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
    let mut lines = Vec::new();
    for key in keys {
        let path_text = key.path.join(".");
        let message = match key
            .path
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice()
        {
            ["alias"] => continue,
            ["alias", "xtask"] if key.value == ALIAS => continue,
            ["alias", "xtask"] => {
                format!("`alias.xtask` must stay {ALIAS}: the gate runs through it")
            }
            [first, ..] if REDIRECTS.contains(first) => continue,
            _ => format!(
                "`{path_text}`: the repository's cargo configuration may hold only the `xtask` alias; other settings (`rustflags`, `[env]`, `runner`, `build.rustc`) act on the builds, and on `cargo xtask` itself, behind cargo's back"
            ),
        };
        if !lines.contains(&key.line) {
            lines.push(key.line);
            findings.push(Finding::at_line(path, key.line, message));
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEMBERS: [&str; 5] = [
        "softshelldb",
        "softshell-core",
        "softshell-storage",
        "softshell-platform",
        "tidepool",
    ];

    fn release(tree: &str) -> Vec<String> {
        check_tree("softshelldb", Build::Release, "", tree, &MEMBERS)
            .unwrap()
            .into_iter()
            .map(|f| f.message)
            .collect()
    }

    #[test]
    fn the_tree_command_leaves_out_dev_dependencies_on_every_platform() {
        assert!(TREE.windows(2).any(|w| w == ["--edges", "normal,build"]));
        assert!(TREE.windows(2).any(|w| w == ["--target", "all"]));
        assert!(TREE.contains(&"--offline") && TREE.contains(&"--locked"));
    }

    #[test]
    fn clean_trees_pass() {
        let tree = "softshelldb v0.1.0 (/ws/crates/softshelldb)\t\n\
                    softshell-core v0.1.0 (/ws/crates/core)\tdefault,std\n\
                    softshell-core v0.1.0 (/ws/crates/core)\t (*)\n";
        assert_eq!(release(tree), Vec::<String>::new());
        assert_eq!(release(&tree.replace('\n', "\r\n")), Vec::<String>::new());
        // A feature whose name contains a forbidden one is not it, and a
        // third-party crate's `dev` is its own business.
        let tree = "softshelldb v0.1.0 (/ws)\t\n\
                    softshell-core v0.1.0 (/ws/crates/core)\tdev-tools,no-fault-injection\n\
                    other v1.0.0\tdev\n";
        assert_eq!(release(tree), Vec::<String>::new());
    }

    #[test]
    fn forbidden_features_are_found_once() {
        let tree = "softshelldb v0.1.0 (/ws/crates/softshelldb)\t\n\
                    softshell-storage v0.1.0 (/ws/crates/storage)\tdefault,dev\n\
                    softshell-platform v0.1.0 (/ws/crates/platform)\tfault-injection (*)\n\
                    softshell-storage v0.1.0 (/ws/crates/storage)\tdefault,dev (*)\n";
        assert_eq!(
            release(tree),
            [
                "a release build of `softshelldb` turns on feature `dev` of `softshell-storage`; `cargo tree -p softshelldb -e features,no-dev --target all -i softshell-storage` shows why",
                "a release build of `softshelldb` turns on feature `fault-injection` of `softshell-platform`; `cargo tree -p softshelldb -e features,no-dev --target all -i softshell-platform` shows why",
            ]
        );
    }

    #[test]
    fn a_members_dependencies_may_not_turn_features_on() {
        let tree = "tidepool v0.1.0 (/ws/apps/tidepool)\t\n\
                    softshelldb v0.1.0 (/ws/crates/softshelldb)\tdefault,dev,testing\n";
        let found = check_tree(
            "tidepool",
            Build::Dependencies,
            "apps/tidepool/Cargo.toml",
            tree,
            &MEMBERS,
        )
        .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].location, "apps/tidepool/Cargo.toml");
        assert!(found[0].message.starts_with(
            "the normal and build dependencies of `tidepool` turn on feature `dev` of `softshelldb`"
        ));
    }

    #[test]
    fn unexpected_output_is_an_error() {
        for tree in [
            "softshelldb v0.1.0 (/ws)\n",
            "",
            "softshell-core v0.1.0 (/ws)\t\n",
        ] {
            assert!(
                check_tree("softshelldb", Build::Release, "", tree, &MEMBERS).is_err(),
                "{tree:?}"
            );
        }
    }

    #[test]
    fn cfgs_behind_cargos_back() {
        let script = "\
fn main() {
    println!(\"cargo::rustc-cfg=feature=\\\"dev\\\"\");
    println!(\"cargo:rustc-cfg={}\", name);
    println!(\"cargo::rustc-cfg=has_statx\");
    println!(\"cargo::rustc-check-cfg=cfg(has_statx)\");
    // rustc-cfg=feature in a comment is not emitted
    out.write_all(b\"cargo::rustc-cfg=feature=\\\"dev\\\"\\n\");
    print!(\"{}\", concat!(\"cargo::rustc-\", \"cfg=test\"));
    println!(\"cargo::rustc-cfg=has_a\\ncargo::rustc-cfg=has_b\\n\");
    println!(\"cargo::rustc-cfg=target_os\");
    print!(\"{}\", include_str!(\"directives.txt\"));
    print!(\"{}\", include_str!(\"plain.txt\"));
}
";
        let include = |path: &str| match path {
            "crates/platform/directives.txt" => Some("cargo::rustc-cfg=doc\n".to_owned()),
            "crates/platform/plain.txt" => Some("cargo::rustc-cfg=has_c\n".to_owned()),
            _ => None,
        };
        let found = check_build_source("crates/platform/build.rs", script, include);
        let lines: Vec<_> = found.iter().map(|f| f.location.as_str()).collect();
        assert_eq!(
            lines,
            [2, 3, 7, 8, 10, 11].map(|l| format!("crates/platform/build.rs:{l}"))
        );
    }

    #[test]
    fn the_configuration_holds_only_the_alias() {
        assert_eq!(
            check_config(
                ".cargo/config.toml",
                include_str!("../../.cargo/config.toml")
            ),
            []
        );
        let config = "\
[alias]
xtask = \"run --profile xtask --package xtask --\"
[build]
rustflags = [\"--cfg\", \"feature=\\\"dev\\\"\"]
rustc = \"tools/rustc.sh\"
[target.'cfg(all())']
runner = \"true\"
[env]
RUSTFLAGS = { value = \"--cap-lints=allow\", force = true }
[source.crates-io]
replace-with = \"v\"
";
        let found = check_config(".cargo/config.toml", config);
        let lines: Vec<_> = found.iter().map(|f| f.location.as_str()).collect();
        assert_eq!(
            lines,
            [3, 4, 5, 6, 7, 8, 9].map(|l| format!(".cargo/config.toml:{l}"))
        );
        let changed = "[alias]\nxtask = \"version\"\n";
        let found = check_config(".cargo/config.toml", changed);
        assert_eq!(found.len(), 1);
        assert!(found[0].message.starts_with("`alias.xtask` must stay"));
    }
}
