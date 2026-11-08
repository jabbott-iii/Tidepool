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

//! Tests of the fuzzer, through [`Target`] and on planted bugs.
//!
//! Ported in part from the tests of Nadir's `xtask/src/fuzz/engine.rs`.
//! Adapted for Tidepool: the targets have seed and crash directories on
//! disk, and the planted bugs are reached by mutation, since only mutated
//! inputs are saved.
//!
//! Targets are plain functions, so each test that records what its target
//! saw has a `static` of its own. Every target but the memory test's has
//! no memory cap: the tests run in one process, and the memory test's
//! allocation raises the peak for all of them. The reserved-memory check
//! needs a process of its own, and is tested in `tests/fuzz_memory.rs`.

use std::io::Write as _;
use std::panic;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::thread;
use std::time::Instant;

use super::*;
use crate::hash::fnv1a64;

/// A directory of its own for one test, under the repository's `target/`,
/// removed when dropped.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(test: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/core-unit-tmp")
            .join(format!(
                "{}-{}-{test}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        // It may be left over from an earlier run that was killed.
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    /// Writes `bytes` to `path` under the root, making its directories.
    fn write(&self, path: &str, bytes: &[u8]) {
        let path = self.root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    /// A target `name` with these seeds in its corpus, each named after
    /// its place (`00`, `01`…), and no memory cap.
    fn target(&self, name: &str, run: fn(&[u8]), seeds: &[&[u8]]) -> Target {
        self.target_in("", name, run, seeds)
    }

    /// [`Scratch::target`] for the repository at `dir` under the root.
    fn target_in(&self, dir: &str, name: &str, run: fn(&[u8]), seeds: &[&[u8]]) -> Target {
        let root = self.root.join(dir);
        for (index, seed) in seeds.iter().enumerate() {
            let path = root.join(format!("fuzz/corpus/{name}/{index:02}"));
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, seed).unwrap();
        }
        Target::new(root, name, run).memory_cap(u64::MAX)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// A configuration that stops after `max_inputs` mutated inputs, long
/// before any of its time limits.
fn inputs(max_inputs: u64) -> Config {
    Config {
        seed: 1,
        duration: Duration::from_secs(3600),
        max_inputs,
        hang_limit: Duration::from_secs(3600),
        ..Config::default()
    }
}

/// The names of the files in `dir`, hidden ones too, sorted; none if it
/// does not exist.
fn files_in(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The file a finding names.
fn path_of(finding: &Finding) -> &Path {
    finding.path().expect("the finding names a file")
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Makes `at` a symbolic link to `target`. Returns why, when this OS or
/// account cannot make links; the test then says it skipped.
fn link(target: &Path, at: &Path) -> Result<(), String> {
    fs::create_dir_all(at.parent().unwrap()).unwrap();
    // `fs::soft_link` is deprecated in favour of one function per OS, but
    // those would need a platform `cfg`, which `core` must not have
    // (`AGENTS.md` rule 5); this one works wherever links do. On Windows it
    // makes a file link even to a directory, which is still a link.
    #[allow(deprecated)]
    fs::soft_link(target, at).map_err(|error| error.to_string())
}

/// Says on standard error, as the gate counts it, that `test` skipped.
fn skipped(test: &str, reason: &str) {
    let _ = writeln!(std::io::stderr(), "SKIPPED: {test}: {reason}");
}

fn accept(_: &[u8]) {}

#[test]
fn a_plain_run_is_the_unchanged_inputs_only() {
    let scratch = Scratch::new("plain");
    let target = scratch.target("clean", accept, &[b"[1, 2]", b"{}"]);
    let outcome = target.run_with(&Config::default()).unwrap();
    assert_eq!((outcome.seeds, outcome.crashes, outcome.inputs), (2, 0, 2));
    assert!(outcome.is_clean() && !outcome.hung());
    assert_eq!(outcome.stopped, Stop::Unchanged);
    outcome.assert_clean();
    // Nothing to save, so the crash directory is not even made.
    assert!(!target.crashes().exists());

    // Asked for, mutated inputs follow.
    let outcome = target.run_with(&inputs(500)).unwrap();
    assert_eq!(outcome.inputs, 502);
    assert_eq!(outcome.stopped, Stop::MaxInputs);
    assert!(outcome.is_clean());
}

/// The reserved check: a plain `cargo test` does not run it, and says so.
#[test]
fn a_plain_run_says_reserved_memory_was_not_checked() {
    let scratch = Scratch::new("not-reserved");
    let outcome = scratch
        .target("plain", accept, &[b"x"])
        .run_with(&Config::default())
        .unwrap();
    let Memory::Skipped(skip) = &outcome.reserved else {
        panic!("{:?}", outcome.reserved);
    };
    assert_eq!(skip.code, "not-asked");
    assert!(
        skip.reason.starts_with(
            "reserved memory is checked only when TIDEPOOL_FUZZ_SECS is set, even to 0"
        ),
        "{}",
        skip.reason
    );
    let report = outcome.to_string();
    assert!(
        report.contains("\n  reserved memory: not checked: reserved memory is checked only"),
        "{report}"
    );
    let line = outcome.summary().to_string();
    assert!(
        line.contains(" reserved=skipped reserved_reason=not-asked"),
        "{line}"
    );
}

#[test]
fn crashes_then_seeds_run_first_and_unchanged() {
    static SEEN: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
    fn record(input: &[u8]) {
        lock(&SEEN).push(input.to_vec());
    }
    let scratch = Scratch::new("order");
    let target = scratch.target("order", record, &[b"first", b"second", b"third"]);
    scratch.write("fuzz/crashes/order/panic-b", b"crash b");
    scratch.write("fuzz/crashes/order/hang-a", b"crash a");
    // Hidden files are not inputs, a replay's own file among them.
    scratch.write("fuzz/crashes/order/.gitkeep", b"");
    scratch.write("fuzz/crashes/order/.last-input", b"not a crash");
    scratch.write("fuzz/corpus/order/.hidden", b"hidden");
    // Seeds read in place: only the extensions asked for, in path order,
    // subdirectories included.
    scratch.write("data/b/two.json", b"extra two");
    scratch.write("data/a.json", b"extra one");
    scratch.write("data/skip.txt", b"not a seed");
    let target = target.seeds_in("data", &["json"]);

    let outcome = target.run_with(&inputs(50)).unwrap();
    assert_eq!((outcome.crashes, outcome.seeds), (2, 5));
    assert_eq!(outcome.inputs, 57);
    let seen = lock(&SEEN);
    let unchanged: Vec<&[u8]> = vec![
        b"crash a",
        b"crash b",
        b"first",
        b"second",
        b"third",
        b"extra one",
        b"extra two",
    ];
    assert_eq!(seen[..7], unchanged[..]);
    // After them, inputs are mutated seeds.
    let changed = seen[7..]
        .iter()
        .filter(|input| !unchanged.contains(&input.as_slice()))
        .count();
    assert!(changed > 40, "{changed}");
}

#[test]
fn the_same_seed_gives_the_same_inputs() {
    static FIRST: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
    static SECOND: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
    static WITH_A_CRASH: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
    static OTHER: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
    fn first(input: &[u8]) {
        lock(&FIRST).push(input.to_vec());
    }
    fn second(input: &[u8]) {
        lock(&SECOND).push(input.to_vec());
    }
    fn with_a_crash(input: &[u8]) {
        lock(&WITH_A_CRASH).push(input.to_vec());
    }
    fn other(input: &[u8]) {
        lock(&OTHER).push(input.to_vec());
    }
    let seeds: &[&[u8]] = &[b"{\"a\": [1, 2, 3]}", b"[true, null]"];
    let tokens: &'static [&'static [u8]] = &[b"\\u0000", b"1e999"];
    let scratch = Scratch::new("same-seed");
    // The same name in two roots: the name picks the random streams.
    let run = |root: &str, run: fn(&[u8]), config: &Config| {
        scratch
            .target_in(root, "same", run, seeds)
            .dictionary(tokens)
            .run_with(config)
            .unwrap()
    };
    run("a", first, &inputs(300));
    run("b", second, &inputs(300));
    // A saved crash runs first, but does not change what follows.
    scratch.write("c/fuzz/crashes/same/panic-0", b"a crash");
    run("c", with_a_crash, &inputs(300));
    run(
        "d",
        other,
        &Config {
            seed: 2,
            ..inputs(300)
        },
    );
    let first = lock(&FIRST);
    assert_eq!(first.len(), 302);
    assert_eq!(*first, *lock(&SECOND));
    let with_a_crash = lock(&WITH_A_CRASH);
    assert_eq!(with_a_crash[0], b"a crash");
    assert_eq!(with_a_crash[1..], first[..]);
    assert_ne!(*first, *lock(&OTHER));
    // The dictionary was used.
    assert!(
        first
            .iter()
            .any(|input| input.windows(5).any(|w| w == b"1e999"))
    );
}

#[test]
fn input_n_depends_on_n_alone() {
    // Two runs that stop at different counts agree on what they share,
    // and so would a run cut short by the clock.
    static SHORT: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
    static LONG: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
    fn short(input: &[u8]) {
        lock(&SHORT).push(input.to_vec());
    }
    fn long(input: &[u8]) {
        lock(&LONG).push(input.to_vec());
    }
    let scratch = Scratch::new("prefix");
    scratch
        .target_in("a", "prefix", short, &[b"abc"])
        .run_with(&inputs(40))
        .unwrap();
    scratch
        .target_in("b", "prefix", long, &[b"abc"])
        .run_with(&inputs(400))
        .unwrap();
    let (short, long) = (lock(&SHORT), lock(&LONG));
    assert_eq!(short[..], long[..41]);
}

#[test]
fn mutated_inputs_respect_the_length_limit() {
    static LONGEST: Mutex<usize> = Mutex::new(0);
    fn measure(input: &[u8]) {
        let mut longest = lock(&LONGEST);
        *longest = (*longest).max(input.len());
    }
    let scratch = Scratch::new("max-len");
    let target = scratch.target("short", measure, &[b"0123456789"]);
    let config = Config {
        max_len: 24,
        ..inputs(2000)
    };
    target.run_with(&config).unwrap();
    assert_eq!(*lock(&LONGEST), 24);
}

/// Panics on any input that holds the token `PLANT`, which no seed holds
/// and the dictionary offers.
fn planted(input: &[u8]) {
    let found = input.windows(5).position(|window| window == b"PLANT");
    if let Some(at) = found {
        panic!("planted at {at}");
    }
}

const PLANT: &[&[u8]] = &[b"PLANT", b"]"];

/// Acceptance (M1.6): a planted panic is found.
#[test]
fn a_planted_panic_reachable_by_mutation_is_found_saved_and_deduplicated() {
    let scratch = Scratch::new("planted-panic");
    let target = scratch
        .target("planted", planted, &[b"[1, 2, 3]", b"{\"a\": null}"])
        .dictionary(PLANT);
    let outcome = target.run_with(&inputs(3000)).unwrap();
    // The run goes on after a panic.
    assert_eq!(outcome.inputs, 3002);
    assert!(!outcome.hung());
    assert_eq!(outcome.findings.len(), 1, "{:?}", outcome.findings);
    assert_eq!(outcome.saved, 1);

    let finding = &outcome.findings[0];
    assert_eq!(finding.kind, Kind::Panic);
    assert!(
        finding.message.starts_with("planted at "),
        "{}",
        finding.message
    );
    assert!(!finding.site_in_std());
    // `file:line:column` of the `panic!` above.
    let site = finding.site.as_deref().unwrap();
    let parts: Vec<&str> = site.rsplitn(3, ':').collect();
    assert_eq!(parts.len(), 3, "{site}");
    assert!(parts[2].ends_with("tests.rs"), "{site}");
    assert!(
        parts[..2].iter().all(|part| part.parse::<u32>().is_ok()),
        "{site}"
    );
    // Many inputs panicked there, and they are one finding.
    assert!(finding.hits > 10, "{}", finding.hits);

    // Exactly one file, which holds an input that panics, written whole:
    // no hidden file is left beside it.
    let Found::File { path, len, number } = &finding.input else {
        panic!("{:?}", finding.input);
    };
    let saved = fs::read(path).unwrap();
    assert_eq!(saved.len(), *len);
    assert!(saved.windows(5).any(|window| window == b"PLANT"));
    let number = number.expect("a mutated input");
    assert!(number < 3000);
    let name = format!("panic-{:016x}", fnv1a64(&saved));
    assert_eq!(*path, target.crashes().join(&name));
    assert_eq!(files_in(&target.crashes()), std::slice::from_ref(&name));

    // The report names the file, the kind and the place, and ends with
    // the command that reproduces it.
    let report = outcome.to_string();
    assert!(
        report.starts_with(
            "fuzz target `planted`: FAILED: 1 finding in 3002 inputs (seed 0x0000000000000001, 2 \
             seeds, 0 saved crashes, "
        ),
        "{report}"
    );
    assert!(
        report.contains(&format!("\n  panic at {site}: planted at ")),
        "{report}"
    );
    assert!(
        report.contains(&format!(
            "\n    input: fuzz/crashes/planted/{name} ({len} bytes), mutated input {number}, the \
             first of {} inputs that did this\n",
            finding.hits
        )),
        "{report}"
    );
    assert!(
        report.contains("\n  stopped: it ran as many inputs as it was allowed\n"),
        "{report}"
    );
    assert!(
        report.ends_with(
            " --test fuzz -- --exact \
             testing::fuzz::tests::a_planted_panic_reachable_by_mutation_is_found_saved_and_deduplicated"
        ),
        "{report}"
    );
    let failed = panic::catch_unwind(|| outcome.assert_clean()).unwrap_err();
    assert_eq!(crate::testing::panic_message(&*failed), report);

    // The next plain run replays the crash first, unchanged: it fails at
    // once, naming the saved file, and saves nothing new.
    let again = target.run_with(&Config::default()).unwrap();
    assert_eq!((again.crashes, again.inputs, again.saved), (1, 3, 0));
    assert_eq!(again.findings.len(), 1);
    assert_eq!(path_of(&again.findings[0]), path);
    assert_eq!(again.findings[0].site, finding.site);
    assert_eq!(files_in(&target.crashes()), [name]);

    // Once the bug is fixed, the saved crash keeps passing.
    let fixed = Target::new(&scratch.root, "planted", accept).memory_cap(u64::MAX);
    let outcome = fixed.run_with(&Config::default()).unwrap();
    assert_eq!(outcome.crashes, 1);
    outcome.assert_clean();
}

#[test]
fn an_unchanged_input_that_fails_is_named_where_it_is() {
    fn reject_empty_arrays(input: &[u8]) {
        assert!(input != b"[]", "an empty array");
    }
    let scratch = Scratch::new("panicking-seed");
    let target = scratch.target("seeds", reject_empty_arrays, &[b"[1]", b"[]", b"[2]"]);
    let outcome = target.run_with(&Config::default()).unwrap();
    assert_eq!((outcome.inputs, outcome.saved), (3, 0));
    assert_eq!(outcome.findings.len(), 1);
    assert_eq!(outcome.findings[0].hits, 1);
    assert_eq!(
        outcome.findings[0].input,
        Found::File {
            path: target.corpus().join("01"),
            len: 2,
            number: None
        }
    );
    assert!(!target.crashes().exists(), "a seed is not saved again");
    assert!(
        outcome
            .to_string()
            .contains("\n    input: fuzz/corpus/seeds/01 (2 bytes)\n")
    );
    assert!(outcome.summary().to_string().contains(" saved=0 "));
}

#[test]
fn panics_from_different_places_are_different_findings() {
    fn two_ways_to_fail(input: &[u8]) {
        if input.len() < 4 {
            panic!("too short: {}", input.len());
        }
        if input.len() > 4 {
            panic!("too long: {}", input.len());
        }
    }
    let scratch = Scratch::new("two-sites");
    let target = scratch.target("two", two_ways_to_fail, &[b"abcd"]);
    let outcome = target.run_with(&inputs(600)).unwrap();
    assert_eq!(outcome.findings.len(), 2, "{:?}", outcome.findings);
    let mut messages: Vec<&str> = outcome
        .findings
        .iter()
        .map(|finding| finding.message.split(':').next().unwrap())
        .collect();
    messages.sort_unstable();
    assert_eq!(messages, ["too long", "too short"]);
    assert_ne!(outcome.findings[0].site, outcome.findings[1].site);
    assert_ne!(outcome.findings[0].input, outcome.findings[1].input);
    // The messages differ from input to input, but the place does not.
    let hits: u64 = outcome.findings.iter().map(|finding| finding.hits).sum();
    assert!(hits > 100, "{hits}");
    assert_eq!(files_in(&target.crashes()).len(), 2);
    assert_eq!(outcome.saved, 2);
}

#[test]
fn a_panic_with_an_unusual_payload_is_still_a_finding() {
    fn odd(_: &[u8]) {
        std::panic::panic_any(42_u32);
    }
    let scratch = Scratch::new("odd-payload");
    let outcome = scratch
        .target("odd", odd, &[b"x"])
        .run_with(&inputs(4))
        .unwrap();
    assert_eq!(outcome.findings.len(), 1);
    assert_eq!(
        outcome.findings[0].message,
        "<a panic payload that is not a string>"
    );
    assert_eq!(outcome.findings[0].hits, 5);
}

#[test]
fn a_panic_in_the_standard_library_says_others_may_be_merged_into_it() {
    /// `Vec::with_capacity` of what an input claims: "capacity overflow",
    /// raised in `alloc`, not here.
    fn claims(input: &[u8]) {
        if input.windows(3).any(|window| window == b"CAP") {
            let claimed: Vec<u64> = Vec::with_capacity(usize::MAX / 4);
            std::hint::black_box(&claimed);
        }
    }
    let scratch = Scratch::new("std-site");
    let outcome = scratch
        .target("std", claims, &[b"a claim"])
        .dictionary(&[b"CAP"])
        .run_with(&inputs(200))
        .unwrap();
    assert_eq!(outcome.findings.len(), 1, "{:?}", outcome.findings);
    let finding = &outcome.findings[0];
    assert!(finding.site_in_std(), "{:?}", finding.site);
    assert_eq!(finding.message, "capacity overflow");
    let report = outcome.to_string();
    assert!(
        report.contains(
            "\n    that place is in the standard library: other places in the target that \
             panic there are merged into this finding\n"
        ),
        "{report}"
    );
    assert!(in_std("/rustc/0123/library/alloc/src/raw_vec/mod.rs:28:5"));
    assert!(!in_std("crates/codec/src/json/parse.rs:252:13"));
}

#[test]
fn a_panic_after_one_the_target_caught_is_named_and_mentions_it() {
    static FIRST_LINE: AtomicU32 = AtomicU32::new(0);
    static SECOND_LINE: AtomicU32 = AtomicU32::new(0);
    /// Panics, catches it, and panics again: the second is the one that
    /// escapes, and the finding is about it.
    fn twice(_: &[u8]) {
        FIRST_LINE.store(line!() + 1, Ordering::Relaxed);
        let _ = panic::catch_unwind(|| panic!("caught inside"));
        SECOND_LINE.store(line!() + 1, Ordering::Relaxed);
        panic!("the one that escaped");
    }
    let scratch = Scratch::new("twice");
    let outcome = scratch
        .target("twice", twice, &[b"x"])
        .run_with(&Config::default())
        .unwrap();
    let finding = &outcome.findings[0];
    assert_eq!(finding.message, "the one that escaped");
    let site = finding.site.as_deref().unwrap();
    let (first, second) = (
        FIRST_LINE.load(Ordering::Relaxed),
        SECOND_LINE.load(Ordering::Relaxed),
    );
    assert!(
        site.contains(&format!("tests.rs:{second}:")),
        "{site}, not line {second}"
    );
    let earlier = finding.earlier.as_deref().unwrap();
    assert!(
        earlier.contains(&format!("tests.rs:{first}:")) && earlier.ends_with(": caught inside"),
        "{earlier}"
    );
    let report = outcome.to_string();
    assert!(
        report.contains(&format!(
            "\n    another panic during the same input, which the target caught, was at \
             {earlier}\n"
        )),
        "{report}"
    );
    // Two escaping bugs behind one caught panic are two findings.
    fn caught_then_two_ways(input: &[u8]) {
        let _ = panic::catch_unwind(|| panic!("caught inside"));
        if input.len() > 3 {
            panic!("bug A");
        }
        panic!("bug B");
    }
    let outcome = scratch
        .target("two", caught_then_two_ways, &[b"ab", b"abcdef"])
        .run_with(&Config::default())
        .unwrap();
    assert_eq!(outcome.findings.len(), 2, "{outcome}");
}

#[test]
fn a_panic_a_drop_caught_while_the_real_one_unwound_is_not_the_place() {
    static ESCAPING_LINE: AtomicU32 = AtomicU32::new(0);
    static DROP_LINE: AtomicU32 = AtomicU32::new(0);
    /// Raises and catches a panic of its own when dropped: during the
    /// target's panic, that one is the latest, but not the one that
    /// escapes.
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            DROP_LINE.store(line!() + 1, Ordering::Relaxed);
            let _ = panic::catch_unwind(|| panic!("caught in a drop"));
        }
    }
    fn guarded(input: &[u8]) {
        let _guard = Guard;
        if input.len() > 3 {
            ESCAPING_LINE.store(line!() + 1, Ordering::Relaxed);
            panic!("bug one");
        }
    }
    let scratch = Scratch::new("drop-catch");
    let outcome = scratch
        .target("guarded", guarded, &[b"ab", b"abcdef"])
        .run_with(&Config::default())
        .unwrap();
    assert_eq!(outcome.findings.len(), 1, "{outcome}");
    let finding = &outcome.findings[0];
    assert_eq!(finding.message, "bug one");
    let site = finding.site.as_deref().unwrap();
    let (escaping, dropped) = (
        ESCAPING_LINE.load(Ordering::Relaxed),
        DROP_LINE.load(Ordering::Relaxed),
    );
    assert!(
        site.contains(&format!("tests.rs:{escaping}:")),
        "{site}, not line {escaping}"
    );
    let caught = finding.earlier.as_deref().unwrap();
    assert!(
        caught.contains(&format!("tests.rs:{dropped}:")) && caught.ends_with(": caught in a drop"),
        "{caught}"
    );
}

#[test]
fn a_planted_hang_is_found_and_its_input_saved() {
    static RELEASE: AtomicBool = AtomicBool::new(false);
    static AFTER: AtomicU32 = AtomicU32::new(0);
    /// Never returns on an input that holds `LOOP`, which mutation adds,
    /// until let go.
    fn stuck(input: &[u8]) {
        let looping = input.windows(4).any(|window| window == b"LOOP");
        while looping && !RELEASE.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(1));
        }
        if RELEASE.load(Ordering::Relaxed) {
            AFTER.fetch_add(1, Ordering::Relaxed);
        }
    }
    // A thread stuck in a target turns the memory checks of later runs
    // off, which the memory tests must not meet.
    let _serial = memory::serial();
    let scratch = Scratch::new("hang");
    let target = scratch
        .target("hang", stuck, &[b"fine", b"also fine"])
        .dictionary(&[b"LOOP"]);
    let config = Config {
        hang_limit: Duration::from_millis(40),
        ..inputs(1_000_000)
    };
    let start = Instant::now();
    let outcome = target.run_with(&config).unwrap();
    // Let the fuzzing thread go, whatever the assertions below say.
    RELEASE.store(true, Ordering::Relaxed);

    assert!(outcome.hung());
    assert_eq!(outcome.stopped, Stop::Hang);
    assert!(start.elapsed() < Duration::from_secs(30));
    assert!(outcome.inputs >= 2, "the seeds finished");
    assert_eq!(outcome.findings.len(), 1, "{:?}", outcome.findings);
    let finding = &outcome.findings[0];
    assert_eq!(finding.kind, Kind::Hang);
    assert_eq!(finding.site, None);
    assert_eq!(finding.message, "did not return within 0.04 s");
    let saved = fs::read(path_of(finding)).unwrap();
    assert!(saved.windows(4).any(|window| window == b"LOOP"));
    let name = format!("hang-{:016x}", fnv1a64(&saved));
    assert_eq!(files_in(&target.crashes()), [name]);
    let report = outcome.to_string();
    assert!(
        report.contains("\n  hang: did not return within 0.04 s\n"),
        "{report}"
    );
    assert!(
        report.contains("\n  stopped: an input did not return, and its thread is still busy\n"),
        "{report}"
    );

    // Once let go, the thread stops instead of fuzzing on behind the
    // caller's back: it finishes the input it was on, and no other.
    assert!(engine::tests_support::no_hung_thread());
    thread::sleep(Duration::from_millis(100));
    assert!(AFTER.load(Ordering::Relaxed) <= 1);
}

