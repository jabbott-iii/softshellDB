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

//! The workspace being checked: its root, the programs the checks run
//! there, and its files.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::error::XtaskError;

/// The workspace `xtask` runs in. Checks never look outside it.
pub(crate) struct Workspace {
    root: PathBuf,
}

/// The files of the workspace, as Git lists them.
#[derive(Debug, Default)]
pub(crate) struct Files {
    /// Regular files, tracked or untracked but not ignored: relative to the
    /// root, `/`-separated, sorted.
    pub(crate) files: Vec<String>,
    /// Submodules (gitlinks), which hold code from elsewhere.
    pub(crate) gitlinks: Vec<String>,
    /// Symbolic links, tracked or untracked: the compiler follows them,
    /// and the checks, which read files, do not.
    pub(crate) links: Vec<String>,
}

impl Workspace {
    /// The workspace of the `xtask` being run: the parent of the
    /// `CARGO_MANIFEST_DIR` that `cargo run` sets, or of the one this
    /// binary was built in when it is run directly. The run-time value
    /// comes first: two copies of the workspace that share a target
    /// directory share one `xtask` binary, built in only one of them.
    pub(crate) fn new() -> Self {
        let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR")
            .map_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")), PathBuf::from);
        let root = manifest_dir
            .parent()
            .map_or_else(|| manifest_dir.clone(), Path::to_path_buf);
        Self { root }
    }

    /// The workspace root.
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// `path` (relative to the root, `/`-separated) as a path on this OS.
    pub(crate) fn path(&self, path: &str) -> PathBuf {
        self.root.join(path)
    }

    /// A `cargo` command run in the root: the `cargo` that runs `xtask`
    /// (`$CARGO`), or the one on `PATH`. It never uses the network.
    pub(crate) fn cargo(&self) -> Command {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
        let mut command = Command::new(cargo);
        command
            .current_dir(&self.root)
            .env("CARGO_NET_OFFLINE", "true");
        command
    }

    /// A `git` command run in the root.
    fn git(&self, args: &[&str]) -> Command {
        let mut command = Command::new("git");
        command.current_dir(&self.root).args(args);
        command
    }

