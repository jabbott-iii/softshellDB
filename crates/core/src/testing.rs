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

//! The test harness: seeded property tests ([`check`], [`check_sized`])
//! and approximate float comparison ([`assert_approx!`]). The fuzz driver
//! joins this module in M0.7 (`docs/04-quality.md`, ADR-0020).
//!
//! ```
//! use softshell_core::testing::check;
//!
//! check("doubling is even", 100, |rng| rng.next_u32() / 2, |&n| {
//!     if (n * 2) % 2 == 0 { Ok(()) } else { Err(format!("{n} doubled is odd")) }
//! });
//! ```
//!
//! Every case gets its own generator, seeded from the run's seed, the
//! property's name and the case number, so cases and properties draw
//! independent values. The run's seed is `SOFTSHELL_SEED` (decimal, or hex
//! after `0x`) or [`DEFAULT_SEED`], so CI runs replay exactly. A failure
//! panics with the case, the input, the property's message and the line
//! that replays it: `replay with SOFTSHELL_SEED=…`. A panicking generator
//! or property is a failure like any other.

use std::any::Any;
use std::ffi::OsStr;
use std::fmt::Debug;
use std::panic::{self, AssertUnwindSafe};

use crate::hash::fnv1a64;
use crate::log::capped_string;
use crate::rng::Pcg32;

#[doc(inline)]
pub use crate::__assert_approx as assert_approx;

/// The largest size [`check_sized`] asks for.
pub const MAX_SIZE: u32 = 100;

/// The seed when `SOFTSHELL_SEED` is not set.
pub const DEFAULT_SEED: u64 = 0x5EED;

/// The tolerance [`assert_approx!`] uses when none is given.
pub const DEFAULT_TOLERANCE: f64 = 1e-9;

/// The environment variable that sets the seed.
const SEED_VAR: &str = "SOFTSHELL_SEED";

/// The most bytes of an input's `Debug` text, or of a message, a failure
/// report shows.
const MAX_REPORT_BYTES: usize = 4096;

/// SplitMix64's increment, 2^64 divided by the golden ratio.
const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;

/// The run's seed: `SOFTSHELL_SEED`, or [`DEFAULT_SEED`] if it is not set.
///
/// # Panics
///
/// If `SOFTSHELL_SEED` is set but is not a `u64` in decimal or `0x` hex;
/// an empty value is not "unset".
#[must_use]
pub fn seed() -> u64 {
    match std::env::var_os(SEED_VAR) {
        None => DEFAULT_SEED,
        Some(text) => parse_seed(&text).unwrap_or_else(|reason| panic!("{SEED_VAR}: {reason}")),
    }
}

fn parse_seed(text: &OsStr) -> Result<u64, String> {
    let text = text.to_str().ok_or("not valid UTF-8")?;
    let parsed = match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => u64::from_str_radix(hex, 16),
        None => text.parse(),
    };
    parsed.map_err(|err| format!("`{text}` is not a u64 in decimal or 0x hex: {err}"))
}

/// Runs `property` on `cases` inputs made by `generate`, each from its own
/// seeded [`Pcg32`].
///
/// # Panics
///
/// On the first case whose generator or property panics, or whose property
/// returns an error, with a report that names the seed to replay it.
#[track_caller]
pub fn check<T: Debug>(
    name: &str,
    cases: u32,
    generate: impl Fn(&mut Pcg32) -> T,
    property: impl Fn(&T) -> Result<(), String>,
) {
    run_check(seed(), name, cases, &generate, &property);
}

/// [`check`] for inputs with a size, which grows from 0 to [`MAX_SIZE`]
/// over the run. A failing case is retried with the same generator seed at
/// every smaller size, and the smallest size that still fails is
/// reported.
///
/// # Panics
///
/// As [`check`] does.
#[track_caller]
pub fn check_sized<T: Debug>(
    name: &str,
    cases: u32,
    generate: impl Fn(&mut Pcg32, u32) -> T,
    property: impl Fn(&T) -> Result<(), String>,
) {
    run_check_sized(seed(), name, cases, &generate, &property);
}