#[test]
fn a_hung_input_that_returns_later_records_nothing_and_later_runs_check_no_memory() {
    static RELEASE: AtomicBool = AtomicBool::new(false);
    /// Waits on an input that holds `LOOP` until let go, then allocates
    /// and panics: what the run must no longer record.
    fn late(input: &[u8]) {
        if input.windows(4).any(|window| window == b"LOOP") {
            while !RELEASE.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(1));
            }
            let big = vec![1_u8; 32 << 20];
            std::hint::black_box(&big);
            panic!("too late");
        }
    }
    let _serial = memory::serial();
    let scratch = Scratch::new("late");
    let target = scratch
        .target("late", late, &[b"fine"])
        .dictionary(&[b"LOOP"])
        .memory_cap(16 << 20);
    let config = Config {
        hang_limit: Duration::from_millis(40),
        ..inputs(1_000_000)
    };
    let outcome = target.run_with(&config).unwrap();
    assert_eq!(outcome.stopped, Stop::Hang);
    let hang = files_in(&target.crashes());
    assert_eq!(hang.len(), 1, "{hang:?}");

    // While the thread is stuck, a later run checks no memory, and says
    // why.
    let other = scratch
        .target("other", accept, &[b"x"])
        .run_with(&Config {
            check_reserved: true,
            ..Config::default()
        })
        .unwrap();
    for memory in [&other.memory, &other.reserved] {
        assert!(
            matches!(memory, Memory::Skipped(skip) if skip.code == "hung-thread"),
            "{memory:?}"
        );
    }

    // Let go, the input allocates and panics, and nothing is recorded or
    // saved for it.
    RELEASE.store(true, Ordering::Relaxed);
    assert!(engine::tests_support::no_hung_thread());
    thread::sleep(Duration::from_millis(50));
    assert_eq!(files_in(&target.crashes()), hang);
    // And the memory checks are back.
    let after = scratch
        .target("after", accept, &[b"x"])
        .run_with(&Config::default())
        .unwrap();
    if fs::File::open(memory::STATUS).is_ok() {
        assert!(matches!(after.memory, Memory::Measured { .. }), "{after}");
    }
}

