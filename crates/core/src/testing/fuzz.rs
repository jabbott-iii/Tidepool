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

//! A dependency-free fuzzer for parsers: the engine behind each crate's
//! `tests/fuzz.rs` and `cargo xtask fuzz-smoke` (ADR-0027).
//!
//! Ported from Nadir's `xtask/src/fuzz.rs` (reading the seeds, describing
//! the findings). Adapted for Tidepool: the engine is a library that each
//! parser crate's tests use, rather than a part of `xtask` that links the
//! parsers; the saved crashes are replayed by every `cargo test`; seeds may
//! also come from test data already in the repository, read in place; the
//! mutations may use a dictionary of tokens; the written and the reserved
//! memory are checked; nothing is read or written through a symbolic link;
//! the run is configured by environment variables, and reports through its
//! return value, a panic message ending with the command that reproduces
//! it, and a result line for `fuzz-smoke` to read.
//!
//! A target is a function that takes any bytes: rejecting the input is
//! fine; a panic, a hang (an input that takes longer than [`HANG_LIMIT`])
//! or a rise of the memory past one of the target's caps is a finding.
//! Each parser crate registers its targets in its `tests/fuzz.rs`, one test
//! per target named after it, with seeds in `fuzz/corpus/<target>/`:
//!
//! ```no_run
//! use tidepool_core::testing::fuzz::Target;
//! # fn main() {}
//!
//! fn parse(input: &[u8]) {
//!     let _ = std::str::from_utf8(input);
//! }
//!
//! #[test]
//! fn utf8() {
//!     let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
//!     let outcome = Target::new(root, "utf8", parse)
//!         .dictionary(&[b"\xef\xbb\xbf", b"\xed\xa0\x80"])
//!         .run();
//!     // `fuzz-smoke` reads this line; libtest shows it only with
//!     // `--nocapture`, or when the test fails.
//!     println!("{}", outcome.summary());
//!     outcome.assert_clean();
//! }
//! ```
//!
//! # A run
//!
//! 1. **Unchanged inputs.** Every saved crash in `fuzz/crashes/<target>/`,
//!    in name order, then every seed: the files of `fuzz/corpus/<target>/`
//!    in name order, then those of each directory added with
//!    [`Target::seeds_in`]. So a saved crash is a regression test, and once
//!    its bug is fixed it keeps passing. Every `cargo test` does this much.
//! 2. **Mutated inputs**, only when [`SECS_VAR`] asks for them: each is a
//!    seed with 1 to 16 random changes, until that many seconds have
//!    passed. Input `n` depends on the seed ([`SEED_VAR`]), the target's
//!    name, the seeds and `n` alone, never on the time or on a saved crash:
//!    the time only decides when to stop. So a run can be replayed exactly,
//!    with more time if it ran more slowly.
//!
//! The target runs on a thread of its own with a [`STACK_SIZE`] stack,
//! the stack of a worker thread in Tidepool (`AGENTS.md` rule 6). A
//! watchdog, the calling thread, notices an input that takes longer than
//! [`HANG_LIMIT`]; such a thread cannot be stopped, so the run ends there.
//! The targets of one process run one at a time ([`Target::run_with`]
//! holds a lock), so that the memory one uses is not blamed on another,
//! until a hang: a thread stuck in a target keeps running after its run has
//! ended, so later runs in the process check no memory (`hung-thread`), and
//! when the stuck input does return, nothing more is recorded for it. The
//! [`Outcome`] says why the run stopped ([`Stop`]).
//!
//! # Findings
//!
//! - **A panic**, with the place it was raised: of the panic that escaped,
//!   when the target caught another during the same input (an earlier
//!   one, or one that a `drop` raised and caught while the escaping one
//!   unwound), which the finding also names.
//!   Panics from one place are one finding, however many inputs raise
//!   them. A place in the standard library (`/rustc/…`, `/library/…`),
//!   such as `Vec::with_capacity`'s "capacity overflow", can stand for
//!   several places in the target that reach it, all merged into one
//!   finding: the report says so. The first panic of an input does not
//!   print the usual message (a run can raise thousands); every later one
//!   during the same input, and any panic elsewhere, goes to the hook
//!   installed before. So when a panic aborts the process during unwinding
//!   (a `drop` that panics), only that later panic is printed.
//! - **A hang**: an input that did not return within [`HANG_LIMIT`], or
//!   that returned after it.
//! - **Written memory** ([`Kind::Memory`]): on Linux, the process's peak
//!   resident memory (`VmHWM`) rose more than the target's cap
//!   ([`Target::memory_cap`]) above where it was at the start. It is read
//!   after every unchanged input, and after mutated inputs once enough of
//!   the target's time has passed for the peak to have risen past the cap,
//!   at 1 MiB a microsecond, some 300 times faster than memory was measured
//!   to become resident: the input named is still the one that took the
//!   peak past the cap, as long as that bound holds.
//! - **Reserved memory** ([`Kind::Reserved`]): when [`SECS_VAR`] is set,
//!   even to 0 (in the runs `fuzz-smoke` starts, and in the reproduce
//!   command of a saved reserved-memory crash, each one target to a
//!   process), the process's peak address space (`VmPeak`, which counts what
//!   was reserved and never written) rose more than its cap
//!   ([`Target::reserved_cap`]) above the address space in use (`VmSize`)
//!   once the target's thread had made its first allocation. Address space
//!   grows at any rate, so a read on the budget names the range of mutated
//!   inputs since the last read ([`Found::Range`]) and saves none; a replay
//!   reads after every input, and names and saves the one. A plain `cargo
//!   test` does not check it, since its other tests' threads move it too,
//!   and says so.
//!
//! Where `/proc` cannot be read, the memory checks are skipped, and
//! [`Memory::Skipped`] says why. Both see the whole process: see
//! `memory.rs` for what they cannot see.
//!
//! A mutated input that fails is saved to `fuzz/crashes/<target>/` as
//! `<kind>-<hash>` ([`Kind::file_prefix`], and the input's FNV-1a hash in
//! 16 hex digits), written whole through a hidden file. An unchanged input
//! that fails is named where it is. [`Outcome::assert_clean`] then fails
//! the test with a report that names each file, its kind and the place,
//! and ends with the command that reproduces it.
//!
//! # A process that dies
//!
//! Some inputs end the process: an allocation too big to make aborts it,
//! a stack overflow kills it, and the operating system may kill a process
//! that takes all the memory. No report is possible then. Run the same
//! inputs again, with [`REPLAY_VAR`] set to `1`, and each input is first
//! written to the hidden `fuzz/crashes/<target>/.last-input`
//! ([`LAST_INPUT`]): when the process dies again, the input it died on is
//! there. `fuzz-smoke`, having seen the death (or an exit with libtest's
//! failure status without a result), keeps it as `died-<hash>`
//! ([`keep_died`]) if the replay wrote it; from then on it runs with the
//! saved crashes, as a regression test. A replay that finds a `.last-input` left over, by a
//! replay that died or was interrupted, reads it and runs it first. Before
//! that first input it writes it whole under a hidden name and renames it
//! over the old file, so it is on disk at every moment: a replay that
//! fails to start leaves it, and dying on it again leaves it again.
//!
//! When the operating system kills a replay for the memory it took,
//! `fuzz-smoke` may be killed with it, and then keeps nothing: systemd
//! stops every process of the unit after an out-of-memory kill unless its
//! `OOMPolicy` is `continue`. For a death that may be one, run the replay
//! as `systemd-run --user --scope -p OOMPolicy=continue cargo xtask
//! fuzz-smoke … --replay`. That covers the kernel's out-of-memory killer
//! only: `systemd-oomd`, where it runs (`systemctl is-active
//! systemd-oomd`), kills every process of the unit it picks whatever
//! `OOMPolicy` says, and then the next replay's note gives the command
//! that keeps the input by hand.
//!
//! A replay's [`SECS_VAR`] counts only the time spent inside the target,
//! so the same seconds usually reach the inputs of the run it replays.
//! Saving each input and reading the memory after it add about 8 µs an
//! input on the development machine, so a target whose normal run takes
//! less than about 0.5 µs an input (2 million inputs a second) can reach
//! the cap of 20 times the seconds in all first, and then more seconds
//! help. The outcome says why it stopped. A replay that ends without dying
//! removes its own file, and nothing else. Run one replay of a target at a
//! time: two would share the file.
//!
//! # Environment
//!
//! | Variable | Meaning | When not set |
//! | :- | :- | :- |
//! | [`SECS_VAR`] (`TIDEPOOL_FUZZ_SECS`) | Seconds to make mutated inputs for; set at all, it also turns on the reserved-memory check | None are made |
//! | [`SEED_VAR`] (`TIDEPOOL_FUZZ_SEED`) | The seed of the mutations: a `u64`, decimal or `0x` hex, as `TIDEPOOL_SEED` | [`DEFAULT_SEED`] |
//! | [`REPLAY_VAR`] (`TIDEPOOL_FUZZ_REPLAY`) | `1` to save each input before it runs | `0` |
//!
//! # Paths
//!
//! Every directory on the way from the root to `fuzz/corpus/<target>/`,
//! `fuzz/crashes/<target>/` and each [`Target::seeds_in`] directory is
//! looked at without following a link, and a link is refused
//! ([`FuzzError::Link`]); directories are made one level at a time. Not
//! covered: a directory swapped for a link while the run uses it.

