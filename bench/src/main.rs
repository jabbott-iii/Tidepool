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

//! `tidepool-bench [FILTER] [--save --machine NAME]`: runs the suites whose
//! names contain the filter and reports each one's median and p95, against
//! `bench/baseline.json` where that was recorded on a machine like this one
//! (ADR-0035). `cargo xtask bench` builds it in the release profile and runs
//! it; `--help` lists the suites.
//!
//! Exit status: 0 when every suite ran, slower ones included; 1 for a bad
//! argument, a filter that matches no suite, a failed suite, a baseline
//! that cannot be read, parsed or written, a `--save` refused because the
//! baseline was recorded on another machine, or a report that cannot be
//! written.

use std::io::{self, Write};
use std::process::ExitCode;

use tidepool_bench::cli::{self, Options};
use tidepool_bench::{BenchError, Facts, Settings, baseline, printable, suites};
use tidepool_core::cli::Args;

fn main() -> ExitCode {
    let suites = suites::all();
    let mut out = io::stdout();
    let result = Args::from_env()
        .map_err(|error| BenchError::Usage(error.to_string()))
        .and_then(Options::parse)
        .and_then(|options| {
            if options.help {
                out.write_all(cli::usage(&suites).as_bytes())
                    .and_then(|()| out.flush())
                    .map_err(BenchError::Output)
            } else {
                cli::run(
                    &options,
                    &suites,
                    &Settings::DEFAULT,
                    &baseline::default_path(),
                    &Facts::detect(),
                    &mut out,
                )
            }
        });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // If standard error is closed, the exit status still tells.
            let mut err = io::stderr();
            let message = error.to_string();
            let _ = writeln!(err, "tidepool-bench: {}", printable(&message));
            if matches!(error, BenchError::Usage(_)) {
                let _ = writeln!(err, "`cargo xtask bench --help` shows the usage");
            }
            ExitCode::FAILURE
        }
    }
}
