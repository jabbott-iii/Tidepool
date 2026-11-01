//! Tests of the process-wide logger, its writer thread, the file sink and
//! redaction, through the public API only.
//!
//! Ported from nadir-core `tests/log.rs` (the file-sink and filter tests),
//! with flushes added for the background writer. The redaction and stderr
//! tests are new.
//!
//! Everything here shares one logger, so each test takes `LOGGER_LOCK` and
//! leaves the logger as it found it. Secrets registered here stay registered
//! for the rest of this test binary, so each test uses its own.

use std::fmt;
use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tidepool_core::log::{self, FileConfig, Level, debug, error, info, trace, warn};

static LOGGER_LOCK: Mutex<()> = Mutex::new(());

/// Serializes the tests, silences stderr and restores the default filter.
/// A test that fails while holding the lock must not fail the others.
fn exclusive_logger() -> MutexGuard<'static, ()> {
    let guard = LOGGER_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    log::__hold_writer(false);
    log::close_file();
    log::set_stderr(false);
    log::__capture_stderr(false);
    log::set_level(Level::Info);
    log::set_target_level(module_path!(), None);
    assert!(log::flush(), "the writer is running");
    let _ = log::__take_captured_stderr();
    guard
}

/// A test's scratch directory. It is removed when the test passes; a
/// failing test leaves it behind for inspection.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            // Best effort: a leftover directory only takes space.
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