mod engine;
mod memory;
mod mutate;
mod paths;

use std::env::{self, VarError};
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

pub use self::paths::{Kept, keep_died};
use super::{DEFAULT_SEED, parse_seed};

/// The environment variable that asks for mutated inputs: how many seconds
/// to make them for, a whole number. Without it, a run is only the
/// unchanged inputs. Set at all, even to `0`, it also turns on the
/// reserved-memory check ([`Config::check_reserved`]).
pub const SECS_VAR: &str = "TIDEPOOL_FUZZ_SECS";

/// The environment variable that sets the seed of the mutations: a `u64`,
/// in decimal or as `0x`-prefixed hex, as for `TIDEPOOL_SEED`.
pub const SEED_VAR: &str = "TIDEPOOL_FUZZ_SEED";

/// The environment variable that turns on replay mode: `1` to write each
/// input to [`LAST_INPUT`] in the crash directory before it runs, and to
/// check the memory after every input; `0` (or not set) for a normal run.
/// A replay's [`SECS_VAR`] counts only the time spent inside the target
/// ([`Config::replay`]).
pub const REPLAY_VAR: &str = "TIDEPOOL_FUZZ_REPLAY";

/// The hidden file in a target's crash directory that a replay writes each
/// input to before it runs. Being hidden, it is never read as a crash;
/// once a replay has died, [`keep_died`] renames it to `died-<hash>`
/// ([`DIED_PREFIX`]), and a replay that finds one left over runs it first.
pub const LAST_INPUT: &str = ".last-input";

/// What the name of an input that a replay died on starts with:
/// `died-<hash>`, its FNV-1a hash in 16 hex digits.
pub const DIED_PREFIX: &str = "died";

/// What the line [`Summary`] writes starts with, so that `fuzz-smoke` can
/// find it in the output of a test.
pub const RESULT_PREFIX: &str = "fuzz-result:";

/// How long one input may take before it counts as a hang.
///
/// Inputs mutated from the seeds take microseconds in `fuzz-smoke`'s
/// optimized profile, and the slowest few milliseconds. In an unoptimized
/// `cargo test` the seeds run perhaps 20 times slower, on a CI machine that
/// runs many tests at once. Five seconds leaves room for both, and finds
/// any loop that does not end, five seconds later.
pub const HANG_LIMIT: Duration = Duration::from_secs(5);

/// The stack of the thread that runs a target: 2 MiB, which is what Rust
/// gives a spawned thread unless `RUST_MIN_STACK` says otherwise, and so
/// what a parser has on a worker thread in Tidepool. It is set explicitly,
/// so that the variable does not hide a deep recursion.
pub const STACK_SIZE: usize = 2 * 1024 * 1024;

/// The longest input a mutation may make, in bytes.
pub const MAX_LEN: usize = 64 * 1024;

/// The largest seed or saved crash read, in bytes. A larger file is an
/// error, not a seed.
pub const MAX_SEED_BYTES: u64 = 1024 * 1024;

/// The cap on the rise of the written peak that a [`Target`] has unless it
/// sets its own ([`Target::memory_cap`]): 64 MiB.
pub const DEFAULT_MEMORY_CAP: u64 = 64 * 1024 * 1024;

