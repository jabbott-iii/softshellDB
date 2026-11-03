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

//! A failing `check` or `check_sized` prints a seed that replays it, end to
//! end: this test runs its own binary again with `SOFTSHELL_SEED` set,
//! since a test cannot set an environment variable for itself without
//! `unsafe`.

use std::process::Command;

use softshell_core::testing::{DEFAULT_SEED, check, check_sized};

/// Set by the tests below for their child processes; without it the demo
/// tests pass at once.
const DEMO_VAR: &str = "SOFTSHELL_CHECK_REPLAY_DEMO";

#[test]
fn deliberately_failing_check() {
    if std::env::var_os(DEMO_VAR).is_none() {
        return;
    }
    check(
        "fails on purpose",
        1000,
        |rng| rng.next_u32(),
        |&v| {
            if v % 10 == 3 {
                Err(format!("{v} ends in 3"))
            } else {
                Ok(())
            }
        },
    );
}

#[test]
fn deliberately_failing_check_sized() {
    if std::env::var_os(DEMO_VAR).is_none() {
        return;
    }
    check_sized(
        "bytes on purpose",
        1000,
        |rng, size| {
            (0..size)
                .map(|_| (rng.next_u32() >> 24) as u8)
                .collect::<Vec<u8>>()
        },
        |v| {
            if v.contains(&255) {
                Err("contains 255".to_owned())
            } else {
                Ok(())
            }
        },
    );
}

/// Runs the demo test `name` in a child with `SOFTSHELL_SEED` set to
/// `seed` (or unset), and returns its report lines and its "panicked at"
/// line.
fn run_child(name: &str, seed: Option<&str>) -> (Vec<String>, String) {
    let mut command = Command::new(std::env::current_exe().expect("the test binary's path"));
    command
        .args(["--exact", name, "--test-threads=1"])
        .env(DEMO_VAR, "1");
    match seed {
        Some(seed) => command.env("SOFTSHELL_SEED", seed),
        None => command.env_remove("SOFTSHELL_SEED"),
    };
    let output = command.output().expect("the child runs");
    assert!(!output.status.success(), "the child's property should fail");
    let text = String::from_utf8_lossy(&output.stdout) + String::from_utf8_lossy(&output.stderr);
    let report = text
        .lines()
        .filter(|line| {
            ["property ", "input: ", "replay with ", "SOFTSHELL_SEED: "]
                .iter()
                .any(|start| line.starts_with(start))
        })
        .map(str::to_owned)
        .collect();
    let location = text
        .lines()
        .find(|line| line.contains("panicked at"))
        .unwrap_or_default()
        .to_owned();
    (report, location)
}

#[test]
fn a_failing_check_prints_a_seed_that_replays_it() {
    let check = "deliberately_failing_check";
    let (first, location) = run_child(check, Some("12345"));
    // Case 30 for seed 12345, per an independent Python implementation.
    assert_eq!(
        first,
        [
            "property `fails on purpose` failed at case 30 of 1000",
            "input: 3428910613",
            "replay with SOFTSHELL_SEED=12345"
        ]
    );
    // The panic names the test that called `check`, not the harness.
    assert!(location.contains("check_replay.rs"), "{location}");
    assert_eq!(
        run_child(check, Some("12345")).0,
        first,
        "the same seed fails the same way"
    );
    let (hex, _) = run_child(check, Some("0x2A"));
    assert_eq!(
        hex[0],
        "property `fails on purpose` failed at case 16 of 1000"
    );
    assert_eq!(
        hex[2], "replay with SOFTSHELL_SEED=42",
        "printed in decimal"
    );
    // Unset, the default seed is used and printed.
    let (default, _) = run_child(check, None);
    assert_eq!(
        default[2],
        format!("replay with SOFTSHELL_SEED={DEFAULT_SEED}")
    );
    assert_eq!(run_child(check, Some(&DEFAULT_SEED.to_string())).0, default);
}

#[test]
fn a_failing_check_sized_replays_and_shrinks() {
    let sized = "deliberately_failing_check_sized";
    let (report, location) = run_child(sized, Some("12345"));
    // Per the Python reference: case 126 (size 12) fails, smallest size 3.
    assert_eq!(
        report,
        [
            "property `bytes on purpose` failed at case 126 of 1000 (size 12; the smallest failing size for this case is 3)",
            "input: [228, 138, 255]",
            "replay with SOFTSHELL_SEED=12345"
        ]
    );
    assert!(location.contains("check_replay.rs"), "{location}");
    let (report, _) = run_child(sized, Some("42"));
    assert_eq!(
        report,
        [
            "property `bytes on purpose` failed at case 65 of 1000 (size 6; the smallest failing size for this case is 5)",
            "input: [1, 129, 28, 26, 255]",
            "replay with SOFTSHELL_SEED=42"
        ]
    );
}

#[test]
fn a_malformed_seed_fails_instead_of_running_the_default() {
    for (seed, error) in [
        (
            "bogus",
            "SOFTSHELL_SEED: `bogus` is not a u64 in decimal or 0x hex: invalid digit found in string",
        ),
        (
            "",
            "SOFTSHELL_SEED: `` is not a u64 in decimal or 0x hex: cannot parse integer from empty string",
        ),
    ] {
        let (report, _) = run_child("deliberately_failing_check", Some(seed));
        assert_eq!(report, [error], "{seed:?}");
    }
}
