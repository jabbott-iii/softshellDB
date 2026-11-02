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

//! [`Limits`]: the sizes every decoder checks before it allocates.

const KIB: usize = 1024;
const MIB: usize = 1024 * KIB;

/// Size and depth limits passed to every decoder. Each declared size is
/// checked against its limit, and against the input that remains, before
/// anything is allocated.
///
/// New limits may be added in any release, so build one from
/// [`Limits::default`] and change the fields you need:
///
/// ```
/// use softshell_core::limits::Limits;
///
/// let mut limits = Limits::default();
/// limits.max_depth = 16;
/// assert_eq!(limits.max_string_bytes, 16 * 1024 * 1024);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Limits {
    /// Deepest nesting of arrays and objects (JSON, SDOC, query documents).
    /// Default 64.
    pub max_depth: usize,
    /// Largest document (SDOC, JSON, protocol messages). Default 16 MiB.
    pub max_document_bytes: usize,
    /// Longest string or byte string (JSON, SDOC). Default 16 MiB.
    pub max_string_bytes: usize,
    /// Most entries in one array or object. Default 1,000,000.
    pub max_entries: usize,
    /// Longest key (keycode, storage keys). Default 8 KiB.
    pub max_key_bytes: usize,
    /// Most dimensions in a vector (`F32Vec`, vector indexes). Default 4,096.
    pub max_vector_dims: usize,
    /// Largest decompressed block (SLZ blocks, series chunks). Default 4 MiB.
    pub max_decompressed_bytes: usize,
}

impl Default for Limits {
    /// The defaults in `docs/subsystems/core-and-codec.md`.
    fn default() -> Self {
        Self {
            max_depth: 64,
            max_document_bytes: 16 * MIB,
            max_string_bytes: 16 * MIB,
            max_entries: 1_000_000,
            max_key_bytes: 8 * KIB,
            max_vector_dims: 4_096,
            max_decompressed_bytes: 4 * MIB,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_spec() {
        let l = Limits::default();
        assert_eq!(l.max_depth, 64);
        assert_eq!(l.max_document_bytes, 16_777_216);
        assert_eq!(l.max_string_bytes, 16_777_216);
        assert_eq!(l.max_entries, 1_000_000);
        assert_eq!(l.max_key_bytes, 8_192);
        assert_eq!(l.max_vector_dims, 4_096);
        assert_eq!(l.max_decompressed_bytes, 4_194_304);
    }
}