impl Deref for Scratch {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

/// Returns a fresh, empty directory for one test, under `target/tmp`. The
/// process id keeps concurrent runs of this binary (two `cargo test`s on
/// one target directory) out of each other's way.
fn scratch_dir(test: &str) -> Scratch {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("tidepool-core-log")
        .join(format!("{test}-{}", std::process::id()));
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("remove the previous run's directory");
    }
    fs::create_dir_all(&dir).expect("create the test directory");
    Scratch(dir)
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// Closes the file sink and waits until everything has been written.
fn close_and_flush() {
    log::close_file();
    assert!(log::flush(), "the writer is running");
}

/// Asserts that `line` starts like a record from this file at `level`: an
/// RFC 3339 UTC timestamp, the level, and this test crate's target.
fn assert_record_start(line: &str, level: &str) {
    let (stamp, rest) = line.split_once(' ').expect("line has a timestamp");
    assert_eq!(stamp.len(), "2026-10-12T19:28:03.123Z".len(), "{line}");
    assert!(
        stamp.ends_with('Z') && stamp.as_bytes()[10] == b'T',
        "{line}"
    );
    assert!(rest.starts_with(&format!("{level:<5} log: ")), "{line}");
}

/// The message part of each line: everything after `<timestamp> <LEVEL> <target>: `.
fn messages(text: &str) -> Vec<&str> {
    text.lines()
        .map(|line| line.split_once(": ").expect("line has a target").1)
        .collect()
}

fn file_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("list the test directory")
        .map(|entry| {
            entry
                .expect("read a directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

#[test]
fn records_reach_the_ring_buffer_with_level_and_target() {
    let _guard = exclusive_logger();
    error!("disk {} is on fire", 3);
    warn!("plain literal");
    let tail = log::tail(2);
    // The target is this file's module path: the test crate's name.
    assert!(
        tail[0].ends_with(" ERROR log: disk 3 is on fire"),
        "{tail:?}"
    );
    assert!(tail[1].ends_with(" WARN  log: plain literal"), "{tail:?}");
    // The line starts with an RFC 3339 UTC timestamp.
    let stamp = tail[0].split(' ').next().expect("line has a timestamp");
    assert_eq!(stamp.len(), "2026-10-12T19:28:03.123Z".len(), "{stamp}");
    assert!(
        stamp.ends_with('Z') && stamp.as_bytes()[10] == b'T',
        "{stamp}"
    );
}

#[test]
fn the_level_filter_drops_less_severe_records() {
    let _guard = exclusive_logger();
    let target = module_path!();

    // Default: Info and more severe.
    assert!(log::enabled(Level::Info, target));
    assert!(!log::enabled(Level::Debug, target));
    info!("marker one");
    debug!("dropped: debug at the default level");
    assert!(log::tail(1)[0].ends_with("marker one"));

    log::set_level(Level::Warn);
    assert!(!log::enabled(Level::Info, target));
    info!("dropped: info at warn level");
    warn!("marker two");
    assert!(log::tail(1)[0].ends_with("marker two"));

    // A per-target override beats the default, in either direction.
    log::set_target_level(target, Some(Level::Trace));
    assert!(log::enabled(Level::Trace, target));
    assert!(!log::enabled(Level::Info, "some_other_crate"));
    trace!("marker three");
    assert!(log::tail(1)[0].ends_with(" TRACE log: marker three"));

    log::set_target_level(target, None);
    assert!(!log::enabled(Level::Trace, target));
    trace!("dropped: trace after the override was removed");
    assert!(log::tail(1)[0].ends_with("marker three"));

    log::set_level(Level::Info);
}

#[test]
fn arguments_of_dropped_records_are_not_formatted() {
    struct Explosive;
    impl fmt::Display for Explosive {
        fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
            panic!("a dropped record was formatted");
        }
    }

    let _guard = exclusive_logger();
    trace!("{Explosive}");
}

#[test]
fn a_display_impl_that_logs_does_not_deadlock() {
    struct Chatty(u32);
    impl fmt::Display for Chatty {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            if self.0 > 0 {
                info!("inner {} sees {}", self.0, Chatty(self.0 - 1));
            }
            write!(f, "chatty-{}", self.0)
        }
    }

    let _guard = exclusive_logger();
    info!("outer sees {}", Chatty(2));
    let tail = log::tail(3);
    // Inner records are written first, because they are logged while the
    // outer message is still being formatted.
    assert!(tail[0].ends_with("inner 1 sees chatty-0"), "{tail:?}");
    assert!(tail[1].ends_with("inner 2 sees chatty-1"), "{tail:?}");
    assert!(tail[2].ends_with("outer sees chatty-2"), "{tail:?}");
}

/// A record logged from inside another record's `Display` impl takes a
/// separate path (the message buffer is busy). It must still be formatted,
/// redacted and sent to every sink like any other.
#[test]
fn records_logged_while_formatting_are_redacted_and_reach_every_sink() {
    struct Leaky(&'static str);
    impl fmt::Display for Leaky {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            info!("inner sees {}", self.0);
            write!(f, "leaky")
        }
    }

    let _guard = exclusive_logger();
    let dir = scratch_dir("nested");
    let path = dir.join("tidepool.log");
    log::open_file(&path, FileConfig::default()).expect("open the log file");
    log::set_stderr(true);
    log::__capture_stderr(true);
    let secret = "tp-nested-5e1f";
    log::register_secret(secret);

    info!("outer sees {}", Leaky(secret));

    let captured = log::__take_captured_stderr();
    close_and_flush();
    log::set_stderr(false);
    log::__capture_stderr(false);

    let file = read(&path);
    let ring = log::tail(2).join("\n");
    let expected = ["inner sees [REDACTED]", "outer sees leaky"];
    for (sink, text) in [("file", &file), ("stderr", &captured), ("ring", &ring)] {
        assert_eq!(messages(text), expected, "{sink}:\n{text}");
        for line in text.lines() {
            assert_record_start(line, "INFO");
        }
        assert!(!text.contains(secret), "{sink}:\n{text}");
    }
}

/// Text Tidepool does not control, such as a file name from an untrusted
/// workspace, cannot send escape sequences to the terminal or forge a
/// record in the file.
#[test]
fn control_characters_never_reach_stderr_or_the_file() {
    let _guard = exclusive_logger();
    let dir = scratch_dir("controls");
    let path = dir.join("tidepool.log");
    log::open_file(&path, FileConfig::default()).expect("open the log file");
    log::set_stderr(true);
    log::__capture_stderr(true);

    let name =
        "evil\u{1b}]52;c;aGk=\u{7}\r\u{9b}2J\n2026-01-01T00:00:00.000Z ERROR tidepool: forged";
    warn!("cannot read {name}");
    info!("next");

    let captured = log::__take_captured_stderr();
    close_and_flush();
    log::set_stderr(false);
    log::__capture_stderr(false);

    let file = read(&path);
    for (sink, text) in [("file", &file), ("stderr", &captured)] {
        assert!(
            text.bytes().all(|b| b >= 0x20 || b == b'\n' || b == b'\t'),
            "{sink}: {text:?}"
        );
        assert!(
            !text.chars().any(|c| ('\u{80}'..='\u{9f}').contains(&c)),
            "{sink}: {text:?}"
        );
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3, "{sink}: {lines:?}");
        assert_record_start(lines[0], "WARN");
        assert!(
            lines[0].ends_with(": cannot read evil\\u{1b}]52;c;aGk=\\u{7}\\u{d}\\u{9b}2J"),
            "{sink}: {lines:?}"
        );
        // The continuation line is indented, so it is no record of its own.
        assert_eq!(
            lines[1], "  2026-01-01T00:00:00.000Z ERROR tidepool: forged",
            "{sink}"
        );
        assert_record_start(lines[2], "INFO");
    }
}

#[test]
fn logging_works_from_several_threads() {
    let _guard = exclusive_logger();
    let dir = scratch_dir("threads");
    let path = dir.join("tidepool.log");
    log::open_file(&path, FileConfig::default()).expect("open the log file");
    std::thread::scope(|scope| {
        for thread in 0..4 {
            scope.spawn(move || {
                for n in 0..50 {
                    info!("thread {thread} record {n}");
                }
            });
        }
    });
    close_and_flush();

    let text = read(&path);
    let mut seen = messages(&text);
    assert_eq!(seen.len(), 200, "every record is one intact line");
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), 200, "no record is lost or duplicated");
    assert_eq!(log::dropped_lines(), 0);
}

