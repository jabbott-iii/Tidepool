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

//! `cargo xtask site [--out DIR]`: builds the demo page that `pages.yml`
//! publishes at <https://jabbott.dev/Tidepool/> (ADR-0030), into
//! `target/site` or `DIR`.
//!
//! 1. Checks `docs/features.jsonc` as `cargo xtask features` does.
//! 2. Runs the gate in-process, the steps of `cargo xtask ci` with its
//!    runner ([`check::run_gate`]), printing what `ci` prints. It keeps
//!    each step's verdict and time, what libtest's `test result:` lines
//!    count and the `SKIPPED:` lines, and of what it printed only the
//!    summary, from its `== summary` line on: never a line that cargo or a
//!    test printed.
//! 3. Runs each of [`EXAMPLES`] with `cargo run --quiet` and keeps its
//!    standard output, which must be UTF-8.
//! 4. Fills `site/index.html` and `site/llms.txt` ([`page`]), and copies
//!    `site/tidepool.css` and `site/tidepool.js` beside them.
//!
//! It fails, writing nothing, when the output directory is not new, empty
//! or one an earlier run made, which it marks with [`MARK`], or holds a
//! directory where it writes a file; when the
//! `TIDEPOOL_SITE_*` variables are not all three or none, or one is not
//! valid ([`given`]); when the features, a step of the gate or an example
//! fail; or when a file of `site/` cannot be read or holds a placeholder
//! the page has no value for. It checks the directory, the variables, the
//! features and the templates before the gate runs. A run removes only
//! what an earlier one wrote ([`WRITTEN`]).
//!
//! The commit's facts come from the workflow, which reads them with `git`
//! ([`given`]). Without them, a local build, `site` reads HEAD's hash from
//! `.git` itself ([`head`]): it runs no program but `cargo`, makes no
//! network calls, and reads and writes only the workspace and the output
//! directory.
//!
//! Ported from nadir xtask `site.rs` (Nadir's ADR-0029), without its
//! captures, encoding, download and packaging. Adapted for Tidepool: the
//! gate and the examples in their place; the commit's facts from the
//! `TIDEPOOL_SITE_*` variables or `.git`, never from `git`; every file read
//! as `crates` reads one ([`crates::read_file`]), so no symbolic link is
//! followed; the output directory checked before anything runs, and
//! written only once everything else has passed; and the mark read without
//! following a link.

mod page;

use std::borrow::Cow;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use tidepool_codec::Limits;
use tidepool_codec::toml;
use tidepool_core::civil::DateTime;
use tidepool_core::cli::Args;

use crate::check::{self, CI, Check, LATER, Later};
use crate::error::Error;
use crate::features::{self, FEATURES, ROADMAP};
use crate::metadata::{self, Resolve};
use crate::{cargo, crates, walk};

use self::page::{Build, GateRun, Milestone, Page, Source};

/// Where the page goes, relative to the workspace root, without `--out`.
const OUTPUT: &str = "target/site";

/// The file that marks a directory as one `site` made, which a later run
/// may empty. Pages leaves out files whose names start with a dot.
const MARK: &str = ".tidepool-site";

/// Everything `site` writes into its directory besides [`MARK`]: all that
/// a later run removes.
const WRITTEN: &[&str] = &["index.html", "llms.txt", "tidepool.css", "tidepool.js"];

/// The page's own files, relative to the workspace root.
const TEMPLATE: &str = "site/index.html";
const TEXT_TEMPLATE: &str = "site/llms.txt";
const OWN_FILES: &[&str] = &["site/tidepool.css", "site/tidepool.js"];

/// The largest file of `site/` read, in bytes: the template is about
/// 50 KB.
const MAX_SITE_BYTES: u64 = 1024 * 1024;

/// The largest `rust-toolchain.toml`, `.git/HEAD` or loose ref read, in
/// bytes.
const MAX_SMALL_BYTES: u64 = 64 * 1024;

/// The largest `.git/packed-refs` read, in bytes.
const MAX_PACKED_REFS_BYTES: u64 = 16 * 1024 * 1024;

/// The variables that give the commit's facts, all three or none
/// (`docs/04-quality.md` → "Demo page").
pub(crate) const COMMIT: &str = "TIDEPOOL_SITE_COMMIT";
pub(crate) const DATE: &str = "TIDEPOOL_SITE_DATE";
pub(crate) const SUBJECT: &str = "TIDEPOOL_SITE_SUBJECT";

/// The longest subject, in characters, once trimmed.
const MAX_SUBJECT_CHARS: usize = 200;

/// A program of the workspace's whose output the page shows.
#[derive(Debug)]
pub(crate) struct Example {
    /// Its package.
    pub(crate) package: &'static str,
    /// Its name, as `--example` takes it.
    pub(crate) name: &'static str,
}

impl Example {
    /// The cargo arguments that run it.
    fn args(&self) -> [&str; 6] {
        ["run", "--quiet", "-p", self.package, "--example", self.name]
    }
}

/// The examples the page shows, in the order of its tabs after the gate's.
pub(crate) const EXAMPLES: [Example; 2] = [
    Example {
        package: "tidepool-codec",
        name: "formats",
    },
    Example {
        package: "tidepool-text",
        name: "editing",
    },
];

/// What `site` runs: the gate and a way to run an example. Its tests
/// replace both.
struct Steps<'a> {
    gate: &'a [Check],
    later: &'a [Later],
    run_example: fn(&Path, &Example) -> Result<Vec<u8>, Error>,
}

/// What the real command runs: `ci`'s steps, and cargo.
const STEPS: Steps<'static> = Steps {
    gate: CI,
    later: LATER,
    run_example,
};

/// Runs `cargo xtask site` on the repository at `root`, as given on the
/// command line.
pub(crate) fn command(mut args: Args, root: &Path, out: &mut dyn Write) -> Result<bool, Error> {
    let usage = |error: tidepool_core::CoreError| Error::Usage(error.to_string());
    let dir = args.value("--out").map_err(usage)?;
    if let Some(argument) = args.finish().map_err(usage)?.first() {
        return Err(Error::Usage(format!(
            "`site` takes only `--out DIR`, but got `{argument}`"
        )));
    }
    let root = crate::absolute_root(root)?;
    let output = match dir {
        Some(dir) if dir.is_empty() => {
            return Err(Error::Usage(
                "option `--out` needs a directory, not an empty value".to_string(),
            ));
        }
        Some(dir) => std::path::absolute(&dir).map_err(|source| Error::Read {
            path: PathBuf::from(&dir),
            source,
        })?,
        None => root.join(OUTPUT),
    };
    let given = given(|name| env::var_os(name))?;
    let passed = build(&root, &output, given, &STEPS, out)?;
    out.flush().map_err(Error::Output)?;
    Ok(passed)
}

