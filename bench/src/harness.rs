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

//! The harness: a [`Suite`] is a name and a function that sets up its
//! input, untimed, and hands the [`Bencher`] one routine to time.
//!
//! The bencher warms the routine up, then takes samples
//! ([`Settings`]):
//!
//! 1. **Warm-up.** It runs the routine in batches of 1, 2, 4, … iterations,
//!    doubling while a batch is shorter than [`Settings::sample_target`],
//!    until [`Settings::warm_up`] has passed, and at least once. The last
//!    batch gives the estimate of one iteration.
//! 2. **Batch size.** Each sample runs enough iterations to last about
//!    [`Settings::sample_target`] by that estimate, and at least one, so
//!    that a fast routine is timed over long enough for [`Instant`] to be
//!    accurate.
//! 3. **Samples.** It takes samples until [`Settings::budget`] has passed,
//!    counting from the first sample and including any untimed setup, but
//!    never fewer than [`Settings::min_samples`] nor more than
//!    [`Settings::max_samples`]. A routine slower than the budget allows
//!    still gets its minimum.
//! 4. **Summary.** Each sample's time divided by its iterations is one
//!    per-iteration time; the [`Summary`] has their median and their p95 by
//!    nearest rank, in whole nanoseconds ([`median`], [`p95`]).
//!
//! A routine that changes its input ([`Bencher::iter_batched`]) gets a new
//! one for every iteration from an untimed setup, made for the whole batch
//! before the batch is timed, and its outputs are dropped after the timing
//! ends. Every input and output goes through [`black_box`], so the
//! optimizer cannot drop the work.
//!
//! Before the first suite, a run primes the allocator ([`prime_allocator`]),
//! so that a suite's numbers do not depend on which suites ran before it in
//! the same process.

use std::any::Any;
use std::hint::black_box;
use std::panic::{self, AssertUnwindSafe};
use std::time::{Duration, Instant};

use crate::BenchError;

/// The most iterations one sample runs, however fast the routine: a bound
/// on the inputs [`Bencher::iter_batched`] holds at once.
pub const MAX_BATCH: usize = 1 << 20;

/// The size of the block [`prime_allocator`] allocates and frees: 31 MiB,
/// just under the 32 MiB at which glibc's malloc caps its dynamic mmap
/// threshold on 64-bit targets.
pub const PRIME_BYTES: usize = 31 << 20;

/// Allocates a block of [`PRIME_BYTES`] and frees it at once, writing
/// nothing to it, so that glibc's malloc keeps the memory a suite frees
/// instead of handing it back to the OS. [`cli::run`](crate::cli::run)
/// calls it once, before the first suite.
///
/// **Why.** glibc's malloc serves a request above its mmap threshold
/// (128 KiB at first) with a new mapping, unmapped again when it is freed;
/// and it gives the OS back freed memory at the top of its heap once that
/// is over its trim threshold. Freeing a mapped block raises the mmap
/// threshold to the block's size, up to 32 MiB, and the trim threshold to
/// twice that. Until some suite has done so, a suite that frees a few
/// megabytes a sample can have them unmapped or trimmed, and fault fresh
/// pages in inside the next sample's timing. On the reference machine, the
/// 10,000-caret keystroke on 400 KB, run alone or first, was bimodal:
/// about 1.9 ms with about 42,000 page faults, or 2.2 to 2.6 ms with about
/// 285,000; after a 100 MB suite it never faulted. Freeing one block of
/// [`PRIME_BYTES`] first sets both thresholds (to about 31 and 62 MiB)
/// where a large suite would leave them, for every suite and in any order:
/// the same suite then has about 2,200 faults on every run.
///
/// **Elsewhere.** It is plain `std` code with no `cfg`. The block is never
/// written, so none of its pages is touched. With another allocator (musl's,
/// macOS's or Windows') it is one allocation and one free, and changes
/// nothing a suite measures. Other allocator behaviour, and the CPU's
/// clocks, can still move a median between runs.
pub fn prime_allocator() {
    drop(black_box(Vec::<u8>::with_capacity(PRIME_BYTES)));
}

/// How the [`Bencher`] measures a routine (see the [module
/// documentation](self)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// How long the warm-up runs the routine; it runs at least once.
    pub warm_up: Duration,
    /// How long one sample should last: a faster routine runs several
    /// iterations per sample.
    pub sample_target: Duration,
    /// How long to take samples for, counted from the start of the first
    /// sample, untimed setup and dropping included: no sample starts after
    /// it, but [`min_samples`](Settings::min_samples) are taken however long
    /// they take.
    pub budget: Duration,
    /// The fewest samples, however long they take.
    pub min_samples: usize,
    /// The most samples, however short.
    pub max_samples: usize,
}

