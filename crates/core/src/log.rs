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

//! Logging: the macros [`error!`], [`warn!`], [`info!`], [`debug!`] and
//! [`trace!`], and the [`Logger`] they write to.
//!
//! A process has at most one logger, installed by the application (the
//! server, the CLI, a test) with [`set_logger`] or [`try_set_logger`]. A
//! library never installs one, so by default nothing is written. A logger
//! has a [`Filter`] (a level per module-path prefix), a [`Clock`] for
//! timestamps, and sinks: [`StderrSink`], a rotating [`FileSink`] and an
//! in-memory [`MemorySink`]. Tests can also [`capture`] what their own
//! thread logs.
//!
//! ```
//! use std::sync::Arc;
//! use softshell_core::clock::{ManualClock, Timestamp};
//! use softshell_core::log::{self, Filter, Level, Logger, MemorySink};
//!
//! let memory = Arc::new(MemorySink::new(100));
//! let logger = Logger::new(Filter::new(Some(Level::Info)), ManualClock::new(Timestamp::UNIX_EPOCH))
//!     .sink(Arc::clone(&memory));
//! logger.log(Level::Info, "softshell_storage::flush", format_args!("flushed {} bytes", 4096));
//! logger.log(Level::Debug, "softshell_storage::flush", format_args!("not kept"));
//! assert_eq!(memory.entries()[0].message, "flushed 4096 bytes");
//! assert_eq!(memory.entries().len(), 1);
//! ```
//!
//! With a logger installed, `log::info!("opened {}", name)` logs under the
//! calling module's path. Arguments are not evaluated when the level is
//! off; with no logger installed a call costs one atomic load. Never log
//! document contents, keys of user data at `info` or above, or tokens.
//!
//! A record's message is rendered once, outside every lock, and cut at
//! [`MAX_MESSAGE_BYTES`]. A message's `Display` code may itself log. A
//! sink must not: it would wait on its own lock.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::error::Error;
use std::fmt::{self, Write as _};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};

use crate::civil;
use crate::clock::{Clock, SystemClock, Timestamp};

#[doc(inline)]
pub use crate::{
    __log_debug as debug, __log_error as error, __log_info as info, __log_trace as trace,
    __log_warn as warn,
};

/// The most bytes of a message a record keeps; the rest is cut at a
/// character boundary and marked `…[truncated]`.
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024;

/// The most entries [`capture`] keeps; older ones are dropped.
const MAX_CAPTURED: usize = 10_000;

/// Logs at [`Level::Error`]: a failure the operator must see.
#[doc(hidden)]
#[macro_export]
macro_rules! __log_error {
    ($($arg:tt)+) => {
        if $crate::log::enabled($crate::log::Level::Error, ::core::module_path!()) {
            $crate::log::__log(
                $crate::log::Level::Error,
                ::core::module_path!(),
                ::core::format_args!($($arg)+),
            );
        }
    };
}

/// Logs at [`Level::Warn`]: something unexpected that was handled.
#[doc(hidden)]
#[macro_export]
macro_rules! __log_warn {
    ($($arg:tt)+) => {
        if $crate::log::enabled($crate::log::Level::Warn, ::core::module_path!()) {
            $crate::log::__log(
                $crate::log::Level::Warn,
                ::core::module_path!(),
                ::core::format_args!($($arg)+),
            );
        }
    };
}

/// Logs at [`Level::Info`]: a notable event, such as opening a database.
#[doc(hidden)]
#[macro_export]
macro_rules! __log_info {
    ($($arg:tt)+) => {
        if $crate::log::enabled($crate::log::Level::Info, ::core::module_path!()) {
            $crate::log::__log(
                $crate::log::Level::Info,
                ::core::module_path!(),
                ::core::format_args!($($arg)+),
            );
        }
    };
}

/// Logs at [`Level::Debug`]: detail for diagnosing a problem.
#[doc(hidden)]
#[macro_export]
macro_rules! __log_debug {
    ($($arg:tt)+) => {
        if $crate::log::enabled($crate::log::Level::Debug, ::core::module_path!()) {
            $crate::log::__log(
                $crate::log::Level::Debug,
                ::core::module_path!(),
                ::core::format_args!($($arg)+),
            );
        }
    };
}

/// Logs at [`Level::Trace`]: step-by-step detail.
#[doc(hidden)]
#[macro_export]
macro_rules! __log_trace {
    ($($arg:tt)+) => {
        if $crate::log::enabled($crate::log::Level::Trace, ::core::module_path!()) {
            $crate::log::__log(
                $crate::log::Level::Trace,
                ::core::module_path!(),
                ::core::format_args!($($arg)+),
            );
        }
    };
}

/// How important a record is. `Error` is the most important; a filter set
/// to a level keeps that level and every more important one.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(u8)]
pub enum Level {
    /// A failure.
    Error = 1,
    /// Something unexpected that was handled.
    Warn = 2,
    /// A notable event.
    Info = 3,
    /// Detail for diagnosis.
    Debug = 4,
    /// Step-by-step detail.
    Trace = 5,
}

