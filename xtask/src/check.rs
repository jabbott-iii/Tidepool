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

//! The checks behind `cargo xtask ci`, and the code that runs and reports
//! them.
//!
//! Ported from nadir xtask `check.rs`. Adapted for Tidepool: `ci` runs
//! `deps` first and keeps going after a failure, so one run reports every
//! problem, except that it does not compile anything once `deps` has
//! failed; the summary lists the checks that later roadmap tasks add as
//! "not yet implemented", and the steps it did not run as "not run", never
//! as passed. A test that prints `SKIPPED: <reason>` did not run either: the
//! test step lists such lines (up to 1000; the rest are counted), and its
//! summary row and the last line of `ci` count them all, but a skip does not fail `ci` (real-tool tests skip
//! by design when their tool is not there). When every violation a check
//! found is an out-of-date lockfile, its last line says so in fixed words
//! ([`ONLY_STALE_LOCKFILES`]).

use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use crate::cargo::Skips;
use crate::error::Error;
use crate::{cargo, deps, headers, layers, ucd, unsafe_audit};

/// What a check found.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Report {
    /// Lines to print whether or not the check passed.
    pub(crate) notes: Vec<String>,
    /// One line per violation. The check passed if there are none.
    pub(crate) violations: Vec<String>,
    /// How many of `violations` say only that a workspace's lockfile is out
    /// of date ([`Report::push_stale_lockfile`]). When that is all of them,
    /// the last line says so ([`ONLY_STALE_LOCKFILES`]).
    pub(crate) stale_lockfiles: usize,
    /// The tests that printed `SKIPPED:` instead of running: never a pass,
    /// but not a violation either.
    pub(crate) skips: Skips,
}

impl Report {
    /// Adds a violation that says only that a workspace's lockfile is out of
    /// date: its graph was not checked, but nothing else was found wrong.
    pub(crate) fn push_stale_lockfile(&mut self, violation: String) {
        self.violations.push(violation);
        self.stale_lockfiles += 1;
    }
}

/// What the last line of a check adds, after the count, when every
/// violation it found is an out-of-date lockfile. Fixed text, so that a
/// caller (the Claude Code hook) can tell that from a policy violation
/// without reading any violation, whose text may hold a path.
pub(crate) const ONLY_STALE_LOCKFILES: &str = "only out-of-date lockfiles";

/// One step of `ci`; the ones that are not plain cargo commands are also
/// subcommands.
pub(crate) struct Check {
    /// The name in output, and the subcommand that runs it.
    pub(crate) name: &'static str,
    /// What it does, for the usage text and the step's heading.
    pub(crate) about: &'static str,
    /// What it runs.
    pub(crate) step: Step,
    /// How it depends on the steps before it in `ci`.
    pub(crate) role: Role,
}

/// What a [`Check`] runs.
#[derive(Clone, Copy)]
pub(crate) enum Step {
    /// One cargo command, which reports its own findings.
    Cargo(&'static [&'static str]),
    /// One cargo command that runs tests: as [`Step::Cargo`], but its
    /// output is also read for `SKIPPED:` lines.
    Tests(&'static [&'static str]),
    /// A check of this tool's own, on the repository at the given root.
    Own(fn(&Path) -> Result<Report, Error>),
}

/// How a step of `ci` depends on the steps before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    /// Runs whatever happened before it.
    Independent,
    /// The dependency check. When it does not pass, the steps that compile
    /// are not run.
    Gate,
    /// Compiles the workspace and runs its code (build scripts, proc
    /// macros, tests). Not run after the gate failed: that would run the
    /// code the gate rejected.
    Compiles,
}

/// `cargo xtask deps`.
pub(crate) const DEPS: Check = Check {
    name: "deps",
    about: "No package outside the repository unless approved in xtask/deps-allowlist.txt",
    step: Step::Own(deps::run),
    role: Role::Gate,
};

/// `cargo xtask layers`.
pub(crate) const LAYERS: Check = Check {
    name: "layers",
    about: "Workspace crates depend only on what docs/01-architecture.md's matrix allows",
    step: Step::Own(layers::run),
    role: Role::Independent,
};

/// `cargo xtask unsafe-audit`.
pub(crate) const UNSAFE_AUDIT: Check = Check {
    name: "unsafe-audit",
    about: "`unsafe` only in tidepool-sys and tidepool-platform, each use documented",
    step: Step::Own(unsafe_audit::run),
    role: Role::Independent,
};

