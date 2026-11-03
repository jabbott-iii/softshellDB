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

//! A small command-line parser for `xtask`, the `softshell` binary and the
//! bench: subcommands, `--flag`, `--key value` (or `--key=value`) and
//! positional arguments.
//!
//! The program asks for what it accepts, in order: subcommands first, then
//! options from [`Args`], then positionals from the [`Positionals`] that
//! [`Args::positionals`] returns once every option has been read. Anything
//! left over is an error, so a misspelled option is never ignored.
//!
//! ```
//! use softshell_core::cli::Args;
//!
//! # fn main() -> Result<(), softshell_core::cli::CliError> {
//! let mut args = Args::new(["fuzz-smoke", "--secs", "60", "--target=json"]);
//! assert_eq!(args.subcommand()?.as_deref(), Some("fuzz-smoke"));
//! let secs: u64 = args.value("--secs")?.unwrap_or(10);
//! let target: Option<String> = args.value("--target")?;
//! let verbose = args.flag("--verbose")?;
//! args.positionals()?.finish()?;
//! assert_eq!((secs, target.as_deref(), verbose), (60, Some("json"), false));
//! # Ok(())
//! # }
//! ```
//!
//! Token rules:
//!
//! - A token that starts with `-` is an option, unless it is `-` alone or
//!   a number such as `-5` or `-0.5`. A token after `--` never is.
//! - An option's value is the next token, unless that token is an option;
//!   `--key=value` always works, for a value such as `--x`, `-v` or an
//!   empty one.
//! - An option name matches a whole token or the part before `=`:
//!   `--out` never matches `--output`. An option given twice is an error.
//! - Arguments may be any `OsString`. A value read as text must be UTF-8;
//!   the `_os` methods take paths as they are, but only from the
//!   separate-token form: splitting a non-UTF-8 `--key=value` would need
//!   `unsafe` or an unstable API.
//! - The result does not depend on the order options are read in: a
//!   value is always the token right after its option, and never one that
//!   is itself an option.
//!
//! Every [`CliError`] means the command line was not understood, which
//! the `softshell` binary and `xtask` report with exit status 2.

use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::str::FromStr;

/// The arguments not yet read. Made by [`Args::new`] or [`Args::from_env`].
#[derive(Clone, Debug)]
pub struct Args {
    /// The arguments before `--`; `None` once read.
    tokens: Vec<Option<OsString>>,
    /// The arguments after `--`, which are always positional.
    after_dashes: Vec<OsString>,
    /// Where the next subcommand would be.
    next_subcommand: usize,
}