impl Level {
    /// The name in upper case, as log lines show it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "ERROR",
            Self::Warn => "WARN",
            Self::Info => "INFO",
            Self::Debug => "DEBUG",
            Self::Trace => "TRACE",
        }
    }
}

/// The upper-case name, padded to a width if one is given.
impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.as_str())
    }
}

/// Reads `error`, `warn`, `info`, `debug` or `trace`, in any case.
impl FromStr for Level {
    type Err = ParseLevelError;

    fn from_str(text: &str) -> Result<Self, ParseLevelError> {
        [
            Self::Error,
            Self::Warn,
            Self::Info,
            Self::Debug,
            Self::Trace,
        ]
        .into_iter()
        .find(|level| level.as_str().eq_ignore_ascii_case(text))
        .ok_or(ParseLevelError)
    }
}

/// Text that is not a [`Level`] name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseLevelError;

impl fmt::Display for ParseLevelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("expected one of error, warn, info, debug, trace")
    }
}

/// A leaf error: it has no source.
impl Error for ParseLevelError {}

/// Which records a logger keeps: a level for each module-path prefix, and
/// a default. `None` keeps nothing.
///
/// A prefix matches a target that equals it or continues with `::`:
/// `softshell_storage` matches `softshell_storage::flush`, not
/// `softshell_storage2`. The longest matching prefix decides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Filter {
    default: Option<Level>,
    targets: Vec<(String, Option<Level>)>,
}

impl Filter {
    /// A filter keeping `default` and more important levels everywhere.
    #[must_use]
    pub const fn new(default: Option<Level>) -> Self {
        Self {
            default,
            targets: Vec::new(),
        }
    }

    /// Sets the level for `prefix` and the modules under it, replacing any
    /// level set for it before.
    #[must_use]
    pub fn target(mut self, prefix: &str, level: Option<Level>) -> Self {
        match self.targets.iter_mut().find(|(p, _)| p == prefix) {
            Some(entry) => entry.1 = level,
            None => self.targets.push((prefix.to_owned(), level)),
        }
        self
    }

    /// Whether a record at `level` from `target` is kept.
    #[must_use]
    pub fn enabled(&self, level: Level, target: &str) -> bool {
        let level_for_target = self
            .targets
            .iter()
            .filter(|(prefix, _)| {
                target
                    .strip_prefix(prefix.as_str())
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with("::"))
            })
            .max_by_key(|(prefix, _)| prefix.len())
            .map_or(self.default, |(_, level)| *level);
        level_for_target.is_some_and(|max| level <= max)
    }

    /// The least important level kept anywhere, or `None` if nothing is.
    #[must_use]
    pub fn max_level(&self) -> Option<Level> {
        self.targets
            .iter()
            .map(|(_, level)| *level)
            .fold(self.default, Ord::max)
    }
}

/// One log record, as sinks receive it.
#[derive(Clone, Copy, Debug)]
pub struct Record<'a> {
    /// Its level.
    pub level: Level,
    /// The module path it was logged from.
    pub target: &'a str,
    /// When it was logged, by the logger's clock.
    pub timestamp: Timestamp,
    /// The rendered message, at most [`MAX_MESSAGE_BYTES`] plus the
    /// truncation mark. Not escaped: [`format_line`] does that.
    pub message: &'a str,
}

/// Where records go. A sink must not log: it would wait on its own lock.
pub trait Sink: Send + Sync {
    /// Takes one record. Errors are the sink's to count or ignore: logging
    /// never fails its caller.
    fn write(&self, record: &Record<'_>);
}

impl<S: Sink + ?Sized> Sink for Arc<S> {
    fn write(&self, record: &Record<'_>) {
        (**self).write(record);
    }
}

impl<S: Sink + ?Sized> Sink for Box<S> {
    fn write(&self, record: &Record<'_>) {
        (**self).write(record);
    }
}

/// A filter, a clock and sinks.
pub struct Logger {
    filter: Filter,
    clock: Box<dyn Clock>,
    sinks: Vec<Box<dyn Sink>>,
}

impl Logger {
    /// A logger with no sinks yet, timestamping records with `clock`.
    #[must_use]
    pub fn new(filter: Filter, clock: impl Clock + 'static) -> Self {
        Self {
            filter,
            clock: Box::new(clock),
            sinks: Vec::new(),
        }
    }

    /// Adds a sink. Pass an `Arc` to keep a handle to it, to read a
    /// [`MemorySink`] or a [`FileSink`]'s error counts.
    #[must_use]
    pub fn sink(mut self, sink: impl Sink + 'static) -> Self {
        self.sinks.push(Box::new(sink));
        self
    }

    /// The filter.
    #[must_use]
    pub const fn filter(&self) -> &Filter {
        &self.filter
    }