#[test]
fn an_input_that_returns_after_the_limit_is_one_hang() {
    // Either part may leave its thread busy for a while after its run,
    // which the memory tests must not meet.
    let _serial = memory::serial();
    // With no time allowed, every input is too slow: the first one, a seed,
    // returns before the watchdog has looked, and the thread that ran it
    // records it.
    let scratch = Scratch::new("slow-returned");
    let config = Config {
        hang_limit: Duration::ZERO,
        ..inputs(100)
    };
    let target = scratch.target("slow", accept, &[b"a", b"b"]);
    let outcome = target.run_with(&config).unwrap();
    assert_eq!(outcome.findings.len(), 1, "{:?}", outcome.findings);
    let finding = &outcome.findings[0];
    assert_eq!(finding.kind, Kind::Hang);
    assert!(outcome.hung());
    match outcome.stopped {
        Stop::SlowInput => assert!(
            finding.message.starts_with("returned after ")
                && finding.message.ends_with(", past the limit of 0 s"),
            "{}",
            finding.message
        ),
        // The watchdog can still look first on a busy machine: either
        // way, it is one finding.
        Stop::Hang => assert_eq!(finding.message, "did not return within 0 s"),
        other => panic!("{other:?}"),
    }
    assert_eq!(path_of(finding), target.corpus().join("00"));

    // A slow input that the watchdog sees too is still one finding.
    fn slow_on_marker(input: &[u8]) {
        if input == b"slow" {
            thread::sleep(Duration::from_millis(120));
        }
    }
    let config = Config {
        hang_limit: Duration::from_millis(40),
        ..inputs(0)
    };
    let outcome = scratch
        .target("watched", slow_on_marker, &[b"slow", b"fast"])
        .run_with(&config)
        .unwrap();
    assert_eq!(outcome.findings.len(), 1, "{:?}", outcome.findings);
    assert_eq!(outcome.findings[0].kind, Kind::Hang);
    assert!(engine::tests_support::no_hung_thread());
}

