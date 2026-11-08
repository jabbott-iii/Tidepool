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

//! A small property-test harness: [`check`], [`check_sized`] and
//! [`assert_approx!`](crate::assert_approx).
//!
//! Ported from nadir-core `testing.rs`. Adapted for Tidepool: the seed
//! variable is `TIDEPOOL_SEED`, the failure report ends with the exact
//! command that reproduces it, the explicit-seed runners are exposed for
//! the harness's own tests, and `assert_approx!` is new.
//!
//! A property test generates many random inputs from a seeded [`Pcg32`] and
//! checks that a property holds for each one:
//!
//! ```
//! use tidepool_core::testing::check;
//!
//! check(
//!     "addition commutes",
//!     200,
//!     |rng| (rng.range(-1000..1000), rng.range(-1000..1000)),
//!     |&(a, b)| if a + b == b + a { Ok(()) } else { Err(format!("{a} + {b}")) },
//! );
//! ```
//!
//! Runs are reproducible. The seed is [`DEFAULT_SEED`] unless the
//! [`SEED_VAR`] environment variable (`TIDEPOOL_SEED`) is set. A failure
//! panics with the seed, the case number, the (smallest) failing input and
//! the property's message, and ends with the command to run it again:
//!
//! ```text
//! property `rope matches model` failed at case 17 of 500 (seed 0x544944455…)
//!   error: …
//!   input: …
//!   to reproduce: TIDEPOOL_SEED=0x5449444550… cargo test -p tidepool-text rope::tests::matches_model
//! ```
//!
//! The package comes from `CARGO_PKG_NAME`, which `cargo test` sets for the
//! test process, and the test filter from the name of the current thread,
//! which libtest sets to the test's path. When either is unavailable the
//! report says `<package>` or uses the property's name instead.

use std::any::Any;
use std::fmt::{self, Debug};
use std::panic::{self, AssertUnwindSafe};

use crate::hash::fnv1a64;
use crate::rng::Pcg32;

pub mod fuzz;

/// The environment variable that overrides the seed: a `u64`, in decimal or
/// in hex with a `0x` prefix.
pub const SEED_VAR: &str = "TIDEPOOL_SEED";

/// The seed used when [`SEED_VAR`] is not set, so that ordinary test runs
/// are reproducible. It is `"TIDEPOOL"` in ASCII.
pub const DEFAULT_SEED: u64 = 0x5449_4445_504f_4f4c;

/// The largest size [`check_sized`] passes to a generator.
pub const MAX_SIZE: u32 = 100;

/// Checks that `property` holds for `cases` inputs made by `generate`.
///
/// `property` returns `Err` with an explanation when it does not hold; it
/// may also simply panic. Each case draws from its own random stream, so a
/// case's input does not depend on how much randomness earlier cases used.
///
/// # Panics
///
/// Panics on the first failing case, with the seed, case number, input and
/// the command that reproduces it. Also panics if `TIDEPOOL_SEED` is set but
/// is not a `u64`.
#[track_caller]
pub fn check<T: Debug>(
    name: &str,
    cases: u32,
    generate: impl Fn(&mut Pcg32) -> T,
    property: impl Fn(&T) -> Result<(), String>,
) {
    __check_with_seed(
        seed_from(std::env::var(SEED_VAR)),
        name,
        cases,
        generate,
        property,
    );
}

/// Like [`check`], for inputs that have a natural size, such as a list of
/// operations.
///
/// `generate` also receives a size between 0 and [`MAX_SIZE`], and should
/// produce an input about that large: for example a list of that many
/// operations. Sizes grow over the run, from 0 in the first case to
/// `MAX_SIZE` in the last.
///
/// When a case fails, the harness tries the same case again at every
/// smaller size and reports the smallest size that still fails. This is all
/// the shrinking there is, so it helps only if a smaller size gives a
/// simpler input.
///
/// # Panics
///
/// As for [`check`].
#[track_caller]
pub fn check_sized<T: Debug>(
    name: &str,
    cases: u32,
    generate: impl Fn(&mut Pcg32, u32) -> T,
    property: impl Fn(&T) -> Result<(), String>,
) {
    __check_sized_with_seed(
        seed_from(std::env::var(SEED_VAR)),
        name,
        cases,
        generate,
        property,
    );
}

