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

//! The workspace's packages and dependencies, from
//! `cargo metadata --format-version 1`, read with our own JSON parser.

use std::path::{Path, PathBuf};

use softshell_codec::json;
use softshell_codec::value::{Object, Value};
use softshell_core::limits::Limits;

use crate::error::XtaskError;
use crate::workspace::Workspace;

/// What `cargo metadata` reports.
#[derive(Debug)]
pub(crate) struct Metadata {
    /// The workspace root, as cargo reports it.
    pub(crate) root: PathBuf,
    /// Every package: the workspace's members, and with
    /// [`Resolve::Yes`] every package in `Cargo.lock`.
    pub(crate) packages: Vec<Package>,
    /// The ids of the workspace's members.
    pub(crate) members: Vec<String>,
}

/// Whether to resolve the dependency graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Resolve {
    /// `--no-deps`: the members and their manifests only.
    No,
    /// Every package `Cargo.lock` holds: `--all-features`, so that optional
    /// dependencies behind a feature are listed too, on every platform.
    Yes,
}

impl Resolve {
    /// The arguments of `cargo metadata`: offline, never touching
    /// `Cargo.lock`.
    pub(crate) const fn args(self) -> [&'static str; 6] {
        let last = match self {
            Self::No => "--no-deps",
            Self::Yes => "--all-features",
        };
        [
            "metadata",
            "--format-version",
            "1",
            "--offline",
            "--locked",
            last,
        ]
    }
}

/// One package.
#[derive(Debug)]
pub(crate) struct Package {
    /// Cargo's package id.
    pub(crate) id: String,
    /// The package name.
    pub(crate) name: String,
    /// The version.
    pub(crate) version: String,
    /// Where it comes from (`registry+…`, `git+…`); `None` for a path
    /// package.
    pub(crate) source: Option<String>,
    /// The Rust edition, such as `2024`.
    pub(crate) edition: String,
    /// The directory holding its `Cargo.toml`.
    pub(crate) dir: PathBuf,
    /// Its `Cargo.toml`.
    pub(crate) manifest: PathBuf,
    /// What its manifest declares, every kind and target.
    pub(crate) dependencies: Vec<Dependency>,
    /// Its library, binaries, tests, examples, benches and build script.
    pub(crate) targets: Vec<Target>,
}

/// A dependency as a manifest declares it.
#[derive(Debug)]
pub(crate) struct Dependency {
    /// The package depended on (not a `package =` rename).
    pub(crate) name: String,
    /// The version requirement, such as `^0.1.0` or `=1.2.3`.
    pub(crate) req: String,
    /// Which table declares it.
    pub(crate) kind: Kind,
    /// Where it comes from; `None` for a path dependency.
    pub(crate) source: Option<String>,
    /// The directory of a path dependency.
    pub(crate) path: Option<PathBuf>,
    /// The platform it is for (`cfg(windows)`), if not every one.
    pub(crate) target: Option<String>,
    /// The registry, if not crates.io.
    pub(crate) registry: Option<String>,
}

/// One compilation target of a package.
#[derive(Debug)]
pub(crate) struct Target {
    /// Its kinds, such as `lib`, `bin`, `test` or `custom-build`.
    pub(crate) kinds: Vec<String>,
    /// Its root source file.
    pub(crate) src_path: PathBuf,
    /// Its Rust edition.
    pub(crate) edition: String,
}

/// The table a dependency is declared in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// `[dependencies]`.
    Normal,
    /// `[dev-dependencies]`.
    Dev,
    /// `[build-dependencies]`.
    Build,
}

impl Kind {
    /// The table's name, for messages.
    pub(crate) const fn table(self) -> &'static str {
        match self {
            Self::Normal => "dependencies",
            Self::Dev => "dev-dependencies",
            Self::Build => "build-dependencies",
        }
    }
}

impl Dependency {
    /// `[table]` or `[table] for `cfg(…)``, for messages.
    pub(crate) fn table(&self) -> String {
        match &self.target {
            Some(target) => format!("[{}] for `{target}`", self.kind.table()),
            None => format!("[{}]", self.kind.table()),
        }
    }
}