#[test]
fn stderr_gets_every_line_in_order() {
    let _guard = exclusive_logger();
    log::set_stderr(true);
    log::__capture_stderr(true);
    for n in 0..20 {
        info!("stderr line {n:02}");
    }
    let captured = log::__take_captured_stderr();
    log::set_stderr(false);
    log::__capture_stderr(false);
    let expected: Vec<String> = (0..20).map(|n| format!("stderr line {n:02}")).collect();
    assert_eq!(messages(&captured), expected);
    assert!(captured.ends_with('\n'));
}

#[test]
fn flush_with_a_timeout_succeeds_when_the_writer_keeps_up() {
    let _guard = exclusive_logger();
    let dir = scratch_dir("flush_timeout");
    let path = dir.join("tidepool.log");
    log::open_file(&path, FileConfig::default()).expect("open the log file");
    info!("flushed in time");
    assert!(log::flush_timeout(Duration::from_secs(10)));
    assert_eq!(messages(&read(&path)), ["flushed in time"]);
    close_and_flush();
}

/// Acceptance: a registered secret never appears in any log sink. The
/// secrets are logged raw, through `{:?}`, nested in `Debug` output, at
/// every level, and from another thread; the ring buffer, the file and the
/// stderr sink are all checked.
#[test]
fn a_registered_secret_never_appears_in_any_sink() {
    let _guard = exclusive_logger();
    let dir = scratch_dir("redaction");
    let path = dir.join("tidepool.log");
    log::open_file(&path, FileConfig::default()).expect("open the log file");
    log::set_stderr(true);
    log::__capture_stderr(true);
    log::set_target_level(module_path!(), Some(Level::Trace));

    let password = "tp-pw-7f3a\"q\\uote";
    let api_key = "tp-key-c0ffee-é\u{301}";
    log::register_secret(password);
    log::register_secret(api_key);

    #[derive(Debug)]
    #[expect(dead_code, reason = "the fields are read only by `Debug`")]
    struct Credentials<'a> {
        user: &'a str,
        password: &'a str,
    }

    error!("login failed with {password}");
    warn!("raw key {api_key} and again {api_key}");
    info!("debug form {:?}", password);
    debug!(
        "nested {:?}",
        Credentials {
            user: "jo",
            password
        }
    );
    trace!("pretty {:#?}", (api_key, 1));
    std::thread::spawn(move || info!("from a worker: {api_key}"))
        .join()
        .expect("the worker does not panic");
    info!("a line with no secret");

    let captured = log::__take_captured_stderr();
    close_and_flush();
    log::set_stderr(false);
    log::__capture_stderr(false);
    log::set_target_level(module_path!(), None);

    let file = read(&path);
    let ring = log::tail(log::RING_CAPACITY).join("\n");
    for (sink, text) in [("file", &file), ("stderr", &captured), ("ring", &ring)] {
        for secret in [password, api_key] {
            let debug = format!("{secret:?}");
            let escaped = debug.trim_matches('"');
            assert!(!text.contains(secret), "{sink} has {secret:?}:\n{text}");
            assert!(!text.contains(escaped), "{sink} has {escaped}:\n{text}");
        }
        assert!(
            text.contains("login failed with [REDACTED]"),
            "{sink}:\n{text}"
        );
        assert!(text.contains("password: \"[REDACTED]\""), "{sink}:\n{text}");
        assert!(
            text.contains("from a worker: [REDACTED]"),
            "{sink}:\n{text}"
        );
        assert!(text.contains("a line with no secret"), "{sink}:\n{text}");
    }
    // Seven records, one of them spread over several lines by `{:#?}`.
    assert_eq!(file.matches(" log: ").count(), 7, "{file}");
    assert_eq!(captured.matches(" log: ").count(), 7, "{captured}");
    // `{:#?}` output is indented by two more spaces as continuation lines.
    assert!(file.contains("\n      \"[REDACTED]\",\n"), "{file}");
}

