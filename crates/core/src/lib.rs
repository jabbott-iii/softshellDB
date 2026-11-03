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

//! Foundation types shared by every softshellDB crate: checked byte readers
//! and writers, varints, errors, `Limits`, CRC-32C, hashes, SHA-256, PCG32,
//! ids, the clock, civil time, logging, metrics, a command-line parser and
//! the test harness.
//!
//! Specified in `docs/subsystems/core-and-codec.md`.

pub mod bytes;
pub mod civil;
pub mod cli;
pub mod clock;
pub mod crc32c;
pub mod error;
pub mod hash;
pub mod id;
pub mod limits;
pub mod log;
pub mod metrics;
pub mod rng;
pub mod sha256;
pub mod testing;
pub mod varint;
