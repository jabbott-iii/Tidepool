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

//! Project automation for the Tidepool repository: the checks behind
//! `cargo xtask ci`, and `cargo xtask fuzz-smoke`. Later roadmap tasks add
//! goldens, theme contrast, tool descriptors, benchmarks and packaging.
//!
//! Specification: `docs/04-quality.md` → "`xtask` checks". Run it as
//! `cargo xtask <command>`; `cargo xtask --help` lists the commands.
//!
//! Ported in part from Nadir's `xtask` (same owner); each ported file says
//! so in its header. Tidepool does not depend on Nadir.
//!
//! # What this tool may do
//!
//! `xtask` is dev-only tooling (layer 6 in `docs/01-architecture.md`): it
//! is never shipped in Tidepool, and it never uploads, publishes, signs,
//! reads secrets or calls the network (`docs/04-quality.md`). The one file
//! it writes in the repository is the generated Unicode tables, with `cargo
//! xtask ucd` (without `--check`).
//!
//! The one outside program it starts is `cargo`, which `cargo xtask ci`
//! exists to run (`cargo fmt`, clippy, the tests, `cargo metadata`, `cargo
//! locate-project`), and `cargo xtask fuzz-smoke` too (`cargo test` of each
//! fuzz target, ADR-0027). That is the documented role of `xtask`, so it spawns
//! cargo itself, with `std::process::Command`, rather than through
//! `tidepool-toolchain` (`AGENTS.md` rule 2), which manages the tools
//! Tidepool drives and which the dependency matrix does not let `xtask`
//! use. The cargo it runs is the one running it (the `CARGO` environment
//! variable), or `cargo` on the `PATH`. Every spawn is an argument vector,
//! never a shell; standard input is closed; and `CARGO_NET_OFFLINE=true` is
//! set in the child's environment, with `CARGO_TERM_COLOR=never` for the
//! children whose output it reads (`cargo metadata`, `cargo
//! locate-project`). `ci` compiles nothing (no clippy, no tests) once
//! `deps` has failed, so it never runs the build scripts of a graph it
//! rejected.
//!
//! # Exit status
//!
//! 0 when every check that ran passed, 1 when a check failed or could not
//! run (including a command that is not implemented yet), 2 when the
//! command line was not understood.

mod cargo;
mod check;
mod crates;
mod deps;
mod error;
mod fuzz;
mod headers;
mod layers;
mod lexer;
mod matrix;
mod metadata;
mod ucd;
mod unsafe_audit;
mod walk;
mod workflows;

use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use tidepool_core::CoreError;
use tidepool_core::cli::Args;

use crate::check::{CI, COMMANDS, LATER};
use crate::error::Error;

/// The exit code for a command line that was not understood.
const EXIT_USAGE: u8 = 2;

fn main() -> ExitCode {
    let args = Args::from_env().map_err(|error| Error::Usage(error.to_string()));
    let result = args.and_then(|args| run(args, &repository_root(), &mut io::stdout()));
    exit(result, &mut io::stderr())
}

/// Turns the result of [`run`] into the exit status, writing any error to
/// `err`.
fn exit(result: Result<bool, Error>, err: &mut dyn Write) -> ExitCode {
    // If standard error is closed, there is nobody left to tell; the exit
    // status still says what happened.
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error @ Error::Usage(_)) => {
            let message = error.to_string();
            let _ = writeln!(err, "xtask: {}\n\n{}", check::printable(&message), usage());
            ExitCode::from(EXIT_USAGE)
        }
        Err(error) => {
            let _ = writeln!(err, "xtask: {}", check::printable(&error.to_string()));
            ExitCode::FAILURE
        }
    }
}

