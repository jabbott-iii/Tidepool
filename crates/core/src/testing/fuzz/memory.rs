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

//! The memory checks (ADR-0027): whether the process's written memory
//! (`VmHWM`, the peak resident set) has risen more than the target's cap
//! above where it was when the run started; and, when
//! `TIDEPOOL_FUZZ_SECS` is set, even to 0 (in the runs `fuzz-smoke`
//! starts, and in the reproduce command of a saved reserved-memory crash,
//! each one target to a process), whether its reserved memory (`VmPeak`,
//! the peak address space) has risen more than its own cap above the
//! address space in use (`VmSize`). All three come from one read of
//! `/proc/self/status`, made once the target's time could have taken the
//! written peak past its cap, and after every unchanged input and every
//! input of a replay.
//!
//! New for Tidepool; Nadir's fuzzer has no memory check.
//!
//! Linux keeps both peaks for the whole process. Where the file cannot be
//! opened or read (macOS, Windows, a Linux without `/proc`), the checks
//! are skipped, and the run's [`Memory`] says why, with a short code for
//! `fuzz-smoke`'s line. There is no `cfg`: the code is the same everywhere,
//! and a file that cannot be read is how it learns it is elsewhere.
//!
//! # What it costs, and why the written check is still exact
//!
//! Reading `/proc/self/status` costs about 3.4 µs, measured on the
//! development machine (Linux 7.2, 16 CPUs): Linux writes the whole file
//! out, about 1.5 KiB, on every read, even from offset 0 of a file kept
//! open. A target's input takes 1 to 5 µs in `fuzz-smoke`'s profile, so
//! reading it after every input cut the inputs per second by 53 to 87%.
//! Reading it only after inputs on which the target's thread took a page
//! fault (`/proc/thread-self/stat`, about 1.2 µs a read) still cut them by
//! 35 to 75%.
//!
//! So mutated inputs are read after only once the written peak could have
//! passed its cap. Memory becomes resident only as fast as the kernel can
//! fault in and zero its pages for the thread that touches it, which on
//! that machine was about 3 GB/s (a 1 GiB `vec![1; n]`). The probe allows
//! [`MAX_GROWTH_PER_MICROSECOND`], 1 MiB a microsecond (about 1 TB/s,
//! some 300 times that), and keeps a budget: the target's time, since the
//! last read, in which the written peak cannot have risen from what that
//! read saw to past the cap. It reads after the input that spends the
//! budget. That input is then the only one since the last read during
//! which the written peak can have passed the cap, so the input named is
//! exact, as long as the bound holds. The closer the peak is to the cap,
//! the more often it reads, up to after every input. Unchanged inputs,
//! which are few, and every input of a replay are read after one by one.
//!
//! Measured over 10 s a target with a 64 MiB cap (inputs a second, without
//! the check, then reading after every input, after each fault, and with
//! the budget):
//!
//! | Target | No check | Every input | After a fault | Budget |
//! | :- | -: | -: | -: | -: |
//! | `json` | 332,000 | 122,000 | 217,000 | 355,000 |
//! | `toml` | 1,290,000 | 173,000 | 324,000 | 1,272,000 |
//! | `encoding` | 262,000 | 123,000 | 163,000 | 293,000 |
//!
//! The budget's cost was within the 10% that one run differs from the
//! next. A smaller cap leaves less room, and so reads more often.
//!
//! # Reserved memory
//!
//! `VmHWM` counts only memory that was written. A parser that reserves
//! what an input claims (`Vec::with_capacity(claimed)`) and writes little
//! of it moves only `VmPeak`, and Linux's overcommit lets such a
//! reservation succeed. So the probe reads `VmPeak` too, in the same read,
//! with its own cap ([`DEFAULT_RESERVED_CAP`](super::DEFAULT_RESERVED_CAP)
//! unless the target sets one), counted from the address space in use
//! (`VmSize`) once the target's thread has allocated once, so that the
//! 64 MiB arena glibc reserves for a thread's first allocation is not
//! counted. Making that arena maps 128 MiB and gives half back, so
//! `VmPeak` starts about 64 MiB above `VmSize`; a check from `VmPeak`
//! would see a reservation only 64 MiB past its cap. The probe finds one
//! when `VmPeak` passes the larger of where it was at the start and
//! `VmSize` at the start plus the cap, so the rise it reports starts at
//! that gap. Address space grows at any
//! rate (8 GiB in 46 µs), so the budget does not bound it: a read on the
//! budget that sees it past the cap knows only that one of the inputs since
//! the last read did it, and the finding names that range and saves
//! nothing. A replay reads after every input, and so names and saves the
//! one.
//!
//! Every new thread moves `VmPeak` by tens of MiB (an arena and a stack),
//! so it is checked only when `TIDEPOOL_FUZZ_SECS` is set, even to 0: in
//! the runs `fuzz-smoke` starts, and in the reproduce command of a saved
//! reserved-memory crash, each one target to a process. A plain `cargo
//! test` says it was not checked, and why.
//!
//! # What it cannot see
//!
//! - The process as a whole: memory that another thread makes resident or
//!   reserves counts too, and is blamed on the input that is running. The
//!   targets of one test binary never run at once
//!   ([`Target::run_with`](super::Target::run_with) holds a lock), but the
//!   other tests may; and a thread left stuck in a target that hung keeps
//!   running, so while one is, later runs in the process check neither
//!   (`hung-thread`).
//! - The written check rests on the 1 MiB-a-microsecond bound, measured on
//!   one machine. Memory made resident faster than that by the target's
//!   thread can be blamed on a later input.
//! - Linux counts resident pages in per-CPU batches, so `VmHWM` can lag
//!   the truth by a few hundred pages (about 2 MiB on a 16-CPU machine): a
//!   cap is exact to about that much.
//! - Small allocations inside an arena's reservation move neither peak
//!   until they are written; a reservation smaller than the reserved cap is
//!   not seen. `Limits` checks before allocating remain the guard.
//! - An input that takes so much memory that the process dies does not
//!   return to be checked; a replay finds it
//!   ([`REPLAY_VAR`](super::REPLAY_VAR)).

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::time::Duration;