/// `cargo xtask headers`.
pub(crate) const HEADERS: Check = Check {
    name: "headers",
    about: "Every source file starts with the Apache-2.0 header of LICENSE's appendix",
    step: Step::Own(headers::run),
    role: Role::Independent,
};

/// What `ucd` does, with and without `--check`.
const UCD_ABOUT: &str = "crates/text/src/pos/grapheme/tables.rs is what crates/text/ucd/17.0.0 \
                         generates (without --check: write it)";

/// `cargo xtask ucd --check`, the step of `ci`.
pub(crate) const UCD: Check = Check {
    name: "ucd",
    about: UCD_ABOUT,
    step: Step::Own(ucd::check),
    role: Role::Independent,
};

/// `cargo xtask ucd`, which writes the tables. Not a step of `ci`, and not
/// in [`COMMANDS`]: `main` runs it for `ucd` without `--check`.
pub(crate) const UCD_WRITE: Check = Check {
    name: "ucd",
    about: UCD_ABOUT,
    step: Step::Own(ucd::write),
    role: Role::Independent,
};

const FMT: &[&str] = &["fmt", "--all", "--", "--check"];
const CLIPPY: &[&str] = &[
    "clippy",
    "--workspace",
    "--all-targets",
    "--",
    "-D",
    "warnings",
];
const TEST: &[&str] = &["test", "--workspace"];

/// The checks that are subcommands of their own. `ucd` is listed as its
/// `--check` form; without `--check` it runs [`UCD_WRITE`].
pub(crate) const COMMANDS: &[Check] = &[DEPS, LAYERS, UNSAFE_AUDIT, HEADERS, UCD];

/// Every step of `ci`, in the order it runs them: `deps` first, so that a
/// dependency problem is reported, and nothing is compiled, before
/// anything else. `fmt` only reads source, so it runs either way.
pub(crate) const CI: &[Check] = &[
    DEPS,
    Check {
        name: "fmt",
        about: "cargo fmt --all -- --check",
        step: Step::Cargo(FMT),
        role: Role::Independent,
    },
    Check {
        name: "clippy",
        about: "cargo clippy --workspace --all-targets -- -D warnings",
        step: Step::Cargo(CLIPPY),
        role: Role::Compiles,
    },
    Check {
        name: "test",
        about: "cargo test --workspace",
        step: Step::Tests(TEST),
        role: Role::Compiles,
    },
    LAYERS,
    UNSAFE_AUDIT,
    HEADERS,
    UCD,
];

/// A command that `AGENTS.md` names but a later roadmap task implements.
pub(crate) struct Later {
    /// The subcommand.
    pub(crate) name: &'static str,
    /// The roadmap task that implements it.
    pub(crate) task: &'static str,
    /// Whether `ci` will run it once it exists (`AGENTS.md` → "Commands").
    pub(crate) in_ci: bool,
}

/// The commands that are not implemented yet. Running one fails; `ci`
/// lists those it will run as not yet implemented.
pub(crate) const LATER: &[Later] = &[
    Later {
        name: "tools",
        task: "roadmap M5.1",
        in_ci: true,
    },
    Later {
        name: "theme",
        task: "roadmap M3.2",
        in_ci: true,
    },
    Later {
        name: "golden",
        task: "roadmap M2.4",
        in_ci: false,
    },
    Later {
        name: "fuzz-smoke",
        task: "roadmap M1.6",
        in_ci: false,
    },
    Later {
        name: "bench",
        task: "no roadmap task yet; first needed by M4.1",
        in_ci: false,
    },
    Later {
        name: "package",
        task: "roadmap M11.6, M12.4 and M14.4",
        in_ci: false,
    },
];

/// Runs what `step` names on the repository at `root`.
fn run_step(step: Step, root: &Path) -> Result<Report, Error> {
    match step {
        Step::Cargo(args) => Ok(cargo_report(
            args,
            cargo::run(root, args)?,
            Skips::default(),
        )),
        Step::Tests(args) => {
            let (passed, skips) = cargo::run_scanning_skips(root, args)?;
            Ok(cargo_report(args, passed, skips))
        }
        Step::Own(run) => run(root),
    }
}

/// The report of a check that is one cargo command, which printed its own
/// findings.
fn cargo_report(args: &[&str], passed: bool, skips: Skips) -> Report {
    let violations = if passed {
        Vec::new()
    } else {
        vec![format!(
            "`{}` failed; see its output above",
            cargo::display(args)
        )]
    };
    Report {
        notes: Vec::new(),
        violations,
        skips,
        ..Report::default()
    }
}