impl Metadata {
    /// Runs `cargo metadata` in `workspace`, offline and without touching
    /// `Cargo.lock` (`--locked`: a lock file that needs updating is an
    /// error).
    ///
    /// # Errors
    ///
    /// [`XtaskError`] if cargo fails or its output is not what version 1
    /// of the format describes.
    pub(crate) fn load(workspace: &Workspace, resolve: Resolve) -> Result<Self, XtaskError> {
        let mut command = workspace.cargo();
        command.args(resolve.args());
        Self::parse(&workspace.output(command)?)
    }

    /// Reads the output of `cargo metadata --format-version 1`.
    ///
    /// # Errors
    ///
    /// [`XtaskError::Json`] if it is not JSON, [`XtaskError::Shape`] if a
    /// field is missing or of the wrong type.
    pub(crate) fn parse(text: &str) -> Result<Self, XtaskError> {
        let what = "the output of cargo metadata";
        let value = json::parse(text, Limits::default()).map_err(|source| XtaskError::Json {
            what: what.to_owned(),
            source,
        })?;
        let top = "the top level";
        let root = object(&value, top)?;
        let packages = array(root, "packages", top)?
            .iter()
            .map(Package::parse)
            .collect::<Result<_, _>>()?;
        let members = array(root, "workspace_members", top)?
            .iter()
            .map(|id| {
                id.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| shape("workspace_members", "an id is not a string"))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            root: PathBuf::from(string(root, "workspace_root", top)?),
            packages,
            members,
        })
    }

    /// The workspace's members.
    pub(crate) fn members(&self) -> impl Iterator<Item = &Package> {
        self.packages.iter().filter(|p| self.is_member(p))
    }

    /// Whether `package` is one of the workspace's members.
    pub(crate) fn is_member(&self, package: &Package) -> bool {
        self.members.contains(&package.id)
    }

    /// The member whose directory is `dir`.
    pub(crate) fn member_at(&self, dir: &Path) -> Option<&Package> {
        self.members().find(|p| p.dir == dir)
    }

    /// `path` relative to the workspace root, `/`-separated (as Git writes
    /// paths), or `None` if it is outside the root.
    pub(crate) fn relative(&self, path: &Path) -> Option<String> {
        let relative = path.strip_prefix(&self.root).ok()?;
        let parts: Vec<_> = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy())
            .collect();
        Some(parts.join("/"))
    }

    /// `path` relative to the root for a message, or as it is if it is
    /// outside the root.
    pub(crate) fn display(&self, path: &Path) -> String {
        self.relative(path)
            .unwrap_or_else(|| path.to_string_lossy().into_owned())
    }
}

impl Package {
    fn parse(value: &Value) -> Result<Self, XtaskError> {
        let package = object(value, "a package")?;
        let name = string(package, "name", "a package")?.to_owned();
        let what = format!("package `{name}`");
        let manifest = PathBuf::from(string(package, "manifest_path", &what)?);
        let dir = manifest
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| shape(&what, "its manifest_path has no directory"))?;
        let dependencies = array(package, "dependencies", &what)?
            .iter()
            .map(|d| Dependency::parse(d, &what))
            .collect::<Result<_, _>>()?;
        let targets = array(package, "targets", &what)?
            .iter()
            .map(|t| Target::parse(t, &what))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            id: string(package, "id", &what)?.to_owned(),
            version: string(package, "version", &what)?.to_owned(),
            source: optional_string(package, "source", &what)?.map(str::to_owned),
            edition: string(package, "edition", &what)?.to_owned(),
            name,
            dir,
            manifest,
            dependencies,
            targets,
        })
    }
}

impl Dependency {
    fn parse(value: &Value, package: &str) -> Result<Self, XtaskError> {
        let what = format!("a dependency of {package}");
        let dependency = object(value, &what)?;
        let kind = match optional_string(dependency, "kind", &what)? {
            None => Kind::Normal,
            Some("dev") => Kind::Dev,
            Some("build") => Kind::Build,
            Some(other) => return Err(shape(&what, &format!("unknown kind `{other}`"))),
        };
        Ok(Self {
            name: string(dependency, "name", &what)?.to_owned(),
            req: string(dependency, "req", &what)?.to_owned(),
            kind,
            source: optional_string(dependency, "source", &what)?.map(str::to_owned),
            path: optional_string(dependency, "path", &what)?.map(PathBuf::from),
            target: optional_string(dependency, "target", &what)?.map(str::to_owned),
            registry: optional_string(dependency, "registry", &what)?.map(str::to_owned),
        })
    }
}

