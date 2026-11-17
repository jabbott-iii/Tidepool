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

//! The command line, `[FILTER] [--save --machine NAME]`, and the report
//! (ADR-0035).
//!
//! - **The filter** is a substring of the suite names, as `cargo test`
//!   filters: every suite whose name contains it runs, and all of them
//!   without one. A filter that matches no suite is an error.
//! - **A run compares and reports.** Each suite's median and p95 are
//!   printed as it finishes. Where the baseline was recorded on a machine
//!   with the same [`Facts`], each median is shown with its change from the
//!   baseline's, and one more than [`SLOWER_PERCENT`] slower is marked.
//!   Where the facts differ, the report says which machine the baseline
//!   is from and shows no changes. A slower suite never fails the run.
//! - **`--save --machine NAME`** merges the measured suites into the
//!   baseline ([`Baseline::merge`]) and writes it, once every suite ran. It
//!   refuses, before measuring anything, to merge into a file recorded on
//!   another machine.
//!
//! [`run`] returns an error, which exits 1, only for a bad argument, a
//! filter that matches nothing, a failed suite, a baseline that cannot be
//! read, parsed or written, a refused merge, or a report that cannot be
//! written.

use std::fmt::Write as _;
use std::io::Write;
use std::path::Path;

use tidepool_core::CoreError;
use tidepool_core::cli::Args;

use crate::BenchError;
use crate::baseline::{Baseline, Entry, Facts, Machine};
use crate::error::{printable, shown};
use crate::harness::{Settings, Suite, Summary, prime_allocator, run_suite};

/// How much slower than the baseline a median must be, in percent, to be
/// marked: `docs/04-quality.md` asks a change that slows a suite by more
/// than this to say so and why.
pub const SLOWER_PERCENT: u64 = 10;

/// What the command line asks for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// Run only the suites whose names contain this.
    pub filter: Option<String>,
    /// Write the measured suites into the baseline.
    pub save: bool,
    /// The machine's name for `--save`.
    pub machine: Option<String>,
    /// Show the usage instead of running.
    pub help: bool,
}

impl Options {
    /// Reads `args`, the arguments without the program's name.
    ///
    /// `--help` or `-h` wins over anything else given. Otherwise there may
    /// be one filter; `--save` needs `--machine NAME` (also
    /// `--machine=NAME`), and `--machine` needs `--save`. A name must not be
    /// blank or hold a control character.
    ///
    /// # Errors
    ///
    /// [`BenchError::Usage`] saying what is wrong.
    pub fn parse(mut args: Args) -> Result<Self, BenchError> {
        let usage = |error: CoreError| BenchError::Usage(error.to_string());
        // Both are taken, so that `-h --help` is no unexpected argument.
        let help = args.flag("--help") | args.flag("-h");
        if help {
            return Ok(Self {
                help,
                ..Self::default()
            });
        }
        let save = args.flag("--save");
        let machine = args.value("--machine").map_err(usage)?;
        let filter = match args.finish().map_err(usage)?.as_slice() {
            [] => None,
            [filter] => Some(filter.clone()),
            [_, extra, ..] => {
                return Err(BenchError::Usage(format!(
                    "only one filter is taken, but `{extra}` came after the first"
                )));
            }
        };
        match (save, &machine) {
            (true, None) => {
                return Err(BenchError::Usage(
                    "`--save` needs `--machine NAME`, the name of this machine, which the \
                     baseline records"
                        .to_string(),
                ));
            }
            (false, Some(_)) => {
                return Err(BenchError::Usage(
                    "`--machine` names the machine for `--save`, so it needs `--save` too"
                        .to_string(),
                ));
            }
            _ => {}
        }
        if let Some(name) = &machine {
            if name.trim().is_empty() {
                return Err(BenchError::Usage(
                    "`--machine` needs a name, not a blank one".to_string(),
                ));
            }
            if name.chars().any(char::is_control) {
                return Err(BenchError::Usage(
                    "`--machine` needs a name without control characters".to_string(),
                ));
            }
        }
        Ok(Self {
            filter,
            save,
            machine,
            help,
        })
    }
}

