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

//! The binary's command line, run as a process: its exit status and what
//! it writes. It runs only the binary under test, which Cargo builds for
//! this test, never an outside program, and only with arguments it
//! refuses, so it opens no window.

use std::process::Command;

/// Runs the binary with `args`: its exit code, standard output and
/// standard error.
fn run(args: &[&str]) -> (Option<i32>, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_tidepool"))
        .args(args)
        .output()
        .expect("the binary under test runs");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn an_argument_it_does_not_take_exits_2_with_the_usage() {
    let (code, out, err) = run(&["--tui"]);
    assert_eq!(code, Some(2), "{err}");
    assert_eq!(out, "");
    assert_eq!(
        err,
        "tidepool: unexpected argument `--tui` (argument 1)\n\
         usage: tidepool [--print-startup-time]\n"
    );

    let (code, out, err) = run(&["--print-startup-time", "notes.md"]);
    assert_eq!(code, Some(2), "{err}");
    assert_eq!(out, "", "nothing is timed when no window opens");
    assert!(
        err.contains("`notes.md`: Tidepool opens no files yet"),
        "{err}"
    );
}

#[test]
fn a_control_character_in_an_argument_is_shown_escaped() {
    let (code, _, err) = run(&["a\u{1b}[31mb"]);
    assert_eq!(code, Some(2), "{err}");
    assert!(err.contains("`a\\u{1b}[31mb`"), "{err}");
    assert!(!err.contains('\u{1b}'), "{err:?}");
}
