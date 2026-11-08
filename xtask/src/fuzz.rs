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

//! `cargo xtask fuzz-smoke [--secs N] [--target T] [--seed S] [--replay]`:
//! runs each fuzz target with mutated inputs for a while, and fails if one
//! of them panics, hangs, over-allocates, or dies.
//!
//! Ported from Nadir's `xtask/src/fuzz.rs` (the table of targets, the
//! command line, a line for each target and the verdict). Adapted for
//! Tidepool: the targets are tests in their own crates, driven by
//! `tidepool_core::testing::fuzz` (ADR-0027), so for each one this runs
//! `cargo test` of its crate's `tests/fuzz.rs`, filtered to the target, in
//! the `fuzz` profile, and reads the result line the test prints (wherever
//! on its line it starts); the table and `fuzz/`, `fuzz/corpus/` and
//! `fuzz/crashes/` must agree, without links; the seed comes from the clock
//! unless `--seed` gives one, and a result line must show the seed and the
//! seconds it was given; a target whose test process died, or that names a
//! range of inputs, is reported with the command that replays it, and a
//! replay that dies again has the input it died on kept as `died-<hash>`;
//! and a cargo failure before the test ran is told apart from a death.
//!
//! Its children are `cargo test`, one for each target, under ADR-0017's
//! rules as ADR-0027 extends them: an argument vector, the root as the
//! directory, standard input closed, `CARGO_NET_OFFLINE=true`, and
//! `CARGO_TERM_COLOR=never`, since it reads their lines. The parsers run in
//! those test processes, never in this one. Their output is passed on as it
//! comes, as `ci`'s test step's is. The one file it changes is the
//! `.last-input` of a replay it saw die or exit without a result, which it
//! renames, or removes when it is already a saved crash ([`keep_died`]):
//! only if the replay wrote it, and not after a signal that stopped the
//! test (`SIGINT`, `SIGTERM`, `SIGHUP`). It reads a leftover one before a
//! replay, to note its size and hash. Run one replay of a target at a
//! time.

use std::fmt::Write as _;
use std::fs;
use std::io::Read as _;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tidepool_core::CoreError;
use tidepool_core::cli::Args;
use tidepool_core::hash::fnv1a64;
use tidepool_core::testing::fuzz::{
    DIED_PREFIX, Kept, LAST_INPUT, MAX_SEED_BYTES, REPLAY_VAR, RESULT_PREFIX, Reading, SECS_VAR,
    SEED_VAR, Stop, Summary, keep_died,
};

use crate::cargo;
use crate::check;
use crate::error::Error;
use crate::walk;

/// A fuzz target: a test of that name in its package's `tests/fuzz.rs`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Target {
    /// The name for `--target`, of the test, and of its directories in
    /// `fuzz/corpus/` and `fuzz/crashes/`.
    pub(crate) name: &'static str,
    /// The package whose `tests/fuzz.rs` has the test.
    pub(crate) package: &'static str,
}

/// Every fuzz target. A new parser adds a test to its crate's
/// `tests/fuzz.rs`, seeds in `fuzz/corpus/<name>/`, and a row here
/// (ADR-0027, `.claude/rules/parsers.md`).
pub(crate) const TARGETS: &[Target] = &[
    Target {
        name: "json",
        package: "tidepool-codec",
    },
    Target {
        name: "toml",
        package: "tidepool-codec",
    },
    Target {
        name: "encoding",
        package: "tidepool-text",
    },
];

/// How long each target is fuzzed when `--secs` is not given.
pub(crate) const DEFAULT_SECS: u64 = 60;

/// The cargo profile the targets are built in (the root `Cargo.toml`).
pub(crate) const PROFILE: &str = "fuzz";

/// The integration test every target is in: `tests/fuzz.rs`.
const TEST: &str = "fuzz";

/// Where the seeds are, one directory a target, relative to the root.
const CORPUS: &str = "fuzz/corpus";

/// Where the saved crashes are, one directory a target.
const CRASHES: &str = "fuzz/crashes";

