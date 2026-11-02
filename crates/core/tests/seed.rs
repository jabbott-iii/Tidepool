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

//! The seed variable, end to end. A failing property prints
//! `to reproduce: TIDEPOOL_SEED=0x… cargo test …`; this test runs its own
//! test binary again with `TIDEPOOL_SEED` set to the printed value, and
//! checks that `check` and `check_sized` read the variable and fail with
//! the same report.
//!
//! The child process is this test binary itself, started with an argument
//! vector and no shell. It is not an outside program (AGENTS.md rule 2 has
//! those go through `tidepool-toolchain`); it is the only way to give
//! `check` a seed through the real environment, because changing this
//! process's environment needs `unsafe`, which this crate denies.

use std::panic::{self, AssertUnwindSafe};
use std::process::Command;

use tidepool_core::rng::Pcg32;
use tidepool_core::testing::{
    __check_sized_with_seed, __check_with_seed, DEFAULT_SEED, SEED_VAR, check, check_sized,
};

/// Set in the child to the entry point it must run: `check` or
/// `check_sized`.
const CHILD_VAR: &str = "TIDEPOOL_CORE_SEED_TEST_CHILD";

/// The test the child runs.
const CHILD_TEST: &str = "a_failing_property_for_the_child";

fn up_to_1000(rng: &mut Pcg32) -> u32 {
    rng.range(0..1000u32)
}

fn below_900(n: &u32) -> Result<(), String> {
    if *n < 900 {
        Ok(())
    } else {
        Err(format!("{n} is not below 900"))
    }
}

/// A list of `size` small numbers.
fn numbers(rng: &mut Pcg32, size: u32) -> Vec<u32> {
    (0..size).map(|_| rng.range(0..10u32)).collect()
}

/// A list must have fewer than five elements. A closure, because the
/// harness hands the property a `&Vec<u32>`, the generator's exact type.
fn shorter_than_5() -> impl Fn(&Vec<u32>) -> Result<(), String> {
    |list| {
        if list.len() < 5 {
            Ok(())
        } else {
            Err(format!("{} elements", list.len()))
        }
    }
}

/// Runs a failing property through the public entry point named by
/// [`CHILD_VAR`], which reads the seed from `TIDEPOOL_SEED`. In an ordinary
/// test run the variable is not set and this does nothing.
#[test]
fn a_failing_property_for_the_child() {
    match std::env::var(CHILD_VAR).as_deref() {
        Ok("check") => check("below 900", 500, up_to_1000, below_900),
        Ok("check_sized") => check_sized("short lists", 50, numbers, shorter_than_5()),
        _ => {}
    }
}

/// The report the explicit-seed runner gives for `entry` and `seed`, made
/// on a thread named like the child's test thread, so that the report
/// names the same test.
fn report(entry: &'static str, seed: u64) -> String {
    std::thread::Builder::new()
        .name(CHILD_TEST.to_string())
        .spawn(move || {
            let result = panic::catch_unwind(AssertUnwindSafe(|| match entry {
                "check" => __check_with_seed(seed, "below 900", 500, up_to_1000, below_900),
                _ => __check_sized_with_seed(seed, "short lists", 50, numbers, shorter_than_5()),
            }));
            let payload = result.expect_err("the property fails");
            match payload.downcast::<String>() {
                Ok(message) => *message,
                Err(_) => panic!("the report is a `String`"),
            }
        })
        .expect("spawn a thread")
        .join()
        .expect("the thread returns the report")
}

#[test]
fn the_printed_seed_reproduces_the_failure_through_the_variable() {
    if std::env::var_os(CHILD_VAR).is_some() {
        return; // This is the child; never start another.
    }
    let exe = std::env::current_exe().expect("the test binary's path");
    for entry in ["check", "check_sized"] {
        // Neither is the default seed, so a harness that ignored the
        // variable, or read another one, would report a different seed.
        for seed in [0x5, 0xdead_beef_cafe_f00d] {
            assert_ne!(seed, DEFAULT_SEED);
            let expected = report(entry, seed);
            // What a reader copies from the report.
            let printed = expected
                .rsplit_once(&format!("{SEED_VAR}="))
                .and_then(|(_, after)| after.split(' ').next())
                .expect("the report prints the seed");
            assert_eq!(printed, format!("0x{seed:016x}"));

            let output = Command::new(&exe)
                .args(["--exact", CHILD_TEST, "--nocapture", "--test-threads=1"])
                .env(SEED_VAR, printed)
                .env(CHILD_VAR, entry)
                .output()
                .expect("run this test binary again");
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                !output.status.success(),
                "`{entry}` with {SEED_VAR}={printed} passed:\n{stdout}\n{stderr}"
            );
            assert!(
                stderr.contains(&format!("{SEED_VAR}={printed} ")),
                "`{entry}` did not print {printed}:\n{stderr}"
            );
            assert!(
                stderr.contains(&expected),
                "`{entry}` with {SEED_VAR}={printed} reported differently.\n\
                 expected:\n{expected}\nchild's stderr:\n{stderr}"
            );
        }
    }
}