#[test]
fn a_slow_input_within_the_limit_is_not_a_hang() {
    fn slow(_: &[u8]) {
        thread::sleep(Duration::from_millis(30));
    }
    let scratch = Scratch::new("slow");
    let config = Config {
        hang_limit: Duration::from_secs(60),
        ..inputs(3)
    };
    let outcome = scratch
        .target("slow", slow, &[b"a"])
        .run_with(&config)
        .unwrap();
    assert_eq!(outcome.inputs, 4);
    assert!(!outcome.hung() && outcome.findings.is_empty());
    assert!(outcome.elapsed >= Duration::from_millis(120));
}

#[test]
fn the_slowest_input_is_reported_with_its_length() {
    fn slow_on_long(input: &[u8]) {
        if input.len() > 5 {
            thread::sleep(Duration::from_millis(40));
        }
    }
    let scratch = Scratch::new("slowest");
    let target = scratch.target("slowest", slow_on_long, &[b"a", b"slowest", b"b"]);
    let outcome = target.run_with(&Config::default()).unwrap();
    let slowest = outcome.slowest.expect("inputs finished");
    assert_eq!(slowest.len, 7);
    assert!(slowest.took >= Duration::from_millis(40), "{slowest:?}");
}

#[test]
fn the_time_limit_ends_the_run() {
    let scratch = Scratch::new("time-limit");
    let config = Config {
        duration: Duration::from_millis(60),
        ..inputs(u64::MAX)
    };
    let outcome = scratch
        .target("time", accept, &[b"a"])
        .run_with(&config)
        .unwrap();
    assert!(outcome.inputs > 1);
    assert_eq!(outcome.stopped, Stop::Time);
    assert!(outcome.elapsed >= Duration::from_millis(60));
    assert!(outcome.elapsed < Duration::from_secs(30));
}

