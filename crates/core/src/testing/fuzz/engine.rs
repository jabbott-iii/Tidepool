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

//! Runs one fuzz target: feeds it its unchanged inputs and then mutated
//! seeds, catches panics, notices hangs, checks the memory, and saves the
//! inputs that caused any of these.
//!
//! Ported from Nadir's `xtask/src/fuzz/engine.rs`. Adapted for Tidepool:
//! it is part of `tidepool_core::testing::fuzz` (ADR-0027); the target's
//! thread has a 2 MiB stack whatever `RUST_MIN_STACK` says; the saved
//! crashes run first, then the seeds, each unchanged, and only the seeds
//! are mutated, so a new crash file does not change the inputs that follow;
//! each mutated input draws from its own random stream; the memory is
//! checked after each unchanged input and each input of a replay, and on a
//! time budget otherwise ([`memory`](super::memory)); an input that
//! returns after the hang limit is a hang too; a replay saves each input
//! before it runs; findings are written whole, through a hidden file, and
//! an unchanged input that fails is named where it is rather than saved
//! again; and the run says why it stopped.
//!
//! The target runs on a thread of its own. The calling thread is the
//! watchdog: a thread that is stuck inside a target cannot be stopped, but
//! it can be noticed, and the input it is stuck on can be saved.

use std::cell::{Cell, RefCell};
use std::fs::{self, File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::mem;
use std::panic;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, Once, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use super::memory::{Exceeded, Probe, Seen};
use super::mutate::mutate;
use super::paths;
use super::{
    Config, Finding, Found, FuzzError, Kind, LAST_INPUT, Memory, STACK_SIZE, Skip, Slowest, Stop,
};
use crate::hash::fnv1a64;
use crate::rng::Pcg32;

/// Everything one run needs, owned, so that a thread stuck in the target
/// can keep it after the watchdog has given up on it.
pub(super) struct Plan {
    /// The target's name, which also picks its random streams.
    pub(super) name: String,
    pub(super) run: fn(&[u8]),
    /// The repository root, under which `fuzz/crashes/<name>/` is.
    pub(super) root: PathBuf,
    /// The saved crashes, in name order: they run first, unchanged.
    pub(super) crashes: Vec<(PathBuf, Vec<u8>)>,
    /// The seeds, in order: they run next, unchanged, and are what every
    /// mutated input starts from.
    pub(super) seeds: Vec<(PathBuf, Vec<u8>)>,
    pub(super) dictionary: &'static [&'static [u8]],
    /// What a replay that did not finish left in `.last-input`: it runs
    /// first, and is saved like a mutated input if it fails.
    pub(super) leftover: Option<Vec<u8>>,
    /// How far the written peak may rise above where it started, in bytes.
    pub(super) memory_cap: u64,
    /// How far the reserved peak may rise, in bytes.
    pub(super) reserved_cap: u64,
    pub(super) config: Config,
}

/// What a run did.
#[derive(Debug)]
pub(super) struct Ran {
    pub(super) inputs: u64,
    pub(super) elapsed: Duration,
    pub(super) findings: Vec<Finding>,
    pub(super) stopped: Stop,
    pub(super) slowest: Option<Slowest>,
    pub(super) memory: Memory,
    pub(super) reserved: Memory,
    /// How many findings were saved to files.
    pub(super) saved: usize,
}

/// What the fuzzing thread and the watchdog share.
struct Shared {
    /// The input the target is working on, valid while `started` is set.
    input: Vec<u8>,
    /// Where `input` came from, if it is an unchanged one.
    origin: Option<PathBuf>,
    /// Its number among the mutated inputs, if it is one.
    number: Option<u64>,
    /// When the target was handed `input`, until it returns.
    started: Option<Instant>,
    /// Whether a hang has been recorded, by the watchdog or the fuzzing
    /// thread: one of them, once.
    hang_claimed: bool,
    inputs: u64,
    findings: Vec<Finding>,
    slowest: Option<Slowest>,
    saved: usize,
    /// The memory checks as last read.
    memory: Memory,
    reserved: Memory,
    /// Why the run stopped, once it has.
    stopped: Option<Stop>,
    /// The replay's own file and the directories it made, to remove when
    /// the run ends without the process dying.
    replay: Option<(PathBuf, Vec<PathBuf>)>,
}