/// [`check`] with an explicit seed instead of [`SEED_VAR`].
///
/// For the harness's own tests, which must not set environment variables:
/// they run a failing property, read the seed from the report, and run it
/// again with that seed.
///
/// # Panics
///
/// As for [`check`].
#[doc(hidden)]
#[track_caller]
pub fn __check_with_seed<T: Debug>(
    seed: u64,
    name: &str,
    cases: u32,
    generate: impl Fn(&mut Pcg32) -> T,
    property: impl Fn(&T) -> Result<(), String>,
) {
    let run = Run::new(name, seed, cases);
    if let Err(failure) = run.unsized_cases(&generate, &property) {
        panic!("{failure}");
    }
}

/// [`check_sized`] with an explicit seed instead of [`SEED_VAR`]. See
/// [`__check_with_seed`].
///
/// # Panics
///
/// As for [`check`].
#[doc(hidden)]
#[track_caller]
pub fn __check_sized_with_seed<T: Debug>(
    seed: u64,
    name: &str,
    cases: u32,
    generate: impl Fn(&mut Pcg32, u32) -> T,
    property: impl Fn(&T) -> Result<(), String>,
) {
    let run = Run::new(name, seed, cases);
    if let Err(failure) = run.sized_cases(&generate, &property) {
        panic!("{failure}");
    }
}

/// Floating-point types that [`assert_approx!`](crate::assert_approx) can
/// compare.
pub trait Approx: Copy + Debug {
    /// The tolerance used when the macro is not given one.
    const DEFAULT_TOLERANCE: Self;

    /// Returns `true` if `self` and `other` differ by at most `tolerance`.
    /// Equal infinities are equal; NaN equals nothing.
    fn approx_eq(self, other: Self, tolerance: Self) -> bool;
}

impl Approx for f32 {
    const DEFAULT_TOLERANCE: Self = 1e-5;

    fn approx_eq(self, other: Self, tolerance: Self) -> bool {
        self == other || (self - other).abs() <= tolerance
    }
}

impl Approx for f64 {
    const DEFAULT_TOLERANCE: Self = 1e-9;

    fn approx_eq(self, other: Self, tolerance: Self) -> bool {
        self == other || (self - other).abs() <= tolerance
    }
}

/// Asserts that two floating-point values differ by at most a tolerance.
///
/// The tolerance is absolute. Without one, it is
/// [`Approx::DEFAULT_TOLERANCE`]: `1e-5` for `f32`, `1e-9` for `f64`. A
/// message in `format!` syntax may follow the tolerance.
///
/// ```
/// use tidepool_core::assert_approx;
///
/// assert_approx!(0.1f32 + 0.2, 0.3);
/// assert_approx!(1.0f64, 1.05, 0.1);
/// assert_approx!(2.0f32, 2.0, 0.0, "scale {} at {}", 2, "origin");
/// ```
///
/// # Panics
///
/// Panics if the values differ by more than the tolerance, or either is
/// NaN.
#[macro_export]
macro_rules! assert_approx {
    ($left:expr, $right:expr $(,)?) => {
        $crate::testing::__assert_approx(
            $left,
            $right,
            ::core::option::Option::None,
            ::core::option::Option::None,
        )
    };
    ($left:expr, $right:expr, $tolerance:expr $(,)?) => {
        $crate::testing::__assert_approx(
            $left,
            $right,
            ::core::option::Option::Some($tolerance),
            ::core::option::Option::None,
        )
    };
    ($left:expr, $right:expr, $tolerance:expr, $($arg:tt)+) => {
        $crate::testing::__assert_approx(
            $left,
            $right,
            ::core::option::Option::Some($tolerance),
            ::core::option::Option::Some(::core::format_args!($($arg)+)),
        )
    };
}

#[doc(inline)]
pub use crate::assert_approx;

/// The function behind [`assert_approx!`](crate::assert_approx).
///
/// # Panics
///
/// As for the macro.
#[doc(hidden)]
#[track_caller]
pub fn __assert_approx<T: Approx>(
    left: T,
    right: T,
    tolerance: Option<T>,
    message: Option<fmt::Arguments<'_>>,
) {
    let tolerance = tolerance.unwrap_or(T::DEFAULT_TOLERANCE);
    if !left.approx_eq(right, tolerance) {
        match message {
            Some(message) => panic!(
                "assertion `left ≈ right` failed: {message}\n  left: {left:?}\n right: {right:?}\n tolerance: {tolerance:?}"
            ),
            None => panic!(
                "assertion `left ≈ right` failed\n  left: {left:?}\n right: {right:?}\n tolerance: {tolerance:?}"
            ),
        }
    }
}