#[test]
fn the_target_runs_on_a_thread_of_its_own() {
    static NAMES: Mutex<Vec<Option<String>>> = Mutex::new(Vec::new());
    fn record(_: &[u8]) {
        lock(&NAMES).push(thread::current().name().map(str::to_string));
    }
    let scratch = Scratch::new("thread");
    scratch
        .target("named", record, &[b"x"])
        .run_with(&inputs(2))
        .unwrap();
    let names = lock(&NAMES);
    assert_eq!(*names, vec![Some("fuzz-named".to_string()); 3]);
    // 2 MiB, Rust's default for a spawned thread.
    assert_eq!(STACK_SIZE, 2 << 20);
}

#[test]
fn targets_of_one_process_never_run_at_once() {
    static INSIDE: AtomicU32 = AtomicU32::new(0);
    static OVERLAPPED: AtomicBool = AtomicBool::new(false);
    fn exclusive(_: &[u8]) {
        if INSIDE.fetch_add(1, Ordering::SeqCst) > 0 {
            OVERLAPPED.store(true, Ordering::SeqCst);
        }
        thread::sleep(Duration::from_micros(200));
        INSIDE.fetch_sub(1, Ordering::SeqCst);
    }
    let scratch = Scratch::new("exclusive");
    let targets: Vec<Target> = (0..4)
        .map(|index| scratch.target(&format!("t{index}"), exclusive, &[b"x"]))
        .collect();
    thread::scope(|scope| {
        for target in &targets {
            scope.spawn(|| target.run_with(&inputs(40)).unwrap());
        }
    });
    assert!(!OVERLAPPED.load(Ordering::SeqCst));
}

/// How much [`over_allocate`] allocates.
static ALLOCATE: AtomicUsize = AtomicUsize::new(0);

/// Allocates and touches [`ALLOCATE`] bytes on the first input that holds
/// `ALLOC`, which mutation adds. Only the first: about half of the mutated
/// inputs hold it, and each would take milliseconds.
fn over_allocate(input: &[u8]) {
    static DONE: AtomicBool = AtomicBool::new(false);
    if input.windows(5).any(|window| window == b"ALLOC") && !DONE.swap(true, Ordering::Relaxed) {
        let big = vec![1_u8; ALLOCATE.load(Ordering::Relaxed)];
        std::hint::black_box(&big);
    }
}

#[test]
fn a_reserved_finding_says_what_its_cap_counted_from() {
    const MIB: u64 = 1024 * 1024;
    // The usual case: the cap counts from what was in use.
    let from_in_use = memory::Exceeded {
        baseline: 100 * MIB,
        in_use: 100 * MIB,
        peak: 400 * MIB,
        cap: 256 * MIB,
    };
    assert_eq!(
        engine::reserved_message(&from_in_use),
        "the reserved peak (VmPeak) rose to 400.0 MiB, 300.0 MiB above the 100.0 MiB of \
         address space in use (VmSize) at the start, past the cap of 256.0 MiB"
    );
    // A peak at the start already more than the cap above what was in use:
    // the finding is a rise past that peak, and says so.
    let from_peak = memory::Exceeded {
        baseline: 144 * MIB,
        in_use: 100 * MIB,
        peak: 401 * MIB,
        cap: 256 * MIB,
    };
    assert_eq!(
        engine::reserved_message(&from_peak),
        "the reserved peak (VmPeak) rose to 401.0 MiB, past the 400.0 MiB it had reached at \
         the start, which was already more than the cap of 256.0 MiB above the 100.0 MiB of \
         address space in use (VmSize) then"
    );
}

#[test]
fn a_planted_over_allocation_is_found_on_linux_and_skipped_elsewhere() {
    let _serial = memory::serial();
    // 48 MiB more than the peak so far, which other tests may have raised
    // (it never comes down): well past the 16 MiB cap, whatever ran
    // before. About 70 MiB in all.
    let peak_so_far = fs::read(memory::STATUS)
        .ok()
        .and_then(|status| memory::fields(&status, true).written)
        .unwrap_or(0);
    ALLOCATE.store(
        usize::try_from(peak_so_far).unwrap() + (48 << 20),
        Ordering::Relaxed,
    );
    let scratch = Scratch::new("memory");
    let target = scratch
        .target("memory", over_allocate, &[b"small", b"also small"])
        .dictionary(&[b"ALLOC"])
        .memory_cap(16 << 20);
    let outcome = target.run_with(&inputs(2000)).unwrap();
    let report = outcome.to_string();
    if fs::File::open(memory::STATUS).is_err() {
        // Not Linux: nothing is found, and the report says why.
        assert!(outcome.is_clean(), "{report}");
        assert!(matches!(&outcome.memory, Memory::Skipped(skip) if skip.code == "unreadable"));
        assert!(
            report.contains("\n  written memory: not checked: /proc/self/status cannot be read ("),
            "{report}"
        );
        assert!(
            outcome
                .summary()
                .to_string()
                .contains(" memory=skipped memory_reason=unreadable ")
        );
        return;
    }
    assert_eq!(outcome.findings.len(), 1, "{report}");
    let finding = &outcome.findings[0];
    assert_eq!(finding.kind, Kind::Memory);
    assert!(
        finding
            .message
            .starts_with("the written peak (VmHWM) rose to ")
            && finding.message.ends_with(" past the cap of 16.0 MiB"),
        "{}",
        finding.message
    );
    // The input named is the one that allocated.
    let saved = fs::read(path_of(finding)).unwrap();
    assert!(
        saved.windows(5).any(|window| window == b"ALLOC"),
        "{report}"
    );
    let name = format!("memory-{:016x}", fnv1a64(&saved));
    assert_eq!(files_in(&target.crashes()), [name]);
    assert!(
        report.contains("\n  over-allocation: the written peak (VmHWM) rose to "),
        "{report}"
    );
    let Memory::Measured {
        baseline,
        peak,
        cap,
        stopped,
    } = outcome.memory
    else {
        panic!("{report}");
    };
    assert!(peak - baseline > cap, "{report}");
    assert!(peak >= ALLOCATE.load(Ordering::Relaxed) as u64, "{report}");
    assert_eq!(cap, 16 << 20);
    assert_eq!(stopped, None);
    // The peak never comes down, so it is found once.
    assert_eq!(finding.hits, 1);
}