/// `" (N SKIPPED)"` for a step whose tests printed `SKIPPED:` lines, or
/// nothing.
fn skipped_suffix(skipped: usize) -> String {
    match skipped {
        0 => String::new(),
        count => format!(" ({count} SKIPPED)"),
    }
}

/// Runs one check and prints what it found. Returns whether it passed.
pub(crate) fn run_one(check: &Check, root: &Path, out: &mut dyn Write) -> io::Result<bool> {
    Ok(run_reported(check, root, out)?.0)
}

/// The last line of a check that ran ([`run_reported`]): `<name>: ok` or
/// `<name>: FAILED`, then in parentheses, `; `-separated, the number of
/// violations, [`ONLY_STALE_LOCKFILES`] if `stale_lockfiles` of them are
/// all there are, and the number of skipped tests. Built from the counts and
/// fixed text only.
fn verdict(name: &str, violations: usize, stale_lockfiles: usize, skipped: usize) -> String {
    let mut details = Vec::new();
    match violations {
        0 => {}
        1 => details.push("1 violation".to_string()),
        count => details.push(format!("{count} violations")),
    }
    if violations > 0 && stale_lockfiles == violations {
        details.push(ONLY_STALE_LOCKFILES.to_string());
    }
    if skipped > 0 {
        details.push(format!("{skipped} SKIPPED"));
    }
    let word = if violations == 0 { "ok" } else { "FAILED" };
    match details.as_slice() {
        [] => format!("{name}: {word}"),
        details => format!("{name}: {word} ({})", details.join("; ")),
    }
}

/// [`run_one`], also returning how many tests printed `SKIPPED:`.
///
/// After the notes and the `violation:` lines, each skipped test gets a
/// `skipped: <reason>` line, and the last line counts them:
/// `test: ok (2 SKIPPED)` or `test: FAILED (1 violation; 2 SKIPPED)`. When
/// every violation is an out-of-date lockfile, it reads `deps: FAILED (1
/// violation; only out-of-date lockfiles)` ([`verdict`]).
fn run_reported(check: &Check, root: &Path, out: &mut dyn Write) -> io::Result<(bool, usize)> {
    let name = check.name;
    match run_step(check.step, root) {
        Ok(report) => {
            for note in &report.notes {
                writeln!(out, "{note}")?;
            }
            for violation in &report.violations {
                writeln!(out, "violation: {violation}")?;
            }
            let skips = &report.skips;
            for reason in &skips.reasons {
                writeln!(out, "skipped: {reason}")?;
            }
            if let Some(more) = skips.count.checked_sub(skips.reasons.len())
                && more > 0
            {
                writeln!(out, "skipped: and {more} more not listed")?;
            }
            writeln!(
                out,
                "{}",
                verdict(
                    name,
                    report.violations.len(),
                    report.stale_lockfiles,
                    skips.count
                )
            )?;
            Ok((report.violations.is_empty(), skips.count))
        }
        Err(error) => {
            writeln!(out, "{name}: FAILED: {error}")?;
            Ok((false, 0))
        }
    }
}

/// How a step of `ci` ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Passed,
    Failed,
    /// Not run, because the gate (named) failed.
    NotRun(&'static str),
}

/// Runs every one of `checks` in order, whatever happened to the ones
/// before, except that a [`Role::Compiles`] step is not run once a
/// [`Role::Gate`] step has failed. Then prints a summary table that also
/// lists the `later` commands `ci` will run once they exist. Returns
/// whether every check passed.
pub(crate) fn run_ci(
    checks: &[Check],
    later: &[Later],
    root: &Path,
    out: &mut dyn Write,
) -> io::Result<bool> {
    let mut outcomes = Vec::with_capacity(checks.len());
    let mut failed_gate = None;
    for check in checks {
        writeln!(out, "== {}: {}", check.name, check.about)?;
        if check.role == Role::Compiles
            && let Some(gate) = failed_gate
        {
            writeln!(
                out,
                "{}: not run: `{gate}` failed, and this step would compile and run the code it \
                 rejected",
                check.name
            )?;
            outcomes.push(Outcome {
                name: check.name,
                status: Status::NotRun(gate),
                elapsed: Duration::ZERO,
                skipped: 0,
            });
            continue;
        }
        // The cargo commands write to the terminal directly, so this
        // heading has to be out before they start.
        out.flush()?;
        let start = Instant::now();
        let (passed, skipped) = run_reported(check, root, out)?;
        let status = if passed {
            Status::Passed
        } else {
            if check.role == Role::Gate {
                failed_gate = Some(check.name);
            }
            Status::Failed
        };
        outcomes.push(Outcome {
            name: check.name,
            status,
            elapsed: start.elapsed(),
            skipped,
        });
        out.flush()?;
    }
    let pending: Vec<&Later> = later.iter().filter(|later| later.in_ci).collect();
    out.write_all(summary(&outcomes, &pending).as_bytes())?;
    Ok(outcomes
        .iter()
        .all(|outcome| outcome.status == Status::Passed))
}

