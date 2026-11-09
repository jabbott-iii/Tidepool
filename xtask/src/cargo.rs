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

//! Running `cargo`, the one outside program `xtask` starts.
//!
//! Ported from nadir xtask `cargo.rs`. Adapted for Tidepool: every child
//! gets `CARGO_NET_OFFLINE=true`, output is captured as bytes for
//! `codec::json`, a child whose output is read gets `CARGO_TERM_COLOR=never`
//! ([`NO_COLOR`]), and the program falls back to `cargo` on the `PATH` also
//! when `CARGO` is set but empty.
//!
//! Commands are argument vectors passed straight to the OS, never a shell
//! string, so no argument is ever re-split or interpreted. Standard input is
//! closed, so a child can never wait for the terminal. A build directory
//! named in the environment is passed on absolute ([`walk::target_dir`]),
//! so a child that runs in another directory uses the same one.
//!
//! The test step's output is passed on to the terminal as it comes, and
//! also read for lines that start with `SKIPPED:`, which a test prints when
//! it cannot run, and for libtest's `test result:` lines, whose counts `site`
//! publishes ([`run_scanning_skips`], [`TestCounts`]). Cargo and the test binaries then
//! write to a pipe rather than the terminal, so they print without colour.
//! `fuzz-smoke`'s `cargo test` is passed on the same way, without colour
//! ([`NO_COLOR`]), with the lines that hold what it asks for kept
//! ([`run_keeping_lines`]).

use std::env;
use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;

use crate::error::Error;
use crate::walk;

/// What a line must start with to report a test that did not run
/// (`docs/04-quality.md`: a gated test that cannot run prints
/// `SKIPPED: <reason>` and is never counted as a pass).
pub(crate) const SKIPPED: &str = "SKIPPED:";

/// The longest line [`SkipScan`] keeps, in bytes; the rest of a longer
/// line is still passed through, but not kept.
const MAX_LINE_BYTES: usize = 4096;

/// The most skip reasons [`SkipScan`] keeps; any more are only counted.
const MAX_SKIP_REASONS: usize = 1000;

/// The most lines [`SkipScan`] keeps for its prefixes; any more are
/// dropped.
const MAX_KEPT_LINES: usize = 1000;

/// What starts the line libtest ends each run of a test binary with, and
/// each crate's doc tests: `test result: ok. 4 passed; 0 failed; 1 ignored;
/// 0 measured; 0 filtered out; finished in 0.01s`.
const TEST_RESULT: &str = "test result: ";

/// What the `test result:` lines a command printed add up to ([`TEST_RESULT`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TestCounts {
    /// How many `test result:` lines there were.
    pub(crate) results: usize,
    /// How many of them did not give a number passed, failed and ignored;
    /// the counts below leave those out.
    pub(crate) unreadable: usize,
    /// The tests that passed: a test that printed `SKIPPED:` and returned is
    /// one of them, as far as libtest knows.
    pub(crate) passed: usize,
    /// The tests that failed.
    pub(crate) failed: usize,
    /// The tests marked `#[ignore]`, which did not run.
    pub(crate) ignored: usize,
}

impl TestCounts {
    /// Adds what another stream, or another step, saw.
    pub(crate) fn add(&mut self, other: Self) {
        self.results += other.results;
        self.unreadable += other.unreadable;
        self.passed += other.passed;
        self.failed += other.failed;
        self.ignored += other.ignored;
    }

    /// Counts the `test result:` line `result`, which starts with
    /// [`TEST_RESULT`]: its numbers passed, failed and ignored, after the
    /// verdict and its `. `, among parts separated by `;`.
    fn read(&mut self, result: &str) {
        self.results += 1;
        let counts = result
            .strip_prefix(TEST_RESULT)
            .and_then(|rest| rest.split_once(". "))
            .and_then(|(_, counts)| {
                let mut found = [None; 3];
                for part in counts.split(';') {
                    let mut words = part.split_whitespace();
                    let (Some(number), Some(word), None) =
                        (words.next(), words.next(), words.next())
                    else {
                        continue;
                    };
                    let slot = match word {
                        "passed" => 0,
                        "failed" => 1,
                        "ignored" => 2,
                        _ => continue,
                    };
                    if found[slot].is_some() {
                        return None;
                    }
                    found[slot] = Some(number.parse::<usize>().ok()?);
                }
                Some((found[0]?, found[1]?, found[2]?))
            });
        match counts {
            Some((passed, failed, ignored)) => {
                self.passed += passed;
                self.failed += failed;
                self.ignored += ignored;
            }
            None => self.unreadable += 1,
        }
    }
}