impl Target {
    fn parse(value: &Value, package: &str) -> Result<Self, XtaskError> {
        let what = format!("a target of {package}");
        let target = object(value, &what)?;
        Ok(Self {
            kinds: strings(target, "kind", &what)?,
            src_path: PathBuf::from(string(target, "src_path", &what)?),
            edition: string(target, "edition", &what)?.to_owned(),
        })
    }
}

fn shape(what: &str, detail: &str) -> XtaskError {
    XtaskError::Shape {
        what: format!("cargo metadata: {what}"),
        detail: detail.to_owned(),
    }
}

fn object<'a>(value: &'a Value, what: &str) -> Result<&'a Object, XtaskError> {
    value
        .as_object()
        .ok_or_else(|| shape(what, "it is not an object"))
}

fn field<'a>(object: &'a Object, key: &str, what: &str) -> Result<&'a Value, XtaskError> {
    object
        .get(key)
        .ok_or_else(|| shape(what, &format!("it has no `{key}`")))
}

fn string<'a>(object: &'a Object, key: &str, what: &str) -> Result<&'a str, XtaskError> {
    field(object, key, what)?
        .as_str()
        .ok_or_else(|| shape(what, &format!("its `{key}` is not a string")))
}

/// A string, null, or (as cargo writes `path` for a dependency that is not
/// a path one) absent.
fn optional_string<'a>(
    object: &'a Object,
    key: &str,
    what: &str,
) -> Result<Option<&'a str>, XtaskError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(shape(what, &format!("its `{key}` is not a string or null"))),
    }
}

fn array<'a>(object: &'a Object, key: &str, what: &str) -> Result<&'a [Value], XtaskError> {
    field(object, key, what)?
        .as_array()
        .ok_or_else(|| shape(what, &format!("its `{key}` is not an array")))
}