#[test]
fn registering_a_secret_rescrubs_the_ring_buffer() {
    let _guard = exclusive_logger();
    let secret = "tp-late-secret-91b2";
    info!("logged before registration: {secret}");
    assert!(log::tail(1)[0].contains(secret));

    log::register_secret(secret);
    let tail = log::tail(log::RING_CAPACITY);
    assert!(tail.iter().all(|line| !line.contains(secret)), "{tail:?}");
    assert!(
        log::tail(1)[0].ends_with("logged before registration: [REDACTED]"),
        "{tail:?}"
    );
    // Registering again, or registering nothing, changes nothing.
    log::register_secret(secret);
    log::register_secret("");
    info!("after: {secret}");
    assert!(log::tail(1)[0].ends_with("after: [REDACTED]"));
}

/// A token read from a file or a tool's output ends with a newline, and is
/// usually logged trimmed. Neither the trimmed token nor the registered
/// string (completed by the newline ending the record) may reach a sink.
#[test]
fn a_secret_that_ends_with_a_line_break_never_appears_in_any_sink() {
    let _guard = exclusive_logger();
    let dir = scratch_dir("trailing_newline");
    let path = dir.join("tidepool.log");
    log::open_file(&path, FileConfig::default()).expect("open the log file");
    log::set_stderr(true);
    log::__capture_stderr(true);

    let token = "tp-tok-5a5a5a\n";
    log::register_secret(token);
    info!("using token {}", token.trim_end());
    info!("next record");

    let captured = log::__take_captured_stderr();
    close_and_flush();
    log::set_stderr(false);
    log::__capture_stderr(false);

    let file = read(&path);
    let ring = log::tail(2).join("\n");
    for (sink, text) in [("file", &file), ("stderr", &captured), ("ring", &ring)] {
        assert!(!text.contains(token), "{sink}:\n{text}");
        assert!(!text.contains(token.trim_end()), "{sink}:\n{text}");
        assert_eq!(
            messages(text),
            ["using token [REDACTED]", "next record"],
            "{sink}:\n{text}"
        );
    }
}