/// What a line of a target's output must hold to be kept, from there on:
/// the result line the test prints (with one test thread, libtest writes
/// `test json ... ` before it on the same line), libtest's verdict, cargo's
/// report of a test process that did not exit successfully, its line after
/// a test binary that failed or could not be started, its report of one
/// it could not start (indented, under `Caused by:`), and its report of a
/// build that failed.
const KEPT: &[&str] = &[
    RESULT_PREFIX,
    "test result:",
    "process didn't exit successfully:",
    "error: test failed",
    "could not execute process",
    "error: could not compile",
];

/// What cargo's report of a test process that ended from a signal holds
/// when the signal asked it to stop, rather than killed it for what it did:
/// an interrupt, a termination, a hang-up. Such an ending is not a death to
/// keep the input of.
const STOPPED_BY: &[&str] = &["SIGINT", "SIGTERM", "SIGHUP"];

/// What the command line asked for.
#[derive(Debug)]
pub(crate) struct Options<'a> {
    /// The targets to run, in table order.
    pub(crate) targets: Vec<&'a Target>,
    /// How long to fuzz each target for.
    pub(crate) secs: u64,
    /// The seed of every target's mutations.
    pub(crate) seed: u64,
    /// Whether the seed came from the clock rather than `--seed`.
    pub(crate) seed_from_clock: bool,
    /// Whether each input is saved before it runs (`--replay`).
    pub(crate) replay: bool,
}

impl<'a> Options<'a> {
    /// Takes `[--secs N] [--target NAME] [--seed S] [--replay]` from
    /// `args`, choosing among `table`. Without `--seed`, the seed is
    /// `clock()`.
    ///
    /// # Errors
    ///
    /// [`Error::Usage`] for a value that does not parse or an unknown
    /// target.
    pub(crate) fn parse(
        args: &mut Args,
        table: &'a [Target],
        clock: impl FnOnce() -> u64,
    ) -> Result<Self, Error> {
        let usage = |error: CoreError| Error::Usage(error.to_string());
        let secs: Option<u64> = args.parsed("--secs").map_err(usage)?;
        let name = args.value("--target").map_err(usage)?;
        let seed = args.value("--seed").map_err(usage)?;
        let replay = args.flag("--replay");
        let targets = match &name {
            None => table.iter().collect(),
            Some(name) => match table.iter().find(|target| target.name == name) {
                Some(target) => vec![target],
                None => {
                    let known: Vec<&str> = table.iter().map(|target| target.name).collect();
                    return Err(Error::Usage(format!(
                        "unknown fuzz target `{name}`; the targets are: {}",
                        known.join(", ")
                    )));
                }
            },
        };
        let (seed, seed_from_clock) = match seed {
            None => (clock(), true),
            Some(text) => match parse_seed(&text) {
                Some(seed) => (seed, false),
                None => {
                    return Err(Error::Usage(format!(
                        "option `--seed` needs a u64, in decimal or as 0x-prefixed hex, not \
                         `{text}`"
                    )));
                }
            },
        };
        Ok(Self {
            targets,
            secs: secs.unwrap_or(DEFAULT_SECS),
            seed,
            seed_from_clock,
            replay,
        })
    }
}

/// A seed from the clock, for a run that was not given one: the
/// nanoseconds since 1970, mixed. Printed, so that the run can be replayed.
pub(crate) fn clock_seed() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    fnv1a64(&nanos.to_le_bytes())
}

