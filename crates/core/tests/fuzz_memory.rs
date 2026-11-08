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

//! The fuzzer's reserved-memory check (`VmPeak`), on a planted reservation
//! that is never written.
//!
//! In a test binary of its own, with one test, because the reserved peak
//! is the whole process's: in the unit tests' binary, every test thread
//! libtest starts reserves an arena and a stack, and moves it by tens of
//! MiB at the very moment the planted input would be blamed. This is why
//! the engine checks it only when `TIDEPOOL_FUZZ_SECS` is set, even to 0:
//! in the runs `fuzz-smoke` starts, and in the reproduce command of a saved
//! reserved-memory crash, each one target to a process (ADR-0027).

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use tidepool_core::hash::fnv1a64;
use tidepool_core::testing::fuzz::{Config, DEFAULT_RESERVED_CAP, Found, Kind, Memory, Target};

/// How much [`reserve`] reserves.
static SIZE: AtomicUsize = AtomicUsize::new(0);

/// Whether a reservation failed: overcommit is off, or the address space
/// is small.
static REFUSED: AtomicBool = AtomicBool::new(false);

/// Reserves [`SIZE`] bytes, never written, on every input that holds
/// `RESV`, which mutation adds, as a parser that trusts a claimed length
/// does with `Vec::with_capacity(claimed)`. `try_reserve_exact` reserves
/// the same way, but where the reservation is refused it does not abort.
fn reserve(input: &[u8]) {
    if input.windows(4).any(|window| window == b"RESV") {
        let mut claimed = Vec::<u8>::new();
        if claimed
            .try_reserve_exact(SIZE.load(Ordering::Relaxed))
            .is_err()
        {
            REFUSED.store(true, Ordering::Relaxed);
        }
        std::hint::black_box(&claimed);
    }
}

/// A `/proc/self/status` field, in bytes.
fn status(name: &str) -> Option<u64> {
    let text = fs::read_to_string("/proc/self/status").ok()?;
    let line = text.lines().find_map(|line| line.strip_prefix(name))?;
    let kib: u64 = line.trim().strip_suffix("kB")?.trim().parse().ok()?;
    Some(kib * 1024)
}

/// Sets [`SIZE`] so that a reservation raises the reserved peak by about
/// 1 GiB from where it is now, however it got there: the gap between the
/// peak and what is in use, and 1 GiB. Each run in this process needs its
/// own, since the peak never comes down. Returns `false` where
/// `/proc/self/status` has no such lines.
fn arm() -> bool {
    let (Some(peak), Some(size)) = (status("VmPeak:"), status("VmSize:")) else {
        return false;
    };
    let size = usize::try_from(peak.saturating_sub(size) + (1 << 30)).unwrap_or(usize::MAX);
    SIZE.store(size, Ordering::Relaxed);
    true
}

/// Reserves, never writing it, what the seed names: `under` 16 MiB less
/// than the default cap, `over` 16 MiB more.
fn reserve_by_name(input: &[u8]) {
    let cap = usize::try_from(DEFAULT_RESERVED_CAP).expect("256 MiB fits a usize");
    let size = match input {
        b"under" => cap - (16 << 20),
        b"over" => cap + (16 << 20),
        _ => return,
    };
    let mut claimed = Vec::<u8>::new();
    if claimed.try_reserve_exact(size).is_err() {
        REFUSED.store(true, Ordering::Relaxed);
    }
    std::hint::black_box(&claimed);
}