/// Runs the command that `args` names on the repository at `default_root`,
/// or at `--root DIR`, made absolute ([`absolute_root`]). Returns whether it
/// passed.
fn run(mut args: Args, default_root: &Path, out: &mut dyn Write) -> Result<bool, Error> {
    let usage_error = |error: CoreError| Error::Usage(error.to_string());
    let command = args.subcommand();
    if args.flag("--help") || args.flag("-h") || command.as_deref() == Some("help") {
        out.write_all(usage().as_bytes()).map_err(Error::Output)?;
        out.flush().map_err(Error::Output)?;
        return Ok(true);
    }
    let root = match args.value("--root").map_err(usage_error)? {
        Some(dir) => PathBuf::from(dir),
        None => default_root.to_path_buf(),
    };
    let Some(command) = command else {
        args.finish().map_err(usage_error)?;
        return Err(Error::Usage("no command given".to_string()));
    };
    // A later command fails as not implemented whatever its options, so
    // that `cargo xtask golden --bless` says what is missing.
    if let Some(later) = LATER.iter().find(|later| later.name == command) {
        return Err(Error::NotImplemented {
            command: later.name,
            task: later.task,
        });
    }
    if command == "fuzz-smoke" {
        let options = fuzz::Options::parse(&mut args, fuzz::TARGETS, fuzz::clock_seed)?;
        if let Some(argument) = args.finish().map_err(usage_error)?.first() {
            return Err(Error::Usage(format!(
                "`fuzz-smoke` takes only options, but got `{argument}`"
            )));
        }
        let root = absolute_root(&root)?;
        let passed = fuzz::run(&options, fuzz::TARGETS, &root, out)?;
        out.flush().map_err(Error::Output)?;
        return Ok(passed);
    }
    // Only `ucd` and `fuzz-smoke` take options of their own; for any other
    // command `--check` is left for `finish` to reject.
    let check_only = command == "ucd" && args.flag("--check");
    let extra = args.finish().map_err(usage_error)?;
    if let Some(argument) = extra.first() {
        return Err(Error::Usage(format!(
            "`{command}` takes no arguments, but got `{argument}`"
        )));
    }
    let root = absolute_root(&root)?;
    let passed = if command == "ci" {
        check::run_ci(CI, LATER, &root, out)
    } else if command == "ucd" && !check_only {
        check::run_one(&check::UCD_WRITE, &root, out)
    } else if let Some(check) = COMMANDS.iter().find(|check| check.name == command) {
        check::run_one(check, &root, out)
    } else {
        return Err(Error::Usage(format!("unknown command `{command}`")));
    };
    let passed = passed.map_err(Error::Output)?;
    out.flush().map_err(Error::Output)?;
    Ok(passed)
}

/// `root` made absolute against the current directory, before any check
/// uses it: the cargo children run in the root and are given paths that
/// start with it, so a relative root (other than `.`) would name the wrong
/// place for them. `..` and links are left as they are.
///
/// # Errors
///
/// [`Error::Usage`] for an empty root (`--root=`); [`Error::Read`] if the
/// current directory cannot be read.
fn absolute_root(root: &Path) -> Result<PathBuf, Error> {
    if root.as_os_str().is_empty() {
        return Err(Error::Usage(
            "option `--root` needs a directory, not an empty value".to_string(),
        ));
    }
    std::path::absolute(root).map_err(|source| Error::Read {
        path: root.to_path_buf(),
        source,
    })
}

/// The repository root: the directory above this crate's manifest.
fn repository_root() -> PathBuf {
    let xtask = Path::new(env!("CARGO_MANIFEST_DIR"));
    xtask.parent().unwrap_or(xtask).to_path_buf()
}

