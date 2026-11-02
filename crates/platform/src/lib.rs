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

//! Operating-system behavior for softshellDB: the `Vfs` file-system trait,
//! its per-OS implementation `OsVfs`, the in-memory `MemVfs`, the
//! fault-injecting `FaultVfs`, and OS randomness.
//!
//! The API is specified in `docs/subsystems/platform.md`, and the
//! durability semantics on each OS in `docs/03-platforms.md`.

// One of the two crates allowed `unsafe` (ADR-0005). Every `unsafe` block
// still needs a `// SAFETY:` comment, and every `pub unsafe fn` a
// `# Safety` section.
#![allow(unsafe_code)]