/// Reads a seed as `tidepool_core::testing` does: decimal, or hex with a
/// `0x` prefix.
fn parse_seed(text: &str) -> Option<u64> {
    let text = text.trim();
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

/// Runs `fuzz-smoke` on the repository at `root`, the targets of `table`
/// that `options` chose. Returns whether every one ran clean.
///
/// # Errors
///
/// [`Error::Output`] if the output cannot be written, [`Error::Read`] if
/// `fuzz/` cannot be listed, [`Error::Spawn`] if cargo cannot be started.
pub(crate) fn run(
    options: &Options<'_>,
    table: &[Target],
    root: &Path,
    out: &mut dyn Write,
) -> Result<bool, Error> {
    let children = Children {
        out: &mut io::stdout(),
        err: &mut io::stderr(),
        env: &[],
    };
    run_with(options, table, root, out, children)
}

/// Where the children's output goes, and what is added to their
/// environment: this process's standard output and error and nothing, but
/// for the tests, which give them buffers and a build directory of their
/// own.
struct Children<'a> {
    out: &'a mut (dyn Write + Send),
    err: &'a mut (dyn Write + Send),
    env: &'a [(&'a str, String)],
}

/// [`run`], with the children's output and environment as `children` says.
fn run_with(
    options: &Options<'_>,
    table: &[Target],
    root: &Path,
    out: &mut dyn Write,
    children: Children<'_>,
) -> Result<bool, Error> {
    let output = Error::Output;
    let violations = check_table(root, table)?;
    if !violations.is_empty() {
        for violation in &violations {
            writeln!(out, "violation: {}", check::printable(violation)).map_err(output)?;
        }
        writeln!(
            out,
            "fuzz-smoke: FAILED ({}); nothing was run",
            plural(violations.len(), "violation")
        )
        .map_err(output)?;
        return Ok(false);
    }
    let origin = if options.seed_from_clock {
        format!(
            "from the clock; pass `--seed 0x{:016x}` to run the same inputs again",
            options.seed
        )
    } else {
        "from --seed".to_string()
    };
    writeln!(
        out,
        "fuzz-smoke: {}, {} s each{}, seed 0x{:016x} ({origin})",
        plural(options.targets.len(), "target"),
        options.secs,
        if options.replay {
            ", saving each input before it runs"
        } else {
            ""
        },
        options.seed
    )
    .map_err(output)?;

    let mut results = Vec::with_capacity(options.targets.len());
    for target in &options.targets {
        let args = test_args(target);
        let mut env = vec![
            (SECS_VAR, options.secs.to_string()),
            (SEED_VAR, format!("0x{:016x}", options.seed)),
            (
                REPLAY_VAR,
                if options.replay { "1" } else { "0" }.to_string(),
            ),
        ];
        writeln!(out, "== {}: {}", target.name, shown_command(&env, &args)).map_err(output)?;
        let last = root.join(CRASHES).join(target.name).join(LAST_INPUT);
        // A replay's input is kept only if the replay wrote the file.
        let before = written_at(&last);
        if options.replay && fs::symlink_metadata(&last).is_ok() {
            writeln!(out, "{}", leftover_note(root, &last)).map_err(output)?;
        }
        env.extend(children.env.iter().cloned());
        // The cargo command writes to the terminal directly, so this line
        // has to be out before it starts.
        out.flush().map_err(output)?;
        let (passed, lines) =
            cargo::run_keeping_lines(root, &args, &env, KEPT, children.out, children.err)?;
        let verdict = classify(target.name, options, passed, &lines);
        let mut text = describe(target, options, &verdict);
        if let (true, Some(signal)) = (options.replay, stopped_by(&verdict)) {
            let _ = write!(
                text,
                "\n  It was stopped by {signal}, not killed for what it did, so its \
                 {CRASHES}/{}/{LAST_INPUT} is not kept; the next replay runs it first.",
                target.name
            );
        } else if options.replay
            && matches!(verdict, Verdict::Died(_) | Verdict::ExitedWithoutResult)
        {
            text.push_str(&keep(root, target.name, before));
        }
        writeln!(out, "{text}").map_err(output)?;
        out.flush().map_err(output)?;
        results.push((target.name, verdict));
    }
    out.write_all(summary(options, &results).as_bytes())
        .map_err(output)?;
    out.flush().map_err(output)?;
    Ok(results
        .iter()
        .all(|(_, verdict)| matches!(verdict, Verdict::Clean(_))))
}