#[track_caller]
fn run_check<T: Debug>(
    seed: u64,
    name: &str,
    cases: u32,
    generate: &dyn Fn(&mut Pcg32) -> T,
    property: &dyn Fn(&T) -> Result<(), String>,
) {
    for case in 0..cases {
        let mut rng = case_rng(seed, name, case);
        if let Err(failure) = attempt(|| generate(&mut rng), property) {
            fail(name, seed, case, cases, None, &failure);
        }
    }
}

#[track_caller]
fn run_check_sized<T: Debug>(
    seed: u64,
    name: &str,
    cases: u32,
    generate: &dyn Fn(&mut Pcg32, u32) -> T,
    property: &dyn Fn(&T) -> Result<(), String>,
) {
    let try_size = |case: u32, size: u32| {
        let mut rng = case_rng(seed, name, case);
        attempt(|| generate(&mut rng, size), property)
    };
    for case in 0..cases {
        let size = size_for(case, cases);
        if let Err(failure) = try_size(case, size) {
            let (smallest, failure) = (0..size)
                .find_map(|smaller| try_size(case, smaller).err().map(|f| (smaller, f)))
                .unwrap_or((size, failure));
            fail(name, seed, case, cases, Some((size, smallest)), &failure);
        }
    }
}

/// Case `case`'s size: 0 for the first case, [`MAX_SIZE`] for the last,
/// evenly between.
fn size_for(case: u32, cases: u32) -> u32 {
    if cases <= 1 {
        return 0;
    }
    let size = u64::from(case) * u64::from(MAX_SIZE) / u64::from(cases - 1);
    u32::try_from(size).unwrap_or(MAX_SIZE).min(MAX_SIZE)
}

/// Case `case`'s generator. Two SplitMix64 outputs from a state that
/// mixes the seed, the property name and the case give the PCG seed and
/// stream; same-seed PCG streams are correlated, so the seeds differ too.
/// Each case's two states are its own: case `i` uses `x + (2i+1)·G` and
/// `x + (2i+2)·G`.
fn case_rng(seed: u64, name: &str, case: u32) -> Pcg32 {
    let mut state = (seed ^ fnv1a64(name.as_bytes()))
        .wrapping_add(u64::from(case).wrapping_mul(2).wrapping_mul(GOLDEN));
    let pcg_seed = splitmix64(&mut state);
    let stream = splitmix64(&mut state);
    Pcg32::new(pcg_seed, stream)
}

/// SplitMix64 (Steele, Lea and Flood, 2014): advances `state` and returns
/// a well-mixed value.
const fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(GOLDEN);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Why a case failed.
struct Failure {
    /// The input's `Debug` text; `None` if the generator panicked.
    input: Option<String>,
    message: String,
}

/// Generates an input and runs the property on it, catching panics.
fn attempt<T: Debug>(
    generate: impl FnOnce() -> T,
    property: &dyn Fn(&T) -> Result<(), String>,
) -> Result<(), Failure> {
    let input = match panic::catch_unwind(AssertUnwindSafe(generate)) {
        Ok(input) => input,
        Err(payload) => {
            return Err(Failure {
                input: None,
                message: format!("the generator panicked: {}", payload_text(&*payload)),
            });
        }
    };
    let message = match panic::catch_unwind(AssertUnwindSafe(|| property(&input))) {
        Ok(Ok(())) => return Ok(()),
        Ok(Err(message)) => capped_string(format_args!("{message}"), MAX_REPORT_BYTES),
        Err(payload) => format!("the property panicked: {}", payload_text(&*payload)),
    };
    let input = panic::catch_unwind(AssertUnwindSafe(|| {
        capped_string(format_args!("{input:?}"), MAX_REPORT_BYTES)
    }))
    .unwrap_or_else(|_| "<its Debug impl panicked>".to_owned());
    Err(Failure {
        input: Some(input),
        message,
    })
}

/// The text of a panic: a `panic!` or `assert!` with only a literal
/// message gives a `&'static str`; a formatted one, `assert_eq!` and
/// `expect` give a `String`.
fn payload_text(payload: &(dyn Any + Send)) -> String {
    let text = payload
        .downcast_ref::<&'static str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("<a panic payload that is not text>");
    capped_string(format_args!("{text}"), MAX_REPORT_BYTES)
}

