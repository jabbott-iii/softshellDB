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

//! CRC-32C (Castagnoli), the checksum on every WAL fragment, manifest
//! record and SST block, computed eight bytes at a time ("slicing-by-8")
//! from tables built at compile time.
//!
//! Every checksum written to disk is stored [`mask`]ed: the CRC of bytes
//! that contain their own CRC is weak, and masking breaks that pattern.
//! [`unmask`] undoes it.
//!
//! ```
//! use softshell_core::crc32c;
//!
//! assert_eq!(crc32c::crc32c(b"123456789"), 0xE306_9283);
//! let head = crc32c::crc32c(b"12345");
//! assert_eq!(crc32c::extend(head, b"6789"), 0xE306_9283);
//! assert_eq!(crc32c::unmask(crc32c::mask(0xE306_9283)), 0xE306_9283);
//! ```

/// The Castagnoli polynomial `0x1EDC6F41`, bit-reversed.
const POLY: u32 = 0x82F6_3B78;

/// Added after rotation by [`mask`]; the constant LevelDB's log format uses.
const MASK_DELTA: u32 = 0xA282_EAD8;

/// `TABLES[0]` is the byte-at-a-time table; `TABLES[k][b]` is the CRC
/// contribution of byte `b` followed by `k` zero bytes.
/// A `static`, not a `const`: one copy in memory, even in debug builds.
static TABLES: [[u32; 256]; 8] = make_tables();

const fn make_tables() -> [[u32; 256]; 8] {
    let mut tables = [[0u32; 256]; 8];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ POLY
            } else {
                crc >> 1
            };
            bit += 1;
        }
        tables[0][i] = crc;
        i += 1;
    }
    let mut k = 1;
    while k < 8 {
        let mut i = 0;
        while i < 256 {
            let prev = tables[k - 1][i];
            tables[k][i] = (prev >> 8) ^ tables[0][(prev & 0xFF) as usize];
            i += 1;
        }
        k += 1;
    }
    tables
}

/// The table entry for one byte. Indexing a 256-entry table with a `u8`
/// cannot go out of bounds.
fn entry(k: usize, byte: u8) -> u32 {
    TABLES[k][byte as usize]
}

/// The CRC-32C of `data`.
#[must_use]
pub fn crc32c(data: &[u8]) -> u32 {
    extend(0, data)
}

/// Continues a CRC-32C: `extend(crc32c(a), b) == crc32c(a ++ b)`, so a
/// checksum can cover a type byte and then a payload without copying them
/// together.
#[must_use]
pub fn extend(crc: u32, data: &[u8]) -> u32 {
    let mut c = !crc;
    let (words, rest) = data.as_chunks::<8>();
    for &[b0, b1, b2, b3, b4, b5, b6, b7] in words {
        let [c0, c1, c2, c3] = c.to_le_bytes();
        c = entry(7, b0 ^ c0)
            ^ entry(6, b1 ^ c1)
            ^ entry(5, b2 ^ c2)
            ^ entry(4, b3 ^ c3)
            ^ entry(3, b4)
            ^ entry(2, b5)
            ^ entry(1, b6)
            ^ entry(0, b7);
    }
    for &byte in rest {
        let [c0, ..] = c.to_le_bytes();
        c = entry(0, byte ^ c0) ^ (c >> 8);
    }
    !c
}

/// Masks a CRC for storage: rotates it right by 15 bits and adds a
/// constant.
#[must_use]
pub const fn mask(crc: u32) -> u32 {
    crc.rotate_right(15).wrapping_add(MASK_DELTA)
}

/// Undoes [`mask`].
#[must_use]
pub const fn unmask(masked: u32) -> u32 {
    masked.wrapping_sub(MASK_DELTA).rotate_left(15)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Pcg32;

    /// The bit-at-a-time definition, sharing nothing with the tables.
    fn bitwise(data: &[u8]) -> u32 {
        let mut c = !0u32;
        for &byte in data {
            c ^= u32::from(byte);
            for _ in 0..8 {
                c = if c & 1 == 1 { (c >> 1) ^ POLY } else { c >> 1 };
            }
        }
        !c
    }

    /// Byte `i` of the shared test pattern.
    fn pattern(len: usize) -> Vec<u8> {
        (0..len)
            .map(|i| u8::try_from((i * 31 + 7) % 256).unwrap())
            .collect()
    }

    #[test]
    fn check_value() {
        assert_eq!(crc32c(b"123456789"), 0xE306_9283);
        assert_eq!(crc32c(b""), 0);
    }

    /// RFC 3720 (iSCSI), appendix B.4.
    #[test]
    fn rfc_3720_vectors() {
        let ascending: Vec<u8> = (0..32).collect();
        let descending: Vec<u8> = (0..32).rev().collect();
        let pdu = [
            0x01, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x14, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x14,
            0x00, 0x00, 0x00, 0x18, 0x28, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        assert_eq!(crc32c(&[0x00; 32]), 0x8A91_36AA);
        assert_eq!(crc32c(&[0xFF; 32]), 0x62A8_AB43);
        assert_eq!(crc32c(&ascending), 0x46DD_794E);
        assert_eq!(crc32c(&descending), 0x113F_DB5C);
        assert_eq!(crc32c(&pdu), 0xD996_3A56);
    }

    /// Every length 0 to 256 (all eight-byte remainders and all table
    /// paths), against a value computed independently from the definition.
    #[test]
    fn every_length_matches_the_independent_reference() {
        let mut crcs = Vec::new();
        for len in 0..=256 {
            let data = pattern(len);
            let crc = crc32c(&data);
            assert_eq!(crc, bitwise(&data), "length {len}");
            crcs.extend_from_slice(&crc.to_le_bytes());
        }
        assert_eq!(crc32c(&crcs), 0x79D8_8FBE);
    }

    #[test]
    fn slicing_by_8_equals_the_bytewise_reference_on_random_inputs() {
        let mut rng = Pcg32::new(0x5EED, 1);
        let mut buf = vec![0u8; 4096];
        for _ in 0..2_000 {
            for byte in &mut buf {
                *byte = rng.next_u32().to_le_bytes()[0];
            }
            // Random start and length, so every alignment is covered.
            let start = usize::try_from(rng.range(0..64)).unwrap();
            let len = usize::try_from(rng.range(0..4_000)).unwrap();
            let data = &buf[start..start + len];
            assert_eq!(crc32c(data), bitwise(data), "start {start}, length {len}");
            let split = usize::try_from(rng.range(0..len as u64 + 1)).unwrap();
            let (a, b) = data.split_at(split);
            assert_eq!(extend(crc32c(a), b), crc32c(data), "split at {split}");
        }
    }

    #[test]
    fn mask_round_trips() {
        assert_eq!(mask(0xE306_9283), 0xC78A_B0E5);
        assert_eq!(mask(mask(0xE306_9283)), 0x044E_79ED);
        assert_eq!(mask(0), MASK_DELTA);
        // LevelDB's formula has exactly two fixed points; masking still
        // changes every other value.
        assert_eq!(mask(0x8299_F00B), 0x8299_F00B);
        assert_eq!(mask(0xD7EE_9AB5), 0xD7EE_9AB5);
        let mut rng = Pcg32::new(7, 7);
        for crc in [0, 1, u32::MAX, 0xE306_9283]
            .into_iter()
            .chain((0..10_000).map(|_| rng.next_u32()))
        {
            assert_eq!(unmask(mask(crc)), crc);
            assert_eq!(unmask(unmask(mask(mask(crc)))), crc);
        }
    }
}
