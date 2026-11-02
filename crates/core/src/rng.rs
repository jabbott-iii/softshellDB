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

//! [`Pcg32`], a small seedable random number generator: PCG-XSH-RR with
//! 64-bit state and 32-bit output, from M. E. O'Neill, "PCG: A Family of
//! Simple Fast Space-Efficient Statistically Good Algorithms for Random
//! Number Generation" (2014).
//!
//! The same seed gives the same sequence on every target, so tests, fuzz
//! runs, HNSW level assignment and generated benchmark data replay
//! exactly. It is predictable, so it must never produce secrets: tokens
//! take their bytes from the operating system.
//!
//! ```
//! use softshell_core::rng::Pcg32;
//!
//! let mut rng = Pcg32::new(42, 54);
//! assert_eq!(rng.next_u32(), 0xA15C_02B7);
//! let die = rng.range(1..7);
//! assert!((1..7).contains(&die));
//! ```

use std::ops::Range;

/// The 64-bit LCG multiplier of the reference implementation.
const MULTIPLIER: u64 = 6_364_136_223_846_793_005;

/// A seeded PCG32 generator. Cloning it forks the sequence: both copies
/// produce the same values from then on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pcg32 {
    state: u64,
    /// The stream's increment; always odd.
    inc: u64,
}

impl Pcg32 {
    /// Creates a generator from `seed` on `stream`, as the reference
    /// `pcg32_srandom_r(seed, stream)` does. The top bit of `stream` is
    /// ignored.
    ///
    /// Different streams give different sequences, but not independent
    /// ones: with the same seed (notably 0, or any seed with many low zero
    /// bits) two streams' outputs are strongly correlated. Generators that
    /// must be independent need different, well-mixed seeds, not just
    /// different streams.
    #[must_use]
    pub const fn new(seed: u64, stream: u64) -> Self {
        let mut rng = Self {
            state: 0,
            inc: (stream << 1) | 1,
        };
        rng.step();
        rng.state = rng.state.wrapping_add(seed);
        rng.step();
        rng
    }

    const fn step(&mut self) {
        self.state = self.state.wrapping_mul(MULTIPLIER).wrapping_add(self.inc);
    }

    /// The next 32 random bits.
    pub const fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.step();
        // Truncation to the low 32 bits is part of the output function.
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// The next 64 random bits: two [`next_u32`](Self::next_u32) values,
    /// the first in the low half.
    pub const fn next_u64(&mut self) -> u64 {
        let low = self.next_u32() as u64;
        let high = self.next_u32() as u64;
        (high << 32) | low
    }

    /// A uniform `f64` in `[0, 1)`, with 53 random bits (every multiple of
    /// 2^-53 in that range is equally likely). It can be exactly `0.0`: for a
    /// value in `(0, 1]`, as `-ln(u)` needs, use `1.0 - next_f64()`, which is
    /// exact.
    pub fn next_f64(&mut self) -> f64 {
        const SCALE: f64 = 1.0 / (1u64 << 53) as f64;
        // A value below 2^53 converts to f64 exactly.
        (self.next_u64() >> 11) as f64 * SCALE
    }

    /// A uniform value in `range`, without bias (Lemire's multiply and
    /// reject). An empty range gives `range.start`.
    pub fn range(&mut self, range: Range<u64>) -> u64 {
        let span = range.end.saturating_sub(range.start);
        if span == 0 {
            return range.start;
        }
        let mut product = u128::from(self.next_u64()) * u128::from(span);
        if (product as u64) < span {
            // Reject the few low products that would favor some values.
            let threshold = span.wrapping_neg() % span;
            while (product as u64) < threshold {
                product = u128::from(self.next_u64()) * u128::from(span);
            }
        }
        // The high half of the product is below `span`.
        range.start + (product >> 64) as u64
    }

    /// Shuffles `items` into a uniformly random order (Fisher–Yates).
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            // `j <= i`, so the swap stays in bounds.
            let j = self.range(0..i as u64 + 1) as usize;
            items.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reference implementation's demo, `pcg32-demo` with seed 42 on
    /// stream 54.
    #[test]
    fn matches_the_reference_output() {
        let mut rng = Pcg32::new(42, 54);
        let first: Vec<u32> = (0..6).map(|_| rng.next_u32()).collect();
        assert_eq!(
            first,
            [
                0xA15C_02B7,
                0x7B47_F409,
                0xBA1D_3330,
                0x83D2_F293,
                0xBFA4_784B,
                0xCBED_606E
            ]
        );
        assert_eq!(Pcg32::new(42, 54).next_u64(), 0x7B47_F409_A15C_02B7);
    }

