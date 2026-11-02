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

//! The log writer thread: the only place stderr and the log file are
//! written.
//!
//! New in Tidepool; not ported. Nadir's logger wrote synchronously on the
//! logging thread; Tidepool's main thread must never wait on I/O
//! (`docs/01-architecture.md`), so lines go through a bounded queue to one
//! background thread instead.
//!
//! - Queueing a line never blocks on I/O. When the queue is full the line is
//!   dropped and counted; the next line that fits is preceded by a notice
//!   saying how many were lost. (The ring buffer, which is synchronous,
//!   still has every line.)
//! - Control items (open or close the file, flush) are never dropped, and
//!   are processed in order with the lines around them. They are bounded
//!   too, so that a stuck writer cannot grow the queue: a flush requested
//!   right after another one shares its ticket, and opening a file is
//!   refused while an earlier file change is still queued.
//! - Each line goes to the file first, then to stderr, so a stderr write
//!   that blocks does not keep that line from the file.
//! - The thread starts on first use and runs until the process exits. If it
//!   cannot start, or dies, lines are dropped and counted rather than
//!   written on the caller's thread.

use std::collections::VecDeque;
use std::io::{self, Write as _};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::file::RotatingFile;
use super::redact::{Redaction, Secrets};

/// The most lines the writer queue holds before new lines are dropped.
pub const QUEUE_CAPACITY: usize = 1024;

/// The most bytes of lines the writer queue holds before new lines are
/// dropped. A single line larger than this is still queued if the queue is
/// empty.
pub const QUEUE_MAX_BYTES: usize = 4 * 1024 * 1024;

/// How often a waiting [`flush`] checks that the writer is still alive.
const FLUSH_POLL: Duration = Duration::from_millis(50);

/// The most bytes the test capture buffer ([`Stderr::Capture`]) keeps; the
/// oldest lines go first.
const CAPTURE_MAX_BYTES: usize = 1024 * 1024;

/// Where a line goes besides the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stderr {
    /// Not to stderr.
    Off,
    /// To the process's stderr.
    Real,
    /// Into the in-memory capture buffer, for tests.
    Capture,
}

/// One formatted, redacted line on its way to the writer.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Line {
    /// Ends with a newline.
    pub(super) text: String,
    pub(super) stderr: Stderr,
}

enum Item {
    Line(Line),
    /// Replace the file sink; `None` closes it.
    File(Option<RotatingFile>),
    /// Mark everything queued before this as written.
    Flush(u64),
}

enum ThreadState {
    NotStarted,
    Running(JoinHandle<()>),
    Failed,
}

/// The queue between logging threads and the writer.
pub(super) struct Queue {
    items: VecDeque<Item>,
    /// `Item::Line`s in `items`.
    lines: usize,
    /// Bytes of those lines.
    bytes: usize,
    /// Lines dropped since the process started.
    dropped: u64,
    /// Lines dropped since the last drop notice was queued.
    unreported: u64,
    /// The last flush ticket handed out.
    next_ticket: u64,
    /// The last flush ticket the writer completed.
    done_ticket: u64,
    /// Whether the writer must leave the queue alone; see [`hold`].
    held: bool,
    thread: ThreadState,
}

impl Queue {
    pub(super) const fn new() -> Self {
        Self {
            items: VecDeque::new(),
            lines: 0,
            bytes: 0,
            dropped: 0,
            unreported: 0,
            next_ticket: 0,
            done_ticket: 0,
            held: false,
            thread: ThreadState::NotStarted,
        }
    }

    /// Queues `line`, unless the queue is full, in which case the line is
    /// dropped and counted. Before the first line queued after a drop,
    /// queues the notice `notice` builds from the number of lines lost.
    /// Returns whether `line` was queued.
    pub(super) fn push_line(
        &mut self,
        line: Line,
        notice: impl FnOnce(u64) -> Option<Line>,
    ) -> bool {
        let full = self.lines >= QUEUE_CAPACITY
            || (self.lines > 0 && self.bytes.saturating_add(line.text.len()) > QUEUE_MAX_BYTES);
        if full {
            self.dropped = self.dropped.saturating_add(1);
            self.unreported = self.unreported.saturating_add(1);
            return false;
        }
        if self.unreported > 0 {
            if let Some(notice) = notice(self.unreported) {
                self.push_unchecked(notice);
            }
            self.unreported = 0;
        }
        self.push_unchecked(line);
        true
    }