/// How many threads of this process are stuck in a target that hung. While
/// one is, it can still allocate, so later runs do not check the memory:
/// what they saw might be its doing.
static HUNG: AtomicUsize = AtomicUsize::new(0);

/// Fuzzes `plan.run` as `plan.config` says: every crash and every seed
/// unchanged, in order, then mutated seeds until the time or the input
/// count runs out, or an input hangs.
pub(super) fn run(plan: Plan) -> Result<Ran, FuzzError> {
    let plan = Arc::new(plan);
    let shared = Arc::new(Mutex::new(Shared {
        input: Vec::new(),
        origin: None,
        number: None,
        started: None,
        hang_claimed: false,
        inputs: 0,
        findings: Vec::new(),
        slowest: None,
        saved: 0,
        memory: Memory::Skipped(not_started()),
        reserved: Memory::Skipped(not_started()),
        stopped: None,
        replay: None,
    }));
    let stop = Arc::new(AtomicBool::new(false));
    let (done, finished) = mpsc::channel();
    let start = Instant::now();
    {
        let (plan, shared, stop) = (Arc::clone(&plan), Arc::clone(&shared), Arc::clone(&stop));
        let spawned = thread::Builder::new()
            .name(format!("fuzz-{}", plan.name))
            .stack_size(STACK_SIZE)
            .spawn(move || {
                // Nobody is listening only if the watchdog gave up on this
                // thread, and then there is nothing left to report.
                let _ = done.send(fuzz(&plan, &shared, &stop));
            });
        if let Err(error) = spawned {
            return Err(FuzzError::Thread(format!(
                "cannot start the thread that runs the target: {error}"
            )));
        }
    }

    let hang_limit = plan.config.hang_limit;
    let poll = (hang_limit / 4).clamp(Duration::from_millis(1), Duration::from_millis(50));
    let mut result = Ok(());
    loop {
        match finished.recv_timeout(poll) {
            Ok(outcome) => {
                result = outcome;
                break;
            }
            Err(RecvTimeoutError::Disconnected) => {
                result = Err(FuzzError::Thread(
                    "the thread that runs the target stopped without a result".to_string(),
                ));
                break;
            }
            Err(RecvTimeoutError::Timeout) => {
                let mut guard = lock(&shared);
                let overdue = guard
                    .started
                    .is_some_and(|started| started.elapsed() > hang_limit);
                if overdue && !guard.hang_claimed {
                    guard.hang_claimed = true;
                    guard.stopped = Some(Stop::Hang);
                    // Its thread stays busy until the target returns, which
                    // it may never do.
                    HUNG.fetch_add(1, Ordering::SeqCst);
                    let (input, origin, number) =
                        (guard.input.clone(), guard.origin.clone(), guard.number);
                    drop(guard);
                    // This cannot interrupt the target. It only keeps the
                    // thread from going on if the target does return.
                    stop.store(true, Ordering::Relaxed);
                    let message = format!("did not return within {}", seconds(hang_limit));
                    let recorded = record(&plan, Kind::Hang, None, message, &input, origin, number);
                    let mut guard = lock(&shared);
                    match recorded {
                        Ok((finding, saved)) => {
                            guard.saved += usize::from(saved);
                            guard.findings.push(finding);
                        }
                        Err(error) => result = Err(error),
                    }
                    break;
                }
            }
        }
    }

    let mut guard = lock(&shared);
    if let Some((file, made)) = guard.replay.take() {
        // The run is over, and the process did not die on its last input:
        // the replay's own file has nothing left to point at. Its
        // directories go too, if nothing else is in them.
        let _ = fs::remove_file(file);
        for dir in made.iter().rev() {
            if fs::remove_dir(dir).is_err() {
                break;
            }
        }
    }
    result?;
    Ok(Ran {
        inputs: guard.inputs,
        elapsed: start.elapsed(),
        findings: mem::take(&mut guard.findings),
        stopped: guard.stopped.unwrap_or(Stop::Unchanged),
        slowest: guard.slowest,
        memory: mem::replace(&mut guard.memory, Memory::Skipped(not_started())),
        reserved: mem::replace(&mut guard.reserved, Memory::Skipped(not_started())),
        saved: guard.saved,
    })
}