/// After a replay of `name` that died, or exited without a result: keeps
/// what it ran last, the input it died on, where every run reads it, if
/// the replay wrote it (`before` is when the file was written before the
/// replay started). Returns what to add to the target's line.
fn keep(root: &Path, name: &str, before: Option<SystemTime>) -> String {
    let last = root.join(CRASHES).join(name).join(LAST_INPUT);
    let after = written_at(&last);
    if after.is_some() && !written_since(before, after) {
        // The replay rewrites the file before its first input.
        return format!(
            "\n  Its {CRASHES}/{name}/{LAST_INPUT} was not written by this replay, which so \
             ended before its first input: it is not kept, and the next replay runs it first."
        );
    }
    match keep_died(root, name) {
        Ok(Some(Kept::New(kept))) => format!(
            "\n  The input it died on is kept as {}, where it runs with the saved crashes from \
             now on.",
            walk::relative(root, &kept)
        ),
        Ok(Some(Kept::Existing(kept))) => format!(
            "\n  It died on the saved crash {}, which every run replays first: fix what it \
             shows, or remove it.",
            walk::relative(root, &kept)
        ),
        Ok(None) => format!("\n  No {CRASHES}/{name}/{LAST_INPUT} was left to keep."),
        Err(error) => format!(
            "\n  The input it died on could not be kept: {}",
            check::printable(&error.to_string())
        ),
    }
}

/// When the file at `path` was last written, if it is one, not a link: the
/// epoch if the system does not say.
fn written_at(path: &Path) -> Option<SystemTime> {
    let metadata = fs::symlink_metadata(path).ok()?;
    metadata
        .is_file()
        .then(|| metadata.modified().unwrap_or(UNIX_EPOCH))
}

/// Whether a file that was written at `before` (or was not there) has been
/// written since, now that it was written at `after` (or is not there).
/// Any change counts, not only a later time: a replay always makes the file
/// anew before its first input, and a leftover's time may be in the future
/// (a clock set back, a copy that kept its times).
fn written_since(before: Option<SystemTime>, after: Option<SystemTime>) -> bool {
    match (before, after) {
        (_, None) => false,
        (None, Some(_)) => true,
        (Some(before), Some(after)) => after != before,
    }
}

/// The note before a replay that an earlier one left `last`: its size and
/// hash, and how to keep it by hand, for when that replay's death took
/// `fuzz-smoke` with it.
fn leftover_note(root: &Path, last: &Path) -> String {
    let shown = walk::relative(root, last);
    let mut bytes = Vec::new();
    let read = match fs::symlink_metadata(last) {
        Ok(metadata) if metadata.is_file() => fs::File::open(last)
            .and_then(|file| file.take(MAX_SEED_BYTES + 1).read_to_end(&mut bytes))
            .is_ok(),
        _ => false,
    };
    if !read || bytes.len() as u64 > MAX_SEED_BYTES {
        return format!("note: {shown}, left by an earlier replay, is there");
    }
    let hash = fnv1a64(&bytes);
    let kept = last.with_file_name(format!("{DIED_PREFIX}-{hash:016x}"));
    format!(
        "note: {shown} ({}, hash {hash:016x}), left by an earlier replay, runs first. If that \
         replay ended with fuzz-smoke itself killed (systemd stops every process of the unit \
         after an out-of-memory kill unless OOMPolicy=continue, and systemd-oomd kills them all), \
         it is likely the input it died on; to keep it:\n  mv {shown} {}",
        plural(bytes.len(), "byte"),
        walk::relative(root, &kept)
    )
}

/// The signal that stopped a test process that died, if it was one that
/// asks a process to stop ([`STOPPED_BY`]) rather than one that kills it
/// for what it did.
fn stopped_by(verdict: &Verdict) -> Option<&'static str> {
    match verdict {
        Verdict::Died(Some(how)) => STOPPED_BY
            .iter()
            .find(|signal| {
                how.contains(&format!("{signal}:")) || how.contains(&format!("{signal})"))
            })
            .copied(),
        _ => None,
    }
}

