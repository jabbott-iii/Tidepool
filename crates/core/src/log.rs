//! Logging: the [`error!`], [`warn!`], [`info!`], [`debug!`] and [`trace!`]
//! macros, a level filter, secret redaction, and three sinks.
//!
//! Ported from nadir-core `log.rs` (levels, macros, filter, ring buffer and
//! file rotation). Adapted for Tidepool: secrets are redacted before any
//! sink sees a line ([`register_secret`]), and stderr and the file are
//! written by a background thread so that logging never blocks on I/O.
//!
//! ```
//! use tidepool_core::log::{self, Level, info, warn};
//!
//! log::set_stderr(false);
//! info!("opened {} files", 12);
//! warn!("settings.toml: unknown key `colour`");
//! log::set_target_level("tidepool_lang", Some(Level::Trace));
//! assert!(log::tail(1)[0].ends_with("unknown key `colour`"));
//! ```
//!
//! # Sinks
//!
//! Every record that passes the filter is formatted once, as
//! `<RFC 3339 UTC time> <LEVEL> <target>: <message>`, redacted, and then
//! goes to every active sink:
//!
//! - an in-memory **ring buffer** of the last [`RING_CAPACITY`] lines,
//!   always on, read with [`tail`]. The Output panel and crash reports use
//!   it. It is updated synchronously, so it always has every line, but it
//!   cuts lines longer than [`RING_MAX_LINE_BYTES`].
//! - **stderr**, on by default ([`set_stderr`]);
//! - a **file** with rotation, off until [`open_file`] is called.
//!
//! A record's *target* is the module path of the code that logged it. The
//! filter has a default level ([`set_level`], initially [`Level::Info`]) and
//! optional per-target overrides ([`set_target_level`]).
//!
//! A record is always one line in the sinks' eyes. Messages often carry
//! text Tidepool does not control (file names, tool output), so control
//! characters other than tab and newline (C0, DEL and C1) are written
//! escaped, as `\u{1b}`, and never reach a terminal raw; trailing line
//! breaks are dropped; and every line after the first in a multi-line
//! message is indented by two spaces, so that it cannot pass for another
//! record.
//!
//! # Threads
//!
//! There is one logger per process, usable from any thread. Logging formats
//! and redacts the line on the calling thread without holding any lock, so
//! that a long record holds up no other thread; then it takes a short lock
//! to add the line to the ring buffer and queue it. It never waits for
//! stderr or the disk. One background thread, `tidepool-log`, writes queued
//! lines to the file and then to stderr. If it falls behind by
//! [`QUEUE_CAPACITY`] lines, further lines are dropped (counted by
//! [`dropped_lines`]) rather than making the caller wait, and the file gets
//! a notice of how many were lost. Call [`flush`] (or [`flush_timeout`])
//! before the process exits, so that queued lines are not lost; never call
//! it from the main loop.
//!
//! Both sinks share that one thread. If a write to stderr blocks, for
//! example on a pipe that the parent process never reads, the file gets the
//! line being written and then stalls with stderr: later lines are dropped
//! from both sinks until stderr drains. Callers still never wait, and the
//! ring buffer still gets every line.
//!
//! # Redaction
//!
//! [`register_secret`] adds a string that must never be written anywhere.
//! From then on, every occurrence of it, and of the escaped forms that
//! `{:?}` and `escape_debug` print for it (up to three levels deep, as in a
//! `{:?}`-formatted error message that holds a `{:?}`-formatted secret) and
//! that control-character escaping gives it, is replaced with [`REDACTED`]
//! in every line before the line reaches any sink, including the ring
//! buffer. A secret that ends with line breaks is also redacted without
//! them. Other encodings of a secret (its bytes' `Debug` form, JSON `\u`
//! escapes, percent-encoding, base64) are not recognised; register them
//! separately if they might be logged. Registering also re-scrubs the ring
//! buffer and lines still queued for the writer. A line that cannot be made
//! safe (a secret overlapping the marker text, or one that the newline
//! ending the record would complete) is withheld entirely and counted by
//! [`withheld_lines`].
//!
//! Register a secret **before** anything that might log it. Lines already
//! written to stderr or the file cannot be recalled. Secrets stay
//! registered until the process exits; the registry never prints them, even
//! in `Debug` output.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt::{self, Write as _};
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime};

use crate::civil::DateTime;

mod file;
mod redact;
mod writer;

pub use self::file::FileConfig;
pub use self::writer::{QUEUE_CAPACITY, QUEUE_MAX_BYTES};

use self::file::RotatingFile;
use self::redact::{Redaction, Secrets};
use self::writer::{Line, Queue, SendFileError, Stderr};

/// How many lines the in-memory ring buffer keeps.
pub const RING_CAPACITY: usize = 1000;

/// The longest line the ring buffer keeps, in bytes, not counting the
/// truncation notice. A longer line is cut at a character boundary and ends
/// with ` …[truncated <n> bytes]`; stderr and the file still get all of it.
/// Where the cut may have gone through a registered secret, what is left of
/// the secret before the cut becomes a [`REDACTED`] marker before the
/// notice. This bounds the ring buffer, and the copy [`tail`] makes, to
/// about 4 MiB.
pub const RING_MAX_LINE_BYTES: usize = 4096;

/// The largest capacity a reused per-thread formatting buffer keeps after a
/// record; a larger one, left by a long record, is freed.
const MAX_REUSED_CAPACITY: usize = 64 * 1024;

/// What a redacted secret is replaced with.
pub const REDACTED: &str = "[REDACTED]";

/// How long [`open_file`] waits for lines queued for the previous file.
pub const OPEN_FLUSH_TIMEOUT: Duration = Duration::from_secs(2);

/// The severity of a log record. Lower is more severe: `Error < Trace`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    /// Something failed and the result is wrong or missing.
    Error = 1,
    /// Something unexpected happened, but Tidepool carried on.
    Warn,
    /// A normal, notable event: startup, a tool installed.
    Info,
    /// Detail that helps when debugging.
    Debug,
    /// Very fine detail, possibly several records per keystroke.
    Trace,
}

impl Level {
    /// Returns the upper-case name used in log lines, such as `"WARN"`.
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

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.as_str())
    }
}

/// Logs at [`Level::Error`], with `format!` syntax.
#[doc(hidden)]
#[macro_export]
macro_rules! __log_error {
    ($($arg:tt)+) => {
        $crate::log::__log($crate::log::Level::Error, ::core::module_path!(), ::core::format_args!($($arg)+))
    };
}

/// Logs at [`Level::Warn`], with `format!` syntax.
#[doc(hidden)]
#[macro_export]
macro_rules! __log_warn {
    ($($arg:tt)+) => {
        $crate::log::__log($crate::log::Level::Warn, ::core::module_path!(), ::core::format_args!($($arg)+))
    };
}

/// Logs at [`Level::Info`], with `format!` syntax.
#[doc(hidden)]
#[macro_export]
macro_rules! __log_info {
    ($($arg:tt)+) => {
        $crate::log::__log($crate::log::Level::Info, ::core::module_path!(), ::core::format_args!($($arg)+))
    };
}

/// Logs at [`Level::Debug`], with `format!` syntax.
#[doc(hidden)]
#[macro_export]
macro_rules! __log_debug {
    ($($arg:tt)+) => {
        $crate::log::__log($crate::log::Level::Debug, ::core::module_path!(), ::core::format_args!($($arg)+))
    };
}

/// Logs at [`Level::Trace`], with `format!` syntax.
#[doc(hidden)]
#[macro_export]
macro_rules! __log_trace {
    ($($arg:tt)+) => {
        $crate::log::__log($crate::log::Level::Trace, ::core::module_path!(), ::core::format_args!($($arg)+))
    };
}

// The macros are exported from the crate root under hidden names, because
// that is the only place `macro_rules!` can export from, and re-exported
// here under their real ones.
#[doc(inline)]
pub use crate::{
    __log_debug as debug, __log_error as error, __log_info as info, __log_trace as trace,
    __log_warn as warn,
};

/// Sets the level for every target that has no override of its own.
/// Records less severe than this are dropped.
pub fn set_level(level: Level) {
    let mut logger = logger();
    logger.filter.default = level;
    publish_most_verbose(&logger.filter);
}

