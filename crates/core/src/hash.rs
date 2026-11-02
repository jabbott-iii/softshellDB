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

//! Non-cryptographic hashes.
//!
//! - [`fnv1a64`] is stable: its output is part of on-disk formats (the SST
//!   bloom filter) and never changes.
//! - [`FastHasher`] is for in-memory maps ([`FastHashMap`],
//!   [`FastHashSet`]). It is fast and deterministic, unlike `std`'s
//!   randomly seeded `RandomState`, so a map built by the same operations
//!   behaves the same on every run of one build on one target. Its output
//!   is never persisted and may change between releases.
//!
//! Neither resists deliberate collisions. Their output is predictable, so
//! someone who chooses many keys can make them collide: a map keyed by
//! values an untrusted client picks in bulk must be small and bounded, or
//! a `BTreeMap`.
//!
//! ```
//! use softshell_core::hash::{FastHashMap, fnv1a64};
//!
//! assert_eq!(fnv1a64(b"a"), 0xAF63_DC4C_8601_EC8C);
//!
//! let mut sizes: FastHashMap<&str, usize> = FastHashMap::default();
//! sizes.insert("notes", 3);
//! assert_eq!(sizes.get("notes"), Some(&3));
//! ```

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

const FNV_OFFSET_BASIS: u64 = 0xCBF2_9CE4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

/// The 64-bit FNV-1a hash of `data`. Stable: it may be persisted.
#[must_use]
pub const fn fnv1a64(data: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    let mut rest = data;
    while let [byte, tail @ ..] = rest {
        hash = (hash ^ *byte as u64).wrapping_mul(FNV_PRIME);
        rest = tail;
    }
    hash
}

/// The mixing multiplier: 2^64 divided by the golden ratio, made odd.
const MIX: u64 = 0x9E37_79B9_7F4A_7C15;

/// The state before anything is written: an arbitrary odd constant.
const START: u64 = 0x5851_F42D_4C95_7F2D;

/// Multiplies `a` by `b` into 128 bits and folds the halves together with
/// XOR, so the high input bits reach the low output bits.
fn fold(a: u64, b: u64) -> u64 {
    let product = u128::from(a) * u128::from(b);
    let [low, high] = [product as u64, (product >> 64) as u64];
    low ^ high
}