/// The cap on the reserved peak that a [`Target`] has unless it sets its
/// own ([`Target::reserved_cap`]): 256 MiB above the address space in use.
/// A thread's arena and stack move it by tens of MiB.
pub const DEFAULT_RESERVED_CAP: u64 = 256 * 1024 * 1024;

/// Held while a target runs, so that the targets of one process never run
/// at once.
static RUN: Mutex<()> = Mutex::new(());

/// One fuzz target: its name, the function it runs, where its seeds and
/// crashes are, its dictionary and its memory caps.
#[derive(Clone, Debug)]
pub struct Target {
    name: String,
    run: fn(&[u8]),
    root: PathBuf,
    extra: Vec<SeedDir>,
    dictionary: &'static [&'static [u8]],
    memory_cap: u64,
    reserved_cap: u64,
}

/// A directory of seeds read in place, relative to the root, with the
/// extensions of the files to take.
#[derive(Clone, Debug)]
struct SeedDir {
    dir: PathBuf,
    extensions: Vec<String>,
}

impl Target {
    /// A target called `name` that hands each input to `run`, for the
    /// repository at `root`: its seeds are in `root/fuzz/corpus/<name>/`,
    /// and its crashes in `root/fuzz/crashes/<name>/`.
    ///
    /// # Panics
    ///
    /// If `name` is empty or has anything but ASCII lower-case letters,
    /// digits, `-` and `_`: it names directories and goes on a command
    /// line.
    #[track_caller]
    pub fn new(root: impl Into<PathBuf>, name: &str, run: fn(&[u8])) -> Self {
        let valid = !name.is_empty()
            && name
                .bytes()
                .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_'));
        assert!(
            valid,
            "a fuzz target's name is lower-case ASCII letters, digits, `-` and `_`, not {name:?}"
        );
        Self {
            name: name.to_string(),
            run,
            root: root.into(),
            extra: Vec::new(),
            dictionary: &[],
            memory_cap: DEFAULT_MEMORY_CAP,
            reserved_cap: DEFAULT_RESERVED_CAP,
        }
    }

    /// Also takes as seeds the files under `dir`, a path relative to the
    /// root, whose extension is one of `extensions`, read in place, in the
    /// order of their paths. Subdirectories are read too. Mark the
    /// directory `-text` in `.gitattributes`, so that every checkout gives
    /// the same seeds.
    #[must_use]
    pub fn seeds_in(mut self, dir: impl AsRef<Path>, extensions: &[&str]) -> Self {
        self.extra.push(SeedDir {
            dir: dir.as_ref().to_path_buf(),
            extensions: extensions.iter().map(ToString::to_string).collect(),
        });
        self
    }

    /// Tokens that mutations insert or write over the input's bytes: the
    /// syntax of the format, such as brackets, keywords and escapes.
    #[must_use]
    pub fn dictionary(mut self, tokens: &'static [&'static [u8]]) -> Self {
        self.dictionary = tokens;
        self
    }

    /// How far the written peak may rise above where it was at the start of
    /// the run, in bytes. [`DEFAULT_MEMORY_CAP`] unless set.
    #[must_use]
    pub fn memory_cap(mut self, bytes: u64) -> Self {
        self.memory_cap = bytes;
        self
    }

    /// How far the reserved peak may rise above the address space in use
    /// (`VmSize`) once the target's thread has made its first allocation,
    /// in bytes (or above the peak then, if that is higher).
    /// [`DEFAULT_RESERVED_CAP`] unless set.
    #[must_use]
    pub fn reserved_cap(mut self, bytes: u64) -> Self {
        self.reserved_cap = bytes;
        self
    }

    /// The target's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// `fuzz/corpus/<name>/` under the root.
    pub fn corpus(&self) -> PathBuf {
        self.root.join(paths::corpus_dir(&self.name))
    }

    /// `fuzz/crashes/<name>/` under the root.
    pub fn crashes(&self) -> PathBuf {
        self.root.join(paths::crash_dir(&self.name))
    }

    /// Runs the target as the environment says ([`Config::from_env`]).
    ///
    /// # Panics
    ///
    /// If the run cannot start or finish: a variable that does not parse,
    /// a seed directory that is missing, empty or holds a link, a crash
    /// that cannot be saved. Findings do not panic here; see
    /// [`Outcome::assert_clean`].
    #[track_caller]
    pub fn run(&self) -> Outcome {
        let run = Config::from_env().and_then(|config| self.run_with(&config));
        match run {
            Ok(outcome) => outcome,
            Err(error) => panic!("fuzz target `{}` could not run: {error}", self.name),
        }
    }

    /// Runs the target with `config`. Waits for any other target of this
    /// process to finish first.
    ///
    /// # Errors
    ///
    /// [`FuzzError`] if the seeds or crashes cannot be read, a directory on
    /// the way to them is a link, a finding cannot be saved, or the
    /// target's thread cannot be started.
    pub fn run_with(&self, config: &Config) -> Result<Outcome, FuzzError> {
        let _running = RUN.lock().unwrap_or_else(PoisonError::into_inner);
        // What an earlier replay left runs first in this one: if that
        // replay died on it, this one dies on it again, and leaves it. It
        // stays on disk until this replay's own file replaces it.
        let leftover = if config.replay {
            paths::leftover(&self.root, &self.name)?
        } else {
            None
        };
        let crashes = read_crashes(&self.root, &paths::crash_dir(&self.name))?;
        let mut seeds = read_corpus(&self.root, &paths::corpus_dir(&self.name))?;
        for extra in &self.extra {
            let before = seeds.len();
            read_tree(&self.root, &extra.dir, &extra.extensions, &mut seeds)?;
            if seeds.len() == before {
                return Err(FuzzError::NoSeeds {
                    path: self.root.join(&extra.dir),
                });
            }
        }
        let (seed_count, crash_count) = (seeds.len(), crashes.len());
        let reproduce = reproduce(&self.name);
        let ran = engine::run(engine::Plan {
            name: self.name.clone(),
            run: self.run,
            root: self.root.clone(),
            crashes,
            seeds,
            dictionary: self.dictionary,
            leftover,
            memory_cap: self.memory_cap,
            reserved_cap: self.reserved_cap,
            config: config.clone(),
        })?;
        Ok(Outcome {
            target: self.name.clone(),
            root: self.root.clone(),
            config: config.clone(),
            seeds: seed_count,
            crashes: crash_count,
            inputs: ran.inputs,
            elapsed: ran.elapsed,
            findings: ran.findings,
            stopped: ran.stopped,
            slowest: ran.slowest,
            memory: ran.memory,
            reserved: ran.reserved,
            saved: ran.saved,
            reproduce,
        })
    }
}