impl Settings {
    /// What `cargo xtask bench` uses: a 300 ms warm-up, samples of about
    /// 2 ms, and samples for 1 s, at least 10 and at most 1,000.
    ///
    /// So a routine of up to about 100 ms gets 10 or more samples within
    /// the second, and a slower one exactly 10: a 3 s grapheme query takes
    /// about 33 s with its warm-up.
    pub const DEFAULT: Self = Self {
        warm_up: Duration::from_millis(300),
        sample_target: Duration::from_millis(2),
        budget: Duration::from_secs(1),
        min_samples: 10,
        max_samples: 1_000,
    };
}

impl Default for Settings {
    /// [`Settings::DEFAULT`].
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// What a suite measured: per iteration, in whole nanoseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Summary {
    /// The median of the samples' per-iteration times.
    pub median_ns: u64,
    /// Their 95th percentile, by nearest rank.
    pub p95_ns: u64,
    /// How many samples there were.
    pub samples: usize,
    /// How many iterations each sample ran.
    pub batch: usize,
}

impl Summary {
    /// The summary of `per_iteration_ns`, the samples' per-iteration times
    /// in nanoseconds, in any order; `None` for no samples. `batch` is
    /// recorded as it is.
    pub fn of(per_iteration_ns: &[f64], batch: usize) -> Option<Self> {
        let mut sorted = per_iteration_ns.to_vec();
        sorted.sort_by(f64::total_cmp);
        Some(Self {
            median_ns: whole_ns(median(&sorted)?),
            p95_ns: whole_ns(p95(&sorted)?),
            samples: sorted.len(),
            batch,
        })
    }
}

/// `ns` rounded to whole nanoseconds; a negative or NaN value is 0.
fn whole_ns(ns: f64) -> u64 {
    // `as` saturates, and turns NaN into 0.
    ns.round() as u64
}

/// The median of `sorted`, which is in ascending order: the middle value,
/// or the mean of the two middle values of an even count. `None` when it is
/// empty.
pub fn median(sorted: &[f64]) -> Option<f64> {
    let n = sorted.len();
    let upper = *sorted.get(n / 2)?;
    if n % 2 == 1 {
        return Some(upper);
    }
    let lower = *sorted.get(n / 2 - 1)?;
    Some(lower + (upper - lower) / 2.0)
}

/// The 95th percentile of `sorted`, which is in ascending order, by
/// nearest rank: the value at rank ⌈0.95 n⌉, counting from 1. So of 10
/// samples it is the largest, and of 100 the 95th. `None` when it is empty.
pub fn p95(sorted: &[f64]) -> Option<f64> {
    let n = sorted.len();
    // ⌈95 n / 100⌉, in integers; at least rank 1.
    let rank = (95 * n).div_ceil(100).max(1);
    sorted.get(rank - 1).copied()
}

/// One benchmark: a hierarchical name (`text/rope/insert/400kb`), and the
/// function that sets up its input and calls one of the [`Bencher`]'s
/// methods once.
#[derive(Clone, Copy, Debug)]
pub struct Suite {
    /// The name: `/`-separated parts, the first naming the crate measured,
    /// so that `cargo xtask bench text` runs all of `text`'s suites.
    pub name: &'static str,
    /// Sets up, untimed, then measures one routine with the bencher. An
    /// error, or a panic, fails the suite.
    pub run: fn(&mut Bencher) -> Result<(), BenchError>,
}

/// Times one routine for a [`Suite`] (see the [module
/// documentation](self)).
#[derive(Debug)]
pub struct Bencher {
    settings: Settings,
    summary: Option<Summary>,
    routines: usize,
}

impl Bencher {
    /// A bencher that measures with `settings`.
    pub fn new(settings: Settings) -> Self {
        Self {
            settings,
            summary: None,
            routines: 0,
        }
    }

    /// Measures `routine`, which may keep state between iterations (a
    /// counter that walks a list of inputs) but must cost the same each
    /// time. Its output is dropped inside the timing, so it should be
    /// cheap to drop.
    pub fn iter<O>(&mut self, mut routine: impl FnMut() -> O) {
        let summary = measure(&self.settings, |batch| {
            let start = Instant::now();
            for _ in 0..batch {
                black_box(routine());
            }
            start.elapsed()
        });
        self.record(summary);
    }