use super::{Memory, Skip};

/// Where Linux reports the process's memory, `VmHWM` and `VmPeak` among
/// it.
pub(super) const STATUS: &str = "/proc/self/status";

/// The fastest the probe takes the target's thread to make memory
/// resident, in bytes a microsecond: 1 MiB, about 1 TB/s.
pub(super) const MAX_GROWTH_PER_MICROSECOND: u64 = 1024 * 1024;

/// How far the written peak may still rise before the next read, for the
/// budget, once it has passed its cap and only the reserved check is
/// left: 64 MiB, a read every 64 µs of the target's time.
const ROOM_AFTER_WRITTEN: u64 = 64 * 1024 * 1024;

/// How much of the file is read at a time.
const CHUNK: u64 = 4096;

/// The most of the file that is read while looking for the lines. Linux
/// writes `VmPeak`, `VmSize` and `VmHWM` in its first few hundred bytes.
const MAX_STATUS_BYTES: usize = 1024 * 1024;

/// Held by the tests that raise the process's peaks, or that would fail
/// if another test raised them while they run: the peaks are the whole
/// process's, and never come down.
#[cfg(test)]
pub(super) static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Locks [`SERIAL`], whatever a test that panicked while holding it left.
#[cfg(test)]
pub(super) fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// One peak and its cap.
#[derive(Clone, Copy, Debug)]
struct Gauge {
    /// What the cap counts from, in bytes: the peak when the run started,
    /// or for the reserved check, the larger of the address space then in
    /// use and the peak then less the cap.
    baseline: u64,
    /// For the reserved check, the address space in use (`VmSize`) when
    /// the run started; for the written one, `baseline`. Only for the
    /// message.
    in_use: u64,
    /// When last read, in bytes.
    peak: u64,
    /// How far above `baseline` it may rise, in bytes.
    cap: u64,
    /// Whether it has gone past the cap. It never comes back, so after
    /// that it is not looked at.
    exceeded: bool,
}