/// Sets, or with `None` removes, the level override for `target` and the
/// modules inside it.
///
/// `target` is a module path such as `tidepool_lang` or
/// `tidepool_lang::rust`. When several overrides match a record, the
/// longest wins.
pub fn set_target_level(target: &str, level: Option<Level>) {
    let mut logger = logger();
    logger.filter.set_target(target, level);
    publish_most_verbose(&logger.filter);
}

/// Returns `true` if a record at `level` from `target` would be logged.
///
/// The macros check this themselves. Call it only to skip expensive work
/// that exists purely to build a log message.
pub fn enabled(level: Level, target: &str) -> bool {
    // Most disabled records are rejected here, without taking the lock.
    level as u8 <= MOST_VERBOSE.load(Ordering::Relaxed)
        && level <= logger().filter.level_for(target)
}

/// Turns the stderr sink on or off. It is on by default.
pub fn set_stderr(enabled: bool) {
    logger().stderr = enabled;
}

/// Starts writing records to the file at `path`, replacing any file sink
/// already open. The parent directory must exist.
///
/// When the file is full (see [`FileConfig`]) it is renamed to `<path>.1`,
/// the previous `<path>.1` to `<path>.2` and so on, the oldest is deleted,
/// and a fresh file is started. If rotating fails, for example because
/// another program holds one of the files, logging continues in the current
/// file.
///
/// The file is opened (and, with [`FileConfig::rotate_on_open`], rotated)
/// on the calling thread, so that errors can be returned; call this at
/// startup or from a worker, not from the main loop. It first waits up to
/// [`OPEN_FLUSH_TIMEOUT`] for lines already queued, so that they reach the
/// previous file before this one is opened or rotated. Writing and later
/// rotations happen on the log writer thread.
///
/// # Errors
///
/// Returns an error if the file cannot be opened, if the writer thread
/// cannot start, or if the writer has not yet taken an earlier
/// `open_file` or [`close_file`] (it is stuck, for example on a stderr
/// pipe nobody reads). In each case the previous file sink, if any, stays
/// in place.
pub fn open_file(path: impl AsRef<Path>, config: FileConfig) -> io::Result<()> {
    // A timeout only means some earlier lines may land in the new file.
    let _ = flush_timeout(OPEN_FLUSH_TIMEOUT);
    let file = RotatingFile::open(path.as_ref(), config)?;
    let mut logger = logger();
    match writer::send_file(Some(file)) {
        Ok(()) => {
            logger.file_open = true;
            Ok(())
        }
        Err(SendFileError::NotRunning) => {
            Err(io::Error::other("the log writer thread is not running"))
        }
        Err(SendFileError::Busy) => Err(io::Error::other(
            "the log writer has not yet caught up with the previous file change",
        )),
    }
}

/// Stops writing records to a file. Lines logged before the call still
/// reach it. Does nothing if no file is open.
pub fn close_file() {
    let mut logger = logger();
    if logger.file_open {
        // Closing is never refused, and a writer that is not running has
        // no file to close.
        let _ = writer::send_file(None);
        logger.file_open = false;
    }
}

/// Returns up to `max_lines` of the most recent lines, oldest first,
/// formatted as they appear in the log file (without the newline), except
/// that lines longer than [`RING_MAX_LINE_BYTES`] are cut.
pub fn tail(max_lines: usize) -> Vec<String> {
    logger().ring.tail(max_lines)
}

/// Registers `secret` so that it never appears in any sink. See
/// [Redaction](self#redaction).
///
/// Call it as soon as the secret is known and before anything could log
/// it. Re-scrubs the ring buffer and the lines still queued for stderr and
/// the file; lines already written there cannot be recalled. An empty
/// string is ignored. Registering the same secret again does nothing.
///
/// A secret that ends with line breaks, as text read from a file or a
/// tool's output usually does, is registered without them as well, since
/// that is how it reads at the end of a record.
///
/// Every occurrence of the secret becomes [`REDACTED`], so a very short
/// secret blanks out unrelated text too. A secret that is part of the
/// marker itself (such as `"E"` or `"ACT"`) cannot be redacted without
/// writing it again, so every line that contains it, or in which any secret
/// is redacted, is withheld from every sink (see [`withheld_lines`]); for
/// `"E"` that includes every `ERROR` record. Likewise `"\n"`, which ends
/// every record, withholds every record. Nothing is logged about such a
/// secret, because saying so would reveal what it is.
///
/// Re-scrubbing holds up other threads' logging while it searches the ring
/// buffer and the queue for the new secret: up to a few milliseconds when
/// both are full of long lines.
pub fn register_secret(secret: &str) {
    let mut logger = logger();
    writer::with_queue(|queue| register(&mut logger, queue, secret));
}

/// The body of [`register_secret`], on the logger and the writer queue it
/// has locked: registers `secret` and re-scrubs both. Lock order: the
/// logger, then the queue, as in [`emit`].
///
/// Every line in the ring and the queue was redacted with the secrets
/// registered when it was logged, all of which are still registered (see
/// [`emit`]), so only the patterns this registration adds need searching
/// for.
fn register(logger: &mut Logger, queue: &mut Queue, secret: &str) {
    let mut secrets = logger.secrets().clone();
    let from = secrets.pattern_count();
    if !secrets.add(secret) {
        return;
    }
    let removed = logger
        .ring
        .rescrub(&secrets, from)
        .saturating_add(queue.rescrub(&secrets, from));
    logger.withheld = logger.withheld.saturating_add(removed);
    // A new value, never a change in place: threads rendering a line
    // against the old one notice, and render it again (see `emit`).
    logger.secrets = Some(Arc::new(secrets));
}

/// Waits until every line logged before the call has been handed to the OS
/// (stderr and the file). Returns `false` if the log writer is not running.
///
/// This blocks. Call it at shutdown, in a crash handler (prefer
/// [`flush_timeout`] there) or in tests; never from the main loop.
pub fn flush() -> bool {
    writer::flush(None)
}

/// Like [`flush`], but gives up after `timeout`. Returns `true` if
/// everything was written in time.
pub fn flush_timeout(timeout: Duration) -> bool {
    writer::flush(Some(timeout))
}

/// How many lines were dropped since the process started because the log
/// writer fell behind. Dropped lines are still in the ring buffer.
pub fn dropped_lines() -> u64 {
    writer::dropped()
}

/// How many lines were withheld from every sink since the process started
/// because redaction could not make them safe.
pub fn withheld_lines() -> u64 {
    logger().withheld
}

/// The entry point behind the logging macros.
#[doc(hidden)]
pub fn __log(level: Level, target: &str, args: fmt::Arguments<'_>) {
    thread_local! {
        /// Reused buffer for the formatted message.
        static MESSAGE: RefCell<String> = const { RefCell::new(String::new()) };
    }

    if !enabled(level, target) {
        return;
    }
    let time = SystemTime::now();
    if let Some(literal) = args.as_str() {
        emit(time, level, target, literal);
        return;
    }
    // Format before taking the lock. Formatting runs arbitrary `Display`
    // code, and if that code logs, it must not find the lock held.
    let formatted = MESSAGE.try_with(|buffer| {
        let Ok(mut buffer) = buffer.try_borrow_mut() else {
            return false;
        };
        buffer.clear();
        // Writing to a `String` only fails if a `Display` impl reports an
        // error; whatever was formatted up to that point is still logged.
        let _ = buffer.write_fmt(args);
        emit(time, level, target, &buffer);
        // One long record must not pin its memory for the thread's life.
        if buffer.capacity() > MAX_REUSED_CAPACITY {
            *buffer = String::new();
        }
        true
    });
    // The buffer is unavailable in two rare cases: this record is being
    // logged from inside another record's `Display` impl, or the thread is
    // shutting down. Fall back to a temporary string.
    if !matches!(formatted, Ok(true)) {
        let message = args.to_string();
        emit(time, level, target, &message);
    }
}

/// Sends stderr output to an in-memory buffer instead of the process's
/// stderr, or back. For tests of the stderr sink only, which cannot capture
/// the real stderr; the lines take exactly the same path up to the final
/// write. The buffer keeps only the most recent lines, about 1 MiB of them,
/// so a stray call cannot grow memory without bound.
#[doc(hidden)]
pub fn __capture_stderr(capture: bool) {
    logger().capture = capture;
}