/// How one run goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// Decides every mutation: the same seed gives the same inputs in the
    /// same order.
    pub seed: u64,
    /// How long to make mutated inputs for. The unchanged inputs always all
    /// run, even when this is zero.
    pub duration: Duration,
    /// Stop after this many mutated inputs, if the time has not run out
    /// first.
    pub max_inputs: u64,
    /// An input that keeps the target busy for longer than this is a hang.
    pub hang_limit: Duration,
    /// The longest input a mutation may make, in bytes.
    pub max_len: usize,
    /// Write each input to [`LAST_INPUT`] in the crash directory before it
    /// runs, and check the memory after every input, which add about 8 µs
    /// an input. So a replay's `duration` counts only the time spent inside
    /// the target: less than the whole time of the run it replays, so that
    /// it usually gets as far as that run did, though not always, since the
    /// target's time varies. It also stops at 20 times `duration` in all,
    /// which a target whose normal run takes less than about 0.5 µs an
    /// input can reach first; more seconds then help. A `.last-input` left
    /// by an earlier replay runs first. Run one replay of a target at a
    /// time.
    pub replay: bool,
    /// Check the reserved peak (`VmPeak`) too. [`Config::from_env`] sets it
    /// when [`SECS_VAR`] is set, even to 0: in the runs `fuzz-smoke` starts,
    /// and in the reproduce command of a saved reserved-memory crash, each
    /// one target to a process. Only for such a process: a thread that
    /// starts anywhere in it moves the peak by tens of MiB.
    pub check_reserved: bool,
}

impl Default for Config {
    /// The unchanged inputs only, as a plain `cargo test` runs them.
    fn default() -> Self {
        Self {
            seed: DEFAULT_SEED,
            duration: Duration::ZERO,
            max_inputs: u64::MAX,
            hang_limit: HANG_LIMIT,
            max_len: MAX_LEN,
            replay: false,
            check_reserved: false,
        }
    }
}

impl Config {
    /// The configuration that [`SECS_VAR`], [`SEED_VAR`] and [`REPLAY_VAR`]
    /// give, the rest as [`Config::default`]. [`SECS_VAR`] set at all
    /// turns on [`Config::check_reserved`].
    ///
    /// # Errors
    ///
    /// [`FuzzError::Var`] if one is set to something it cannot be.
    pub fn from_env() -> Result<Self, FuzzError> {
        Self::from_vars(env::var)
    }

    /// [`Config::from_env`] with the variables read by `var`.
    fn from_vars(
        var: impl Fn(&'static str) -> Result<String, VarError>,
    ) -> Result<Self, FuzzError> {
        let read = |name: &'static str, expected: &'static str| match var(name) {
            Ok(value) => Ok(Some(value)),
            Err(VarError::NotPresent) => Ok(None),
            Err(VarError::NotUnicode(value)) => Err(FuzzError::Var {
                name,
                value: value.to_string_lossy().into_owned(),
                expected,
            }),
        };
        let bad = |name, value: String, expected| FuzzError::Var {
            name,
            value,
            expected,
        };
        let mut config = Self::default();
        const SECONDS: &str = "a whole number of seconds";
        if let Some(text) = read(SECS_VAR, SECONDS)? {
            let secs: u64 = text
                .trim()
                .parse()
                .map_err(|_| bad(SECS_VAR, text.clone(), SECONDS))?;
            config.duration = Duration::from_secs(secs);
            config.check_reserved = true;
        }
        const SEED: &str = "a u64, in decimal or as 0x-prefixed hex";
        if let Some(text) = read(SEED_VAR, SEED)? {
            config.seed = parse_seed(&text).ok_or_else(|| bad(SEED_VAR, text.clone(), SEED))?;
        }
        const SWITCH: &str = "`1` or `0`";
        if let Some(text) = read(REPLAY_VAR, SWITCH)? {
            config.replay = match text.trim() {
                "1" => true,
                "0" | "" => false,
                _ => return Err(bad(REPLAY_VAR, text, SWITCH)),
            };
        }
        Ok(config)
    }
}

/// What went wrong with an input.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// The target panicked.
    Panic,
    /// The target did not return within [`Config::hang_limit`], or returned
    /// after it.
    Hang,
    /// The written peak (`VmHWM`) rose past the target's cap after the
    /// input.
    Memory,
    /// The reserved peak (`VmPeak`) rose past the target's cap.
    Reserved,
}

impl Kind {
    /// What the file of a saved input starts with: `panic`, `hang`,
    /// `memory` or `reserve`.
    pub fn file_prefix(self) -> &'static str {
        match self {
            Self::Panic => "panic",
            Self::Hang => "hang",
            Self::Memory => "memory",
            Self::Reserved => "reserve",
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Panic => "panic",
            Self::Hang => "hang",
            Self::Memory => "over-allocation",
            Self::Reserved => "over-reservation",
        })
    }
}

/// Where the input of a finding is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    /// In a file: where it was saved, or for an unchanged input, where it
    /// was read from.
    File {
        /// The file.
        path: PathBuf,
        /// The input's length in bytes.
        len: usize,
        /// Its number among the mutated inputs, from 0; `None` for an
        /// unchanged input.
        number: Option<u64>,
    },
    /// One of the mutated inputs `first..=last`, none of them saved: a rise
    /// of the reserved peak that a read on the budget saw. A replay names
    /// and saves the one.
    Range {
        /// The first mutated input since the read before.
        first: u64,
        /// The last.
        last: u64,
    },
}

/// One distinct failure, and the first input that caused it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    /// What went wrong.
    pub kind: Kind,
    /// For a panic, where it was raised, as `file:line:column` (the
    /// message if the place is not known). Panics from the same place are
    /// one finding. When the target raised more than one panic during the
    /// input, it is the last, the one that escaped.
    pub site: Option<String>,
    /// For a panic during an input that raised another, which the target
    /// caught: the caught one's place and message. Usually it came first,
    /// and the target raised the one that escaped after catching it; it can
    /// also be a panic that a `drop` raised and caught while the one that
    /// escaped unwound.
    pub earlier: Option<String>,
    /// The panic message, how long the target was given, or how far the
    /// memory rose.
    pub message: String,
    /// Where the input is.
    pub input: Found,
    /// How many inputs failed this way.
    pub hits: u64,
}

