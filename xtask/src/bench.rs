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

//! `cargo xtask bench [args]`: builds `tidepool-bench` in the release
//! profile and runs it, with every argument after `bench` passed on
//! unchanged (ADR-0035).
//!
//! Written for Tidepool. The suites and the baseline are in
//! `tidepool-bench`, which `xtask` cannot link (its matrix row allows only
//! `core` and `codec`), so `xtask` starts one more cargo child: `cargo run
//! --release --quiet -p tidepool-bench -- [args]`, through [`cargo::run`],
//! in the root, with standard input closed, `CARGO_NET_OFFLINE=true`, and
//! its output going straight to the terminal. `xtask` reads none of it,
//! and passes on whether it succeeded. `bench` is not a step of `ci`.

use std::path::Path;

use crate::cargo;
use crate::error::Error;

/// The command's name.
pub(crate) const NAME: &str = "bench";

/// What `cargo` is given before the arguments passed on.
const CARGO_ARGS: [&str; 6] = ["run", "--release", "--quiet", "-p", "tidepool-bench", "--"];

/// The arguments after `bench`, if the command line is `bench …`: the
/// command must be the first argument, as every command's is.
pub(crate) fn arguments(command_line: &[String]) -> Option<&[String]> {
    match command_line.split_first() {
        Some((command, rest)) if command == NAME => Some(rest),
        _ => None,
    }
}

/// The arguments of the cargo child for `rest`, the arguments after
/// `bench`.
fn cargo_args(rest: &[String]) -> Vec<&str> {
    CARGO_ARGS
        .iter()
        .copied()
        .chain(rest.iter().map(String::as_str))
        .collect()
}

/// Runs `cargo run --release --quiet -p tidepool-bench -- <rest>` in `root`.
/// Returns whether it succeeded: `tidepool-bench` exits 1 for a bad
/// argument, a failed suite or a baseline it cannot read, and cargo fails
/// if the build does.
///
/// # Errors
///
/// [`Error::Spawn`] if cargo cannot be started.
pub(crate) fn run(root: &Path, rest: &[String]) -> Result<bool, Error> {
    cargo::run(root, &cargo_args(rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn only_a_command_line_that_starts_with_bench_is_bench() {
        let line = strings(&["bench", "text", "--save", "--machine", "MSI Pulse"]);
        assert_eq!(
            arguments(&line),
            Some(&strings(&["text", "--save", "--machine", "MSI Pulse"])[..])
        );
        assert_eq!(arguments(&strings(&["bench"])), Some(&[][..]));
        for other in [
            &["ci"][..],
            &[],
            &["--root", "x", "bench"],
            &["Bench"],
            &["benchmark"],
        ] {
            assert_eq!(arguments(&strings(other)), None, "{other:?}");
        }
    }

    #[test]
    fn every_argument_is_passed_on_unchanged_after_a_separator() {
        assert_eq!(
            cargo::display(&cargo_args(&[])),
            "cargo run --release --quiet -p tidepool-bench --"
        );
        // Options of xtask's own, `--` and arguments with spaces go to
        // the bench binary as they are, each one argument.
        let rest = strings(&["text", "--help", "--root", "x", "--", "a b", ""]);
        let args = cargo_args(&rest);
        assert_eq!(
            args,
            [
                "run",
                "--release",
                "--quiet",
                "-p",
                "tidepool-bench",
                "--",
                "text",
                "--help",
                "--root",
                "x",
                "--",
                "a b",
                ""
            ]
        );
    }
}