/// One invocation of [`check`] or [`check_sized`].
struct Run<'a> {
    name: &'a str,
    seed: u64,
    cases: u32,
    /// How the report tells the reader to reproduce the failure.
    reproduce: Reproduce,
}

/// The parts of the `cargo test` command line in a failure report.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Reproduce {
    package: String,
    filter: String,
}

impl Reproduce {
    /// Works out the package and test filter for the running test. See the
    /// module documentation.
    fn current(name: &str) -> Self {
        let package = std::env::var("CARGO_PKG_NAME")
            .ok()
            .filter(|package| !package.is_empty())
            .unwrap_or_else(|| "<package>".to_string());
        let thread = std::thread::current();
        let filter = match thread.name() {
            Some(test) if test != "main" && !test.is_empty() => test.to_string(),
            _ => format!("{name:?}"),
        };
        Self { package, filter }
    }
}

impl<'a> Run<'a> {
    fn new(name: &'a str, seed: u64, cases: u32) -> Self {
        Self {
            name,
            seed,
            cases,
            reproduce: Reproduce::current(name),
        }
    }

    fn unsized_cases<T: Debug>(
        &self,
        generate: &impl Fn(&mut Pcg32) -> T,
        property: &impl Fn(&T) -> Result<(), String>,
    ) -> Result<(), Box<Failure>> {
        for case in 0..self.cases {
            self.run_case(case, generate, property)
                .map_err(|(input, message)| self.failure(case, None, input, message))?;
        }
        Ok(())
    }

    fn sized_cases<T: Debug>(
        &self,
        generate: &impl Fn(&mut Pcg32, u32) -> T,
        property: &impl Fn(&T) -> Result<(), String>,
    ) -> Result<(), Box<Failure>> {
        for case in 0..self.cases {
            let size = self.size_of_case(case);
            let Err(mut found) = self.run_case(case, |rng| generate(rng, size), property) else {
                continue;
            };
            // Shrink: the smallest size at which this same case still fails.
            let mut shrunk_to = size;
            for smaller in 0..size {
                if let Err(simpler) = self.run_case(case, |rng| generate(rng, smaller), property) {
                    (found, shrunk_to) = (simpler, smaller);
                    break;
                }
            }
            let sizes = Sizes {
                failed_at: size,
                shrunk_to,
            };
            return Err(self.failure(case, Some(sizes), found.0, found.1));
        }
        Ok(())
    }

    /// Sizes ramp linearly from 0 in the first case to `MAX_SIZE` in the
    /// last.
    fn size_of_case(&self, case: u32) -> u32 {
        match self.cases.saturating_sub(1) {
            0 => MAX_SIZE,
            // At most `MAX_SIZE`, because `case <= last`.
            last => (u64::from(case) * u64::from(MAX_SIZE) / u64::from(last)) as u32,
        }
    }

    /// The generator for one case. It depends only on the seed, the
    /// property's name and the case number.
    fn case_rng(&self, case: u32) -> Pcg32 {
        Pcg32::new(self.seed ^ fnv1a64(self.name.as_bytes()), u64::from(case))
    }

    /// Runs one case. On failure, returns the input (as `Debug` text) and
    /// the reason.
    fn run_case<T: Debug>(
        &self,
        case: u32,
        generate: impl FnOnce(&mut Pcg32) -> T,
        property: &impl Fn(&T) -> Result<(), String>,
    ) -> Result<(), (String, String)> {
        // A panic is a failure like any other, and needs the seed in its
        // report just as much. The closures are not used again after a
        // panic, so observing broken state through them is not a concern.
        let mut rng = self.case_rng(case);
        let input =
            panic::catch_unwind(AssertUnwindSafe(|| generate(&mut rng))).map_err(|payload| {
                let message = format!("the generator panicked: {}", panic_message(&*payload));
                ("<not generated>".to_string(), message)
            })?;
        match panic::catch_unwind(AssertUnwindSafe(|| property(&input))) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(message)) => Err((format!("{input:?}"), message)),
            Err(payload) => Err((
                format!("{input:?}"),
                format!("panicked: {}", panic_message(&*payload)),
            )),
        }
    }

    fn failure(
        &self,
        case: u32,
        sizes: Option<Sizes>,
        input: String,
        message: String,
    ) -> Box<Failure> {
        Box::new(Failure {
            name: self.name.to_string(),
            seed: self.seed,
            case,
            cases: self.cases,
            sizes,
            input,
            message,
            reproduce: self.reproduce.clone(),
        })
    }
}