/// Flushes, then takes what the stderr sink has written into the capture
/// buffer (see [`__capture_stderr`]).
#[doc(hidden)]
pub fn __take_captured_stderr() -> String {
    flush();
    writer::take_captured()
}

/// Stops the log writer from taking lines off its queue, or lets it start
/// again. For tests only: they hold the writer to keep lines queued while
/// they register a secret. Lines logged meanwhile are queued, or dropped
/// once the queue is full, and [`flush`] waits until the writer is let go.
#[doc(hidden)]
pub fn __hold_writer(hold: bool) {
    writer::hold(hold);
}

/// Formats, redacts and records one line, then queues it for the writer.
///
/// The line is rendered without holding the lock, against the secrets
/// registered when rendering started. If a secret was registered since, the
/// line is rendered again under the lock, so that no line is recorded or
/// queued without every secret registered before it was.
fn emit(time: SystemTime, level: Level, target: &str, message: &str) {
    let snapshot = logger().secrets.clone();
    let mut record = render_record(time, level, target, message, secrets_in(&snapshot));
    let mut logger = logger();
    if !same_secrets(&snapshot, &logger.secrets) {
        record = render_record(time, level, target, message, logger.secrets());
    }
    if let Some(line) = logger.commit(&mut record) {
        // Queued under the logger lock, so that `register_secret` cannot
        // run between recording this line and queueing it.
        writer::send_line(line, |dropped| logger.drop_notice(time, dropped));
    }
    // A record no sink took is freed after the lock is released.
    drop(logger);
    drop(record);
}

/// Registered secrets, when none have been.
static NO_SECRETS: Secrets = Secrets::new();

/// The secrets in a [`Logger::secrets`] value.
fn secrets_in(secrets: &Option<Arc<Secrets>>) -> &Secrets {
    secrets.as_deref().unwrap_or(&NO_SECRETS)
}

/// Returns `true` if `a` and `b` are the same registry value.
fn same_secrets(a: &Option<Arc<Secrets>>, b: &Option<Arc<Secrets>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        _ => false,
    }
}

/// The most verbose level any target is set to, as a `Level as u8`.
static MOST_VERBOSE: AtomicU8 = AtomicU8::new(Level::Info as u8);

static LOGGER: Mutex<Logger> = Mutex::new(Logger::new());

fn logger() -> MutexGuard<'static, Logger> {
    // A panic while the lock was held (say, in a panic hook's own logging)
    // must not silence the log for good: the state behind the lock is still
    // usable.
    LOGGER.lock().unwrap_or_else(PoisonError::into_inner)
}

fn publish_most_verbose(filter: &Filter) {
    MOST_VERBOSE.store(filter.most_verbose() as u8, Ordering::Relaxed);
}

struct Logger {
    filter: Filter,
    stderr: bool,
    /// Whether stderr output goes to the capture buffer instead.
    capture: bool,
    file_open: bool,
    ring: Ring,
    /// The registered secrets; `None` until the first. Replaced by a new
    /// value, never changed in place, so that [`emit`] can tell whether the
    /// value it rendered a line against is still current.
    secrets: Option<Arc<Secrets>>,
    /// Lines withheld because redaction could not make them safe.
    withheld: u64,
}

impl Logger {
    const fn new() -> Self {
        Self {
            filter: Filter::new(),
            stderr: true,
            capture: false,
            file_open: false,
            ring: Ring::new(),
            secrets: None,
            withheld: 0,
        }
    }

    /// The registered secrets.
    fn secrets(&self) -> &Secrets {
        secrets_in(&self.secrets)
    }

    fn stderr_mode(&self) -> Stderr {
        match (self.stderr, self.capture) {
            (false, _) => Stderr::Off,
            (true, false) => Stderr::Real,
            (true, true) => Stderr::Capture,
        }
    }

    /// Renders one record against the registered secrets and commits it,
    /// as [`emit`] does but under one `&mut`.
    #[cfg(test)]
    fn write(
        &mut self,
        time: SystemTime,
        level: Level,
        target: &str,
        message: &str,
    ) -> Option<Line> {
        let mut record = render_record(time, level, target, message, self.secrets());
        self.commit(&mut record)
    }

    /// Appends a record from [`render_record`] to the ring buffer, or counts
    /// it as withheld if it is `None`. Returns the line for the writer, taken
    /// out of `record`, if stderr or a file wants it; otherwise `record` is
    /// left for the caller to free.
    fn commit(&mut self, record: &mut Option<Record>) -> Option<Line> {
        let Some(Record { text, cut }) = record else {
            self.withheld = self.withheld.saturating_add(1);
            return None;
        };
        match cut {
            None => self
                .ring
                .push(text.strip_suffix('\n').unwrap_or(text), None),
            Some((short, cut)) => self.ring.push(short, Some(*cut)),
        }
        let stderr = self.stderr_mode();
        if stderr == Stderr::Off && !self.file_open {
            return None;
        }
        let text = record.take()?.text;
        Some(Line { text, stderr })
    }

    /// The line that tells stderr and the file how many lines were dropped.
    fn drop_notice(&self, time: SystemTime, dropped: u64) -> Option<Line> {
        let message =
            format!("{dropped} log lines were dropped because the log writer fell behind");
        let text = render_record(time, Level::Warn, module_path!(), &message, self.secrets())?.text;
        Some(Line {
            text,
            stderr: self.stderr_mode(),
        })
    }
}

/// One record, rendered for every sink but not yet recorded anywhere.
struct Record {
    /// The line for stderr and the file, ending with its newline.
    text: String,
    /// The ring buffer's copy, if `text` is too long for the ring.
    cut: Option<(String, Cut)>,
}

/// Formats and renders one record against `secrets`: the line for stderr
/// and the file, and the ring buffer's copy if that has to be cut. Returns
/// `None` if the record must be withheld.
///
/// Runs no code but this module's, so [`emit`] calls it without the lock.
fn render_record(
    time: SystemTime,
    level: Level,
    target: &str,
    message: &str,
    secrets: &Secrets,
) -> Option<Record> {
    let mut raw = String::with_capacity(message.len() + 64);
    format_line(&mut raw, time, level, target, message);
    let mut text = render(raw, secrets)?;
    // The newline that ends the record comes after redaction, so it must
    // not complete a secret that ends with one.
    if secrets.completed_by_newline(&text) {
        return None;
    }
    let cut = if text.len() > RING_MAX_LINE_BYTES {
        Some(cut_for_ring(&text, 0, secrets)?)
    } else {
        None
    };
    text.push('\n');
    Some(Record { text, cut })
}

/// Appends `<time> <LEVEL> <target>: <message>` to `out`, as it is, without
/// a newline. [`render`] makes it safe to write.
fn format_line(out: &mut String, time: SystemTime, level: Level, target: &str, message: &str) {
    // Writing to a `String` cannot fail.
    let _ = write!(
        out,
        "{} {level:<5} {target}: {message}",
        DateTime::from_system_time(time)
    );
}

/// The line breaks trimmed from the end of a record.
const LINE_BREAKS: [char; 2] = ['\n', '\r'];

/// What continuation lines of a multi-line record start with, so that no
/// line inside a record can pass for the start of another.
const CONTINUATION_INDENT: &str = "  ";

/// Turns a line from [`format_line`] into what the sinks get (without the
/// final newline): redacted, trailing line breaks trimmed, control
/// characters escaped and continuation lines indented, then redacted again.
/// Returns `None` if the line must be withheld.
///
/// The first pass catches a secret in the form it was logged in, before
/// trimming or escaping can change it. The second catches the form it takes
/// once escaped, and any secret that escaping happens to spell out.
/// Trimming only removes a suffix, which cannot create a secret.
fn render(raw: String, secrets: &Secrets) -> Option<String> {
    let mut text = redact(raw, secrets)?;
    let len = text.trim_end_matches(LINE_BREAKS).len();
    text.truncate(len);
    if let Some(escaped) = escape_controls(&text) {
        text = escaped;
    }
    redact(text, secrets)
}

/// Applies `secrets` to `text`. Returns `None` if the line must be
/// withheld.
fn redact(text: String, secrets: &Secrets) -> Option<String> {
    match secrets.redact(&text) {
        Redaction::Clean => Some(text),
        Redaction::Redacted(out) => Some(out),
        Redaction::Withheld => None,
    }
}