/// What the memory checks are before the run's thread has started them.
fn not_started() -> Skip {
    Skip {
        code: "not-started",
        reason: "the run ended before the check started".to_string(),
    }
}

/// Why the reserved check does not run when it was not asked for.
fn not_asked() -> Skip {
    Skip {
        code: "not-asked",
        reason: "reserved memory is checked only when TIDEPOOL_FUZZ_SECS is set, even to 0: in \
                 the runs fuzz-smoke starts, and in the reproduce command of a saved \
                 reserved-memory crash, each one target to a process"
            .to_string(),
    }
}

/// Why neither memory check runs while a thread of this process is stuck
/// in a target that hung.
fn hung_thread() -> Skip {
    Skip {
        code: "hung-thread",
        reason: "a thread of this process is still stuck in a target that hung, and what it \
                 allocates would be blamed on this run"
            .to_string(),
    }
}

/// The body of the fuzzing thread. Returns once every input has run, or
/// the watchdog has said to stop.
fn fuzz(plan: &Plan, shared: &Mutex<Shared>, stop: &AtomicBool) -> Result<(), FuzzError> {
    install_panic_hook();
    // The thread's first allocation, which makes glibc reserve an arena
    // for it, is made before the reserved peak's baseline is read.
    std::hint::black_box(Box::new(0_u64));
    let reserved_cap = plan.config.check_reserved.then_some(plan.reserved_cap);
    let started_probe = if HUNG.load(Ordering::SeqCst) > 0 {
        Err(hung_thread())
    } else {
        Probe::start(plan.memory_cap, reserved_cap)
    };
    let mut probe = match started_probe {
        Ok(probe) => Some(probe),
        Err(skip) => {
            let mut guard = lock(shared);
            guard.memory = Memory::Skipped(skip.clone());
            guard.reserved = Memory::Skipped(if reserved_cap.is_some() {
                skip
            } else {
                not_asked()
            });
            None
        }
    };
    publish(shared, probe.as_ref(), reserved_cap.is_some());
    let mut replay = if plan.config.replay {
        let replay = ReplayFile::create(plan)?;
        lock(shared).replay = Some((replay.path.clone(), replay.made.clone()));
        Some(replay)
    } else {
        None
    };

    // What a replay that did not finish left runs first. It is saved to
    // `.last-input` again before it runs, so dying on it leaves it there.
    let leftover = plan.leftover.iter().map(|bytes| (None, bytes));
    let unchanged = plan
        .crashes
        .iter()
        .chain(&plan.seeds)
        .map(|(path, bytes)| (Some(path.as_path()), bytes));
    for (origin, bytes) in leftover.chain(unchanged) {
        if stop.load(Ordering::Relaxed) || lock(shared).stopped.is_some() {
            break;
        }
        if let Some(replay) = replay.as_mut() {
            replay.save(bytes)?;
        }
        let step = Step {
            input: bytes,
            origin,
            number: None,
            every: true,
            since: 0,
        };
        let (_, read) = run_one(plan, shared, probe.as_mut(), &step)?;
        if read {
            publish(shared, probe.as_ref(), reserved_cap.is_some());
        }
    }

    let corpus: Vec<Vec<u8>> = plan.seeds.iter().map(|(_, bytes)| bytes.clone()).collect();
    // Each target has its own streams, so that adding a target does not
    // change what the others are given.
    let streams = plan.config.seed ^ fnv1a64(plan.name.as_bytes());
    let started = Instant::now();
    let mut in_target = Duration::ZERO;
    let mut input = Vec::new();
    let mut mutated = 0_u64;
    // The first mutated input that no read has followed yet.
    let mut since = 0_u64;
    let stopped = loop {
        if let Some(reason) = lock(shared).stopped {
            break reason;
        }
        if stop.load(Ordering::Relaxed) {
            break Stop::Hang;
        }
        if plan.config.duration.is_zero() {
            break Stop::Unchanged;
        }
        if mutated >= plan.config.max_inputs {
            break Stop::MaxInputs;
        }
        if let Some(reason) = time_is_up(&plan.config, started.elapsed(), in_target) {
            break reason;
        }
        // Input `mutated` depends on the seed, the seeds and its number
        // alone: never on the time, nor on the inputs before it.
        let mut rng = Pcg32::new(streams, mutated);
        let Some(base) = rng.choose(&corpus) else {
            break Stop::Unchanged;
        };
        input.clear();
        input.extend_from_slice(base);
        mutate(
            &mut rng,
            &mut input,
            &corpus,
            plan.dictionary,
            plan.config.max_len,
        );
        if let Some(replay) = replay.as_mut() {
            replay.save(&input)?;
        }
        let step = Step {
            input: &input,
            origin: None,
            number: Some(mutated),
            every: plan.config.replay,
            since,
        };
        let (took, read) = run_one(plan, shared, probe.as_mut(), &step)?;
        mutated += 1;
        in_target = in_target.saturating_add(took);
        if read {
            since = mutated;
            publish(shared, probe.as_ref(), reserved_cap.is_some());
        }
    };

    if lock(shared).stopped == Some(Stop::Hang) {
        // The watchdog has reported the run; nothing after it is anyone's.
        return Ok(());
    }
    if let Some(probe) = probe.as_mut() {
        // A last read, so that a rise since the last one is not missed.
        let seen = probe.after_input(Duration::ZERO, true);
        if mutated > since {
            report_memory(shared, &seen, since, mutated - 1);
        }
    }
    let mut guard = lock(shared);
    guard.stopped.get_or_insert(stopped);
    if let Some(probe) = probe {
        let (memory, reserved) = probe.finish();
        guard.memory = memory;
        guard.reserved = reserved.unwrap_or_else(|| Memory::Skipped(not_asked()));
    }
    Ok(())
}