#[test]
fn replay_mode_saves_each_input_before_it_runs() {
    static CROSS: Mutex<Option<PathBuf>> = Mutex::new(None);
    static MISMATCHES: AtomicU32 = AtomicU32::new(0);
    static CHECKED: AtomicU32 = AtomicU32::new(0);
    /// Reads the replay's file: what a process that died on this input
    /// would leave behind.
    fn on_disk(input: &[u8]) {
        let path = lock(&CROSS).clone().expect("set before the run");
        let saved = fs::read(path).unwrap_or_default();
        if saved != input {
            MISMATCHES.fetch_add(1, Ordering::Relaxed);
        }
        CHECKED.fetch_add(1, Ordering::Relaxed);
    }
    let scratch = Scratch::new("replay");
    let target = scratch.target("replay", on_disk, &[b"a longer first seed", b"b"]);
    scratch.write("fuzz/crashes/replay/panic-0", b"an old crash");
    let last = target.crashes().join(LAST_INPUT);
    assert_eq!(LAST_INPUT, ".last-input");
    *lock(&CROSS) = Some(last.clone());
    let config = Config {
        replay: true,
        ..inputs(300)
    };
    let outcome = target.run_with(&config).unwrap();
    assert_eq!(outcome.inputs, 303);
    assert_eq!(CHECKED.load(Ordering::Relaxed), 303);
    assert_eq!(MISMATCHES.load(Ordering::Relaxed), 0);
    // The run ended without dying, so its own file is gone, and nothing
    // else is.
    assert_eq!(files_in(&target.crashes()), ["panic-0"]);

    // A replay of a target with no crashes leaves no directory behind,
    // not even `fuzz/crashes/`.
    let clean = scratch.target_in("other", "clean", accept, &[b"x"]);
    clean.run_with(&config).unwrap();
    assert!(!scratch.root.join("other/fuzz/crashes").exists());
}

#[test]
fn a_replay_runs_what_an_earlier_replay_left_first_and_keeps_nothing() {
    static SEEN: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
    fn record(input: &[u8]) {
        lock(&SEEN).push(input.to_vec());
        assert!(input != b"it died here", "still a bug");
    }
    let scratch = Scratch::new("leftover");
    let target = scratch.target("left", record, &[b"seed"]);
    scratch.write("fuzz/crashes/left/.last-input", b"it died here");

    // A plain run does not read the hidden file.
    let outcome = target.run_with(&Config::default()).unwrap();
    assert_eq!(outcome.crashes, 0);
    assert!(outcome.is_clean());
    lock(&SEEN).clear();

    let config = Config {
        replay: true,
        ..inputs(5)
    };
    // A replay that cannot start, here for a saved crash too large to
    // read, leaves it as it was.
    let last = target.crashes().join(".last-input");
    let too_large = target.crashes().join("zbig");
    fs::write(&too_large, vec![b'x'; MAX_SEED_BYTES as usize + 1]).unwrap();
    let error = target.run_with(&config).unwrap_err();
    assert!(matches!(error, FuzzError::TooLarge { .. }), "{error:?}");
    assert_eq!(fs::read(&last).unwrap(), b"it died here");
    assert!(lock(&SEEN).is_empty());
    fs::remove_file(&too_large).unwrap();

    // A replay runs it first. It is not a saved crash, so a finding on it
    // saves it as any mutated input's is saved; it is not kept as
    // `died-<hash>`, since nothing here knows a process died on it. The
    // replay's own file replaced it, and goes at the end.
    let outcome = target.run_with(&config).unwrap();
    assert_eq!(outcome.crashes, 0);
    assert_eq!(lock(&SEEN)[0], b"it died here");
    assert_eq!(outcome.findings.len(), 1);
    let saved = format!("panic-{:016x}", fnv1a64(b"it died here"));
    assert_eq!(path_of(&outcome.findings[0]), target.crashes().join(&saved));
    assert_eq!(files_in(&target.crashes()), [saved]);
}

#[test]
fn keep_died_renames_the_dead_replay_s_input_whatever_is_there() {
    let scratch = Scratch::new("died");
    let target = scratch.target("died", accept, &[b"seed"]);
    let kept = target
        .crashes()
        .join(format!("died-{:016x}", fnv1a64(b"it died here")));
    assert_eq!(keep_died(&scratch.root, "died").unwrap(), None);
    assert_eq!(keep_died(&scratch.root, "missing").unwrap(), None);

    scratch.write("fuzz/crashes/died/.last-input", b"it died here");
    assert_eq!(
        keep_died(&scratch.root, "died").unwrap(),
        Some(Kept::New(kept.clone()))
    );
    assert_eq!(fs::read(&kept).unwrap(), b"it died here");
    // From then on every run has it, as a regression test.
    let outcome = target.run_with(&Config::default()).unwrap();
    assert_eq!(outcome.crashes, 1);

    // The same input, kept again over a file of that name that was edited:
    // the file holds the input's bytes again.
    fs::write(&kept, b"edited by hand").unwrap();
    scratch.write("fuzz/crashes/died/.last-input", b"it died here");
    let again = keep_died(&scratch.root, "died").unwrap();
    assert_eq!(again, Some(Kept::Existing(kept.clone())));
    assert_eq!(again.unwrap().path(), kept.as_path());
    assert_eq!(fs::read(&kept).unwrap(), b"it died here");
    let name_of = |path: &Path| path.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(files_in(&target.crashes()), [name_of(&kept)]);

    // A replay that died on a saved crash of another kind is not given a
    // second copy of it: the crash stays, and the dead replay's file goes.
    for kind in [Kind::Panic, Kind::Hang, Kind::Memory, Kind::Reserved] {
        let input = format!("a saved {kind} crash");
        let saved = target.crashes().join(format!(
            "{}-{:016x}",
            kind.file_prefix(),
            fnv1a64(input.as_bytes())
        ));
        fs::write(&saved, &input).unwrap();
        scratch.write("fuzz/crashes/died/.last-input", input.as_bytes());
        assert_eq!(
            keep_died(&scratch.root, "died").unwrap(),
            Some(Kept::Existing(saved.clone())),
            "{kind}"
        );
        assert_eq!(fs::read(&saved).unwrap(), input.as_bytes());
        assert_eq!(
            files_in(&target.crashes()),
            [name_of(&kept), name_of(&saved)]
        );
        fs::remove_file(&saved).unwrap();
    }
    // One of that name that holds other bytes, edited by hand, is not the
    // input: it is kept as `died-<hash>`, and both stay.
    let input = b"a panic, edited";
    let edited = target
        .crashes()
        .join(format!("panic-{:016x}", fnv1a64(input)));
    fs::write(&edited, b"something else").unwrap();
    scratch.write("fuzz/crashes/died/.last-input", input);
    let died = target
        .crashes()
        .join(format!("died-{:016x}", fnv1a64(input)));
    assert_eq!(
        keep_died(&scratch.root, "died").unwrap(),
        Some(Kept::New(died.clone()))
    );
    assert_eq!(fs::read(&died).unwrap(), input);
    assert_eq!(fs::read(&edited).unwrap(), b"something else");
}

#[test]
fn a_replay_counts_the_time_in_the_target() {
    let normal = Config {
        duration: Duration::from_secs(60),
        ..Config::default()
    };
    let replay = Config {
        replay: true,
        ..normal.clone()
    };
    let (s, ms) = (Duration::from_secs, Duration::from_millis);
    // A normal run goes by the clock alone.
    assert_eq!(engine::time_is_up(&normal, s(59), s(59)), None);
    assert_eq!(engine::time_is_up(&normal, s(60), ms(1)), Some(Stop::Time));
    // A replay, by the time in the target, which a run of 60 s had less
    // of: so it usually gets as far.
    assert_eq!(engine::time_is_up(&replay, s(600), s(59)), None);
    assert_eq!(
        engine::time_is_up(&replay, s(61), s(60)),
        Some(Stop::TargetTime)
    );
    // But not for ever.
    assert_eq!(engine::REPLAY_WALL_FACTOR, 20);
    assert_eq!(engine::time_is_up(&replay, s(1199), ms(1)), None);
    assert_eq!(
        engine::time_is_up(&replay, s(1200), ms(1)),
        Some(Stop::WallCap)
    );
    for (stop, code) in [
        (Stop::Unchanged, "unchanged"),
        (Stop::Time, "time"),
        (Stop::TargetTime, "target-time"),
        (Stop::WallCap, "wall-cap"),
        (Stop::MaxInputs, "max-inputs"),
        (Stop::Hang, "hang"),
        (Stop::SlowInput, "slow-input"),
    ] {
        assert_eq!(stop.code(), code);
        assert_eq!(Stop::describe(code), Some(stop.to_string().as_str()));
    }
    assert_eq!(Stop::describe("other"), None);
}