/// Panics with the failure report.
#[track_caller]
fn fail(
    name: &str,
    seed: u64,
    case: u32,
    cases: u32,
    sizes: Option<(u32, u32)>,
    failure: &Failure,
) -> ! {
    let sizes = sizes.map_or_else(String::new, |(size, smallest)| {
        format!(" (size {size}; the smallest failing size for this case is {smallest})")
    });
    let input = failure.input.as_deref().unwrap_or("<not generated>");
    panic!(
        "property `{name}` failed at case {case} of {cases}{sizes}\n{}\ninput: {input}\nreplay with {SEED_VAR}={seed}",
        failure.message
    );
}

/// Whether `a` and `b` are equal within `tolerance`, relative to their
/// size but never finer than absolute `tolerance` near zero:
/// `|a - b| <= tolerance * max(1, |a|, |b|)`. Equal values (equal
/// infinities included) always are; NaN never is; an infinity is never
/// near a finite value.
#[must_use]
pub fn approx_eq(a: f64, b: f64, tolerance: f64) -> bool {
    if a == b {
        return true;
    }
    if !a.is_finite() || !b.is_finite() {
        return false;
    }
    let scale = 1f64.max(a.abs()).max(b.abs());
    let difference = (a - b).abs();
    if difference.is_finite() {
        difference <= tolerance * scale
    } else {
        // The difference overflowed, so the values are near `f64::MAX`;
        // halving both sides is exact there.
        (a * 0.5 - b * 0.5).abs() <= tolerance * 0.5 * scale
    }
}