/// The text of `--help`.
fn usage() -> String {
    let mut text = String::from(
        "Usage: cargo xtask <command> [--root DIR]\n\
         \n\
         Commands:\n",
    );
    let ci_steps: Vec<&str> = CI.iter().map(|check| check.name).collect();
    let ci_about = format!(
        "Run {} in that order, keeping going after a failure (but compiling nothing once deps \
         has failed)",
        ci_steps.join(", ")
    );
    let fuzz_about = format!(
        "Run each fuzz target for --secs (default {}) with mutated inputs; fail on a panic, a \
         hang, over-allocation or a death",
        fuzz::DEFAULT_SECS
    );
    let commands = [("ci", ci_about.as_str())]
        .into_iter()
        .chain(COMMANDS.iter().map(|check| (check.name, check.about)))
        .chain([("fuzz-smoke", fuzz_about.as_str())]);
    // Writing to a `String` cannot fail.
    for (name, about) in commands {
        let _ = writeln!(text, "  {name:<12}  {about}");
    }
    text.push_str("\nNot yet implemented:\n");
    for later in LATER {
        let _ = writeln!(text, "  {:<12}  {}", later.name, later.task);
    }
    text.push_str(
        "\nOptions:\n  \
         --root DIR    Check the repository at DIR instead of this one (for xtask's own tests)\n  \
         --check       ucd only: compare the tables instead of writing them\n  \
         --secs N      fuzz-smoke only: fuzz each target for N seconds\n  \
         --target T    fuzz-smoke only: fuzz only the target T\n  \
         --seed S      fuzz-smoke only: the seed of the inputs (decimal or 0x hex; default: \
         from the clock)\n  \
         --replay      fuzz-smoke only: save each input before it runs and check memory after \
         each, to name the one that kills the process or reserves too much\n  \
         --help        Show this text\n\
         \n\
         Exit status: 0 if every check passed, 1 if a check failed or could not run,\n\
         2 if the command line was not understood.\n",
    );
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs a command line that must be rejected, and returns the message.
    fn usage_error(args: &[&str]) -> String {
        let mut out = Vec::new();
        let args = Args::new(args.iter().copied()).expect("within the limits");
        match run(args, Path::new("."), &mut out) {
            Err(Error::Usage(message)) => {
                assert_eq!(out, b"", "a rejected command line printed something");
                message
            }
            other => panic!("expected a usage error, got {other:?}"),
        }
    }

    #[test]
    fn help_lists_every_command() {
        for help in [&["--help"][..], &["-h"], &["help"], &["deps", "--help"]] {
            let mut out = Vec::new();
            let args = Args::new(help.iter().copied()).unwrap();
            assert!(run(args, Path::new("."), &mut out).unwrap());
            assert_eq!(String::from_utf8(out).unwrap(), usage());
        }
        let text = usage();
        for name in [
            "ci",
            "deps",
            "layers",
            "crates",
            "unsafe-audit",
            "headers",
            "workflows",
            "tools",
            "theme",
            "ucd",
            "golden",
            "fuzz-smoke",
            "bench",
            "package",
        ] {
            assert!(
                text.lines()
                    .any(|line| line.starts_with(&format!("  {name} "))),
                "`{name}` is missing from:\n{text}"
            );
        }
        assert!(text.contains(
            "  ci            Run deps, fmt, clippy, test, layers, crates, unsafe-audit, headers, \
             workflows, ucd in that order"
        ));
        assert!(text.contains(
            "  ucd           crates/text/src/pos/grapheme/tables.rs is what crates/text/ucd/17.0.0 \
             generates (without --check: write it)\n"
        ));
        assert!(
            text.contains(
                "\n  --check       ucd only: compare the tables instead of writing them\n"
            )
        );
        // `ucd` and `fuzz-smoke` are implemented: neither is listed as not
        // yet implemented.
        let later = text
            .split("\nNot yet implemented:\n")
            .nth(1)
            .and_then(|rest| rest.split("\nOptions:\n").next())
            .unwrap();
        assert!(!later.contains("ucd"), "{later}");
        assert!(!later.contains("fuzz-smoke"), "{later}");
        assert!(
            text.contains("\n  --secs N      fuzz-smoke only: fuzz each target for N seconds\n"),
            "{text}"
        );
        assert!(
            text.contains("\n  --replay      fuzz-smoke only: "),
            "{text}"
        );
        assert!(text.contains("  tools         roadmap M5.1\n"));
    }

    #[test]
    fn a_missing_or_unknown_command_is_a_usage_error() {
        assert_eq!(usage_error(&[]), "no command given");
        assert_eq!(usage_error(&["frobnicate"]), "unknown command `frobnicate`");
        // Names are matched whole and case-sensitively.
        assert_eq!(usage_error(&["dep"]), "unknown command `dep`");
        assert_eq!(usage_error(&["CI"]), "unknown command `CI`");
        // The `fmt`, `clippy` and `test` steps of `ci` are not commands.
        assert_eq!(usage_error(&["fmt"]), "unknown command `fmt`");
        // `crates` is one since roadmap M0.9, but only in lower case.
        assert_eq!(usage_error(&["Crates"]), "unknown command `Crates`");
    }

    #[test]
    fn stray_arguments_are_usage_errors() {
        assert_eq!(
            usage_error(&["deps", "extra"]),
            "`deps` takes no arguments, but got `extra`"
        );
        assert_eq!(
            usage_error(&["ucd", "extra"]),
            "`ucd` takes no arguments, but got `extra`"
        );
        assert_eq!(
            usage_error(&["crates", "extra"]),
            "`crates` takes no arguments, but got `extra`"
        );
        assert_eq!(
            usage_error(&["ucd", "--check", "extra"]),
            "`ucd` takes no arguments, but got `extra`"
        );
        // `--check` belongs to `ucd` alone, and is taken once.
        assert_eq!(
            usage_error(&["deps", "--check"]),
            "unexpected argument `--check` (argument 2)"
        );
        assert_eq!(
            usage_error(&["ci", "--check"]),
            "unexpected argument `--check` (argument 2)"
        );
        assert_eq!(
            usage_error(&["ucd", "--check", "--check"]),
            "unexpected argument `--check` (argument 3)"
        );
        assert_eq!(
            usage_error(&["ucd", "--fix"]),
            "unexpected argument `--fix` (argument 2)"
        );
        assert_eq!(
            usage_error(&["ci", "--fast"]),
            "unexpected argument `--fast` (argument 2)"
        );
        assert_eq!(
            usage_error(&["--verbose"]),
            "unexpected argument `--verbose` (argument 1)"
        );
        assert_eq!(
            usage_error(&["ci", "--", "--help"]),
            "`ci` takes no arguments, but got `--help`"
        );
        assert_eq!(
            usage_error(&["layers", "--root"]),
            "option `--root` (argument 2) needs a value"
        );
        assert_eq!(
            usage_error(&["layers", "--root="]),
            "option `--root` needs a directory, not an empty value"
        );
    }

    #[test]
    fn later_commands_are_not_implemented_and_never_pass() {
        for (args, command, task) in [
            (&["tools"][..], "tools", "roadmap M5.1"),
            (&["theme"], "theme", "roadmap M3.2"),
            (&["golden", "--bless", "scene"], "golden", "roadmap M2.4"),
            (
                &["bench", "search"],
                "bench",
                "no roadmap task yet; first needed by M4.1",
            ),
            (
                &["package", "linux"],
                "package",
                "roadmap M11.6, M12.4 and M14.4",
            ),
        ] {
            let mut out = Vec::new();
            let result = run(
                Args::new(args.iter().copied()).unwrap(),
                Path::new("."),
                &mut out,
            );
            match result {
                Err(Error::NotImplemented {
                    command: c,
                    task: t,
                }) => {
                    assert_eq!((c, t), (command, task));
                }
                other => panic!("expected `{command}` to be not implemented, got {other:?}"),
            }
            assert!(out.is_empty());
        }
    }

    #[test]
    fn fuzz_smoke_takes_only_its_options() {
        assert_eq!(
            usage_error(&["fuzz-smoke", "extra"]),
            "`fuzz-smoke` takes only options, but got `extra`"
        );
        assert_eq!(
            usage_error(&["fuzz-smoke", "--target", "yaml"]),
            "unknown fuzz target `yaml`; the targets are: json, toml, encoding"
        );
        assert_eq!(
            usage_error(&["fuzz-smoke", "--secs", "soon"]),
            "invalid value `soon` for option `--secs` (argument 2)"
        );
        assert_eq!(
            usage_error(&["fuzz-smoke", "--seed", "0xnope"]),
            "option `--seed` needs a u64, in decimal or as 0x-prefixed hex, not `0xnope`"
        );
        assert_eq!(
            usage_error(&["fuzz-smoke", "--check"]),
            "unexpected argument `--check` (argument 2)"
        );
        // Its options belong to it alone.
        assert_eq!(
            usage_error(&["deps", "--secs", "5"]),
            "unexpected argument `--secs` (argument 2)"
        );
    }

    #[test]
    fn exit_statuses_are_0_1_and_2() {
        let mut err = Vec::new();
        assert_eq!(exit(Ok(true), &mut err), ExitCode::SUCCESS);
        assert_eq!(exit(Ok(false), &mut err), ExitCode::FAILURE);
        assert!(err.is_empty());
        assert_eq!(
            exit(Err(Error::Usage("no command given".to_string())), &mut err),
            ExitCode::from(2)
        );
        let text = String::from_utf8(std::mem::take(&mut err)).unwrap();
        assert!(
            text.starts_with("xtask: no command given\n\nUsage: cargo xtask"),
            "{text}"
        );
        assert_eq!(
            exit(
                Err(Error::NotImplemented {
                    command: "tools",
                    task: "roadmap M5.1"
                }),
                &mut err
            ),
            ExitCode::FAILURE
        );
        assert_eq!(
            String::from_utf8(std::mem::take(&mut err)).unwrap(),
            "xtask: `tools` is not yet implemented (roadmap M5.1)\n"
        );
        // A command line, like a path, reaches the terminal escaped.
        let forged = Error::Usage("unknown command `a\u{1b}[2K\rb`".to_string());
        assert_eq!(exit(Err(forged), &mut err), ExitCode::from(2));
        let text = String::from_utf8(err).unwrap();
        assert!(
            text.starts_with("xtask: unknown command `a\\u{1b}[2K\\rb`\n\nUsage:"),
            "{text}"
        );
    }

    #[test]
    fn a_command_runs_its_check_on_the_given_root() {
        // A directory that is not a repository: the audit runs, and fails
        // because the two crate roots are not there.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut out = Vec::new();
        let args = Args::new(["unsafe-audit", "--root", &root.to_string_lossy()]).unwrap();
        let passed = run(args, Path::new("/nonexistent"), &mut out).unwrap();
        assert!(!passed);
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.contains("violation: crates/sys/src/lib.rs was not found"),
            "{text}"
        );
        assert!(
            text.ends_with("unsafe-audit: FAILED (2 violations)\n"),
            "{text}"
        );
    }

    /// Runs `args` on the repository at `root`; returns whether it passed
    /// and what it printed.
    fn run_on(args: &[&str], root: &Path) -> (bool, String) {
        let mut args: Vec<String> = args.iter().map(ToString::to_string).collect();
        args.extend(["--root".to_string(), root.to_string_lossy().into_owned()]);
        let mut out = Vec::new();
        let passed = run(
            Args::new(args).unwrap(),
            Path::new("/nonexistent"),
            &mut out,
        )
        .unwrap();
        (passed, String::from_utf8(out).unwrap())
    }

    #[test]
    fn ucd_checks_with_check_and_writes_without() {
        let tree = ucd::testing::tree_with_inputs("main-ucd");
        let output = tree.path().join(ucd::OUTPUT);

        let (passed, text) = run_on(&["ucd", "--check"], tree.path());
        assert!(!passed, "{text}");
        assert!(
            text.ends_with(&format!(
                "violation: {} is missing; run cargo xtask ucd\nucd: FAILED (1 violation)\n",
                ucd::OUTPUT
            )),
            "{text}"
        );
        assert!(!output.exists(), "--check wrote the tables");

        let (passed, text) = run_on(&["ucd"], tree.path());
        assert!(passed, "{text}");
        assert!(
            text.ends_with(&format!("wrote {}\nucd: ok\n", ucd::OUTPUT)),
            "{text}"
        );
        assert!(output.is_file());

        // The option may come after `--root` too.
        let mut out = Vec::new();
        let root = tree.path().to_string_lossy().into_owned();
        let args = Args::new(["ucd", "--root", &root, "--check"]).unwrap();
        assert!(run(args, Path::new("/nonexistent"), &mut out).unwrap());
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.starts_with(&format!(
                "checked 5 files in {} against their SHA-256 pins\n",
                ucd::DIR
            )),
            "{text}"
        );
        assert!(
            text.ends_with("ranges (Unicode 17.0.0)\nucd: ok\n"),
            "{text}"
        );

        let (passed, text) = run_on(&["ucd"], tree.path());
        assert!(passed, "{text}");
        assert!(
            text.ends_with(&format!("{} is up to date\nucd: ok\n", ucd::OUTPUT)),
            "{text}"
        );
    }

    #[test]
    fn a_relative_root_is_resolved_against_the_current_directory() {
        // cargo runs tests in the package directory, so `..` is the
        // repository root. The cargo children run in the root: given `..`
        // as it is, they would look for `../Cargo.toml` above it.
        let mut out = Vec::new();
        let args = Args::new(["layers", "--root", ".."]).unwrap();
        let passed = run(args, Path::new("/nonexistent"), &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(passed, "{text}");
        assert!(text.ends_with("layers: ok\n"), "{text}");
    }

    #[test]
    fn the_repository_root_holds_the_workspace_manifest() {
        let manifest = std::fs::read_to_string(repository_root().join("Cargo.toml")).unwrap();
        assert!(manifest.contains("[workspace]"), "{manifest}");
    }
}