/// The `SKIPPED:` lines a command printed.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Skips {
    /// The reasons, in the order they were printed: the text after
    /// `SKIPPED:`, trimmed. At most [`MAX_SKIP_REASONS`].
    pub(crate) reasons: Vec<String>,
    /// How many `SKIPPED:` lines there were, kept or not.
    pub(crate) count: usize,
}

impl Skips {
    /// Adds what another stream saw.
    fn merge(&mut self, other: Self) {
        self.count += other.count;
        let room = MAX_SKIP_REASONS.saturating_sub(self.reasons.len());
        self.reasons.extend(other.reasons.into_iter().take(room));
    }
}

/// Finds `SKIPPED:` lines in a stream of bytes that arrives in pieces, and
/// keeps the lines that hold one of `prefixes`, from where it starts.
#[derive(Debug, Default)]
struct SkipScan {
    /// The start of the current line, up to [`MAX_LINE_BYTES`].
    line: Vec<u8>,
    skips: Skips,
    /// What a line to keep holds. libtest can write other text before it
    /// on the same line (`test json ... fuzz-result: …` with one test
    /// thread), so it is looked for anywhere, and the line is kept from it.
    prefixes: &'static [&'static str],
    /// The lines kept, from the prefix on, at most [`MAX_KEPT_LINES`].
    kept: Vec<String>,
    /// What libtest's `test result:` lines say.
    tests: TestCounts,
}

impl SkipScan {
    /// Reads the next piece of the stream.
    fn feed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            if byte == b'\n' {
                self.end_line();
            } else if self.line.len() < MAX_LINE_BYTES {
                self.line.push(byte);
            }
        }
    }

    /// Ends the stream: a last line without a newline still counts.
    fn finish(mut self) -> Skips {
        if !self.line.is_empty() {
            self.end_line();
        }
        self.skips
    }

    fn end_line(&mut self) {
        let line = String::from_utf8_lossy(&self.line);
        if let Some(reason) = skip_reason(&line) {
            self.skips.count += 1;
            if self.skips.reasons.len() < MAX_SKIP_REASONS {
                self.skips.reasons.push(reason);
            }
        }
        let found = self
            .prefixes
            .iter()
            .filter_map(|prefix| line.find(prefix))
            .min();
        if let Some(at) = found
            && self.kept.len() < MAX_KEPT_LINES
        {
            self.kept.push(line[at..].trim_end().to_string());
        }
        // Looked for anywhere, as a kept line is: another write can start
        // the same line.
        if let Some(at) = line.find(TEST_RESULT) {
            self.tests.read(line[at..].trim_end());
        }
        self.line.clear();
    }
}

/// The reason a line gives for a skipped test, if it is a `SKIPPED:` line:
/// one that starts with `SKIPPED:` after any indentation. A line that only
/// mentions the word elsewhere is not one.
fn skip_reason(line: &str) -> Option<String> {
    let reason = line
        .trim_end_matches('\r')
        .trim_start()
        .strip_prefix(SKIPPED)?
        .trim();
    Some(if reason.is_empty() {
        "(no reason given)".to_string()
    } else {
        reason.to_string()
    })
}

/// Copies `from` to `to` as it arrives, and returns the `SKIPPED:` lines
/// in it. If `to` stops accepting output, the rest is still read (so the
/// child never blocks on a full pipe) and scanned.
#[cfg(test)]
fn tee(from: impl Read, to: &mut dyn Write) -> io::Result<Skips> {
    Ok(tee_keeping(from, to, &[])?.0)
}

/// Copies `from` to `to` as it arrives, and returns the `SKIPPED:` lines
/// in it, the lines that hold one of `prefixes`, from where it starts, and
/// what its `test result:` lines count. If `to` stops accepting output, the rest is
/// still read (so the child never blocks on a full pipe) and scanned.
fn tee_keeping(
    mut from: impl Read,
    to: &mut dyn Write,
    prefixes: &'static [&'static str],
) -> io::Result<(Skips, Vec<String>, TestCounts)> {
    let mut scan = SkipScan {
        prefixes,
        ..SkipScan::default()
    };
    let mut forwarding = true;
    let mut buffer = [0_u8; 8192];
    loop {
        let read = match from.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        let piece = buffer.get(..read).unwrap_or(&[]);
        if forwarding {
            forwarding = to.write_all(piece).and_then(|()| to.flush()).is_ok();
        }
        scan.feed(piece);
    }
    // A last line without a newline counts too.
    if !scan.line.is_empty() {
        scan.end_line();
    }
    let kept = std::mem::take(&mut scan.kept);
    let tests = scan.tests;
    Ok((scan.finish(), kept, tests))
}