/// How one step of `ci` went.
#[derive(Clone, Copy, Debug)]
struct Outcome<'a> {
    name: &'a str,
    status: Status,
    elapsed: Duration,
    /// How many of its tests printed `SKIPPED:`.
    skipped: usize,
}

/// Formats the table that ends a `ci` run.
fn summary(outcomes: &[Outcome<'_>], pending: &[&Later]) -> String {
    let width = outcomes
        .iter()
        .map(|outcome| outcome.name.len())
        .chain(pending.iter().map(|later| later.name.len()))
        .max()
        .unwrap_or(0);
    // `ok` or `FAILED`, then the skipped count if any.
    let word = |outcome: &Outcome<'_>| {
        let word = match outcome.status {
            Status::Passed => "ok",
            _ => "FAILED",
        };
        format!("{word}{}", skipped_suffix(outcome.skipped))
    };
    let word_width = outcomes
        .iter()
        .filter(|outcome| !matches!(outcome.status, Status::NotRun(_)))
        .map(|outcome| word(outcome).len())
        .chain([6])
        .max()
        .unwrap_or(6);
    let mut table = String::from("== summary\n");
    let mut failed = Vec::new();
    let mut not_run = Vec::new();
    let skipped: usize = outcomes.iter().map(|outcome| outcome.skipped).sum();
    // Writing to a `String` cannot fail.
    for outcome in outcomes {
        let name = outcome.name;
        match outcome.status {
            Status::Passed => {}
            Status::Failed => failed.push(name),
            Status::NotRun(gate) => {
                not_run.push(name);
                let _ = writeln!(table, "{name:<width$}  not run ({gate} failed)");
                continue;
            }
        }
        let _ = writeln!(
            table,
            "{name:<width$}  {:<word_width$}  {:>7.1} s",
            word(outcome),
            outcome.elapsed.as_secs_f64()
        );
    }
    for later in pending {
        let _ = writeln!(
            table,
            "{:<width$}  not yet implemented ({})",
            later.name, later.task
        );
    }
    let total = outcomes.len();
    let not_run = match not_run.as_slice() {
        [] => String::new(),
        names => format!("; not run: {}", names.join(", ")),
    };
    let not_yet = match pending {
        [] => String::new(),
        _ => {
            let names: Vec<&str> = pending.iter().map(|later| later.name).collect();
            format!("; not yet implemented: {}", names.join(", "))
        }
    };
    // A skipped test is not a pass, so the verdict says how many there
    // were, right after `ok` or `FAILED`.
    let skipped = match skipped {
        0 => String::new(),
        1 => " (1 test SKIPPED)".to_string(),
        count => format!(" ({count} tests SKIPPED)"),
    };
    let _ = if failed.is_empty() && not_run.is_empty() {
        writeln!(
            table,
            "ci: ok{skipped}: {total} of {total} steps passed{not_yet}"
        )
    } else {
        writeln!(
            table,
            "ci: FAILED{skipped}: {} of {total} steps failed: {}{not_run}{not_yet}",
            failed.len(),
            failed.join(", ")
        )
    };
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passing(_: &Path) -> Result<Report, Error> {
        Ok(Report {
            notes: vec!["looked at 3 things".to_string()],
            ..Report::default()
        })
    }

    fn silent(_: &Path) -> Result<Report, Error> {
        Ok(Report::default())
    }

    fn violating(_: &Path) -> Result<Report, Error> {
        Ok(Report {
            notes: vec!["looked at 3 things".to_string()],
            violations: vec!["a is wrong".to_string(), "b is wrong".to_string()],
            ..Report::default()
        })
    }

    fn one_violation(_: &Path) -> Result<Report, Error> {
        Ok(Report {
            violations: vec!["a is wrong".to_string()],
            ..Report::default()
        })
    }

    /// Tests that ran, two of which printed `SKIPPED:`.
    fn skipping(_: &Path) -> Result<Report, Error> {
        Ok(Report {
            skips: Skips {
                reasons: vec![
                    "needs_git: TIDEPOOL_IT_GIT is not set".to_string(),
                    "needs_ra: rust-analyzer not found".to_string(),
                ],
                count: 2,
            },
            ..Report::default()
        })
    }

    /// Tests that failed, and one that printed `SKIPPED:`, of three that
    /// did (one reason not kept).
    fn failing_and_skipping(_: &Path) -> Result<Report, Error> {
        Ok(Report {
            violations: vec!["`cargo test` failed".to_string()],
            skips: Skips {
                reasons: vec!["needs_git: TIDEPOOL_IT_GIT is not set".to_string()],
                count: 2,
            },
            ..Report::default()
        })
    }

    /// One workspace whose lockfile is only out of date.
    fn one_stale_lockfile(_: &Path) -> Result<Report, Error> {
        let mut report = Report::default();
        report.push_stale_lockfile("the lockfile of A is out of date".to_string());
        Ok(report)
    }

    /// Two such workspaces.
    fn two_stale_lockfiles(_: &Path) -> Result<Report, Error> {
        let mut report = Report::default();
        report.push_stale_lockfile("the lockfile of A is out of date".to_string());
        report.push_stale_lockfile("the lockfile of B is out of date".to_string());
        Ok(report)
    }

    /// A stale lockfile and another violation, whose text says the same
    /// words but is not of that kind.
    fn stale_lockfile_and_more(_: &Path) -> Result<Report, Error> {
        let mut report = Report::default();
        report.push_stale_lockfile("the lockfile of A is out of date".to_string());
        report
            .violations
            .push("/x/Cargo.lock is out of date/evil is outside the repository".to_string());
        Ok(report)
    }

    fn erroring(_: &Path) -> Result<Report, Error> {
        Err(Error::UnexpectedOutput {
            command: "cargo metadata".to_string(),
            detail: "nothing".to_string(),
        })
    }

    fn check(name: &'static str, run: fn(&Path) -> Result<Report, Error>) -> Check {
        with_role(name, run, Role::Independent)
    }

    fn with_role(name: &'static str, run: fn(&Path) -> Result<Report, Error>, role: Role) -> Check {
        Check {
            name,
            about: "demo",
            step: Step::Own(run),
            role,
        }
    }

    fn output_of_one(run: fn(&Path) -> Result<Report, Error>) -> (bool, String) {
        let mut out = Vec::new();
        let passed = run_one(&check("demo", run), Path::new("."), &mut out).unwrap();
        (passed, String::from_utf8(out).unwrap())
    }

    /// The output of `run_ci`, with the measured times replaced by `?`.
    fn output_of_ci(checks: &[Check], later: &[Later]) -> (bool, String) {
        let mut out = Vec::new();
        let passed = run_ci(checks, later, Path::new("."), &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        let lines: Vec<String> = text
            .lines()
            .map(|line| match line.strip_suffix(" s") {
                Some(timed) => format!(
                    "{} ? s",
                    timed.trim_end_matches(|c: char| c != ' ').trim_end()
                ),
                None => line.to_string(),
            })
            .collect();
        (passed, lines.join("\n"))
    }

    #[test]
    fn a_passing_check_prints_its_notes_and_ok() {
        assert_eq!(
            output_of_one(passing),
            (true, "looked at 3 things\ndemo: ok\n".to_string())
        );
        assert_eq!(output_of_one(silent), (true, "demo: ok\n".to_string()));
    }

    #[test]
    fn a_check_with_violations_fails_and_lists_them() {
        assert_eq!(
            output_of_one(violating),
            (
                false,
                "looked at 3 things\nviolation: a is wrong\nviolation: b is wrong\n\
                 demo: FAILED (2 violations)\n"
                    .to_string()
            )
        );
        assert_eq!(
            output_of_one(one_violation),
            (
                false,
                "violation: a is wrong\ndemo: FAILED (1 violation)\n".to_string()
            )
        );
    }

    #[test]
    fn only_out_of_date_lockfiles_are_named_on_the_last_line() {
        assert_eq!(
            output_of_one(one_stale_lockfile),
            (
                false,
                "violation: the lockfile of A is out of date\n\
                 demo: FAILED (1 violation; only out-of-date lockfiles)\n"
                    .to_string()
            )
        );
        assert_eq!(
            output_of_one(two_stale_lockfiles),
            (
                false,
                "violation: the lockfile of A is out of date\n\
                 violation: the lockfile of B is out of date\n\
                 demo: FAILED (2 violations; only out-of-date lockfiles)\n"
                    .to_string()
            )
        );
        // Any violation of another kind gives the plain form, whatever its
        // text says.
        assert_eq!(
            output_of_one(stale_lockfile_and_more),
            (
                false,
                "violation: the lockfile of A is out of date\n\
                 violation: /x/Cargo.lock is out of date/evil is outside the repository\n\
                 demo: FAILED (2 violations)\n"
                    .to_string()
            )
        );
        // The suffix is fixed text and the count, never a violation's.
        assert_eq!(
            verdict("deps", 1, 1, 0),
            "deps: FAILED (1 violation; only out-of-date lockfiles)"
        );
        assert_eq!(
            verdict("deps", 3, 3, 0),
            "deps: FAILED (3 violations; only out-of-date lockfiles)"
        );
        assert_eq!(verdict("deps", 3, 2, 0), "deps: FAILED (3 violations)");
        assert_eq!(verdict("deps", 0, 0, 0), "deps: ok");
    }

    #[test]
    fn ci_reads_the_same_when_deps_found_only_out_of_date_lockfiles() {
        let (passed, output) = output_of_ci(
            &[
                with_role("deps", one_stale_lockfile, Role::Gate),
                with_role("clippy", passing, Role::Compiles),
                check("layers", silent),
            ],
            &[],
        );
        assert!(!passed);
        assert_eq!(
            output,
            "== deps: demo\nviolation: the lockfile of A is out of date\n\
             deps: FAILED (1 violation; only out-of-date lockfiles)\n\
             == clippy: demo\nclippy: not run: `deps` failed, and this step would compile and \
             run the code it rejected\n\
             == layers: demo\nlayers: ok\n\
             == summary\n\
             deps    FAILED ? s\n\
             clippy  not run (deps failed)\n\
             layers  ok ? s\n\
             ci: FAILED: 1 of 3 steps failed: deps; not run: clippy"
        );
    }

    #[test]
    fn a_check_that_cannot_run_fails_with_the_reason() {
        assert_eq!(
            output_of_one(erroring),
            (
                false,
                "demo: FAILED: unexpected output from `cargo metadata`: nothing\n".to_string()
            )
        );
    }

    #[test]
    fn all_checks_passing_is_a_pass_that_still_names_what_is_missing() {
        let later = [
            Later {
                name: "tools",
                task: "roadmap M5.1",
                in_ci: true,
            },
            Later {
                name: "golden",
                task: "roadmap M2.4",
                in_ci: false,
            },
        ];
        let (passed, output) =
            output_of_ci(&[check("one", passing), check("second", silent)], &later);
        assert!(passed);
        assert_eq!(
            output,
            "== one: demo\nlooked at 3 things\none: ok\n== second: demo\nsecond: ok\n\
             == summary\n\
             one     ok ? s\n\
             second  ok ? s\n\
             tools   not yet implemented (roadmap M5.1)\n\
             ci: ok: 2 of 2 steps passed; not yet implemented: tools"
        );
    }

    #[test]
    fn the_run_keeps_going_after_a_failure_and_fails() {
        let (passed, output) = output_of_ci(
            &[
                check("one", silent),
                check("two", one_violation),
                check("three", erroring),
                check("four", passing),
            ],
            &[],
        );
        assert!(!passed);
        assert_eq!(
            output,
            "== one: demo\none: ok\n\
             == two: demo\nviolation: a is wrong\ntwo: FAILED (1 violation)\n\
             == three: demo\nthree: FAILED: unexpected output from `cargo metadata`: nothing\n\
             == four: demo\nlooked at 3 things\nfour: ok\n\
             == summary\n\
             one    ok ? s\n\
             two    FAILED ? s\n\
             three  FAILED ? s\n\
             four   ok ? s\n\
             ci: FAILED: 2 of 4 steps failed: two, three"
        );
    }

    #[test]
    fn nothing_is_compiled_after_the_gate_fails() {
        let (passed, output) = output_of_ci(
            &[
                with_role("deps", one_violation, Role::Gate),
                check("fmt", silent),
                with_role("clippy", passing, Role::Compiles),
                with_role("test", passing, Role::Compiles),
                check("layers", passing),
            ],
            &[],
        );
        assert!(!passed);
        assert_eq!(
            output,
            "== deps: demo\nviolation: a is wrong\ndeps: FAILED (1 violation)\n\
             == fmt: demo\nfmt: ok\n\
             == clippy: demo\nclippy: not run: `deps` failed, and this step would compile and \
             run the code it rejected\n\
             == test: demo\ntest: not run: `deps` failed, and this step would compile and run \
             the code it rejected\n\
             == layers: demo\nlooked at 3 things\nlayers: ok\n\
             == summary\n\
             deps    FAILED ? s\n\
             fmt     ok ? s\n\
             clippy  not run (deps failed)\n\
             test    not run (deps failed)\n\
             layers  ok ? s\n\
             ci: FAILED: 1 of 5 steps failed: deps; not run: clippy, test"
        );
        // A gate that cannot run counts as failed too; one that passes
        // lets everything run.
        let (_, output) = output_of_ci(
            &[
                with_role("deps", erroring, Role::Gate),
                with_role("clippy", silent, Role::Compiles),
            ],
            &[],
        );
        assert!(output.contains("clippy: not run"), "{output}");
        let (passed, output) = output_of_ci(
            &[
                with_role("deps", silent, Role::Gate),
                with_role("clippy", silent, Role::Compiles),
            ],
            &[],
        );
        assert!(passed, "{output}");
        assert!(output.contains("clippy: ok"), "{output}");
    }

    #[test]
    fn a_skipped_test_is_listed_and_counted_but_does_not_fail() {
        assert_eq!(
            output_of_one(skipping),
            (
                true,
                "skipped: needs_git: TIDEPOOL_IT_GIT is not set\n\
                 skipped: needs_ra: rust-analyzer not found\n\
                 demo: ok (2 SKIPPED)\n"
                    .to_string()
            )
        );
        assert_eq!(
            output_of_one(failing_and_skipping),
            (
                false,
                "violation: `cargo test` failed\n\
                 skipped: needs_git: TIDEPOOL_IT_GIT is not set\n\
                 skipped: and 1 more not listed\n\
                 demo: FAILED (1 violation; 2 SKIPPED)\n"
                    .to_string()
            )
        );
    }

    #[test]
    fn the_step_line_counts_every_skip() {
        assert_eq!(verdict("test", 0, 0, 1), "test: ok (1 SKIPPED)");
        assert_eq!(
            verdict("test", 1, 0, 1),
            "test: FAILED (1 violation; 1 SKIPPED)"
        );
        assert_eq!(verdict("test", 0, 0, 0), "test: ok");
    }

    #[test]
    fn a_run_with_skipped_tests_never_reads_as_a_clean_pass() {
        let (passed, output) = output_of_ci(&[check("deps", silent), check("test", skipping)], &[]);
        // Skips do not fail `ci`: real-tool tests skip by design.
        assert!(passed, "{output}");
        assert_eq!(
            output,
            "== deps: demo\ndeps: ok\n\
             == test: demo\n\
             skipped: needs_git: TIDEPOOL_IT_GIT is not set\n\
             skipped: needs_ra: rust-analyzer not found\n\
             test: ok (2 SKIPPED)\n\
             == summary\n\
             deps  ok ? s\n\
             test  ok (2 SKIPPED) ? s\n\
             ci: ok (2 tests SKIPPED): 2 of 2 steps passed"
        );
        // A failed run says so too, in the singular when there is one.
        let outcomes = [
            Outcome {
                name: "fmt",
                status: Status::Failed,
                elapsed: Duration::from_millis(500),
                skipped: 0,
            },
            Outcome {
                name: "test",
                status: Status::Passed,
                elapsed: Duration::from_secs(3),
                skipped: 1,
            },
        ];
        assert_eq!(
            summary(&outcomes, &[]),
            "== summary\n\
             fmt   FAILED              0.5 s\n\
             test  ok (1 SKIPPED)      3.0 s\n\
             ci: FAILED (1 test SKIPPED): 1 of 2 steps failed: fmt\n"
        );
        // The skips of a step that failed count too.
        let outcomes = [Outcome {
            name: "test",
            status: Status::Failed,
            elapsed: Duration::from_secs(3),
            skipped: 2,
        }];
        assert_eq!(
            summary(&outcomes, &[]),
            "== summary\n\
             test  FAILED (2 SKIPPED)      3.0 s\n\
             ci: FAILED (2 tests SKIPPED): 1 of 1 steps failed: test\n"
        );
    }

    #[test]
    fn the_summary_lines_up_names_results_and_times() {
        let outcome = |name, status, elapsed| Outcome {
            name,
            status,
            elapsed,
            skipped: 0,
        };
        let outcomes = [
            outcome("deps", Status::Passed, Duration::from_millis(340)),
            outcome(
                "unsafe-audit",
                Status::Passed,
                Duration::from_millis(12_060),
            ),
            outcome("test", Status::Failed, Duration::from_secs(125)),
        ];
        let theme = Later {
            name: "theme",
            task: "roadmap M3.2",
            in_ci: true,
        };
        assert_eq!(
            summary(&outcomes, &[&theme]),
            "== summary\n\
             deps          ok          0.3 s\n\
             unsafe-audit  ok         12.1 s\n\
             test          FAILED    125.0 s\n\
             theme         not yet implemented (roadmap M3.2)\n\
             ci: FAILED: 1 of 3 steps failed: test; not yet implemented: theme\n"
        );
        assert_eq!(
            summary(&[], &[]),
            "== summary\nci: ok: 0 of 0 steps passed\n"
        );
    }

    #[test]
    fn ci_runs_deps_first_then_the_documented_order() {
        let steps: Vec<(&str, Role)> = CI.iter().map(|check| (check.name, check.role)).collect();
        assert_eq!(
            steps,
            [
                ("deps", Role::Gate),
                ("fmt", Role::Independent),
                ("clippy", Role::Compiles),
                ("test", Role::Compiles),
                ("layers", Role::Independent),
                ("unsafe-audit", Role::Independent),
                ("headers", Role::Independent),
                ("ucd", Role::Independent),
            ]
        );
        // Only the test step's output is read for `SKIPPED:` lines.
        let scanned: Vec<&str> = CI
            .iter()
            .filter(|check| matches!(check.step, Step::Tests(_)))
            .map(|check| check.name)
            .collect();
        assert_eq!(scanned, ["test"]);
        let pending: Vec<&str> = LATER
            .iter()
            .filter(|later| later.in_ci)
            .map(|later| later.name)
            .collect();
        assert_eq!(pending, ["tools", "theme"]);
    }

    #[test]
    fn every_step_runs_what_its_heading_says() {
        let expected = [
            ("deps", None),
            ("fmt", Some("cargo fmt --all -- --check")),
            (
                "clippy",
                Some("cargo clippy --workspace --all-targets -- -D warnings"),
            ),
            ("test", Some("cargo test --workspace")),
            ("layers", None),
            ("unsafe-audit", None),
            ("headers", None),
            ("ucd", None),
        ];
        assert_eq!(CI.len(), expected.len());
        for (check, (name, command)) in CI.iter().zip(expected) {
            assert_eq!(check.name, name);
            match (check.step, command) {
                (Step::Cargo(args) | Step::Tests(args), Some(command)) => {
                    assert_eq!(cargo::display(args), command, "{name}");
                    assert_eq!(check.about, command, "{name}");
                }
                (Step::Own(run), None) => {
                    let own = COMMANDS
                        .iter()
                        .find(|command| command.name == name)
                        .expect("the step is also a command");
                    assert!(
                        matches!(own.step, Step::Own(same) if std::ptr::fn_addr_eq(same, run)),
                        "{name}"
                    );
                }
                _ => panic!("`{name}` runs the wrong kind of step"),
            }
        }
        // And each command runs its own check.
        let own = |name: &str| match COMMANDS.iter().find(|c| c.name == name).map(|c| c.step) {
            Some(Step::Own(run)) => run,
            _ => panic!("`{name}` is not one of this tool's checks"),
        };
        let deps_run: fn(&Path) -> Result<Report, Error> = deps::run;
        let layers_run: fn(&Path) -> Result<Report, Error> = layers::run;
        let audit_run: fn(&Path) -> Result<Report, Error> = unsafe_audit::run;
        let headers_run: fn(&Path) -> Result<Report, Error> = headers::run;
        let ucd_check: fn(&Path) -> Result<Report, Error> = ucd::check;
        let ucd_write: fn(&Path) -> Result<Report, Error> = ucd::write;
        assert!(std::ptr::fn_addr_eq(own("deps"), deps_run));
        assert!(std::ptr::fn_addr_eq(own("layers"), layers_run));
        assert!(std::ptr::fn_addr_eq(own("unsafe-audit"), audit_run));
        assert!(std::ptr::fn_addr_eq(own("headers"), headers_run));
        // `ucd` as a command and as the step of `ci` only checks; the
        // writing form has the same name and heading.
        assert!(std::ptr::fn_addr_eq(own("ucd"), ucd_check));
        assert!(matches!(UCD_WRITE.step, Step::Own(run) if std::ptr::fn_addr_eq(run, ucd_write)));
        assert_eq!((UCD_WRITE.name, UCD_WRITE.about), (UCD.name, UCD.about));
        assert!(!COMMANDS.iter().chain(CI).any(|check| matches!(
            check.step,
            Step::Own(run) if std::ptr::fn_addr_eq(run, ucd_write)
        )));
    }

    #[test]
    fn command_names_are_unique() {
        let mut names: Vec<&str> = COMMANDS
            .iter()
            .map(|check| check.name)
            .chain(LATER.iter().map(|later| later.name))
            .chain(["ci"])
            .collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "{names:?}");
    }
}