impl Gauge {
    fn new(level: u64, cap: u64) -> Self {
        Self::from(level, level, level, cap)
    }

    /// A gauge whose cap counts from `baseline`, with `in_use` in use, its
    /// peak now `peak`.
    fn from(baseline: u64, in_use: u64, peak: u64, cap: u64) -> Self {
        Self {
            baseline,
            in_use,
            peak,
            cap,
            exceeded: false,
        }
    }

    /// Takes a new reading; returns the crossing the first time the peak
    /// is past the cap.
    fn update(&mut self, level: u64) -> Option<Exceeded> {
        if self.exceeded {
            return None;
        }
        self.peak = self.peak.max(level);
        if self.peak.saturating_sub(self.baseline) <= self.cap {
            return None;
        }
        self.exceeded = true;
        Some(Exceeded {
            baseline: self.baseline,
            in_use: self.in_use,
            peak: self.peak,
            cap: self.cap,
        })
    }

    fn memory(&self, stopped: Option<Skip>) -> Memory {
        Memory::Measured {
            baseline: self.baseline,
            peak: self.peak,
            cap: self.cap,
            stopped,
        }
    }
}

/// The memory checks of one run.
pub(super) struct Probe {
    status: File,
    written: Gauge,
    /// The reserved check, when it was asked for and `VmPeak` is there.
    reserved: Option<Gauge>,
    /// Why the reserved check was asked for but does not run.
    reserved_skipped: Option<Skip>,
    /// The target's time in which the written peak cannot have risen past
    /// the cap since it was last read, in nanoseconds.
    budget: u64,
    /// How many times the file was read, the first included.
    reads: u64,
    /// Why the probe stopped reading, if a read failed during the run.
    stopped: Option<Skip>,
    buffer: Vec<u8>,
}

/// A peak that an input took past its cap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Exceeded {
    /// What the cap counts from, in bytes: as [`Gauge`]'s.
    pub(super) baseline: u64,
    /// For the reserved check, the address space in use at the start of
    /// the run, in bytes; for the written one, `baseline`.
    pub(super) in_use: u64,
    /// After the input, in bytes.
    pub(super) peak: u64,
    /// The cap, in bytes above `baseline`.
    pub(super) cap: u64,
}

/// What a check after an input found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Seen {
    /// Whether the file was read.
    pub(super) read: bool,
    /// The written peak, if it went past its cap.
    pub(super) written: Option<Exceeded>,
    /// The reserved peak, if it went past its cap.
    pub(super) reserved: Option<Exceeded>,
}

/// The three lines of `/proc/self/status` the probe reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Fields {
    /// `VmHWM`, in bytes.
    pub(super) written: Option<u64>,
    /// `VmPeak`, in bytes.
    pub(super) reserved: Option<u64>,
    /// `VmSize`, the address space in use, in bytes.
    pub(super) in_use: Option<u64>,
}

impl Probe {
    /// Starts the checks: the written one with `written_cap`, and the
    /// reserved one too if `reserved_cap` is given. Call it on the target's
    /// thread after its first allocation. Returns why the checks cannot
    /// run, if they cannot.
    pub(super) fn start(written_cap: u64, reserved_cap: Option<u64>) -> Result<Self, Skip> {
        Self::start_with(STATUS, written_cap, reserved_cap)
    }

