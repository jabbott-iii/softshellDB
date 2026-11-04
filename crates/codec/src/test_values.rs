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

//! Random [`Value`]s for the property tests.
//!
//! `wide` values cover every variant's edge cases (NaN, ±0, infinities,
//! subnormals, the `i64` and `Timestamp` extremes, escapes and astral
//! characters). `narrow` values come from small pools, so that equal and
//! nearly equal values are common, which order properties need.

use softshell_core::clock::Timestamp;
use softshell_core::id::Id;
use softshell_core::rng::Pcg32;

use crate::value::{Object, Value};

const TWO_POW_53: i64 = 1 << 53;

const INTS: [i64; 14] = [
    0,
    1,
    -1,
    2,
    TWO_POW_53 - 1,
    TWO_POW_53,
    TWO_POW_53 + 1,
    -TWO_POW_53,
    -TWO_POW_53 - 1,
    i64::MAX,
    i64::MAX - 1,
    i64::MIN,
    i64::MIN + 1,
    i64::MAX - 1024,
];

const FLOATS: [f64; 22] = [
    0.0,
    -0.0,
    1.0,
    -1.0,
    0.5,
    2.0,
    f64::NAN,
    f64::INFINITY,
    f64::NEG_INFINITY,
    f64::MAX,
    f64::MIN,
    f64::MIN_POSITIVE,
    5e-324,
    -5e-324,
    9_007_199_254_740_992.0,
    9_007_199_254_740_994.0,
    9_223_372_036_854_775_808.0,
    -9_223_372_036_854_775_808.0,
    9_223_372_036_854_774_784.0,
    0.1,
    1e16,
    1e-5,
];

const F32S: [f32; 12] = [
    0.0,
    -0.0,
    1.0,
    -1.5,
    f32::NAN,
    f32::INFINITY,
    f32::NEG_INFINITY,
    f32::MAX,
    f32::MIN_POSITIVE,
    1e-45,
    0.1,
    16_777_216.0,
];

/// Characters that strings are made of: every one the writer escapes,
/// and a spread of the rest.
const CHARS: [char; 24] = [
    'a',
    'b',
    'Z',
    '0',
    ' ',
    '"',
    '\\',
    '/',
    '\n',
    '\r',
    '\t',
    '\u{8}',
    '\u{c}',
    '\0',
    '\u{1f}',
    '\u{7f}',
    'é',
    '\u{2028}',
    '\u{feff}',
    '\u{fffd}',
    '\u{ffff}',
    '😀',
    '\u{10ffff}',
    '$',
];

/// The keys extended JSON reserves, which a generated object never has.
const RESERVED: [&str; 5] = ["$id", "$ts", "$bytes", "$f32vec", "$float"];

fn below(rng: &mut Pcg32, n: u64) -> u64 {
    rng.range(0..n)
}

fn pick<T: Copy>(rng: &mut Pcg32, items: &[T]) -> T {
    let i = below(rng, items.len() as u64) as usize;
    items[i]
}

/// A random value; `size` bounds its nesting and lengths.
pub(crate) fn wide(rng: &mut Pcg32, size: u32) -> Value {
    generate(rng, size, size / 20 + 1, false)
}

/// A value from small pools, so that equal values are common.
pub(crate) fn narrow(rng: &mut Pcg32, size: u32) -> Value {
    generate(rng, size.min(12), 2, true)
}

fn generate(rng: &mut Pcg32, size: u32, depth: u32, narrow: bool) -> Value {
    let kinds = if depth == 0 { 9 } else { 11 };
    match below(rng, kinds) {
        0 => Value::Null,
        1 => Value::Bool(rng.next_u32() & 1 == 1),
        2 => Value::Int(int(rng, narrow)),
        3 => Value::Float(float(rng, narrow)),
        4 => Value::String(string(rng, size, narrow)),
        5 => Value::Bytes(bytes(rng, size, narrow)),
        6 => Value::Timestamp(timestamp(rng, narrow)),
        7 => Value::Id(id(rng, narrow)),
        8 => Value::F32Vec(f32_vec(rng, size, narrow)),
        9 => {
            let len = container_len(rng, size, narrow);
            Value::Array(
                (0..len)
                    .map(|_| generate(rng, size / 2, depth - 1, narrow))
                    .collect(),
            )
        }
        _ => {
            let len = container_len(rng, size, narrow);
            let mut object = Object::new();
            for _ in 0..len {
                let key = string(rng, 6, narrow);
                if RESERVED.contains(&key.as_str()) {
                    continue;
                }
                let value = generate(rng, size / 2, depth - 1, narrow);
                object.insert(key, value);
            }
            Value::Object(object)
        }
    }
}