    /// Logs one record directly, if the filter keeps it. The macros go
    /// through the installed logger instead.
    pub fn log(&self, level: Level, target: &str, args: fmt::Arguments<'_>) {
        if self.filter.enabled(level, target) {
            let message = render(args);
            self.write(&Record {
                level,
                target,
                timestamp: self.clock.now(),
                message: &message,
            });
        }
    }

    fn write(&self, record: &Record<'_>) {
        for sink in &self.sinks {
            sink.write(record);
        }
    }
}

impl fmt::Debug for Logger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Logger")
            .field("filter", &self.filter)
            .field("sinks", &self.sinks.len())
            .finish_non_exhaustive()
    }
}

/// The installed logger.
static LOGGER: RwLock<Option<Arc<Logger>>> = RwLock::new(None);
/// The least important level any record could be kept at, as a number (0:
/// none): the installed filter's, or `Trace` while any thread captures.
static MAX_LEVEL: AtomicU8 = AtomicU8::new(0);
/// How many threads are capturing.
static CAPTURES: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    /// This thread's capture, if it is in [`capture`].
    static CAPTURE: RefCell<Option<VecDeque<Entry>>> = const { RefCell::new(None) };
}

fn read_logger() -> Option<Arc<Logger>> {
    LOGGER
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// Recomputes [`MAX_LEVEL`]. Called with the write lock held, so the last
/// writer sees every capture count and logger change before it.
fn refresh_max_level(logger: &Option<Arc<Logger>>) {
    let installed = logger
        .as_ref()
        .and_then(|l| l.filter.max_level())
        .map_or(0, |level| level as u8);
    let max = if CAPTURES.load(Ordering::SeqCst) > 0 {
        Level::Trace as u8
    } else {
        installed
    };
    MAX_LEVEL.store(max, Ordering::SeqCst);
}

/// Installs `logger` for the whole process, replacing any other. For
/// applications and tests; a library calls [`try_set_logger`], if anything.
pub fn set_logger(logger: Logger) {
    let old = replace_logger(Some(Arc::new(logger)));
    // Dropped after the lock is released: its sinks' and clock's `Drop`
    // code may log.
    drop(old);
}

/// Installs `logger`, returning the previous one.
fn replace_logger(logger: Option<Arc<Logger>>) -> Option<Arc<Logger>> {
    let mut slot = LOGGER.write().unwrap_or_else(PoisonError::into_inner);
    let old = std::mem::replace(&mut *slot, logger);
    refresh_max_level(&slot);
    old
}

/// Installs `logger` only if none is installed.
///
/// # Errors
///
/// Gives `logger` back if one is already installed.
pub fn try_set_logger(logger: Logger) -> Result<(), Logger> {
    let mut slot = LOGGER.write().unwrap_or_else(PoisonError::into_inner);
    if slot.is_some() {
        return Err(logger);
    }
    *slot = Some(Arc::new(logger));
    refresh_max_level(&slot);
    Ok(())
}

/// Removes the installed logger, if any.
pub fn clear_logger() {
    let old = replace_logger(None);
    // Dropped after the lock is released, as in `set_logger`.
    drop(old);
}

/// The fast path's level, as a number. For tests of this module only.
#[doc(hidden)]
#[must_use]
pub fn __max_level() -> u8 {
    MAX_LEVEL.load(Ordering::SeqCst)
}

/// Whether a record at `level` from `target` would be kept by the
/// installed logger or a capture on this thread. The macros call it before
/// evaluating their arguments.
#[inline]
#[must_use]
pub fn enabled(level: Level, target: &str) -> bool {
    level as u8 <= MAX_LEVEL.load(Ordering::Relaxed) && enabled_slow(level, target)
}

#[cold]
fn enabled_slow(level: Level, target: &str) -> bool {
    capturing() || read_logger().is_some_and(|l| l.filter.enabled(level, target))
}

/// Whether this thread is in [`capture`]. False during thread-local
/// teardown, when a destructor may log after `CAPTURE` is gone.
fn capturing() -> bool {
    CAPTURE
        .try_with(|capture| capture.borrow().is_some())
        .unwrap_or(false)
}

/// What the macros call. Not for direct use.
#[doc(hidden)]
pub fn __log(level: Level, target: &str, args: fmt::Arguments<'_>) {
    // The read lock is released here, before any message code runs.
    let logger = read_logger();
    let to_logger = logger.as_ref().filter(|l| l.filter.enabled(level, target));
    let capturing = capturing();
    if !capturing && to_logger.is_none() {
        return;
    }
    let message = render(args);
    let timestamp = logger
        .as_ref()
        .map_or_else(|| SystemClock.now(), |l| l.clock.now());
    let record = Record {
        level,
        target,
        timestamp,
        message: &message,
    };
    if capturing {
        // `capturing` is true only inside `capture`, never in teardown.
        let _ = CAPTURE.try_with(|capture| {
            if let Some(entries) = &mut *capture.borrow_mut() {
                if entries.len() >= MAX_CAPTURED {
                    entries.pop_front();
                }
                entries.push_back(Entry::from(&record));
            }
        });
    }
    if let Some(logger) = to_logger {
        logger.write(&record);
    }
}

/// Runs `f` and returns what it returned, with every record this thread
/// logged meanwhile, at every level, whatever the installed filter. Records
/// also go to the installed logger as usual. Records from other threads
/// are not captured; at most the last 10,000 are kept. A capture inside
/// `f` takes over until it ends.
pub fn capture<R>(f: impl FnOnce() -> R) -> (R, Vec<Entry>) {
    /// Restores the previous capture when `f` returns or panics.
    struct Guard(Option<Option<VecDeque<Entry>>>);

    impl Drop for Guard {
        fn drop(&mut self) {
            if let Some(previous) = self.0.take() {
                CAPTURE.set(previous);
            }
            CAPTURES.fetch_sub(1, Ordering::SeqCst);
            refresh_max_level(&LOGGER.write().unwrap_or_else(PoisonError::into_inner));
        }
    }

    let previous = CAPTURE.replace(Some(VecDeque::new()));
    CAPTURES.fetch_add(1, Ordering::SeqCst);
    refresh_max_level(&LOGGER.write().unwrap_or_else(PoisonError::into_inner));
    let mut guard = Guard(Some(previous));
    let result = f();
    let entries = CAPTURE.replace(guard.0.take().unwrap_or_default());
    (result, entries.map(Vec::from).unwrap_or_default())
}

/// A record kept by a [`MemorySink`] or [`capture`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Its level.
    pub level: Level,
    /// The module path it was logged from.
    pub target: String,
    /// When it was logged.
    pub timestamp: Timestamp,
    /// The rendered message.
    pub message: String,
}