#[test]
fn a_crash_directory_that_cannot_be_made_is_an_error() {
    static BLOCK: Mutex<Option<PathBuf>> = Mutex::new(None);
    /// Puts a file where the crash directory will be made, once the run
    /// has read the (missing) directory, then panics as [`planted`] does.
    fn block_then_panic(input: &[u8]) {
        if let Some(path) = lock(&BLOCK).take() {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"in the way").unwrap();
        }
        planted(input);
    }
    let scratch = Scratch::new("unwritable");
    let target = scratch
        .target("blocked", block_then_panic, &[b"x"])
        .dictionary(PLANT);
    *lock(&BLOCK) = Some(target.crashes());
    let error = target.run_with(&inputs(3000)).unwrap_err();
    assert!(matches!(error, FuzzError::NotAFile { .. }), "{error:?}");
}

#[test]
fn links_on_the_way_are_refused_and_nothing_goes_through_them() {
    let scratch = Scratch::new("links");
    let outside = Scratch::new("links-outside");
    fs::create_dir_all(outside.root.join("crashes/linked")).unwrap();
    outside.write("crashes/linked/keep-me", b"outside");

    // A linked `fuzz/crashes/`: the run refuses it, before any input.
    let target = scratch.target("linked", planted, &[b"x"]).dictionary(PLANT);
    if let Err(reason) = link(
        &outside.root.join("crashes"),
        &scratch.root.join("fuzz/crashes"),
    ) {
        skipped(
            "links_on_the_way_are_refused_and_nothing_goes_through_them",
            &reason,
        );
        return;
    }
    for config in [
        inputs(3000),
        Config {
            replay: true,
            ..inputs(300)
        },
    ] {
        let error = target.run_with(&config).unwrap_err();
        assert!(
            matches!(&error, FuzzError::Link { path } if *path == scratch.root.join("fuzz/crashes")),
            "{error:?}"
        );
        assert!(
            error
                .to_string()
                .ends_with("is a symbolic link, which the fuzzer never follows"),
            "{error}"
        );
    }
    assert_eq!(files_in(&outside.root.join("crashes/linked")), ["keep-me"]);
    assert_eq!(
        keep_died(&scratch.root, "linked").unwrap_err().to_string(),
        format!(
            "`{}` is a symbolic link, which the fuzzer never follows",
            scratch.root.join("fuzz/crashes").display()
        )
    );

    // A linked `fuzz/`.
    let other = Scratch::new("links-fuzz");
    outside.write("fuzz/corpus/t/seed", b"x");
    link(&outside.root.join("fuzz"), &other.root.join("fuzz")).unwrap();
    let error = Target::new(&other.root, "t", accept)
        .run_with(&Config::default())
        .unwrap_err();
    assert!(
        matches!(&error, FuzzError::Link { path } if *path == other.root.join("fuzz")),
        "{error:?}"
    );

    // A linked seed in a corpus, a linked crash, and a linked directory on
    // the way to data read in place.
    let third = Scratch::new("links-inside");
    let target = third.target("t", accept, &[b"x"]);
    link(
        &outside.root.join("fuzz/corpus/t/seed"),
        &third.root.join("fuzz/corpus/t/link"),
    )
    .unwrap();
    let error = target.run_with(&Config::default()).unwrap_err();
    assert!(matches!(error, FuzzError::Link { .. }), "{error:?}");
    fs::remove_file(third.root.join("fuzz/corpus/t/link")).unwrap();
    link(
        &outside.root.join("fuzz/corpus/t/seed"),
        &third.root.join("fuzz/crashes/t/panic-0"),
    )
    .unwrap();
    let error = target.run_with(&Config::default()).unwrap_err();
    assert!(matches!(error, FuzzError::Link { .. }), "{error:?}");
    fs::remove_file(third.root.join("fuzz/crashes/t/panic-0")).unwrap();
    link(
        &outside.root.join("fuzz/corpus"),
        &third.root.join("data/linked"),
    )
    .unwrap();
    let error = target
        .clone()
        .seeds_in("data/linked/t", &["seed"])
        .run_with(&Config::default())
        .unwrap_err();
    assert!(
        matches!(&error, FuzzError::Link { path } if *path == third.root.join("data/linked")),
        "{error:?}"
    );

    // A link at the replay's own file: refused, and what it leads to is
    // left alone.
    link(
        &outside.root.join("crashes/linked/keep-me"),
        &third.root.join("fuzz/crashes/t/.last-input"),
    )
    .unwrap();
    let error = target
        .run_with(&Config {
            replay: true,
            ..inputs(10)
        })
        .unwrap_err();
    assert!(matches!(error, FuzzError::Link { .. }), "{error:?}");
    assert_eq!(
        fs::read(outside.root.join("crashes/linked/keep-me")).unwrap(),
        b"outside"
    );
}

#[test]
fn seed_directories_that_cannot_be_used_are_errors() {
    let scratch = Scratch::new("bad-seeds");
    let missing = Target::new(&scratch.root, "missing", accept);
    let error = missing.run_with(&Config::default()).unwrap_err();
    assert!(matches!(error, FuzzError::Read { .. }), "{error:?}");

    fs::create_dir_all(scratch.root.join("fuzz/corpus/empty")).unwrap();
    scratch.write("fuzz/corpus/empty/.gitkeep", b"");
    let error = Target::new(&scratch.root, "empty", accept)
        .run_with(&Config::default())
        .unwrap_err();
    assert!(matches!(error, FuzzError::NoSeeds { .. }), "{error:?}");
    assert!(
        error
            .to_string()
            .ends_with("fuzz/corpus/empty` holds no seeds"),
        "{error}"
    );

    // A corpus is flat: a directory in it is not a seed.
    let nested = scratch.target("nested", accept, &[b"x"]);
    scratch.write("fuzz/corpus/nested/sub/y", b"y");
    let error = nested.run_with(&Config::default()).unwrap_err();
    assert!(matches!(error, FuzzError::NotAFile { .. }), "{error:?}");

    let big = scratch.target("big", accept, &[b"x"]);
    scratch.write(
        "fuzz/corpus/big/huge",
        &vec![b'x'; MAX_SEED_BYTES as usize + 1],
    );
    let error = big.run_with(&Config::default()).unwrap_err();
    assert!(matches!(error, FuzzError::TooLarge { .. }), "{error:?}");

    // Extra seeds that name no file are a mistake, not an empty set; and
    // they are in the repository.
    scratch.write("data/a.txt", b"a");
    let error = scratch
        .target("extra", accept, &[b"x"])
        .seeds_in("data", &["json"])
        .run_with(&Config::default())
        .unwrap_err();
    assert!(matches!(error, FuzzError::NoSeeds { .. }), "{error:?}");
    for outside in ["../data", "/data"] {
        let error = scratch
            .target("extra", accept, &[b"x"])
            .seeds_in(outside, &["json"])
            .run_with(&Config::default())
            .unwrap_err();
        assert!(matches!(error, FuzzError::NotUnderRoot { .. }), "{error:?}");
    }

    // A crash directory that is a file.
    let target = scratch.target("crashfile", accept, &[b"x"]);
    scratch.write("fuzz/crashes/crashfile", b"not a directory");
    let error = target.run_with(&Config::default()).unwrap_err();
    assert!(matches!(error, FuzzError::NotAFile { .. }), "{error:?}");
}