/// Copies what the probe has read so far to `shared`, for a run that ends
/// at a hang, whose thread cannot hand it over.
fn publish(shared: &Mutex<Shared>, probe: Option<&Probe>, reserved_asked: bool) {
    let Some(probe) = probe else {
        return;
    };
    let (memory, reserved) = probe.snapshot();
    let mut guard = lock(shared);
    guard.memory = memory;
    guard.reserved = match reserved {
        Some(reserved) => reserved,
        None if reserved_asked => Memory::Skipped(not_started()),
        None => Memory::Skipped(not_asked()),
    };
}

/// One input to run, and how to check it.
struct Step<'a> {
    input: &'a [u8],
    /// Where it was read from, if it is unchanged.
    origin: Option<&'a Path>,
    /// Its number among the mutated inputs, if it is one.
    number: Option<u64>,
    /// Read the memory after it, whatever the budget says.
    every: bool,
    /// The first mutated input since the last read, for a reserved rise
    /// seen on the budget.
    since: u64,
}

/// Runs one input and records what it did. Returns how long the target
/// took, and whether the memory was read after it.
fn run_one(
    plan: &Plan,
    shared: &Mutex<Shared>,
    probe: Option<&mut Probe>,
    step: &Step<'_>,
) -> Result<(Duration, bool), FuzzError> {
    let now = Instant::now();
    {
        let mut guard = lock(shared);
        guard.input.clear();
        guard.input.extend_from_slice(step.input);
        guard.origin = step.origin.map(Path::to_path_buf);
        guard.number = step.number;
        guard.started = Some(now);
    }
    let panicked = run_target(plan.run, step.input);
    let took = now.elapsed();
    {
        // One critical section decides whose input this is: the watchdog
        // claims only an input that is still `started`, so once that is
        // cleared here it cannot claim this one, and if it already has,
        // this sees it.
        let mut guard = lock(shared);
        if guard.stopped == Some(Stop::Hang) {
            // The watchdog gave up on this input and has reported the
            // run: record nothing, save nothing, read nothing. The thread
            // is no longer stuck.
            HUNG.fetch_sub(1, Ordering::SeqCst);
            return Ok((took, false));
        }
        guard.started = None;
    }
    let seen = probe.map_or_else(Seen::default, |probe| probe.after_input(took, step.every));

    let mut found = Vec::new();
    {
        let mut guard = lock(shared);
        guard.inputs += 1;
        if guard.slowest.is_none_or(|slowest| took > slowest.took) {
            guard.slowest = Some(Slowest {
                took,
                len: step.input.len(),
            });
        }
        // An input that returned, but later than the limit, is a hang
        // too, whenever the watchdog happened to look. Whichever of the two
        // claims it first records it.
        if took > plan.config.hang_limit && !guard.hang_claimed {
            guard.hang_claimed = true;
            guard.stopped = Some(Stop::SlowInput);
            let message = format!(
                "returned after {}, past the limit of {}",
                seconds(took),
                seconds(plan.config.hang_limit)
            );
            found.push(Pending::new(Kind::Hang, message));
        }
        if let Some(panicked) = panicked {
            let known = guard.findings.iter_mut().find(|finding| {
                finding.kind == Kind::Panic && finding.site.as_ref() == Some(&panicked.site)
            });
            match known {
                Some(finding) => finding.hits += 1,
                None => found.push(Pending {
                    site: Some(panicked.site),
                    earlier: panicked.earlier,
                    ..Pending::new(Kind::Panic, panicked.message)
                }),
            }
        }
    }
    for pending in found {
        save(plan, shared, pending, step)?;
    }
    if seen.read {
        // Every read after an unchanged input or a replay's names the input
        // exactly; a read on the budget, for reserved memory, a range.
        let exact = step.every;
        if let Some(exceeded) = seen.written {
            let pending = Pending::new(Kind::Memory, written_message(&exceeded));
            save(plan, shared, pending, step)?;
        }
        if let Some(exceeded) = seen.reserved {
            let message = reserved_message(&exceeded);
            match (exact, step.number) {
                (false, Some(number)) => {
                    range(shared, message, step.since, number);
                }
                _ => save(plan, shared, Pending::new(Kind::Reserved, message), step)?,
            }
        }
    }
    Ok((took, seen.read))
}