    /// Pins every derived output, so it stays identical on every target
    /// and only changes deliberately. The values were computed by an
    /// independent implementation of the documented algorithms.
    #[test]
    fn derived_outputs_are_pinned() {
        let mut rng = Pcg32::new(1, 2);
        let floats: Vec<u64> = (0..3).map(|_| rng.next_f64().to_bits()).collect();
        assert_eq!(
            floats,
            [
                0x3FD6_F5D4_96C3_D77A,
                0x3FDD_0723_FE6C_91CC,
                0x3FE2_6AC5_7FE4_4DCF
            ]
        );
        let ranged: Vec<u64> = (0..6).map(|_| rng.range(10..1_000)).collect();
        assert_eq!(ranged, [593, 172, 717, 669, 363, 207]);
        let mut items: Vec<u32> = (0..10).collect();
        rng.shuffle(&mut items);
        assert_eq!(items, [1, 7, 6, 2, 4, 3, 5, 0, 8, 9]);
        // Pins how many draws the calls above consumed.
        assert_eq!(rng.next_u32(), 0x5BA5_F12D);
    }

    #[test]
    fn streams_and_seeds_differ() {
        let a: Vec<u32> = {
            let mut r = Pcg32::new(1, 1);
            (0..8).map(|_| r.next_u32()).collect()
        };
        let b: Vec<u32> = {
            let mut r = Pcg32::new(1, 2);
            (0..8).map(|_| r.next_u32()).collect()
        };
        let c: Vec<u32> = {
            let mut r = Pcg32::new(2, 1);
            (0..8).map(|_| r.next_u32()).collect()
        };
        assert_ne!(a, b);
        assert_ne!(a, c);
        let mut r = Pcg32::new(9, 9);
        let mut fork = r.clone();
        assert_eq!(r.next_u64(), fork.next_u64());
    }

    #[test]
    fn range_stays_in_bounds_and_handles_edges() {
        let mut rng = Pcg32::new(3, 4);
        assert_eq!(rng.range(5..5), 5);
        assert_eq!(rng.range(Range { start: 9, end: 2 }), 9);
        assert_eq!(rng.range(7..8), 7);
        // Even a range with one value consumes one draw.
        assert_eq!(rng.next_u32(), 0x1A27_2F64);
        // Spans near 2^64 and 2^63.
        for range in [
            0..u64::MAX,
            1..u64::MAX,
            (1 << 63) - 1..u64::MAX,
            0..(1 << 63) + 1,
            0..3,
        ] {
            for _ in 0..10_000 {
                let v = rng.range(range.clone());
                assert!(range.contains(&v), "{v} not in {range:?}");
            }
        }
    }

    /// A span of `2^63 + 1` makes about half of all draws fall in the
    /// rejected zone, and `3 * 2^62` makes the bias without rejection large
    /// (about half of all values would be multiples of 3). Pinned values
    /// come from an independent implementation.
    #[test]
    fn range_rejects_biased_draws() {
        let mut rng = Pcg32::new(17, 18);
        let pinned: Vec<u64> = (0..6).map(|_| rng.range(0..(1 << 63) + 1)).collect();
        assert_eq!(
            pinned,
            [
                0x1275_C7C0_29DC_4149,
                0x72F8_6EBF_07CE_F069,
                0x55D0_3575_502C_4CD8,
                0x71C4_9661_6CF3_6278,
                0x73B6_72DA_1285_8F96,
                0x2561_7FAC_F8F5_7BE8
            ]
        );
        let mut rng = Pcg32::new(13, 14);
        let thirds = (0..30_000)
            .filter(|_| rng.range(0..3 << 62).is_multiple_of(3))
            .count();
        // Expected 10,000; without rejection it is about 15,000.
        assert!((9_500..=10_500).contains(&thirds), "{thirds}");
    }

    #[test]
    fn range_is_uniform() {
        let mut rng = Pcg32::new(11, 12);
        let mut counts = [0u32; 6];
        for _ in 0..600_000 {
            counts[rng.range(0..6) as usize] += 1;
        }
        // Expected 100,000 each; a fair die stays well within 1%.
        assert!(
            counts.iter().all(|&n| (99_000..=101_000).contains(&n)),
            "{counts:?}"
        );
    }

    #[test]
    fn next_f64_is_in_the_unit_interval() {
        let mut rng = Pcg32::new(5, 6);
        let mut sum = 0.0;
        for _ in 0..100_000 {
            let x = rng.next_f64();
            assert!((0.0..1.0).contains(&x), "{x}");
            sum += x;
        }
        let mean = sum / 100_000.0;
        assert!((mean - 0.5).abs() < 0.005, "{mean}");
    }

    #[test]
    fn shuffle_is_a_uniform_permutation() {
        let mut rng = Pcg32::new(21, 22);
        let mut counts = std::collections::BTreeMap::new();
        for _ in 0..60_000 {
            let mut items = [0u8, 1, 2];
            rng.shuffle(&mut items);
            *counts.entry(items).or_insert(0u32) += 1;
        }
        // All 6 orders, about 10,000 each.
        assert_eq!(counts.len(), 6);
        assert!(
            counts.values().all(|&n| (9_500..=10_500).contains(&n)),
            "{counts:?}"
        );
        let mut empty: [u8; 0] = [];
        rng.shuffle(&mut empty);
        let mut one = [7];
        rng.shuffle(&mut one);
        assert_eq!(one, [7]);
    }
}