impl Finding {
    /// The file that holds the input, if one does.
    pub fn path(&self) -> Option<&Path> {
        match &self.input {
            Found::File { path, .. } => Some(path),
            Found::Range { .. } => None,
        }
    }

    /// Whether the place of the panic is in the standard library, where
    /// several places in the target can end up.
    pub fn site_in_std(&self) -> bool {
        self.site.as_deref().is_some_and(in_std)
    }
}

/// Whether `site`, a panic's place, is in the standard library.
fn in_std(site: &str) -> bool {
    site.starts_with("/rustc/") || site.contains("/library/")
}

/// The slowest input of a run that returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slowest {
    /// How long the target took on it.
    pub took: Duration,
    /// Its length in bytes.
    pub len: usize,
}

/// Why a check did not run, or stopped: a short code for `fuzz-smoke`'s
/// line (`unreadable`, `no-vmhwm`, `no-vmpeak`, `not-asked`,
/// `hung-thread`, `read-failed`, `not-started`), and the reason in words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skip {
    /// The code, without white space.
    pub code: &'static str,
    /// The reason.
    pub reason: String,
}

/// What a memory check saw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Memory {
    /// The check ran. The sizes are in bytes.
    Measured {
        /// What the cap counts from. For the written check, the peak at
        /// the start of the run. For the reserved check, the larger of the
        /// address space in use (`VmSize`) at the start and the peak then
        /// less the cap: a peak already more than the cap above what was
        /// in use is a finding only once it rises further.
        baseline: u64,
        /// The peak at the end of the run, or where a read failed.
        peak: u64,
        /// How far above `baseline` it was allowed to rise.
        cap: u64,
        /// Why it stopped partway, if it did: what it measured is kept.
        stopped: Option<Skip>,
    },
    /// The check did not run, and why.
    Skipped(Skip),
}

/// Why a run stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stop {
    /// Only the unchanged inputs were asked for.
    Unchanged,
    /// The time ran out.
    Time,
    /// A replay's time in the target ran out.
    TargetTime,
    /// A replay reached 20 times its time in all.
    WallCap,
    /// [`Config::max_inputs`] mutated inputs ran.
    MaxInputs,
    /// An input did not return; its thread is still busy.
    Hang,
    /// An input returned after [`Config::hang_limit`].
    SlowInput,
}

impl Stop {
    /// The code in the result line: `unchanged`, `time`, `target-time`,
    /// `wall-cap`, `max-inputs`, `hang` or `slow-input`.
    pub fn code(self) -> &'static str {
        match self {
            Self::Unchanged => "unchanged",
            Self::Time => "time",
            Self::TargetTime => "target-time",
            Self::WallCap => "wall-cap",
            Self::MaxInputs => "max-inputs",
            Self::Hang => "hang",
            Self::SlowInput => "slow-input",
        }
    }

    /// The reason for `code`, as [`Stop`]'s `Display` gives it, if `code`
    /// is one.
    pub fn describe(code: &str) -> Option<&'static str> {
        Some(match code {
            "unchanged" => "only the unchanged inputs were asked for",
            "time" => "its time ran out",
            "target-time" => "the replay's time in the target ran out",
            "wall-cap" => "the replay reached 20 times its seconds in all",
            "max-inputs" => "it ran as many inputs as it was allowed",
            "hang" => "an input did not return, and its thread is still busy",
            "slow-input" => "an input returned after the hang limit",
            _ => return None,
        })
    }
}

impl fmt::Display for Stop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(Self::describe(self.code()).unwrap_or(""))
    }
}

/// What one run of a target did.
#[derive(Clone, Debug)]
pub struct Outcome {
    /// The target's name.
    pub target: String,
    root: PathBuf,
    /// How the run went.
    pub config: Config,
    /// How many seeds there were.
    pub seeds: usize,
    /// How many saved crashes there were.
    pub crashes: usize,
    /// How many inputs the target finished, the unchanged ones included.
    pub inputs: u64,
    /// How long the run took.
    pub elapsed: Duration,
    /// What failed. Empty if the run was clean.
    pub findings: Vec<Finding>,
    /// Why the run stopped.
    pub stopped: Stop,
    /// The slowest input that returned. A target far slower on some input
    /// than on the rest has a path that grows faster than its input, short
    /// of a hang.
    pub slowest: Option<Slowest>,
    /// What the written-memory check saw.
    pub memory: Memory,
    /// What the reserved-memory check saw.
    pub reserved: Memory,
    /// How many findings were saved to files in this run.
    pub saved: usize,
    /// The command that runs the same test again.
    pub reproduce: String,
}

impl Outcome {
    /// Whether nothing was found.
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }

    /// Whether the run ended at a hang.
    pub fn hung(&self) -> bool {
        matches!(self.stopped, Stop::Hang | Stop::SlowInput)
    }

    /// The line that reports this run to `fuzz-smoke`.
    pub fn summary(&self) -> Summary {
        Summary {
            target: self.target.clone(),
            seed: self.config.seed,
            seeds: self.seeds,
            crashes: self.crashes,
            inputs: self.inputs,
            elapsed: self.elapsed,
            findings: self.findings.len(),
            saved: self.saved,
            ranges: self
                .findings
                .iter()
                .filter(|finding| matches!(finding.input, Found::Range { .. }))
                .count(),
            hung: self.hung(),
            stopped: self.stopped.code().to_string(),
            memory: Reading::of(&self.memory),
            reserved: Reading::of(&self.reserved),
            slowest: self.slowest,
        }
    }

    /// Fails the test if anything was found, with this outcome's report.
    ///
    /// # Panics
    ///
    /// If [`Outcome::findings`] is not empty.
    #[track_caller]
    pub fn assert_clean(&self) {
        if !self.is_clean() {
            panic!("{self}");
        }
    }

    /// `path`, relative to the root when it is under it, with `/` between
    /// its parts.
    fn shown(&self, path: &Path) -> String {
        match path.strip_prefix(&self.root) {
            Ok(inside) => inside
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/"),
            Err(_) => path.display().to_string(),
        }
    }

    /// The command that replays this run, saving each input and checking
    /// the memory after each.
    fn replay_command(&self) -> String {
        format!(
            "{SEED_VAR}=0x{:016x} {SECS_VAR}={} {REPLAY_VAR}=1 {}",
            self.config.seed,
            self.config.duration.as_secs(),
            self.reproduce.replacen(
                "cargo test ",
                &format!("cargo test --profile {FUZZ_PROFILE} "),
                1
            )
        )
    }
}