/// Asserts that two floats are [`approx_eq`]: `assert_approx!(a, b)` with
/// [`DEFAULT_TOLERANCE`], or `assert_approx!(a, b, tolerance)`. Each
/// argument is anything that converts into `f64` (`f32`, `f64`, `i32`,
/// `u32`, …), is evaluated once, and is moved.
#[doc(hidden)]
#[macro_export]
macro_rules! __assert_approx {
    ($left:expr, $right:expr $(,)?) => {
        $crate::testing::assert_approx!($left, $right, $crate::testing::DEFAULT_TOLERANCE)
    };
    ($left:expr, $right:expr, $tolerance:expr $(,)?) => {
                match ($left, $right, $tolerance) {
            (left, right, tolerance) => {
                let left: f64 = ::core::convert::Into::into(left);
                let right: f64 = ::core::convert::Into::into(right);
                let tolerance: f64 = ::core::convert::Into::into(tolerance);
                if !$crate::testing::approx_eq(left, right, tolerance) {
                    ::core::panic!(
                        "assertion `left ≈ right` failed (tolerance {tolerance:e})\n  left: {left:?}\n right: {right:?}"
                    );
                }
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The panic message of `f`, which must panic, uncut.
    fn panic_message(f: impl FnOnce()) -> String {
        let payload = panic::catch_unwind(AssertUnwindSafe(f)).expect_err("it should panic");
        match payload.downcast::<String>() {
            Ok(text) => *text,
            Err(payload) => payload
                .downcast_ref::<&str>()
                .expect("a text payload")
                .to_string(),
        }
    }

    fn ends_in_3(&v: &u32) -> Result<(), String> {
        if v % 10 == 3 {
            Err(format!("{v} ends in 3"))
        } else {
            Ok(())
        }
    }

    #[test]
    fn case_generators_match_the_reference() {
        // From an independent Python implementation of FNV-1a, SplitMix64
        // and PCG32.
        let pinned = [
            (0, "", 0, [0x484A_BD25, 0x8862_1BCC]),
            (0x5EED, "p", 0, [0xE065_DF6E, 0x4A03_5EE7]),
            (0x5EED, "p", 1, [0x92F7_AE1E, 0xB743_0238]),
            (0x5EED, "p", 2, [0x486C_C3A8, 0xC79E_8A70]),
            (12345, "keycode order", 99, [0x836B_6601, 0xB193_1AD5]),
            (u64::MAX, "x", u32::MAX, [0x46BA_1647, 0x104F_6600]),
        ];
        for (seed, name, case, expected) in pinned {
            let mut rng = case_rng(seed, name, case);
            assert_eq!(
                [rng.next_u32(), rng.next_u32()],
                expected,
                "{seed} {name:?} {case}"
            );
        }
    }

    #[test]
    fn a_passing_property_runs_every_case() {
        let count = std::cell::Cell::new(0);
        run_check(1, "counts", 250, &|rng| rng.next_u32(), &|_| {
            count.set(count.get() + 1);
            Ok(())
        });
        assert_eq!(count.get(), 250);
        run_check(
            1,
            "no cases",
            0,
            &|_| -> u32 { panic!("never generated") },
            &|_| Ok(()),
        );
    }

    #[test]
    fn a_failure_reports_the_seed_and_replays_the_same_case() {
        // Case 30 is the first whose value ends in 3, per the Python
        // reference (value 3428910613).
        let run = || {
            panic_message(|| {
                run_check(
                    12345,
                    "fails on purpose",
                    1000,
                    &|rng| rng.next_u32(),
                    &ends_in_3,
                )
            })
        };
        let report = run();
        assert_eq!(
            report,
            "property `fails on purpose` failed at case 30 of 1000\n3428910613 ends in 3\ninput: 3428910613\nreplay with SOFTSHELL_SEED=12345"
        );
        assert_eq!(run(), report);
        let other = panic_message(|| {
            run_check(
                42,
                "fails on purpose",
                1000,
                &|rng| rng.next_u32(),
                &ends_in_3,
            )
        });
        assert!(
            other.starts_with("property `fails on purpose` failed at case 16 of 1000\n"),
            "{other}"
        );
        assert!(other.ends_with("replay with SOFTSHELL_SEED=42"));
    }

    #[test]
    fn check_sized_reports_the_smallest_failing_size() {
        let report = panic_message(|| {
            run_check_sized(
                7,
                "short vectors",
                1000,
                &|rng, size| {
                    (0..size)
                        .map(|_| rng.range(0..256) as u8)
                        .collect::<Vec<u8>>()
                },
                &|v| {
                    if v.len() >= 7 {
                        Err(format!("length {}", v.len()))
                    } else {
                        Ok(())
                    }
                },
            );
        });
        // Sizes grow by one about every ten cases: case 70 is the first of
        // size 7, and size 7 is its smallest failure.
        assert!(
            report.starts_with(
                "property `short vectors` failed at case 70 of 1000 (size 7; the smallest failing size for this case is 7)\nlength 7\ninput: ["
            ),
            "{report}"
        );
        // A property that fails only above a size shrinks below the
        // failing case's size.
        let report = panic_message(|| {
            run_check_sized(
                7,
                "sum",
                11,
                &|rng, size| (0..size).map(|_| rng.range(0..10)).collect::<Vec<u64>>(),
                &|v| {
                    if v.len() > 25 {
                        Err("too long".to_owned())
                    } else {
                        Ok(())
                    }
                },
            );
        });
        assert!(
            report.starts_with(
                "property `sum` failed at case 3 of 11 (size 30; the smallest failing size for this case is 26)\n"
            ),
            "{report}"
        );
    }

    #[test]
    fn panics_are_failures() {
        let report = panic_message(|| {
            run_check(
                1,
                "generator",
                10,
                &|_| -> u32 { panic!("generator broke") },
                &|_| Ok(()),
            );
        });
        assert_eq!(
            report,
            "property `generator` failed at case 0 of 10\nthe generator panicked: generator broke\ninput: <not generated>\nreplay with SOFTSHELL_SEED=1"
        );
        let report = panic_message(|| {
            run_check(1, "property", 10, &|rng| rng.next_u32() % 4, &|&v| {
                assert!(v != 2, "two is not allowed");
                Ok(())
            });
        });
        assert!(
            report.contains("\nthe property panicked: two is not allowed\ninput: 2\n"),
            "{report}"
        );
        // A panicking Debug impl does not lose the report.
        struct BadDebug;
        impl Debug for BadDebug {
            fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                panic!("no Debug")
            }
        }
        let report =
            panic_message(|| run_check(1, "debug", 1, &|_| BadDebug, &|_| Err("no".to_owned())));
        assert!(
            report.contains("\ninput: <its Debug impl panicked>\n"),
            "{report}"
        );
    }

    #[test]
    fn reports_are_capped() {
        let report = panic_message(|| {
            run_check(1, "huge", 1, &|_| "é".repeat(10_000), &|_| {
                Err("x".repeat(10_000))
            });
        });
        let lines: Vec<&str> = report.lines().collect();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[1].len(), MAX_REPORT_BYTES + "…[truncated]".len());
        assert!(lines[2].ends_with("…[truncated]"));
        assert!(lines[2].len() <= "input: ".len() + MAX_REPORT_BYTES + "…[truncated]".len());
        // Panic text too, from a property and from a generator.
        let cut = MAX_REPORT_BYTES + "…[truncated]".len();
        let report = panic_message(|| {
            run_check(1, "huge panic", 1, &|_| 0u8, &|_| {
                panic!("{}", "x".repeat(10_000))
            });
        });
        assert_eq!(
            report.lines().nth(1).map(str::len),
            Some("the property panicked: ".len() + cut)
        );
        let report = panic_message(|| {
            run_check(
                1,
                "huge panic",
                1,
                &|_| -> u8 { panic!("{}", "x".repeat(10_000)) },
                &|_| Ok(()),
            );
        });
        assert_eq!(
            report.lines().nth(1).map(str::len),
            Some("the generator panicked: ".len() + cut)
        );
    }

    #[test]
    fn panic_payloads_of_every_kind_are_reported() {
        let message = |property: &dyn Fn(&u32) -> Result<(), String>| {
            let report = panic_message(|| run_check(1, "payload", 1, &|_| 1u32, property));
            report
                .lines()
                .skip(1)
                .take_while(|l| !l.starts_with("input: "))
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert_eq!(
            message(&|v| panic!("bad {v}")),
            "the property panicked: bad 1"
        );
        assert_eq!(
            message(&|&v| {
                assert_eq!(v, 0);
                Ok(())
            }),
            "the property panicked: assertion `left == right` failed\n  left: 1\n right: 0"
        );
        assert_eq!(
            message(&|_| std::panic::panic_any(7u8)),
            "the property panicked: <a panic payload that is not text>"
        );
    }

    #[test]
    fn shrinking_regenerates_the_same_case_at_smaller_sizes() {
        let generate = |rng: &mut Pcg32, size: u32| -> Vec<u8> {
            (0..size).map(|_| (rng.next_u32() >> 24) as u8).collect()
        };
        let property = |v: &Vec<u8>| {
            if v.contains(&255) {
                Err("contains 255".to_owned())
            } else {
                Ok(())
            }
        };
        let report = panic_message(|| {
            run_check_sized(12345, "bytes on purpose", 1000, &generate, &property)
        });
        // Per the Python reference, case 126 (size 12) fails first, and its
        // generator gives 228, 138, 255 first, so size 3 is its smallest
        // failure.
        assert_eq!(
            report,
            "property `bytes on purpose` failed at case 126 of 1000 (size 12; the smallest failing size for this case is 3)\ncontains 255\ninput: [228, 138, 255]\nreplay with SOFTSHELL_SEED=12345"
        );
        // A failure that does not depend on the size shrinks to 0.
        let report = panic_message(|| {
            run_check_sized(
                12345,
                "fails on purpose",
                1000,
                &|rng, size| (rng.next_u32(), size),
                &|&(v, _)| {
                    if v % 10 == 3 {
                        Err(format!("{v} ends in 3"))
                    } else {
                        Ok(())
                    }
                },
            );
        });
        assert_eq!(
            report,
            "property `fails on purpose` failed at case 30 of 1000 (size 3; the smallest failing size for this case is 0)\n3428910613 ends in 3\ninput: (3428910613, 0)\nreplay with SOFTSHELL_SEED=12345"
        );
    }

    #[test]
    fn sizes_grow_from_zero_to_the_maximum() {
        assert_eq!(size_for(0, 0), 0);
        assert_eq!(size_for(0, 1), 0);
        assert_eq!((size_for(0, 2), size_for(1, 2)), (0, MAX_SIZE));
        assert_eq!(
            (size_for(0, 101), size_for(50, 101), size_for(100, 101)),
            (0, 50, 100)
        );
        assert_eq!(size_for(u32::MAX - 1, u32::MAX), MAX_SIZE);
        // (2^31 - 1) * 100 / (2^32 - 2) is exactly 50.
        assert_eq!(size_for(u32::MAX / 2, u32::MAX), 50);
        assert_eq!(size_for(u32::MAX / 2 - 1, u32::MAX), 49);
    }

    #[test]
    fn seeds_parse_in_decimal_or_hex() {
        assert_eq!(parse_seed(OsStr::new("0")), Ok(0));
        assert_eq!(parse_seed(OsStr::new("12345")), Ok(12345));
        assert_eq!(parse_seed(OsStr::new("0x2A")), Ok(42));
        assert_eq!(parse_seed(OsStr::new("0X2a")), Ok(42));
        assert_eq!(parse_seed(OsStr::new("18446744073709551615")), Ok(u64::MAX));
        for bad in [
            "",
            "-1",
            "0x",
            "seed",
            "1 ",
            "18446744073709551616",
            "0x1_0",
        ] {
            assert!(parse_seed(OsStr::new(bad)).is_err(), "{bad:?}");
        }
        assert_eq!(
            parse_seed(OsStr::new("abc")),
            Err(
                "`abc` is not a u64 in decimal or 0x hex: invalid digit found in string".to_owned()
            )
        );
    }

    #[test]
    fn approx_eq_is_relative_and_strict_about_infinities() {
        // The scale is the larger magnitude, whichever argument it is.
        assert!(approx_eq(1.0, 1.9, 0.5) && approx_eq(1.9, 1.0, 0.5));
        assert!(approx_eq(1e12 + 100.0, 1e12, 1e-10) && approx_eq(1e12, 1e12 + 100.0, 1e-10));
        assert!(!approx_eq(1.0, 2.0, 0.49) && !approx_eq(2.0, 1.0, 0.49));
        // A difference that overflows is still compared exactly.
        assert!(!approx_eq(f64::MAX, -f64::MAX, 1.5));
        assert!(approx_eq(f64::MAX, -f64::MAX, 2.0));
        assert!(approx_eq(f64::MAX, -f64::MAX, 3.0));
        assert!(!approx_eq(1e308, -1e308, 1.9));
        assert!(approx_eq(1.0, 1.0 + 1e-10, 1e-9));
        assert!(!approx_eq(1.0, 1.0 + 1e-8, 1e-9));
        assert!(approx_eq(1e12, 1e12 + 100.0, 1e-9));
        assert!(!approx_eq(1e12, 1e12 + 10_000.0, 1e-9));
        assert!(approx_eq(0.0, 1e-10, 1e-9));
        assert!(approx_eq(0.0, -0.0, 0.0));
        assert!(approx_eq(f64::INFINITY, f64::INFINITY, 1e-9));
        assert!(!approx_eq(f64::INFINITY, 0.0, 1e-9));
        assert!(!approx_eq(f64::INFINITY, f64::NEG_INFINITY, 1e-9));
        assert!(!approx_eq(f64::INFINITY, f64::MAX, 1e-9));
        assert!(!approx_eq(f64::MAX, -f64::MAX, 1e-9));
        assert!(!approx_eq(f64::NAN, f64::NAN, 1.0));
        assert!(!approx_eq(f64::NAN, 0.0, f64::INFINITY));
    }

    #[test]
    fn assert_approx_macro() {
        assert_approx!(0.1 + 0.2, 0.3);
        assert_approx!(1.0f32, 1.000_000_1f32, 1e-6);
        assert_approx!(3, 3.0);
        let mut calls = 0;
        let mut next = || {
            calls += 1;
            2.0
        };
        assert_approx!(next(), 2.0, 0.0);
        assert_eq!(calls, 1, "each argument is evaluated once");
        // A type that is not `Copy` but converts into `f64`.
        struct Meters(f64);
        impl From<Meters> for f64 {
            fn from(m: Meters) -> f64 {
                m.0
            }
        }
        assert_approx!(Meters(2.5), 2.5);
        let report = panic_message(|| assert_approx!(1.0, 1.1, 1e-3));
        assert_eq!(
            report,
            "assertion `left ≈ right` failed (tolerance 1e-3)\n  left: 1.0\n right: 1.1"
        );
    }
}