/// Everything needed to understand and reproduce a failing case.
#[derive(Debug, PartialEq, Eq)]
struct Failure {
    name: String,
    seed: u64,
    case: u32,
    cases: u32,
    sizes: Option<Sizes>,
    input: String,
    message: String,
    reproduce: Reproduce,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Sizes {
    /// The size at which the case first failed.
    failed_at: u32,
    /// The smallest size at which it still fails. `input` is from this one.
    shrunk_to: u32,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            name,
            seed,
            case,
            cases,
            sizes,
            input,
            message,
            reproduce: Reproduce { package, filter },
        } = self;
        writeln!(
            f,
            "property `{name}` failed at case {case} of {cases} (seed 0x{seed:016x})"
        )?;
        writeln!(f, "  error: {message}")?;
        writeln!(f, "  input: {input}")?;
        if let Some(Sizes {
            failed_at,
            shrunk_to,
        }) = sizes
        {
            writeln!(f, "  size: {shrunk_to} (first failed at size {failed_at})")?;
        }
        write!(
            f,
            "  to reproduce: {SEED_VAR}=0x{seed:016x} cargo test -p {package} {filter}"
        )
    }
}

fn panic_message(payload: &(dyn Any + Send)) -> &str {
    if let Some(message) = payload.downcast_ref::<&str>() {
        message
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message
    } else {
        "<a panic payload that is not a string>"
    }
}

/// The seed for a run, given the value of [`SEED_VAR`] as
/// `std::env::var` returns it.
///
/// # Panics
///
/// Panics if the variable is set but is not a `u64` in a form
/// [`parse_seed`] accepts.
fn seed_from(var: Result<String, std::env::VarError>) -> u64 {
    match var {
        Ok(text) => match parse_seed(&text) {
            Some(seed) => seed,
            None => panic!(
                "{SEED_VAR} must be a u64, in decimal or as 0x-prefixed hex, but it is {text:?}"
            ),
        },
        Err(std::env::VarError::NotPresent) => DEFAULT_SEED,
        Err(std::env::VarError::NotUnicode(_)) => panic!("{SEED_VAR} is not valid Unicode"),
    }
}

