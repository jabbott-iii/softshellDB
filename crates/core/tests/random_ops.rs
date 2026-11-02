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

//! Random sequences of reader and varint calls on random input, with
//! hostile sizes and limits: nothing panics, a failed call changes
//! nothing, and offsets track positions. A fixed-seed stand-in for the
//! property harness that M0.4 brings.

use std::num::NonZeroUsize;

use softshell_core::bytes::ByteReader;
use softshell_core::error::CoreError;
use softshell_core::varint;

/// xorshift64: deterministic and enough for a smoke test.
struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())]
    }
}

/// The reader's observable state.
fn state(r: &ByteReader<'_>) -> (usize, u64, usize) {
    (r.position(), r.offset(), r.remaining())
}

/// Runs one random call on `r`; returns any reader it split off.
fn step<'a>(
    rng: &mut XorShift,
    r: &mut ByteReader<'a>,
) -> Result<Option<ByteReader<'a>>, CoreError> {
    let size = match rng.below(5) {
        0 => r.remaining(),
        1 => r.remaining() + 1,
        2 => usize::MAX,
        _ => rng.below(12),
    };
    let max = rng.pick(&[0, 1, 16, usize::MAX]);
    let min = NonZeroUsize::new(rng.pick(&[1, 2, 4, 9, usize::MAX])).expect("not zero");
    let random = rng.next();
    let count = rng.pick(&[0, 1, 3, u64::MAX, random]);
    match rng.below(16) {
        0 => r.read_u8().map(drop),
        1 => r.read_u16_le().map(drop),
        2 => r.read_u32_be().map(drop),
        3 => r.read_u64_le().map(drop),
        4 => r.read_array::<16>().map(drop),
        5 => r.read_bytes(size).map(drop),
        6 => return r.sub_reader(size).map(Some),
        7 => return r.split_tail(size).map(Some),
        8 => r.check_count(count, min).map(drop),
        9 => r.finish(),
        10 => varint::read_u32(r).map(drop),
        11 => varint::read_u64(r).map(drop),
        12 => varint::read_i64(r).map(drop),
        13 => varint::read_len(r, max).map(drop),
        14 => varint::read_at_most(r, max).map(drop),
        _ => varint::read_count(r, max, min).map(drop),
    }
    .map(|()| None)
}

#[test]
fn random_calls_never_panic_and_failures_change_nothing() {
    let mut rng = XorShift(0x9E37_79B9_7F4A_7C15);
    let interesting = [0u8, 1, 0x7F, 0x80, 0x81, 0xFF];
    for _ in 0..20_000 {
        let len = rng.below(40);
        let data: Vec<u8> = (0..len)
            .map(|_| {
                if rng.below(2) == 0 {
                    rng.pick(&interesting)
                } else {
                    rng.next() as u8
                }
            })
            .collect();
        let base = rng.pick(&[0, 7, u64::MAX - 5]);
        let end = base.saturating_add(data.len() as u64);
        // Each reader with the absolute offset of its first byte.
        let mut readers = vec![(ByteReader::with_base(&data, base), base)];
        for _ in 0..24 {
            let i = rng.below(readers.len());
            let (r, start) = &mut readers[i];
            let start = *start;
            let before = state(r);
            match step(&mut rng, r) {
                Err(_) => assert_eq!(state(r), before, "a failed call moved the reader"),
                Ok(Some(split)) => {
                    // A split-off reader stays inside the input.
                    assert_eq!(split.position(), 0);
                    assert!(split.offset() >= base);
                    assert!(split.offset().saturating_add(split.remaining() as u64) <= end);
                    let split_start = split.offset();
                    readers.push((split, split_start));
                }
                Ok(None) => {}
            }
            let (r, _) = &readers[i];
            assert!(r.position() <= data.len());
            assert!(r.offset().saturating_add(r.remaining() as u64) <= end);
            assert_eq!(r.offset(), start.saturating_add(r.position() as u64));
        }
    }
}
