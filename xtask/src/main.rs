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

//! Build automation for the softshellDB workspace, run as
//! `cargo xtask <command>`.
//!
//! Specified in `docs/subsystems/cli-and-tools.md` and `docs/04-quality.md`.
//! It never uploads, publishes, reads secrets or calls the network: every
//! cargo command it runs is offline.
//!
//! Exit status: 0 when every check passed, 1 when a check found a problem
//! or could not run, 2 for a command line it does not understand.

// Printing is this binary's job (`docs/01-architecture.md`, workspace lints).
#![allow(clippy::print_stdout, clippy::print_stderr)]

mod check;
mod deps;
mod error;
mod features;
mod headers;
mod layers;
mod lexer;
mod metadata;
mod release;
mod toml_keys;
mod unsafe_audit;
mod workspace;

use std::io::Write;
use std::process::ExitCode;

use softshell_core::cli::{Args, CliError};
use softshell_core::error::Chain;

use crate::check::Finding;
use crate::error::XtaskError;
use crate::workspace::Workspace;

/// `println!` that ends the process quietly, with status 1, when stdout is
/// gone (`cargo xtask ci | head`), instead of panicking.
macro_rules! say {
    ($($arg:tt)*) => {
        if writeln!(std::io::stdout(), $($arg)*).is_err() {
            std::process::exit(1);
        }
    };
}

const USAGE: &str = "\
usage: cargo xtask <command>

commands:
  ci              every check below: the pull-request gate
  fmt             cargo fmt --check
  clippy          cargo clippy, warnings as errors
  test            cargo test --workspace
  deps            no crate from outside the workspace unless approved
  layers          crates depend only as the dependency matrix allows
  unsafe-audit    `unsafe` only in softshell-sys and softshell-platform
  headers         every source file starts with the license header
  release-no-dev  release builds have no `dev` or `fault-injection`
  features        docs/features.jsonc agrees with docs/roadmap.md
  help            this text

Exit status: 0 when every check passed, 1 when one found a problem or
could not run, 2 for a command line not understood. Cargo's own 101
means it could not build or resolve `xtask`, which includes a dependency
it cannot find offline.
";

/// A check `cargo xtask` runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Deps,
    Layers,
    UnsafeAudit,
    Headers,
    Features,
    ReleaseNoDev,
    Fmt,
    Clippy,
    Test,
}

impl Step {
    /// Every check, in the order `ci` runs them: the quick ones first.
    const ALL: [Self; 9] = [
        Self::Deps,
        Self::Layers,
        Self::UnsafeAudit,
        Self::Headers,
        Self::Features,
        Self::ReleaseNoDev,
        Self::Fmt,
        Self::Clippy,
        Self::Test,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::Deps => "deps",
            Self::Layers => "layers",
            Self::UnsafeAudit => "unsafe-audit",
            Self::Headers => "headers",
            Self::Features => "features",
            Self::ReleaseNoDev => "release-no-dev",
            Self::Fmt => "fmt",
            Self::Clippy => "clippy",
            Self::Test => "test",
        }
    }

    /// Runs the check, printing what it finds; whether it passed.
    fn run(self, workspace: &Workspace) -> bool {
        say!("== {}", self.name());
        let result = match self {
            Self::Deps => deps::run(workspace),
            Self::Layers => layers::run(workspace),
            Self::UnsafeAudit => unsafe_audit::run(workspace),
            Self::Headers => headers::run(workspace),
            Self::Features => features::run(workspace),
            Self::ReleaseNoDev => release::run(workspace),
            Self::Fmt => cargo_step(workspace, FMT),
            Self::Clippy => cargo_step(workspace, CLIPPY),
            Self::Test => cargo_step(workspace, TEST),
        };
        match result {
            Ok(findings) if findings.is_empty() => {
                say!("ok");
                true
            }
            Ok(findings) => {
                for finding in &findings {
                    say!("{finding}");
                }
                let n = findings.len();
                say!("{n} problem{}", if n == 1 { "" } else { "s" });
                false
            }
            Err(err) => {
                say!("error: {}", Chain::new(&err));
                false
            }
        }
    }
}

/// `cargo fmt`'s arguments for `fmt`.
const FMT: &[&str] = &["fmt", "--all", "--check"];

/// `cargo clippy`'s arguments for `clippy`.
const CLIPPY: &[&str] = &[
    "clippy",
    "--workspace",
    "--all-targets",
    "--offline",
    "--locked",
    "--",
    "-D",
    "warnings",
];

/// `cargo test`'s arguments for `test`.
const TEST: &[&str] = &["test", "--workspace", "--offline", "--locked"];

/// Runs every step in `steps` with `run`, even after one fails; the names
/// of those that failed.
fn failed_steps(steps: &[Step], mut run: impl FnMut(Step) -> bool) -> Vec<&'static str> {
    let mut failed = Vec::new();
    for &step in steps {
        if !run(step) {
            failed.push(step.name());
        }
    }
    failed
}

