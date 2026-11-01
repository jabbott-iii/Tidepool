//! Project automation for the Tidepool repository: the checks behind
//! `cargo xtask ci`. Later roadmap tasks add goldens, fuzz smoke, theme
//! contrast, tool descriptors, benchmarks and packaging.
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
//! reads secrets or calls the network (`docs/04-quality.md`).
//!
//! The one outside program it starts is `cargo`, which `cargo xtask ci`
//! exists to run (`cargo fmt`, clippy, the tests, `cargo metadata`, `cargo
//! locate-project`). That is the documented role of `xtask`, so it spawns
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
mod deps;
mod error;
mod layers;
mod lexer;
mod matrix;
mod metadata;
mod unsafe_audit;
mod walk;

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
            let _ = writeln!(err, "xtask: {error}\n\n{}", usage());
            ExitCode::from(EXIT_USAGE)
        }
        Err(error) => {
            let _ = writeln!(err, "xtask: {error}");
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
    let extra = args.finish().map_err(usage_error)?;
    if let Some(argument) = extra.first() {
        return Err(Error::Usage(format!(
            "`{command}` takes no arguments, but got `{argument}`"
        )));
    }
    let root = absolute_root(&root)?;
    let passed = if command == "ci" {
        check::run_ci(CI, LATER, &root, out)
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
    let commands = [("ci", ci_about.as_str())]
        .into_iter()
        .chain(COMMANDS.iter().map(|check| (check.name, check.about)));
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
            "unsafe-audit",
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
            "  ci            Run deps, fmt, clippy, test, layers, unsafe-audit in that order"
        ));
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
    }

    #[test]
    fn stray_arguments_are_usage_errors() {
        assert_eq!(
            usage_error(&["deps", "extra"]),
            "`deps` takes no arguments, but got `extra`"
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
            (&["ucd"], "ucd", "roadmap M1.1"),
            (&["golden", "--bless", "scene"], "golden", "roadmap M2.4"),
            (
                &["fuzz-smoke", "--secs", "60"],
                "fuzz-smoke",
                "roadmap M1.6",
            ),
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
            String::from_utf8(err).unwrap(),
            "xtask: `tools` is not yet implemented (roadmap M5.1)\n"
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