#[test]
#[should_panic(expected = "a fuzz target's name is lower-case ASCII letters")]
fn a_target_name_must_be_plain() {
    let _ = Target::new(".", "Json targets", accept);
}

#[test]
#[should_panic(expected = "fuzz target `missing` could not run: cannot read `")]
fn run_panics_when_the_target_cannot_run() {
    let scratch = Scratch::new("run-panics");
    let _ = Target::new(&scratch.root, "missing", accept).run();
}

#[test]
fn the_configuration_comes_from_the_environment() {
    let vars = |pairs: &'static [(&'static str, &'static str)]| {
        move |name: &'static str| match pairs.iter().find(|(key, _)| *key == name) {
            Some((_, value)) => Ok((*value).to_string()),
            None => Err(VarError::NotPresent),
        }
    };
    assert_eq!(Config::from_vars(vars(&[])).unwrap(), Config::default());
    let config = Config::from_vars(vars(&[
        (SECS_VAR, " 60\n"),
        (SEED_VAR, "0xff"),
        (REPLAY_VAR, "1"),
    ]))
    .unwrap();
    assert_eq!(
        config,
        Config {
            seed: 255,
            duration: Duration::from_secs(60),
            replay: true,
            check_reserved: true,
            ..Config::default()
        }
    );
    // Set at all, the seconds turn the reserved check on.
    let zero = Config::from_vars(vars(&[(SECS_VAR, "0")])).unwrap();
    assert!(zero.check_reserved && zero.duration.is_zero());
    assert_eq!(
        Config::from_vars(vars(&[(SEED_VAR, "12")])).unwrap().seed,
        12
    );
    assert!(
        !Config::from_vars(vars(&[(REPLAY_VAR, "0")]))
            .unwrap()
            .replay
    );
    for (pairs, message) in [
        (
            &[(SECS_VAR, "1.5")][..],
            "TIDEPOOL_FUZZ_SECS must be a whole number of seconds, but it is \"1.5\"",
        ),
        (
            &[(SEED_VAR, "seed")],
            "TIDEPOOL_FUZZ_SEED must be a u64, in decimal or as 0x-prefixed hex, but it is \"seed\"",
        ),
        (
            &[(REPLAY_VAR, "yes")],
            "TIDEPOOL_FUZZ_REPLAY must be `1` or `0`, but it is \"yes\"",
        ),
    ] {
        let pairs: &'static [(&'static str, &'static str)] = pairs;
        let error = Config::from_vars(vars(pairs)).unwrap_err();
        assert_eq!(error.to_string(), message);
    }
    // The names, as `fuzz-smoke` and the docs spell them.
    assert_eq!(
        (SECS_VAR, SEED_VAR, REPLAY_VAR),
        (
            "TIDEPOOL_FUZZ_SECS",
            "TIDEPOOL_FUZZ_SEED",
            "TIDEPOOL_FUZZ_REPLAY"
        )
    );
}

#[test]
fn the_result_line_reads_back() {
    let scratch = Scratch::new("summary");
    let outcome = scratch
        .target("line", accept, &[b"x"])
        .run_with(&inputs(10))
        .unwrap();
    let summary = outcome.summary();
    let line = summary.to_string();
    assert!(
        line.starts_with(
            "fuzz-result: target=line seed=0x0000000000000001 seeds=1 crashes=0 inputs=11 micros="
        ),
        "{line}"
    );
    assert!(
        line.contains(" findings=0 saved=0 ranges=0 hung=0 stopped=max-inputs "),
        "{line}"
    );
    let mut expected = summary.clone();
    // Times are kept to the microsecond.
    expected.elapsed = Duration::from_micros(summary.elapsed.as_micros() as u64);
    expected.slowest = summary.slowest.map(|slowest| Slowest {
        took: Duration::from_micros(slowest.took.as_micros() as u64),
        ..slowest
    });
    assert_eq!(Summary::parse(&line), Some(expected.clone()));
    assert_eq!(Summary::parse(&format!("  {line}\r")), Some(expected));

    let full = Summary {
        target: "json".to_string(),
        seed: u64::MAX,
        seeds: 3,
        crashes: 1,
        inputs: 1_000_000,
        elapsed: Duration::from_micros(60_000_123),
        findings: 2,
        saved: 1,
        ranges: 1,
        hung: true,
        stopped: "hang".to_string(),
        memory: Reading::Measured(PeakMemory {
            baseline: 1 << 20,
            peak: 5 << 20,
            cap: 64 << 20,
            stopped: Some("read-failed".to_string()),
        }),
        reserved: Reading::Skipped("not-asked".to_string()),
        slowest: Some(Slowest {
            took: Duration::from_micros(1234),
            len: 99,
        }),
    };
    let line = full.to_string();
    assert_eq!(
        line,
        "fuzz-result: target=json seed=0xffffffffffffffff seeds=3 crashes=1 inputs=1000000 \
         micros=60000123 findings=2 saved=1 ranges=1 hung=1 stopped=hang memory=measured \
         memory_baseline=1048576 memory_peak=5242880 memory_cap=67108864 \
         memory_stopped=read-failed reserved=skipped reserved_reason=not-asked \
         slowest_micros=1234 slowest_bytes=99"
    );
    assert_eq!(Summary::parse(&line), Some(full.clone()));
    let bare = Summary {
        memory: Reading::Skipped("unreadable".to_string()),
        reserved: Reading::Measured(PeakMemory {
            baseline: 1,
            peak: 2,
            cap: 3,
            stopped: None,
        }),
        slowest: None,
        hung: false,
        ..full
    };
    assert_eq!(Summary::parse(&bare.to_string()), Some(bare));

    let good = "fuzz-result: target=json seed=1 seeds=1 crashes=0 inputs=1 micros=1 findings=0 \
                saved=0 ranges=0 hung=0 stopped=time memory=skipped memory_reason=x \
                reserved=skipped reserved_reason=y";
    assert!(Summary::parse(good).is_some());
    // Keys it does not know are ignored, for a later version's line.
    assert!(Summary::parse(&format!("{good} later=1")).is_some());
    for bad in [
        "",
        "test json ... ok",
        "fuzz-result:",
        "fuzz-result: target=json",
        &good.replace("hung=0", "hung=2"),
        &good.replace("memory=skipped", "memory=maybe"),
        &good.replace("inputs=1", "inputs=x"),
        &good.replace(" memory_reason=x", ""),
        &good.replace(" saved=0", ""),
        &good.replace(" stopped=time", ""),
        &good.replace("reserved=skipped", "reserved=measured"),
        &format!("{good} slowest_micros=4"),
        &good.replace("target=json", "target=json target=toml"),
        &good.replace("target=json", "stray target=json"),
    ] {
        assert_eq!(Summary::parse(bad), None, "{bad:?}");
    }
}

#[test]
fn only_panics_inside_a_target_are_kept_quiet() {
    fn inside(_: &[u8]) {
        panic!("inside");
    }
    fn outside() {
        panic!("outside");
    }
    engine::tests_support::install_panic_hook();

    // Inside a target: the place is recorded and handed over.
    let (message, site) = engine::tests_support::run_target(inside, b"x").unwrap();
    assert_eq!(message, "inside");
    assert!(site.contains("tests.rs:"), "{site}");
    assert!(!engine::tests_support::in_target());

    // Afterwards this thread is no longer inside a target, so its panics
    // take the ordinary route.
    let caught = panic::catch_unwind(outside);
    assert_eq!(caught.unwrap_err().downcast_ref::<&str>(), Some(&"outside"));
    assert!(engine::tests_support::run_target(accept, b"x").is_none());
}