    /// [`Probe::start`], reading `status` instead of [`STATUS`].
    pub(super) fn start_with(
        status: &str,
        written_cap: u64,
        reserved_cap: Option<u64>,
    ) -> Result<Self, Skip> {
        let unreadable = |error: &io::Error| Skip {
            code: "unreadable",
            reason: format!("{status} cannot be read ({error}), so memory is not checked here"),
        };
        let file = File::open(status).map_err(|error| unreadable(&error))?;
        let mut probe = Self {
            status: file,
            written: Gauge::new(0, written_cap),
            reserved: None,
            reserved_skipped: None,
            budget: 0,
            reads: 0,
            stopped: None,
            buffer: Vec::with_capacity(CHUNK as usize),
        };
        let fields = probe.read().map_err(|error| unreadable(&error))?;
        let Some(written) = fields.written else {
            return Err(Skip {
                code: "no-vmhwm",
                reason: format!("{status} has no VmHWM line, so memory is not checked here"),
            });
        };
        probe.written = Gauge::new(written, written_cap);
        if let Some(cap) = reserved_cap {
            match (fields.reserved, fields.in_use) {
                (Some(reserved), Some(in_use)) => {
                    // A finding once the peak passes the larger of where it
                    // is and what is in use plus the cap.
                    let baseline = in_use.max(reserved.saturating_sub(cap));
                    probe.reserved = Some(Gauge::from(baseline, in_use, reserved, cap));
                }
                _ => {
                    probe.reserved_skipped = Some(Skip {
                        code: "no-vmpeak",
                        reason: format!(
                            "{status} has no VmPeak or VmSize line, so reserved memory is not \
                             checked here"
                        ),
                    });
                }
            }
        }
        probe.refill();
        Ok(probe)
    }

    /// Checks the peaks after an input that kept the target busy for
    /// `took`: always if `every`, otherwise only once the budget is spent.
    /// Reports each peak the first time it is past its cap.
    pub(super) fn after_input(&mut self, took: Duration, every: bool) -> Seen {
        let reserved_left = self.reserved.is_some_and(|gauge| !gauge.exceeded);
        if self.stopped.is_some() || (self.written.exceeded && !reserved_left) {
            return Seen::default();
        }
        let took = u64::try_from(took.as_nanos()).unwrap_or(u64::MAX);
        if !every && took < self.budget {
            self.budget -= took;
            return Seen::default();
        }
        let fields = match self.read() {
            Ok(fields) => fields,
            Err(error) => {
                self.stop(format!("reading {STATUS} failed during the run: {error}"));
                return Seen::default();
            }
        };
        let (Some(written), true) = (
            fields.written,
            self.reserved.is_none() || fields.reserved.is_some(),
        ) else {
            self.stop(format!("{STATUS} lost a line it had at the start"));
            return Seen::default();
        };
        let mut seen = Seen {
            read: true,
            written: self.written.update(written),
            reserved: None,
        };
        if let (Some(gauge), Some(reserved)) = (self.reserved.as_mut(), fields.reserved) {
            seen.reserved = gauge.update(reserved);
        }
        self.refill();
        seen
    }

    /// The written check, and the reserved one if it was asked for: what
    /// they measured, up to where a read failed if one did.
    pub(super) fn finish(self) -> (Memory, Option<Memory>) {
        self.snapshot()
    }

    /// [`Probe::finish`], as far as the probe has read.
    pub(super) fn snapshot(&self) -> (Memory, Option<Memory>) {
        let written = self.written.memory(self.stopped.clone());
        let reserved = match (&self.reserved, &self.reserved_skipped) {
            (Some(gauge), _) => Some(gauge.memory(self.stopped.clone())),
            (None, Some(skip)) => Some(Memory::Skipped(skip.clone())),
            (None, None) => None,
        };
        (written, reserved)
    }

    /// How many times the file has been read, the first included.
    #[cfg(test)]
    pub(super) fn reads(&self) -> u64 {
        self.reads
    }

    /// The budget left, in nanoseconds.
    #[cfg(test)]
    pub(super) fn budget(&self) -> u64 {
        self.budget
    }

    /// Stops reading, keeping what was measured.
    fn stop(&mut self, reason: String) {
        self.stopped = Some(Skip {
            code: "read-failed",
            reason,
        });
    }

    /// Sets the budget from the room left under the written cap.
    fn refill(&mut self) {
        let room = if self.written.exceeded {
            ROOM_AFTER_WRITTEN
        } else {
            let limit = self.written.baseline.saturating_add(self.written.cap);
            limit.saturating_sub(self.written.peak)
        };
        self.budget = budget_for(room);
    }