/// Records the memory findings of the last read of a run, which came after
/// the mutated inputs `since..=last`. Within the budget the written peak
/// cannot have passed its cap since the read before, so a written finding
/// here means the bound did not hold, or another thread did it: it names
/// the range, as a reserved one does.
fn report_memory(shared: &Mutex<Shared>, seen: &Seen, since: u64, last: u64) {
    if let Some(exceeded) = seen.written {
        range_of(
            shared,
            Kind::Memory,
            written_message(&exceeded),
            since,
            last,
        );
    }
    if let Some(exceeded) = seen.reserved {
        range(shared, reserved_message(&exceeded), since, last);
    }
}

/// Records a reserved-memory finding for one of the mutated inputs
/// `first..=last`, which were not saved.
fn range(shared: &Mutex<Shared>, message: String, first: u64, last: u64) {
    range_of(shared, Kind::Reserved, message, first, last);
}

fn range_of(shared: &Mutex<Shared>, kind: Kind, message: String, first: u64, last: u64) {
    lock(shared).findings.push(Finding {
        kind,
        site: None,
        earlier: None,
        message,
        input: Found::Range { first, last },
        hits: 1,
    });
}

/// A finding about to be recorded: what went wrong, where, and any earlier
/// panic of the same input.
struct Pending {
    kind: Kind,
    site: Option<String>,
    message: String,
    earlier: Option<String>,
}

impl Pending {
    fn new(kind: Kind, message: String) -> Self {
        Self {
            kind,
            site: None,
            message,
            earlier: None,
        }
    }
}