/// Returns `true` for the characters a log line never carries raw: the C0
/// controls other than tab and newline, DEL, and the C1 controls.
fn is_escaped_control(c: char) -> bool {
    c.is_control() && c != '\t' && c != '\n'
}

/// Escapes `text` for a log line: each control character from
/// [`is_escaped_control`] becomes `\u{<hex>}`, and each newline is followed
/// by [`CONTINUATION_INDENT`]. Returns `None` if nothing needs changing.
///
/// Each character is replaced independently, so a string's escaped form
/// appears verbatim inside the escaped form of any text that contains it.
/// Redaction relies on that.
fn escape_controls(text: &str) -> Option<String> {
    if !text.chars().any(|c| c == '\n' || is_escaped_control(c)) {
        return None;
    }
    let mut out = String::with_capacity(text.len() + 16);
    for c in text.chars() {
        if c == '\n' {
            out.push('\n');
            out.push_str(CONTINUATION_INDENT);
        } else if is_escaped_control(c) {
            // Writing to a `String` cannot fail.
            let _ = write!(out, "\\u{{{:x}}}", u32::from(c));
        } else {
            out.push(c);
        }
    }
    Some(out)
}

/// Cuts `line`, which is redacted and longer than [`RING_MAX_LINE_BYTES`],
/// for the ring buffer: its first `RING_MAX_LINE_BYTES` (to a character
/// boundary), then the truncation notice. `removed` is how many bytes were
/// already cut off the end of `line` (0 for a whole record). Returns `None`
/// if the cut line must be withheld.
///
/// `line` was redacted whole, so no registered secret goes through the
/// cut. But the notice could complete one that starts at the end of the
/// kept text; then that start is redacted as well.
fn cut_for_ring(line: &str, removed: usize, secrets: &Secrets) -> Option<(String, Cut)> {
    let at = line.floor_char_boundary(RING_MAX_LINE_BYTES);
    let kept = line.get(..at)?;
    let removed = removed.saturating_add(line.len() - at);
    let nothing_new = secrets.pattern_count();
    fit_cut(kept, removed, secrets, nothing_new).or_else(|| fit_cut(kept, removed, secrets, 0))
}

/// Builds the ring buffer's copy of a cut line from `kept`, the text kept
/// before the cut, and the notice that `removed` bytes were cut off.
/// Returns `None` if the result still contains a registered secret.
///
/// `kept` is redacted, and so is its longest end that is the start of a
/// pattern from index `from` on: whatever followed `kept` is gone, so such a
/// pattern may have gone through the cut ([`Secrets::redact_cut`]).
fn fit_cut(kept: &str, removed: usize, secrets: &Secrets, from: usize) -> Option<(String, Cut)> {
    let notice = redact(format!(" …[truncated {removed} bytes]"), secrets)?;
    let mut text = match secrets.redact_cut(kept, from) {
        Redaction::Clean => kept.to_string(),
        Redaction::Redacted(kept) => kept,
        Redaction::Withheld => return None,
    };
    let cut = Cut {
        kept: text.len(),
        removed,
    };
    text.push_str(&notice);
    if secrets.occurs_in(&text) {
        return None;
    }
    Some((text, cut))
}

struct Filter {
    default: Level,
    /// Per-target overrides, in no particular order.
    targets: Vec<(String, Level)>,
}

impl Filter {
    const fn new() -> Self {
        Self {
            default: Level::Info,
            targets: Vec::new(),
        }
    }

    fn set_target(&mut self, target: &str, level: Option<Level>) {
        self.targets.retain(|(existing, _)| existing != target);
        if let Some(level) = level {
            self.targets.push((target.to_string(), level));
        }
    }

    fn level_for(&self, target: &str) -> Level {
        self.targets
            .iter()
            .filter(|(prefix, _)| is_inside(target, prefix))
            .max_by_key(|(prefix, _)| prefix.len())
            .map_or(self.default, |&(_, level)| level)
    }

    fn most_verbose(&self) -> Level {
        self.targets
            .iter()
            .map(|&(_, level)| level)
            .fold(self.default, Level::max)
    }
}

/// Returns `true` if `target` is the module `module` or is nested inside it.
fn is_inside(target: &str, module: &str) -> bool {
    target
        .strip_prefix(module)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with("::"))
}

/// One line in the ring buffer.
#[derive(Default)]
struct RingLine {
    /// The line as [`tail`] returns it.
    text: String,
    /// How the line was cut, if it was too long.
    cut: Option<Cut>,
}

/// How a line in the ring buffer was cut to [`RING_MAX_LINE_BYTES`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cut {
    /// How many bytes at the start of [`RingLine::text`] are what was kept
    /// of the line, redacted. Whatever followed them in the line is gone;
    /// the truncation notice follows them instead.
    kept: usize,
    /// How many bytes were cut off the end of the line.
    removed: usize,
}

struct Ring {
    lines: VecDeque<RingLine>,
}

impl Ring {
    const fn new() -> Self {
        Self {
            lines: VecDeque::new(),
        }
    }

    /// Appends `text`, which `cut` describes if it is a cut line.
    fn push(&mut self, text: &str, cut: Option<Cut>) {
        // Once full, recycle the oldest line's allocation.
        let mut slot = if self.lines.len() >= RING_CAPACITY {
            self.lines.pop_front().unwrap_or_default()
        } else {
            RingLine::default()
        };
        slot.text.clear();
        slot.text.push_str(text);
        slot.cut = cut;
        self.lines.push_back(slot);
    }

    fn tail(&self, max_lines: usize) -> Vec<String> {
        let skip = self.lines.len().saturating_sub(max_lines);
        self.lines
            .iter()
            .skip(skip)
            .map(|line| line.text.clone())
            .collect()
    }

    /// Redacts the stored lines again, after a secret was registered.
    /// `secrets` has every registered secret; the stored lines are already
    /// free of the patterns before index `from`. Lines that cannot be made
    /// safe are removed. Returns how many were removed.
    fn rescrub(&mut self, secrets: &Secrets, from: usize) -> u64 {
        let mut withheld = 0;
        self.lines.retain_mut(|line| {
            let keep =
                rescrub_ring_line(line, secrets, from) && !secrets.completed_by_newline(&line.text);
            if !keep {
                withheld += 1;
            }
            keep
        });
        withheld
    }
}