    /// Reads the file from its start until the three lines are there, the file
    /// ends, or [`MAX_STATUS_BYTES`] were read. Linux makes the file's text
    /// afresh on a read from offset 0.
    fn read(&mut self) -> io::Result<Fields> {
        self.reads += 1;
        self.status.seek(SeekFrom::Start(0))?;
        self.buffer.clear();
        loop {
            let read = (&mut self.status)
                .take(CHUNK)
                .read_to_end(&mut self.buffer)?;
            let ended = read == 0 || self.buffer.len() >= MAX_STATUS_BYTES;
            let fields = fields(&self.buffer, ended);
            let all =
                fields.written.is_some() && fields.reserved.is_some() && fields.in_use.is_some();
            if ended || all {
                return Ok(fields);
            }
        }
    }
}

/// The time, in nanoseconds, in which memory growing at
/// [`MAX_GROWTH_PER_MICROSECOND`] adds no more than `room` bytes.
pub(super) fn budget_for(room: u64) -> u64 {
    let nanos = u128::from(room) * 1000 / u128::from(MAX_GROWTH_PER_MICROSECOND);
    u64::try_from(nanos).unwrap_or(u64::MAX)
}

/// The three lines in `/proc/self/status` text. A last line without a
/// newline counts only if `ended`: it may be cut off.
pub(super) fn fields(status: &[u8], ended: bool) -> Fields {
    let mut lines: Vec<&[u8]> = status.split(|&byte| byte == b'\n').collect();
    if !ended {
        // What follows the last newline may be the start of a line.
        lines.pop();
    }
    let value = |name: &[u8]| lines.iter().find_map(|line| kib_line(line, name));
    Fields {
        written: value(b"VmHWM:"),
        reserved: value(b"VmPeak:"),
        in_use: value(b"VmSize:"),
    }
}

/// The value of a `<name>\t    1234 kB` line, in bytes.
fn kib_line(line: &[u8], name: &[u8]) -> Option<u64> {
    let text = std::str::from_utf8(line.strip_prefix(name)?).ok()?;
    let kib: u64 = text.trim().strip_suffix("kB")?.trim().parse().ok()?;
    kib.checked_mul(1024)
}

