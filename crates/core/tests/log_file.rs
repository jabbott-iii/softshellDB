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

//! `FileSink` rotation, in a real directory under `target/tmp`.

use std::fs;
use std::path::{Path, PathBuf};

use softshell_core::clock::Timestamp;
use softshell_core::log::{FileSink, Level, Record, Sink};

/// A fresh, empty directory for one test. `target/tmp` survives between
/// runs, so whatever an earlier run left is removed first.
fn dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("log_file")
        .join(name);
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("remove the old test directory");
    }
    fs::create_dir_all(&dir).expect("create the test directory");
    dir
}

/// Writes one record per message.
fn write(sink: &FileSink, messages: impl IntoIterator<Item = String>) {
    for message in messages {
        sink.write(&Record {
            level: Level::Info,
            target: "t",
            timestamp: Timestamp::UNIX_EPOCH,
            message: &message,
        });
    }
}

/// Messages `m01`, `m02`, …, whose lines are all `LINE` bytes long.
fn numbered(range: std::ops::RangeInclusive<u32>) -> impl Iterator<Item = String> {
    range.map(|i| format!("m{i:02}"))
}

const LINE: u64 = "1970-01-01T00:00:00.000Z INFO  t: m01\n".len() as u64;

/// The messages in a file, or `None` if it does not exist.
fn messages(path: &Path) -> Option<Vec<String>> {
    let text = fs::read_to_string(path).ok()?;
    Some(
        text.lines()
            .map(|line| line.rsplit_once(": ").expect("a log line").1.to_owned())
            .collect(),
    )
}

fn rotated(path: &Path, n: u32) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{n}"));
    PathBuf::from(name)
}

#[test]
fn rotates_and_keeps_the_newest_files() {
    let path = dir("keep3").join("app.log");
    let sink = FileSink::open(&path, 2 * LINE, 3).unwrap();
    write(&sink, numbered(1..=9));
    assert_eq!(messages(&path).unwrap(), ["m09"]);
    assert_eq!(messages(&rotated(&path, 1)).unwrap(), ["m07", "m08"]);
    assert_eq!(messages(&rotated(&path, 2)).unwrap(), ["m05", "m06"]);
    assert_eq!(messages(&rotated(&path, 3)).unwrap(), ["m03", "m04"]);
    assert_eq!(messages(&rotated(&path, 4)), None);
    assert_eq!((sink.failed_writes(), sink.failed_rotations()), (0, 0));
    // A reopened sink appends, and knows the file's size.
    drop(sink);
    let sink = FileSink::open(&path, 2 * LINE, 3).unwrap();
    write(&sink, numbered(10..=11));
    assert_eq!(messages(&path).unwrap(), ["m11"]);
    assert_eq!(messages(&rotated(&path, 1)).unwrap(), ["m09", "m10"]);
    assert_eq!(messages(&rotated(&path, 3)).unwrap(), ["m05", "m06"]);
}

#[test]
fn keep_one_and_keep_zero() {
    let path = dir("keep1").join("app.log");
    let sink = FileSink::open(&path, 2 * LINE, 1).unwrap();
    write(&sink, numbered(1..=5));
    assert_eq!(messages(&path).unwrap(), ["m05"]);
    assert_eq!(messages(&rotated(&path, 1)).unwrap(), ["m03", "m04"]);
    assert_eq!(messages(&rotated(&path, 2)), None);

    let path = dir("keep0").join("app.log");
    let sink = FileSink::open(&path, 2 * LINE, 0).unwrap();
    write(&sink, numbered(1..=5));
    assert_eq!(messages(&path).unwrap(), ["m05"]);
    assert_eq!(messages(&rotated(&path, 1)), None);
}