/// The cargo profile `fuzz-smoke` builds the targets in.
const FUZZ_PROFILE: &str = "fuzz";

impl fmt::Display for Outcome {
    /// The report: what ran, each finding and its input, why the run
    /// stopped, the memory, and the command that reproduces it.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let verdict = match self.findings.len() {
            0 => "no findings".to_string(),
            1 => "FAILED: 1 finding".to_string(),
            count => format!("FAILED: {count} findings"),
        };
        writeln!(
            f,
            "fuzz target `{}`: {verdict} in {} (seed 0x{:016x}, {} seeds, {} saved crashes, {:.1} s)",
            self.target,
            plural(self.inputs, "input"),
            self.config.seed,
            self.seeds,
            self.crashes,
            self.elapsed.as_secs_f64()
        )?;
        for finding in &self.findings {
            match (&finding.kind, &finding.site) {
                (Kind::Panic, Some(site)) if *site != finding.message => {
                    writeln!(f, "  panic at {site}: {}", finding.message)?;
                }
                (kind, _) => writeln!(f, "  {kind}: {}", finding.message)?,
            }
            match &finding.input {
                Found::File { path, len, number } => {
                    write!(
                        f,
                        "    input: {} ({})",
                        self.shown(path),
                        plural(*len as u64, "byte")
                    )?;
                    if let Some(number) = number {
                        write!(f, ", mutated input {number}")?;
                    }
                }
                Found::Range { first, last } => write!(
                    f,
                    "    input: one of the mutated inputs {first} to {last}, none of them saved; \
                     a replay reads after every input, and names and saves the one"
                )?,
            }
            match finding.hits {
                1 => writeln!(f)?,
                hits => writeln!(f, ", the first of {hits} inputs that did this")?,
            }
            if let Some(earlier) = &finding.earlier {
                writeln!(
                    f,
                    "    another panic during the same input, which the target caught, was at \
                     {earlier}"
                )?;
            }
            if finding.site_in_std() {
                writeln!(
                    f,
                    "    that place is in the standard library: other places in the target \
                     that panic there are merged into this finding"
                )?;
            }
        }
        writeln!(f, "  stopped: {}", self.stopped)?;
        write_memory(f, "written memory", &self.memory)?;
        write_memory(f, "reserved memory", &self.reserved)?;
        let reserved_file = self
            .findings
            .iter()
            .any(|finding| finding.kind == Kind::Reserved && finding.path().is_some());
        if reserved_file {
            // A plain `cargo test` does not check reserved memory.
            write!(f, "  to reproduce: {SECS_VAR}=0 {}", self.reproduce)?;
        } else {
            write!(f, "  to reproduce: {}", self.reproduce)?;
        }
        if self
            .findings
            .iter()
            .any(|finding| matches!(finding.input, Found::Range { .. }))
        {
            write!(f, "\n  to name the input: {}", self.replay_command())?;
        }
        Ok(())
    }
}

/// A memory check's line in the report.
fn write_memory(f: &mut fmt::Formatter<'_>, what: &str, memory: &Memory) -> fmt::Result {
    match memory {
        Memory::Measured {
            baseline,
            peak,
            cap,
            stopped,
        } => {
            write!(
                f,
                "  {what}: {} (+{} of a {} cap)",
                engine::mib(*peak),
                engine::mib(peak.saturating_sub(*baseline)),
                engine::mib(*cap)
            )?;
            match stopped {
                Some(skip) => writeln!(f, "; then not checked: {}", skip.reason),
                None => writeln!(f),
            }
        }
        Memory::Skipped(skip) => writeln!(f, "  {what}: not checked: {}", skip.reason),
    }
}

/// The parts of a run's [`Outcome`] that `fuzz-smoke` reports, written as
/// one line that starts with [`RESULT_PREFIX`] and read back with
/// [`Summary::parse`]:
///
/// ```text
/// fuzz-result: target=json seed=0x… seeds=21 crashes=0 inputs=1200000 micros=60000412 findings=0 saved=0 ranges=0 hung=0 stopped=time memory=measured memory_baseline=… memory_peak=… memory_cap=… reserved=measured reserved_baseline=… reserved_peak=… reserved_cap=… slowest_micros=310 slowest_bytes=4096
/// ```
///
/// A check that did not run has `memory=skipped memory_reason=<code>`; one
/// that stopped partway adds `memory_stopped=<code>` (and the same with
/// `reserved_`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    /// The target's name.
    pub target: String,
    /// The seed of the mutations.
    pub seed: u64,
    /// How many seeds there were.
    pub seeds: usize,
    /// How many saved crashes there were.
    pub crashes: usize,
    /// How many inputs the target finished.
    pub inputs: u64,
    /// How long the run took, to the microsecond.
    pub elapsed: Duration,
    /// How many distinct findings there were.
    pub findings: usize,
    /// How many of them were saved to files in this run.
    pub saved: usize,
    /// How many of them name a range of inputs that a replay narrows down.
    pub ranges: usize,
    /// Whether the run ended at a hang.
    pub hung: bool,
    /// Why the run stopped: a [`Stop::code`].
    pub stopped: String,
    /// The written-memory check.
    pub memory: Reading,
    /// The reserved-memory check.
    pub reserved: Reading,
    /// The slowest input that returned, to the microsecond.
    pub slowest: Option<Slowest>,
}

/// A memory check, as a [`Summary`] carries it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reading {
    /// It ran.
    Measured(PeakMemory),
    /// It did not run: the [`Skip::code`].
    Skipped(String),
}

impl Reading {
    fn of(memory: &Memory) -> Self {
        match memory {
            Memory::Measured {
                baseline,
                peak,
                cap,
                stopped,
            } => Self::Measured(PeakMemory {
                baseline: *baseline,
                peak: *peak,
                cap: *cap,
                stopped: stopped.as_ref().map(|skip| skip.code.to_string()),
            }),
            Memory::Skipped(skip) => Self::Skipped(skip.code.to_string()),
        }
    }