/// Records a finding for the input of `step`, saving it unless it is an
/// unchanged one, which is named where it is.
fn save(
    plan: &Plan,
    shared: &Mutex<Shared>,
    pending: Pending,
    step: &Step<'_>,
) -> Result<(), FuzzError> {
    let origin = step.origin.map(Path::to_path_buf);
    let Pending {
        kind,
        site,
        message,
        earlier,
    } = pending;
    let (mut finding, saved) = record(plan, kind, site, message, step.input, origin, step.number)?;
    finding.earlier = earlier;
    let mut guard = lock(shared);
    guard.saved += usize::from(saved);
    guard.findings.push(finding);
    Ok(())
}

fn written_message(exceeded: &Exceeded) -> String {
    format!(
        "the written peak (VmHWM) rose to {}, {} above the {} at the start, past the cap of {}",
        mib(exceeded.peak),
        mib(exceeded.peak - exceeded.baseline),
        mib(exceeded.baseline),
        mib(exceeded.cap)
    )
}

pub(super) fn reserved_message(exceeded: &Exceeded) -> String {
    if exceeded.baseline == exceeded.in_use {
        return format!(
            "the reserved peak (VmPeak) rose to {}, {} above the {} of address space in use \
             (VmSize) at the start, past the cap of {}",
            mib(exceeded.peak),
            mib(exceeded.peak - exceeded.baseline),
            mib(exceeded.in_use),
            mib(exceeded.cap)
        );
    }
    // The peak at the start was already past the cap above what was in
    // use, so the cap counted from the peak then less the cap.
    format!(
        "the reserved peak (VmPeak) rose to {}, past the {} it had reached at the start, which \
         was already more than the cap of {} above the {} of address space in use (VmSize) then",
        mib(exceeded.peak),
        mib(exceeded.baseline + exceeded.cap),
        mib(exceeded.cap),
        mib(exceeded.in_use)
    )
}

/// How many times its `duration` a replay may take in all.
pub(super) const REPLAY_WALL_FACTOR: u32 = 20;

/// Why a run that has made mutated inputs for `elapsed`, of which the
/// target took `in_target`, should stop, if it should.
///
/// A normal run stops when `elapsed` reaches `config.duration`. A replay
/// saves each input and reads the memory after it, about 8 µs an input on
/// top of the target's own time, measured with a target that does nothing.
/// So it counts only `in_target`, which is less than the whole time of the
/// run it replays: it usually gets at least as far as that run did, but
/// not always, since the target's own time varies from run to run. It also
/// stops at [`REPLAY_WALL_FACTOR`] times the duration in all, so that a
/// target that takes no time does not make it go on for hours. A target
/// whose normal run takes less than about 0.5 µs an input can reach that
/// cap before the point it replays; more seconds then help.
pub(super) fn time_is_up(config: &Config, elapsed: Duration, in_target: Duration) -> Option<Stop> {
    if !config.replay {
        return (elapsed >= config.duration).then_some(Stop::Time);
    }
    if in_target >= config.duration {
        Some(Stop::TargetTime)
    } else if elapsed >= config.duration.saturating_mul(REPLAY_WALL_FACTOR) {
        Some(Stop::WallCap)
    } else {
        None
    }
}

/// A panic that a target raised.
struct Panicked {
    /// Where, as `file:line:column`; the message if the place is not known.
    site: String,
    message: String,
    /// Another panic during the same input, which the target caught: its
    /// place, and its message if it had one.
    earlier: Option<String>,
}

/// A panic's place, `file:line:column` (empty if not known), and its
/// message when it is text.
type Raised = (String, Option<String>);

thread_local! {
    /// Whether this thread is inside a fuzz target right now.
    static IN_TARGET: Cell<bool> = const { Cell::new(false) };
    /// The latest panic inside the current target call on this thread.
    static PANIC_SITE: RefCell<Option<Raised>> = const { RefCell::new(None) };
    /// The first panic of the current target call, once a later one has
    /// taken its place in [`PANIC_SITE`].
    static EARLIER: RefCell<Option<Raised>> = const { RefCell::new(None) };
}