/// A `{:?}`-formatted password inside an error message that is itself
/// logged with `{:?}`: the doubly escaped password must not reach a sink.
#[test]
fn a_secret_in_nested_debug_output_never_appears_in_any_sink() {
    let _guard = exclusive_logger();
    let dir = scratch_dir("nested_debug");
    let path = dir.join("tidepool.log");
    log::open_file(&path, FileConfig::default()).expect("open the log file");

    let pw = "pa\"ss\\word-tp";
    log::register_secret(pw);
    let err: Result<(), String> = Err(format!("bad password {pw:?}"));
    error!("login: {err:?}");
    close_and_flush();

    let file = read(&path);
    assert_eq!(
        messages(&file),
        [r#"login: Err("bad password \"[REDACTED]\"")"#],
        "{file}"
    );
    assert!(!file.contains("word-tp"), "{file}");
}

/// The ring buffer cuts long lines. A secret registered after such a line
/// was logged, with the cut going through it, must not leave its start
/// behind in the ring.
#[test]
fn registering_a_secret_rescrubs_a_ring_line_cut_through_it() {
    let _guard = exclusive_logger();
    let secret = "sk-live-0123456789abcdef0123456789abcdef";
    // A record from this file starts `<timestamp> INFO  log: `.
    let start = "2026-10-12T19:28:03.123Z INFO  log: ".len();
    let pad = "x".repeat(log::RING_MAX_LINE_BYTES - start - 39);
    info!("{pad}{secret} and more");
    let before = log::tail(1).remove(0);
    assert!(
        before.contains(&secret[..39]),
        "the line is cut in the secret"
    );

    log::register_secret(secret);
    let after = log::tail(1).remove(0);
    let (kept, notice) = after
        .rsplit_once(" …[truncated ")
        .expect("the line is still cut");
    for (end, _) in secret.char_indices().skip(1) {
        assert!(
            !kept.ends_with(&secret[..end]),
            "…{}",
            &kept[kept.len() - 60..]
        );
    }
    assert!(
        kept.ends_with(&format!("x{}", log::REDACTED)),
        "…{}",
        &kept[kept.len() - 60..]
    );
    assert_eq!(notice, "10 bytes]");
}

/// Lines still queued for stderr and the file when a secret is registered
/// are re-scrubbed in the writer's own queue, so they reach neither sink
/// with the secret in them.
#[test]
fn registering_a_secret_rescrubs_lines_queued_for_stderr_and_the_file() {
    let _guard = exclusive_logger();
    let dir = scratch_dir("queued");
    let path = dir.join("tidepool.log");
    log::open_file(&path, FileConfig::default()).expect("open the log file");
    log::set_stderr(true);
    log::__capture_stderr(true);
    assert!(log::flush());

    let secret = "tp-queued-77c1";
    // Held, the writer leaves these lines in its queue.
    log::__hold_writer(true);
    info!("early {secret}");
    info!("also early: {secret:?}");
    log::register_secret(secret);
    info!("late {secret}");
    log::__hold_writer(false);

    let captured = log::__take_captured_stderr();
    close_and_flush();
    log::set_stderr(false);
    log::__capture_stderr(false);

    let file = read(&path);
    let expected = [
        "early [REDACTED]",
        "also early: \"[REDACTED]\"",
        "late [REDACTED]",
    ];
    for (sink, text) in [("file", &file), ("stderr", &captured)] {
        assert_eq!(messages(text), expected, "{sink}:\n{text}");
        assert!(!text.contains(secret), "{sink}:\n{text}");
    }
}

/// Logging renders a record before it takes the logger's lock, so a huge
/// record on one thread does not hold up a small one on another for as
/// long as rendering it takes.
#[test]
fn a_huge_record_does_not_hold_up_other_threads() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;

    let _guard = exclusive_logger();
    // Something to search for, so that redaction has work to do.
    log::register_secret("tp-latency-0d15");
    let huge = "z".repeat(16 * 1024 * 1024);
    let done = AtomicBool::new(false);
    let (slowest_small, fastest_huge) = std::thread::scope(|scope| {
        let small = scope.spawn(|| {
            let mut slowest = Duration::ZERO;
            let mut n = 0u64;
            while !done.load(Ordering::Relaxed) {
                let started = Instant::now();
                info!("small {n}");
                slowest = slowest.max(started.elapsed());
                n += 1;
            }
            slowest
        });
        let mut fastest = Duration::MAX;
        for _ in 0..3 {
            let started = Instant::now();
            info!("{huge}");
            fastest = fastest.min(started.elapsed());
        }
        done.store(true, Ordering::Relaxed);
        let slowest = small.join().expect("the small logger does not panic");
        (slowest, fastest)
    });
    // Holding the lock while rendering made a small record wait for about
    // as long as a huge one takes. Scheduling noise is far below half of
    // that, at least in an unoptimised build, where rendering 16 MiB takes
    // tens of milliseconds; an optimised one may be too fast to tell.
    if fastest_huge >= Duration::from_millis(40) {
        assert!(
            slowest_small < fastest_huge / 2,
            "a small record waited {slowest_small:?}; a huge one takes {fastest_huge:?}"
        );
    }
    assert!(log::tail(1)[0].len() < log::RING_MAX_LINE_BYTES + 64);
}