/// The arguments of the `cargo test` that runs `target`.
fn test_args(target: &Target) -> Vec<&'static str> {
    vec![
        "test",
        "--profile",
        PROFILE,
        "-p",
        target.package,
        "--test",
        TEST,
        "--",
        "--exact",
        target.name,
        "--nocapture",
    ]
}

/// The command line a person would type to run what a target's child
/// runs: its variables, then cargo's arguments.
fn shown_command(env: &[(&str, String)], args: &[&str]) -> String {
    let mut line = String::new();
    for (name, value) in env {
        let _ = write!(line, "{name}={value} ");
    }
    line.push_str(&cargo::display(args));
    line
}

/// How a target's run ended.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Verdict {
    /// It ran as it was asked, and found nothing.
    Clean(Summary),
    /// It ran, and found something: the test's report is above.
    Found(Summary),
    /// It reported, but its test failed anyway, or its line does not match
    /// what it was asked; or it failed before it could report.
    Failed(Option<Summary>, String),
    /// Its test passed without reporting: no test of that name ran.
    NotRun,
    /// The build failed.
    NotBuilt,
    /// Cargo failed before the test binary ran.
    CargoFailed,
    /// The test process ran and ended without a result: it died. How it
    /// ended, as cargo put it, if it said.
    Died(Option<String>),
    /// The test binary ran, and exited with libtest's failure status (101)
    /// but no result: an `exit(101)`, or libtest failing on its own.
    ExitedWithoutResult,
}

/// Reads what a target's child printed: whether it `passed`, and the
/// [`KEPT`] lines. Text that came from the child is escaped where a
/// message quotes it.
fn classify(name: &str, options: &Options<'_>, passed: bool, lines: &[String]) -> Verdict {
    let summaries: Vec<Summary> = lines
        .iter()
        .filter_map(|line| Summary::parse(line))
        .collect();
    let has = |prefix: &str| lines.iter().any(|line| line.starts_with(prefix));
    match summaries.as_slice() {
        [summary] if summary.target != name => Verdict::Failed(
            Some(summary.clone()),
            format!(
                "its test reported for the target `{}`",
                check::printable(&summary.target)
            ),
        ),
        [summary] if summary.seed != options.seed => Verdict::Failed(
            Some(summary.clone()),
            format!(
                "its test used the seed 0x{:016x}, not the 0x{:016x} it was given",
                summary.seed, options.seed
            ),
        ),
        [summary] if summary.findings > 0 || summary.hung => Verdict::Found(summary.clone()),
        [summary] if !options.replay && summary.elapsed < Duration::from_secs(options.secs) => {
            Verdict::Failed(
                Some(summary.clone()),
                format!(
                    "its test ran for {:.1} s, not the {} s it was given",
                    summary.elapsed.as_secs_f64(),
                    options.secs
                ),
            )
        }
        [summary] if passed => Verdict::Clean(summary.clone()),
        [summary] => Verdict::Failed(
            Some(summary.clone()),
            "its test failed after reporting no findings".to_string(),
        ),
        [] if has("error: could not compile") => Verdict::NotBuilt,
        // A test binary, or its runner, that cargo could not start: it
        // never ran, and cargo then says the test failed too.
        [] if has("could not execute process") => Verdict::CargoFailed,
        [] if passed => Verdict::NotRun,
        [] if has("test result:") => Verdict::Failed(
            None,
            "its test failed before it could report (see its output above)".to_string(),
        ),
        [] => {
            // Cargo names the test binary's command line when it ended from
            // a signal or with a status other than libtest's 101. For 101 it
            // says only that the test failed. Any other failure is cargo's
            // own, before the test ran.
            let marker = format!(" --exact {name} ");
            let died = lines.iter().find(|line| {
                line.starts_with("process didn't exit successfully:") && line.contains(&marker)
            });
            match died {
                Some(line) => Verdict::Died(
                    line.rfind(" (")
                        .map(|at| check::printable(&line[at + 1..]).into_owned()),
                ),
                None if has("error: test failed") => Verdict::ExitedWithoutResult,
                None => Verdict::CargoFailed,
            }
        }
        [..] => Verdict::Failed(None, format!("its test reported {} times", summaries.len())),
    }
}