/// Builds the page of the repository at `root` into `output`, with the
/// commit's facts `given` by the workflow, running `steps`.
fn build(
    root: &Path,
    output: &Path,
    given: Option<Source>,
    steps: &Steps,
    out: &mut dyn Write,
) -> Result<bool, Error> {
    check_output(output)?;

    let report = features::run(root)?;
    if !report.violations.is_empty() {
        for line in report.notes.iter().chain(&report.violations) {
            say(out, line)?;
        }
        say(
            out,
            &format!("FAILED: {FEATURES} does not pass `features`, so no page was written"),
        )?;
        return Ok(false);
    }
    let roadmap = features::read(root, ROADMAP)?;
    let tasks = features::tasks(&roadmap);
    let features: Vec<_> = features::parse(&features::read(root, FEATURES)?)
        .map_err(|problems| Error::Doc {
            path: FEATURES.to_string(),
            line: 0,
            detail: problems.join("; "),
        })?
        .into_iter()
        .map(|feature| {
            let status = features::status(&feature, &tasks);
            (feature, status)
        })
        .collect();
    let milestones = milestones(&roadmap, &tasks);
    let done = tasks.iter().filter(|task| task.done).count();

    let source = match given {
        Some(source) => source,
        None => {
            let commit = match head(root) {
                Ok(commit) => Some(commit),
                Err(reason) => {
                    say(
                        out,
                        &format!("HEAD's commit is unknown, and the page says so: {reason}"),
                    )?;
                    None
                }
            };
            Source::Local { commit }
        }
    };
    let build = build_info(root, source)?;
    let template = read(root, TEMPLATE)?;
    let text_template = read(root, TEXT_TEMPLATE)?;
    let own = OWN_FILES
        .iter()
        .map(|file| -> Result<(&str, String), Error> {
            Ok((file.rsplit('/').next().unwrap_or(file), read(root, file)?))
        })
        .collect::<Result<Vec<_>, Error>>()?;

    // Filled once before the gate, so that a placeholder the page has no
    // value for fails before anything runs.
    let render = |gate: &GateRun, formats: &str, editing: &str| -> Result<_, Error> {
        let page = Page {
            build: &build,
            gate,
            formats,
            editing,
            milestones: &milestones,
            tasks: (done, tasks.len()),
            features: &features,
        };
        let html = page::render(&template, &page).map_err(|detail| doc(TEMPLATE, detail))?;
        let text = page::render_text(&text_template, &page)
            .map_err(|detail| doc(TEXT_TEMPLATE, detail))?;
        Ok((html, text))
    };
    render(&GateRun::default(), "", "")?;

    say(out, "running the gate, the steps of `cargo xtask ci`")?;
    out.flush().map_err(Error::Output)?;
    let start = Instant::now();
    let mut tee = Tee {
        out: &mut *out,
        kept: Vec::new(),
    };
    let gate = check::run_gate(steps.gate, steps.later, root, &mut tee).map_err(Error::Output)?;
    let elapsed = start.elapsed();
    let printed = tee.kept;
    if !gate.passed() {
        say(out, "FAILED: the gate failed, so no page was written")?;
        return Ok(false);
    }
    let gate = gate_run(&gate, &printed, elapsed)?;

    let mut outputs = Vec::with_capacity(EXAMPLES.len());
    for example in &EXAMPLES {
        let args = example.args();
        say(out, &format!("running `{}`", cargo::display(&args)))?;
        out.flush().map_err(Error::Output)?;
        let bytes = (steps.run_example)(root, example)?;
        outputs.push(example_output(bytes, &args)?);
    }
    let (html, text) = render(&gate, &outputs[0], &outputs[1])?;

    prepare(output)?;
    write(&output.join("index.html"), html.as_bytes())?;
    write(&output.join("llms.txt"), text.as_bytes())?;
    for (name, text) in &own {
        write(&output.join(name), text.as_bytes())?;
    }
    say(
        out,
        &format!(
            "wrote {} for {}: {} of {} steps, {} tests passed, {} SKIPPED; {} features",
            walk::shown(&output.join("index.html")),
            build.short(),
            gate.steps.0,
            gate.steps.1,
            gate.passed,
            gate.skipped,
            features.len()
        ),
    )?;
    Ok(true)
}

/// Prints `line` as `site` says what it does, escaped as every message
/// is ([`check::printable`]).
fn say(out: &mut dyn Write, line: &str) -> Result<(), Error> {
    writeln!(out, "site: {}", check::printable(line)).map_err(Error::Output)
}

/// The error for the template `path`, which `detail` says is wrong.
fn doc(path: &str, detail: String) -> Error {
    Error::Doc {
        path: path.to_string(),
        line: 0,
        detail,
    }
}

/// Passes what it is given on to `out`, and keeps a copy.
struct Tee<'a> {
    out: &'a mut dyn Write,
    kept: Vec<u8>,
}

impl Write for Tee<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.out.write_all(bytes)?;
        self.kept.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}

/// What the page shows of the gate's run: the steps' count, the summary
/// that `printed` ends with, from its `== summary` line on, and the tests
/// libtest counted, less those that printed `SKIPPED:`, which libtest
/// counts as passed.
///
/// # Errors
///
/// [`Error::UnexpectedOutput`] if there is no summary, no `test result:`
/// line, one whose counts cannot be read, or more `SKIPPED:` lines than
/// passing tests: the page would show counts it cannot stand behind.
fn gate_run(
    gate: &check::Gate,
    printed: &[u8],
    elapsed: std::time::Duration,
) -> Result<GateRun, Error> {
    let unexpected = |detail: String| Error::UnexpectedOutput {
        command: "cargo xtask ci".to_string(),
        detail,
    };
    let printed = String::from_utf8_lossy(printed);
    let heading = format!("{}\n", check::SUMMARY);
    let at = if printed.starts_with(&heading) {
        Some(0)
    } else {
        printed.rfind(&format!("\n{heading}")).map(|at| at + 1)
    };
    let summary = at
        .map(|at| printed[at..].trim_end_matches('\n').to_string())
        .ok_or_else(|| unexpected(format!("it printed no `{}` line", check::SUMMARY)))?;
    let tests = gate.tests();
    if tests.results == 0 {
        return Err(unexpected(
            "its test step printed no `test result:` line to count the tests from".to_string(),
        ));
    }
    if tests.unreadable > 0 {
        return Err(unexpected(format!(
            "{} of its {} `test result:` lines do not give a number passed, failed and ignored",
            tests.unreadable, tests.results
        )));
    }
    let skipped = gate.skipped();
    let passed = tests.passed.checked_sub(skipped).ok_or_else(|| {
        unexpected(format!(
            "its tests printed {skipped} `SKIPPED:` lines, more than the {} tests libtest \
             counted as passed",
            tests.passed
        ))
    })?;
    Ok(GateRun {
        summary,
        steps: gate.steps(),
        passed,
        failed: tests.failed,
        ignored: tests.ignored,
        skipped,
        elapsed,
    })
}

/// Runs `example` in `root` with cargo, as every cargo child runs
/// ([`cargo::capture`]: an argument vector, offline, standard input
/// closed), and returns its standard output.
fn run_example(root: &Path, example: &Example) -> Result<Vec<u8>, Error> {
    cargo::capture(root, &example.args())
}

/// What an example run by `args` printed, `bytes`, as the page shows it:
/// UTF-8, without the one line break that ends it.
///
/// # Errors
///
/// [`Error::UnexpectedOutput`] if it is not UTF-8.
fn example_output(bytes: Vec<u8>, args: &[&str]) -> Result<String, Error> {
    let mut text = String::from_utf8(bytes).map_err(|_| Error::UnexpectedOutput {
        command: cargo::display(args),
        detail: "its output is not UTF-8".to_string(),
    })?;
    if text.ends_with('\n') {
        text.pop();
    }
    Ok(text)
}