#[test]
fn a_line_that_cannot_be_made_safe_is_withheld_everywhere() {
    let _guard = exclusive_logger();
    let dir = scratch_dir("withheld");
    let path = dir.join("tidepool.log");
    log::open_file(&path, FileConfig::default()).expect("open the log file");
    // Replacing the first occurrence with "[REDACTED]" would form the
    // secret again across the marker's last two characters. (A secret
    // inside the marker, such as "EDAC", would do the same, but would then
    // withhold every redacted line in the rest of this test binary.)
    log::register_secret("D]Zq9");
    let before = log::withheld_lines();
    info!("D]Zq9Zq9");
    info!("kept");
    close_and_flush();
    assert_eq!(log::withheld_lines(), before + 1);
    assert_eq!(messages(&read(&path)), ["kept"]);
    assert!(log::tail(1)[0].ends_with("kept"));
}

#[test]
fn the_file_rotates_by_size_and_keeps_three_copies() {
    let _guard = exclusive_logger();
    let dir = scratch_dir("rotate_by_size");
    let path = dir.join("tidepool.log");
    let config = FileConfig {
        max_bytes: 400,
        keep: 3,
        rotate_on_open: false,
    };
    log::open_file(&path, config).expect("open the log file");
    for n in 0..100 {
        info!("record {n:03}");
    }
    close_and_flush();

    // The current file plus exactly three rotated copies; older ones are gone.
    assert_eq!(
        file_names(&dir),
        [
            "tidepool.log",
            "tidepool.log.1",
            "tidepool.log.2",
            "tidepool.log.3"
        ]
    );

    // No file exceeds the limit, and every rotated file is as full as it
    // can be: one more line would not have fit.
    let line_len = read(&path)
        .lines()
        .next()
        .expect("the current file has a line")
        .len() as u64
        + 1;
    for name in file_names(&dir) {
        let size = fs::metadata(dir.join(&name))
            .expect("stat a log file")
            .len();
        assert!(size <= config.max_bytes, "{name} is {size} bytes");
        if name != "tidepool.log" {
            assert!(
                size + line_len > config.max_bytes,
                "{name} was rotated early, at {size} bytes"
            );
        }
    }

    // Oldest to newest, the files hold a gapless run of records that ends
    // with the last one written.
    let texts = [
        "tidepool.log.3",
        "tidepool.log.2",
        "tidepool.log.1",
        "tidepool.log",
    ]
    .map(|name| read(&dir.join(name)));
    let kept: Vec<&str> = texts.iter().flat_map(|text| messages(text)).collect();
    let first: usize = kept[0]
        .strip_prefix("record ")
        .expect("record prefix")
        .parse()
        .expect("record number");
    assert!(
        first > 0,
        "with 100 records and 400-byte files, the oldest records must have been dropped"
    );
    let expected: Vec<String> = (first..100).map(|n| format!("record {n:03}")).collect();
    assert_eq!(kept, expected);
}

#[test]
fn opening_appends_by_default() {
    let _guard = exclusive_logger();
    let dir = scratch_dir("append");
    let path = dir.join("tidepool.log");
    fs::write(&path, "from an earlier run\n").expect("seed the log file");

    log::open_file(&path, FileConfig::default()).expect("open the log file");
    info!("from this run");
    close_and_flush();

    let text = read(&path);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], "from an earlier run");
    assert!(lines[1].ends_with("from this run"));
    assert_eq!(file_names(&dir), ["tidepool.log"]);
}