/// Runs `cargo <args>` with its output shown; a finding if it fails.
fn cargo_step(workspace: &Workspace, args: &[&str]) -> Result<Vec<Finding>, XtaskError> {
    let mut command = workspace.cargo();
    command.args(args);
    let status = command.status().map_err(|source| XtaskError::Io {
        doing: format!("running cargo {}", args.join(" ")),
        source,
    })?;
    Ok(if status.success() {
        Vec::new()
    } else {
        vec![Finding::new(
            "",
            format!("`cargo {}` failed ({status})", args.join(" ")),
        )]
    })
}

/// What the command line asks for.
enum Command {
    Help,
    Ci,
    One(Step),
}

fn parse(mut args: Args) -> Result<Command, CliError> {
    let name = args.subcommand()?;
    // `--help` or `-h` anywhere asks for the usage.
    if args.flag("--help")? || args.flag("-h")? {
        return Ok(Command::Help);
    }
    let name = name.ok_or(CliError::MissingCommand)?;
    let command = match name.as_str() {
        "help" => Command::Help,
        "ci" => Command::Ci,
        _ => Step::ALL
            .into_iter()
            .find(|step| step.name() == name)
            .map(Command::One)
            .ok_or(CliError::UnknownCommand(name))?,
    };
    args.positionals()?.finish()?;
    Ok(command)
}

fn main() -> ExitCode {
    let command = match parse(Args::from_env()) {
        Ok(command) => command,
        Err(err) => {
            eprintln!("error: {err}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let workspace = Workspace::new();
    let passed = match command {
        Command::Help => {
            say!("{}", USAGE.trim_end());
            return ExitCode::SUCCESS;
        }
        Command::One(step) => step.run(&workspace),
        Command::Ci => {
            say!("== ci: checking {}", workspace.root().display());
            let failed = failed_steps(&Step::ALL, |step| step.run(&workspace));
            if failed.is_empty() {
                say!("== ci: all {} checks passed", Step::ALL.len());
            } else {
                say!(
                    "== ci: {} of {} checks failed: {}",
                    failed.len(),
                    Step::ALL.len(),
                    failed.join(", ")
                );
            }
            failed.is_empty()
        }
    };
    if passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(args: &[&str]) -> Result<Command, CliError> {
        parse(Args::new(args.iter().copied()))
    }

    #[test]
    fn command_lines() {
        assert!(matches!(parsed(&["ci"]), Ok(Command::Ci)));
        assert!(matches!(parsed(&["help"]), Ok(Command::Help)));
        assert!(matches!(parsed(&["deps", "--help"]), Ok(Command::Help)));
        assert!(matches!(parsed(&["--help"]), Ok(Command::Help)));
        assert!(matches!(parsed(&["-h"]), Ok(Command::Help)));
        for step in Step::ALL {
            assert!(matches!(parsed(&[step.name()]), Ok(Command::One(s)) if s == step));
        }
        assert_eq!(parsed(&[]).err(), Some(CliError::MissingCommand));
        assert_eq!(
            parsed(&["nope"]).err(),
            Some(CliError::UnknownCommand("nope".to_owned()))
        );
        assert_eq!(
            parsed(&["deps", "--fix"]).err(),
            Some(CliError::UnknownOption("--fix".to_owned()))
        );
        assert_eq!(
            parsed(&["deps", "extra"]).err(),
            Some(CliError::UnexpectedArgument("extra".to_owned()))
        );
    }

    #[test]
    fn ci_runs_every_step_even_after_failures() {
        let mut ran = Vec::new();
        let failed = failed_steps(&Step::ALL, |step| {
            ran.push(step);
            !matches!(step, Step::Deps | Step::Clippy)
        });
        assert_eq!(ran, Step::ALL);
        assert_eq!(failed, ["deps", "clippy"]);
        // The quick checks come first, and the gate has every one.
        assert_eq!(Step::ALL.last(), Some(&Step::Test));
        assert_eq!(Step::ALL.len(), 9);
    }

    #[test]
    fn the_cargo_steps_are_the_gate_the_docs_name() {
        assert_eq!(FMT, ["fmt", "--all", "--check"]);
        assert!(CLIPPY.starts_with(&["clippy", "--workspace", "--all-targets"]));
        assert!(CLIPPY.ends_with(&["--", "-D", "warnings"]));
        assert!(TEST.starts_with(&["test", "--workspace"]));
        for args in [CLIPPY, TEST] {
            assert!(args.contains(&"--offline") && args.contains(&"--locked"));
        }
    }

    #[test]
    fn usage_names_every_command() {
        for step in Step::ALL {
            assert!(
                USAGE.contains(&format!("\n  {} ", step.name())),
                "{}",
                step.name()
            );
        }
    }
}