/// The command that runs `name` again with these options, `--replay` added
/// if `replay`.
fn again(name: &str, options: &Options<'_>, replay: bool) -> String {
    format!(
        "cargo xtask fuzz-smoke --target {name} --seed 0x{:016x} --secs {}{}",
        options.seed,
        options.secs,
        if replay { " --replay" } else { "" }
    )
}

/// The line that reports one target.
fn describe(target: &Target, options: &Options<'_>, verdict: &Verdict) -> String {
    let name = target.name;
    match verdict {
        Verdict::Clean(summary) if options.replay => {
            let stopped =
                Stop::describe(&summary.stopped).unwrap_or("for a reason it did not give");
            format!(
                "{name}: ok: {}\n  The replay found nothing in {}: it stopped because \
                 {stopped}. It may not have reached the input the run it replays failed on; \
                 try more --secs.",
                numbers(summary),
                plural_u64(summary.inputs, "input")
            )
        }
        Verdict::Clean(summary) => format!("{name}: ok: {}", numbers(summary)),
        Verdict::Found(summary) => {
            let mut text = format!(
                "{name}: FAILED: {} (the report is above): {}",
                plural(summary.findings, "finding"),
                numbers(summary)
            );
            if summary.saved > 0 {
                let _ = write!(
                    text,
                    "\n  The saved inputs are in {CRASHES}/{name}/. Where they are lost, as on \
                     a CI runner, the seed makes the same inputs again:\n  {}",
                    // A replay saved what a plain run names only by range.
                    again(name, options, options.replay)
                );
            } else if summary.findings > summary.ranges {
                text.push_str("\n  The failing inputs are the files named in the report above.");
            }
            if summary.ranges > 0 {
                let _ = write!(
                    text,
                    "\n  {} a range of inputs, none of them saved. A replay reads the memory \
                     after every input, and names and saves the one:\n  {}",
                    if summary.ranges == 1 {
                        "1 finding names".to_string()
                    } else {
                        format!("{} findings name", summary.ranges)
                    },
                    again(name, options, true)
                );
            }
            text
        }
        Verdict::Failed(summary, why) => match summary {
            Some(summary) => format!("{name}: FAILED: {why}: {}", numbers(summary)),
            None => format!("{name}: FAILED: {why}"),
        },
        Verdict::NotRun => format!(
            "{name}: FAILED: no test named `{name}` ran in {}'s tests/fuzz.rs; the table in \
             xtask/src/fuzz.rs names it",
            target.package
        ),
        Verdict::NotBuilt => format!(
            "{name}: FAILED: {} did not build (see above)",
            target.package
        ),
        Verdict::CargoFailed => {
            format!("{name}: FAILED: cargo failed before its test ran (see its output above)")
        }
        Verdict::Died(how) => {
            let how = how
                .as_deref()
                .map(|how| format!(" {how}"))
                .unwrap_or_default();
            let mut text = format!(
                "{name}: DIED: its test process ended without a result{how}: an allocation too \
                 big to make, a stack overflow, a kill, or a call to exit (see its output \
                 above)."
            );
            if !options.replay {
                text.push_str(&replay_advice(name, options));
            }
            text
        }
        Verdict::ExitedWithoutResult => {
            let mut text = format!(
                "{name}: FAILED: its test binary exited with libtest's failure status without a \
                 result (see its output above)."
            );
            if !options.replay {
                text.push_str(&replay_advice(name, options));
            }
            text
        }
    }
}

/// How to find the input a run ended on without a result: the saved
/// crashes first, then a replay.
fn replay_advice(name: &str, options: &Options<'_>) -> String {
    format!(
        "\n  The saved crashes in {CRASHES}/{name}/ run first; if it holds {DIED_PREFIX}-* \
         files, it may have died on one of them. Otherwise the inputs follow from the seed \
         alone. To find the one it died on, replay them: each is saved to \
         {CRASHES}/{name}/{LAST_INPUT} before it runs, and when the process dies again \
         fuzz-smoke keeps the last as {CRASHES}/{name}/{DIED_PREFIX}-<hash>. A replay counts \
         only the time spent in the target, so the same seconds usually reach the input this \
         run died on; it adds about 8 µs an input, so for a target that ran faster than about \
         0.5 µs an input (2 million a second), give it more --secs:\n  {}",
        again(name, options, true)
    )
}