    /// Measures `routine` on a new input from `setup` each iteration:
    /// `setup` is not timed, nor is dropping what `routine` returns. Each
    /// sample makes its whole batch of inputs before it is timed, so up to
    /// [`MAX_BATCH`] of them exist at once; keep them cheap (an O(1) clone
    /// of a rope, not a copy of its text).
    pub fn iter_batched<I, O>(
        &mut self,
        mut setup: impl FnMut() -> I,
        mut routine: impl FnMut(I) -> O,
    ) {
        let summary = measure(&self.settings, |batch| {
            let inputs: Vec<I> = (0..batch).map(|_| setup()).collect();
            let mut outputs = Vec::with_capacity(batch);
            let start = Instant::now();
            for input in inputs {
                outputs.push(black_box(routine(black_box(input))));
            }
            let elapsed = start.elapsed();
            drop(outputs);
            elapsed
        });
        self.record(summary);
    }

    fn record(&mut self, summary: Summary) {
        self.routines += 1;
        self.summary = Some(summary);
    }
}

/// Runs `suite` with `settings`, and returns what it measured.
///
/// # Errors
///
/// [`BenchError::Suite`], naming the suite, if its function returned an
/// error or panicked (the panic's message also goes to standard error, as
/// the panic hook prints it), or measured no routine or more than one.
pub fn run_suite(suite: &Suite, settings: &Settings) -> Result<Summary, BenchError> {
    let mut bencher = Bencher::new(*settings);
    let outcome = panic::catch_unwind(AssertUnwindSafe(|| (suite.run)(&mut bencher)));
    let failed = |detail: String| BenchError::Suite {
        name: suite.name.to_string(),
        detail,
    };
    match outcome {
        Err(payload) => Err(failed(format!("it panicked: {}", panic_message(&*payload)))),
        Ok(Err(error)) => Err(failed(error.to_string())),
        Ok(Ok(())) => match (bencher.routines, bencher.summary) {
            (1, Some(summary)) => Ok(summary),
            (0, _) => Err(failed("it measured no routine".to_string())),
            (count, _) => Err(failed(format!("it measured {count} routines, not one"))),
        },
    }
}

/// The message of a panic, if it was a string.
fn panic_message(payload: &(dyn Any + Send)) -> &str {
    if let Some(message) = payload.downcast_ref::<&str>() {
        message
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message
    } else {
        "(no message)"
    }
}

/// Warms up, sizes the batch and takes the samples, as the [module
/// documentation](self) says. `run_batch(n)` runs `n` iterations and
/// returns how long they took.
fn measure(settings: &Settings, mut run_batch: impl FnMut(usize) -> Duration) -> Summary {
    let started = Instant::now();
    let mut batch = 1;
    let mut estimate_ns;
    loop {
        let elapsed = run_batch(batch);
        estimate_ns = nanos(elapsed) / batch as f64;
        if started.elapsed() >= settings.warm_up {
            break;
        }
        if elapsed < settings.sample_target {
            batch = (batch * 2).min(MAX_BATCH);
        }
    }
    let batch = batch_for(settings.sample_target, estimate_ns);

    let max = settings.max_samples.max(1);
    let min = settings.min_samples.clamp(1, max);
    let mut samples = Vec::with_capacity(max.min(1_024));
    let started = Instant::now();
    while samples.len() < max && (samples.len() < min || started.elapsed() < settings.budget) {
        samples.push(nanos(run_batch(batch)) / batch as f64);
    }
    // There is at least one sample, so there is a summary.
    Summary::of(&samples, batch).unwrap_or(Summary {
        median_ns: 0,
        p95_ns: 0,
        samples: 0,
        batch,
    })
}

/// The iterations per sample for a routine of `estimate_ns` per iteration
/// to last about `target`: at least 1, at most [`MAX_BATCH`].
fn batch_for(target: Duration, estimate_ns: f64) -> usize {
    if estimate_ns <= 0.0 || estimate_ns.is_nan() {
        return MAX_BATCH;
    }
    let batch = (nanos(target) / estimate_ns).ceil();
    // `as` saturates; the bounds keep it in range anyway.
    (batch as usize).clamp(1, MAX_BATCH)
}

