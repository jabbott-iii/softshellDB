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

//! What a check reports: one [`Finding`] per problem.

use std::fmt;

/// A problem a check found, and where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Finding {
    /// A path relative to the workspace root, with `:line` when the check
    /// knows it; empty when the problem has no one place.
    pub(crate) location: String,
    /// What is wrong, as a sentence for the developer.
    pub(crate) message: String,
}

impl Finding {
    /// A finding at `location`.
    pub(crate) fn new(location: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            location: location.into(),
            message: message.into(),
        }
    }

    /// A finding at line `line` (from 1) of `path`.
    pub(crate) fn at_line(path: &str, line: usize, message: impl Into<String>) -> Self {
        Self::new(format!("{path}:{line}"), message)
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.location.is_empty() {
            f.write_str(&self.message)
        } else {
            write!(f, "{}: {}", self.location, self.message)
        }
    }
}