/// The commit's facts from the `TIDEPOOL_SITE_*` variables, read with
/// `lookup`: `None` when none is set, a live build when all three are and
/// each is valid.
///
/// - [`COMMIT`]: 40 lowercase hex digits.
/// - [`DATE`]: the committer date, `YYYY-MM-DD`, a day of the calendar.
/// - [`SUBJECT`]: once trimmed, at most [`MAX_SUBJECT_CHARS`] characters,
///   holding none that `xtask` escapes in what it prints
///   ([`walk::escape_controls`]): no control character, so one line, and
///   no line or paragraph separator or bidirectional control either, which
///   would break or reorder the page's line. It may be blank, as `git`
///   allows.
///
/// # Errors
///
/// [`Error::Env`], naming the variable, never its value, when one or two
/// are set, or one is not valid.
fn given(lookup: impl Fn(&str) -> Option<OsString>) -> Result<Option<Source>, Error> {
    let names = [COMMIT, DATE, SUBJECT];
    let values = names.map(&lookup);
    if values.iter().all(Option::is_none) {
        return Ok(None);
    }
    let set: Vec<&str> = names
        .iter()
        .zip(&values)
        .filter(|(_, value)| value.is_some())
        .map(|(name, _)| *name)
        .collect();
    let [commit, date, subject] = values;
    let text = |variable: &'static str, value: Option<OsString>| {
        let value = value.ok_or_else(|| Error::Env {
            variable,
            problem: format!(
                "is not set, but {} {}: set all three of {COMMIT}, {DATE} and {SUBJECT}, or none",
                set.join(" and "),
                if set.len() == 1 { "is" } else { "are" }
            ),
        })?;
        value.into_string().map_err(|_| Error::Env {
            variable,
            problem: "is not UTF-8".to_string(),
        })
    };
    let (commit, date, subject) = (
        text(COMMIT, commit)?,
        text(DATE, date)?,
        text(SUBJECT, subject)?,
    );
    if !is_commit(&commit) {
        return Err(Error::Env {
            variable: COMMIT,
            problem: "is not a commit's full hash, 40 lowercase hex digits".to_string(),
        });
    }
    if !is_date(&date) {
        return Err(Error::Env {
            variable: DATE,
            problem: "is not a date written YYYY-MM-DD".to_string(),
        });
    }
    let subject = subject.trim();
    let problem = if subject.chars().count() > MAX_SUBJECT_CHARS {
        Some(format!(
            "is longer than {MAX_SUBJECT_CHARS} characters once trimmed"
        ))
    } else if matches!(walk::escape_controls(subject, |_| false), Cow::Owned(_)) {
        Some(
            "holds a control character, a line or paragraph separator or a bidirectional \
             control: it must be one line of plain text"
                .to_string(),
        )
    } else {
        None
    };
    if let Some(problem) = problem {
        return Err(Error::Env {
            variable: SUBJECT,
            problem,
        });
    }
    Ok(Some(Source::Live {
        commit,
        date,
        subject: subject.to_string(),
    }))
}

/// Whether `text` is a full SHA-1 commit hash: 40 lowercase hex digits.
fn is_commit(text: &str) -> bool {
    text.len() == 40
        && text
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// Whether `text` is a day of the calendar written `YYYY-MM-DD`.
fn is_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    let digits = |range: std::ops::Range<usize>| -> Option<u32> {
        let part = bytes.get(range)?;
        if !part.iter().all(u8::is_ascii_digit) {
            return None;
        }
        std::str::from_utf8(part).ok()?.parse().ok()
    };
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    let (Some(year), Some(month), Some(day)) = (digits(0..4), digits(5..7), digits(8..10)) else {
        return false;
    };
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&day)
}

/// HEAD's commit, read from the `.git` directory under `root` without
/// running `git` or following a symbolic link: `.git/HEAD` holds the hash,
/// or names a ref, whose hash is in its loose file under `.git` or on its
/// line of `.git/packed-refs`.
///
/// # Errors
///
/// Why the commit cannot be read that way, for a note: `.git` is missing,
/// a link or a file (as in a linked worktree or a submodule, whose HEAD is
/// elsewhere), or a file there is missing, a link, or not what `git`
/// writes. Then the page says the commit is unknown (ADR-0030).
fn head(root: &Path) -> Result<String, String> {
    match fs::symlink_metadata(root.join(".git")) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(".git is a symbolic link, which xtask does not follow".to_string());
        }
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => {
            return Err(
                ".git is a file, as in a linked worktree or a submodule, whose HEAD is \
                 elsewhere"
                    .to_string(),
            );
        }
        Err(error) => return Err(format!(".git cannot be read: {error}")),
    }
    let read_git = |rel: &str, limit| crates::read_file(root, &format!(".git/{rel}"), limit);
    let text = read_git("HEAD", MAX_SMALL_BYTES).map_err(|error| error.to_string())?;
    let line = only_line(&text).ok_or(".git/HEAD is not one line")?;
    if is_commit(line) {
        return Ok(line.to_string());
    }
    let name = line
        .strip_prefix("ref: ")
        .filter(|name| is_ref_name(name))
        .ok_or(".git/HEAD is neither a commit's hash nor a ref under refs/")?;
    match read_git(name, MAX_SMALL_BYTES) {
        Ok(text) => match only_line(&text) {
            Some(line) if is_commit(line) => Ok(line.to_string()),
            _ => Err(format!(".git/{name} is not a commit's full hash")),
        },
        Err(Error::Read { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
            let packed = read_git("packed-refs", MAX_PACKED_REFS_BYTES)
                .map_err(|error| format!("{name} has no loose file, and {error}"))?;
            packed_ref(&packed, name)
                .ok_or_else(|| format!("{name} is neither a loose ref nor in .git/packed-refs"))
        }
        Err(error) => Err(error.to_string()),
    }
}

/// The one line of `text`, a file `git` wrote, without its line break.
fn only_line(text: &str) -> Option<&str> {
    let line = text.strip_suffix('\n').unwrap_or(text);
    (!line.contains(['\n', '\r'])).then_some(line)
}

/// Whether `name` is a ref that may be read as a path under `.git`: under
/// `refs/`, its components not empty and not starting with `.`, of
/// characters `git` allows in a ref but for those a path could treat
/// differently.
fn is_ref_name(name: &str) -> bool {
    name.strip_prefix("refs/").is_some_and(|rest| {
        rest.split('/').all(|part| {
            !part.is_empty()
                && !part.starts_with('.')
                && !part.ends_with(".lock")
                && part.chars().all(|c| {
                    !c.is_control()
                        && !c.is_whitespace()
                        && !matches!(c, '\\' | ':' | '?' | '*' | '[' | '~' | '^')
                })
        })
    })
}

/// The hash `packed-refs` gives the ref `name`: its line `<hash> <name>`.
/// Comments (`#`) and peeled lines (`^`) are not refs.
fn packed_ref(packed: &str, name: &str) -> Option<String> {
    packed.lines().find_map(|line| {
        let (hash, other) = line.split_once(' ')?;
        (other == name && is_commit(hash)).then(|| hash.to_string())
    })
}

/// The workspace at `root`, for the commit of `source`.
fn build_info(root: &Path, source: Source) -> Result<Build, Error> {
    let toolchain = toolchain(&crates::read_file(
        root,
        "rust-toolchain.toml",
        MAX_SMALL_BYTES,
    )?)
    .ok_or_else(|| Error::Doc {
        path: "rust-toolchain.toml".to_string(),
        line: 0,
        detail: "names no `channel` in `[toolchain]`".to_string(),
    })?;
    let metadata = metadata::load(root, &root.join("Cargo.toml"), Resolve::Yes)?;
    let members: Vec<_> = metadata.packages.iter().filter(|p| p.is_member).collect();
    let version = match members.as_slice() {
        [first, rest @ ..] if rest.iter().all(|p| p.version == first.version) => {
            first.version.clone()
        }
        _ => {
            return Err(Error::UnexpectedOutput {
                command: "cargo metadata".to_string(),
                detail: "the workspace's members do not share one version".to_string(),
            });
        }
    };
    let year = match &source {
        Source::Live { date, .. } => date.get(..4).unwrap_or(date).to_string(),
        Source::Local { .. } => DateTime::now().year.to_string(),
    };
    Ok(Build {
        source,
        toolchain,
        version,
        crates: members.len(),
        third_party: metadata.packages.len() - members.len(),
        platform: platform(env::consts::OS, env::consts::ARCH),
        year,
    })
}

/// The `channel` that `rust-toolchain.toml`'s `[toolchain]` pins, if it is
/// one line of plain text.
fn toolchain(text: &str) -> Option<String> {
    let table = toml::parse(text, Limits::DEFAULT).ok()?;
    let channel = table.get("toolchain")?.get("channel")?.as_str()?;
    let plain = matches!(walk::escape_controls(channel, |_| false), Cow::Borrowed(_));
    (plain && !channel.trim().is_empty()).then(|| channel.to_string())
}