    fn write(&self, f: &mut fmt::Formatter<'_>, key: &str) -> fmt::Result {
        match self {
            Self::Measured(PeakMemory {
                baseline,
                peak,
                cap,
                stopped,
            }) => {
                write!(
                    f,
                    " {key}=measured {key}_baseline={baseline} {key}_peak={peak} {key}_cap={cap}"
                )?;
                match stopped {
                    Some(code) => write!(f, " {key}_stopped={code}"),
                    None => Ok(()),
                }
            }
            Self::Skipped(code) => write!(f, " {key}=skipped {key}_reason={code}"),
        }
    }

    fn parse(get: &dyn Fn(&str) -> Option<String>, key: &str) -> Option<Self> {
        let number = |name: &str| get(&format!("{key}_{name}"))?.parse::<u64>().ok();
        match get(key)?.as_str() {
            "measured" => Some(Self::Measured(PeakMemory {
                baseline: number("baseline")?,
                peak: number("peak")?,
                cap: number("cap")?,
                stopped: get(&format!("{key}_stopped")),
            })),
            "skipped" => Some(Self::Skipped(get(&format!("{key}_reason"))?)),
            _ => None,
        }
    }
}

/// What one memory check measured, in bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeakMemory {
    /// What the cap counts from, as for [`Memory::Measured`]: the written
    /// peak at the start; for the reserved check, the larger of the
    /// address space in use at the start and the peak then less the cap.
    pub baseline: u64,
    /// At the end, or where a read failed.
    pub peak: u64,
    /// How far above `baseline` it was allowed to rise.
    pub cap: u64,
    /// The [`Skip::code`] of why it stopped partway, if it did.
    pub stopped: Option<String>,
}

impl fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{RESULT_PREFIX} target={} seed=0x{:016x} seeds={} crashes={} inputs={} micros={} \
             findings={} saved={} ranges={} hung={} stopped={}",
            self.target,
            self.seed,
            self.seeds,
            self.crashes,
            self.inputs,
            self.elapsed.as_micros(),
            self.findings,
            self.saved,
            self.ranges,
            u8::from(self.hung),
            self.stopped
        )?;
        self.memory.write(f, "memory")?;
        self.reserved.write(f, "reserved")?;
        if let Some(Slowest { took, len }) = self.slowest {
            write!(
                f,
                " slowest_micros={} slowest_bytes={len}",
                took.as_micros()
            )?;
        }
        Ok(())
    }
}

impl Summary {
    /// Reads a line that [`Summary`] wrote, with any white space around it.
    /// Returns `None` for any other line.
    pub fn parse(line: &str) -> Option<Self> {
        let rest = line.trim().strip_prefix(RESULT_PREFIX)?;
        let mut fields = Vec::new();
        for field in rest.split_ascii_whitespace() {
            let (key, value) = field.split_once('=')?;
            if fields.iter().any(|&(seen, _)| seen == key) {
                return None;
            }
            fields.push((key, value));
        }
        let get = |key: &str| {
            fields
                .iter()
                .find(|&&(name, _)| name == key)
                .map(|&(_, value)| value.to_string())
        };
        let number = |key: &str| get(key)?.parse::<u64>().ok();
        let count = |key: &str| usize::try_from(number(key)?).ok();
        let micros = |key: &str| number(key).map(Duration::from_micros);
        let slowest = match (get("slowest_micros"), get("slowest_bytes")) {
            (None, None) => None,
            (Some(_), Some(_)) => Some(Slowest {
                took: micros("slowest_micros")?,
                len: count("slowest_bytes")?,
            }),
            _ => return None,
        };
        Some(Self {
            target: get("target")?,
            seed: parse_seed(&get("seed")?)?,
            seeds: count("seeds")?,
            crashes: count("crashes")?,
            inputs: number("inputs")?,
            elapsed: micros("micros")?,
            findings: count("findings")?,
            saved: count("saved")?,
            ranges: count("ranges")?,
            hung: match get("hung")?.as_str() {
                "0" => false,
                "1" => true,
                _ => return None,
            },
            stopped: get("stopped")?,
            memory: Reading::parse(&get, "memory")?,
            reserved: Reading::parse(&get, "reserved")?,
            slowest,
        })
    }
}

/// Why a run could not start or finish.
#[derive(Debug)]
pub enum FuzzError {
    /// An environment variable is set to something it cannot be.
    Var {
        /// The variable.
        name: &'static str,
        /// Its value (lossily, if it is not Unicode).
        value: String,
        /// What it should be.
        expected: &'static str,
    },
    /// A directory or file of seeds or crashes cannot be read.
    Read {
        /// The directory or file.
        path: PathBuf,
        /// What the OS reported.
        source: io::Error,
    },
    /// A symbolic link where the fuzzer reads or writes: on the way to a
    /// seed or crash directory, in one, or at the replay's file. It is never
    /// followed.
    Link {
        /// The link.
        path: PathBuf,
    },
    /// An entry where a directory or a regular file should be, which is
    /// neither: a seed directory that is a file, or a directory in a
    /// corpus, which is flat.
    NotAFile {
        /// The entry.
        path: PathBuf,
    },
    /// A seed directory named by a path that leaves the root (`..`, or an
    /// absolute path).
    NotUnderRoot {
        /// The path, as given.
        path: PathBuf,
    },
    /// A seed or saved crash is larger than [`MAX_SEED_BYTES`].
    TooLarge {
        /// The file.
        path: PathBuf,
    },
    /// A seed directory holds no seed.
    NoSeeds {
        /// The directory.
        path: PathBuf,
    },
    /// A finding, the replay's file or a directory for them cannot be
    /// written.
    Write {
        /// The file or directory.
        path: PathBuf,
        /// What the OS reported.
        source: io::Error,
    },
    /// The thread that runs the target could not be started, or stopped
    /// without a result.
    Thread(String),
}