/// Runs `cargo <args>` in `root` with its output passed on to this
/// process's standard output and error as it comes, as [`run`] does, and
/// also read for `SKIPPED:` lines and libtest's `test result:` lines.
/// Returns whether it succeeded, the `SKIPPED:` lines it found, on either
/// stream, and what the `test result:` lines count.
///
/// # Errors
///
/// [`Error::Spawn`] if cargo cannot be started or its output cannot be
/// read.
pub(crate) fn run_scanning_skips(
    root: &Path,
    args: &[&str],
) -> Result<(bool, Skips, TestCounts), Error> {
    scan_skips(
        command(root, args),
        args,
        &mut io::stdout(),
        &mut io::stderr(),
    )
}

/// Runs `cargo <args>` in `root` with `env` added to its environment and
/// without colour ([`NO_COLOR`], which a CI job's `CARGO_TERM_COLOR=always`
/// would otherwise put into the lines it reads), its output passed on to
/// `out` and `err` as it comes. Returns whether it succeeded, and the
/// lines, from either stream, that hold one of `prefixes`, from where it
/// starts: those of standard output first, in order, then those of
/// standard error.
///
/// # Errors
///
/// [`Error::Spawn`] if cargo cannot be started or its output cannot be
/// read.
pub(crate) fn run_keeping_lines(
    root: &Path,
    args: &[&str],
    env: &[(&str, String)],
    prefixes: &'static [&'static str],
    out: &mut (dyn Write + Send),
    err: &mut (dyn Write + Send),
) -> Result<(bool, Vec<String>), Error> {
    let mut command = command(root, args);
    command.envs(env.iter().map(|(name, value)| (name, value)));
    // Last, so that nothing given or inherited turns colour back on.
    command.env(NO_COLOR.0, NO_COLOR.1);
    let (passed, _, kept, _) = scan_output(command, args, out, err, prefixes)?;
    Ok((passed, kept))
}

/// [`run_scanning_skips`] for `command`, which runs `cargo <args>`, with
/// its output passed on to `out` and `err`.
fn scan_skips(
    command: Command,
    args: &[&str],
    out: &mut (dyn Write + Send),
    err: &mut (dyn Write + Send),
) -> Result<(bool, Skips, TestCounts), Error> {
    let (passed, skips, _, tests) = scan_output(command, args, out, err, &[])?;
    Ok((passed, skips, tests))
}

/// [`scan_skips`], also keeping the lines that hold one of `prefixes`,
/// from where it starts ([`tee_keeping`]).
fn scan_output(
    mut command: Command,
    args: &[&str],
    out: &mut (dyn Write + Send),
    err: &mut (dyn Write + Send),
    prefixes: &'static [&'static str],
) -> Result<(bool, Skips, Vec<String>, TestCounts), Error> {
    let spawn_error = |source| Error::Spawn {
        command: display(args),
        source,
    };
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(spawn_error)?;
    let (stdout, stderr) = (child.stdout.take(), child.stderr.take());
    let (from_stdout, from_stderr) = thread::scope(|scope| {
        let out = scope.spawn(|| match stdout {
            Some(stdout) => tee_keeping(stdout, out, prefixes),
            None => Ok((Skips::default(), Vec::new(), TestCounts::default())),
        });
        let err = scope.spawn(|| match stderr {
            Some(stderr) => tee_keeping(stderr, err, prefixes),
            None => Ok((Skips::default(), Vec::new(), TestCounts::default())),
        });
        type Scanned = io::Result<(Skips, Vec<String>, TestCounts)>;
        let joined = |handle: thread::ScopedJoinHandle<'_, Scanned>| {
            handle
                .join()
                .unwrap_or_else(|_| Err(io::Error::other("the thread reading cargo's output died")))
        };
        (joined(out), joined(err))
    });
    // Wait for the child even when reading failed, so it is not left
    // behind.
    let status = child.wait().map_err(spawn_error)?;
    let (mut skips, mut kept, mut tests) = from_stdout.map_err(spawn_error)?;
    let (more_skips, more_kept, more_tests) = from_stderr.map_err(spawn_error)?;
    skips.merge(more_skips);
    kept.extend(more_kept);
    tests.add(more_tests);
    Ok((status.success(), skips, kept, tests))
}

/// The variable, and its value, that keeps every cargo child off the
/// network: `xtask` never calls the network (`docs/04-quality.md`).
pub(crate) const OFFLINE: (&str, &str) = ("CARGO_NET_OFFLINE", "true");