/// Narrow containers have up to three elements at every level, so that
/// lexicographic comparison goes past the first.
fn container_len(rng: &mut Pcg32, size: u32, narrow: bool) -> u64 {
    if narrow {
        below(rng, 4)
    } else {
        below(rng, u64::from(size / 4 + 1))
    }
}

pub(crate) fn int(rng: &mut Pcg32, narrow: bool) -> i64 {
    if narrow {
        return pick(
            rng,
            &[
                -2,
                -1,
                0,
                1,
                2,
                TWO_POW_53,
                TWO_POW_53 + 1,
                i64::MAX,
                i64::MIN,
            ],
        );
    }
    match below(rng, 3) {
        0 => pick(rng, &INTS),
        1 => rng.next_u64() as i64,
        _ => {
            let shift = below(rng, 64);
            let magnitude = (rng.next_u64() >> shift) as i64;
            if rng.next_u32() & 1 == 0 {
                magnitude
            } else {
                magnitude.wrapping_neg()
            }
        }
    }
}

pub(crate) fn float(rng: &mut Pcg32, narrow: bool) -> f64 {
    if narrow {
        return pick(
            rng,
            &[
                -1.0,
                -0.0,
                0.0,
                0.5,
                1.0,
                2.0,
                f64::NAN,
                f64::INFINITY,
                9_007_199_254_740_992.0,
                9_223_372_036_854_775_808.0,
                -9_223_372_036_854_775_808.0,
            ],
        );
    }
    match below(rng, 4) {
        0 => pick(rng, &FLOATS),
        1 => f64::from_bits(rng.next_u64()),
        2 => (rng.next_u32() as i32) as f64 / 8.0,
        _ => int(rng, false) as f64,
    }
}

fn string(rng: &mut Pcg32, size: u32, narrow: bool) -> String {
    if narrow {
        return pick(rng, &["", "a", "b", "ab", "é"]).to_owned();
    }
    let len = below(rng, u64::from(size) + 1);
    (0..len).map(|_| pick(rng, &CHARS)).collect()
}

fn bytes(rng: &mut Pcg32, size: u32, narrow: bool) -> Vec<u8> {
    if narrow {
        return pick(rng, &[&[][..], &[0], &[0, 0], &[1], &[255]]).to_vec();
    }
    let len = below(rng, u64::from(size) + 1);
    (0..len).map(|_| rng.next_u32() as u8).collect()
}

fn timestamp(rng: &mut Pcg32, narrow: bool) -> Timestamp {
    if narrow {
        return Timestamp::from_nanos(below(rng, 3) as i64 - 1);
    }
    match below(rng, 4) {
        0 => Timestamp::MIN,
        1 => Timestamp::MAX,
        2 => Timestamp::UNIX_EPOCH,
        _ => Timestamp::from_nanos(rng.next_u64() as i64),
    }
}

fn id(rng: &mut Pcg32, narrow: bool) -> Id {
    if narrow {
        return Id::from_u128(u128::from(below(rng, 3)));
    }
    match below(rng, 3) {
        0 => Id::from_u128(0),
        1 => Id::from_u128(u128::MAX),
        _ => Id::from_u128(u128::from(rng.next_u64()) << 64 | u128::from(rng.next_u64())),
    }
}

fn f32_vec(rng: &mut Pcg32, size: u32, narrow: bool) -> Vec<f32> {
    let len = below(rng, u64::from(size / 4 + 1));
    (0..len)
        .map(|_| {
            if narrow {
                pick(rng, &[-1.0, -0.0, 0.0, 1.0, f32::NAN])
            } else if rng.next_u32() & 1 == 0 {
                pick(rng, &F32S)
            } else {
                f32::from_bits(rng.next_u32())
            }
        })
        .collect()
}