#[test]
fn a_gap_is_filled_without_losing_older_files() {
    let path = dir("gap").join("app.log");
    fs::write(rotated(&path, 1), "x: old1\n").unwrap();
    fs::write(rotated(&path, 3), "x: old3\n").unwrap();
    let sink = FileSink::open(&path, 2 * LINE, 3).unwrap();
    write(&sink, numbered(1..=3));
    assert_eq!(messages(&path).unwrap(), ["m03"]);
    assert_eq!(messages(&rotated(&path, 1)).unwrap(), ["m01", "m02"]);
    assert_eq!(messages(&rotated(&path, 2)).unwrap(), ["old1"]);
    assert_eq!(messages(&rotated(&path, 3)).unwrap(), ["old3"]);
}

#[test]
fn a_record_longer_than_the_limit_goes_whole_into_a_fresh_file() {
    let path = dir("long").join("app.log");
    let sink = FileSink::open(&path, 2 * LINE, 2).unwrap();
    write(
        &sink,
        ["short".to_owned(), "x".repeat(200), "after".to_owned()],
    );
    assert_eq!(messages(&path).unwrap(), ["after"]);
    assert_eq!(messages(&rotated(&path, 1)).unwrap(), ["x".repeat(200)]);
    assert_eq!(messages(&rotated(&path, 2)).unwrap(), ["short"]);
}

#[test]
fn an_empty_file_is_never_rotated() {
    // A first record longer than the limit goes into the empty file.
    let path = dir("empty").join("app.log");
    fs::write(rotated(&path, 1), "x: old\n").unwrap();
    let sink = FileSink::open(&path, 2 * LINE, 2).unwrap();
    write(&sink, ["y".repeat(200)]);
    assert_eq!(messages(&path).unwrap(), ["y".repeat(200)]);
    assert_eq!(messages(&rotated(&path, 1)).unwrap(), ["old"]);
    assert_eq!(messages(&rotated(&path, 2)), None);
    // With a limit of 0, each record starts a new file.
    let path = dir("zero").join("app.log");
    let sink = FileSink::open(&path, 0, 3).unwrap();
    write(&sink, numbered(1..=3));
    assert_eq!(messages(&path).unwrap(), ["m03"]);
    assert_eq!(messages(&rotated(&path, 1)).unwrap(), ["m02"]);
    assert_eq!(messages(&rotated(&path, 2)).unwrap(), ["m01"]);
    assert_eq!(messages(&rotated(&path, 3)), None);
}

#[test]
fn a_failed_rotation_still_writes_and_retries_later() {
    let path = dir("blocked").join("app.log");
    // A non-empty directory where `app.log.1` would go: renaming a file
    // onto it fails on every supported OS.
    let blocker = rotated(&path, 1);
    fs::create_dir(&blocker).unwrap();
    fs::write(blocker.join("keep"), "").unwrap();
    let sink = FileSink::open(&path, 2 * LINE, 1).unwrap();
    write(&sink, numbered(1..=3));
    // The third record should have rotated: it failed, and was written.
    assert_eq!(sink.failed_rotations(), 1);
    assert_eq!(sink.failed_writes(), 0);
    assert_eq!(messages(&path).unwrap(), ["m01", "m02", "m03"]);
    // No new attempt until the file grows by another `max_bytes`.
    write(&sink, numbered(4..=4));
    assert_eq!(sink.failed_rotations(), 1);
    write(&sink, numbered(5..=5));
    assert_eq!(sink.failed_rotations(), 2);
    // Once the way is clear, the next attempt (after another `max_bytes`)
    // succeeds.
    fs::remove_dir_all(&blocker).unwrap();
    write(&sink, numbered(6..=7));
    assert_eq!(sink.failed_rotations(), 2);
    assert_eq!(
        messages(&rotated(&path, 1)).unwrap(),
        ["m01", "m02", "m03", "m04", "m05", "m06"]
    );
    assert_eq!(messages(&path).unwrap(), ["m07"]);
    // And the next one comes after `max_bytes` again, not the old retry
    // point.
    write(&sink, numbered(8..=9));
    assert_eq!(messages(&rotated(&path, 1)).unwrap(), ["m07", "m08"]);
    assert_eq!(messages(&path).unwrap(), ["m09"]);
    assert_eq!(sink.failed_rotations(), 2);
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .lines()
            .all(|l| l.len() as u64 == LINE - 1)
    );
}