/// Makes panics inside a fuzz target quiet, and records where they were
/// raised. A fuzz run can hit the same panic many thousands of times; the
/// standard message for each one would bury everything else.
///
/// Panics anywhere else, on any thread, go to the hook that was installed
/// before, as they did: libtest's report of a failing test is unchanged.
///
/// So does every panic after the first during one target call. The latest
/// one is recorded as the one that escapes, since a target may catch a
/// panic inside and then raise another; the first is kept beside it, for
/// the report. When a `drop` raises and catches a panic of its own while
/// the first unwinds, the first is the one that escapes: [`run_target`]
/// tells the two apart by the escaping payload's message. A second panic
/// raised while the first unwinds that nothing catches aborts the process:
/// then only that second panic is printed, by the hook before this one,
/// and the first, which was quiet, is not. This crate never prints.
fn install_panic_hook() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if !IN_TARGET.get() {
                previous(info);
                return;
            }
            let site = info.location().map_or_else(String::new, |location| {
                format!(
                    "{}:{}:{}",
                    location.file(),
                    location.line(),
                    location.column()
                )
            });
            let raised = (site, info.payload_as_str().map(str::to_string));
            if PANIC_SITE.with_borrow(Option::is_none) {
                PANIC_SITE.set(Some(raised));
            } else {
                if EARLIER.with_borrow(Option::is_none) {
                    EARLIER.set(PANIC_SITE.take());
                }
                PANIC_SITE.set(Some(raised));
                previous(info);
            }
        }));
    });
}

/// Hands `input` to `target`. Returns the panic, if it panicked.
fn run_target(target: fn(&[u8]), input: &[u8]) -> Option<Panicked> {
    PANIC_SITE.set(None);
    EARLIER.set(None);
    IN_TARGET.set(true);
    let result = panic::catch_unwind(|| target(input));
    IN_TARGET.set(false);
    let (latest, first) = (PANIC_SITE.take(), EARLIER.take());
    let payload = result.err()?;
    let message = crate::testing::panic_message(&*payload).to_string();
    // The panic that escapes is the latest, unless a `drop` raised and
    // caught one of its own while the first unwound: then the first
    // escapes, and its message is the payload's.
    let escaped = |raised: &Option<Raised>| {
        raised
            .as_ref()
            .is_some_and(|(_, text)| text.as_deref() == Some(message.as_str()))
    };
    let (latest, caught) = if !escaped(&latest) && escaped(&first) {
        (first, latest)
    } else {
        (latest, first)
    };
    let site = latest.map(|(site, _)| site).filter(|site| !site.is_empty());
    let caught = caught.map(|(site, text)| match text {
        Some(text) => format!("{site}: {text}"),
        None => site,
    });
    Some(Panicked {
        site: site.unwrap_or_else(|| message.clone()),
        message,
        earlier: caught,
    })
}

/// Describes a failure, saving `input` first unless it is an unchanged
/// input, which is named where it already is (`origin`). Returns the
/// finding and whether a file was saved.
///
/// The file, in `fuzz/crashes/<target>/`, is named after the kind of
/// failure and a hash of the input, so saving the same input twice gives
/// one file. It is written whole, through a hidden file.
fn record(
    plan: &Plan,
    kind: Kind,
    site: Option<String>,
    message: String,
    input: &[u8],
    origin: Option<PathBuf>,
    number: Option<u64>,
) -> Result<(Finding, bool), FuzzError> {
    let (path, saved) = match origin {
        Some(origin) => (origin, false),
        None => {
            let relative = paths::crash_dir(&plan.name);
            paths::make_dirs(&plan.root, &relative)?;
            let name = format!("{}-{:016x}", kind.file_prefix(), fnv1a64(input));
            (
                paths::write_whole(&plan.root.join(relative), &name, input)?,
                true,
            )
        }
    };
    let finding = Finding {
        kind,
        site,
        earlier: None,
        message,
        input: Found::File {
            path,
            len: input.len(),
            number,
        },
        hits: 1,
    };
    Ok((finding, saved))
}