/// Runs `cargo <args>` in `root`, with its output going straight to this
/// process's standard output and error. Returns whether it succeeded.
///
/// # Errors
///
/// [`Error::Spawn`] if cargo cannot be started.
pub(crate) fn run(root: &Path, args: &[&str]) -> Result<bool, Error> {
    let status = command(root, args)
        .status()
        .map_err(|source| Error::Spawn {
            command: display(args),
            source,
        })?;
    Ok(status.success())
}

/// The variable, and its value, that keeps colour codes out of the output
/// of the cargo children whose output `xtask` reads ([`capture`]). Cargo
/// colours its messages when `CARGO_TERM_COLOR=always` is set (as a CI job
/// may set it) or a cargo configuration file above the repository sets
/// `term.color`, even into a pipe; this variable overrides both. A
/// coloured `error:` line would then start with an escape sequence, and
/// `deps` would no longer tell an out-of-date lockfile from any other
/// failure.
pub(crate) const NO_COLOR: (&str, &str) = ("CARGO_TERM_COLOR", "never");

/// Runs `cargo <args>` in `root` and returns what it wrote to standard
/// output. Its output is never coloured ([`NO_COLOR`]).
///
/// # Errors
///
/// [`Error::Spawn`] if cargo cannot be started, [`Error::CommandFailed`]
/// (with its standard error, less any source lines cargo quotes:
/// [`without_quoted_source`]) if it exits unsuccessfully.
pub(crate) fn capture(root: &Path, args: &[&str]) -> Result<Vec<u8>, Error> {
    let output = capture_command(root, args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|source| Error::Spawn {
            command: display(args),
            source,
        })?;
    if !output.status.success() {
        return Err(Error::CommandFailed {
            command: display(args),
            status: output.status,
            stderr: without_userinfo(&without_quoted_source(&String::from_utf8_lossy(
                &output.stderr,
            ))),
        });
    }
    Ok(output.stdout)
}

/// `text` with the user information of every URL in it (`https://user:token@`)
/// replaced by `…`: a source URL can carry credentials, and secrets never go
/// into messages (`AGENTS.md` rule 7).
pub(crate) fn without_userinfo(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("://") {
        let (before, after) = rest.split_at(at + 3);
        out.push_str(before);
        let authority = after
            .find(|c: char| matches!(c, '/' | '?' | '#' | '"' | '\'' | '`') || c.is_whitespace())
            .unwrap_or(after.len());
        match after[..authority].rfind('@') {
            Some(user) => {
                out.push('…');
                rest = &after[user..];
            }
            None => rest = after,
        }
    }
    out.push_str(rest);
    out
}

/// The line that stands in for each run of lines [`without_quoted_source`]
/// leaves out.
const QUOTED_SOURCE_LEFT_OUT: &str = "  (cargo's quoted source lines are left out)";

/// `stderr`, which a cargo child wrote, without the lines in which cargo
/// quotes a file it could not parse ([`is_quoted_source`]). Each run of
/// them becomes one [`QUOTED_SOURCE_LEFT_OUT`] line; every other line
/// (`error:`, `Caused by:`, `TOML parse error at line 4, column 9`, the
/// message itself) is kept as it is.
///
/// When a cargo configuration file or a manifest has a TOML syntax error,
/// cargo's message quotes the line it is on, and so the value there, which
/// could be a secret such as a registry token. Secrets never go into logs or
/// error messages (AGENTS.md rule 7), and `deps`, `layers` and
/// `unsafe-audit` embed what [`capture`] keeps of cargo's standard error in
/// theirs, so [`capture`] keeps only this. The output of the steps `ci`
/// passes on as it comes ([`run`], [`run_scanning_skips`]) is not changed:
/// it is Rust source diagnostics, and is not captured into any message.
fn without_quoted_source(stderr: &str) -> String {
    let mut kept = String::with_capacity(stderr.len());
    let mut leaving_out = false;
    for line in stderr.split_inclusive('\n') {
        if !is_quoted_source(line) {
            kept.push_str(line);
            leaving_out = false;
        } else if !leaving_out {
            kept.push_str(QUOTED_SOURCE_LEFT_OUT);
            kept.push('\n');
            leaving_out = true;
        }
    }
    kept
}

/// Whether `line` is one in which cargo quotes source: trimmed, it starts
/// with `|` (the gutter, with the marker under the quoted text), or with a
/// line number and then `|` (`4 | token = "..."`).
fn is_quoted_source(line: &str) -> bool {
    let line = line.trim();
    let after_number = line.trim_start_matches(|c: char| c.is_ascii_digit());
    line.starts_with('|')
        || (after_number.len() < line.len() && after_number.trim_start().starts_with('|'))
}