/// The OS and architecture `std::env::consts` names, as the page says
/// them: `Linux x86_64`, `macOS aarch64`, `Windows x86_64`.
fn platform(os: &str, arch: &str) -> String {
    let os = match os {
        "linux" => "Linux",
        "macos" => "macOS",
        "windows" => "Windows",
        other => other,
    };
    format!("{os} {arch}")
}

/// The roadmap's milestones, its `## M1 — Title` headings, with how many
/// of their tasks are ticked.
fn milestones(roadmap: &str, tasks: &[features::Task]) -> Vec<Milestone> {
    crates::roadmap_lines(roadmap)
        .filter_map(|line| {
            let (id, title) = line.strip_prefix("## ")?.split_once(" \u{2014} ")?;
            let number = id.strip_prefix('M')?;
            if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            let prefix = format!("{id}.");
            let own: Vec<_> = tasks
                .iter()
                .filter(|task| task.id.starts_with(&prefix))
                .collect();
            Some(Milestone {
                id: id.to_string(),
                title: title.to_string(),
                done: own.iter().filter(|task| task.done).count(),
                total: own.len(),
            })
        })
        .collect()
}

/// The text of the file at `rel` under `root`, read as `crates` reads one
/// ([`crates::read_file`]): a regular file, reached through no symbolic
/// link, of at most [`MAX_SITE_BYTES`], in UTF-8.
fn read(root: &Path, rel: &str) -> Result<String, Error> {
    crates::read_file(root, rel, MAX_SITE_BYTES)
}

/// Whether `dir` holds the mark of an earlier run, as a regular file, not
/// a link to one.
fn is_marked(dir: &Path) -> bool {
    fs::symlink_metadata(dir.join(MARK)).is_ok_and(|meta| meta.is_file())
}