/// Parses a seed as [`SEED_VAR`] accepts it: decimal, or hex with a `0x`
/// prefix, with surrounding whitespace ignored.
fn parse_seed(text: &str) -> Option<u64> {
    let text = text.trim();
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    const SEED: u64 = 0x1234_5678_9abc_def0;

    fn run(name: &str, cases: u32) -> Run<'_> {
        Run {
            name,
            seed: SEED,
            cases,
            reproduce: Reproduce {
                package: "tidepool-example".to_string(),
                filter: "example::tests::it_works".to_string(),
            },
        }
    }

    fn below_900(n: &u32) -> Result<(), String> {
        if *n < 900 {
            Ok(())
        } else {
            Err(format!("{n} is not below 900"))
        }
    }

    fn up_to_1000(rng: &mut Pcg32) -> u32 {
        rng.range(0..1000u32)
    }

    /// A list of `size` small numbers.
    fn numbers(rng: &mut Pcg32, size: u32) -> Vec<u32> {
        (0..size).map(|_| rng.range(0..10u32)).collect()
    }

    /// The property for the sized tests: a list must have fewer than five
    /// elements. Returned as a closure because the harness hands the
    /// property a `&Vec<u32>`, the generator's exact output type.
    fn shorter_than_5() -> impl Fn(&Vec<u32>) -> Result<(), String> {
        |list| {
            if list.len() < 5 {
                Ok(())
            } else {
                Err(format!("{} elements", list.len()))
            }
        }
    }

    fn panic_text(result: std::thread::Result<()>) -> String {
        let payload = result.expect_err("check should have panicked");
        panic_message(&*payload).to_string()
    }

    /// The seed printed in a report's last line.
    fn printed_seed(report: &str) -> u64 {
        let after = report
            .rsplit_once(&format!("{SEED_VAR}="))
            .expect("the report names the variable")
            .1;
        let seed = after.split(' ').next().expect("the seed comes first");
        parse_seed(seed).expect("the printed seed parses")
    }

    #[test]
    fn the_default_seed_spells_tidepool() {
        assert_eq!(DEFAULT_SEED.to_be_bytes(), *b"TIDEPOOL");
    }

    #[test]
    fn a_passing_property_runs_every_case() {
        let count = Cell::new(0);
        check(
            "counts cases",
            250,
            |rng| rng.next_u32(),
            |_| {
                count.set(count.get() + 1);
                Ok(())
            },
        );
        assert_eq!(count.get(), 250);

        count.set(0);
        check_sized(
            "counts sized cases",
            40,
            |rng, _| rng.next_u32(),
            |_| {
                count.set(count.get() + 1);
                Ok(())
            },
        );
        assert_eq!(count.get(), 40);
    }

    #[test]
    fn zero_cases_pass_without_running_anything() {
        check(
            "never runs",
            0,
            |_| panic!("generated"),
            |_: &()| Err("checked".to_string()),
        );
        check_sized(
            "never runs",
            0,
            |_, _| panic!("generated"),
            |_: &()| Err("checked".to_string()),
        );
    }

    /// Acceptance: a failing `check` prints a reproducible seed. The panic
    /// message, which libtest prints, carries the `TIDEPOOL_SEED=0x…`
    /// command.
    #[test]
    #[should_panic(expected = "TIDEPOOL_SEED=0x")]
    fn a_failing_check_prints_the_seed() {
        check("below 900", 500, up_to_1000, below_900);
    }

    #[test]
    #[should_panic(expected = "TIDEPOOL_SEED=0x")]
    fn a_failing_check_sized_prints_the_seed() {
        check_sized("short lists", 50, numbers, shorter_than_5());
    }

    /// Acceptance: the printed seed reproduces the failure. Run a failing
    /// property, read the seed back out of its report, and run it again with
    /// that seed through the explicit-seed runner: the failure, including
    /// the input, is the same.
    #[test]
    fn the_printed_seed_reproduces_the_identical_failure() {
        let seen = std::sync::Mutex::new(Vec::new());
        let record = |n: &u32| {
            seen.lock().expect("not poisoned").push(*n);
            below_900(n)
        };
        let first = panic_text(panic::catch_unwind(AssertUnwindSafe(|| {
            check("below 900", 500, up_to_1000, record)
        })));
        let first_input = *seen.lock().expect("not poisoned").last().expect("ran");
        let seed = printed_seed(&first);

        seen.lock().expect("not poisoned").clear();
        let again = panic_text(panic::catch_unwind(AssertUnwindSafe(|| {
            __check_with_seed(seed, "below 900", 500, up_to_1000, record)
        })));
        let again_input = *seen.lock().expect("not poisoned").last().expect("ran");

        assert_eq!(first, again, "the same seed gives the same report");
        assert_eq!(first_input, again_input);
        assert!(
            first.contains(&format!("input: {first_input}\n")),
            "{first}"
        );
    }

    #[test]
    fn the_report_names_the_package_and_the_running_test() {
        let text = panic_text(panic::catch_unwind(|| {
            check("below 900", 500, up_to_1000, below_900)
        }));
        // Worked out here rather than with the harness's own functions, so
        // that a fault in how `check` picks its seed shows up. Run the
        // tests with `TIDEPOOL_SEED` set as well to exercise that path.
        let seed = match std::env::var("TIDEPOOL_SEED") {
            Ok(text) => {
                let text = text.trim();
                match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
                    Some(hex) => u64::from_str_radix(hex, 16),
                    None => text.parse(),
                }
                .expect("TIDEPOOL_SEED is a u64")
            }
            Err(_) => 0x5449_4445_504f_4f4c,
        };
        let tail = format!("  to reproduce: TIDEPOOL_SEED=0x{seed:016x} cargo test -p ");
        assert!(text.contains(&tail), "{text}");
        // libtest names the thread after the test, so the filter is this
        // test's path.
        assert!(
            text.ends_with(" testing::tests::the_report_names_the_package_and_the_running_test"),
            "{text}"
        );
        if let Ok(package) = std::env::var("CARGO_PKG_NAME") {
            assert!(text.contains(&format!(" -p {package} ")), "{text}");
        }
    }

    #[test]
    fn without_a_test_thread_name_the_filter_is_the_property_name() {
        let reproduce = std::thread::Builder::new()
            .spawn(|| Reproduce::current("my property"))
            .expect("spawn a thread")
            .join()
            .expect("the thread does not panic");
        assert_eq!(reproduce.filter, "\"my property\"");
    }

    #[test]
    fn a_failure_reports_the_seed_case_input_and_message() {
        let failure = run("below 900", 500)
            .unsized_cases(&up_to_1000, &below_900)
            .expect_err("some case is >= 900");
        assert_eq!(failure.name, "below 900");
        assert_eq!(failure.seed, SEED);
        assert_eq!(failure.cases, 500);
        assert_eq!(failure.sizes, None);
        let input: u32 = failure.input.parse().expect("the input is a number");
        assert!(input >= 900);
        assert_eq!(failure.message, format!("{input} is not below 900"));

        let text = failure.to_string();
        let case = failure.case;
        assert_eq!(
            text,
            format!(
                "property `below 900` failed at case {case} of 500 (seed 0x123456789abcdef0)\n  \
                 error: {input} is not below 900\n  \
                 input: {input}\n  \
                 to reproduce: TIDEPOOL_SEED=0x123456789abcdef0 cargo test -p tidepool-example \
                 example::tests::it_works"
            )
        );
    }

    #[test]
    fn the_reported_seed_and_case_reproduce_the_failure() {
        let first = run("below 900", 500)
            .unsized_cases(&up_to_1000, &below_900)
            .expect_err("fails");

        // Running again with the same seed fails identically.
        let again = Run {
            seed: first.seed,
            ..run("below 900", 500)
        };
        assert_eq!(again.unsized_cases(&up_to_1000, &below_900), Err(first));

        // The failing input can be regenerated from the seed and case alone.
        let first = again
            .unsized_cases(&up_to_1000, &below_900)
            .expect_err("fails");
        let regenerated = up_to_1000(&mut again.case_rng(first.case));
        assert_eq!(regenerated.to_string(), first.input);
        // It was the first failing case: every earlier one passes.
        for case in 0..first.case {
            assert!(below_900(&up_to_1000(&mut again.case_rng(case))).is_ok());
        }
    }

    #[test]
    fn different_seeds_and_names_give_different_inputs() {
        let inputs = |run: Run<'_>| -> Vec<u32> {
            (0..20).map(|case| run.case_rng(case).next_u32()).collect()
        };
        let base = inputs(run("a", 20));
        assert_eq!(base, inputs(run("a", 20)));
        assert_ne!(base, inputs(run("b", 20)));
        assert_ne!(
            base,
            inputs(Run {
                seed: SEED + 1,
                ..run("a", 20)
            })
        );
        // Cases are independent streams, not one sequence.
        let mut sorted = base.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 20);
    }

    #[test]
    fn a_panicking_property_is_reported_with_its_seed_and_input() {
        let explode = |n: &u32| -> Result<(), String> {
            assert!(*n < 900, "index {n} out of range");
            Ok(())
        };
        let failure = run("panics", 500)
            .unsized_cases(&up_to_1000, &explode)
            .expect_err("some case panics");
        let input: u32 = failure.input.parse().expect("the input is a number");
        assert!(input >= 900);
        assert_eq!(
            failure.message,
            format!("panicked: index {input} out of range")
        );
        assert_eq!(failure.seed, SEED);
    }

    #[test]
    fn a_panicking_generator_is_reported() {
        let generate = |rng: &mut Pcg32| -> u32 {
            let n = rng.range(0..1000u32);
            assert!(n < 900, "cannot generate {n}");
            n
        };
        let failure = run("generator", 500)
            .unsized_cases(&generate, &|_: &u32| Ok(()))
            .expect_err("the generator panics");
        assert_eq!(failure.input, "<not generated>");
        assert!(
            failure
                .message
                .starts_with("the generator panicked: cannot generate 9"),
            "{}",
            failure.message
        );
    }

    #[test]
    fn non_string_panics_are_reported_too() {
        let explode = |_: &u32| -> Result<(), String> { std::panic::panic_any(42) };
        let failure = run("odd panic", 3)
            .unsized_cases(&up_to_1000, &explode)
            .expect_err("panics");
        assert_eq!(failure.case, 0);
        assert_eq!(
            failure.message,
            "panicked: <a panic payload that is not a string>"
        );
    }

    #[test]
    fn sizes_ramp_from_zero_to_the_maximum() {
        let sizes = |cases: u32| -> Vec<u32> {
            (0..cases)
                .map(|case| run("sizes", cases).size_of_case(case))
                .collect()
        };
        assert_eq!(sizes(1), [MAX_SIZE]);
        assert_eq!(sizes(2), [0, MAX_SIZE]);
        assert_eq!(sizes(5), [0, 25, 50, 75, 100]);
        let many = sizes(1000);
        assert_eq!((many[0], many[999]), (0, MAX_SIZE));
        assert!(many.is_sorted());
        // Every size is used when there are enough cases.
        let mut distinct = many.clone();
        distinct.dedup();
        assert_eq!(distinct.len(), MAX_SIZE as usize + 1);
        // The multiplication must not overflow for a huge run.
        assert_eq!(run("sizes", u32::MAX).size_of_case(u32::MAX - 1), MAX_SIZE);
    }

    #[test]
    fn the_generator_receives_the_size_of_its_case() {
        let seen = Cell::new(0);
        let run = run("sees sizes", 101);
        let generate = |_: &mut Pcg32, size: u32| size;
        let property = |&size: &u32| {
            seen.set(seen.get() + 1);
            if size == seen.get() - 1 {
                Ok(())
            } else {
                Err(format!("case {} got size {size}", seen.get() - 1))
            }
        };
        assert_eq!(run.sized_cases(&generate, &property), Ok(()));
        assert_eq!(seen.get(), 101);
    }

    #[test]
    fn a_sized_failure_is_shrunk_to_the_smallest_failing_size() {
        let failure = run("short lists", 101)
            .sized_cases(&numbers, &shorter_than_5())
            .expect_err("long lists fail");
        // Sizes 0 to 4 pass, so case 5 (size 5) is the first to fail, and
        // nothing smaller fails.
        assert_eq!(failure.case, 5);
        assert_eq!(
            failure.sizes,
            Some(Sizes {
                failed_at: 5,
                shrunk_to: 5
            })
        );
        assert_eq!(failure.message, "5 elements");

        // With few cases the first failure is at a large size, and shrinks.
        let failure = run("short lists", 3)
            .sized_cases(&numbers, &shorter_than_5())
            .expect_err("long lists fail");
        assert_eq!(failure.case, 1);
        assert_eq!(
            failure.sizes,
            Some(Sizes {
                failed_at: 50,
                shrunk_to: 5
            })
        );
        assert_eq!(failure.message, "5 elements");
        // The reported input is the shrunk one, regenerated from the same
        // case at the smaller size.
        let regenerated = numbers(&mut run("short lists", 3).case_rng(1), 5);
        assert_eq!(failure.input, format!("{regenerated:?}"));

        let text = failure.to_string();
        assert!(
            text.contains("\n  size: 5 (first failed at size 50)\n"),
            "{text}"
        );
        assert!(
            text.contains(&format!("\n  input: {regenerated:?}\n")),
            "{text}"
        );
    }

    #[test]
    fn check_sized_panics_with_the_shrunk_report() {
        let text = panic_text(panic::catch_unwind(|| {
            check_sized("short lists", 3, numbers, shorter_than_5())
        }));
        assert!(
            text.starts_with("property `short lists` failed at case 1 of 3 (seed 0x"),
            "{text}"
        );
        assert!(text.contains("\n  error: 5 elements\n"), "{text}");
        assert!(
            text.contains("\n  size: 5 (first failed at size 50)\n"),
            "{text}"
        );
        // Re-running with the printed seed gives the same shrunk report.
        let seed = printed_seed(&text);
        let again = panic_text(panic::catch_unwind(|| {
            __check_sized_with_seed(seed, "short lists", 3, numbers, shorter_than_5())
        }));
        assert_eq!(text, again);
    }

    #[test]
    fn the_seed_comes_from_the_variable_or_the_default() {
        use std::env::VarError;
        assert_eq!(seed_from(Err(VarError::NotPresent)), DEFAULT_SEED);
        assert_eq!(seed_from(Ok("0x5".to_string())), 5);
        assert_eq!(seed_from(Ok("5".to_string())), 5);
        assert_eq!(seed_from(Ok(" 0XFF\n".to_string())), 255);
    }

    #[test]
    #[should_panic(expected = "TIDEPOOL_SEED must be a u64")]
    fn a_malformed_seed_variable_is_an_error() {
        seed_from(Ok("0xnope".to_string()));
    }

    #[test]
    #[should_panic(expected = "TIDEPOOL_SEED is not valid Unicode")]
    fn a_seed_variable_that_is_not_unicode_is_an_error() {
        seed_from(Err(std::env::VarError::NotUnicode(
            std::ffi::OsString::from("seed"),
        )));
    }

    /// Acceptance, the other half: setting `TIDEPOOL_SEED` to the text a
    /// failure prints selects the seed of that failure, and so reproduces
    /// it. The seed here is not the default, so a harness that ignored the
    /// printed value would fail.
    #[test]
    fn the_printed_seed_text_selects_the_failing_seed() {
        for seed in [0x5, 0xdead_beef_cafe_f00d, 0] {
            let first = panic_text(panic::catch_unwind(|| {
                __check_with_seed(seed, "below 900", 500, up_to_1000, below_900)
            }));
            let printed = first
                .rsplit_once("TIDEPOOL_SEED=")
                .and_then(|(_, after)| after.split(' ').next())
                .expect("the report prints the seed");
            let chosen = seed_from(Ok(printed.to_string()));
            assert_eq!(chosen, seed, "{printed}");
            let again = panic_text(panic::catch_unwind(|| {
                __check_with_seed(chosen, "below 900", 500, up_to_1000, below_900)
            }));
            assert_eq!(first, again);
        }
    }

    #[test]
    fn seeds_parse_as_decimal_or_hex() {
        assert_eq!(parse_seed("0"), Some(0));
        assert_eq!(parse_seed("12345"), Some(12345));
        assert_eq!(parse_seed(" 42\n"), Some(42));
        assert_eq!(parse_seed("0xff"), Some(255));
        assert_eq!(parse_seed("0XFF"), Some(255));
        assert_eq!(parse_seed("0x54494445504f4f4c"), Some(DEFAULT_SEED));
        assert_eq!(parse_seed("18446744073709551615"), Some(u64::MAX));
        assert_eq!(parse_seed("0xffffffffffffffff"), Some(u64::MAX));
        for bad in [
            "",
            "0x",
            "seed",
            "-1",
            "1.5",
            "18446744073709551616",
            "0x1_0000_0000_0000_0000",
            "ff",
        ] {
            assert_eq!(parse_seed(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_printed_seed_is_zero_padded_hex_and_parses_back() {
        let failure = Run {
            seed: 0xab,
            ..run("below 900", 500)
        }
        .unsized_cases(&up_to_1000, &below_900)
        .expect_err("fails");
        let text = failure.to_string();
        assert!(text.contains("TIDEPOOL_SEED=0x00000000000000ab "), "{text}");
        assert_eq!(printed_seed(&text), 0xab);
    }

    #[test]
    fn assert_approx_accepts_close_values() {
        assert_approx!(0.1f32 + 0.2, 0.3);
        assert_approx!(1.0f64, 1.0 + 1e-10);
        assert_approx!(10.0f32, 10.4, 0.5);
        assert_approx!(f32::INFINITY, f32::INFINITY);
        assert_approx!(-0.0f64, 0.0, 0.0, "signed zeros are equal");
    }

    #[test]
    #[should_panic(expected = "assertion `left ≈ right` failed")]
    fn assert_approx_rejects_distant_values() {
        assert_approx!(1.0f32, 1.1);
    }

    #[test]
    #[should_panic(expected = "failed: at step 3")]
    fn assert_approx_includes_the_message() {
        assert_approx!(1.0f64, 2.0, 0.5, "at step {}", 3);
    }

    #[test]
    #[should_panic(expected = "left: NaN")]
    fn assert_approx_rejects_nan() {
        assert_approx!(f32::NAN, f32::NAN, 1.0);
    }
}