impl From<&Record<'_>> for Entry {
    fn from(record: &Record<'_>) -> Self {
        Self {
            level: record.level,
            target: record.target.to_owned(),
            timestamp: record.timestamp,
            message: record.message.to_owned(),
        }
    }
}

/// One record as a line of text, ending in `\n`:
/// `2026-11-03T12:34:56.789Z WARN  softshell_storage::flush: message`.
///
/// The target and message are escaped so a record is always one line and
/// cannot imitate another: `\` becomes `\\`, a newline `\n`, a carriage
/// return `\r`, a tab `\t`, and every other control character, the line
/// and paragraph separators and the bidirectional overrides `\u{…}`.
#[must_use]
pub fn format_line(record: &Record<'_>) -> String {
    let mut line = String::with_capacity(32 + record.target.len() + record.message.len());
    // Writing to a `String` cannot fail.
    let _ = civil::write_rfc3339_millis(record.timestamp, &mut line);
    let _ = write!(line, " {:<5} ", record.level);
    push_escaped(&mut line, record.target);
    line.push_str(": ");
    push_escaped(&mut line, record.message);
    line.push('\n');
    line
}

fn push_escaped(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control()
                || matches!(c, '\u{2028}' | '\u{2029}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}') =>
            {
                let _ = write!(out, "\\u{{{:x}}}", u32::from(c));
            }
            c => out.push(c),
        }
    }
}

/// Renders `args`, keeping at most [`MAX_MESSAGE_BYTES`].
fn render(args: fmt::Arguments<'_>) -> String {
    capped_string(args, MAX_MESSAGE_BYTES)
}

/// Renders `args` into at most `limit` bytes, cut at a character boundary
/// and followed by `…[truncated]` if it was longer. A `Display` impl that
/// fails leaves what it wrote so far. Never panics on its own.
pub(crate) fn capped_string(args: fmt::Arguments<'_>, limit: usize) -> String {
    struct Capped {
        text: String,
        limit: usize,
        truncated: bool,
    }

    impl fmt::Write for Capped {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            // Once cut, nothing more is kept, even from a `Display` impl
            // that ignored the error, so the text stays a prefix.
            if self.truncated {
                return Err(fmt::Error);
            }
            let room = self.limit.saturating_sub(self.text.len());
            if s.len() <= room {
                self.text.push_str(s);
                return Ok(());
            }
            let cut = s.floor_char_boundary(room);
            self.text.push_str(s.get(..cut).unwrap_or_default());
            self.truncated = true;
            Err(fmt::Error)
        }
    }

    let mut capped = Capped {
        text: String::new(),
        limit,
        truncated: false,
    };
    // An error is the cap or the Display impl; either way keep the text.
    let _ = fmt::write(&mut capped, args);
    if capped.truncated {
        capped.text.push_str("…[truncated]");
    }
    capped.text
}

/// Writes each record to standard error as a [`format_line`] line, in one
/// unbuffered write. Errors are ignored.
#[derive(Clone, Copy, Debug, Default)]
pub struct StderrSink;

impl Sink for StderrSink {
    fn write(&self, record: &Record<'_>) {
        let _ = io::stderr()
            .lock()
            .write_all(format_line(record).as_bytes());
    }
}