/// Checks that `output` may take the page: it is new, empty, or one an
/// earlier run made, with no directory at a name of [`WRITTEN`]. Changes
/// nothing.
///
/// # Errors
///
/// [`Error::Usage`] for a directory that is none of those, or one an
/// earlier run made that holds a directory (not a link) where `site`
/// writes a file, so that `--out` never deletes what this tool did not
/// write; [`Error::Read`] if it cannot be listed, is not a directory, or
/// an entry at a name of [`WRITTEN`] cannot be looked at.
fn check_output(output: &Path) -> Result<(), Error> {
    match fs::read_dir(output) {
        Ok(mut entries) => {
            if entries.next().is_none() {
                return Ok(());
            }
            if !is_marked(output) {
                return Err(Error::Usage(format!(
                    "`{}` is not empty, and `cargo xtask site` did not make it: give a new or \
                     empty directory",
                    walk::shown(output)
                )));
            }
            for name in WRITTEN {
                written(&output.join(name))?;
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(Error::Read {
            path: output.to_path_buf(),
            source,
        }),
    }
}

/// What is at `path`, a name of [`WRITTEN`] in a directory an earlier run
/// made, looked at without following a link: `None` if nothing is, its
/// metadata if a file or a link is.
///
/// # Errors
///
/// [`Error::Usage`], naming it, if it is a directory: `site` only ever
/// writes a file there, so a directory there is someone else's, and is
/// never removed. [`Error::Read`] if it cannot be looked at.
fn written(path: &Path) -> Result<Option<fs::Metadata>, Error> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => Err(Error::Usage(format!(
            "`{}` is a directory, where `cargo xtask site` writes a file: move it, or give a \
             new or empty directory",
            walk::shown(path)
        ))),
        Ok(meta) => Ok(Some(meta)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(Error::Read {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Makes `output` ready for the page: a new or empty directory, or one an
/// earlier run made, without what that run wrote. Refuses any other
/// directory, and one that holds a directory where `site` writes a file
/// ([`check_output`]), removing nothing then.
fn prepare(output: &Path) -> Result<(), Error> {
    check_output(output)?;
    for name in WRITTEN {
        let path = output.join(name);
        // Looked at again: a directory that appeared since the check is
        // refused too, never emptied.
        let Some(meta) = written(&path)? else {
            continue;
        };
        // A file, or a link, which is removed as a link and never followed.
        // Windows removes a link to a directory, or a junction, only as a
        // directory: `remove_dir` removes the link, never what it leads to,
        // and never a directory that holds anything.
        let removed = fs::remove_file(&path).or_else(|error| {
            if meta.file_type().is_symlink() {
                fs::remove_dir(&path).map_err(|_| error)
            } else {
                Err(error)
            }
        });
        match removed {
            Err(error) if error.kind() != io::ErrorKind::NotFound => {
                return Err(Error::Write {
                    path,
                    source: error,
                });
            }
            _ => {}
        }
    }
    fs::create_dir_all(output).map_err(|source| Error::Write {
        path: output.to_path_buf(),
        source,
    })?;
    write(
        &output.join(MARK),
        b"Made by `cargo xtask site`, which empties this directory of what it wrote \
          when it runs again.\n",
    )
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    fs::write(path, bytes).map_err(|source| Error::Write {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cargo::Skips;
    use crate::cargo::TestCounts;
    use crate::check::{Report, Role, Step};
    use crate::walk::testing::{TempTree, symlink};

    const HASH: &str = "0123456789abcdef0123456789abcdef01234567";

    /// The variables `values` sets, as `given` reads them.
    fn lookup(values: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let values: Vec<(String, String)> = values
            .iter()
            .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
            .collect();
        move |name| {
            values
                .iter()
                .find(|(set, _)| set == name)
                .map(|(_, value)| OsString::from(value))
        }
    }

    fn env_error(values: &[(&str, &str)]) -> (&'static str, String) {
        match given(lookup(values)) {
            Err(Error::Env { variable, problem }) => (variable, problem),
            other => panic!("expected an error, got {other:?}"),
        }
    }

    const ALL: [(&str, &str); 3] = [
        (COMMIT, HASH),
        (DATE, "2026-11-08"),
        (SUBJECT, "  site: the demo page (M0.14)  "),
    ];

    #[test]
    fn the_commit_s_facts_are_all_three_variables_or_none() {
        assert_eq!(given(lookup(&[])).unwrap(), None);
        assert_eq!(
            given(lookup(&ALL)).unwrap(),
            Some(Source::Live {
                commit: HASH.to_string(),
                date: "2026-11-08".to_string(),
                subject: "site: the demo page (M0.14)".to_string(),
            })
        );
        // One or two: the first one missing is named, with those set.
        assert_eq!(
            env_error(&ALL[..1]),
            (
                DATE,
                "is not set, but TIDEPOOL_SITE_COMMIT is: set all three of \
                 TIDEPOOL_SITE_COMMIT, TIDEPOOL_SITE_DATE and TIDEPOOL_SITE_SUBJECT, or none"
                    .to_string()
            )
        );
        assert_eq!(env_error(&ALL[1..]).0, COMMIT);
        let (variable, problem) = env_error(&[ALL[0], ALL[1]]);
        assert_eq!(variable, SUBJECT);
        assert!(
            problem.starts_with("is not set, but TIDEPOOL_SITE_COMMIT and TIDEPOOL_SITE_DATE are:"),
            "{problem}"
        );
        // Set but empty counts as set, and is not valid.
        assert_eq!(env_error(&[(COMMIT, ""), ALL[1], ALL[2]]).0, COMMIT);
    }

    #[test]
    fn each_variable_is_checked_and_its_value_never_shown() {
        let with = |name: &str, value: &str| -> Vec<(&str, String)> {
            ALL.iter()
                .map(|(set, other)| {
                    (
                        *set,
                        if *set == name {
                            value.to_string()
                        } else {
                            (*other).to_string()
                        },
                    )
                })
                .collect()
        };
        let error_with = |name: &str, value: &str| {
            let values = with(name, value);
            let pairs: Vec<(&str, &str)> = values.iter().map(|(n, v)| (*n, v.as_str())).collect();
            let (variable, problem) = env_error(&pairs);
            let message = Error::Env { variable, problem }.to_string();
            if !value.trim().is_empty() {
                assert!(!message.contains(value.trim()), "{message}");
            }
            (variable, message)
        };
        for bad in [
            "0123456",
            "0123456789ABCDEF0123456789ABCDEF01234567",
            &format!("{HASH}0"),
            &format!("{HASH}\n"),
            &format!("{}g", &HASH[..39]),
            &"a".repeat(64),
        ] {
            let (variable, message) = error_with(COMMIT, bad);
            assert_eq!(variable, COMMIT);
            assert!(message.ends_with("40 lowercase hex digits"), "{message}");
        }
        for bad in [
            "2026-11-8",
            "2026/11/08",
            "2026-13-01",
            "2026-00-10",
            "2026-02-29",
            "2026-04-31",
            "2026-11-00",
            "20261108",
            "２０２６-11-08",
            "+026-11-08",
            "2026-11-08T10:00",
        ] {
            assert_eq!(error_with(DATE, bad).0, DATE, "{bad}");
        }
        for good in ["2024-02-29", "2000-02-29", "1999-12-31"] {
            let values = with(DATE, good);
            let pairs: Vec<(&str, &str)> = values.iter().map(|(n, v)| (*n, v.as_str())).collect();
            assert!(given(lookup(&pairs)).is_ok(), "{good}");
        }
        assert!(!is_date("1900-02-29"));
        let long = "é".repeat(MAX_SUBJECT_CHARS + 1);
        for bad in [
            "two\nlines",
            "a\rb",
            "tab\there",
            "esc \u{1b}[2K",
            "rtl \u{202e}txt",
            "line \u{2028} sep",
            long.as_str(),
        ] {
            assert_eq!(error_with(SUBJECT, bad).0, SUBJECT, "{bad:?}");
        }
        // 200 characters, not bytes, once trimmed.
        let most = format!(" {} ", "é".repeat(MAX_SUBJECT_CHARS));
        let values = with(SUBJECT, &most);
        let pairs: Vec<(&str, &str)> = values.iter().map(|(n, v)| (*n, v.as_str())).collect();
        assert!(given(lookup(&pairs)).is_ok());
        // Blank, as `git commit --allow-empty-message` makes it.
        let values = with(SUBJECT, "  ");
        let pairs: Vec<(&str, &str)> = values.iter().map(|(n, v)| (*n, v.as_str())).collect();
        assert!(given(lookup(&pairs)).is_ok_and(|given| matches!(
            given,
            Some(Source::Live { subject, .. }) if subject.is_empty()
        )));
        // HTML is fine here: the page escapes it.
        let values = with(SUBJECT, "<script>alert(1)</script> & co");
        let pairs: Vec<(&str, &str)> = values.iter().map(|(n, v)| (*n, v.as_str())).collect();
        assert!(given(lookup(&pairs)).is_ok());
    }

    /// A tree with `.git/HEAD` holding `head`.
    fn git(name: &str, head: &str) -> TempTree {
        let tree = TempTree::new(name);
        tree.write(".git/HEAD", head);
        tree
    }

    #[test]
    fn head_is_read_from_git_as_a_hash_a_loose_ref_or_a_packed_one() {
        let other = "fedcba9876543210fedcba9876543210fedcba98";
        // Detached.
        let tree = git("site-head-detached", &format!("{HASH}\n"));
        assert_eq!(head(tree.path()), Ok(HASH.to_string()));
        // A loose ref wins over a packed one.
        let tree = git("site-head-loose", "ref: refs/heads/main\n");
        tree.write(".git/refs/heads/main", format!("{HASH}\n"));
        tree.write(
            ".git/packed-refs",
            format!("# pack-refs with: peeled fully-peeled sorted\n{other} refs/heads/main\n"),
        );
        assert_eq!(head(tree.path()), Ok(HASH.to_string()));
        // Packed, between comments, other refs and peeled lines.
        let tree = git("site-head-packed", "ref: refs/heads/topic/a\n");
        tree.write(
            ".git/packed-refs",
            format!(
                "# pack-refs with: peeled fully-peeled sorted\n{other} refs/heads/main\n\
                 {HASH} refs/heads/topic/a\n^{other}\n{other} refs/tags/v1\n"
            ),
        );
        assert_eq!(head(tree.path()), Ok(HASH.to_string()));
    }

    #[test]
    fn head_that_cannot_be_read_that_way_is_unknown_with_the_reason() {
        let reason = |tree: &TempTree| head(tree.path()).expect_err("unknown");
        let tree = TempTree::new("site-head-none");
        assert!(reason(&tree).starts_with(".git cannot be read: "));
        // A linked worktree's `.git` is a file that points elsewhere.
        let tree = TempTree::new("site-head-worktree");
        tree.write(".git", "gitdir: /elsewhere/.git/worktrees/w\n");
        assert!(
            reason(&tree).starts_with(".git is a file"),
            "{}",
            reason(&tree)
        );
        for (name, text, expected) in [
            (
                "site-head-garbage",
                "hello\n",
                "neither a commit's hash nor a ref",
            ),
            (
                "site-head-two",
                "ref: refs/heads/main\nmore\n",
                "not one line",
            ),
            (
                "site-head-up",
                "ref: refs/../../secret\n",
                "neither a commit's hash",
            ),
            (
                "site-head-outside",
                "ref: HEAD\n",
                "neither a commit's hash",
            ),
            ("site-head-short", "0123456\n", "neither a commit's hash"),
            (
                "site-head-upper",
                "ref: refs/heads/.hidden\n",
                "neither a commit's hash",
            ),
        ] {
            let tree = git(name, text);
            let why = reason(&tree);
            assert!(why.contains(expected), "{name}: {why}");
        }
        // A ref that is neither loose nor packed, or whose file is not a hash.
        let tree = git("site-head-missing", "ref: refs/heads/main\n");
        assert!(
            reason(&tree).contains("has no loose file"),
            "{}",
            reason(&tree)
        );
        tree.write(".git/packed-refs", "# nothing\n");
        assert!(
            reason(&tree).contains("neither a loose ref nor in .git/packed-refs"),
            "{}",
            reason(&tree)
        );
        tree.write(".git/refs/heads/main", "ref: refs/heads/other\n");
        assert!(reason(&tree).contains("is not a commit's full hash"));
        // SHA-256 repositories are not read.
        tree.write(".git/refs/heads/main", format!("{}\n", "a".repeat(64)));
        assert!(reason(&tree).contains("is not a commit's full hash"));
    }

    #[test]
    fn head_follows_no_symbolic_link() {
        let tree = TempTree::new("site-head-links");
        let outside = TempTree::new("site-head-links-outside");
        outside.write(".git/HEAD", format!("{HASH}\n"));
        outside.write("refs/heads/main", format!("{HASH}\n"));
        if let Err(reason) = symlink(&tree, ".git", &outside.path().join(".git")) {
            let _ = writeln!(
                io::stderr(),
                "SKIPPED: head_follows_no_symbolic_link: cannot make a link here: {reason}"
            );
            return;
        }
        assert_eq!(
            head(tree.path()),
            Err(".git is a symbolic link, which xtask does not follow".to_string())
        );
        let tree = git("site-head-linked-ref", "ref: refs/heads/main\n");
        symlink(
            &tree,
            ".git/refs/heads/main",
            &outside.path().join("refs/heads/main"),
        )
        .expect("a second link");
        let why = head(tree.path()).expect_err("unknown");
        assert!(why.contains("symbolic link"), "{why}");
        let tree = TempTree::new("site-head-linked-head");
        symlink(&tree, ".git/HEAD", &outside.path().join(".git/HEAD")).expect("a third link");
        let why = head(tree.path()).expect_err("unknown");
        assert!(why.contains("symbolic link"), "{why}");
    }

    #[test]
    fn ref_names_cannot_leave_git() {
        for good in [
            "refs/heads/main",
            "refs/heads/topic/a-b_c.d",
            "refs/heads/é",
        ] {
            assert!(is_ref_name(good), "{good}");
        }
        for bad in [
            "HEAD",
            "refs/",
            "refs//x",
            "refs/heads/../../x",
            "refs/heads/./x",
            "refs/heads/a\\b",
            "refs/heads/C:x",
            "refs/heads/a b",
            "refs/heads/x.lock",
            "refs/heads/\u{1b}",
            "/refs/heads/main",
        ] {
            assert!(!is_ref_name(bad), "{bad}");
        }
    }

    #[test]
    fn the_toolchain_is_the_pinned_channel() {
        let text = "[toolchain]\nchannel = \"1.99.0\"\ncomponents = [\"rustfmt\"]\n";
        assert_eq!(toolchain(text), Some("1.99.0".to_string()));
        assert_eq!(toolchain("[toolchain]\n"), None);
        assert_eq!(toolchain("channel = \"1.99.0\"\n"), None);
        assert_eq!(toolchain("[toolchain]\nchannel = \"a\\nb\"\n"), None);
        assert_eq!(toolchain("not toml ["), None);
        let pinned = include_str!("../../rust-toolchain.toml");
        assert!(toolchain(pinned).is_some_and(|channel| !channel.is_empty()));
    }

    #[test]
    fn the_platform_is_named_as_the_page_says_it() {
        assert_eq!(platform("linux", "x86_64"), "Linux x86_64");
        assert_eq!(platform("macos", "aarch64"), "macOS aarch64");
        assert_eq!(platform("windows", "x86_64"), "Windows x86_64");
        assert_eq!(platform("freebsd", "x86_64"), "freebsd x86_64");
    }

    #[test]
    fn milestones_count_their_ticked_tasks() {
        let roadmap = "\
## M0 — Foundation and guardrails

- [x] **M0.1 — Workspace skeleton**
- [x] **M0.2 — core: errors, logging**

## M1 — Text engine and formats (headless)

- [x] **M1.1 — text: rope and positions**
- [ ] **M1.5 — text: search**

## Backlog (M20+, each needs a trigger)
";
        let tasks = features::tasks(roadmap);
        assert_eq!(
            milestones(roadmap, &tasks),
            [
                Milestone {
                    id: "M0".to_string(),
                    title: "Foundation and guardrails".to_string(),
                    done: 2,
                    total: 2,
                },
                Milestone {
                    id: "M1".to_string(),
                    title: "Text engine and formats (headless)".to_string(),
                    done: 1,
                    total: 2,
                },
            ]
        );
        // The real roadmap has its milestones, every task in one of them.
        let real = include_str!("../../docs/roadmap.md");
        let tasks = features::tasks(real);
        let found = milestones(real, &tasks);
        assert!(found.len() > 10, "{found:?}");
        assert_eq!(found[0].id, "M0");
        assert_eq!(
            found.iter().map(|milestone| milestone.total).sum::<usize>(),
            tasks.len()
        );
    }

    #[test]
    fn an_example_s_output_is_utf_8_less_one_line_break() {
        let args = EXAMPLES[0].args();
        assert_eq!(
            args,
            [
                "run",
                "--quiet",
                "-p",
                "tidepool-codec",
                "--example",
                "formats"
            ]
        );
        assert_eq!(
            example_output(b"a\n\nb\n\n".to_vec(), &args).unwrap(),
            "a\n\nb\n"
        );
        assert_eq!(example_output(b"a".to_vec(), &args).unwrap(), "a");
        assert_eq!(
            example_output("é 🌊\r\n".as_bytes().to_vec(), &args).unwrap(),
            "é 🌊\r"
        );
        match example_output(vec![b'a', 0xff, b'\n'], &args) {
            Err(error @ Error::UnexpectedOutput { .. }) => assert_eq!(
                error.to_string(),
                "unexpected output from `cargo run --quiet -p tidepool-codec --example \
                 formats`: its output is not UTF-8"
            ),
            other => panic!("expected an error, got {other:?}"),
        }
    }

    #[test]
    fn every_example_is_one_its_package_has() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        for example in &EXAMPLES {
            let dir = example.package.strip_prefix("tidepool-").unwrap();
            let path = root.join(format!("crates/{dir}/examples/{}.rs", example.name));
            assert!(path.is_file(), "{}", path.display());
        }
    }

    /// A fresh directory for test `name`.
    fn scratch(name: &str) -> TempTree {
        TempTree::new(&format!("site-{name}"))
    }

    #[test]
    fn the_output_must_be_new_empty_or_made_by_an_earlier_run() {
        let tree = scratch("prepare");
        let dir = tree.path();
        // New, then empty: made, and marked.
        let new = dir.join("new");
        check_output(&new).unwrap();
        prepare(&new).unwrap();
        assert!(new.join(MARK).is_file());
        let empty = dir.join("empty");
        fs::create_dir(&empty).unwrap();
        prepare(&empty).unwrap();
        assert!(empty.join(MARK).is_file());

        // Not empty, and not made by `site`: refused, and left alone.
        let other = dir.join("other");
        fs::create_dir_all(other.join("src")).unwrap();
        fs::write(other.join("index.html"), "mine").unwrap();
        for result in [check_output(&other), prepare(&other)] {
            match result {
                Err(Error::Usage(message)) => {
                    assert!(message.contains("is not empty"), "{message}");
                }
                result => panic!("expected a refusal, got {result:?}"),
            }
        }
        assert_eq!(
            fs::read_to_string(other.join("index.html")).unwrap(),
            "mine"
        );
        assert!(other.join("src").is_dir());

        // Made by `site`: what it wrote goes, and nothing else.
        for name in WRITTEN {
            fs::write(new.join(name), "old").unwrap();
        }
        fs::create_dir_all(new.join("index.html.d/deep")).unwrap();
        fs::write(new.join("notes.txt"), "kept").unwrap();
        prepare(&new).unwrap();
        for name in WRITTEN {
            assert!(!new.join(name).exists(), "{name} is still there");
        }
        assert_eq!(fs::read_to_string(new.join("notes.txt")).unwrap(), "kept");
        assert!(new.join("index.html.d/deep").is_dir());
        assert!(new.join(MARK).is_file());

        // Made by `site`, but with a directory where it writes a file,
        // which `site` never made: refused, naming it, and nothing is
        // removed, not even what `site` wrote at the other names.
        for name in ["index.html", "tidepool.js"] {
            for other in WRITTEN.iter().filter(|other| **other != name) {
                fs::write(new.join(other), "old").unwrap();
            }
            // The file the pass before wrote there, if any.
            let _ = fs::remove_file(new.join(name));
            fs::create_dir(new.join(name)).unwrap();
            fs::write(new.join(name).join("keep.txt"), "mine").unwrap();
            for result in [check_output(&new), prepare(&new)] {
                match result {
                    Err(Error::Usage(message)) => assert_eq!(
                        message,
                        format!(
                            "`{}` is a directory, where `cargo xtask site` writes a file: move \
                             it, or give a new or empty directory",
                            walk::shown(&new.join(name))
                        )
                    ),
                    result => panic!("expected a refusal, got {result:?}"),
                }
            }
            assert_eq!(
                fs::read_to_string(new.join(name).join("keep.txt")).unwrap(),
                "mine"
            );
            for other in WRITTEN.iter().filter(|other| **other != name) {
                assert_eq!(fs::read_to_string(new.join(other)).unwrap(), "old");
            }
            assert!(new.join(MARK).is_file());
            fs::remove_dir_all(new.join(name)).unwrap();
        }
        prepare(&new).unwrap();
        for name in WRITTEN {
            assert!(!new.join(name).exists(), "{name} is still there");
        }

        // A file is not a directory to write into.
        let file = dir.join("file");
        fs::write(&file, "x").unwrap();
        assert!(matches!(check_output(&file), Err(Error::Read { .. })));
    }

    #[test]
    fn a_linked_mark_does_not_make_a_directory_site_s() {
        let tree = scratch("linked-mark");
        let outside = scratch("linked-mark-outside");
        outside.write("mark", "x");
        tree.write("out/index.html", "mine");
        if let Err(reason) = symlink(&tree, &format!("out/{MARK}"), &outside.path().join("mark")) {
            let _ = writeln!(
                io::stderr(),
                "SKIPPED: a_linked_mark_does_not_make_a_directory_site_s: cannot make a link \
                 here: {reason}"
            );
            return;
        }
        assert!(matches!(
            check_output(&tree.path().join("out")),
            Err(Error::Usage(_))
        ));
    }

    #[test]
    fn a_link_where_site_writes_a_file_is_removed_as_a_link_never_followed() {
        let tree = scratch("written-links");
        let outside = scratch("written-links-outside");
        outside.write("dir/keep.txt", "mine");
        outside.write("file.txt", "mine too");
        tree.write(&format!("out/{MARK}"), "x");
        let linked = symlink(&tree, "out/index.html", &outside.path().join("dir"))
            .and_then(|()| symlink(&tree, "out/llms.txt", &outside.path().join("file.txt")));
        if let Err(reason) = linked {
            let _ = writeln!(
                io::stderr(),
                "SKIPPED: a_link_where_site_writes_a_file_is_removed_as_a_link_never_followed: \
                 cannot make a link here: {reason}"
            );
            return;
        }
        let out = tree.path().join("out");
        check_output(&out).unwrap();
        prepare(&out).unwrap();
        for name in ["index.html", "llms.txt"] {
            assert!(
                fs::symlink_metadata(out.join(name)).is_err(),
                "{name} is still there"
            );
        }
        assert_eq!(
            fs::read_to_string(outside.path().join("dir/keep.txt")).unwrap(),
            "mine"
        );
        assert_eq!(
            fs::read_to_string(outside.path().join("file.txt")).unwrap(),
            "mine too"
        );
    }

    #[test]
    fn site_takes_only_an_output_directory() {
        let mut out = Vec::new();
        let result = command(
            Args::new(["--out", "x", "extra"]).unwrap(),
            Path::new("."),
            &mut out,
        );
        match result {
            Err(Error::Usage(message)) => {
                assert_eq!(message, "`site` takes only `--out DIR`, but got `extra`");
            }
            other => panic!("expected a usage error, got {other:?}"),
        }
        let result = command(Args::new(["--out="]).unwrap(), Path::new("."), &mut out);
        match result {
            Err(Error::Usage(message)) => {
                assert_eq!(
                    message,
                    "option `--out` needs a directory, not an empty value"
                );
            }
            other => panic!("expected a usage error, got {other:?}"),
        }
        let result = command(
            Args::new(["--out", "a", "--out", "b"]).unwrap(),
            Path::new("."),
            &mut out,
        );
        assert!(matches!(result, Err(Error::Usage(_))), "{result:?}");
        assert!(out.is_empty());
    }

    // The whole build, on a tree of the real repository's `site/`,
    // `docs/` and `rust-toolchain.toml`, with the gate and the examples
    // replaced. `cargo metadata` runs on it, as `site` runs it.

    /// A workspace of one member, with the files `site` reads.
    fn workspace(name: &str) -> TempTree {
        let real = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let tree = scratch(name);
        tree.write(
            "Cargo.toml",
            "[workspace]\nresolver = \"3\"\nmembers = [\"one\"]\n\n[workspace.package]\n\
             version = \"0.1.0\"\n",
        );
        tree.write(
            "one/Cargo.toml",
            "[package]\nname = \"one\"\nversion.workspace = true\nedition = \"2024\"\n",
        );
        tree.write("one/src/lib.rs", "");
        tree.write(
            "Cargo.lock",
            "# This file is automatically @generated by Cargo.\n# It is not intended for manual \
             editing.\nversion = 4\n\n[[package]]\nname = \"one\"\nversion = \"0.1.0\"\n",
        );
        for file in [
            "rust-toolchain.toml",
            ROADMAP,
            FEATURES,
            TEMPLATE,
            TEXT_TEMPLATE,
            OWN_FILES[0],
            OWN_FILES[1],
        ] {
            tree.write(file, fs::read(real.join(file)).unwrap());
        }
        tree.write(".git/HEAD", format!("{HASH}\n"));
        tree
    }

    fn passing(_: &Path) -> Result<Report, Error> {
        Ok(Report::default())
    }

    fn failing(_: &Path) -> Result<Report, Error> {
        Ok(Report {
            violations: vec!["it is wrong".to_string()],
            ..Report::default()
        })
    }

    /// The test step: 7 passed, one of which printed `SKIPPED:`.
    fn tested(_: &Path) -> Result<Report, Error> {
        Ok(Report {
            skips: Skips {
                reasons: vec!["needs_git: TIDEPOOL_IT_GIT is not set".to_string()],
                count: 1,
            },
            tests: TestCounts {
                results: 2,
                unreadable: 0,
                passed: 7,
                failed: 0,
                ignored: 0,
            },
            notes: vec!["test result: ok. 7 passed; a line of cargo's".to_string()],
            ..Report::default()
        })
    }

    fn step(name: &'static str, run: fn(&Path) -> Result<Report, Error>) -> Check {
        Check {
            name,
            about: "demo",
            step: Step::Own(run),
            role: Role::Independent,
        }
    }

    /// The gate the tests run: `deps`, which gates, and the test step.
    const GATE: &[Check] = &[
        Check {
            name: "deps",
            about: "demo",
            step: Step::Own(passing),
            role: Role::Gate,
        },
        Check {
            name: "test",
            about: "demo",
            step: Step::Own(tested),
            role: Role::Independent,
        },
    ];

    fn printing(_: &Path, example: &Example) -> Result<Vec<u8>, Error> {
        Ok(format!("{} <says> & prints\u{1b}[0m\n", example.name).into_bytes())
    }

    fn not_utf_8(_: &Path, _: &Example) -> Result<Vec<u8>, Error> {
        Ok(vec![0xff])
    }

    fn exiting(_: &Path, example: &Example) -> Result<Vec<u8>, Error> {
        Err(Error::UnexpectedOutput {
            command: cargo::display(&example.args()),
            detail: "it exited with 1".to_string(),
        })
    }

    /// Runs `build` on `tree` into its `out`, and returns its result and
    /// what it printed.
    fn run(tree: &TempTree, given: Option<Source>, steps: &Steps) -> (Result<bool, Error>, String) {
        let mut out = Vec::new();
        let result = build(
            tree.path(),
            &tree.path().join("out"),
            given,
            steps,
            &mut out,
        );
        (result, String::from_utf8(out).unwrap())
    }

    #[test]
    fn a_build_writes_the_page_and_its_files_and_nothing_else() {
        let tree = workspace("build");
        let steps = Steps {
            gate: GATE,
            later: LATER,
            run_example: printing,
        };
        let live = Source::Live {
            commit: HASH.to_string(),
            date: "2026-11-08".to_string(),
            subject: "site: <the page> & more".to_string(),
        };
        let (result, printed) = run(&tree, Some(live), &steps);
        assert!(result.unwrap(), "{printed}");
        let out = tree.path().join("out");
        let mut names: Vec<String> = fs::read_dir(&out)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                MARK,
                "index.html",
                "llms.txt",
                "tidepool.css",
                "tidepool.js"
            ]
        );
        let html = fs::read_to_string(out.join("index.html")).unwrap();
        assert!(!html.contains("{{"));
        for expected in [
            "Live build",
            "<dd>2026-11-08</dd>",
            "site: &lt;the page&gt; &amp; more",
            "<dd>6 passed</dd>",
            "<div><dt>Skipped</dt><dd>1</dd></div>",
            "<dd>2 of 2 passed</dd>",
            "\n== summary\ndeps",
            "ci: ok (1 test SKIPPED): 2 of 2 steps passed; not yet implemented: tools, theme</pre>",
            "--example formats\nformats &lt;says&gt; &amp; prints\\u{1b}[0m</pre>",
            "--example editing\nediting &lt;says&gt; &amp; prints\\u{1b}[0m</pre>",
            "<dd>1 in the workspace</dd>",
        ] {
            assert!(html.contains(expected), "missing {expected:?}");
        }
        // No line the gate's steps printed reaches the page: only the summary.
        assert!(!html.contains("a line of cargo"));
        assert!(!html.contains("needs_git"));
        assert!(printed.contains("test result: ok. 7 passed; a line of cargo's"));
        let text = fs::read_to_string(out.join("llms.txt")).unwrap();
        assert!(text.contains("site: <the page> & more"), "{text}");
        assert!(
            text.contains("```\nformats <says> & prints\\u{1b}[0m\n```"),
            "{text}"
        );
        assert!(!text.contains("a line of cargo"));
        for file in OWN_FILES {
            let name = file.rsplit('/').next().unwrap();
            assert_eq!(
                fs::read(out.join(name)).unwrap(),
                fs::read(tree.path().join(file)).unwrap()
            );
        }
        assert!(printed.contains("site: wrote "), "{printed}");

        // Again, as a local build into the same directory: HEAD from .git.
        let (result, printed) = run(&tree, None, &steps);
        assert!(result.unwrap(), "{printed}");
        let html = fs::read_to_string(out.join("index.html")).unwrap();
        assert!(html.contains("Local build"));
        assert!(html.contains(">0123456</a>"));
        assert!(html.contains("<dd>Not given</dd>"));
    }

    /// Writes a page into `tree`'s `out`, then calls `spoil`, runs `build`
    /// with `steps`, which must fail, and checks the page was left alone.
    fn fails_writing_nothing(
        tree: &TempTree,
        steps: &Steps,
        spoil: impl FnOnce(),
    ) -> (Result<bool, Error>, String) {
        let out = tree.path().join("out");
        let ok = Steps {
            gate: GATE,
            later: &[],
            run_example: printing,
        };
        let (result, printed) = run(tree, None, &ok);
        assert!(result.unwrap(), "{printed}");
        let before = fs::read(out.join("index.html")).unwrap();
        spoil();
        let (result, printed) = run(tree, None, steps);
        assert!(!matches!(result, Ok(true)), "{printed}");
        assert_eq!(fs::read(out.join("index.html")).unwrap(), before);
        assert!(out.join(MARK).is_file());
        (result, printed)
    }

    #[test]
    fn a_failing_step_or_example_writes_nothing() {
        let tree = workspace("fails");
        // A step of the gate.
        let gate = [
            step("deps", passing),
            step("test", tested),
            step("layers", failing),
        ];
        let steps = Steps {
            gate: &gate,
            later: &[],
            run_example: printing,
        };
        let (result, printed) = fails_writing_nothing(&tree, &steps, || {});
        assert!(!result.unwrap());
        assert!(
            printed.ends_with("site: FAILED: the gate failed, so no page was written\n"),
            "{printed}"
        );
        assert!(!printed.contains("running `cargo run"), "{printed}");
        // An example that fails, or prints what is not UTF-8.
        for run_example in [exiting, not_utf_8] {
            let steps = Steps {
                gate: GATE,
                later: &[],
                run_example,
            };
            let (result, _) = fails_writing_nothing(&tree, &steps, || {});
            assert!(
                matches!(result, Err(Error::UnexpectedOutput { .. })),
                "{result:?}"
            );
        }
        // A gate whose test step counted no tests.
        let gate = [step("deps", passing), step("test", passing)];
        let steps = Steps {
            gate: &gate,
            later: &[],
            run_example: printing,
        };
        let (result, _) = fails_writing_nothing(&tree, &steps, || {});
        match result {
            Err(error) => assert!(
                error.to_string().contains("no `test result:` line"),
                "{error}"
            ),
            other => panic!("expected an error, got {other:?}"),
        }
    }

    /// A gate step that must not run: the build should have stopped first.
    fn unreachable(_: &Path) -> Result<Report, Error> {
        panic!("the gate ran");
    }

    #[test]
    fn the_features_and_the_templates_are_checked_before_the_gate_runs() {
        let tree = workspace("early");
        let steps = Steps {
            gate: &[Check {
                name: "deps",
                about: "demo",
                step: Step::Own(unreachable),
                role: Role::Gate,
            }],
            later: &[],
            run_example: printing,
        };
        // A placeholder the page has no value for.
        let good = fs::read_to_string(tree.path().join(TEMPLATE)).unwrap();
        let (result, _) = fails_writing_nothing(&tree, &steps, || {
            tree.write(TEMPLATE, good.replace("{{year}}", "{{yaer}}"));
        });
        match result {
            Err(error) => assert_eq!(
                error.to_string(),
                "site/index.html: the template's `{{yaer}}` has no value"
            ),
            other => panic!("expected an error, got {other:?}"),
        }
        tree.write(TEMPLATE, &good);
        let text = fs::read_to_string(tree.path().join(TEXT_TEMPLATE)).unwrap();
        let (result, _) = fails_writing_nothing(&tree, &steps, || {
            tree.write(TEXT_TEMPLATE, format!("{text}{{{{structured_data}}}}\n"));
        });
        assert!(result.is_err_and(|error| error.to_string().starts_with("site/llms.txt: ")));
        tree.write(TEXT_TEMPLATE, &text);
        // A feature the roadmap does not tick as it says.
        let features = fs::read_to_string(tree.path().join(FEATURES)).unwrap();
        let (result, printed) = fails_writing_nothing(&tree, &steps, || {
            tree.write(
                FEATURES,
                features.replacen("\"ticked\": [", "\"ticked\": [\"M99.1\", ", 1),
            );
        });
        assert!(!result.unwrap());
        assert!(printed.contains("site: docs/features.jsonc: "), "{printed}");
        assert!(
            printed.ends_with(
                "site: FAILED: docs/features.jsonc does not pass `features`, so no page was \
                 written\n"
            ),
            "{printed}"
        );
        tree.write(FEATURES, &features);
        // A directory `site` did not make.
        let other = tree.path().join("other");
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("index.html"), "mine").unwrap();
        let mut out = Vec::new();
        let result = build(tree.path(), &other, None, &steps, &mut out);
        assert!(matches!(result, Err(Error::Usage(_))), "{result:?}");
        assert_eq!(
            fs::read_to_string(other.join("index.html")).unwrap(),
            "mine"
        );
        // A directory `site` made, holding a directory where it writes a
        // file: refused before the gate, and kept.
        let marked = tree.path().join("out");
        fs::remove_file(marked.join("index.html")).unwrap();
        tree.write("out/index.html/keep.txt", "mine");
        let result = build(tree.path(), &marked, None, &steps, &mut out);
        match result {
            Err(Error::Usage(message)) => {
                assert!(message.ends_with("is a directory, where `cargo xtask site` writes a file: move it, or give a new or empty directory"), "{message}");
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
        assert_eq!(
            fs::read_to_string(marked.join("index.html/keep.txt")).unwrap(),
            "mine"
        );
        assert!(marked.join("llms.txt").is_file());
        assert!(out.is_empty());
    }

    #[test]
    fn a_local_build_whose_head_cannot_be_read_says_so() {
        let tree = workspace("unknown");
        fs::remove_dir_all(tree.path().join(".git")).unwrap();
        tree.write(".git", "gitdir: /elsewhere\n");
        let steps = Steps {
            gate: GATE,
            later: &[],
            run_example: printing,
        };
        let (result, printed) = run(&tree, None, &steps);
        assert!(result.unwrap(), "{printed}");
        assert!(
            printed.starts_with(
                "site: HEAD's commit is unknown, and the page says so: .git is a file"
            ),
            "{printed}"
        );
        let html = fs::read_to_string(tree.path().join("out/index.html")).unwrap();
        assert!(html.contains("Tidepool\">unknown</a>"));
    }
}