/// The suites whose names contain `filter`, in their order; all of them
/// without a filter.
///
/// # Errors
///
/// [`BenchError::NoSuiteMatches`] when none does.
pub fn select<'a>(suites: &'a [Suite], filter: Option<&str>) -> Result<Vec<&'a Suite>, BenchError> {
    let selected: Vec<&Suite> = suites
        .iter()
        .filter(|suite| filter.is_none_or(|filter| suite.name.contains(filter)))
        .collect();
    if selected.is_empty() {
        return Err(BenchError::NoSuiteMatches {
            filter: filter.unwrap_or_default().to_string(),
        });
    }
    Ok(selected)
}

/// The text of `--help`, which lists `suites`.
pub fn usage(suites: &[Suite]) -> String {
    let mut text = format!(
        "Usage: cargo xtask bench [FILTER] [--save --machine NAME]\n\
         \n\
         Runs the suites whose names contain FILTER (all of them without one) in a release\n\
         build, and prints each one's median and p95 per iteration. Where bench/baseline.json\n\
         was recorded on a machine with the same OS, architecture, thread count and CPU model,\n\
         it shows each median's change and marks one more than {SLOWER_PERCENT}% slower.\n\
         \n\
         Options:\n  \
         --save          Write the suites measured into bench/baseline.json, keeping its\n                  \
         other suites; refused if the file was recorded on another machine\n  \
         --machine NAME  With --save: this machine's name, which the file records\n  \
         --help, -h      Show this text\n\
         \n\
         Exit status: 0 when every suite ran, slower ones included; 1 for a bad argument, a\n\
         filter that matches no suite, a failed suite, a baseline that cannot be read,\n\
         parsed or written, a --save refused because the baseline was recorded on another\n\
         machine, or a report that cannot be written.\n\
         \n\
         Suites:\n"
    );
    for suite in suites {
        // Writing to a `String` cannot fail.
        let _ = writeln!(text, "  {}", suite.name);
    }
    text
}

/// Whether a median of `now_ns` is more than [`SLOWER_PERCENT`] slower than
/// the baseline's `baseline_ns`.
pub fn is_slower(baseline_ns: u64, now_ns: u64) -> bool {
    u128::from(now_ns) * 100 > u128::from(baseline_ns) * u128::from(100 + SLOWER_PERCENT)
}

/// The change from `baseline_ns` to `now_ns`, in percent; `None` when the
/// baseline is 0, which no change is a percentage of.
pub fn change_percent(baseline_ns: u64, now_ns: u64) -> Option<f64> {
    if baseline_ns == 0 {
        return None;
    }
    let (base, now) = (baseline_ns as f64, now_ns as f64);
    Some((now - base) / base * 100.0)
}

/// `ns` for people: `850 ns`, `12.3 µs`, `1.52 ms`, `2.40 s`, with three
/// significant digits from a microsecond on.
pub fn duration(ns: u64) -> String {
    let (value, unit) = match ns {
        0..1_000 => return format!("{ns} ns"),
        1_000..1_000_000 => (ns as f64 / 1e3, "µs"),
        1_000_000..1_000_000_000 => (ns as f64 / 1e6, "ms"),
        _ => (ns as f64 / 1e9, "s"),
    };
    if value < 10.0 {
        format!("{value:.2} {unit}")
    } else if value < 100.0 {
        format!("{value:.1} {unit}")
    } else {
        format!("{value:.0} {unit}")
    }
}