/// Keeps the last `capacity` records in memory, for tests and for
/// showing recent events.
#[derive(Debug)]
pub struct MemorySink {
    capacity: usize,
    state: Mutex<MemoryState>,
}

#[derive(Debug, Default)]
struct MemoryState {
    entries: VecDeque<Entry>,
    dropped: u64,
}

impl MemorySink {
    /// A sink keeping up to `capacity` records; when it is full, the oldest
    /// is dropped.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            state: Mutex::default(),
        }
    }

    /// The records kept, oldest first.
    #[must_use]
    pub fn entries(&self) -> Vec<Entry> {
        self.lock().entries.iter().cloned().collect()
    }

    /// Removes and returns the records kept, oldest first.
    #[must_use]
    pub fn take(&self) -> Vec<Entry> {
        Vec::from(std::mem::take(&mut self.lock().entries))
    }

    /// How many records were dropped to make room.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.lock().dropped
    }

    fn lock(&self) -> MutexGuard<'_, MemoryState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Sink for MemorySink {
    fn write(&self, record: &Record<'_>) {
        let mut state = self.lock();
        if state.entries.len() >= self.capacity {
            if state.entries.pop_front().is_none() {
                // Capacity 0: nothing is kept.
                state.dropped = state.dropped.saturating_add(1);
                return;
            }
            state.dropped = state.dropped.saturating_add(1);
        }
        state.entries.push_back(Entry::from(record));
    }
}

/// Appends records to a file as [`format_line`] lines, one unbuffered
/// write each, and rotates it when it would grow past a size.
///
/// The file is `path`; rotated files are `path.1` (the newest) to
/// `path.keep`. Before a write that would take a non-empty file past
/// `max_bytes`, the files shift up one number, filling the first missing
/// number (or dropping `path.keep` if none is missing), and a new `path` is
/// started. With `keep` 0, `path` is truncated instead. A record longer
/// than `max_bytes` goes whole into a fresh file.
///
/// Logging never fails its caller. A record that cannot be written is
/// lost and counted in [`failed_writes`](Self::failed_writes); the file is
/// reopened on the next write. A rotation that fails is counted in
/// [`failed_rotations`](Self::failed_rotations); the record is still
/// written to `path`, and rotation is tried again after another `max_bytes`.
/// Log files are not synced: they are not database files.
#[derive(Debug)]
pub struct FileSink {
    path: PathBuf,
    max_bytes: u64,
    keep: u32,
    state: Mutex<FileState>,
    failed_writes: AtomicU64,
    failed_rotations: AtomicU64,
    /// Opens `path` for appending and reads its length: [`open_append`],
    /// or one that fails, in tests.
    opener: fn(&Path) -> io::Result<(File, u64)>,
}

#[derive(Debug)]
struct FileState {
    file: Option<File>,
    /// The file's length as far as this sink knows.
    size: u64,
    /// After a failed rotation, the size the file must reach before the
    /// next attempt.
    retry_at: u64,
}

impl FileSink {
    /// Opens `path` for appending, creating it if needed.
    ///
    /// # Errors
    ///
    /// If the file cannot be opened or its length read.
    pub fn open(path: impl Into<PathBuf>, max_bytes: u64, keep: u32) -> io::Result<Self> {
        let path = path.into();
        let (file, size) = open_append(&path)?;
        Ok(Self {
            path,
            max_bytes,
            keep,
            state: Mutex::new(FileState {
                file: Some(file),
                size,
                retry_at: 0,
            }),
            failed_writes: AtomicU64::new(0),
            failed_rotations: AtomicU64::new(0),
            opener: open_append,
        })
    }

    /// How many records could not be written.
    #[must_use]
    pub fn failed_writes(&self) -> u64 {
        self.failed_writes.load(Ordering::Relaxed)
    }

    /// How many rotations failed.
    #[must_use]
    pub fn failed_rotations(&self) -> u64 {
        self.failed_rotations.load(Ordering::Relaxed)
    }

    /// `path.n`.
    fn rotated(&self, n: u32) -> PathBuf {
        let mut name = self.path.as_os_str().to_owned();
        name.push(format!(".{n}"));
        PathBuf::from(name)
    }

    /// Shifts the files up and leaves `path` absent (or empty, with `keep`
    /// 0). The handle is closed first, since Windows may refuse to rename an
    /// open file.
    fn rotate(&self, state: &mut FileState) -> io::Result<()> {
        state.file = None;
        if self.keep == 0 {
            File::create(&self.path)?;
        } else {
            // Only the files up to the first missing number move, so a
            // rotation that failed part-way loses nothing.
            let free = (1..=self.keep)
                .find(|&n| {
                    fs::symlink_metadata(self.rotated(n))
                        .is_err_and(|e| e.kind() == io::ErrorKind::NotFound)
                })
                .unwrap_or(self.keep);
            for n in (1..free).rev() {
                fs::rename(self.rotated(n), self.rotated(n + 1))?;
            }
            fs::rename(&self.path, self.rotated(1))?;
        }
        state.size = 0;
        state.retry_at = 0;
        Ok(())
    }
}