/// The command line as the user would type it, for messages.
pub(crate) fn display(args: &[&str]) -> String {
    let mut line = String::from("cargo");
    for arg in args {
        line.push(' ');
        // A `--manifest-path` is a path from the walk, shown escaped like
        // any other ([`walk::escape_controls`]).
        line.push_str(&walk::escape_controls(arg, |_| false));
    }
    line
}

/// The command [`capture`] runs: [`command`] without colour.
fn capture_command(root: &Path, args: &[&str]) -> Command {
    let mut command = command(root, args);
    command.env(NO_COLOR.0, NO_COLOR.1);
    command
}

/// The command for `cargo <args>` in `root`.
fn command(root: &Path, args: &[&str]) -> Command {
    with_target_dir(base(root, args), walk::target_dir())
}

/// The command for `cargo <args>` in `root`, with nothing taken from the
/// environment.
fn base(root: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(program());
    command
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .env(OFFLINE.0, OFFLINE.1);
    command
}

/// `command` with `CARGO_TARGET_DIR` set to `target_dir`, if there is one:
/// the absolute form of what the environment named. Cargo, and the test
/// binaries it runs, would resolve a relative value against their own
/// directory, which is not the one `xtask` was started in.
fn with_target_dir(mut command: Command, target_dir: Option<PathBuf>) -> Command {
    if let Some(dir) = target_dir {
        command.env("CARGO_TARGET_DIR", dir);
    }
    command
}

