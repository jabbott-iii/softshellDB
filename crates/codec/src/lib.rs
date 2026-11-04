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

//! Byte formats for softshellDB: the `Value` model, JSON and JSONC, the SDOC
//! binary document encoding, the order-preserving keycode, SLZ block
//! compression, record framing and the series chunk codec.
//!
//! Specified in `docs/subsystems/core-and-codec.md`.

pub mod base64;
pub mod json;
pub mod jsonc;
pub mod value;

mod macros;
#[cfg(test)]
mod test_values;