fn open_append(path: &Path) -> io::Result<(File, u64)> {
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    let size = file.metadata()?.len();
    Ok((file, size))
}

impl Sink for FileSink {
    fn write(&self, record: &Record<'_>) {
        let line = format_line(record);
        let len = line.len() as u64;
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let full = state.size > 0 && state.size.saturating_add(len) > self.max_bytes;
        if full && state.size >= state.retry_at && self.rotate(&mut state).is_err() {
            self.failed_rotations.fetch_add(1, Ordering::Relaxed);
            state.retry_at = state.size.saturating_add(self.max_bytes);
        }
        if state.file.is_none() {
            match (self.opener)(&self.path) {
                Ok((file, size)) => {
                    state.file = Some(file);
                    state.size = size;
                }
                Err(_) => {
                    self.failed_writes.fetch_add(1, Ordering::Relaxed);
                    return;
                }
            }
        }
        let written = state
            .file
            .as_mut()
            .map(|file| file.write_all(line.as_bytes()));
        match written {
            Some(Ok(())) => state.size = state.size.saturating_add(len),
            _ => {
                state.file = None;
                self.failed_writes.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ManualClock;

    fn record<'a>(level: Level, target: &'a str, message: &'a str) -> Record<'a> {
        Record {
            level,
            target,
            timestamp: Timestamp::from_nanos(1_793_534_400_123_456_789),
            message,
        }
    }

    #[test]
    fn levels() {
        assert!(Level::Error < Level::Warn && Level::Debug < Level::Trace);
        assert_eq!("WARN".parse(), Ok(Level::Warn));
        assert_eq!("trace".parse(), Ok(Level::Trace));
        assert_eq!("Info".parse(), Ok(Level::Info));
        assert_eq!("verbose".parse::<Level>(), Err(ParseLevelError));
        assert_eq!("".parse::<Level>(), Err(ParseLevelError));
        assert_eq!(
            ParseLevelError.to_string(),
            "expected one of error, warn, info, debug, trace"
        );
        assert_eq!(format!("[{:<5}]", Level::Info), "[INFO ]");
    }

    #[test]
    fn filter_uses_the_longest_matching_prefix() {
        let filter = Filter::new(Some(Level::Info))
            .target("softshell_storage", Some(Level::Debug))
            .target("softshell_storage::compaction", Some(Level::Trace))
            .target("softshell_server", None);
        assert!(filter.enabled(Level::Info, "softshell_txn"));
        assert!(!filter.enabled(Level::Debug, "softshell_txn"));
        assert!(filter.enabled(Level::Debug, "softshell_storage"));
        assert!(filter.enabled(Level::Debug, "softshell_storage::flush"));
        assert!(!filter.enabled(Level::Trace, "softshell_storage::flush"));
        assert!(filter.enabled(Level::Trace, "softshell_storage::compaction::picker"));
        assert!(!filter.enabled(Level::Error, "softshell_server::session"));
        // A prefix ends on a `::` boundary.
        assert!(!filter.enabled(Level::Debug, "softshell_storage2"));
        assert!(!filter.enabled(Level::Trace, "softshell_storage::compactions"));
        assert_eq!(filter.max_level(), Some(Level::Trace));
        // Setting a prefix again replaces its level.
        let filter = filter.target("softshell_storage::compaction", Some(Level::Warn));
        assert!(!filter.enabled(Level::Info, "softshell_storage::compaction"));
        assert_eq!(filter.max_level(), Some(Level::Debug));
        assert_eq!(Filter::new(None).max_level(), None);
        assert!(!Filter::new(None).enabled(Level::Error, "x"));
        let quiet = Filter::new(None).target("softshell_storage", Some(Level::Warn));
        assert_eq!(quiet.max_level(), Some(Level::Warn));
        assert!(!quiet.enabled(Level::Error, "softshell_txn"));
    }

    #[test]
    fn lines_are_escaped_to_one_line() {
        assert_eq!(
            format_line(&record(
                Level::Warn,
                "softshell_storage::flush",
                "flushed 3 files"
            )),
            "2026-11-01T12:00:00.123Z WARN  softshell_storage::flush: flushed 3 files\n"
        );
        let line = format_line(&record(
            Level::Error,
            "a\nb",
            "x\ny\r\tz \\n \u{1b}[31m \u{0} \u{85} \u{2028} \u{2029} \u{202E} \u{2066} é",
        ));
        // Every bidirectional control and both separators; not their
        // neighbors.
        for c in ('\u{202A}'..='\u{202E}')
            .chain('\u{2066}'..='\u{2069}')
            .chain(['\u{2028}', '\u{2029}'])
        {
            let escaped = format_line(&record(Level::Info, "t", &c.to_string()));
            assert!(
                escaped.ends_with(&format!(": \\u{{{:x}}}\n", u32::from(c))),
                "{c:?}"
            );
        }
        for c in ['\u{2027}', '\u{202F}', '\u{2065}', '\u{206A}'] {
            let kept = format_line(&record(Level::Info, "t", &c.to_string()));
            assert!(kept.ends_with(&format!(": {c}\n")), "{c:?}");
        }
        assert_eq!(
            line,
            "2026-11-01T12:00:00.123Z ERROR a\\nb: x\\ny\\r\\tz \\\\n \\u{1b}[31m \\u{0} \\u{85} \\u{2028} \\u{2029} \\u{202e} \\u{2066} é\n"
        );
        assert_eq!(line.lines().count(), 1);
    }

    #[test]
    fn messages_are_capped_at_a_char_boundary() {
        assert_eq!(render(format_args!("{}", "short")), "short");
        let long = "é".repeat(MAX_MESSAGE_BYTES); // 2 bytes each
        let message = render(format_args!("{long}"));
        assert!(message.ends_with("…[truncated]"));
        assert_eq!(message.len(), MAX_MESSAGE_BYTES + "…[truncated]".len());
        let odd = format!("x{long}");
        let message = render(format_args!("{odd}"));
        assert_eq!(message.len(), MAX_MESSAGE_BYTES - 1 + "…[truncated]".len());
        // A Display impl that fails keeps what it wrote.
        struct Fails;
        impl fmt::Display for Fails {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("partial")?;
                Err(fmt::Error)
            }
        }
        assert_eq!(render(format_args!("{Fails} never")), "partial");
        // A Display impl that ignores the cut adds nothing after it.
        struct Persists;
        impl fmt::Display for Persists {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let _ = f.write_str("abcdé");
                let _ = f.write_str("x");
                f.write_str("y")
            }
        }
        assert_eq!(
            capped_string(format_args!("{Persists}"), 5),
            "abcd…[truncated]"
        );
        assert_eq!(capped_string(format_args!("abcdef"), 3), "abc…[truncated]");
        assert_eq!(capped_string(format_args!("abc"), 3), "abc");
    }

    #[test]
    fn capture_keeps_the_newest_ten_thousand() {
        let ((), entries) = capture(|| {
            for i in 0..MAX_CAPTURED + 5 {
                crate::log::info!("{i}");
            }
        });
        assert_eq!(entries.len(), 10_000);
        assert_eq!(entries[0].message, "5");
        assert_eq!(entries[9_999].message, "10004");
    }

    #[test]
    fn logging_from_a_thread_local_destructor_is_safe() {
        struct LogsOnDrop;
        impl Drop for LogsOnDrop {
            fn drop(&mut self) {
                crate::log::info!("dropped late");
            }
        }
        thread_local! {
            static LATE: LogsOnDrop = const { LogsOnDrop };
        }
        // A capture on this thread keeps the fast path open, so the
        // destructor's call reaches the thread-local check.
        let ((), _) = capture(|| {
            std::thread::spawn(|| {
                // Touched first, so destroyed after `CAPTURE`.
                LATE.with(|_| {});
                crate::log::info!("registers CAPTURE");
            })
            .join()
            .unwrap();
        });
    }

    /// The opener a [`FileSink`] in `failing_opener_and_failed_writes`
    /// uses: 0 opens for real, 1 fails, 2 gives a read-only handle (so
    /// writes fail) and claims the file is empty.
    static OPENER_MODE: AtomicU8 = AtomicU8::new(0);

    fn test_opener(path: &Path) -> io::Result<(File, u64)> {
        match OPENER_MODE.load(Ordering::SeqCst) {
            0 => open_append(path),
            1 => Err(io::Error::other("cannot open")),
            _ => Ok((File::open(path)?, 0)),
        }
    }

    #[test]
    fn failed_writes_and_reopens_are_counted_and_recovered() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/log_unit/opener");
        if dir.exists() {
            fs::remove_dir_all(&dir).unwrap();
        }
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("app.log");
        fs::write(&path, "x".repeat(900)).unwrap();
        let sink = FileSink {
            path: path.clone(),
            max_bytes: 1000,
            keep: 1,
            state: Mutex::new(FileState {
                file: None,
                size: 0,
                retry_at: 0,
            }),
            failed_writes: AtomicU64::new(0),
            failed_rotations: AtomicU64::new(0),
            opener: test_opener,
        };
        let write = |message: &str| {
            sink.write(&Record {
                timestamp: Timestamp::UNIX_EPOCH,
                ..record(Level::Info, "t", message)
            })
        };
        // A handle that cannot write: the record is lost and the handle
        // dropped.
        OPENER_MODE.store(2, Ordering::SeqCst);
        write("r1");
        assert_eq!(sink.failed_writes(), 1);
        // The reopen fails: lost too.
        OPENER_MODE.store(1, Ordering::SeqCst);
        write("r2");
        assert_eq!(sink.failed_writes(), 2);
        // A real reopen learns the file's true size, 900 bytes, so the
        // third record written takes it past 1000 and rotates.
        OPENER_MODE.store(0, Ordering::SeqCst);
        for message in ["r3", "r4", "r5"] {
            write(message);
        }
        assert_eq!((sink.failed_writes(), sink.failed_rotations()), (2, 0));
        let line = |m: &str| format!("1970-01-01T00:00:00.000Z INFO  t: {m}\n");
        let rotated = fs::read_to_string(dir.join("app.log.1")).unwrap();
        assert_eq!(
            rotated,
            format!("{}{}{}", "x".repeat(900), line("r3"), line("r4"))
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), line("r5"));
    }

    #[test]
    fn a_logger_filters_and_feeds_every_sink() {
        let first = Arc::new(MemorySink::new(10));
        let second = Arc::new(MemorySink::new(10));
        let clock = Arc::new(ManualClock::new(Timestamp::from_nanos(5)));
        let logger = Logger::new(Filter::new(Some(Level::Info)), Arc::clone(&clock))
            .sink(Arc::clone(&first))
            .sink(Box::new(Arc::clone(&second)) as Box<dyn Sink>);
        logger.log(Level::Warn, "t", format_args!("w {}", 1));
        logger.log(Level::Debug, "t", format_args!("dropped"));
        clock.set(Timestamp::from_nanos(9));
        logger.log(Level::Info, "t", format_args!("i"));
        let expected = [
            Entry {
                level: Level::Warn,
                target: "t".to_owned(),
                timestamp: Timestamp::from_nanos(5),
                message: "w 1".to_owned(),
            },
            Entry {
                level: Level::Info,
                target: "t".to_owned(),
                timestamp: Timestamp::from_nanos(9),
                message: "i".to_owned(),
            },
        ];
        assert_eq!(first.entries(), expected);
        assert_eq!(second.take(), expected);
        assert!(second.entries().is_empty());
        assert_eq!(logger.filter().max_level(), Some(Level::Info));
    }

    #[test]
    fn memory_sink_keeps_the_newest() {
        let sink = MemorySink::new(3);
        for i in 0..5 {
            sink.write(&record(Level::Info, "t", &i.to_string()));
        }
        let kept: Vec<String> = sink.entries().into_iter().map(|e| e.message).collect();
        assert_eq!(kept, ["2", "3", "4"]);
        assert_eq!(sink.dropped(), 2);
        let none = MemorySink::new(0);
        none.write(&record(Level::Info, "t", "x"));
        assert!(none.entries().is_empty());
        assert_eq!(none.dropped(), 1);
    }

    // `capture` is per thread, so these run beside other tests safely.
    #[test]
    fn capture_sees_every_level_from_this_thread() {
        let ((), entries) = capture(|| {
            crate::log::trace!("t {}", 1);
            crate::log::error!("e");
            std::thread::spawn(|| crate::log::error!("another thread"))
                .join()
                .unwrap();
        });
        let seen: Vec<(Level, &str, &str)> = entries
            .iter()
            .map(|e| (e.level, e.target.as_str(), e.message.as_str()))
            .collect();
        assert_eq!(
            seen,
            [
                (Level::Trace, "softshell_core::log::tests", "t 1"),
                (Level::Error, "softshell_core::log::tests", "e")
            ]
        );
        // Outside a capture, with no logger, nothing is enabled.
        assert!(!capturing());
    }

    #[test]
    fn captures_nest_and_end_on_panic() {
        let (inner, outer) = capture(|| {
            crate::log::info!("outer 1");
            let ((), inner) = capture(|| crate::log::info!("inner"));
            crate::log::info!("outer 2");
            inner
        });
        assert_eq!(inner.len(), 1);
        assert_eq!(inner[0].message, "inner");
        let outer: Vec<&str> = outer.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(outer, ["outer 1", "outer 2"]);
        let panicked = std::panic::catch_unwind(|| capture(|| panic!("boom")));
        assert!(panicked.is_err());
        assert!(!capturing());
    }

    #[test]
    fn message_code_may_log() {
        struct Logs;
        impl fmt::Display for Logs {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                crate::log::warn!("from Display");
                f.write_str("outer")
            }
        }
        let ((), entries) = capture(|| crate::log::info!("{Logs}"));
        let messages: Vec<&str> = entries.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(messages, ["from Display", "outer"]);
    }

    #[test]
    fn arguments_are_not_evaluated_when_off() {
        let evaluated = std::cell::Cell::new(false);
        let touch = || {
            evaluated.set(true);
            1
        };
        // This test binary installs no logger, and this thread is not
        // capturing, so trace is off here.
        crate::log::trace!("{}", touch());
        assert!(!evaluated.get());
        let ((), _) = capture(|| crate::log::trace!("{}", touch()));
        assert!(evaluated.get());
    }
}