#[test]
fn rotate_on_open_starts_every_run_with_a_fresh_file() {
    let _guard = exclusive_logger();
    let dir = scratch_dir("rotate_on_open");
    let path = dir.join("tidepool.log");
    let config = FileConfig {
        rotate_on_open: true,
        ..FileConfig::default()
    };

    // Five runs, one record each. An empty file is not worth rotating, so
    // the very first open creates no `.1`. Each run ends with a flush, as a
    // real one does, so that its line is in its file before the next open
    // decides whether to rotate.
    for run in 0..5 {
        log::open_file(&path, config).expect("open the log file");
        info!("run {run}");
        close_and_flush();
    }

    assert_eq!(
        file_names(&dir),
        [
            "tidepool.log",
            "tidepool.log.1",
            "tidepool.log.2",
            "tidepool.log.3"
        ]
    );
    assert_eq!(messages(&read(&path)), ["run 4"]);
    assert_eq!(messages(&read(&dir.join("tidepool.log.1"))), ["run 3"]);
    assert_eq!(messages(&read(&dir.join("tidepool.log.2"))), ["run 2"]);
    assert_eq!(messages(&read(&dir.join("tidepool.log.3"))), ["run 1"]);
}

#[test]
fn keeping_no_copies_truncates_instead_of_rotating() {
    let _guard = exclusive_logger();
    let dir = scratch_dir("keep_zero");
    let path = dir.join("tidepool.log");
    log::open_file(
        &path,
        FileConfig {
            max_bytes: 200,
            keep: 0,
            rotate_on_open: false,
        },
    )
    .expect("open the log file");
    for n in 0..20 {
        info!("record {n:03}");
    }
    close_and_flush();

    assert_eq!(file_names(&dir), ["tidepool.log"]);
    let text = read(&path);
    assert!(text.len() <= 200, "{} bytes", text.len());
    assert_eq!(messages(&text).last(), Some(&"record 019"));
}

#[test]
fn an_oversized_record_is_written_whole() {
    let _guard = exclusive_logger();
    let dir = scratch_dir("oversized");
    let path = dir.join("tidepool.log");
    log::open_file(
        &path,
        FileConfig {
            max_bytes: 100,
            keep: 3,
            rotate_on_open: false,
        },
    )
    .expect("open the log file");
    let long = "x".repeat(500);
    info!("{long}");
    info!("after");
    close_and_flush();

    // The long record went into the empty first file without rotating, and
    // the next record rotated it out.
    assert_eq!(file_names(&dir), ["tidepool.log", "tidepool.log.1"]);
    assert_eq!(
        messages(&read(&dir.join("tidepool.log.1"))),
        [long.as_str()]
    );
    assert_eq!(messages(&read(&path)), ["after"]);
}

#[test]
fn a_failed_rotation_keeps_logging_to_the_current_file() {
    let _guard = exclusive_logger();
    let dir = scratch_dir("rotation_fails");
    let path = dir.join("tidepool.log");
    // A non-empty directory where `tidepool.log.1` should go makes the
    // rename fail.
    fs::create_dir(dir.join("tidepool.log.1")).expect("create the obstacle");
    fs::write(dir.join("tidepool.log.1").join("occupied"), "x").expect("fill the obstacle");

    log::open_file(
        &path,
        FileConfig {
            max_bytes: 200,
            keep: 1,
            rotate_on_open: false,
        },
    )
    .expect("open the log file");
    for n in 0..20 {
        info!("record {n:03}");
    }
    close_and_flush();

    // Nothing was lost: every record is in the one file that could be written.
    let text = read(&path);
    let expected: Vec<String> = (0..20).map(|n| format!("record {n:03}")).collect();
    assert_eq!(messages(&text), expected);
}

#[test]
fn opening_a_file_in_a_missing_directory_fails_and_keeps_the_old_sink() {
    let _guard = exclusive_logger();
    let dir = scratch_dir("open_fails");
    let path = dir.join("tidepool.log");
    log::open_file(&path, FileConfig::default()).expect("open the log file");

    let missing = dir.join("no").join("such").join("dir").join("tidepool.log");
    assert!(log::open_file(&missing, FileConfig::default()).is_err());

    info!("still going to the first file");
    close_and_flush();
    assert_eq!(messages(&read(&path)), ["still going to the first file"]);
}