/// The cargo that is running this tool, which sets `CARGO` for it. Falls
/// back to whatever `cargo` is on the `PATH`.
fn program() -> OsString {
    env::var_os("CARGO")
        .filter(|program| !program.is_empty())
        .unwrap_or_else(|| OsString::from("cargo"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_is_the_command_line() {
        assert_eq!(
            display(&["fmt", "--all", "--", "--check"]),
            "cargo fmt --all -- --check"
        );
        assert_eq!(display(&[]), "cargo");
        // A path argument is shown escaped, like any path.
        assert_eq!(
            display(&["metadata", "--manifest-path", "a\n::error::x/Cargo.toml"]),
            "cargo metadata --manifest-path a\\n::error::x/Cargo.toml"
        );
    }

    #[test]
    fn every_child_is_offline_with_no_input() {
        // The literal name and value, not the constant: a misspelt variable
        // is one cargo ignores.
        assert_eq!(OFFLINE, ("CARGO_NET_OFFLINE", "true"));
        let command = command(Path::new("."), &["metadata"]);
        let offline: Vec<_> = command
            .get_envs()
            .filter(|(name, _)| *name == "CARGO_NET_OFFLINE")
            .collect();
        assert_eq!(offline.len(), 1);
        assert_eq!(offline[0].1, Some(std::ffi::OsStr::new("true")));
        // Standard input is closed too (`Stdio::null()` in `command`), but
        // `Command` has no getter for it, so no test can see it here, and
        // a child that does not read its input behaves the same either way.
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, ["metadata"]);
        assert_eq!(command.get_current_dir(), Some(Path::new(".")));
    }

    #[test]
    fn output_that_is_read_is_never_coloured() {
        // The literal name and value, as for `OFFLINE`.
        assert_eq!(NO_COLOR, ("CARGO_TERM_COLOR", "never"));
        let colour = |command: &Command| -> Vec<Option<std::ffi::OsString>> {
            command
                .get_envs()
                .filter(|(name, _)| *name == "CARGO_TERM_COLOR")
                .map(|(_, value)| value.map(std::ffi::OsStr::to_os_string))
                .collect()
        };
        assert_eq!(
            colour(&capture_command(Path::new("."), &["metadata"])),
            [Some(std::ffi::OsString::from("never"))]
        );
        // The steps whose output goes to the user keep cargo's own choice.
        assert_eq!(colour(&command(Path::new("."), &["test"])), []);
    }

    #[test]
    fn a_build_directory_from_the_environment_is_passed_on_absolute() {
        let dir = std::env::temp_dir().join("invocation").join("reltarget");
        let command = with_target_dir(base(Path::new("."), &["test"]), Some(dir.clone()));
        let set: Vec<_> = command
            .get_envs()
            .filter(|(name, _)| *name == "CARGO_TARGET_DIR")
            .collect();
        assert_eq!(
            set,
            [(
                std::ffi::OsStr::new("CARGO_TARGET_DIR"),
                Some(dir.as_os_str())
            )]
        );
        assert!(dir.is_absolute());
        // Nothing named, nothing set: cargo's own default applies.
        let command = with_target_dir(base(Path::new("."), &["test"]), None);
        assert!(
            command
                .get_envs()
                .all(|(name, _)| name != "CARGO_TARGET_DIR")
        );
    }

    #[test]
    fn skipped_lines_are_found_and_everything_is_passed_on() {
        let input = "running 3 tests\n\
                     SKIPPED: needs_git: TIDEPOOL_IT_GIT is not set\n\
                     test a ... ok\n\
                     \x20 SKIPPED:   indented, with spaces around   \r\n\
                     note: this is not SKIPPED: it only mentions it\n\
                     skipped: lower case does not count\n\
                     SKIPPED:\n\
                     SKIPPED: the last line has no newline";
        let mut out = Vec::new();
        let skips = tee(input.as_bytes(), &mut out).unwrap();
        assert_eq!(out, input.as_bytes(), "the output is passed on unchanged");
        assert_eq!(
            skips,
            Skips {
                reasons: vec![
                    "needs_git: TIDEPOOL_IT_GIT is not set".to_string(),
                    "indented, with spaces around".to_string(),
                    "(no reason given)".to_string(),
                    "the last line has no newline".to_string(),
                ],
                count: 4,
            }
        );
    }

    #[test]
    fn a_skipped_line_split_across_reads_is_still_one_line() {
        let mut scan = SkipScan::default();
        for piece in ["test x ... ok\nSKI", "PPED: spl", "it\ntest y ... ok\n"] {
            scan.feed(piece.as_bytes());
        }
        let skips = scan.finish();
        assert_eq!(skips.reasons, ["split"]);
        assert_eq!(skips.count, 1);
    }

    #[test]
    fn test_result_lines_are_counted_on_either_stream_and_never_guessed() {
        // As libtest 1.99 writes them: a test binary's, a failing one's, the
        // doc tests', and one after another write on the same line.
        let out = "running 2 tests\n\
                   test a ... ok\n\
                   test result: ok. 4 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; \
                   finished in 0.01s\n\
                   test result: FAILED. 2 passed; 3 failed; 0 ignored; 0 measured; 0 filtered \
                   out; finished in 0.20s\r\n\
                   note: say test result: ok. 7 passed; 0 failed; 2 ignored; 0 measured\n\
                   test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; \
                   finished in 0.00s";
        let mut scanned = Vec::new();
        let (_, _, tests) = tee_keeping(out.as_bytes(), &mut scanned, &[]).unwrap();
        assert_eq!(scanned, out.as_bytes(), "the output is passed on unchanged");
        assert_eq!(
            tests,
            TestCounts {
                results: 4,
                unreadable: 0,
                passed: 13,
                failed: 3,
                ignored: 3,
            }
        );
        // A line whose numbers cannot all be read is counted as unreadable,
        // and adds nothing.
        let odd = "test result: ok. many passed; 0 failed; 0 ignored\n\
                   test result: ok. 1 passed; 0 failed\n\
                   test result: ok. 1 passed; 1 passed; 0 failed; 0 ignored\n\
                   test result: ok\n";
        let (_, _, mut tests) = tee_keeping(odd.as_bytes(), &mut Vec::new(), &[]).unwrap();
        assert_eq!(
            tests,
            TestCounts {
                results: 4,
                unreadable: 4,
                ..TestCounts::default()
            }
        );
        // The two streams' counts add up.
        tests.add(TestCounts {
            results: 1,
            unreadable: 0,
            passed: 2,
            failed: 0,
            ignored: 1,
        });
        assert_eq!((tests.results, tests.passed, tests.ignored), (5, 2, 1));
        // The words are looked for anywhere on a line, so a line that only
        // mentions them is one that cannot be read: `site` then refuses to
        // publish counts, rather than guess.
        let (_, _, tests) =
            tee_keeping(&b"the test result: was fine\n"[..], &mut Vec::new(), &[]).unwrap();
        assert_eq!((tests.results, tests.unreadable), (1, 1));
        let (_, _, tests) =
            tee_keeping(&b"test results: none\n"[..], &mut Vec::new(), &[]).unwrap();
        assert_eq!(tests, TestCounts::default());
    }

    #[test]
    fn scanning_is_bounded() {
        // A very long line is kept only up to the bound.
        let mut scan = SkipScan::default();
        scan.feed(b"SKIPPED: ");
        scan.feed(&vec![b'x'; 3 * MAX_LINE_BYTES]);
        scan.feed(b"\n");
        let skips = scan.finish();
        assert_eq!(skips.count, 1);
        assert_eq!(skips.reasons[0].len(), MAX_LINE_BYTES - "SKIPPED: ".len());
        // Past the bound, reasons are counted but not kept.
        let mut scan = SkipScan::default();
        for _ in 0..MAX_SKIP_REASONS + 5 {
            scan.feed(b"SKIPPED: again\n");
        }
        let mut skips = scan.finish();
        assert_eq!(skips.count, MAX_SKIP_REASONS + 5);
        assert_eq!(skips.reasons.len(), MAX_SKIP_REASONS);
        skips.merge(Skips {
            reasons: vec!["more".to_string()],
            count: 1,
        });
        assert_eq!(skips.count, MAX_SKIP_REASONS + 6);
        assert_eq!(skips.reasons.len(), MAX_SKIP_REASONS);
        // Merging counts what the other stream saw, kept or not.
        let mut skips = Skips {
            reasons: vec!["a".to_string()],
            count: 1,
        };
        skips.merge(Skips {
            reasons: vec!["b".to_string()],
            count: 3,
        });
        assert_eq!(
            skips,
            Skips {
                reasons: vec!["a".to_string(), "b".to_string()],
                count: 4,
            }
        );
    }

    #[test]
    fn lines_with_a_prefix_are_kept_trimmed_and_bounded() {
        let input = "running 1 test\n  fuzz-result: target=json \r\ntest json ... fuzz-result: x\n\
                     test result: ok.\nfuzz-result: split";
        let mut out = Vec::new();
        let (skips, kept, _) = tee_keeping(
            input.as_bytes(),
            &mut out,
            &["fuzz-result:", "test result:"],
        )
        .unwrap();
        assert_eq!(out, input.as_bytes(), "the output is passed on unchanged");
        assert_eq!(skips, Skips::default());
        assert_eq!(
            kept,
            [
                "fuzz-result: target=json",
                "fuzz-result: x",
                "test result: ok.",
                "fuzz-result: split"
            ]
        );
        let mut scan = SkipScan {
            prefixes: &["x"],
            ..SkipScan::default()
        };
        for _ in 0..MAX_KEPT_LINES + 5 {
            scan.feed(b"x\n");
        }
        assert_eq!(scan.kept.len(), MAX_KEPT_LINES);
        // Without prefixes, nothing is kept.
        let (_, kept, _) = tee_keeping(&b"x\n"[..], &mut Vec::new(), &[]).unwrap();
        assert!(kept.is_empty());
    }

    #[test]
    fn a_command_with_lines_kept_gets_its_environment() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut command = command(root, &["--version"]);
        command.envs([("XTASK_TEST_VARIABLE", "1")]);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let (passed, _, kept, _) =
            scan_output(command, &["--version"], &mut out, &mut err, &["cargo "])
                .expect("cargo runs");
        assert!(passed);
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert!(kept[0].starts_with("cargo 1."), "{kept:?}");
    }

    #[test]
    fn output_nobody_reads_is_still_scanned() {
        struct Closed;
        impl Write for Closed {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::from(io::ErrorKind::BrokenPipe))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let skips = tee(&b"a\nSKIPPED: b\n"[..], &mut Closed).unwrap();
        assert_eq!(skips.count, 1);
    }

    #[test]
    fn a_scanned_command_passes_its_output_on_and_reports_success() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let scanned = |args: &[&str]| {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let (passed, skips, tests) =
                scan_skips(command(root, args), args, &mut out, &mut err).expect("cargo runs");
            assert_eq!(skips, Skips::default());
            assert_eq!(tests, TestCounts::default());
            (
                passed,
                String::from_utf8(out).unwrap(),
                String::from_utf8(err).unwrap(),
            )
        };
        let (passed, out, _) = scanned(&["--version"]);
        assert!(passed);
        assert!(out.starts_with("cargo "), "{out}");
        let (passed, _, err) = scanned(&["no-such-cargo-subcommand-for-xtask"]);
        assert!(!passed);
        assert!(err.contains("no-such-cargo-subcommand-for-xtask"), "{err}");
    }

    #[test]
    fn cargo_s_quoted_source_lines_are_left_out() {
        // What cargo 1.99 writes for an unterminated string in a cargo
        // configuration file, and in a manifest.
        assert_eq!(
            without_quoted_source(
                "error: could not load Cargo configuration\n\nCaused by:\n  could not parse \
                 TOML configuration in `/w/.cargo/config.toml`\n\nCaused by:\n  TOML parse error \
                 at line 4, column 9\n    |\n  4 | token = \"FAKETOKEN_x\n    |         ^\n  \
                 invalid basic string, expected `\"`\n"
            ),
            "error: could not load Cargo configuration\n\nCaused by:\n  could not parse TOML \
             configuration in `/w/.cargo/config.toml`\n\nCaused by:\n  TOML parse error at line \
             4, column 9\n  (cargo's quoted source lines are left out)\n  invalid basic string, \
             expected `\"`\n"
        );
        assert_eq!(
            without_quoted_source(
                "error: invalid basic string, expected `\"`\n --> Cargo.toml:6:16\n  |\n6 | \
                 edition = \"FAKETOKEN_y\n  |                ^\n"
            ),
            "error: invalid basic string, expected `\"`\n --> Cargo.toml:6:16\n  (cargo's quoted \
             source lines are left out)\n"
        );
        // A span over several lines, Windows line ends, and two runs: each
        // run is one line, and every other line is kept as it is.
        assert_eq!(
            without_quoted_source(
                "error: first\r\n   |\r\n12 | / a = \"\"\"\r\n13 | | FAKETOKEN_z\r\n   | |___^\r\n\
                 between\n10|x\n||\n"
            ),
            "error: first\r\n  (cargo's quoted source lines are left out)\nbetween\n  (cargo's \
             quoted source lines are left out)\n"
        );
        // A bar elsewhere, or a number that no bar follows, is not quoted
        // source.
        for kept in [
            "warning: a | b\n",
            "error: 12 crates | not a gutter\n",
            "  = note: 4 | 5\n",
            "4\n",
            "\n",
            "",
            "no newline at the end",
        ] {
            assert_eq!(without_quoted_source(kept), kept, "{kept:?}");
        }
    }

    #[test]
    fn a_failing_command_never_shows_the_source_cargo_quotes() {
        // A manifest with a TOML syntax error on a line that holds a
        // (fake) secret: the error keeps cargo's message, not the line.
        let tree = crate::walk::testing::TempTree::new("cargo-quoted-source");
        tree.write(
            "Cargo.toml",
            "[package]\nname = \"p\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
             [package.metadata]\ntoken = \"FAKETOKEN_unterminated\n",
        );
        tree.write("src/lib.rs", "");
        let manifest = tree.path().join("Cargo.toml");
        let manifest = manifest.to_string_lossy();
        let error = capture(
            tree.path(),
            &[
                "metadata",
                "--format-version",
                "1",
                "--offline",
                "--no-deps",
                "--manifest-path",
                &manifest,
            ],
        )
        .expect_err("a manifest that is not TOML fails");
        let shown = error.to_string();
        assert!(!shown.contains("FAKETOKEN"), "{shown}");
        assert!(!format!("{error:?}").contains("FAKETOKEN"), "{error:?}");
        assert!(shown.contains(QUOTED_SOURCE_LEFT_OUT), "{shown}");
        assert!(shown.contains("error: "), "{shown}");
    }

    #[test]
    fn a_failing_command_never_shows_the_credentials_of_a_url() {
        // Cargo names the URL of a git dependency it cannot fetch offline,
        // credentials and all; the error keeps the URL without them.
        let tree = crate::walk::testing::TempTree::new("cargo-url-credentials");
        tree.write(
            "Cargo.toml",
            "[package]\nname = \"p\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
             [workspace]\n\n[dependencies]\nleft = { git = \"https://FAKETOKEN@example.invalid/x\" }\n",
        );
        tree.write("src/lib.rs", "");
        let manifest = tree.path().join("Cargo.toml");
        let manifest = manifest.to_string_lossy();
        let error = capture(
            tree.path(),
            &[
                "metadata",
                "--format-version",
                "1",
                "--offline",
                "--manifest-path",
                &manifest,
            ],
        )
        .expect_err("a git dependency cannot be fetched offline");
        let shown = error.to_string();
        assert!(!shown.contains("FAKETOKEN"), "{shown}");
        assert!(!format!("{error:?}").contains("FAKETOKEN"), "{error:?}");
        assert!(shown.contains("https://…@example.invalid/x"), "{shown}");
    }

    #[test]
    fn a_failing_command_reports_its_status_and_stderr() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let error = capture(root, &["no-such-cargo-subcommand-for-xtask"])
            .expect_err("an unknown subcommand fails");
        match error {
            Error::CommandFailed {
                command, stderr, ..
            } => {
                assert_eq!(command, "cargo no-such-cargo-subcommand-for-xtask");
                assert!(
                    stderr.contains("no-such-cargo-subcommand-for-xtask"),
                    "{stderr}"
                );
            }
            other => panic!("expected a failed command, got {other:?}"),
        }
    }
}