/// `1234 inputs in 60.0 s (20567 a second); written memory …; reserved
/// memory …; slowest …`.
fn numbers(summary: &Summary) -> String {
    let seconds = summary.elapsed.as_secs_f64();
    let mut text = format!(
        "{} in {seconds:.1} s ({} a second)",
        plural_u64(summary.inputs, "input"),
        rate(summary.inputs, summary.elapsed)
    );
    for (what, reading) in [
        ("written memory", &summary.memory),
        ("reserved memory", &summary.reserved),
    ] {
        match reading {
            Reading::Measured(memory) => {
                let _ = write!(
                    text,
                    "; {what} {} (+{} of a {} cap)",
                    mib(memory.peak),
                    mib(memory.peak.saturating_sub(memory.baseline)),
                    mib(memory.cap)
                );
                if let Some(code) = &memory.stopped {
                    let _ = write!(text, ", then not checked ({})", check::printable(code));
                }
            }
            Reading::Skipped(code) => {
                let _ = write!(text, "; {what} not checked ({})", check::printable(code));
            }
        }
    }
    if let Some(slowest) = summary.slowest {
        let _ = write!(
            text,
            "; slowest input {} ({})",
            millis(slowest.took),
            plural(slowest.len, "byte")
        );
    }
    text
}

/// A reading's rise, for the summary table.
fn rise(reading: &Reading) -> String {
    match reading {
        Reading::Measured(memory) => {
            format!("+{}", mib(memory.peak.saturating_sub(memory.baseline)))
        }
        Reading::Skipped(_) => "not checked".to_string(),
    }
}

/// The table that ends a run, and the verdict.
fn summary(options: &Options<'_>, results: &[(&str, Verdict)]) -> String {
    let width = results
        .iter()
        .map(|(name, _)| name.len())
        .chain(["target".len()])
        .max()
        .unwrap_or(6);
    let mut table = String::from("== summary\n");
    let _ = writeln!(
        table,
        "{:<width$}  {:<7}  {:>10}  {:>10}  {:>11}  {:>11}  {:>10}",
        "target", "result", "inputs", "a second", "written", "reserved", "slowest"
    );
    let mut failed = Vec::new();
    for (name, verdict) in results {
        let (word, summary) = match verdict {
            Verdict::Clean(summary) => ("ok", Some(summary)),
            Verdict::Found(summary) => ("FAILED", Some(summary)),
            Verdict::Failed(summary, _) => ("FAILED", summary.as_ref()),
            Verdict::NotRun
            | Verdict::NotBuilt
            | Verdict::CargoFailed
            | Verdict::ExitedWithoutResult => ("FAILED", None),
            Verdict::Died(_) => ("DIED", None),
        };
        if word != "ok" {
            failed.push(*name);
        }
        let _ = match summary {
            Some(summary) => writeln!(
                table,
                "{name:<width$}  {word:<7}  {:>10}  {:>10}  {:>11}  {:>11}  {:>10}",
                summary.inputs,
                rate(summary.inputs, summary.elapsed),
                rise(&summary.memory),
                rise(&summary.reserved),
                summary
                    .slowest
                    .map_or_else(|| "-".to_string(), |slowest| millis(slowest.took))
            ),
            None => writeln!(table, "{name:<width$}  {word}"),
        };
    }
    let total = results.len();
    let seed = format!("seed 0x{:016x}", options.seed);
    let _ = if failed.is_empty() {
        writeln!(
            table,
            "fuzz-smoke: ok: {total} of {total} targets ran clean for {} s each ({seed})",
            options.secs
        )
    } else {
        writeln!(
            table,
            "fuzz-smoke: FAILED: {} of {total} targets failed: {} ({seed})",
            failed.len(),
            failed.join(", ")
        )
    };
    table
}