/// Redacts one ring buffer line again; see [`Ring::rescrub`]. Returns
/// `false` if the line must be removed.
fn rescrub_ring_line(line: &mut RingLine, secrets: &Secrets, from: usize) -> bool {
    let Some(cut) = line.cut else {
        if !secrets.occurs_since(&line.text, from) {
            return true;
        }
        let Some(text) = redact(std::mem::take(&mut line.text), secrets) else {
            return false;
        };
        // Markers can make a line longer than the limit.
        if text.len() <= RING_MAX_LINE_BYTES {
            line.text = text;
            return true;
        }
        let Some((short, cut)) = cut_for_ring(&text, 0, secrets) else {
            return false;
        };
        (line.text, line.cut) = (short, Some(cut));
        return true;
    };
    // A cut line: the kept text may end with the start of a new secret that
    // went on past the cut, which no redaction of the whole line ever saw.
    let Some(kept) = line.text.get(..cut.kept) else {
        return false;
    };
    if !secrets.occurs_since(&line.text, from) && secrets.partial_at_end(kept, from) == 0 {
        return true;
    }
    let kept = match secrets.redact_cut(kept, from) {
        Redaction::Clean => kept.to_string(),
        Redaction::Redacted(kept) => kept,
        Redaction::Withheld => return false,
    };
    // The marker for the end may take the kept text a marker's length past
    // the limit; other markers may take it further, and then it is cut
    // again, before the old cut.
    let rebuilt = if kept.len() > RING_MAX_LINE_BYTES + REDACTED.len() {
        cut_for_ring(&kept, cut.removed, secrets)
    } else {
        fit_cut(&kept, cut.removed, secrets, secrets.pattern_count())
    };
    let Some((text, cut)) = rebuilt else {
        return false;
    };
    (line.text, line.cut) = (text, Some(cut));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::check;
    use std::time::{Duration, UNIX_EPOCH};

    // These tests use private `Logger`, `Filter` and `Ring` values, never
    // the process-wide logger: unit tests run in parallel and would step on
    // each other's settings. The process-wide logger, the writer thread and
    // the file sink are tested in `tests/log.rs`.

    fn quiet_logger() -> Logger {
        let mut logger = Logger::new();
        logger.stderr = false;
        logger
    }

    /// Registers `secret` with a private logger, as `register_secret` does
    /// with the process-wide one; the writer queue is a throwaway one.
    fn add_secret(logger: &mut Logger, secret: &str) {
        register(logger, &mut Queue::new(), secret);
    }

    #[test]
    fn levels_order_from_most_to_least_severe() {
        assert!(Level::Error < Level::Warn);
        assert!(Level::Warn < Level::Info);
        assert!(Level::Info < Level::Debug);
        assert!(Level::Debug < Level::Trace);
    }

    #[test]
    fn level_names() {
        let names = [
            Level::Error,
            Level::Warn,
            Level::Info,
            Level::Debug,
            Level::Trace,
        ]
        .map(Level::as_str);
        assert_eq!(names, ["ERROR", "WARN", "INFO", "DEBUG", "TRACE"]);
        assert_eq!(Level::Warn.to_string(), "WARN");
        assert_eq!(format!("[{:<5}]", Level::Warn), "[WARN ]");
    }

    #[test]
    fn filter_defaults_to_info() {
        let filter = Filter::new();
        assert_eq!(filter.level_for("tidepool_lang"), Level::Info);
        assert_eq!(filter.most_verbose(), Level::Info);
    }

    #[test]
    fn target_overrides_apply_to_a_module_and_everything_inside_it() {
        let mut filter = Filter::new();
        filter.set_target("tidepool_lang", Some(Level::Trace));
        assert_eq!(filter.level_for("tidepool_lang"), Level::Trace);
        assert_eq!(filter.level_for("tidepool_lang::rust"), Level::Trace);
        assert_eq!(filter.level_for("tidepool_lang::rust::cargo"), Level::Trace);
        // A name that merely starts with the same letters is another crate.
        assert_eq!(filter.level_for("tidepool_language"), Level::Info);
        assert_eq!(filter.level_for("tidepool_vcs"), Level::Info);
        assert_eq!(filter.level_for("tidepool"), Level::Info);
    }

    #[test]
    fn the_longest_matching_override_wins() {
        let mut filter = Filter::new();
        filter.set_target("tidepool_lang::rust", Some(Level::Error));
        filter.set_target("tidepool_lang", Some(Level::Debug));
        assert_eq!(filter.level_for("tidepool_lang::rust::cargo"), Level::Error);
        assert_eq!(filter.level_for("tidepool_lang::markdown"), Level::Debug);
        // An override can make a target quieter than the default as well.
        assert_eq!(filter.most_verbose(), Level::Debug);
    }

    #[test]
    fn overrides_can_be_replaced_and_removed() {
        let mut filter = Filter::new();
        filter.set_target("editor", Some(Level::Trace));
        filter.set_target("editor", Some(Level::Warn));
        assert_eq!(filter.targets.len(), 1);
        assert_eq!(filter.level_for("editor::vim"), Level::Warn);
        assert_eq!(filter.most_verbose(), Level::Info);

        filter.set_target("editor", None);
        assert!(filter.targets.is_empty());
        assert_eq!(filter.level_for("editor::vim"), Level::Info);
        // Removing an override that does not exist is fine.
        filter.set_target("editor", None);
    }

    #[test]
    fn most_verbose_follows_the_default_and_the_overrides() {
        let mut filter = Filter::new();
        filter.default = Level::Error;
        assert_eq!(filter.most_verbose(), Level::Error);
        filter.set_target("a", Some(Level::Debug));
        filter.set_target("b", Some(Level::Warn));
        assert_eq!(filter.most_verbose(), Level::Debug);
        filter.default = Level::Trace;
        assert_eq!(filter.most_verbose(), Level::Trace);
    }

    #[test]
    fn lines_have_a_timestamp_level_target_and_message() {
        let mut logger = quiet_logger();
        let time = UNIX_EPOCH + Duration::from_millis(1_234_567_890_123);
        logger.write(
            time,
            Level::Info,
            "tidepool_workspace::files",
            "opened 12 files",
        );
        logger.write(time, Level::Error, "tidepool", "boom");
        assert_eq!(
            logger.ring.tail(10),
            [
                "2009-02-13T23:31:30.123Z INFO  tidepool_workspace::files: opened 12 files",
                "2009-02-13T23:31:30.123Z ERROR tidepool: boom",
            ]
        );
    }

    #[test]
    fn multi_line_messages_stay_one_record() {
        let mut logger = quiet_logger();
        logger.file_open = true;
        let line = logger
            .write(UNIX_EPOCH, Level::Warn, "app", "first\nsecond\r\n\n")
            .expect("the file wants it");
        // Continuation lines are indented; trailing line breaks go.
        let expected = "1970-01-01T00:00:00.000Z WARN  app: first\n  second";
        assert_eq!(logger.ring.tail(10), [expected]);
        assert_eq!(line.text, format!("{expected}\n"));
    }

    #[test]
    fn control_characters_are_escaped() {
        let mut logger = quiet_logger();
        logger.file_open = true;
        // ESC (an OSC 52 clipboard write), BEL, CR, NUL, DEL and C1 CSI;
        // tab stays.
        let line = logger
            .write(
                UNIX_EPOCH,
                Level::Info,
                "t",
                "a\u{1b}]52;c;aGk=\u{7}b\rc\0d\u{7f}e\u{9b}2Jf\tg",
            )
            .expect("the file wants it");
        assert_eq!(
            line.text,
            "1970-01-01T00:00:00.000Z INFO  t: \
             a\\u{1b}]52;c;aGk=\\u{7}b\\u{d}c\\u{0}d\\u{7f}e\\u{9b}2Jf\tg\n"
        );
        assert_eq!(logger.ring.tail(1)[0], line.text.trim_end());
    }

    #[test]
    fn a_message_cannot_forge_another_record() {
        let mut logger = quiet_logger();
        logger.file_open = true;
        let name = "evil\u{1b}]52;c;aGk=\u{7}\n2026-01-01T00:00:00.000Z ERROR tidepool: forged";
        let line = logger
            .write(UNIX_EPOCH, Level::Warn, "t", &format!("cannot read {name}"))
            .expect("the file wants it");
        assert!(
            line.text
                .bytes()
                .all(|b| b >= 0x20 || b == b'\n' || b == b'\t'),
            "{:?}",
            line.text
        );
        let lines: Vec<&str> = line.text.lines().collect();
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines[0].starts_with("1970-01-01T00:00:00.000Z WARN  t: cannot read evil"));
        assert!(
            lines[1].starts_with("  2026-01-01T00:00:00.000Z ERROR"),
            "{lines:?}"
        );
    }

    #[test]
    fn escaping_that_spells_out_a_secret_is_redacted() {
        // The secret is not in the message, but indenting the continuation
        // line produces it.
        let mut logger = quiet_logger();
        add_secret(&mut logger, "\n  k");
        logger.write(UNIX_EPOCH, Level::Info, "t", "x\nk");
        assert_eq!(
            logger.ring.tail(1),
            ["1970-01-01T00:00:00.000Z INFO  t: x[REDACTED]"]
        );
        // Likewise for the text an escaped control character turns into.
        add_secret(&mut logger, "\\u{1b}[2J");
        logger.write(UNIX_EPOCH, Level::Info, "t", "clear \u{1b}[2J");
        assert_eq!(
            logger.ring.tail(1),
            ["1970-01-01T00:00:00.000Z INFO  t: clear [REDACTED]"]
        );
    }

    #[test]
    fn a_secret_at_the_end_of_a_message_is_redacted_before_trimming() {
        let mut logger = quiet_logger();
        add_secret(&mut logger, "pw\n");
        logger.write(UNIX_EPOCH, Level::Info, "t", "token pw\n");
        assert_eq!(
            logger.ring.tail(1),
            ["1970-01-01T00:00:00.000Z INFO  t: token [REDACTED]"]
        );
    }

    #[test]
    fn lines_go_to_the_writer_only_when_a_sink_wants_them() {
        let mut logger = quiet_logger();
        assert_eq!(
            logger.write(UNIX_EPOCH, Level::Info, "t", "ring only"),
            None
        );

        logger.stderr = true;
        let line = logger
            .write(UNIX_EPOCH, Level::Info, "t", "to stderr")
            .expect("stderr wants it");
        assert_eq!(line.text, "1970-01-01T00:00:00.000Z INFO  t: to stderr\n");
        assert_eq!(line.stderr, Stderr::Real);

        logger.capture = true;
        let line = logger
            .write(UNIX_EPOCH, Level::Info, "t", "captured")
            .expect("stderr wants it");
        assert_eq!(line.stderr, Stderr::Capture);

        logger.stderr = false;
        logger.file_open = true;
        let line = logger
            .write(UNIX_EPOCH, Level::Info, "t", "to the file")
            .expect("the file wants it");
        assert_eq!(line.stderr, Stderr::Off);
        assert_eq!(logger.ring.tail(10).len(), 4);
    }

    #[test]
    fn secrets_are_redacted_before_the_ring_and_the_writer() {
        let mut logger = quiet_logger();
        logger.file_open = true;
        add_secret(&mut logger, "hunter2");
        let line = logger
            .write(UNIX_EPOCH, Level::Info, "t", "password=hunter2")
            .expect("the file wants it");
        assert_eq!(
            line.text,
            "1970-01-01T00:00:00.000Z INFO  t: password=[REDACTED]\n"
        );
        assert_eq!(
            logger.ring.tail(1),
            ["1970-01-01T00:00:00.000Z INFO  t: password=[REDACTED]"]
        );

        // A line that cannot be made safe reaches no sink at all: neither
        // the earlier line, whose marker contains the new secret, nor a new
        // one.
        add_secret(&mut logger, "ACT");
        assert_eq!(logger.withheld, 1);
        assert!(logger.ring.tail(10).is_empty());
        assert_eq!(logger.write(UNIX_EPOCH, Level::Info, "t", "ACT"), None);
        assert_eq!(logger.withheld, 2);
        assert!(logger.ring.tail(10).is_empty());
    }

    #[test]
    fn the_drop_notice_is_redacted_too() {
        let mut logger = quiet_logger();
        logger.stderr = true;
        let notice = logger.drop_notice(UNIX_EPOCH, 3).expect("a notice");
        assert!(
            notice
                .text
                .ends_with(": 3 log lines were dropped because the log writer fell behind\n")
        );
        add_secret(&mut logger, "dropped");
        let notice = logger.drop_notice(UNIX_EPOCH, 3).expect("a notice");
        assert!(!notice.text.contains("dropped"), "{}", notice.text);
    }

    #[test]
    fn registering_rescrubs_the_ring_and_the_writer_queue() {
        let mut logger = quiet_logger();
        logger.file_open = true;
        let mut queue = Queue::new();
        let secret = "tp-queued-3c9d";
        for message in [format!("early {secret}"), "D]Zq9Zq9".to_string()] {
            let line = logger
                .write(UNIX_EPOCH, Level::Info, "t", &message)
                .expect("the file wants it");
            assert!(queue.push_line(line, |_| None));
        }

        register(&mut logger, &mut queue, secret);
        assert_eq!(
            queue.queued_lines(),
            [
                "1970-01-01T00:00:00.000Z INFO  t: early [REDACTED]\n",
                "1970-01-01T00:00:00.000Z INFO  t: D]Zq9Zq9\n"
            ]
        );
        assert_eq!(
            logger.ring.tail(1),
            ["1970-01-01T00:00:00.000Z INFO  t: D]Zq9Zq9"]
        );
        assert_eq!(logger.withheld, 0);

        // A line that cannot be made safe leaves the ring and the queue,
        // and counts once for each.
        register(&mut logger, &mut queue, "D]Zq9");
        assert_eq!(
            queue.queued_lines(),
            ["1970-01-01T00:00:00.000Z INFO  t: early [REDACTED]\n"]
        );
        assert_eq!(
            logger.ring.tail(10),
            ["1970-01-01T00:00:00.000Z INFO  t: early [REDACTED]"]
        );
        assert_eq!(logger.withheld, 2);

        // Registering again re-scrubs nothing.
        register(&mut logger, &mut queue, secret);
        assert_eq!(logger.withheld, 2);
    }

    #[test]
    fn rescrubbing_the_ring_redacts_earlier_lines() {
        let mut logger = quiet_logger();
        logger.write(UNIX_EPOCH, Level::Info, "t", "key is s3cr3t-key");
        logger.write(UNIX_EPOCH, Level::Info, "t", "harmless");
        logger.write(UNIX_EPOCH, Level::Info, "t", "D]Zq9Zq9 marker clash");
        add_secret(&mut logger, "s3cr3t-key");
        add_secret(&mut logger, "D]Zq9");
        assert_eq!(
            logger.withheld, 1,
            "the line that clashes with the marker goes"
        );
        assert_eq!(
            logger.ring.tail(10),
            [
                "1970-01-01T00:00:00.000Z INFO  t: key is [REDACTED]",
                "1970-01-01T00:00:00.000Z INFO  t: harmless",
            ]
        );
    }

    /// The forms a secret takes in a line, worked out from `std` (and the
    /// line format) rather than from the patterns under test: raw, as a
    /// line writes it, `{:?}` one, two and three levels deep, and without
    /// its trailing line breaks.
    fn expected_forms(secret: &str) -> Vec<String> {
        let inner = |text: &str| {
            let debug = format!("{text:?}");
            debug[1..debug.len() - 1].to_string()
        };
        let written = |text: &str| escape_controls(text).unwrap_or_else(|| text.to_string());
        let d1 = inner(secret);
        let d2 = inner(&d1);
        let d3 = inner(&d2);
        let mut forms = vec![secret.to_string(), written(secret), d1, d2, d3];
        let trimmed = secret.trim_end_matches(LINE_BREAKS);
        if !trimmed.is_empty() && trimmed != secret {
            forms.extend([trimmed.to_string(), written(trimmed)]);
        }
        forms
    }

    #[test]
    fn no_registered_secret_reaches_any_sink() {
        // Random secrets (with quotes, backslashes, line breaks, a tab, a
        // combining mark and non-ASCII), logged raw, through `{:?}` one and
        // two levels deep, at the very end of a message, and straddling the
        // ring buffer's cut, at random points before and after registration.
        // Lines go through a writer queue, from which some are taken
        // ("written") as the run goes; those taken before registration are
        // out of reach. Once the secrets are registered:
        //
        // - neither the ring buffer nor the stream of everything the writer
        //   takes afterwards (one string, so that the newline ending each
        //   record counts) contains any form of any secret;
        // - no cut ring line ends, before its notice, with the start of a
        //   form;
        // - nothing is withheld: no character of the alphabet or of its
        //   escapes occurs in `[REDACTED]`, so redaction never has to.
        const ALPHABET: [&str; 13] = [
            "k", "Q", "7", "\"", "\\", "\n", "\r", "\t", "'", "é", "\u{301}", " ", "\u{1b}",
        ];
        const STEPS: u32 = 8;
        const PREFIX: usize = "1970-01-01T00:00:00.000Z INFO  t: ".len();
        check(
            "logger never leaks a registered secret",
            500,
            |rng| {
                let secrets: Vec<String> = (0..3)
                    .map(|_| {
                        let len = rng.range(4..10u32);
                        (0..len)
                            .map(|_| *rng.choose(&ALPHABET).expect("not empty"))
                            .collect()
                    })
                    .collect();
                // Per step: the message shape, which secret, where the cut
                // falls in it, and how many lines the writer takes.
                let steps: Vec<[u32; 4]> = (0..STEPS)
                    .map(|_| {
                        [
                            rng.range(0..7u32),
                            rng.range(0..3u32),
                            rng.next_u32(),
                            rng.range(0..3u32),
                        ]
                    })
                    .collect();
                let register_at = rng.range(0..STEPS);
                (secrets, steps, register_at)
            },
            |(secrets, steps, register_at)| {
                let mut logger = quiet_logger();
                logger.file_open = true;
                let mut queue = Queue::new();
                let mut written = Vec::new();
                for (step, &[shape, which, at, takes]) in (0..).zip(steps) {
                    if step == *register_at {
                        for secret in secrets {
                            register(&mut logger, &mut queue, secret);
                        }
                        written.clear();
                    }
                    let secret = &secrets[which as usize];
                    let trimmed = secret.trim_end_matches(LINE_BREAKS);
                    // For the long shapes: the secret's form starts this many
                    // bytes before the cut, so that the cut goes through it.
                    // The padding is `x`, which no form contains, so a cut
                    // line can end with the start of a form only if a secret
                    // was cut.
                    let straddle = |form: &str| {
                        let k = 1 + at as usize % form.len();
                        "x".repeat(RING_MAX_LINE_BYTES - PREFIX - k)
                    };
                    let message = match shape {
                        0 => format!("token={secret} end"),
                        1 => format!("config {:?}", ("api_key", secret)),
                        2 => format!("{:?}", Err::<(), _>(format!("bad {secret:?}"))),
                        3 => format!("{:?}", format!("{secret:?}")),
                        4 => format!("token {trimmed}"),
                        5 => {
                            let written = escape_controls(secret).unwrap_or(secret.clone());
                            format!("{}{secret}xxxx", straddle(&written))
                        }
                        _ => {
                            let debug = format!("{secret:?}");
                            let inner = &debug[1..debug.len() - 1];
                            format!("{}{inner}xxxx", straddle(inner))
                        }
                    };
                    let Some(line) = logger.write(UNIX_EPOCH, Level::Info, "t", &message) else {
                        return Err(format!("step {step}: {message:?} was withheld"));
                    };
                    if !queue.push_line(line, |_| None) {
                        return Err(format!("step {step}: the queue is full"));
                    }
                    for _ in 0..takes {
                        written.extend(queue.pop_line());
                    }
                }
                written.extend(std::iter::from_fn(|| queue.pop_line()));
                if logger.withheld != 0 {
                    return Err(format!("{} lines were withheld", logger.withheld));
                }
                let stream = written.concat();
                let ring = logger.ring.tail(RING_CAPACITY);
                for secret in secrets {
                    for form in expected_forms(secret) {
                        if stream.contains(&form) {
                            return Err(format!("{form:?} leaked into the stream {stream:?}"));
                        }
                        if let Some(line) = ring.iter().find(|line| line.contains(&form)) {
                            return Err(format!("{form:?} leaked into the ring: {line:?}"));
                        }
                        for line in &ring {
                            let Some((kept, _)) = line.rsplit_once(" …[truncated") else {
                                continue;
                            };
                            if let Some((end, _)) = form
                                .char_indices()
                                .skip(1)
                                .find(|&(end, _)| kept.ends_with(&form[..end]))
                            {
                                return Err(format!(
                                    "a cut ring line ends with {:?}, the start of {form:?}",
                                    &form[..end]
                                ));
                            }
                        }
                    }
                }
                Ok(())
            },
        );
    }

    /// The last few characters of a long line, for assertion messages.
    fn end_of(line: &str) -> &str {
        &line[line.floor_char_boundary(line.len().saturating_sub(60))..]
    }

    #[test]
    fn a_secret_cut_by_the_ring_is_redacted_when_registered_later() {
        // A line longer than the ring's limit is logged before the secret is
        // registered, with the secret going through the cut at every
        // possible place. Re-scrubbing must not leave the part before the
        // cut behind.
        const PREFIX: &str = "1970-01-01T00:00:00.000Z INFO  t: ";
        for secret in ["sk-live-0123456789abcdef0123456789abcdef", "tp-é-ü-😀-ключ"] {
            for k in 1..secret.len() {
                let mut logger = quiet_logger();
                let pad = "x".repeat(RING_MAX_LINE_BYTES - PREFIX.len() - k);
                logger.write(
                    UNIX_EPOCH,
                    Level::Info,
                    "t",
                    &format!("{pad}{secret} and more"),
                );
                add_secret(&mut logger, secret);
                let tail = logger.ring.tail(1);
                let (kept, notice) = tail[0]
                    .rsplit_once(" …[truncated ")
                    .expect("the line is cut");
                for (end, _) in secret.char_indices().skip(1) {
                    assert!(
                        !kept.ends_with(&secret[..end]),
                        "k = {k}: {:?} is left: …{:?}",
                        &secret[..end],
                        end_of(kept)
                    );
                }
                // The cut falls inside the secret (moved back to a character
                // boundary, which is never the secret's start here), and a
                // marker takes the place of what is left of it.
                let full = format!("{PREFIX}{pad}{secret} and more");
                let at = full.floor_char_boundary(RING_MAX_LINE_BYTES);
                assert!(at > PREFIX.len() + pad.len(), "k = {k}");
                assert!(
                    kept == format!("{PREFIX}{pad}[REDACTED]"),
                    "k = {k}: …{:?}",
                    end_of(kept)
                );
                assert_eq!(notice, format!("{} bytes]", full.len() - at), "k = {k}");
                assert_eq!(logger.withheld, 0);
            }
        }
    }

    #[test]
    fn a_secret_cut_by_the_ring_after_registration_is_redacted_whole() {
        // Registered first, the secret is redacted before the line is cut,
        // so the cut goes through the marker instead.
        let secret = "sk-live-0123456789abcdef0123456789abcdef";
        let prefix = "1970-01-01T00:00:00.000Z INFO  t: ";
        for k in 1..secret.len() {
            let mut logger = quiet_logger();
            add_secret(&mut logger, secret);
            let pad = "x".repeat(RING_MAX_LINE_BYTES - prefix.len() - k);
            logger.write(UNIX_EPOCH, Level::Info, "t", &format!("{pad}{secret}{pad}"));
            let tail = logger.ring.tail(1);
            let (kept, _) = tail[0].rsplit_once(" …[truncated ").expect("cut");
            let redacted = format!("{prefix}{pad}{REDACTED}{pad}");
            assert!(
                kept == &redacted[..RING_MAX_LINE_BYTES],
                "k = {k}: …{:?}",
                end_of(kept)
            );
        }
    }

    #[test]
    fn a_cut_line_rescrubbed_again_keeps_its_notice() {
        let mut logger = quiet_logger();
        let prefix = "1970-01-01T00:00:00.000Z INFO  t: ";
        let pad = "x".repeat(RING_MAX_LINE_BYTES - prefix.len() - 4);
        logger.write(UNIX_EPOCH, Level::Info, "t", &format!("{pad}abcdefgh tail"));
        add_secret(&mut logger, "abcdefgh");
        let once = logger.ring.tail(1);
        assert!(
            once[0] == format!("{prefix}{pad}[REDACTED] …[truncated 9 bytes]"),
            "…{:?}",
            end_of(&once[0])
        );
        // An unrelated secret leaves the line as it was.
        add_secret(&mut logger, "zzzz");
        assert_eq!(logger.ring.tail(1), once);
        // One that occurs in the kept text is redacted there, and the notice
        // stays. (As in any re-scrubbed line, the new marker does not merge
        // with the one already there.)
        add_secret(&mut logger, "xxxxxxxx");
        let tail = logger.ring.tail(1);
        assert_eq!(
            tail[0],
            format!("{prefix}[REDACTED][REDACTED] …[truncated 9 bytes]"),
            "the padding is a secret now"
        );
        assert_eq!(
            logger.ring.lines[0].cut,
            Some(Cut {
                kept: prefix.len() + 2 * REDACTED.len(),
                removed: 9
            })
        );
        assert_eq!(logger.withheld, 0);
    }

    #[test]
    fn a_secret_that_ends_with_a_line_break_is_redacted_without_it() {
        // A token read from a file or a tool's output ends with a newline,
        // and is usually logged trimmed. The newline ending the record must
        // not complete it.
        let mut logger = quiet_logger();
        logger.file_open = true;
        add_secret(&mut logger, "tp-tok-5a5a5a\n");
        let line = logger
            .write(UNIX_EPOCH, Level::Info, "t", "using token tp-tok-5a5a5a")
            .expect("the file wants it");
        assert_eq!(
            line.text,
            "1970-01-01T00:00:00.000Z INFO  t: using token [REDACTED]\n"
        );
        assert!(!line.text.contains("tp-tok-5a5a5a"));
        assert_eq!(
            logger.ring.tail(1),
            ["1970-01-01T00:00:00.000Z INFO  t: using token [REDACTED]"]
        );
        // `\r\n` endings too.
        add_secret(&mut logger, "tp-crlf-77\r\n");
        let line = logger
            .write(UNIX_EPOCH, Level::Info, "t", "crlf tp-crlf-77")
            .expect("the file wants it");
        assert_eq!(
            line.text,
            "1970-01-01T00:00:00.000Z INFO  t: crlf [REDACTED]\n"
        );
    }

    #[test]
    fn the_newline_ending_a_record_never_completes_a_secret() {
        // A secret that is a newline is in every record's ending, so every
        // record is withheld, the drop notice included.
        let mut logger = quiet_logger();
        logger.stderr = true;
        logger.write(UNIX_EPOCH, Level::Info, "t", "before");
        add_secret(&mut logger, "\n");
        assert_eq!(logger.withheld, 1, "the ring line goes too");
        assert!(logger.ring.tail(10).is_empty());
        assert_eq!(
            logger.write(UNIX_EPOCH, Level::Info, "t", "single line"),
            None
        );
        assert_eq!(logger.withheld, 2);
        assert!(logger.ring.tail(10).is_empty());
        assert_eq!(logger.drop_notice(UNIX_EPOCH, 3), None);
    }

    #[test]
    fn rescrubbing_keeps_every_queued_line_one_line() {
        // A secret that ends with a line break must not take the newline
        // that ends a queued line with it.
        let mut logger = quiet_logger();
        logger.file_open = true;
        let mut queue = Queue::new();
        for message in ["token pw", "next record"] {
            let line = logger
                .write(UNIX_EPOCH, Level::Info, "t", message)
                .expect("the file wants it");
            assert!(queue.push_line(line, |_| None));
        }
        register(&mut logger, &mut queue, "pw\n");
        let queued = queue.queued_lines();
        assert_eq!(
            queued,
            [
                "1970-01-01T00:00:00.000Z INFO  t: token [REDACTED]\n",
                "1970-01-01T00:00:00.000Z INFO  t: next record\n"
            ]
        );
        assert!(queued.iter().all(|line| line.ends_with('\n')));
        assert_eq!(queued.concat().lines().count(), queued.len());
    }

    #[test]
    fn register_secret_rescrubs_the_process_wide_writer_queue() {
        // The public entry point must re-scrub the writer's real queue, not
        // a copy. This test binary never logs through the process-wide
        // logger, so the queue holds only what this test puts there; the
        // writer is held in case it ever runs.
        let secret = "tp-probe-wire-6b1d";
        writer::hold(true);
        writer::with_queue(|queue| {
            let line = Line {
                text: format!("x {secret}\n"),
                stderr: Stderr::Off,
            };
            assert!(queue.push_line(line, |_| None));
        });
        register_secret(secret);
        let queued = writer::with_queue(|queue| {
            let mut queued = Vec::new();
            while let Some(line) = queue.pop_line() {
                queued.push(line);
            }
            queued
        });
        writer::hold(false);
        assert_eq!(queued, ["x [REDACTED]\n"]);
    }

    #[test]
    fn ring_keeps_only_the_most_recent_lines() {
        let mut ring = Ring::new();
        assert!(ring.tail(5).is_empty());
        for n in 0..RING_CAPACITY + 250 {
            ring.push(&format!("line {n}"), None);
        }
        assert_eq!(ring.lines.len(), RING_CAPACITY);
        let all = ring.tail(usize::MAX);
        assert_eq!(all.len(), RING_CAPACITY);
        assert_eq!(all[0], "line 250");
        assert_eq!(
            all[RING_CAPACITY - 1],
            format!("line {}", RING_CAPACITY + 249)
        );
        // Oldest first, and no more than asked for.
        assert_eq!(
            ring.tail(2),
            [
                format!("line {}", RING_CAPACITY + 248),
                format!("line {}", RING_CAPACITY + 249)
            ]
        );
        assert!(ring.tail(0).is_empty());
    }

    #[test]
    fn long_lines_are_cut_for_the_ring_but_not_for_the_writer() {
        let mut logger = quiet_logger();
        logger.file_open = true;
        // The two-byte `é` straddles the limit, so the cut falls before it.
        let prefix = "1970-01-01T00:00:00.000Z INFO  t: ";
        let message = format!(
            "{}é{}",
            "x".repeat(RING_MAX_LINE_BYTES - 1 - prefix.len()),
            "y".repeat(100)
        );
        let line = logger
            .write(UNIX_EPOCH, Level::Info, "t", &message)
            .expect("the file wants it");
        assert_eq!(line.text, format!("{prefix}{message}\n"));

        let kept = &logger.ring.tail(1)[0];
        let (cut, notice) = kept.split_once(" …").expect("a truncation notice");
        assert_eq!(cut.len(), RING_MAX_LINE_BYTES - 1);
        assert!(cut.ends_with('x') && line.text.starts_with(cut));
        let full = line.text.len() - 1;
        assert_eq!(notice, format!("[truncated {} bytes]", full - cut.len()));
    }

    #[test]
    fn the_ring_stays_within_its_byte_budget() {
        // A thousand lines of 1 MiB cost the ring at most `RING_CAPACITY`
        // cut lines.
        let secrets = Secrets::new();
        let huge = "z".repeat(1024 * 1024);
        let (short, cut) = cut_for_ring(&huge, 0, &secrets).expect("nothing to redact");
        let mut ring = Ring::new();
        for _ in 0..RING_CAPACITY + 10 {
            ring.push(&short, Some(cut));
        }
        let notice = " …[truncated 1044480 bytes]";
        assert_eq!(short.len(), RING_MAX_LINE_BYTES + notice.len());
        assert_eq!(
            cut,
            Cut {
                kept: RING_MAX_LINE_BYTES,
                removed: 1_044_480
            }
        );
        let total: usize = ring.lines.iter().map(|line| line.text.len()).sum();
        assert_eq!(ring.lines.len(), RING_CAPACITY);
        assert!(total <= RING_CAPACITY * (RING_MAX_LINE_BYTES + notice.len()));
        assert!(total < 5 * 1024 * 1024, "{total} bytes");

        // Through the logger, a huge message ends up cut too.
        let mut logger = quiet_logger();
        logger.write(UNIX_EPOCH, Level::Info, "t", &huge);
        assert!(logger.ring.tail(1)[0].len() < RING_MAX_LINE_BYTES + 64);
    }

    #[test]
    fn a_truncation_notice_that_completes_a_secret_is_redacted() {
        let mut secrets = Secrets::new();
        secrets.add("x …[trunc");
        let line = "x".repeat(RING_MAX_LINE_BYTES + 10);
        let (short, cut) = cut_for_ring(&line, 0, &secrets).expect("redactable");
        assert!(!short.contains("x …[trunc"), "{short}");
        // The `x` that would start the secret goes, and the notice stays
        // whole.
        assert_eq!(
            short,
            format!(
                "{}[REDACTED] …[truncated 10 bytes]",
                "x".repeat(RING_MAX_LINE_BYTES - 1)
            )
        );
        assert_eq!(
            cut,
            Cut {
                kept: RING_MAX_LINE_BYTES - 1 + REDACTED.len(),
                removed: 10
            }
        );
    }

    #[test]
    fn rescrubbing_keeps_ring_lines_within_the_limit() {
        // Markers longer than the secrets they replace must not grow a
        // line past the limit.
        let mut ring = Ring::new();
        ring.push(&"q-".repeat(RING_MAX_LINE_BYTES / 2), None);
        let mut secrets = Secrets::new();
        secrets.add("q");
        assert_eq!(ring.rescrub(&secrets, 0), 0);
        assert!(ring.lines[0].cut.is_some());
        let line = &ring.lines[0].text;
        assert!(
            line.len() < RING_MAX_LINE_BYTES + 64,
            "{} bytes",
            line.len()
        );
        assert!(!line.contains('q'));
    }

    #[test]
    fn a_full_ring_recycles_allocations() {
        let mut ring = Ring::new();
        for _ in 0..RING_CAPACITY {
            ring.push("a line of typical length, give or take a few", None);
        }
        let oldest = ring.lines[0].text.as_ptr();
        ring.push("short", None);
        assert_eq!(ring.lines[RING_CAPACITY - 1].text, "short");
        assert_eq!(ring.lines[RING_CAPACITY - 1].text.as_ptr(), oldest);
    }
}