fn nanos(duration: Duration) -> f64 {
    duration.as_nanos() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Settings that finish at once: no warm-up beyond one batch, and a
    /// fixed number of samples.
    fn quick(samples: usize) -> Settings {
        Settings {
            warm_up: Duration::ZERO,
            sample_target: Duration::from_micros(50),
            budget: Duration::ZERO,
            min_samples: samples,
            max_samples: samples,
        }
    }

    #[test]
    fn the_median_is_the_middle_or_the_mean_of_the_two_middles() {
        assert_eq!(median(&[]), None);
        assert_eq!(median(&[7.0]), Some(7.0));
        assert_eq!(median(&[1.0, 2.0, 9.0]), Some(2.0));
        assert_eq!(median(&[1.0, 2.0, 4.0, 9.0]), Some(3.0));
        assert_eq!(median(&[1.0, 2.0]), Some(1.5));
        // Large values do not overflow into infinity.
        assert_eq!(median(&[f64::MAX, f64::MAX]), Some(f64::MAX));
    }

    #[test]
    fn p95_is_the_nearest_rank() {
        assert_eq!(p95(&[]), None);
        assert_eq!(p95(&[3.0]), Some(3.0));
        // ⌈0.95 × 10⌉ = 10: the largest of 10.
        let ten: Vec<f64> = (1..=10).map(f64::from).collect();
        assert_eq!(p95(&ten), Some(10.0));
        // ⌈0.95 × 20⌉ = 19.
        let twenty: Vec<f64> = (1..=20).map(f64::from).collect();
        assert_eq!(p95(&twenty), Some(19.0));
        // ⌈0.95 × 100⌉ = 95, and ⌈0.95 × 101⌉ = ⌈95.95⌉ = 96.
        let hundred: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(p95(&hundred), Some(95.0));
        let more: Vec<f64> = (1..=101).map(f64::from).collect();
        assert_eq!(p95(&more), Some(96.0));
        // ⌈0.95 × 2⌉ = 2, and ⌈0.95 × 19⌉ = ⌈18.05⌉ = 19.
        assert_eq!(p95(&[1.0, 2.0]), Some(2.0));
        let nineteen: Vec<f64> = (1..=19).map(f64::from).collect();
        assert_eq!(p95(&nineteen), Some(19.0));
    }

    #[test]
    fn a_summary_sorts_and_rounds_to_whole_nanoseconds() {
        let summary = Summary::of(&[30.4, 10.6, 20.5, 40.0], 8).unwrap();
        assert_eq!(
            summary,
            Summary {
                median_ns: 25, // (20.5 + 30.4) / 2 = 25.45
                p95_ns: 40,
                samples: 4,
                batch: 8,
            }
        );
        // Half a nanosecond rounds up.
        assert_eq!(Summary::of(&[20.0, 31.0], 1).unwrap().median_ns, 26);
        assert_eq!(Summary::of(&[], 1), None);
        assert_eq!(whole_ns(-3.0), 0);
        assert_eq!(whole_ns(f64::NAN), 0);
    }

    #[test]
    fn batches_last_about_the_sample_target() {
        let ms = Duration::from_millis(2);
        assert_eq!(batch_for(ms, 2_000_000.0), 1);
        assert_eq!(batch_for(ms, 120_000_000.0), 1);
        assert_eq!(batch_for(ms, 1_000.0), 2_000);
        assert_eq!(batch_for(ms, 3_000.0), 667);
        assert_eq!(batch_for(ms, 0.0), MAX_BATCH);
        assert_eq!(batch_for(ms, 0.000_1), MAX_BATCH);
        assert_eq!(batch_for(ms, f64::NAN), MAX_BATCH);
    }

    #[test]
    fn the_minimum_and_maximum_sample_counts_hold() {
        // A budget already spent still gives the minimum.
        let summary = measure(&quick(7), |_| Duration::from_micros(1));
        assert_eq!(summary.samples, 7);
        // The maximum caps a long budget.
        let settings = Settings {
            budget: Duration::from_secs(3_600),
            min_samples: 2,
            max_samples: 5,
            ..quick(0)
        };
        assert_eq!(measure(&settings, |_| Duration::ZERO).samples, 5);
        // A minimum above the maximum, or a zero, still means one or more.
        let settings = Settings {
            min_samples: 9,
            max_samples: 0,
            ..quick(0)
        };
        assert_eq!(measure(&settings, |_| Duration::ZERO).samples, 1);
    }

    #[test]
    fn samples_are_per_iteration_and_the_batch_follows_the_warm_up() {
        // A fake routine of 1 µs an iteration: with a 50 µs target, samples
        // run 50 iterations, and each reports 1,000 ns.
        let mut batches = Vec::new();
        let summary = measure(&quick(4), |batch| {
            batches.push(batch);
            Duration::from_micros(batch as u64)
        });
        assert_eq!(batches, [1, 50, 50, 50, 50]);
        assert_eq!(summary.batch, 50);
        assert_eq!((summary.median_ns, summary.p95_ns), (1_000, 1_000));
    }

    #[test]
    fn the_warm_up_doubles_its_batch_until_it_is_long_enough() {
        // Long enough that four calls of the fake, each sleeping 200 µs,
        // fit in it even on a loaded machine.
        let settings = Settings {
            warm_up: Duration::from_millis(100),
            ..quick(1)
        };
        // 10 µs an iteration, against a 50 µs target: 1, 2, 4, 8, then 8
        // until the warm-up is over. The fake reports times it did not
        // take, so the real clock, which its sleeps move on, ends it.
        let mut batches = Vec::new();
        let summary = measure(&settings, |batch| {
            batches.push(batch);
            std::thread::sleep(Duration::from_micros(200));
            Duration::from_micros(10 * batch as u64)
        });
        assert_eq!(&batches[..4], [1, 2, 4, 8]);
        assert!(
            batches[4..batches.len() - 1].iter().all(|&b| b == 8),
            "{batches:?}"
        );
        assert_eq!(summary.batch, 5);
    }

    #[test]
    fn iter_times_the_routine_and_batched_inputs_are_fresh() {
        let mut bencher = Bencher::new(quick(3));
        let mut calls = 0_u64;
        bencher.iter(|| {
            calls += 1;
            calls
        });
        assert!(calls >= 3);
        let summary = bencher.summary.unwrap();
        assert_eq!(summary.samples, 3);

        // Each iteration gets its own input from setup.
        let mut bencher = Bencher::new(quick(3));
        let (mut made, mut used) = (0, 0);
        bencher.iter_batched(
            || {
                made += 1;
                vec![1_u8; 16]
            },
            |mut input| {
                used += 1;
                assert_eq!(input, [1; 16], "the input was used before");
                input.fill(0);
                input
            },
        );
        assert_eq!(made, used);
        assert_eq!(bencher.routines, 1);
    }

    #[test]
    fn batched_setup_and_dropping_are_not_timed() {
        /// An output that takes 2 ms to drop.
        struct SlowDrop;
        impl Drop for SlowDrop {
            fn drop(&mut self) {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        // One iteration a sample, whatever the estimate.
        let settings = Settings {
            sample_target: Duration::ZERO,
            ..quick(3)
        };
        let mut bencher = Bencher::new(settings);
        bencher.iter_batched(
            || {
                std::thread::sleep(Duration::from_millis(2));
                7_u64
            },
            |input| (black_box(input) + 1, SlowDrop),
        );
        let summary = bencher.summary.unwrap();
        assert_eq!((summary.batch, summary.samples), (1, 3));
        // The routine itself takes nanoseconds; 2 ms of setup or of
        // dropping in the timing would show.
        assert!(
            summary.median_ns < 500_000,
            "the median is {} ns",
            summary.median_ns
        );
    }

    #[test]
    fn a_suite_fails_when_it_errs_panics_or_measures_other_than_once() {
        fn none(_: &mut Bencher) -> Result<(), BenchError> {
            Ok(())
        }
        fn twice(b: &mut Bencher) -> Result<(), BenchError> {
            b.iter(|| 1);
            b.iter(|| 2);
            Ok(())
        }
        fn errs(_: &mut Bencher) -> Result<(), BenchError> {
            Err(BenchError::Check("the setup went wrong".to_string()))
        }
        fn panics(_: &mut Bencher) -> Result<(), BenchError> {
            panic!("on purpose, in a test")
        }
        fn once(b: &mut Bencher) -> Result<(), BenchError> {
            b.iter(|| black_box(2_u64).pow(10));
            Ok(())
        }
        let run = |run: fn(&mut Bencher) -> Result<(), BenchError>| {
            run_suite(
                &Suite {
                    name: "fake/one",
                    run,
                },
                &quick(2),
            )
            .map_err(|e| e.to_string())
        };
        assert_eq!(
            run(none).unwrap_err(),
            "suite `fake/one` failed: it measured no routine"
        );
        assert_eq!(
            run(twice).unwrap_err(),
            "suite `fake/one` failed: it measured 2 routines, not one"
        );
        assert_eq!(
            run(errs).unwrap_err(),
            "suite `fake/one` failed: the setup went wrong"
        );
        assert_eq!(
            run(panics).unwrap_err(),
            "suite `fake/one` failed: it panicked: on purpose, in a test"
        );
        assert_eq!(run(once).unwrap().samples, 2);
    }
}
