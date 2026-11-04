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

//! JSONC: JSON with `//` and `/* */` comments and trailing commas, for
//! files people edit, such as the server's `softshell.jsonc`.
//!
//! Comments may go wherever whitespace may; a `//` comment runs to the end
//! of its line, and `/* */` comments do not nest. A trailing comma may
//! follow the last element of an array or entry of an object, but `[,]`
//! and `[1,,]` are still errors. Everything else, including extended JSON
//! and the limits, is as in [`json`](crate::json); the writers there write
//! plain JSON, which JSONC reads too.
//!
//! ```
//! use softshell_codec::jsonc;
//! use softshell_core::limits::Limits;
//!
//! let text = "{\n  // where the data lives\n  \"dir\": \"/var/lib/softshell\", /* the default */\n}";
//! let v = jsonc::parse(text, Limits::default()).unwrap();
//! assert_eq!(v.as_object().unwrap().get("dir").unwrap().as_str(), Some("/var/lib/softshell"));
//! ```

use softshell_core::limits::Limits;

use crate::json::{Dialect, JsonError};
use crate::value::Value;

/// Parses `text` as one JSONC value: [`json::parse`](crate::json::parse)
/// with comments and trailing commas allowed.
///
/// # Errors
///
/// A [`JsonError`] with the line and column of the first problem found.
pub fn parse(text: &str, limits: Limits) -> Result<Value, JsonError> {
    crate::json::parse_dialect(text, limits, Dialect::Jsonc)
}