    fn push_unchecked(&mut self, line: Line) {
        self.lines += 1;
        self.bytes = self.bytes.saturating_add(line.text.len());
        self.items.push_back(Item::Line(line));
    }

    /// Queues a flush after everything queued so far and returns its
    /// ticket. If the last item is a flush already, nothing was queued
    /// since, so its ticket serves this request too.
    fn request_flush(&mut self) -> u64 {
        if let Some(Item::Flush(ticket)) = self.items.back() {
            return *ticket;
        }
        self.next_ticket += 1;
        self.items.push_back(Item::Flush(self.next_ticket));
        self.next_ticket
    }

    /// Queues a change of file sink, or hands `file` back if it opens a
    /// file while an earlier change is still queued: the writer is that far
    /// behind, and queueing more open files would only pile them up.
    /// Closing is always queued; the logger closes only after an open, so
    /// at most one open and one close are ever queued.
    fn push_file(&mut self, file: Option<RotatingFile>) -> Result<(), Option<RotatingFile>> {
        let change_queued = self.items.iter().any(|item| matches!(item, Item::File(_)));
        if file.is_some() && change_queued {
            return Err(file);
        }
        self.items.push_back(Item::File(file));
        Ok(())
    }

    fn pop(&mut self) -> Option<Item> {
        let item = self.items.pop_front()?;
        if let Item::Line(line) = &item {
            self.lines -= 1;
            self.bytes = self.bytes.saturating_sub(line.text.len());
        }
        Some(item)
    }

    /// Redacts queued lines again, after a secret was registered. `secrets`
    /// has every registered secret; the queued lines are already free of
    /// the patterns before index `from`. Lines that cannot be made safe are
    /// removed. Returns how many were removed.
    ///
    /// A line is redacted without its terminating newline, which is put
    /// back afterwards, so that a line stays one line even when a secret
    /// ends with a line break. The newline must not complete a secret
    /// either.
    pub(super) fn rescrub(&mut self, secrets: &Secrets, from: usize) -> u64 {
        let mut withheld = 0;
        let mut lines = 0;
        let mut bytes = 0usize;
        self.items.retain_mut(|item| {
            let Item::Line(line) = item else {
                return true;
            };
            let body = line.text.strip_suffix('\n').unwrap_or(&line.text);
            let redacted = if secrets.occurs_since(body, from) {
                match secrets.redact(body) {
                    Redaction::Clean => None,
                    Redaction::Redacted(text) => Some(text),
                    Redaction::Withheld => {
                        withheld += 1;
                        return false;
                    }
                }
            } else {
                None
            };
            if secrets.completed_by_newline(redacted.as_deref().unwrap_or(body)) {
                withheld += 1;
                return false;
            }
            if let Some(mut text) = redacted {
                text.push('\n');
                line.text = text;
            }
            lines += 1;
            bytes = bytes.saturating_add(line.text.len());
            true
        });
        self.lines = lines;
        self.bytes = bytes;
        withheld
    }

    /// Takes the oldest queued line, as the writer would, dropping any
    /// control items before it; for tests.
    #[cfg(test)]
    pub(super) fn pop_line(&mut self) -> Option<String> {
        while let Some(item) = self.pop() {
            if let Item::Line(line) = item {
                return Some(line.text);
            }
        }
        None
    }

    /// The lines queued, oldest first, for tests.
    #[cfg(test)]
    pub(super) fn queued_lines(&self) -> Vec<&str> {
        self.items
            .iter()
            .filter_map(|item| match item {
                Item::Line(line) => Some(line.text.as_str()),
                _ => None,
            })
            .collect()
    }
}

struct Shared {
    queue: Mutex<Queue>,
    /// Signalled when an item is queued.
    work: Condvar,
    /// Signalled when the writer completes a flush.
    done: Condvar,
}

static SHARED: Shared = Shared {
    queue: Mutex::new(Queue::new()),
    work: Condvar::new(),
    done: Condvar::new(),
};

/// What the writer wrote to the capture buffer ([`Stderr::Capture`]).
static CAPTURED: Mutex<String> = Mutex::new(String::new());

fn lock() -> MutexGuard<'static, Queue> {
    // A panic while the lock was held must not silence the log for good:
    // the queue behind the lock is still consistent.
    SHARED.queue.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Queues a line for the writer. See [`Queue::push_line`].
