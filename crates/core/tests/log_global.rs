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

//! The process-wide logger and the macros, used from another crate. One
//! test only: the logger is global, and tests in one binary run in
//! parallel.

// A crate with its own `error` module still imports the `error!` macro.
#[allow(dead_code, reason = "only its name matters")]
mod error {
    pub(crate) struct OwnError;
}

use std::sync::Arc;

use softshell_core::clock::{ManualClock, Timestamp};
use softshell_core::log::{
    self, Filter, Level, Logger, MemorySink, Record, Sink, StderrSink, clear_logger, debug, error,
    info, set_logger, trace, try_set_logger, warn,
};

#[warn(unused)]
fn log_everything(n: u32) {
    error!("error {n}");
    warn!("warn {n}");
    info!("info {n}");
    debug!("debug {n}");
    trace!("trace {n}");
}

mod storage {
    pub(crate) fn flush() {
        softshell_core::log::debug!("flushed");
    }
}

#[test]
fn the_installed_logger_receives_what_its_filter_keeps() {
    let _ = error::OwnError;
    let memory = Arc::new(MemorySink::new(100));
    let clock = Arc::new(ManualClock::new(Timestamp::from_nanos(7)));
    let filter = Filter::new(Some(Level::Info)).target("log_global::storage", Some(Level::Debug));
    set_logger(Logger::new(filter, Arc::clone(&clock)).sink(Arc::clone(&memory)));
    log_everything(1);
    storage::flush();
    let seen: Vec<(Level, String, String)> = memory
        .take()
        .into_iter()
        .map(|e| (e.level, e.target, e.message))
        .collect();
    let expected = [
        (Level::Error, "log_global", "error 1"),
        (Level::Warn, "log_global", "warn 1"),
        (Level::Info, "log_global", "info 1"),
        (Level::Debug, "log_global::storage", "flushed"),
    ];
    assert_eq!(seen.len(), expected.len());
    for ((level, target, message), (l, t, m)) in seen.iter().zip(expected) {
        assert_eq!((*level, target.as_str(), message.as_str()), (l, t, m));
    }
    assert!(log::enabled(Level::Info, "anything"));
    assert!(!log::enabled(Level::Debug, "log_global"));

    // A second logger is refused while one is installed.
    let refused = try_set_logger(Logger::new(
        Filter::new(None),
        ManualClock::new(Timestamp::MIN),
    ));
    assert!(refused.is_err());
    info!("still the first");
    assert_eq!(memory.take().len(), 1);

    // Captures see every level; the logger still gets what it keeps.
    let ((), captured) = log::capture(|| log_everything(2));
    assert_eq!(captured.len(), 5);
    assert_eq!(
        captured[0].timestamp,
        Timestamp::from_nanos(7),
        "the logger's clock"
    );
    assert_eq!(memory.take().len(), 3);

    // Cleared: nothing is enabled or kept.
    clear_logger();
    assert!(!log::enabled(Level::Error, "log_global"));
    log_everything(3);
    assert!(memory.take().is_empty());
    let replacement = Arc::new(MemorySink::new(10));
    assert!(
        try_set_logger(
            Logger::new(
                Filter::new(Some(Level::Error)),
                ManualClock::new(Timestamp::MAX)
            )
            .sink(Arc::clone(&replacement))
        )
        .is_ok()
    );
    log_everything(4);
    assert_eq!(replacement.entries().len(), 1);
    assert_eq!(replacement.entries()[0].timestamp, Timestamp::MAX);
    clear_logger();

    // The fast path is closed again once no logger or capture needs it.
    assert_eq!(log::__max_level(), 0);
    let ((), _) = log::capture(|| assert_eq!(log::__max_level(), Level::Trace as u8));
    assert_eq!(log::__max_level(), 0);
    set_logger(Logger::new(
        Filter::new(Some(Level::Info)),
        ManualClock::new(Timestamp::MIN),
    ));
    assert_eq!(log::__max_level(), Level::Info as u8);
    let ((), _) = log::capture(|| {});
    assert_eq!(log::__max_level(), Level::Info as u8);

    // A replaced logger is dropped outside the lock, so a sink that logs
    // as it is dropped reaches the new logger instead of deadlocking.
    struct LogsOnDrop;
    impl Sink for LogsOnDrop {
        fn write(&self, _: &Record<'_>) {}
    }
    impl Drop for LogsOnDrop {
        fn drop(&mut self) {
            warn!("old sink dropped");
        }
    }
    set_logger(
        Logger::new(
            Filter::new(Some(Level::Info)),
            ManualClock::new(Timestamp::MIN),
        )
        .sink(LogsOnDrop),
    );
    let after = Arc::new(MemorySink::new(10));
    set_logger(
        Logger::new(
            Filter::new(Some(Level::Info)),
            ManualClock::new(Timestamp::MIN),
        )
        .sink(Arc::clone(&after)),
    );
    assert_eq!(after.take()[0].message, "old sink dropped");
    set_logger(Logger::new(Filter::new(None), ManualClock::new(Timestamp::MIN)).sink(LogsOnDrop));
    clear_logger();
    assert_eq!(log::__max_level(), 0);

    // The stderr sink writes one line (shown in the test's output).
    StderrSink.write(&Record {
        level: Level::Info,
        target: "log_global",
        timestamp: Timestamp::UNIX_EPOCH,
        message: "StderrSink wrote this line",
    });
}