/// Runs the suites `options` selects from `suites`, with `settings`, and
/// reports to `out`, comparing with the baseline at `path` where it was
/// recorded on a machine with this one's `facts`. With `--save`, writes the
/// baseline after every suite ran, keeping the file's suites that are still
/// in `suites`. Before the first suite it primes the allocator
/// ([`prime_allocator`]).
///
/// # Errors
///
/// [`BenchError::NoSuiteMatches`]; [`BenchError::Read`] or
/// [`BenchError::Parse`] for the baseline, and
/// [`BenchError::OtherMachine`] for a `--save` into another machine's,
/// before any suite runs; [`BenchError::SuitesFailed`] after the others
/// ran (nothing is saved then); [`BenchError::Write`];
/// [`BenchError::Output`].
pub fn run(
    options: &Options,
    suites: &[Suite],
    settings: &Settings,
    path: &Path,
    facts: &Facts,
    out: &mut dyn Write,
) -> Result<(), BenchError> {
    let selected = select(suites, options.filter.as_deref())?;
    let baseline = Baseline::read(path)?;
    let machine = match (&options.machine, options.save) {
        (Some(name), true) => Some(Machine {
            name: name.clone(),
            facts: facts.clone(),
        }),
        _ => None,
    };
    if let (Some(machine), Some(existing)) = (&machine, &baseline) {
        existing
            .check_machine(machine)
            .map_err(|detail| BenchError::OtherMachine {
                path: path.to_path_buf(),
                detail,
            })?;
    }
    let output = BenchError::Output;
    // The baseline's suites, if they were measured on a machine like this.
    let compared = baseline
        .as_ref()
        .filter(|baseline| baseline.machine.facts == *facts);

    let count = selected.len();
    let suites_word = if count == 1 { "suite" } else { "suites" };
    let matching = match &options.filter {
        Some(filter) => format!(" whose names contain `{}`", printable(filter)),
        None => String::new(),
    };
    writeln!(out, "running {count} {suites_word}{matching}").map_err(output)?;
    match (&baseline, compared) {
        (None, _) => writeln!(
            out,
            "no baseline at `{}`: nothing to compare with",
            shown(path)
        ),
        (Some(baseline), Some(_)) => writeln!(
            out,
            "comparing with `{}`, recorded on `{}` ({})",
            shown(path),
            printable(&baseline.machine.name),
            baseline.machine.facts.describe()
        ),
        (Some(baseline), None) => writeln!(
            out,
            "the baseline was recorded on `{}`, which is not this machine ({} there, {} here): \
             no changes are shown",
            printable(&baseline.machine.name),
            baseline.machine.facts.describe(),
            facts.describe()
        ),
    }
    .map_err(output)?;
    if cfg!(debug_assertions) {
        writeln!(
            out,
            "note: this build has debug assertions on, so its timings do not compare with a \
             release build's; `cargo xtask bench` builds in release"
        )
        .map_err(output)?;
    }

    let width = selected
        .iter()
        .map(|suite| suite.name.len())
        .max()
        .unwrap_or(0);
    let mut measured: Vec<(String, Summary)> = Vec::with_capacity(count);
    let mut failed = Vec::new();
    let mut slower = Vec::new();
    // So that no suite's numbers depend on which ran before it.
    prime_allocator();
    for suite in &selected {
        write!(out, "{:<width$}  ", suite.name).map_err(output)?;
        out.flush().map_err(output)?;
        let summary = match run_suite(suite, settings) {
            Ok(summary) => summary,
            Err(error) => {
                writeln!(out, "FAILED: {}", printable(&error.to_string())).map_err(output)?;
                failed.push(suite.name.to_string());
                continue;
            }
        };
        let mut line = format!(
            "median {:>9}  p95 {:>9}  {:>4} samples",
            duration(summary.median_ns),
            duration(summary.p95_ns),
            summary.samples
        );
        if let Some(compared) = compared {
            match compared.suites.get(suite.name) {
                Some(entry) => {
                    match change_percent(entry.median_ns, summary.median_ns) {
                        Some(percent) => {
                            let _ = write!(line, "  {percent:+.1}%");
                        }
                        None => line.push_str("  (the baseline's median is 0 ns)"),
                    }
                    if is_slower(entry.median_ns, summary.median_ns) {
                        let _ = write!(line, "  SLOWER by more than {SLOWER_PERCENT}%");
                        slower.push(suite.name);
                    }
                }
                None => line.push_str("  (not in the baseline)"),
            }
        }
        writeln!(out, "{line}").map_err(output)?;
        out.flush().map_err(output)?;
        measured.push((suite.name.to_string(), summary));
    }

    let ran = measured.len();
    let mut last = format!("{ran} of {count} {suites_word} ran");
    if compared.is_some() {
        if slower.is_empty() {
            let _ = write!(
                last,
                "; no median is more than {SLOWER_PERCENT}% slower than the baseline's"
            );
        } else {
            let _ = write!(
                last,
                "; {} more than {SLOWER_PERCENT}% slower than the baseline's: {} (a change that \
                 does this says so and why, docs/04-quality.md)",
                slower.len(),
                slower.join(", ")
            );
        }
    }
    writeln!(out, "{last}").map_err(output)?;
    if !failed.is_empty() {
        return Err(BenchError::SuitesFailed { names: failed });
    }
    if let Some(machine) = machine {
        let name = machine.name.clone();
        let known: Vec<&str> = suites.iter().map(|suite| suite.name).collect();
        let merged = Baseline::merge(
            baseline,
            machine,
            measured
                .into_iter()
                .map(|(name, summary)| (name, Entry::from(summary))),
            &known,
        )
        .map_err(|detail| BenchError::OtherMachine {
            path: path.to_path_buf(),
            detail,
        })?;
        merged.write(path)?;
        writeln!(
            out,
            "saved {ran} {suites_word} to `{}`, recorded on `{}`; it holds {} now",
            shown(path),
            printable(&name),
            merged.suites.len()
        )
        .map_err(output)?;
    }
    out.flush().map_err(output)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::harness::Bencher;
    use crate::testing::{TempDir, facts};

    fn parse(args: &[&str]) -> Result<Options, String> {
        Options::parse(Args::new(args.iter().copied()).unwrap()).map_err(|e| e.to_string())
    }

    #[test]
    fn options_are_a_filter_and_save_with_a_machine() {
        assert_eq!(parse(&[]), Ok(Options::default()));
        assert_eq!(
            parse(&["text"]),
            Ok(Options {
                filter: Some("text".to_string()),
                ..Options::default()
            })
        );
        let saving = Options {
            filter: Some("text/rope".to_string()),
            save: true,
            machine: Some("MSI Pulse GL66".to_string()),
            help: false,
        };
        for args in [
            &["text/rope", "--save", "--machine", "MSI Pulse GL66"][..],
            &["--save", "--machine=MSI Pulse GL66", "text/rope"],
            &["--machine", "MSI Pulse GL66", "--save", "--", "text/rope"],
        ] {
            assert_eq!(parse(args), Ok(saving.clone()), "{args:?}");
        }
        // `--help` wins over anything else.
        for args in [&["--help"][..], &["-h"], &["text", "--bogus", "--help"]] {
            assert_eq!(
                parse(args),
                Ok(Options {
                    help: true,
                    ..Options::default()
                })
            );
        }
    }

    #[test]
    fn bad_arguments_are_usage_errors() {
        assert_eq!(
            parse(&["--save"]).unwrap_err(),
            "`--save` needs `--machine NAME`, the name of this machine, which the baseline \
             records"
        );
        assert_eq!(
            parse(&["--machine", "laptop"]).unwrap_err(),
            "`--machine` names the machine for `--save`, so it needs `--save` too"
        );
        assert_eq!(
            parse(&["--save", "--machine"]).unwrap_err(),
            "option `--machine` (argument 2) needs a value"
        );
        assert_eq!(
            parse(&["--save", "--machine= "]).unwrap_err(),
            "`--machine` needs a name, not a blank one"
        );
        assert_eq!(
            parse(&["--save", "--machine", "a\u{1b}[2K"]).unwrap_err(),
            "`--machine` needs a name without control characters"
        );
        assert_eq!(
            parse(&["text", "rope"]).unwrap_err(),
            "only one filter is taken, but `rope` came after the first"
        );
        assert_eq!(
            parse(&["--fast"]).unwrap_err(),
            "unexpected argument `--fast` (argument 1)"
        );
        assert_eq!(
            parse(&["--save", "--save", "--machine", "x"]).unwrap_err(),
            "unexpected argument `--save` (argument 2)"
        );
    }

    fn fast(b: &mut Bencher) -> Result<(), BenchError> {
        b.iter(|| std::hint::black_box(3_u64).pow(3));
        Ok(())
    }

    fn broken(_: &mut Bencher) -> Result<(), BenchError> {
        Err(BenchError::Check("its check failed".to_string()))
    }

    const FAKES: &[Suite] = &[
        Suite {
            name: "fake/a/one",
            run: fast,
        },
        Suite {
            name: "fake/a/two",
            run: fast,
        },
        Suite {
            name: "fake/b",
            run: fast,
        },
    ];

    #[test]
    fn the_filter_is_a_substring_and_must_match() {
        let names = |filter: Option<&str>| -> Vec<&str> {
            select(FAKES, filter)
                .unwrap()
                .iter()
                .map(|suite| suite.name)
                .collect()
        };
        assert_eq!(names(None), ["fake/a/one", "fake/a/two", "fake/b"]);
        assert_eq!(names(Some("fake")), ["fake/a/one", "fake/a/two", "fake/b"]);
        assert_eq!(names(Some("a/")), ["fake/a/one", "fake/a/two"]);
        assert_eq!(names(Some("two")), ["fake/a/two"]);
        assert_eq!(names(Some("")), ["fake/a/one", "fake/a/two", "fake/b"]);
        let error = select(FAKES, Some("nosuchsuite")).unwrap_err();
        assert!(
            matches!(&error, BenchError::NoSuiteMatches { filter } if filter == "nosuchsuite"),
            "{error:?}"
        );
        // Case matters, as in `cargo test`.
        assert!(select(FAKES, Some("FAKE")).is_err());
    }

    #[test]
    fn the_usage_lists_every_suite() {
        let text = usage(FAKES);
        assert!(text.starts_with("Usage: cargo xtask bench [FILTER] [--save --machine NAME]\n"));
        assert!(
            text.ends_with("Suites:\n  fake/a/one\n  fake/a/two\n  fake/b\n"),
            "{text}"
        );
    }

    #[test]
    fn a_median_more_than_ten_percent_slower_is_marked() {
        assert!(!is_slower(1_000, 1_000));
        assert!(!is_slower(1_000, 1_100), "exactly 10% is not more");
        assert!(is_slower(1_000, 1_101));
        assert!(!is_slower(1_000, 500), "faster is never slower");
        assert!(is_slower(0, 1));
        assert!(!is_slower(0, 0));
        assert!(!is_slower(u64::MAX, u64::MAX), "no overflow");
        assert_eq!(change_percent(1_000, 1_100), Some(10.0));
        assert_eq!(change_percent(1_000, 950), Some(-5.0));
        assert_eq!(change_percent(0, 5), None);
    }

    #[test]
    fn durations_are_shown_in_readable_units() {
        assert_eq!(duration(0), "0 ns");
        assert_eq!(duration(999), "999 ns");
        assert_eq!(duration(1_000), "1.00 µs");
        assert_eq!(duration(12_345), "12.3 µs");
        assert_eq!(duration(123_456), "123 µs");
        assert_eq!(duration(1_520_000), "1.52 ms");
        assert_eq!(duration(120_400_000), "120 ms");
        assert_eq!(duration(2_400_000_000), "2.40 s");
        assert_eq!(duration(u64::MAX), "18446744074 s");
    }

    /// Settings that finish at once.
    const QUICK: Settings = Settings {
        warm_up: Duration::ZERO,
        sample_target: Duration::from_micros(20),
        budget: Duration::ZERO,
        min_samples: 3,
        max_samples: 3,
    };

    /// Runs `options` on `suites` against the baseline at `path`, with
    /// `facts`; returns what it printed, or the error.
    fn report(
        args: &[&str],
        suites: &[Suite],
        path: &Path,
        facts: &Facts,
    ) -> Result<String, String> {
        let options = parse(args)?;
        let mut out = Vec::new();
        run(&options, suites, &QUICK, path, facts, &mut out).map_err(|e| e.to_string())?;
        Ok(String::from_utf8(out).unwrap())
    }

    /// A baseline of `entries` on the machine `name` with `facts`.
    fn write_baseline(path: &Path, name: &str, facts: Facts, entries: &[(&str, u64)]) {
        let baseline = Baseline {
            machine: Machine {
                name: name.to_string(),
                facts,
            },
            suites: entries
                .iter()
                .map(|&(suite, median_ns)| {
                    (
                        suite.to_string(),
                        Entry {
                            median_ns,
                            p95_ns: median_ns,
                            samples: 10,
                        },
                    )
                })
                .collect(),
        };
        baseline.write(path).unwrap();
    }

    #[test]
    fn without_a_baseline_a_run_only_reports() {
        let dir = TempDir::new("report-none");
        let path = dir.path().join("baseline.json");
        let text = report(&["fake/a"], FAKES, &path, &facts()).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "running 2 suites whose names contain `fake/a`");
        assert!(lines[1].starts_with("no baseline at `"), "{text}");
        assert!(
            lines[1].ends_with("baseline.json`: nothing to compare with"),
            "{text}"
        );
        let rows: Vec<&&str> = lines.iter().filter(|l| l.starts_with("fake/")).collect();
        assert_eq!(rows.len(), 2, "{text}");
        assert!(rows[0].starts_with("fake/a/one  median "), "{text}");
        assert!(rows[0].ends_with("   3 samples"), "{text}");
        assert_eq!(lines.last(), Some(&"2 of 2 suites ran"));
        assert!(!path.exists(), "a run without --save wrote the baseline");
    }

    /// A routine of at least 20 µs, whatever the machine: its median is
    /// above a 1 ns baseline and below a 1 s one.
    fn sleeps(b: &mut Bencher) -> Result<(), BenchError> {
        b.iter(|| std::thread::sleep(Duration::from_micros(20)));
        Ok(())
    }

    #[test]
    fn a_run_on_the_same_machine_shows_changes_and_marks_slower_medians() {
        let dir = TempDir::new("report-same");
        let path = dir.path().join("baseline.json");
        let suites = ["fake/zero", "fake/faster", "fake/slower", "fake/new"]
            .map(|name| Suite { name, run: sleeps });
        write_baseline(
            &path,
            "laptop",
            facts(),
            &[
                ("fake/zero", 0),
                ("fake/faster", 1_000_000_000),
                ("fake/slower", 1),
            ],
        );
        // The run succeeds (exit 0) with slower suites.
        let text = report(&[], &suites, &path, &facts()).unwrap();
        assert!(
            text.contains("comparing with `")
                && text.contains(
                    "`, recorded on `laptop` (linux x86_64, 16 threads, CPU \"11th Gen Intel(R) \
                     Core(TM) i7-11800H @ 2.30GHz\")\n"
                ),
            "{text}"
        );
        let row = |name: &str| {
            text.lines()
                .find(|line| line.starts_with(name))
                .unwrap_or_else(|| panic!("no row for {name}:\n{text}"))
                .to_string()
        };
        assert!(
            row("fake/zero")
                .ends_with(" samples  (the baseline's median is 0 ns)  SLOWER by more than 10%"),
            "{text}"
        );
        assert!(row("fake/faster").contains(" samples  -"), "{text}");
        let slower = row("fake/slower");
        assert!(slower.ends_with("%  SLOWER by more than 10%"), "{text}");
        assert!(slower.contains(" samples  +"), "{text}");
        assert!(
            row("fake/new").ends_with(" samples  (not in the baseline)"),
            "{text}"
        );
        assert!(
            text.ends_with(
                "4 of 4 suites ran; 2 more than 10% slower than the baseline's: fake/zero, \
                 fake/slower (a change that does this says so and why, docs/04-quality.md)\n"
            ),
            "{text}"
        );

        // Nothing slower: the last line says so.
        write_baseline(&path, "laptop", facts(), &[("fake/faster", 1_000_000_000)]);
        let text = report(&["faster"], &suites, &path, &facts()).unwrap();
        assert!(
            text.ends_with(
                "1 of 1 suite ran; no median is more than 10% slower than the baseline's\n"
            ),
            "{text}"
        );
    }

    #[test]
    fn a_baseline_from_another_machine_is_named_and_not_compared() {
        let dir = TempDir::new("report-other");
        let path = dir.path().join("baseline.json");
        let mut other = facts();
        other.threads = 4;
        write_baseline(&path, "desktop", other, &[("fake/b", 1)]);
        let text = report(&["fake/b"], FAKES, &path, &facts()).unwrap();
        assert!(
            text.contains(
                "the baseline was recorded on `desktop`, which is not this machine (linux \
                 x86_64, 4 threads, CPU"
            ),
            "{text}"
        );
        assert!(text.contains("no changes are shown"), "{text}");
        assert!(!text.contains('%'), "{text}");
        assert!(text.ends_with("\n1 of 1 suite ran\n"), "{text}");
    }

    #[test]
    fn save_merges_into_the_file_and_keeps_other_suites() {
        let dir = TempDir::new("report-save");
        let path = dir.path().join("baseline.json");
        // First save: only what ran.
        let text = report(
            &["fake/a", "--save", "--machine", "laptop"],
            FAKES,
            &path,
            &facts(),
        )
        .unwrap();
        assert!(text.contains("saved 2 suites to `"), "{text}");
        assert!(
            text.ends_with("`, recorded on `laptop`; it holds 2 now\n"),
            "{text}"
        );
        let saved = Baseline::read(&path).unwrap().unwrap();
        assert_eq!(saved.machine.name, "laptop");
        assert_eq!(saved.machine.facts, facts());
        let names: Vec<&str> = saved.suites.keys().map(String::as_str).collect();
        assert_eq!(names, ["fake/a/one", "fake/a/two"]);
        assert!(saved.suites.values().all(|entry| entry.samples == 3));

        // A suite that no longer exists is dropped; the others are kept.
        let mut edited = saved.clone();
        edited.suites.insert(
            "fake/gone".to_string(),
            Entry {
                median_ns: 5,
                p95_ns: 5,
                samples: 10,
            },
        );
        edited.write(&path).unwrap();
        report(
            &["fake/b", "--save", "--machine", "laptop"],
            FAKES,
            &path,
            &facts(),
        )
        .unwrap();
        let merged = Baseline::read(&path).unwrap().unwrap();
        let names: Vec<&str> = merged.suites.keys().map(String::as_str).collect();
        assert_eq!(names, ["fake/a/one", "fake/a/two", "fake/b"]);
        assert_eq!(merged.suites["fake/a/one"], saved.suites["fake/a/one"]);
    }

    #[test]
    fn save_refuses_another_machine_s_file_before_running() {
        let dir = TempDir::new("report-refuse");
        let path = dir.path().join("baseline.json");
        write_baseline(&path, "desktop", facts(), &[("fake/b", 7)]);
        let before = std::fs::read(&path).unwrap();
        let error = report(
            &["--save", "--machine", "laptop"],
            &[Suite {
                name: "fake/b",
                run: broken,
            }],
            &path,
            &facts(),
        )
        .unwrap_err();
        assert!(error.starts_with("refusing to merge into `"), "{error}");
        assert!(
            error.ends_with(
                "baseline.json`: it was recorded on `desktop`, not on `laptop`; one baseline \
                 never mixes machines, so move the file aside to record one on this machine"
            ),
            "{error}"
        );
        // The same name on other facts is refused too.
        let mut other = facts();
        other.cpu = "another CPU".to_string();
        let error = report(&["--save", "--machine", "desktop"], FAKES, &path, &other).unwrap_err();
        assert!(
            error.contains("it was recorded on `desktop` as linux x86_64"),
            "{error}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), before, "the file changed");
    }

    #[test]
    fn a_failed_suite_fails_the_run_after_the_others_and_saves_nothing() {
        let dir = TempDir::new("report-failed");
        let path = dir.path().join("baseline.json");
        let suites = [
            Suite {
                name: "fake/broken",
                run: broken,
            },
            Suite {
                name: "fake/fine",
                run: fast,
            },
        ];
        let options = parse(&["--save", "--machine", "laptop"]).unwrap();
        let mut out = Vec::new();
        let error = run(&options, &suites, &QUICK, &path, &facts(), &mut out).unwrap_err();
        assert_eq!(error.to_string(), "1 suite failed: fake/broken");
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.contains("fake/broken  FAILED: suite `fake/broken` failed: its check failed\n"),
            "{text}"
        );
        assert!(text.contains("fake/fine    median "), "{text}");
        assert!(text.ends_with("1 of 2 suites ran\n"), "{text}");
        assert!(
            !path.exists(),
            "a run with a failed suite saved the baseline"
        );
    }

    #[test]
    fn a_bad_baseline_or_filter_fails_before_any_suite_runs() {
        let dir = TempDir::new("report-bad");
        let path = dir.path().join("baseline.json");
        std::fs::write(&path, "{ not json").unwrap();
        let suites = [Suite {
            name: "fake/broken",
            run: broken,
        }];
        let options = Options::default();
        let mut out = Vec::new();
        let error = run(&options, &suites, &QUICK, &path, &facts(), &mut out).unwrap_err();
        assert!(matches!(error, BenchError::Parse { .. }), "{error:?}");
        assert!(out.is_empty(), "it ran before reading the baseline");

        let options = parse(&["nosuchsuite"]).unwrap();
        let error = run(&options, &suites, &QUICK, &path, &facts(), &mut out).unwrap_err();
        assert!(
            matches!(error, BenchError::NoSuiteMatches { .. }),
            "{error:?}"
        );
        assert!(out.is_empty());
    }
}