/// Checks that the table and the repository agree: `fuzz`, `fuzz/corpus`
/// and `fuzz/crashes` are directories, not links, walked one component at a
/// time; every directory in `fuzz/corpus/` and `fuzz/crashes/` is a
/// target's; and every target has at least one seed, a regular file that
/// is not hidden.
///
/// # Errors
///
/// [`Error::Read`] if a directory that exists cannot be listed.
fn check_table(root: &Path, table: &[Target]) -> Result<Vec<String>, Error> {
    let mut violations = Vec::new();
    for dir in ["fuzz", CORPUS, CRASHES] {
        let path = root.join(dir);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                violations.push(format!(
                    "{dir} is a symbolic link, which fuzz-smoke does not follow"
                ));
                // Nothing past it is looked at.
                return Ok(violations);
            }
            Ok(metadata) if !metadata.is_dir() => {
                violations.push(format!("{dir} is not a directory"));
                return Ok(violations);
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => return Err(Error::Read { path, source }),
        }
    }
    let named = |name: &str| table.iter().any(|target| target.name == name);
    for (dir, what) in [(CORPUS, "seeds"), (CRASHES, "saved crashes")] {
        for (path, is_dir) in list(&root.join(dir))? {
            let shown = walk::relative(root, &path);
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !is_dir {
                violations.push(format!(
                    "{shown} is not a directory: {dir}/ holds a directory of {what} for each \
                     target (a symbolic link is never followed)"
                ));
            } else if !named(&name) {
                violations.push(format!(
                    "{shown} is the {what} of no fuzz target: the table in xtask/src/fuzz.rs \
                     has no row `{name}`"
                ));
            }
        }
    }
    for target in table {
        let corpus = root.join(CORPUS).join(target.name);
        let shown = walk::relative(root, &corpus);
        let is_dir = fs::symlink_metadata(&corpus).is_ok_and(|metadata| metadata.is_dir());
        if !is_dir {
            violations.push(format!(
                "the fuzz target `{}` has no seeds: {shown} is not a directory",
                target.name
            ));
            continue;
        }
        let seeds = fs::read_dir(&corpus)
            .map_err(|source| Error::Read {
                path: corpus.clone(),
                source,
            })?
            .filter_map(Result::ok)
            .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .count();
        if seeds == 0 {
            violations.push(format!(
                "the fuzz target `{}` has no seeds: {shown} holds no regular file that is not \
                 hidden",
                target.name
            ));
        }
    }
    Ok(violations)
}

/// The entries of `dir` that are not hidden, in name order, each with
/// whether it is a directory (not a link to one); none if `dir` does not
/// exist.
fn list(dir: &Path) -> Result<Vec<(PathBuf, bool)>, Error> {
    let read_error = |source| Error::Read {
        path: dir.to_path_buf(),
        source,
    };
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(read_error(source)),
    };
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.map_err(read_error)?;
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let is_dir = entry.file_type().map_err(read_error)?.is_dir();
        found.push((entry.path(), is_dir));
    }
    found.sort();
    Ok(found)
}

/// Inputs a second, rounded.
fn rate(inputs: u64, elapsed: Duration) -> u64 {
    let seconds = elapsed.as_secs_f64();
    if seconds > 0.0 {
        (inputs as f64 / seconds).round() as u64
    } else {
        0
    }
}

/// Bytes in MiB, to one decimal place.
fn mib(bytes: u64) -> String {
    format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
}

/// A duration in milliseconds, to two decimal places.
fn millis(duration: Duration) -> String {
    format!("{:.2} ms", duration.as_secs_f64() * 1000.0)
}

/// `1 target`, `2 targets`.
fn plural(count: usize, noun: &str) -> String {
    plural_u64(count as u64, noun)
}

/// [`plural`] for a `u64`.
fn plural_u64(count: u64, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

#[cfg(test)]
mod tests;