/// How much to reserve so that the reserved peak rises `rise` bytes above
/// what it is now: the gap between the peak so far, which earlier tests may
/// have raised, and the address space in use now, and then `rise`.
#[cfg(test)]
pub(super) fn reservation(rise: u64) -> usize {
    let status = std::fs::read(STATUS).unwrap_or_default();
    let value = |name: &[u8]| {
        status
            .split(|&byte| byte == b'\n')
            .find_map(|line| kib_line(line, name))
            .unwrap_or(0)
    };
    let gap = value(b"VmPeak:").saturating_sub(value(b"VmSize:"));
    usize::try_from(gap + rise).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    #[test]
    fn the_three_lines_are_read_in_bytes() {
        let status = b"Name:\tfuzz\nVmPeak:\t  900 kB\nVmSize:\t 800 kB\nVmHWM:\t    1234 kB\nVmRSS:\t 1000 kB\n";
        assert_eq!(
            fields(status, true),
            Fields {
                written: Some(1234 * 1024),
                reserved: Some(900 * 1024),
                in_use: Some(800 * 1024),
            }
        );
        assert_eq!(fields(b"VmHWM:\t0 kB", true).written, Some(0));
        // A last line that may be cut off is not read until the end.
        assert_eq!(fields(b"VmHWM:\t12", false).written, None);
        assert_eq!(fields(b"VmHWM:\t12 kB\n", false).written, Some(12 * 1024));
        for bad in [
            &b"Name:\tfuzz\n"[..],
            b"VmHWM:\t12 MB\n",
            b"VmHWM:\t\n",
            b"VmHWM: x kB\n",
            b"",
            b"VmHWM:\t99999999999999999999 kB\n",
        ] {
            assert_eq!(
                fields(bad, true).written,
                None,
                "{:?}",
                String::from_utf8_lossy(bad)
            );
        }
    }

    #[test]
    fn the_budget_is_the_time_to_fill_the_room_at_the_fastest_rate() {
        // 1 MiB a microsecond.
        assert_eq!(budget_for(0), 0);
        assert_eq!(budget_for(1024 * 1024), 1000);
        assert_eq!(budget_for(64 * 1024 * 1024), 64_000);
        assert_eq!(budget_for(1048), 0, "less than a nanosecond's worth");
        assert_eq!(budget_for(1049), 1);
        // The room under a cap of `u64::MAX` takes longer than any run.
        assert!(budget_for(u64::MAX) > 500_000 * 1_000_000_000);
    }

    #[test]
    fn without_proc_the_checks_are_skipped_and_say_why() {
        let skip = Probe::start_with("/nonexistent/proc/self/status", 1, Some(1))
            .err()
            .expect("there is no such file");
        assert_eq!(skip.code, "unreadable");
        assert!(
            skip.reason
                .starts_with("/nonexistent/proc/self/status cannot be read ("),
            "{}",
            skip.reason
        );
        assert!(
            skip.reason.ends_with("so memory is not checked here"),
            "{}",
            skip.reason
        );
    }

    #[test]
    fn the_peaks_are_read_only_once_the_budget_is_spent_unless_asked() {
        let _serial = serial();
        // 64 MiB of room: 64 µs of the target's time.
        let mut probe = match Probe::start(64 * 1024 * 1024, Some(u64::MAX)) {
            Ok(probe) => probe,
            Err(skip) => {
                // Not a failure: there is nothing to check here.
                assert!(File::open(STATUS).is_err(), "{STATUS} opens, but: {skip:?}");
                return;
            }
        };
        assert!(probe.written.baseline > 0);
        assert!(probe.reserved.is_some_and(|gauge| gauge.baseline > 0));
        assert_eq!(probe.reads(), 1);
        let budget = probe.budget();
        assert!(budget > 0 && budget <= 64_000, "{budget}");
        // Inputs that together take less than the budget are not read
        // after.
        let half = Duration::from_nanos(budget / 2);
        assert_eq!(probe.after_input(half, false), Seen::default());
        assert_eq!(probe.reads(), 1);
        // The one that spends it is.
        let seen = probe.after_input(Duration::from_secs(1), false);
        assert!(seen.read && seen.written.is_none() && seen.reserved.is_none());
        assert_eq!(probe.reads(), 2);
        // And any input, when asked.
        assert!(probe.after_input(Duration::ZERO, true).read);
        assert_eq!(probe.reads(), 3);
        let (written, reserved) = probe.finish();
        assert!(matches!(written, Memory::Measured { stopped: None, .. }));
        assert!(matches!(reserved, Some(Memory::Measured { .. })));
    }

    #[test]
    fn a_rise_past_either_cap_is_reported_once() {
        let _serial = serial();
        let Ok(mut probe) = Probe::start(1024 * 1024, None) else {
            assert!(File::open(STATUS).is_err());
            return;
        };
        // 8 MiB more than the written peak so far (which earlier tests may
        // have raised), touched, and freed before the check: the peak keeps
        // it.
        let size = usize::try_from(probe.written.baseline).unwrap() + (8 << 20);
        let big = vec![1_u8; size];
        std::hint::black_box(&big);
        drop(big);
        let seen = probe.after_input(Duration::from_millis(10), false);
        let written = seen.written.expect("the written peak rose past 1 MiB");
        assert!(written.peak - written.baseline > written.cap);
        assert_eq!(probe.after_input(Duration::ZERO, true), Seen::default());

        // Reserved and never written, memory moves only the reserved peak:
        // 512 MiB more than the reserved peak so far. A probe of its own,
        // read once, so that other tests' threads have no time to move the
        // peak first.
        let size = reservation(512 << 20);
        let mut probe = Probe::start(u64::MAX, Some(256 << 20)).expect("it started before");
        let mut reserved = Vec::<u8>::new();
        if reserved.try_reserve_exact(size).is_err() {
            let _ = writeln!(
                std::io::stderr(),
                "SKIPPED: a_rise_past_either_cap_is_reported_once: cannot reserve {size} bytes"
            );
            return;
        }
        std::hint::black_box(&reserved);
        drop(reserved);
        let seen = probe.after_input(Duration::ZERO, true);
        let crossed = seen.reserved.expect("512 MiB reserved is past 256 MiB");
        assert!(crossed.peak - crossed.baseline > crossed.cap);
        assert_eq!(seen.written, None, "nothing was written");
        assert_eq!(probe.after_input(Duration::ZERO, true).reserved, None);
    }

    /// A status file of its own, which a test rewrites between reads.
    fn fake_status(test: &str) -> std::path::PathBuf {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/core-unit-tmp")
            .join(format!("{}-{test}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("status")
    }

    fn status(peak_kib: u64, size_kib: u64) -> String {
        format!("VmPeak:\t{peak_kib} kB\nVmSize:\t{size_kib} kB\nVmHWM:\t100 kB\n")
    }

    #[test]
    fn the_reserved_cap_counts_from_the_address_space_in_use() {
        let path = fake_status("reserved-cap");
        // The peak 64 MiB above what is in use, as after glibc's arena
        // trim: the cap counts from `VmSize`, not from the peak.
        std::fs::write(&path, status(800 * 1024 + 64 * 1024, 800 * 1024)).unwrap();
        let file = path.to_str().unwrap();
        let mut probe = Probe::start_with(file, u64::MAX, Some(256 << 20)).unwrap();
        // Exactly the cap above what was in use: not past it.
        std::fs::write(&path, status(800 * 1024 + 256 * 1024, 800 * 1024)).unwrap();
        assert_eq!(probe.after_input(Duration::ZERO, true).reserved, None);
        // A KiB more is.
        std::fs::write(&path, status(800 * 1024 + 256 * 1024 + 1, 800 * 1024)).unwrap();
        let crossed = probe
            .after_input(Duration::ZERO, true)
            .reserved
            .expect("past VmSize plus the cap");
        assert_eq!(crossed.baseline, 800 << 20);
        assert_eq!(crossed.in_use, 800 << 20);
        assert_eq!(crossed.peak - crossed.baseline, (256 << 20) + 1024);

        // A peak already past `VmSize` plus the cap at the start, from what
        // ran before: a finding only once it rises further.
        std::fs::write(&path, status(2000 * 1024, 800 * 1024)).unwrap();
        let mut probe = Probe::start_with(file, u64::MAX, Some(256 << 20)).unwrap();
        assert_eq!(probe.after_input(Duration::ZERO, true).reserved, None);
        std::fs::write(&path, status(2000 * 1024 + 1, 800 * 1024)).unwrap();
        let crossed = probe
            .after_input(Duration::ZERO, true)
            .reserved
            .expect("past the peak at the start");
        // The cap counts from the peak then less the cap; what was in use
        // is kept for the message.
        assert_eq!(crossed.baseline + crossed.cap, 2000 << 20);
        assert_eq!(crossed.in_use, 800 << 20);

        // Without a `VmSize` line the reserved check is skipped, and says so.
        std::fs::write(&path, "VmPeak:\t1 kB\nVmHWM:\t1 kB\n").unwrap();
        let probe = Probe::start_with(file, u64::MAX, Some(1)).unwrap();
        let (_, reserved) = probe.finish();
        assert!(
            matches!(&reserved, Some(Memory::Skipped(skip)) if skip.code == "no-vmpeak"),
            "{reserved:?}"
        );
        let _ = std::fs::remove_file(&path);
        if let Some(dir) = path.parent() {
            let _ = std::fs::remove_dir(dir);
        }
    }

    #[test]
    fn a_reserved_check_not_asked_for_is_not_there() {
        let Ok(probe) = Probe::start(u64::MAX, None) else {
            assert!(File::open(STATUS).is_err());
            return;
        };
        let (written, reserved) = probe.finish();
        assert!(matches!(written, Memory::Measured { .. }));
        assert_eq!(reserved, None);
    }
}