impl fmt::Display for FuzzError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Var {
                name,
                value,
                expected,
            } => write!(f, "{name} must be {expected}, but it is {value:?}"),
            Self::Read { path, source } => {
                write!(f, "cannot read `{}`: {source}", path.display())
            }
            Self::Link { path } => write!(
                f,
                "`{}` is a symbolic link, which the fuzzer never follows",
                path.display()
            ),
            Self::NotAFile { path } => write!(
                f,
                "`{}` is not a regular file or directory where one should be",
                path.display()
            ),
            Self::NotUnderRoot { path } => write!(
                f,
                "`{}` must be a path inside the repository, without `..`",
                path.display()
            ),
            Self::TooLarge { path } => write!(
                f,
                "`{}` is larger than {MAX_SEED_BYTES} bytes, the most a seed may be",
                path.display()
            ),
            Self::NoSeeds { path } => write!(f, "`{}` holds no seeds", path.display()),
            Self::Write { path, source } => {
                write!(f, "cannot write `{}`: {source}", path.display())
            }
            Self::Thread(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for FuzzError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } | Self::Write { source, .. } => Some(source),
            Self::Var { .. }
            | Self::Link { .. }
            | Self::NotAFile { .. }
            | Self::NotUnderRoot { .. }
            | Self::TooLarge { .. }
            | Self::NoSeeds { .. }
            | Self::Thread(_) => None,
        }
    }
}

/// The command that runs the current test again: the package from
/// `CARGO_PKG_NAME`, which `cargo test` sets, and the test from the name of
/// the current thread, which libtest sets to the test's name. Every fuzz
/// test is in its crate's `tests/fuzz.rs` (ADR-0027).
fn reproduce(target: &str) -> String {
    let package = env::var("CARGO_PKG_NAME")
        .ok()
        .filter(|package| !package.is_empty())
        .unwrap_or_else(|| "<package>".to_string());
    let thread = std::thread::current();
    let test = match thread.name() {
        Some(test) if test != "main" && !test.is_empty() => test.to_string(),
        _ => target.to_string(),
    };
    format!("cargo test -p {package} --test fuzz -- --exact {test}")
}

/// `1 input`, `2 inputs`.
fn plural(count: u64, noun: &str) -> String {
    match count {
        1 => format!("1 {noun}"),
        count => format!("{count} {noun}s"),
    }
}

/// The saved crashes in `relative` under `root`, in name order; none if it
/// does not exist.
fn read_crashes(root: &Path, relative: &Path) -> Result<Vec<(PathBuf, Vec<u8>)>, FuzzError> {
    if paths::walk(root, relative)? == paths::Walked::Missing {
        return Ok(Vec::new());
    }
    let dir = root.join(relative);
    // A replay in another process can remove a crash directory it made, and
    // a file can go between the listing and the read: both mean nothing to
    // read.
    let listed = match entries(&dir, false) {
        Ok(listed) => listed,
        Err(FuzzError::Read { path, source })
            if path == dir && source.kind() == io::ErrorKind::NotFound =>
        {
            return Ok(Vec::new());
        }
        Err(error) => return Err(error),
    };
    let mut crashes = Vec::new();
    for path in listed {
        match read_seed(&path) {
            Ok(bytes) => crashes.push((path, bytes)),
            Err(FuzzError::Read { source, .. }) if source.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(crashes)
}

/// The seeds in `relative` under `root`, which must exist and hold at
/// least one, in name order.
fn read_corpus(root: &Path, relative: &Path) -> Result<Vec<(PathBuf, Vec<u8>)>, FuzzError> {
    let dir = root.join(relative);
    if paths::walk(root, relative)? == paths::Walked::Missing {
        return Err(FuzzError::Read {
            path: dir,
            source: io::ErrorKind::NotFound.into(),
        });
    }
    let mut seeds = Vec::new();
    for path in entries(&dir, false)? {
        let bytes = read_seed(&path)?;
        seeds.push((path, bytes));
    }
    if seeds.is_empty() {
        return Err(FuzzError::NoSeeds { path: dir });
    }
    Ok(seeds)
}

/// Adds to `seeds` the files under `relative` (under `root`) whose
/// extension is one of `extensions`, in path order.
fn read_tree(
    root: &Path,
    relative: &Path,
    extensions: &[String],
    seeds: &mut Vec<(PathBuf, Vec<u8>)>,
) -> Result<(), FuzzError> {
    let dir = root.join(relative);
    if paths::walk(root, relative)? == paths::Walked::Missing {
        return Err(FuzzError::Read {
            path: dir,
            source: io::ErrorKind::NotFound.into(),
        });
    }
    for path in entries(&dir, true)? {
        let wanted = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extensions.iter().any(|wanted| wanted == extension));
        if wanted {
            let bytes = read_seed(&path)?;
            seeds.push((path, bytes));
        }
    }
    Ok(())
}

/// The regular files in `dir`, in name order, and with `recurse` those in
/// its subdirectories, each where its name sorts. Hidden files (a name
/// that starts with `.`) are skipped. A symbolic link is refused
/// ([`FuzzError::Link`]), and so is anything else that is not a regular
/// file or a directory to recurse into.
fn entries(dir: &Path, recurse: bool) -> Result<Vec<PathBuf>, FuzzError> {
    let read_error = |source| FuzzError::Read {
        path: dir.to_path_buf(),
        source,
    };
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).map_err(read_error)? {
        let entry = entry.map_err(read_error)?;
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let file_type = entry.file_type().map_err(read_error)?;
        if file_type.is_symlink() {
            return Err(FuzzError::Link { path: entry.path() });
        }
        if !(file_type.is_file() || recurse && file_type.is_dir()) {
            return Err(FuzzError::NotAFile { path: entry.path() });
        }
        found.push((entry.path(), file_type.is_dir()));
    }
    found.sort();
    let mut files = Vec::with_capacity(found.len());
    for (path, is_dir) in found {
        if is_dir {
            files.extend(entries(&path, true)?);
        } else {
            files.push(path);
        }
    }
    Ok(files)
}

/// Reads one seed or crash, at most [`MAX_SEED_BYTES`].
fn read_seed(path: &Path) -> Result<Vec<u8>, FuzzError> {
    let read_error = |source| FuzzError::Read {
        path: path.to_path_buf(),
        source,
    };
    let mut bytes = Vec::new();
    fs::File::open(path)
        .and_then(|file| file.take(MAX_SEED_BYTES + 1).read_to_end(&mut bytes))
        .map_err(read_error)?;
    if bytes.len() as u64 > MAX_SEED_BYTES {
        return Err(FuzzError::TooLarge {
            path: path.to_path_buf(),
        });
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests;