fn strings(object: &Object, key: &str, what: &str) -> Result<Vec<String>, XtaskError> {
    array(object, key, what)?
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or_else(|| shape(what, &format!("its `{key}` holds a non-string")))
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A metadata document for tests, rooted at `/ws`: `packages` as JSON
    /// objects, written by [`package`] and the like.
    pub(crate) fn metadata(packages: &[String], members: &[&str]) -> Metadata {
        let members: Vec<String> = members.iter().map(|id| format!("\"{id}\"")).collect();
        let text = format!(
            r#"{{"packages": [{}], "workspace_members": [{}], "workspace_root": "/ws",
                "version": 1}}"#,
            packages.join(","),
            members.join(",")
        );
        Metadata::parse(&text).unwrap()
    }

    /// A 2024-edition package at `/ws/<dir>` with a library at
    /// `src/lib.rs`, and `dependencies` as JSON objects.
    pub(crate) fn package(name: &str, dir: &str, source: Option<&str>, deps: &[String]) -> String {
        let source = source.map_or("null".to_owned(), |s| format!("\"{s}\""));
        let targets = target("lib", &format!("{dir}/src/lib.rs"), "2024");
        package_with(name, "0.1.0", dir, &source, "2024", &[targets], deps)
    }

    /// A package with every field given; `source` is JSON.
    pub(crate) fn package_with(
        name: &str,
        version: &str,
        dir: &str,
        source: &str,
        edition: &str,
        targets: &[String],
        deps: &[String],
    ) -> String {
        format!(
            r#"{{"name": "{name}", "version": "{version}", "id": "{name}-id", "source": {source},
                "edition": "{edition}", "manifest_path": "/ws/{dir}/Cargo.toml",
                "dependencies": [{}], "targets": [{}], "features": {{}}}}"#,
            deps.join(","),
            targets.join(",")
        )
    }

    /// A target of kind `kind` whose root is `/ws/<src>`.
    pub(crate) fn target(kind: &str, src: &str, edition: &str) -> String {
        format!(
            r#"{{"kind": ["{kind}"], "crate_types": ["{kind}"], "name": "t",
                "src_path": "/ws/{src}", "edition": "{edition}"}}"#
        )
    }

    /// Version `version` of `name` from crates.io.
    pub(crate) fn registry_package(name: &str, version: &str) -> String {
        let source = "\"registry+https://github.com/rust-lang/crates.io-index\"";
        let dir = format!("registry/{name}-{version}");
        let lib = target("lib", &format!("{dir}/src/lib.rs"), "2021");
        package_with(name, version, &dir, source, "2021", &[lib], &[])
    }

    /// A dependency on `name`, from `/ws/<path>` or from `source`.
    pub(crate) fn dependency(
        name: &str,
        kind: Option<&str>,
        path: Option<&str>,
        source: Option<&str>,
        req: &str,
    ) -> String {
        dependency_with(name, kind, path, source, req, &[], "null")
    }

    /// A dependency with `features` and `registry` (JSON) too.
    pub(crate) fn dependency_with(
        name: &str,
        kind: Option<&str>,
        path: Option<&str>,
        source: Option<&str>,
        req: &str,
        features: &[&str],
        registry: &str,
    ) -> String {
        let kind = kind.map_or("null".to_owned(), |k| format!("\"{k}\""));
        let source = source.map_or("null".to_owned(), |s| format!("\"{s}\""));
        // Cargo leaves `path` out, not null, for a dependency that is not a
        // path one.
        let path = path.map_or(String::new(), |p| format!(r#", "path": "/ws/{p}""#));
        let features: Vec<String> = features.iter().map(|f| format!("\"{f}\"")).collect();
        format!(
            r#"{{"name": "{name}", "source": {source}, "req": "{req}", "kind": {kind},
                "rename": null, "optional": false, "uses_default_features": true,
                "features": [{}], "target": null, "registry": {registry}{path}}}"#,
            features.join(",")
        )
    }

    #[test]
    fn reads_packages_and_dependencies() {
        let m = metadata(
            &[package(
                "a",
                "crates/a",
                None,
                &[
                    dependency("b", None, Some("crates/b"), None, "^0.1.0"),
                    dependency(
                        "x",
                        Some("dev"),
                        None,
                        Some("registry+https://github.com/rust-lang/crates.io-index"),
                        "=1.0.0",
                    ),
                ],
            )],
            &["a-id"],
        );
        let a = &m.packages[0];
        assert_eq!(
            (a.name.as_str(), a.dir.as_path()),
            ("a", Path::new("/ws/crates/a"))
        );
        assert!(m.is_member(a));
        assert_eq!(
            a.dependencies[0].path.as_deref(),
            Some(Path::new("/ws/crates/b"))
        );
        assert_eq!(a.dependencies[0].kind, Kind::Normal);
        assert_eq!(a.dependencies[1].kind, Kind::Dev);
        assert_eq!(a.dependencies[1].path, None);
        assert_eq!(a.dependencies[1].req, "=1.0.0");
        assert_eq!(a.targets[0].src_path, Path::new("/ws/crates/a/src/lib.rs"));
        assert_eq!(m.relative(&a.dir).as_deref(), Some("crates/a"));
        assert_eq!(m.relative(Path::new("/elsewhere/x")), None);
        assert_eq!(m.display(Path::new("/elsewhere/x")), "/elsewhere/x");
    }

    #[test]
    fn the_resolution_lists_optional_dependencies() {
        // Without `--all-features`, cargo leaves out optional dependencies
        // that no default feature turns on, though Cargo.lock has them.
        assert_eq!(Resolve::Yes.args().last(), Some(&"--all-features"));
        assert_eq!(Resolve::No.args().last(), Some(&"--no-deps"));
        for resolve in [Resolve::No, Resolve::Yes] {
            assert!(resolve.args().contains(&"--offline") && resolve.args().contains(&"--locked"));
        }
    }

    #[test]
    fn rejects_unexpected_shapes() {
        for text in [
            "",
            "[]",
            r#"{"packages": [], "workspace_members": [1]}"#,
            r#"{"packages": [{"name": "a"}], "workspace_members": []}"#,
            r#"{"workspace_members": []}"#,
            r#"{"packages": [], "workspace_members": []}"#,
        ] {
            assert!(Metadata::parse(text).is_err(), "{text}");
        }
    }
}