    /// Runs `command` and returns its stdout, which must be UTF-8.
    ///
    /// # Errors
    ///
    /// [`XtaskError::Io`] if it cannot start, [`XtaskError::Failed`] (with
    /// its stderr) if it exits unsuccessfully, [`XtaskError::Shape`] if its
    /// output is not UTF-8.
    pub(crate) fn output(&self, mut command: Command) -> Result<String, XtaskError> {
        let line = command_line(&command);
        let output = command
            .stdin(Stdio::null())
            .output()
            .map_err(|source| XtaskError::Io {
                doing: format!("running `{line}`"),
                source,
            })?;
        if !output.status.success() {
            return Err(XtaskError::Failed {
                command: line,
                status: output.status,
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }
        String::from_utf8(output.stdout).map_err(|_| XtaskError::Shape {
            what: format!("the output of `{line}`"),
            detail: "it is not UTF-8".to_owned(),
        })
    }

    /// The text of the file at `path` (relative to the root).
    ///
    /// # Errors
    ///
    /// [`XtaskError::Io`] if it cannot be read, [`XtaskError::Shape`] if
    /// it is not UTF-8.
    pub(crate) fn read_text(&self, path: &str) -> Result<String, XtaskError> {
        let bytes = std::fs::read(self.path(path)).map_err(|source| XtaskError::Io {
            doing: format!("reading {path}"),
            source,
        })?;
        String::from_utf8(bytes).map_err(|_| XtaskError::Shape {
            what: path.to_owned(),
            detail: "it is not UTF-8".to_owned(),
        })
    }

    /// The text of the file at `path`, or `None` if there is none.
    ///
    /// # Errors
    ///
    /// As [`read_text`](Self::read_text), except for a missing file.
    pub(crate) fn read_text_if_present(&self, path: &str) -> Result<Option<String>, XtaskError> {
        if self.path(path).exists() {
            self.read_text(path).map(Some)
        } else {
            Ok(None)
        }
    }

    /// The workspace's files as Git lists them: tracked files (from the
    /// index, with their modes) and untracked files that are not ignored,
    /// so that a new file is checked before it is committed. Tracked files
    /// deleted from the working tree are left out; submodules and symbolic
    /// links are listed apart.
    ///
    /// # Errors
    ///
    /// [`XtaskError`] if Git fails, a name is not UTF-8, or the listing
    /// does not hold this workspace's own files (not a Git checkout).
    pub(crate) fn files(&self) -> Result<Files, XtaskError> {
        let hint = |err: XtaskError| XtaskError::Shape {
            what: "listing the workspace's files".to_owned(),
            detail: format!(
                "the source checks need a Git checkout: {}",
                softshell_core::error::Chain::new(&err)
            ),
        };
        let tracked = self
            .output(self.git(&["ls-files", "-z", "--stage"]))
            .map_err(hint)?;
        let untracked = self
            .output(self.git(&["ls-files", "-z", "--others", "--exclude-standard"]))
            .map_err(hint)?;
        let mut listing = parse_stage(&tracked)?;
        for name in untracked.split('\0').filter(|name| !name.is_empty()) {
            // Git lists an untracked directory holding its own repository
            // as `dir/`, without its files: code from elsewhere.
            if let Some(dir) = name.strip_suffix('/') {
                listing.gitlinks.push(dir.to_owned());
                continue;
            }
            let Ok(metadata) = std::fs::symlink_metadata(self.path(name)) else {
                continue;
            };
            if metadata.file_type().is_symlink() {
                listing.links.push(name.to_owned());
            } else if metadata.file_type().is_file() {
                listing.files.push(name.to_owned());
            }
        }
        // A tracked file may be a link in the working tree, or deleted.
        let mut files = Vec::new();
        for name in std::mem::take(&mut listing.files) {
            match std::fs::symlink_metadata(self.path(&name)) {
                Ok(m) if m.file_type().is_symlink() => listing.links.push(name),
                Ok(m) if m.file_type().is_file() => files.push(name),
                _ => {}
            }
        }
        listing.files = files;
        listing.links.sort_unstable();
        listing.links.dedup();
        listing.files.sort_unstable();
        listing.files.dedup();
        listing.gitlinks.sort_unstable();
        listing.gitlinks.dedup();
        if !listing.files.iter().any(|f| f == "xtask/src/main.rs") {
            return Err(XtaskError::Shape {
                what: "listing the workspace's files".to_owned(),
                detail:
                    "Git did not list xtask/src/main.rs; run from a Git checkout of the workspace"
                        .to_owned(),
            });
        }
        Ok(listing)
    }
}

/// Reads `git ls-files -z --stage`: `<mode> <object> <stage>\t<path>`
/// entries. A path in a merge conflict comes once per stage.
fn parse_stage(text: &str) -> Result<Files, XtaskError> {
    let mut listing = Files::default();
    for entry in text.split('\0').filter(|e| !e.is_empty()) {
        let (info, path) = entry.split_once('\t').ok_or_else(|| XtaskError::Shape {
            what: "the output of git ls-files --stage".to_owned(),
            detail: format!("the entry `{entry}` has no tab"),
        })?;
        match info.split(' ').next() {
            Some("160000") => listing.gitlinks.push(path.to_owned()),
            Some("120000") => listing.links.push(path.to_owned()),
            _ => listing.files.push(path.to_owned()),
        }
    }
    Ok(listing)
}

/// `command` as a line of text, for messages.
fn command_line(command: &Command) -> String {
    let program = Path::new(command.get_program())
        .file_name()
        .unwrap_or(command.get_program())
        .to_string_lossy()
        .into_owned();
    let mut line = program;
    for arg in command.get_args() {
        line.push(' ');
        line.push_str(&arg.to_string_lossy());
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_listings() {
        let text = [
            "100644 aaaa 0\tsrc/lib.rs",
            "100755 bbbb 0\tscripts/x.sh",
            "120000 cccc 0\tlink.rs",
            "160000 dddd 0\tvendor/sub",
            "100644 eeee 1\tconflict.rs",
            "100644 ffff 2\tconflict.rs",
            "",
        ]
        .join("\0");
        let text = text.as_str();
        let listing = parse_stage(text).unwrap();
        assert_eq!(
            listing.files,
            ["src/lib.rs", "scripts/x.sh", "conflict.rs", "conflict.rs"]
        );
        assert_eq!(listing.gitlinks, ["vendor/sub"]);
        assert_eq!(listing.links, ["link.rs"]);
        assert!(parse_stage("100644 aaaa 0 no-tab\0").is_err());
    }
}