/// The file a replay writes each input to before running it, so that if
/// the process dies on an input, the input is on disk: the hidden
/// [`LAST_INPUT`], which no run reads as a crash until it is kept as
/// `died-<hash>` ([`paths::keep_died`]).
struct ReplayFile {
    path: PathBuf,
    file: File,
    /// The directories made for it, shallowest first.
    made: Vec<PathBuf>,
    /// The file's length.
    len: u64,
}

impl ReplayFile {
    fn create(plan: &Plan) -> Result<Self, FuzzError> {
        let relative = paths::crash_dir(&plan.name);
        let made = paths::make_dirs(&plan.root, &relative)?;
        let dir = plan.root.join(relative);
        // A leftover one was read before the run started, and is the
        // first input. It is written whole under a hidden name and renamed
        // over the old file, which so stays until its bytes are on disk
        // again: a run that fails to start, or dies before its first
        // input, loses nothing. The rename replaces a link at the name
        // without following it.
        if let Some(bytes) = plan.leftover.as_deref() {
            let (path, file) = paths::write_whole_open(&dir, LAST_INPUT, bytes)?;
            return Ok(Self {
                path,
                file,
                made,
                len: bytes.len() as u64,
            });
        }
        let path = dir.join(LAST_INPUT);
        // `create_new` refuses anything already at the name, a link above
        // all.
        let opened = OpenOptions::new().write(true).create_new(true).open(&path);
        match opened {
            Ok(file) => Ok(Self {
                path,
                file,
                made,
                len: 0,
            }),
            Err(source) => Err(FuzzError::Write { path, source }),
        }
    }

    /// Replaces what the file holds with `input`. The kernel has the bytes
    /// once this returns, so they outlive this process, though not a crash
    /// of the machine; nothing is synced to the disk.
    ///
    /// The file is cut only when `input` is shorter than what it holds: a
    /// write from the start that is as long or longer replaces all of it,
    /// and cutting a file costs several times what writing a small one does.
    fn save(&mut self, input: &[u8]) -> Result<(), FuzzError> {
        let len = input.len() as u64;
        let mut written = self
            .file
            .seek(SeekFrom::Start(0))
            .and_then(|_| self.file.write_all(input));
        if written.is_ok() && len < self.len {
            written = self.file.set_len(len);
        }
        self.len = len;
        written.map_err(|source| FuzzError::Write {
            path: self.path.clone(),
            source,
        })
    }
}

/// `5 s`, `0.04 s`.
pub(super) fn seconds(duration: Duration) -> String {
    format!("{} s", duration.as_secs_f64())
}

/// Bytes in MiB, to one decimal place.
pub(super) fn mib(bytes: u64) -> String {
    format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
}

/// Locks `mutex`. A lock is only ever poisoned by a bug in this file, and
/// the numbers behind it are still the best there are.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The panic hook and the call that catches a target's panic, for the
/// tests of [`super`].
#[cfg(test)]
pub(super) mod tests_support {
    /// Installs the hook ([`super::install_panic_hook`]).
    pub(in crate::testing::fuzz) fn install_panic_hook() {
        super::install_panic_hook();
    }

    /// Runs `target` as the engine does; returns the message and the site
    /// of its panic, if it panicked.
    pub(in crate::testing::fuzz) fn run_target(
        target: fn(&[u8]),
        input: &[u8],
    ) -> Option<(String, String)> {
        super::run_target(target, input).map(|panicked| (panicked.message, panicked.site))
    }

    /// Whether this thread is inside a target now.
    pub(in crate::testing::fuzz) fn in_target() -> bool {
        super::IN_TARGET.get()
    }

    /// Waits, up to 10 s, until no thread of this process is stuck in a
    /// target that hung. Returns whether none is.
    pub(in crate::testing::fuzz) fn no_hung_thread() -> bool {
        let start = std::time::Instant::now();
        while super::HUNG.load(std::sync::atomic::Ordering::SeqCst) > 0 {
            if start.elapsed() > std::time::Duration::from_secs(10) {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        true
    }
}