/// SplitMix64's output function (Stafford's "Mix13"): a bijection in which
/// every input bit affects every output bit. A folded multiply alone leaves
/// a pattern in the low bits when keys differ only in their low bits.
const fn avalanche(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A fast, deterministic [`Hasher`] for in-memory maps: each 8-byte word
/// is XORed into the state, which is then mixed by a folded multiply, and
/// [`finish`](Hasher::finish) avalanches the result. Its output is the same
/// on every supported target and run; it is never persisted. See the
/// [module documentation](self) on collisions.
#[derive(Clone, Debug)]
pub struct FastHasher {
    state: u64,
}

impl FastHasher {
    fn add(&mut self, word: u64) {
        self.state = fold(self.state ^ word, MIX);
    }
}

impl Default for FastHasher {
    fn default() -> Self {
        Self { state: START }
    }
}

impl Hasher for FastHasher {
    fn finish(&self) -> u64 {
        avalanche(self.state)
    }

    /// Mixes `bytes` in 8-byte little-endian words. A partial last word is
    /// padded with zeros and holds its byte count in its top byte, which the
    /// padding always leaves free, so `write(b"a")` and `write(b"a\0")`
    /// differ.
    fn write(&mut self, bytes: &[u8]) {
        let (words, rest) = bytes.as_chunks::<8>();
        for word in words {
            self.add(u64::from_le_bytes(*word));
        }
        if !rest.is_empty() {
            let mut last = [0u8; 8];
            for (dst, src) in last.iter_mut().zip(rest) {
                *dst = *src;
            }
            // `rest` holds 1 to 7 bytes, so byte 7 is padding.
            last[7] = rest.len() as u8;
            self.add(u64::from_le_bytes(last));
        }
    }

    fn write_u8(&mut self, value: u8) {
        self.add(u64::from(value));
    }

    fn write_u16(&mut self, value: u16) {
        self.add(u64::from(value));
    }

    fn write_u32(&mut self, value: u32) {
        self.add(u64::from(value));
    }

    fn write_u64(&mut self, value: u64) {
        self.add(value);
    }

    fn write_u128(&mut self, value: u128) {
        self.add(value as u64);
        self.add((value >> 64) as u64);
    }

    fn write_usize(&mut self, value: usize) {
        // Every supported target is 64-bit, so this is lossless.
        self.add(value as u64);
    }
}

/// Builds [`FastHasher`]s for [`FastHashMap`] and [`FastHashSet`].
pub type FastBuildHasher = BuildHasherDefault<FastHasher>;

/// A `HashMap` with [`FastHasher`]. Build one with `FastHashMap::default()`.
/// Its iteration order depends on the whole sequence of operations
/// (capacity, inserts, removals, `reserve`), the target and the toolchain:
/// never let it reach disk, a result or a test assertion.
pub type FastHashMap<K, V> = HashMap<K, V, FastBuildHasher>;

/// A `HashSet` with [`FastHasher`]. Build one with `FastHashSet::default()`.
/// Its iteration order depends on the whole sequence of operations
/// (capacity, inserts, removals, `reserve`), the target and the toolchain:
/// never let it reach disk, a result or a test assertion.
pub type FastHashSet<T> = HashSet<T, FastBuildHasher>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::hash::{BuildHasher, Hash};

    fn hash_of<T: Hash + ?Sized>(value: &T) -> u64 {
        FastBuildHasher::default().hash_one(value)
    }

    fn written(bytes: &[u8]) -> u64 {
        let mut h = FastHasher::default();
        h.write(bytes);
        h.finish()
    }

    /// Byte `i` of the shared test pattern.
    fn pattern(len: usize) -> Vec<u8> {
        (0..len)
            .map(|i| u8::try_from((i * 31 + 7) % 256).unwrap())
            .collect()
    }

    /// The published FNV-1a test values, and every length 0 to 256 against
    /// a value computed independently from the definition.
    #[test]
    fn fnv1a64_known_values() {
        assert_eq!(fnv1a64(b""), 0xCBF2_9CE4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xAF63_DC4C_8601_EC8C);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_F739_67E8);
        let mut all = Vec::new();
        for len in 0..=256 {
            all.extend_from_slice(&fnv1a64(&pattern(len)).to_le_bytes());
        }
        assert_eq!(fnv1a64(&all), 0x0AE6_71CB_951A_BB38);
        const AT_COMPILE_TIME: u64 = fnv1a64(b"a");
        assert_eq!(AT_COMPILE_TIME, 0xAF63_DC4C_8601_EC8C);
    }

    /// Pins the output, so any change to the algorithm is a deliberate one.
    /// The values were computed by an independent implementation of the
    /// documented algorithm and of `std`'s `Hash` encodings (`str` adds a
    /// `0xFF` terminator, slices a length prefix, `u128` its low half
    /// first).
    #[test]
    fn fast_hasher_output_is_pinned() {
        assert_eq!(FastHasher::default().finish(), 0xF2EE_2134_306F_F565);
        assert_eq!(hash_of(&0u64), 0x323F_1EAB_72B5_15DF);
        assert_eq!(hash_of(&1u64), 0x8DEA_F538_ECB5_9137);
        assert_eq!(hash_of(&u64::MAX), 0x6186_6E79_BC28_D6AD);
        assert_eq!(hash_of(&7u32), 0x9402_C914_726F_D457);
        assert_eq!(hash_of(&7u16), 0x9402_C914_726F_D457);
        assert_eq!(hash_of(&0x1234u16), 0x5793_6493_52F2_43DE);
        assert_eq!(hash_of(&(1u128 << 100)), 0x6563_36C8_7E28_63AE);
        assert_eq!(hash_of("notes"), 0x3593_B365_152B_906B);
        assert_eq!(hash_of(&b"notes"[..]), 0x309F_FFE3_4285_8282);
        assert_eq!(hash_of(&(3u64, "a")), 0x98DD_91C3_09A9_2992);
        assert_eq!(written(b""), 0xF2EE_2134_306F_F565);
        assert_eq!(written(b"12345678"), 0x70F3_7E4C_0AAC_4C22);
        assert_eq!(written(&pattern(257)), 0x8AAC_7BE5_7A48_3753);
        // Trailing zero bytes change the hash.
        assert_eq!(hash_of("a"), 0x85DE_8788_25CB_25CC);
        assert_eq!(hash_of("a\0"), 0x9C2D_67A1_41DD_3D71);
    }

    #[test]
    fn trailing_zero_bytes_never_collide() {
        // "a" followed by 0 to 16 zero bytes: 17 distinct hashes.
        let mut hashes: Vec<u64> = (0..=16)
            .map(|zeros| {
                let mut bytes = vec![b'a'];
                bytes.resize(1 + zeros, 0);
                written(&bytes)
            })
            .collect();
        hashes.sort_unstable();
        hashes.dedup();
        assert_eq!(hashes.len(), 17);
        let strs: Vec<u64> = ["id", "id\0", "id\0\0", "notes", "notes\0\0\0"]
            .iter()
            .map(|s| hash_of(*s))
            .collect();
        let mut unique = strs.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), strs.len());
    }

    #[test]
    fn integer_and_byte_writes_agree() {
        // `write_u64(v)` and `write(&v.to_le_bytes())` hash the same word.
        for v in [0u64, 1, 0xDEAD_BEEF, u64::MAX] {
            let mut h = FastHasher::default();
            h.write_u64(v);
            assert_eq!(h.finish(), written(&v.to_le_bytes()));
        }
    }

    #[test]
    fn no_collisions_among_sequential_keys() {
        let mut hashes: Vec<u64> = (0..1_000_000u64).map(|v| hash_of(&v)).collect();
        hashes.sort_unstable();
        hashes.dedup();
        assert_eq!(hashes.len(), 1_000_000);
        let mut two_bytes: Vec<u64> = (0..=u16::MAX).map(|v| written(&v.to_le_bytes())).collect();
        two_bytes.sort_unstable();
        two_bytes.dedup();
        assert_eq!(two_bytes.len(), 65_536);
    }

    /// `HashMap` picks a bucket from the low bits and a tag from the top 7:
    /// both must spread structured keys evenly, whether they differ in
    /// their low bits, by a power-of-two stride, or only in high bits.
    #[test]
    fn low_and_top_bits_are_spread() {
        /// A name and the key it makes from 0, 1, 2, …
        type Shape = (&'static str, fn(u64) -> u64);
        let shapes: [Shape; 3] = [
            ("sequential", |v| v),
            ("stride 4096", |v| v << 12),
            ("high bits", |v| v << 40),
        ];
        for (shape, key) in shapes {
            let mut low = vec![0u32; 1024];
            let mut top = vec![0u32; 128];
            for v in 0..131_072u64 {
                let h = hash_of(&key(v));
                low[usize::try_from(h & 1023).unwrap()] += 1;
                top[usize::try_from(h >> 57).unwrap()] += 1;
            }
            // Expected 128 and 1,024 per bucket; the bounds are more than five
            // standard deviations wide.
            assert!(
                low.iter().all(|&n| (64..=192).contains(&n)),
                "{shape}: {low:?}"
            );
            assert!(
                top.iter().all(|&n| (768..=1280).contains(&n)),
                "{shape}: {top:?}"
            );
        }
    }

    #[test]
    fn maps_and_sets_work() {
        let mut map: FastHashMap<String, u32> = FastHashMap::default();
        let mut set: FastHashSet<u64> = FastHashSet::default();
        for i in 0..1_000u32 {
            map.insert(i.to_string(), i);
            set.insert(u64::from(i) * 3);
        }
        assert_eq!(map.get("999"), Some(&999));
        assert!(set.contains(&2_997));
        assert!(!set.contains(&2_998));
    }
}
