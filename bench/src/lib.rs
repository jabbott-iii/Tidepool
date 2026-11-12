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

//! Tidepool's own benchmark harness and suites.
//!
//! Specification: `docs/04-quality.md` → "Benchmarks", and ADR-0035 for
//! the command line and the baseline. `cargo xtask bench [FILTER]` builds
//! this crate's binary in the release profile and runs it.
//!
//! - [`harness`]: a [`Suite`] sets up untimed and hands the [`Bencher`] one
//!   routine; the bencher warms it up, batches it if it is fast, takes
//!   samples within a time budget but never fewer than a minimum, and
//!   reports the median and p95 per iteration ([`Summary`]).
//! - [`baseline`]: `bench/baseline.json`, the machine it was recorded on
//!   and each suite's numbers, read and written with
//!   `tidepool_codec::json`.
//! - [`cli`]: the command line, `[FILTER] [--save --machine NAME]`, and the
//!   report, which compares each median with the baseline's where the
//!   baseline was recorded on a machine like this one.
//! - [`suites`]: the suites, by crate; today `text`'s.
//!
//! It starts no program and opens no file but the baseline and, for the
//! CPU's model, `/proc/cpuinfo`.

// rustdoc does not apply the workspace `unsafe_code = "deny"` to doc tests,
// so this forbids `unsafe` in them, README examples included (ADR-0036).
// `cargo xtask unsafe-audit` enforces that this line is here.
#![doc(test(attr(forbid(unsafe_code))))]

pub mod baseline;
pub mod cli;
mod error;
pub mod harness;
pub mod suites;

#[cfg(test)]
mod testing;

// The README's examples are compiled as doc tests too.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub use baseline::{Baseline, Entry, Facts, Machine};
pub use error::{BenchError, printable};
pub use harness::{Bencher, Settings, Suite, Summary, run_suite};