/// A fresh directory for the test, in the target directory cargo gives
/// integration tests.
fn scratch() -> PathBuf {
    let root =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("fuzz-memory-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("fuzz/corpus/reserve")).expect("make the corpus");
    fs::write(root.join("fuzz/corpus/reserve/00"), b"a claim").expect("write a seed");
    fs::write(root.join("fuzz/corpus/reserve/01"), b"another one").expect("write a seed");
    root
}

/// A reservation 16 MiB under the cap is not a finding, and one 16 MiB over
/// it is, named exactly: the cap counts from the address space in use, not
/// from the peak, which glibc's arena trim leaves 64 MiB above it. This
/// runs first in the process, before anything has raised the peak.
fn the_reserved_cap_is_the_documented_one(root: &Path) {
    for (name, bytes) in [("00", &b"under"[..]), ("01", b"over")] {
        let dir = root.join("fuzz/corpus/cap");
        fs::create_dir_all(&dir).expect("make the corpus");
        fs::write(dir.join(name), bytes).expect("write a seed");
    }
    let target = Target::new(root, "cap", reserve_by_name).memory_cap(16 << 20);
    let outcome = target
        .run_with(&Config {
            check_reserved: true,
            ..Config::default()
        })
        .expect("the run starts");
    let report = outcome.to_string();
    assert_eq!(outcome.findings.len(), 1, "{report}");
    let finding = &outcome.findings[0];
    assert_eq!(finding.kind, Kind::Reserved, "{report}");
    assert_eq!(
        finding.path(),
        Some(target.corpus().join("01").as_path()),
        "{report}"
    );
}

/// Acceptance (M1.6 review): an untouched reservation is found in a run
/// that mutates, and its replay names and saves the input; and the cap is
/// where the docs say.
#[test]
fn a_reservation_is_found_by_range_and_saved_by_its_replay() {
    if status("VmPeak:").is_none() || status("VmSize:").is_none() {
        let _ = writeln!(
            std::io::stderr(),
            "SKIPPED: a_reservation_is_found_by_range_and_saved_by_its_replay: \
             /proc/self/status has no VmPeak here, so reserved memory is not checked"
        );
        return;
    }
    let root = scratch();
    the_reserved_cap_is_the_documented_one(&root);
    if REFUSED.load(Ordering::Relaxed) {
        let _ = writeln!(
            std::io::stderr(),
            "SKIPPED: a_reservation_is_found_by_range_and_saved_by_its_replay: this system \
             refuses to reserve the default cap"
        );
        return;
    }
    if !arm() {
        let _ = writeln!(
            std::io::stderr(),
            "SKIPPED: a_reservation_is_found_by_range_and_saved_by_its_replay: \
             /proc/self/status has no VmPeak here, so reserved memory is not checked"
        );
        return;
    }
    let target = Target::new(&root, "reserve", reserve)
        .dictionary(&[b"RESV"])
        .memory_cap(16 << 20);
    let config = Config {
        seed: 7,
        duration: Duration::from_secs(3600),
        max_inputs: 400,
        check_reserved: true,
        ..Config::default()
    };

    // The run that mutates reads on the budget: it names the inputs since
    // the read before, and saves none.
    let outcome = target.run_with(&config).unwrap();
    if REFUSED.load(Ordering::Relaxed) {
        let _ = writeln!(
            std::io::stderr(),
            "SKIPPED: a_reservation_is_found_by_range_and_saved_by_its_replay: this system \
             refuses to reserve {} bytes",
            SIZE.load(Ordering::Relaxed)
        );
        return;
    }
    let report = outcome.to_string();
    let reserved: Vec<_> = outcome
        .findings
        .iter()
        .filter(|finding| finding.kind == Kind::Reserved)
        .collect();
    assert_eq!(reserved.len(), 1, "{report}");
    let Found::Range { first, last } = reserved[0].input else {
        panic!("{report}");
    };
    assert!(first <= last, "{report}");
    assert_eq!(outcome.saved, 0, "{report}");
    assert!(!target.crashes().exists(), "{report}");
    assert!(
        reserved[0]
            .message
            .starts_with("the reserved peak (VmPeak) rose to "),
        "{report}"
    );
    assert!(
        report.contains(&format!(
            "\n    input: one of the mutated inputs {first} to {last}, none of them saved; a \
             replay reads after every input, and names and saves the one\n"
        )),
        "{report}"
    );
    assert!(
        report.contains(
            "\n  to name the input: TIDEPOOL_FUZZ_SEED=0x0000000000000007 TIDEPOOL_FUZZ_SECS=3600 \
             TIDEPOOL_FUZZ_REPLAY=1 cargo test --profile fuzz -p tidepool-core --test fuzz"
        ),
        "{report}"
    );
    assert!(
        matches!(outcome.reserved, Memory::Measured { .. }),
        "{report}"
    );
    assert_eq!(outcome.summary().ranges, 1);
    // Nothing was written, so the written peak saw nothing.
    assert!(
        outcome
            .findings
            .iter()
            .all(|finding| finding.kind != Kind::Memory),
        "{report}"
    );

    // Its replay reads after every input: it names the one, which is in
    // that range, and saves it.
    let replay = Config {
        replay: true,
        ..config
    };
    assert!(arm());
    let outcome = target.run_with(&replay).unwrap();
    let report = outcome.to_string();
    let reserved: Vec<_> = outcome
        .findings
        .iter()
        .filter(|finding| finding.kind == Kind::Reserved)
        .collect();
    assert_eq!(reserved.len(), 1, "{report}");
    let Found::File { path, number, .. } = &reserved[0].input else {
        panic!("{report}");
    };
    let number = number.expect("a mutated input");
    assert!(
        (first..=last).contains(&number),
        "{number} in {first}..={last}: {report}"
    );
    let saved = fs::read(path).unwrap();
    assert!(saved.windows(4).any(|window| window == b"RESV"));
    assert_eq!(
        *path,
        target
            .crashes()
            .join(format!("reserve-{:016x}", fnv1a64(&saved)))
    );
    assert_eq!(outcome.saved, 1);
    assert!(
        report.contains(" to reproduce: TIDEPOOL_FUZZ_SECS=0 cargo test -p tidepool-core"),
        "{report}"
    );

    // As a saved crash it runs first, and a run that checks reserved
    // memory names it at once; a plain run does not check it.
    let unchanged = Config {
        check_reserved: true,
        ..Config::default()
    };
    assert!(arm());
    let outcome = target.run_with(&unchanged).unwrap();
    assert_eq!(outcome.findings.len(), 1, "{outcome}");
    assert_eq!(outcome.findings[0].path(), Some(path.as_path()));
    assert!(arm());
    let plain = target.run_with(&Config::default()).unwrap();
    assert!(plain.is_clean(), "{plain}");

    let _ = fs::remove_dir_all(&root);
}