impl Args {
    /// The arguments, without the program name.
    pub fn new<I>(args: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<OsString>,
    {
        let mut tokens = Vec::new();
        let mut after_dashes = Vec::new();
        let mut dashes = false;
        for arg in args {
            let arg = arg.into();
            if dashes {
                after_dashes.push(arg);
            } else if arg == "--" {
                dashes = true;
            } else {
                tokens.push(Some(arg));
            }
        }
        Self {
            tokens,
            after_dashes,
            next_subcommand: 0,
        }
    }

    /// This process's arguments, without the program name.
    #[must_use]
    pub fn from_env() -> Self {
        Self::new(std::env::args_os().skip(1))
    }

    /// The next subcommand: the argument after the subcommands already
    /// read, if it is not an option. Call it again for a nested subcommand
    /// (`admin token create`). Subcommands come before every option.
    ///
    /// A program that needs one reports `None` with
    /// [`CliError::MissingCommand`] and a name it does not know with
    /// [`CliError::UnknownCommand`].
    ///
    /// # Errors
    ///
    /// [`CliError::NotUnicode`] if the argument is not UTF-8.
    pub fn subcommand(&mut self) -> Result<Option<String>, CliError> {
        let Some(slot) = self.tokens.get_mut(self.next_subcommand) else {
            return Ok(None);
        };
        match slot.as_deref() {
            Some(token) if !is_option(token) => {
                let name = to_text(token, "subcommand")?.to_owned();
                *slot = None;
                self.next_subcommand += 1;
                Ok(Some(name))
            }
            _ => Ok(None),
        }
    }

    /// Whether the flag `name` (such as `--verbose`) was given.
    ///
    /// # Errors
    ///
    /// [`CliError::Repeated`] if it was given twice;
    /// [`CliError::UnexpectedValue`] for `name=value`.
    pub fn flag(&mut self, name: &str) -> Result<bool, CliError> {
        let mut found = false;
        for slot in &mut self.tokens {
            let Some(token) = slot.as_deref() else {
                continue;
            };
            let Some(rest) = token.as_encoded_bytes().strip_prefix(name.as_bytes()) else {
                continue;
            };
            if rest.starts_with(b"=") {
                return Err(CliError::UnexpectedValue(name.to_owned()));
            }
            if rest.is_empty() {
                if found {
                    return Err(CliError::Repeated(name.to_owned()));
                }
                found = true;
                *slot = None;
            }
        }
        Ok(found)
    }

    /// The value of the option `name` (such as `--secs`), parsed as `T`.
    ///
    /// # Errors
    ///
    /// [`CliError::MissingValue`], [`CliError::Repeated`],
    /// [`CliError::NotUnicode`] or [`CliError::InvalidValue`].
    pub fn value<T>(&mut self, name: &str) -> Result<Option<T>, CliError>
    where
        T: FromStr,
        T::Err: fmt::Display,
    {
        let Some(value) = self.value_os(name)? else {
            return Ok(None);
        };
        let text = to_text(&value, name)?;
        text.parse()
            .map(Some)
            .map_err(|err: T::Err| CliError::InvalidValue {
                option: name.to_owned(),
                value: text.to_owned(),
                reason: err.to_string(),
            })
    }

    /// The value of the option `name`, which must be given.
    ///
    /// # Errors
    ///
    /// [`CliError::MissingOption`] if it was not, or any error of
    /// [`value`](Self::value).
    pub fn required_value<T>(&mut self, name: &str) -> Result<T, CliError>
    where
        T: FromStr,
        T::Err: fmt::Display,
    {
        self.value(name)?
            .ok_or_else(|| CliError::MissingOption(name.to_owned()))
    }

    /// The value of the option `name` as given, which must be given.
    ///
    /// # Errors
    ///
    /// [`CliError::MissingOption`] if it was not, or any error of
    /// [`value_os`](Self::value_os).
    pub fn required_value_os(&mut self, name: &str) -> Result<OsString, CliError> {
        self.value_os(name)?
            .ok_or_else(|| CliError::MissingOption(name.to_owned()))
    }

    /// The value of the option `name`, as given (for paths). A value that
    /// is not UTF-8 must follow as its own argument, not after `=`.
    ///
    /// # Errors
    ///
    /// [`CliError::MissingValue`], [`CliError::Repeated`], or
    /// [`CliError::NotUnicode`] for `name=value` with a value that is not
    /// UTF-8.
    pub fn value_os(&mut self, name: &str) -> Result<Option<OsString>, CliError> {
        let mut found = None;
        let mut i = 0;
        while let Some(slot) = self.tokens.get_mut(i) {
            let inline = match slot.as_deref() {
                Some(token) => split_option(token, name)?,
                None => None,
            };
            let Some(inline) = inline else {
                i += 1;
                continue;
            };
            if found.is_some() {
                return Err(CliError::Repeated(name.to_owned()));
            }
            *slot = None;
            let value = match inline {
                Some(value) => value,
                None => match self.tokens.get_mut(i + 1) {
                    Some(next) if next.as_deref().is_some_and(|t| !is_option(t)) => {
                        i += 1;
                        next.take().unwrap_or_default()
                    }
                    _ => return Err(CliError::MissingValue(name.to_owned())),
                },
            };
            found = Some(value);
            i += 1;
        }
        Ok(found)
    }

    /// The positional arguments, once every option has been read.
    ///
    /// # Errors
    ///
    /// [`CliError::UnknownOption`] for the first option no call read.
    pub fn positionals(self) -> Result<Positionals, CliError> {
        let mut args = Vec::new();
        for token in self.tokens.into_iter().flatten() {
            if is_option(&token) {
                let name = token.to_string_lossy();
                let name = name.split_once('=').map_or(&*name, |(name, _)| name);
                return Err(CliError::UnknownOption(name.to_owned()));
            }
            args.push(token);
        }
        args.extend(self.after_dashes);
        args.reverse();
        Ok(Positionals { rest: args })
    }
}

/// The positional arguments, read in order.
#[derive(Clone, Debug)]
pub struct Positionals {
    /// Unread arguments, last first.
    rest: Vec<OsString>,
}

impl Positionals {
    /// The next argument, called `name` in errors (such as `<dir>`),
    /// parsed as `T`.
    ///
    /// # Errors
    ///
    /// [`CliError::MissingArgument`], [`CliError::NotUnicode`] or
    /// [`CliError::InvalidValue`].
    pub fn required<T>(&mut self, name: &'static str) -> Result<T, CliError>
    where
        T: FromStr,
        T::Err: fmt::Display,
    {
        self.optional(name)?.ok_or(CliError::MissingArgument(name))
    }

    /// The next argument parsed as `T`, if there is one.
    ///
    /// # Errors
    ///
    /// [`CliError::NotUnicode`] or [`CliError::InvalidValue`].
    pub fn optional<T>(&mut self, name: &'static str) -> Result<Option<T>, CliError>
    where
        T: FromStr,
        T::Err: fmt::Display,
    {
        let Some(arg) = self.rest.pop() else {
            return Ok(None);
        };
        let text = to_text(&arg, name)?;
        text.parse()
            .map(Some)
            .map_err(|err: T::Err| CliError::InvalidValue {
                option: name.to_owned(),
                value: text.to_owned(),
                reason: err.to_string(),
            })
    }

    /// The next argument as given (for paths).
    ///
    /// # Errors
    ///
    /// [`CliError::MissingArgument`] if there is none.
    pub fn required_os(&mut self, name: &'static str) -> Result<OsString, CliError> {
        self.rest.pop().ok_or(CliError::MissingArgument(name))
    }

    /// The next argument as given, if there is one.
    pub fn optional_os(&mut self) -> Option<OsString> {
        self.rest.pop()
    }

    /// Every argument not yet read, in order.
    #[must_use]
    pub fn rest(mut self) -> Vec<OsString> {
        self.rest.reverse();
        self.rest
    }

    /// Checks that every argument was read.
    ///
    /// # Errors
    ///
    /// [`CliError::UnexpectedArgument`] for the first one left.
    pub fn finish(mut self) -> Result<(), CliError> {
        match self.rest.pop() {
            None => Ok(()),
            Some(arg) => Err(CliError::UnexpectedArgument(
                arg.to_string_lossy().into_owned(),
            )),
        }
    }
}

/// A command line the program does not accept. `Display` says what is
/// wrong in a sentence for the user.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CliError {
    /// An option no call asked for.
    UnknownOption(String),
    /// An option that takes a value was last, or followed by another
    /// option.
    MissingValue(String),
    /// A required option was not given.
    MissingOption(String),
    /// A value that does not parse.
    InvalidValue {
        /// The option, or the positional argument's name.
        option: String,
        /// The value given.
        value: String,
        /// Why it does not parse.
        reason: String,
    },
    /// An option given more than once.
    Repeated(String),
    /// A flag given a value with `=`.
    UnexpectedValue(String),
    /// A positional argument the program does not take.
    UnexpectedArgument(String),
    /// A required positional argument is missing.
    MissingArgument(&'static str),
    /// An argument read as text is not valid UTF-8. Holds the option or
    /// argument name.
    NotUnicode(String),
    /// A subcommand the program does not have.
    UnknownCommand(String),
    /// No subcommand was given where one is needed.
    MissingCommand,
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownOption(name) => write!(f, "unknown option `{name}`"),
            Self::MissingValue(name) => write!(f, "`{name}` needs a value"),
            Self::MissingOption(name) => write!(f, "missing required option `{name}`"),
            Self::InvalidValue {
                option,
                value,
                reason,
            } => write!(f, "invalid value `{value}` for `{option}`: {reason}"),
            Self::Repeated(name) => write!(f, "`{name}` was given more than once"),
            Self::UnexpectedValue(name) => write!(f, "`{name}` does not take a value"),
            Self::UnexpectedArgument(arg) => write!(f, "unexpected argument `{arg}`"),
            Self::MissingArgument(name) => write!(f, "missing argument {name}"),
            Self::NotUnicode(name) => write!(f, "`{name}` is not valid UTF-8"),
            Self::UnknownCommand(name) => write!(f, "unknown command `{name}`"),
            Self::MissingCommand => f.write_str("missing command"),
        }
    }
}

/// A leaf error: it has no source.
impl Error for CliError {}

/// Whether `token` is an option: it starts with `-` and is neither `-`
/// alone nor a number.
fn is_option(token: &OsStr) -> bool {
    let bytes = token.as_encoded_bytes();
    let number = token
        .to_str()
        .is_some_and(|text| text.parse::<f64>().is_ok());
    bytes.len() > 1 && bytes.starts_with(b"-") && !number
}

/// Whether `token` is the option `name`: `Some(None)` for `name` alone,
/// `Some(Some(value))` for `name=value`, `None` for anything else.
fn split_option(token: &OsStr, name: &str) -> Result<Option<Option<OsString>>, CliError> {
    let Some(rest) = token.as_encoded_bytes().strip_prefix(name.as_bytes()) else {
        return Ok(None);
    };
    if rest.is_empty() {
        return Ok(Some(None));
    }
    let Some(value) = rest.strip_prefix(b"=") else {
        return Ok(None);
    };
    // Splitting an `OsStr` needs `unsafe` or an unstable API, so a value
    // after `=` must be UTF-8.
    match std::str::from_utf8(value) {
        Ok(value) => Ok(Some(Some(value.into()))),
        Err(_) => Err(CliError::NotUnicode(name.to_owned())),
    }
}

fn to_text<'a>(arg: &'a OsStr, name: &str) -> Result<&'a str, CliError> {
    arg.to_str()
        .ok_or_else(|| CliError::NotUnicode(name.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Args {
        Args::new(list.iter().copied())
    }

    #[test]
    fn xtask_and_cli_command_lines() {
        let mut a = args(&["crash", "--minutes", "5", "--seed", "0x2A", "--exhaustive"]);
        assert_eq!(a.subcommand().unwrap().as_deref(), Some("crash"));
        assert_eq!(a.value::<u32>("--minutes"), Ok(Some(5)));
        assert_eq!(a.value::<String>("--seed"), Ok(Some("0x2A".to_owned())));
        assert_eq!(a.flag("--exhaustive"), Ok(true));
        assert_eq!(a.flag("--verbose"), Ok(false));
        a.positionals().unwrap().finish().unwrap();

        // Nested subcommands, options in any order, then a positional.
        let mut a = args(&[
            "admin", "token", "create", "--role", "read", "db", "--name", "ci",
        ]);
        assert_eq!(a.subcommand().unwrap().as_deref(), Some("admin"));
        assert_eq!(a.subcommand().unwrap().as_deref(), Some("token"));
        assert_eq!(a.subcommand().unwrap().as_deref(), Some("create"));
        assert_eq!(a.subcommand(), Ok(None), "an option is not a subcommand");
        assert_eq!(a.value::<String>("--name"), Ok(Some("ci".to_owned())));
        assert_eq!(a.value::<String>("--role"), Ok(Some("read".to_owned())));
        let mut p = a.positionals().unwrap();
        assert_eq!(p.required_os("<dir>"), Ok(OsString::from("db")));
        p.finish().unwrap();

        // `export <dir> <collection> [--out file]`, `--out=` form.
        let mut a = args(&["export", "data", "notes", "--out=notes.jsonl"]);
        a.subcommand().unwrap();
        assert_eq!(a.value_os("--out"), Ok(Some(OsString::from("notes.jsonl"))));
        let mut p = a.positionals().unwrap();
        assert_eq!(p.required::<String>("<dir>"), Ok("data".to_owned()));
        assert_eq!(
            p.optional::<String>("<collection>"),
            Ok(Some("notes".to_owned()))
        );
        assert_eq!(p.optional::<String>("<extra>"), Ok(None));
        assert_eq!(p.optional_os(), None);
        p.finish().unwrap();

        // `bench [filter]` with no filter.
        let mut a = args(&["bench"]);
        a.subcommand().unwrap();
        assert_eq!(a.subcommand(), Ok(None));
        assert_eq!(a.positionals().unwrap().rest(), Vec::<OsString>::new());
        assert_eq!(args(&[]).subcommand(), Ok(None));
    }

    #[test]
    fn value_forms() {
        let mut a = args(&["--empty=", "--neg", "-5", "--dash", "-", "--eq=a=b"]);
        assert_eq!(a.value::<String>("--empty"), Ok(Some(String::new())));
        assert_eq!(a.value::<i32>("--neg"), Ok(Some(-5)));
        assert_eq!(a.value::<String>("--dash"), Ok(Some("-".to_owned())));
        assert_eq!(a.value::<String>("--eq"), Ok(Some("a=b".to_owned())));
        assert_eq!(a.value::<String>("--absent"), Ok(None));
        a.positionals().unwrap().finish().unwrap();
        // Numbers are values and positionals, not options; an option-like
        // value needs `=`.
        let mut a = args(&["--off", "-0.5", "--msg=-v", "-3", "-1e3"]);
        assert_eq!(a.value::<f64>("--off"), Ok(Some(-0.5)));
        assert_eq!(a.value::<String>("--msg"), Ok(Some("-v".to_owned())));
        let rest: Vec<OsString> = a.positionals().unwrap().rest();
        assert_eq!(rest, ["-3", "-1e3"]);
        // A name is matched whole, not as a prefix.
        let mut a = args(&["--secsx", "1"]);
        assert_eq!(a.value::<u32>("--secs"), Ok(None));
        assert_eq!(
            a.positionals().unwrap_err(),
            CliError::UnknownOption("--secsx".to_owned())
        );
    }

    #[test]
    fn double_dash_ends_options() {
        let mut a = args(&["run", "--fast", "--", "--not-an-option", "-x", "--"]);
        assert_eq!(a.subcommand().unwrap().as_deref(), Some("run"));
        assert_eq!(a.flag("--fast"), Ok(true));
        assert_eq!(a.flag("--not-an-option"), Ok(false));
        let rest: Vec<OsString> = a.positionals().unwrap().rest();
        assert_eq!(rest, ["--not-an-option", "-x", "--"]);
        // Positionals before `--` come first.
        let mut p = args(&["a", "--", "b"]).positionals().unwrap();
        assert_eq!(p.required::<String>("<x>"), Ok("a".to_owned()));
        assert_eq!(p.required::<String>("<y>"), Ok("b".to_owned()));
        assert_eq!(
            p.required::<String>("<z>"),
            Err(CliError::MissingArgument("<z>"))
        );
    }

    #[test]
    fn errors() {
        assert_eq!(
            args(&["--secs"]).value::<u32>("--secs"),
            Err(CliError::MissingValue("--secs".to_owned()))
        );
        assert_eq!(
            args(&["--secs", "--target", "json"]).value::<u32>("--secs"),
            Err(CliError::MissingValue("--secs".to_owned()))
        );
        assert_eq!(
            args(&["--secs", "1", "--secs=2"]).value::<u32>("--secs"),
            Err(CliError::Repeated("--secs".to_owned()))
        );
        assert_eq!(
            args(&["-v", "-v"]).flag("-v"),
            Err(CliError::Repeated("-v".to_owned()))
        );
        assert_eq!(
            args(&["--verbose=yes"]).flag("--verbose"),
            Err(CliError::UnexpectedValue("--verbose".to_owned()))
        );
        let err = args(&["--secs", "ten"]).value::<u32>("--secs").unwrap_err();
        assert_eq!(
            err,
            CliError::InvalidValue {
                option: "--secs".to_owned(),
                value: "ten".to_owned(),
                reason: "invalid digit found in string".to_owned()
            }
        );
        assert_eq!(
            err.to_string(),
            "invalid value `ten` for `--secs`: invalid digit found in string"
        );
        assert_eq!(
            args(&["--bogus=1", "x"])
                .positionals()
                .unwrap_err()
                .to_string(),
            "unknown option `--bogus`"
        );
        assert_eq!(
            args(&["-h"]).positionals().unwrap_err(),
            CliError::UnknownOption("-h".to_owned())
        );
        let mut p = args(&["a", "b"]).positionals().unwrap();
        assert_eq!(p.required_os("<a>"), Ok(OsString::from("a")));
        assert_eq!(
            p.finish(),
            Err(CliError::UnexpectedArgument("b".to_owned()))
        );
        let mut p = args(&["x"]).positionals().unwrap();
        assert_eq!(
            p.required::<u8>("<count>").unwrap_err().to_string(),
            "invalid value `x` for `<count>`: invalid digit found in string"
        );
        assert_eq!(
            CliError::MissingArgument("<dir>").to_string(),
            "missing argument <dir>"
        );
        assert_eq!(
            CliError::MissingValue("--out".to_owned()).to_string(),
            "`--out` needs a value"
        );
        assert_eq!(
            args(&[]).required_value::<String>("--name"),
            Err(CliError::MissingOption("--name".to_owned()))
        );
        assert_eq!(
            args(&[])
                .required_value_os("--config")
                .unwrap_err()
                .to_string(),
            "missing required option `--config`"
        );
        assert_eq!(
            args(&["--config", "a"]).required_value_os("--config"),
            Ok(OsString::from("a"))
        );
        assert_eq!(
            CliError::UnknownCommand("lint".to_owned()).to_string(),
            "unknown command `lint`"
        );
        assert_eq!(CliError::MissingCommand.to_string(), "missing command");
    }

    #[test]
    fn a_lone_dash_is_an_argument() {
        // `-` (stdin, by convention) is a positional, and can be a
        // subcommand name the program then rejects.
        let mut a = args(&["import", "d", "c", "-"]);
        assert_eq!(a.subcommand().unwrap().as_deref(), Some("import"));
        assert_eq!(a.subcommand().unwrap().as_deref(), Some("d"));
        let rest: Vec<OsString> = a.positionals().unwrap().rest();
        assert_eq!(rest, ["c", "-"]);
        assert_eq!(args(&["-"]).subcommand().unwrap().as_deref(), Some("-"));
        let mut p = args(&["x", "y"]).positionals().unwrap();
        assert_eq!(p.optional_os(), Some(OsString::from("x")));
        assert_eq!(p.optional_os(), Some(OsString::from("y")));
        assert_eq!(p.optional_os(), None);
    }

    #[test]
    fn every_error_displays_a_sentence() {
        let cases = [
            (
                CliError::UnknownOption("--x".to_owned()),
                "unknown option `--x`",
            ),
            (
                CliError::MissingValue("--x".to_owned()),
                "`--x` needs a value",
            ),
            (
                CliError::MissingOption("--x".to_owned()),
                "missing required option `--x`",
            ),
            (
                CliError::InvalidValue {
                    option: "--x".to_owned(),
                    value: "v".to_owned(),
                    reason: "bad".to_owned(),
                },
                "invalid value `v` for `--x`: bad",
            ),
            (
                CliError::Repeated("--x".to_owned()),
                "`--x` was given more than once",
            ),
            (
                CliError::UnexpectedValue("--x".to_owned()),
                "`--x` does not take a value",
            ),
            (
                CliError::UnexpectedArgument("a".to_owned()),
                "unexpected argument `a`",
            ),
            (CliError::MissingArgument("<dir>"), "missing argument <dir>"),
            (
                CliError::NotUnicode("--x".to_owned()),
                "`--x` is not valid UTF-8",
            ),
            (
                CliError::UnknownCommand("lint".to_owned()),
                "unknown command `lint`",
            ),
            (CliError::MissingCommand, "missing command"),
        ];
        for (error, text) in cases {
            assert_eq!(error.to_string(), text);
        }
    }

    /// Reading the options in another order gives the same result.
    #[test]
    fn reading_order_does_not_matter() {
        // Single-dash names too: `-v` is never taken as `--msg`'s value.
        for flag_first in [true, false] {
            let mut a = args(&["--msg", "-v"]);
            let (flag, msg) = if flag_first {
                let flag = a.flag("-v");
                (flag, a.value::<String>("--msg"))
            } else {
                let msg = a.value::<String>("--msg");
                (a.flag("-v"), msg)
            };
            assert_eq!(flag, Ok(true));
            assert_eq!(msg, Err(CliError::MissingValue("--msg".to_owned())));
        }
        for short_first in [true, false] {
            let mut a = args(&["--x", "-n", "5"]);
            let (n, x) = if short_first {
                let n = a.value::<u32>("-n");
                (n, a.value::<String>("--x"))
            } else {
                let x = a.value::<String>("--x");
                (a.value::<u32>("-n"), x)
            };
            assert_eq!(n, Ok(Some(5)));
            assert_eq!(x, Err(CliError::MissingValue("--x".to_owned())));
        }
        let line = ["crash", "--seed", "--exhaustive", "7"];
        let mut a = args(&line);
        a.subcommand().unwrap();
        assert_eq!(a.flag("--exhaustive"), Ok(true));
        assert_eq!(
            a.value::<u64>("--seed"),
            Err(CliError::MissingValue("--seed".to_owned()))
        );
        let mut b = args(&line);
        b.subcommand().unwrap();
        assert_eq!(
            b.value::<u64>("--seed"),
            Err(CliError::MissingValue("--seed".to_owned()))
        );

        let line = ["create", "--name", "--role", "read", "db"];
        for name_first in [true, false] {
            let mut a = args(&line);
            a.subcommand().unwrap();
            let (name, role) = if name_first {
                let name = a.value::<String>("--name");
                (name, a.value::<String>("--role"))
            } else {
                let role = a.value::<String>("--role");
                (a.value::<String>("--name"), role)
            };
            assert_eq!(name, Err(CliError::MissingValue("--name".to_owned())));
            assert_eq!(role, Ok(Some("read".to_owned())));
        }
        // A subcommand is found by position, after options were read too.
        let mut a = args(&["--x", "v", "cmd"]);
        assert_eq!(a.value::<String>("--x"), Ok(Some("v".to_owned())));
        assert_eq!(a.subcommand(), Ok(None));
    }

    /// The `_os` methods return arguments as given. (`NotUnicode` is not
    /// tested: `std` has no safe, portable way to build an `OsString` that
    /// is not UTF-8, and core has no platform code.)
    #[test]
    fn values_and_positionals_keep_os_strings() {
        let mut a = Args::new([OsString::from("--out"), OsString::from("dir/ünïcode")]);
        assert_eq!(a.value_os("--out"), Ok(Some(OsString::from("dir/ünïcode"))));
        let mut p = Args::new([OsString::from("ü")]).positionals().unwrap();
        assert_eq!(p.required_os("<dir>"), Ok(OsString::from("ü")));
    }
}
