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

//! Why a check could not run. A problem a check finds in the workspace is
//! a [`Finding`](crate::check::Finding), not an error.

use std::error::Error;
use std::fmt;
use std::io;
use std::process::ExitStatus;

use softshell_codec::json::JsonError;

/// A check that could not run to the end.
///
/// `Display` describes this layer only; the cause, if any, is the
/// [`source`](Error::source), and `main` prints the whole chain.
#[derive(Debug)]
pub(crate) enum XtaskError {
    /// Reading a file or starting a program failed.
    Io {
        /// What was being done, such as "reading docs/roadmap.md".
        doing: String,
        /// The cause.
        source: io::Error,
    },
    /// A program exited unsuccessfully.
    Failed {
        /// The command line, for the message.
        command: String,
        /// How it exited.
        status: ExitStatus,
        /// What it wrote to stderr, trimmed.
        stderr: String,
    },
    /// A file or a program's output is not valid JSON or JSONC.
    Json {
        /// What was being read, such as "the output of cargo metadata".
        what: String,
        /// The cause.
        source: JsonError,
    },
    /// A file or a program's output does not have the expected shape.
    Shape {
        /// What was being read.
        what: String,
        /// What is wrong with it.
        detail: String,
    },
}

impl fmt::Display for XtaskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { doing, .. } => f.write_str(doing),
            Self::Failed {
                command,
                status,
                stderr,
            } => {
                write!(f, "`{command}` failed ({status})")?;
                if !stderr.is_empty() {
                    write!(f, ":\n{stderr}")?;
                }
                Ok(())
            }
            Self::Json { what, .. } => write!(f, "reading {what}"),
            Self::Shape { what, detail } => write!(f, "{what}: {detail}"),
        }
    }
}

impl Error for XtaskError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Json { source, .. } => Some(source),
            Self::Failed { .. } | Self::Shape { .. } => None,
        }
    }
}