pub(super) fn send_line(line: Line, notice: impl FnOnce(u64) -> Option<Line>) {
    let mut queue = lock();
    if !ensure_running(&mut queue) {
        queue.dropped = queue.dropped.saturating_add(1);
        return;
    }
    if queue.push_line(line, notice) {
        SHARED.work.notify_one();
    }
}

/// Why [`send_file`] could not queue a file change. The file, if any, is
/// dropped (closed) on the calling thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SendFileError {
    /// The writer thread could not start or has died.
    NotRunning,
    /// An earlier file change is still waiting for the writer.
    Busy,
}

/// Replaces the writer's file sink, in order with the lines around it.
pub(super) fn send_file(file: Option<RotatingFile>) -> Result<(), SendFileError> {
    let mut queue = lock();
    if file.is_none() && matches!(queue.thread, ThreadState::NotStarted) {
        // Nothing was ever opened, so there is nothing to close.
        return Ok(());
    }
    if !ensure_running(&mut queue) {
        return Err(SendFileError::NotRunning);
    }
    queue.push_file(file).map_err(|_| SendFileError::Busy)?;
    SHARED.work.notify_one();
    Ok(())
}

/// Runs `f` on the writer queue, under its lock.
pub(super) fn with_queue<R>(f: impl FnOnce(&mut Queue) -> R) -> R {
    f(&mut lock())
}

/// Stops the writer from taking items off the queue, or lets it go on.
/// For tests, through [`super::__hold_writer`].
pub(super) fn hold(hold: bool) {
    lock().held = hold;
    if !hold {
        SHARED.work.notify_one();
    }
}

/// Lines dropped since the process started.
pub(super) fn dropped() -> u64 {
    lock().dropped
}

/// Waits until every line queued before the call has been handed to the
/// OS, or `timeout` passes (`None`: no timeout). Returns `false` on timeout
/// or if the writer is not running.
pub(super) fn flush(timeout: Option<Duration>) -> bool {
    let deadline = timeout.and_then(|timeout| Instant::now().checked_add(timeout));
    let mut queue = lock();
    match &queue.thread {
        // Nothing was ever queued for stderr or a file.
        ThreadState::NotStarted => return true,
        ThreadState::Failed => return false,
        ThreadState::Running(_) => {}
    }
    let ticket = queue.request_flush();
    SHARED.work.notify_one();
    loop {
        if queue.done_ticket >= ticket {
            return true;
        }
        if !is_alive(&mut queue) {
            return false;
        }
        let wait = match deadline {
            Some(deadline) => {
                let now = Instant::now();
                if now >= deadline {
                    return false;
                }
                deadline.saturating_duration_since(now).min(FLUSH_POLL)
            }
            None => FLUSH_POLL,
        };
        queue = SHARED
            .done
            .wait_timeout(queue, wait)
            .unwrap_or_else(PoisonError::into_inner)
            .0;
    }
}

/// Takes what the writer has captured so far.
pub(super) fn take_captured() -> String {
    std::mem::take(&mut *CAPTURED.lock().unwrap_or_else(PoisonError::into_inner))
}

/// Starts the writer if it has not been started. Returns `false` if it
/// could not be started or has died.
fn ensure_running(queue: &mut Queue) -> bool {
    if matches!(queue.thread, ThreadState::NotStarted) {
        let spawned = thread::Builder::new()
            .name("tidepool-log".to_string())
            .spawn(run);
        queue.thread = match spawned {
            Ok(handle) => ThreadState::Running(handle),
            Err(_) => ThreadState::Failed,
        };
    }
    is_alive(queue)
}

/// Returns `true` if the writer is running, noting it as failed if it has
/// exited (which only a bug could make it do).
fn is_alive(queue: &mut Queue) -> bool {
    match &queue.thread {
        ThreadState::Running(handle) if handle.is_finished() => {
            queue.thread = ThreadState::Failed;
            false
        }
        ThreadState::Running(_) => true,
        ThreadState::NotStarted | ThreadState::Failed => false,
    }
}

/// The writer thread's body.
fn run() {
    let mut file: Option<RotatingFile> = None;
    loop {
        match next_item() {
            Item::Line(line) => write_line(&line, &mut file),
            // Assigning drops (and so closes) the previous file here, on
            // this thread.
            Item::File(new) => file = new,
            Item::Flush(ticket) => {
                if let Some(file) = &mut file {
                    // Errors here are as unreportable as write errors.
                    let _ = file.flush();
                }
                let _ = io::stderr().flush();
                lock().done_ticket = ticket;
                SHARED.done.notify_all();
            }
        }
    }
}

fn next_item() -> Item {
    let mut queue = lock();
    loop {
        if !queue.held
            && let Some(item) = queue.pop()
        {
            return item;
        }
        queue = SHARED
            .work
            .wait(queue)
            .unwrap_or_else(PoisonError::into_inner);
    }
}

fn write_line(line: &Line, file: &mut Option<RotatingFile>) {
    // The file first: a stderr write can block for as long as the reader
    // of the pipe pleases.
    if let Some(file) = file {
        // A failed write cannot be reported through the log itself; the
        // line is still in the ring buffer.
        let _ = file.write_line(&line.text);
    }
    match line.stderr {
        Stderr::Off => {}
        Stderr::Real => {
            // If stderr is closed or full there is nowhere to report it.
            let _ = io::stderr().lock().write_all(line.text.as_bytes());
        }
        Stderr::Capture => append_bounded(
            &mut CAPTURED.lock().unwrap_or_else(PoisonError::into_inner),
            &line.text,
            CAPTURE_MAX_BYTES,
        ),
    }
}

/// Appends `line` to `buffer`, then drops the oldest whole lines until
/// `buffer` is at most `max` bytes (or empty).
fn append_bounded(buffer: &mut String, line: &str, max: usize) {
    buffer.push_str(line);
    let Some(excess) = buffer.len().checked_sub(max).filter(|&excess| excess > 0) else {
        return;
    };
    // Cut just after a newline, which is always a character boundary.
    let cut = buffer
        .as_bytes()
        .iter()
        .skip(excess.saturating_sub(1))
        .position(|&byte| byte == b'\n')
        .map_or(buffer.len(), |at| excess + at);
    buffer.drain(..cut);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str) -> Line {
        Line {
            text: format!("{text}\n"),
            stderr: Stderr::Off,
        }
    }

    fn no_notice(_: u64) -> Option<Line> {
        None
    }

    #[test]
    fn a_full_queue_drops_and_counts_lines_without_blocking() {
        let mut queue = Queue::new();
        for n in 0..QUEUE_CAPACITY {
            assert!(queue.push_line(line(&format!("line {n}")), no_notice));
        }
        assert!(!queue.push_line(line("one too many"), no_notice));
        assert!(!queue.push_line(line("two too many"), no_notice));
        assert_eq!(queue.dropped, 2);
        assert_eq!(queue.lines, QUEUE_CAPACITY);

        // Once there is room, the next line is preceded by a notice.
        assert!(matches!(queue.pop(), Some(Item::Line(_))));
        let notice = |lost: u64| Some(line(&format!("{lost} lines dropped")));
        assert!(queue.push_line(line("after"), notice));
        let queued = queue.queued_lines();
        assert_eq!(
            &queued[queued.len() - 2..],
            ["2 lines dropped\n", "after\n"]
        );
        assert_eq!(queue.unreported, 0);
        assert_eq!(queue.dropped, 2);
    }

    #[test]
    fn the_queue_is_bounded_by_bytes_too() {
        let mut queue = Queue::new();
        let big = "x".repeat(QUEUE_MAX_BYTES);
        // One oversized line still goes into an empty queue...
        assert!(queue.push_line(line(&big), no_notice));
        // ...but nothing more fits after it.
        assert!(!queue.push_line(line("small"), no_notice));
        assert_eq!(queue.dropped, 1);
        assert!(queue.pop().is_some());
        assert_eq!((queue.lines, queue.bytes), (0, 0));
        assert!(queue.push_line(line("small"), no_notice));
    }

    #[test]
    fn repeated_flush_requests_do_not_grow_the_queue() {
        // A stuck writer pops nothing, while a caller polls
        // `flush_timeout(Duration::ZERO)`.
        let mut queue = Queue::new();
        let first = queue.request_flush();
        for _ in 0..10_000 {
            assert_eq!(queue.request_flush(), first);
        }
        assert_eq!(queue.items.len(), 1);

        // A line queued after a flush needs a flush of its own, so that
        // the flush covers it; then requests share that one again.
        for n in 0..100 {
            queue.push_line(line(&format!("line {n}")), no_notice);
            let ticket = queue.request_flush();
            assert_eq!(ticket, first + n + 1);
            for _ in 0..100 {
                assert_eq!(queue.request_flush(), ticket);
            }
        }
        assert_eq!(queue.items.len(), 1 + 2 * 100);
        assert!(queue.items.len() <= 1 + 2 * queue.lines);

        // Once the writer pops the last flush, the next request is new.
        let last = queue.next_ticket;
        while queue.pop().is_some() {}
        assert_eq!(queue.request_flush(), last + 1);
    }

    #[test]
    fn opening_a_file_waits_until_the_previous_change_is_taken() {
        let mut queue = Queue::new();
        assert!(
            queue
                .push_file(Some(RotatingFile::unwritable_for_tests()))
                .is_ok()
        );
        // A second open while the first is still queued is refused...
        let refused = queue.push_file(Some(RotatingFile::unwritable_for_tests()));
        assert!(matches!(refused, Err(Some(_))));
        // ...but closing is always queued.
        assert!(queue.push_file(None).is_ok());
        assert!(matches!(
            queue.push_file(Some(RotatingFile::unwritable_for_tests())),
            Err(Some(_))
        ));
        assert_eq!(queue.items.len(), 2);

        // Once the writer has taken both, opening works again.
        assert!(matches!(queue.pop(), Some(Item::File(Some(_)))));
        assert!(matches!(queue.pop(), Some(Item::File(None))));
        assert!(
            queue
                .push_file(Some(RotatingFile::unwritable_for_tests()))
                .is_ok()
        );
    }

    #[test]
    fn the_capture_buffer_keeps_only_the_newest_lines() {
        let mut buffer = String::new();
        for n in 0..100 {
            append_bounded(&mut buffer, &format!("line {n:02}\n"), 40);
        }
        // Each line is 8 bytes, so five fit.
        assert_eq!(buffer, "line 95\nline 96\nline 97\nline 98\nline 99\n");
        // Exactly at the limit nothing is dropped.
        let mut buffer = String::new();
        append_bounded(&mut buffer, "1234567\n", 8);
        assert_eq!(buffer, "1234567\n");
        // A line longer than the limit alone empties the buffer rather than
        // keeping part of a line.
        append_bounded(&mut buffer, "a line that is far too long\n", 8);
        assert_eq!(buffer, "");
        // Multi-byte text is never cut inside a character.
        let mut buffer = String::new();
        append_bounded(&mut buffer, "ééé\n", 7);
        append_bounded(&mut buffer, "ü\n", 7);
        assert_eq!(buffer, "ü\n");
    }

    #[test]
    fn rescrub_redacts_queued_lines_and_removes_unsafe_ones() {
        let mut queue = Queue::new();
        queue.push_line(line("token=hunter2"), no_notice);
        queue.push_line(line("nothing here"), no_notice);
        queue.push_line(line("D]Zq9Zq9"), no_notice);
        queue.items.push_back(Item::Flush(1));

        let mut secrets = Secrets::new();
        secrets.add("hunter2");
        secrets.add("D]Zq9");
        assert_eq!(queue.rescrub(&secrets, 0), 1);
        assert_eq!(
            queue.queued_lines(),
            ["token=[REDACTED]\n", "nothing here\n"]
        );
        assert_eq!(queue.lines, 2);
        assert_eq!(
            queue.bytes,
            "token=[REDACTED]\n".len() + "nothing here\n".len()
        );
        // Control items are kept.
        assert_eq!(queue.items.len(), 3);
    }

    #[test]
    fn rescrub_searches_only_for_the_new_patterns() {
        // Queued lines are free of the patterns registered before, so a line
        // with none of the new ones is left as it is, even if it contains an
        // old one (which, in real use, it never does).
        let mut queue = Queue::new();
        queue.push_line(line("old hunter2 and new s3cr3t"), no_notice);
        queue.push_line(line("only old hunter2"), no_notice);
        let mut secrets = Secrets::new();
        secrets.add("hunter2");
        let from = secrets.pattern_count();
        secrets.add("s3cr3t");
        assert_eq!(queue.rescrub(&secrets, from), 0);
        assert_eq!(
            queue.queued_lines(),
            ["old [REDACTED] and new [REDACTED]\n", "only old hunter2\n"]
        );
    }

    #[test]
    fn rescrub_withholds_a_line_its_newline_would_complete_a_secret_in() {
        let mut queue = Queue::new();
        queue.push_line(line("kept"), no_notice);
        let mut secrets = Secrets::new();
        secrets.add("\n");
        assert_eq!(queue.rescrub(&secrets, 0), 1);
        assert!(queue.queued_lines().is_empty());
        assert_eq!((queue.lines, queue.bytes), (0, 0));
    }
}
