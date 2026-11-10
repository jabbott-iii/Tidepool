# 04 — Quality

## Test layers

| Layer | Where | What it proves |
| :- | :- | :- |
| Unit | `#[cfg(test)] mod tests` beside the code | Functions and types behave as documented |
| Property | Unit tests using `tidepool_core::testing` | Invariants hold for generated inputs (rope vs `String` model, position round-trips, undo/redo) |
| Integration | `crates/<name>/tests/` | A crate's public API works end to end |
| Golden frames | `tests/golden/*.qoi`, compared by `cargo xtask golden` | `gfx`, `font` and `ui` render what they rendered when blessed |
| TUI snapshots | `tests/golden/tui/*.txt` | The TUI's cell grid (text + style codes) for a scripted session |
| Protocol transcripts | `crates/testkit/transcripts/` | LSP, DAP and TAP clients send and accept exactly the recorded messages |
| Fake tools | `crates/testkit/fake-tools/` | `toolchain`, `vcs` and `mobile` handle real output formats and exit codes without the real tools |
| Real-tool | Gated by `TIDEPOOL_IT_RA`, `TIDEPOOL_IT_GIT`, `TIDEPOOL_IT_ANDROID`, `TIDEPOOL_IT_IOS` | Integration with the actual installed tool |
| Fuzz smoke | Each parser crate's `tests/fuzz.rs`; mutated by `cargo xtask fuzz-smoke` | No parser panics, hangs or over-allocates on its seeds, its saved crashes or mutated input |
| Responsiveness | `bench` latency suite | Keystroke-to-frame stays within budget with slow fakes attached |
| Vim conformance | `crates/editor/tests/vim/` | Each supported command in `docs/subsystems/vim-mode.md` behaves as in Vim on a fixture |

A gated test that can't run prints `SKIPPED: <reason>` and is never counted
as a pass.

## Own harnesses

### Property testing — `tidepool_core::testing`

- `check(name, cases, |rng| ...)` runs generated cases from a seeded
  `Pcg32`; `check_sized` adds size-directed shrinking.
- A failure prints the seed and the minimal case:
  `TIDEPOOL_SEED=0x… cargo test -p tidepool-text rope_matches_model`.
- Port the harness design from Nadir's `nadir-core::testing` rather than
  inventing a new one.

### Golden frames

- Scenes are drawn offscreen by test binaries into a framebuffer at a fixed
  size and scale, then encoded as QOI.
- `cargo xtask golden` compares them to `tests/golden/` with a per-scene
  tolerance (default: exact for `gfx`, ≤ 1/255 per channel for `font`
  coverage). On mismatch it writes `actual` and `diff` images to
  `target/golden/`.
- `--bless` replaces references only for the scenes named, and only after
  the user has reviewed the diff images.
- Theme changes (`themes/tidepool-dark.toml`) must re-bless with review.

### Fakes

- **Fake LSP server** (`testkit`): replays a transcript, can delay, drop,
  reorder or corrupt messages, and crash on cue.
- **Mock agent** (`agents/tp-agent-mock`): deterministic TAP responses,
  latency injection, malformed output modes.
- **Fake tools**: tiny Rust binaries in `testkit` named like the real tools
  (`git`, `rustup`, `sdkmanager`, `apksigner`, `xcodebuild`...) that print
  recorded output and exit codes, selected by putting their directory first
  on the `PATH` the test hands to `toolchain`.

### Fuzz smoke (stable Rust, no dependencies)

The design is ADR-0027's; the engine's rustdoc
(`tidepool_core::testing::fuzz`) is authoritative, and this summarizes it.

- **Targets** are tests. Each crate with a parser has a dev-only
  integration test, `tests/fuzz.rs`, with one `#[test]` per target, named
  after it. It builds a `Target` (its name, a function that takes any
  bytes, seed directories, a dictionary of tokens, its memory caps) and
  runs it. Rejecting an input is fine; a panic, a hang or too much memory
  is a finding. A target may also check what its module promises beyond
  "no panic", and panic on a mismatch.
- **Registered** in three places: the test; its seeds in
  `fuzz/corpus/<target>/`; and its row in `fuzz-smoke`'s table (`TARGETS`
  in `xtask/src/fuzz.rs`: the target and its package).
  `.claude/rules/parsers.md` requires one for every parser. `fuzz-smoke`
  fails, running nothing, when `fuzz/`, `fuzz/corpus/` or `fuzz/crashes/`
  is a symbolic link or not a directory, when a directory in
  `fuzz/corpus/` or `fuzz/crashes/` has no row, or when a row's
  `fuzz/corpus/<target>/` is missing or holds no regular file that is not
  hidden. `fuzz/NOTICE.md` says where a saved input may come from.
- **Targets now** (M1.6). The parsers run with small `Limits` (4 KiB of
  input, depth 32, strings of 1 KiB, numbers of 64 bytes, 256 entries),
  and their corpora hold `edge-*` seeds that sit exactly at each limit, so
  that one mutation crosses it; the decoder of `text::encoding` takes no
  `Limits`. Each has a 16 MiB written-memory cap.

  | Target | Crate | Runs | Also checks | Seeds read in place |
  | :- | :- | :- | :- | :- |
  | `json` | `codec` | `json::parse_bytes`, both dialects; and for input that is not UTF-8, `json::parse` of its lossy decoding | an error's offset is in the input, at a character boundary, with its line and column, and an `InvalidUtf8` one where the UTF-8 stops; strict JSON reads the same as JSONC, and JSONC accepts more only for a comment or a trailing comma; the compact and pretty output read back equal | `crates/codec/tests/json/` (the RFC 8259 examples and the other hand-made vectors) |
  | `toml` | `codec` | `toml::parse_bytes`; for input that parses, `toml::Document::parse_bytes` and four edits, each on a copy; and for input that is not UTF-8, `toml::parse` of its lossy decoding | the same error checks; every span is a non-empty range of characters of the input, and holds what its kind says (quotes, a bare key's name, a boolean's word, `[`, `[[` or `{`); the document holds the input and the parser's tree; the edits (the leaf that starts last becomes a string that needs escapes; `fz = 7`, and a new table with a key in it, go into a table of any kind that an FNV-1a hash of the input picks, so that a replay picks it again; a new table with a key in it goes into the root) each change exactly the bytes they should and read back holding the value, or are refused for a limit (or, for a `fz` that is a table, as not replaceable) and change nothing; a refusal as unverified is a finding | `crates/codec/tests/toml-test/`, valid and invalid (ADR-0026) |
  | `encoding` | `text` | `encoding::Decoder`, fed in pieces that the input decides, and `decode` | the pieces decode as the whole does; the text and the detected format are what `std` decodes; the runs of replaced bytes keep their promises; `original_bytes()` is the input; a text decoded without a replacement encodes back to it | — |

  Later: XML, plist, YAML, CSV, INI, DEFLATE, ZIP, PNG, QOI, TrueType,
  Markdown, regex compile, VT parser, JSON-RPC framing, LSP messages, TAP
  messages, git porcelain v2, cargo JSON diagnostics, tool version
  strings, each with its parser's task.
- **Every `cargo test` replays.** A target's test runs, unchanged, every
  saved crash in `fuzz/crashes/<target>/` (in name order), then every seed
  (`fuzz/corpus/<target>/` in name order, then the data it names, in path
  order). So a saved crash is a regression test, and a fixed one keeps
  passing. Hidden files are skipped; a seed over 1 MiB, an empty seed
  directory, and anything that is neither a file nor (for data read in
  place) a directory fail the test. Every directory a target reads seeds
  from is `-text` in `.gitattributes`, so that every checkout gives the
  same bytes, and so the same inputs.
- **Mutation** happens only when `fuzz-smoke` asks, through
  `TIDEPOOL_FUZZ_SECS`. Each mutated input is a seed with 1 to 16 changes:
  bit flips, random bytes, runs inserted, deleted or duplicated, splices
  from other seeds, boundary integers, small steps on what may be a
  length, and the target's dictionary tokens. Inputs are at most 64 KiB.
  Input `n` depends on the seed, the target's name, the seeds and `n`
  alone: never on the time, which only decides when to stop, nor on the
  saved crashes. So a run can be replayed exactly.
- **The engine** runs the target on a thread of its own with a 2 MiB
  stack, whatever `RUST_MIN_STACK` says, as a parser has on a worker
  thread (`AGENTS.md` rule 6). The targets of one process run one at a
  time, until a hang, whose stuck thread runs on. The calling thread is a
  watchdog: an input that takes more than 5 s is a hang, and so is one
  that returns after 5 s, whenever the watchdog looked; a hang ends the
  run, since a stuck thread cannot be stopped. Panics inside the target
  are caught with the place they were raised, and panics from one place
  are one finding. When the target caught another panic during the same
  input, the finding is the one that escaped, and also names the caught
  one: an earlier one, or one that a `drop` raised and caught while the
  escaping one unwound (told apart by the escaping panic's message). A
  place in the standard library (`/rustc/…`), such as
  `Vec::with_capacity`'s "capacity overflow", may stand for several places
  in the target, merged into one finding; the report says so. The first
  panic of an input does not print the usual message; every later one,
  and any panic elsewhere, goes to the hook that was installed before. So
  when a panic aborts the process during unwinding (a `drop` that
  panics), only that later panic is printed. The outcome says why the
  run stopped (`stopped=` in the result line): `unchanged`,
  `time`, `target-time`, `wall-cap`, `max-inputs`, `hang` or
  `slow-input`.
- **Memory** (Linux only; elsewhere both checks are skipped, and the
  report says why). From one read of `/proc/self/status`:
  - **Written** (`VmHWM`, the peak resident set): an input that takes it
    more than the target's cap above where it was at the start is a
    finding. Reading costs about 3.4 µs, as much as an input, so mutated
    inputs are read after only once enough of the target's time has passed
    for the peak to have passed the cap, at a rate of 1 MiB a microsecond
    (300 times the rate measured): the input named is still the one that
    took it past, as long as that bound holds. Unchanged inputs and every
    input of a replay are read after one by one.
  - **Reserved** (`VmPeak`, the peak address space, which counts memory
    reserved and never written, such as `Vec::with_capacity(claimed)`):
    a peak more than 256 MiB above the address space in use (`VmSize`)
    once the target's thread has made its first allocation is a finding
    (or above the peak then, if that is higher; the finding says which,
    and the result line's `reserved_baseline` is the larger of `VmSize`
    then and the peak then less the cap). Counting from `VmSize` matters:
    glibc's arena trim leaves `VmPeak` 64 MiB above it, which would
    otherwise add 64 MiB to the cap. Threads move it by tens of MiB,
    so it is checked only when `TIDEPOOL_FUZZ_SECS` is set, even to 0: in
    the runs `fuzz-smoke` starts, and in the reproduce command of a saved
    reserved-memory crash, each one target to a process. A plain `cargo
    test` says it was not checked (`not-asked`). It grows at any rate, so a
    read on the budget names the range of inputs since the last read and
    saves none (`ranges=` in the result line); `fuzz-smoke` then prints
    the replay, which names and saves the one as `reserve-<hash>`. Such a
    saved crash is a regression test only where the check runs: its
    report's command sets `TIDEPOOL_FUZZ_SECS=0`.
  - A thread left stuck in a target that hung keeps running, and can
    allocate. While one is, later runs in the process check neither
    (`hung-thread`), and when the stuck input returns, nothing more is
    recorded for it.

  The measurements are in the module docs of
  `crates/core/src/testing/fuzz/memory.rs`. Limits: the checks see the
  whole process; the written one is exact to about 2 MiB and rests on the
  measured rate; a reservation smaller than its cap is not seen, so the
  `Limits` checks before allocating remain the guard; a read that fails
  partway keeps what was measured, and says so. An allocation too large to
  make kills the process instead; see the replay below.
- **Findings** are saved to `fuzz/crashes/<target>/<kind>-<hash>`: `panic`,
  `hang`, `memory` or `reserve`, and the input's FNV-1a hash in 16 hex
  digits, written whole through a hidden file and renamed into place. An
  unchanged input that fails is named where it is, not saved again. The
  test then fails with a report: each file (or range), its kind and the
  place, why the run stopped, both memory checks, and the command that
  reproduces it (`cargo test -p <package> --test fuzz -- --exact
  <target>`, which replays the saved crash first). `.gitattributes` keeps
  `fuzz/**` byte for byte.
- **A process that dies** (an allocation too big to make aborts, a stack
  overflow kills) cannot report, nor can a test binary that exits with
  libtest's failure status without a result. `fuzz-smoke` then prints the
  seed and the replay command: `cargo xtask fuzz-smoke --target <target>
  --seed <seed> --secs <N> --replay`. A replay writes each input to the
  hidden `fuzz/crashes/<target>/.last-input` before it runs, so when the
  process dies again, or exits again without a result, the input is
  there, and `fuzz-smoke` keeps it as `died-<hash>`, replacing a file of
  that name, which then holds the input's bytes whatever it held. It keeps
  it only if the replay wrote the file (its modification time changed
  while the replay ran), and nothing when a signal stopped the test (`SIGINT`,
  `SIGTERM`, `SIGHUP`). When the input is already a saved crash, a
  `died-<hash>` or another kind's file of the same hash and bytes, it
  says so, and makes no second copy. From then on the kept input runs
  with the saved crashes.
- **A `.last-input` left over**, by a replay that died or was
  interrupted, is read by the next replay and run first (saved as a
  mutated input is saved, should it fail). Before that first input the
  replay writes it whole under a hidden name and renames it over the old
  file, so it is on disk at every moment, and a replay that fails to
  start leaves it. `fuzz-smoke` notes it before the
  replay starts, with its size and hash and the `mv` command that keeps
  it as `died-<hash>` by hand. That matters when an out-of-memory kill
  took `fuzz-smoke` too, as systemd does to every process of the unit
  unless its `OOMPolicy` is `continue`: nothing was kept, and the
  leftover is likely the input it died on. For a death that may be an
  out-of-memory kill, run the replay as `systemd-run --user --scope -p
  OOMPolicy=continue cargo xtask fuzz-smoke … --replay`. That covers the
  kernel's out-of-memory killer only: `systemd-oomd`, where it runs,
  kills every process of the unit it picks whatever `OOMPolicy` says,
  and then the note's `mv` command keeps the input.
- **A replay's time.** Saving each input and reading the memory after it
  add about 8 µs an input on the development machine, measured with a
  target that does nothing (most of it cutting the file, about 3.6 µs
  when an input is shorter than the one before, and the read, about
  3.4 µs; writing is about 0.5 µs). So a replay's seconds count only the
  time spent inside the target, which the run it replays had less of: it
  usually gets as far, but the target's time varies, so not always. It
  also stops at 20 times its seconds in all, which a target whose normal
  run takes less than about 0.5 µs an input (2 million inputs a second)
  can reach first; more `--secs` then help. A replay that finds nothing
  says why it stopped, and that it may not have reached the input the run
  it replays failed on. It removes its own `.last-input`, and the
  directories it made if they are empty, and nothing else. Run one replay
  of a target at a time: two would share the file.
- **Paths.** Every directory on the way from the root to a target's seed
  and crash directories, and to each directory of seeds read in place, is
  looked at without following a link, and a link is refused; directories
  are made one level at a time, and files with `create_new`. Not covered:
  a directory swapped for a link while the run uses it.
- **Environment** (read by the tests; `fuzz-smoke` sets them):

  | Variable | Meaning | When not set |
  | :- | :- | :- |
  | `TIDEPOOL_FUZZ_SECS` | Seconds to make mutated inputs for; set at all, it also turns on the reserved-memory check | Only the unchanged inputs run |
  | `TIDEPOOL_FUZZ_SEED` | The seed of the mutations, a `u64` in decimal or `0x` hex, as `TIDEPOOL_SEED` | `TIDEPOOL_SEED`'s default |
  | `TIDEPOOL_FUZZ_REPLAY` | `1` to save each input before it runs, and read the memory after each | `0` |

- **The profile** `fuzz` in the root `Cargo.toml` inherits `release`, with
  `debug-assertions` and `overflow-checks` on (an overflow on a mutated
  input is then a panic the fuzzer sees), no LTO, 16 codegen units, line
  tables, no stripping, and unwinding, which catching a panic needs.
- **`cargo xtask fuzz-smoke [--secs N] [--target T] [--seed S]
  [--replay]`** runs, for each target in table order (or `T`), `cargo test
  --profile fuzz -p <package> --test fuzz -- --exact <target> --nocapture`
  with the variables above and `CARGO_TERM_COLOR=never`: 60 s each unless
  `--secs` says otherwise, and a seed from the clock, printed, unless
  `--seed` gives one. Each test prints one result line (`fuzz-result:
  target=… inputs=… …`), which `fuzz-smoke` reads wherever on its line it
  starts (with one test thread, libtest writes `test json ... ` first). It
  prints, for each target, the inputs, inputs a second, findings, both
  memory checks (or the code of why one was not checked) and the slowest
  input, then a summary table. It exits 1 if a target found something;
  if its result line shows another seed, or, outside a replay or a hang,
  fewer seconds than it was given; if its test failed, did not build or
  did not run; if cargo failed before the test ran; if the test binary
  exited with libtest's failure status without a result; or if the
  test's process died (an allocation too big to make, a stack overflow, a
  kill, or a call to exit). A test binary that cargo could not start (a
  missing runner, no permission to run it) is cargo's failure. For a
  death or an exit without a result outside a replay, it prints the
  replay command, and says that the saved crashes run first (a `died-*`
  among them may be what it died on); for a finding that names a range,
  the replay that names and saves the one; for a finding saved to a file,
  the command that makes the same inputs again where the file is lost, as
  on a CI runner. `ci` does not run it; CI has its own step (below), and
  every `cargo test` already replays the seeds and crashes.

### Benchmarks — `tidepool-bench`

- **Method** (`tidepool-bench`'s `harness`; its rustdoc has the details):
  each suite sets up untimed, then hands the harness one routine, which a
  warm-up runs at least once. Then:
  - a fast routine runs in batches of about 2 ms a sample, so that
    `Instant` is accurate;
  - samples are taken for 1 s, but never fewer than 10, so a 3 s query
    still gets 10;
  - a routine that changes its input gets a new one for every iteration,
    made and dropped outside the timing;
  - inputs and outputs go through `std::hint::black_box`.

  The report is each suite's median and p95 (nearest rank) per iteration,
  in nanoseconds.
- **Priming the allocator** (`harness::prime_allocator`): before the first
  suite, a run allocates and frees one 31 MiB block that it never writes.
  This targets glibc's malloc, which unmaps or trims freed memory until
  freeing a large block raises its dynamic mmap and trim thresholds. So a
  suite run alone or first could fault fresh pages in inside its timing:
  on the reference machine the 10,000-caret keystroke on 400 KB was 1.9
  or 2.2–2.6 ms from run to run, with about 42,000 or 285,000 page faults,
  and never faulted after a 100 MB suite. The block sets both thresholds
  where such a suite leaves them, so every suite's numbers are the same in
  any order (about 2,200 faults every run). It is plain `std` code with
  no `cfg`; with another allocator it is one untouched allocation and one
  free, and harmless. It does nothing about the CPU's clocks: with it, on
  the reference machine, that suite's median still went from 1.71 to
  2.10 ms over eight runs left to the scheduler, and from 1.91 to 1.95 ms
  over eight kept on one core (`taskset -c 4`).
- **Running and comparing:** `cargo xtask bench [FILTER] [--save --machine
  NAME]` runs, in the release profile, the suites whose names contain the
  filter. Names are hierarchical, the first part naming the crate measured
  (`text/history/keystroke_10k_carets/100mb`), and `cargo xtask bench
  --help` lists them. ADR-0035 decides the rest: the child `xtask` starts,
  the arguments, when a run compares with the baseline and marks a slower
  median, and when it exits 1.
- **Baseline:** `bench/baseline.json`, which only `--save` writes. Its
  format, and the rule that one file never mixes machines, are ADR-0035's;
  the `baseline` module's rustdoc shows the file.
- **Suites:** rope edits, position conversions, a 10,000-caret keystroke
  with its undo and redo, typing into 100 MB, and grapheme queries on
  crafted 100 MB lines (M1.9). Highlighting a 10k-line file, project
  search, frame build + raster at 1080p, glyph atlas fill and startup come
  with their tasks, which record them with `--save` on the reference
  machine (ADR-0035).
- A change that slows a suite by more than 10% against the baseline must
  say so and why. Repeat a run that marks a suite before reading it as a
  slowdown: the baseline was recorded from a whole run, and a suite run
  alone, left to the scheduler, landed from −4% to +17% of its baseline
  median over eight runs on the reference machine (roadmap M1.9's note).

### Responsiveness

- A scripted driver feeds keystrokes to `app` with a fake LSP server that
  waits 2 s per request and a mock agent that waits 2 s, and records
  keystroke-to-present time. Budget: p99 ≤ 16 ms (`docs/00-overview.md`).

## `xtask` checks

| Command | Fails when |
| :- | :- |
| `cargo xtask ci` | `deps`, `cargo fmt --check`, clippy `-D warnings`, `cargo test`, `layers`, `crates`, `features`, `unsafe-audit`, `headers`, `workflows` or `ucd` fails (`tools` and `theme` join once implemented) |
| `cargo xtask deps` | A workspace resolves or declares a registry or git package that `xtask/deps-allowlist.txt` doesn't approve (or declares it without `=version`), a `Cargo.lock` has an unapproved `source`, a path package lies outside the repository or isn't a member of the workspace that uses it, a workspace other than the root one and `agents/` exists, a cargo config file holds more than aliases, the repository holds a compiled artifact, a symbolic link or a file `deps` can't read, or cargo can't resolve a workspace offline |
| `cargo xtask layers` | A crate declares a workspace crate its matrix row doesn't allow (normal, dev or build dependency) or a path crate that isn't a member, members and matrix rows don't match, or `docs/01-architecture.md` breaks the rules below for how the matrix is written |
| `cargo xtask unsafe-audit` | `unsafe` outside `sys`/`platform`, a block or `unsafe impl` without `// SAFETY:`, the `unsafe_code` lint relaxed in source outside their crate roots, a `pub unsafe fn` without `# Safety`, an `extern` block outside `sys`, an exported macro in `sys`/`platform` that wraps its caller's tokens in `unsafe`, or one of the listed ways of bringing in code it did not read |
| `cargo xtask headers` | A source file (Rust, shell script, workflow, HTML, CSS, JavaScript) does not start with the Apache-2.0 header of `LICENSE`'s appendix in its comment syntax, then a blank line (after a script's `#!` line or an HTML file's doctype), unless it is exempt by path; a source file is over 8 MiB or not UTF-8; or anything under the root is a symbolic link (ADR-0021, ADR-0030) |
| `cargo xtask ucd [--check]` | A file in `crates/text/ucd/17.0.0/` is missing, a symbolic link, not a regular file, over 4 MiB, not UTF-8 or not matching its SHA-256 pin; a property file does not parse (a wrong first line, `@missing` default or `# Total code points:` count, an unknown value, a code point assigned twice); a code point is both ZWJ and Extended_Pictographic; or, with `--check` (the `ci` step), `crates/text/src/pos/grapheme/tables.rs` is missing, a link or not what the files generate. Without `--check` it writes that file when it changed |
| `cargo xtask tools` | Not yet implemented (M5.1). Planned: a `toolchains/*.toml` descriptor lacks a pinned version, HTTPS official source, SHA-256 (for downloads) or license |
| `cargo xtask theme` | Not yet implemented (M3.2). Planned: a theme token pair used for text falls below its contrast minimum (`docs/subsystems/ui-and-theme.md`) |
| `cargo xtask golden [--bless]` | Not yet implemented (M2.4). Planned: a golden frame or TUI snapshot differs |
| `cargo xtask fuzz-smoke [--secs N] [--target T] [--seed S] [--replay]` | `fuzz/`, `fuzz/corpus/` or `fuzz/crashes/` is a link or not a directory, a directory in `fuzz/corpus/` or `fuzz/crashes/` has no row in its table, or a row has no seeds (then nothing runs); or a target panics, hangs, takes the written or the reserved memory past its cap, fails its own check, reports another seed or too few seconds, or its test fails, does not build, does not run or dies, or cargo fails before it runs ("Fuzz smoke", above). Not a step of `ci` |
| `cargo xtask bench [filter] [--save --machine NAME]` | Reports only, and is not a step of `ci`: it fails only when cargo cannot build or start `tidepool-bench`, or that binary exits 1 for a bad argument, a filter that matches no suite, a failed suite, or a baseline that cannot be read, parsed or written (or, with `--save`, was recorded on another machine). A median more than 10% slower than the baseline's is marked, not failed ("Benchmarks", ADR-0035) |
| `cargo xtask workflows` | A workflow in `.github/workflows/` has no row in the check's table, uses an action its row does not allow, pins one by anything but a full commit SHA with a `# v…` comment, gives its token more than its row allows, uses `pull_request_target`, or checks out without `persist-credentials: false` (ADR-0022); or it is written in a form the check cannot read line by line (below); or there is no workflow to check, a workflow is over 1 MiB or not UTF-8, an entry there named like a workflow is not a regular file, or a symbolic link is in or on the way to `.github/workflows/` |
| `cargo xtask crates` | A member of the root workspace does not inherit `edition`, `version` and `license` from the workspace (each only `workspace = true`), lacks `publish = false`, has a `[lints]` table that is missing or not only `workspace = true`, or has a target (`[lib]`, `[[bin]]`, `[[test]]`, `[[bench]]`, `[[example]]`) with an `edition` of its own; the root `Cargo.toml` does not set `edition = "2024"` in `[workspace.package]`; a member's `README.md` is missing, a symbolic link or reached through one, not a regular file, over 1 MiB or not UTF-8, does not start with the line `# <package name>`, lacks one of the lines `## Status`, `## Usage`, `## Documentation`, `## Rules`, `## Checks` and `## License` outside code blocks, HTML blocks and comments, has one twice or out of that order, has `## Features` twice or anywhere but between `## Usage` and `## Documentation`, or names a task ID that `docs/roadmap.md` has no task for; a member's `LICENSE` or `NOTICE` is missing, a symbolic link or reached through one, not a regular file, over 1 MiB, not UTF-8 or not byte for byte the root's, or the root's is missing or cannot be read; a manifest that cargo loaded cannot be read or parsed here, or is not under the root; or `docs/roadmap.md` cannot be read or has no task, or `cargo metadata` fails (then it cannot run) (ADR-0023, ADR-0028) |
| `cargo xtask features` | `docs/features.jsonc` is not one object whose only member is the array `features`, of entries each with a `name`, `area` and `summary` that are not blank and are one line of plain text (no control character, line or paragraph separator or bidirectional control), `tasks` (at least one task ID) and, if any were ticked, `ticked`, each given once, and nothing else; two features share a name; a feature names a task twice, or one `docs/roadmap.md` lacks; a feature's `ticked` names a task its `tasks` does not, or is not the tasks of its `tasks` that the roadmap ticks; a ticked task belongs to no feature; a line of the roadmap looks like a task's but is not one, or has a CR inside it, not ending it, and a part that looks like a task's, or two tasks share an ID; or either file cannot be read (ADR-0030, "`features`" below) |
| `cargo xtask site [--out DIR]` | The output directory is not new, empty or marked by an earlier run, or is marked but holds a directory where `site` writes a file; the `TIDEPOOL_SITE_*` variables are not all three or none, or one is not valid; `features`, a step of the gate or an example fails, or an example's output is not UTF-8; the test step printed no `test result:` line, one whose counts cannot be read, or more `SKIPPED:` lines than tests libtest passed; or a file of `site/` cannot be read, or holds a placeholder `site` does not know. Then it writes nothing. Otherwise it builds the demo page ("Demo page", under "CI" below). Not a step of `ci` |

`xtask` never uploads, publishes or signs with real credentials. `deps`
never opens a file named like a secret (below); `unsafe-audit` reads every
`.rs` file, whatever the rest of its name, and `headers` every source file
it checks, though it never prints a line of one. `workflows` reads each
workflow, and quotes only the part of a line it rejects, such as an action's
reference or a permission. `crates` reads the root `Cargo.toml`, `LICENSE` and `NOTICE`,
each member's `Cargo.toml`, `README.md`, `LICENSE` and `NOTICE`, and
`docs/roadmap.md`. Its violations quote no line of them, only paths,
package and target names, sections and task IDs;
a failed `cargo metadata` is relayed as for the other checks ("Secrets in
messages", below). `features` reads `docs/features.jsonc` and
`docs/roadmap.md`, and its violations quote only feature names, task IDs,
task titles, the name of a member it does not know and the character a
JSON parse error names, each escaped on one line. `site` reads `site/`, `rust-toolchain.toml`, `.git` only
as far as HEAD's hash when the `TIDEPOOL_SITE_*` variables are not set,
and its examples' output ("Demo page"). It reads other files only
as far as a check needs: the first 1 KiB, to compare with compiled-code
signatures (none of it is kept or printed), or the whole of a file a check
parses. The one file it writes in the repository is the generated Unicode
tables, with `cargo xtask ucd` without `--check` (below), and the one it
renames is a dead replay's `fuzz/crashes/<target>/.last-input`, to
`died-<hash>`; the fuzz tests that `fuzz-smoke` runs save their findings
to `fuzz/crashes/` ("Fuzz smoke"), and `bench`'s child writes
`bench/baseline.json`, only with `--save` ("Benchmarks"). Besides those, `site` writes the demo
page into `target/site`, or its `--out` directory, and removes there only
what an earlier run wrote. It opens no sockets, and its cargo children run with
`CARGO_NET_OFFLINE=true`. The tests that `ci` runs inherit the environment,
so with a `TIDEPOOL_IT_*` variable set, a real-tool test may still reach the
network through its tool.

### Command line and rules

**Command line**

- `cargo xtask <command> [--root DIR]`. Every command takes no other
  argument, except `ucd`, which also accepts `--check` (once);
  `fuzz-smoke`, which accepts `--secs N`, `--target T`, `--seed S` and
  `--replay`, each once ("Fuzz smoke"); and `site`, which accepts
  `--out DIR` (once; "Demo page"). An unknown target, or a value that
  does not parse, is a usage error. `bench` is the exception: when it is
  the first argument, `xtask` parses nothing after it and passes every
  argument on to `tidepool-bench`, `--help` and `--root` included
  ("Benchmarks", ADR-0035).
  `--root DIR` checks the repository
  at `DIR` instead of the one `xtask` was built from; xtask's own tests use
  it on generated fixtures. A relative `DIR` is made absolute against the
  directory `xtask` started in, before any check runs
  (`std::path::absolute`). Links are not resolved, and `..` is kept except
  on Windows. `--help`, `-h` or `help` lists the commands.
- Exit status: 0 if every check that ran passed; 1 if a check failed or
  could not run (for `fuzz-smoke`, a target that did not run clean; for
  `bench`, if its child failed, a bad argument to it included); 2 if
  the command line was not understood (no or unknown command, a stray
  argument, `--root` without a value or with an empty one).
- A check prints its notes, one `violation:` line per problem, then
  `<check>: ok` or `<check>: FAILED (N violations)`. A check that cannot run
  prints `<check>: FAILED: <reason>`. A violation or reason that quotes
  cargo's error goes on over the following lines, each starting with
  `  | ` (M0.10, below). So for a check that ran, the verdict is always the
  last line; for one that could not run, the `FAILED:` line may not be.
- Secrets in messages (`AGENTS.md` rule 7): when `xtask` quotes the error
  of a cargo child it reads (`cargo metadata`, `cargo locate-project`), it
  leaves out the source lines cargo quotes (`  4 | token = "…"`). In those
  errors, and wherever it shows a package source, every URL has its user
  information replaced by `…` (`git+https://…@host/repo`). Not covered:
  cargo's message text can still name a value that is not a URL, such as a
  `rustc` path or an `include` file. And `ci`'s fmt, clippy and test steps
  (and `site`'s run of them), and `fuzz-smoke`'s tests, stream cargo's
  output as cargo prints it, and `bench`'s child passes on as printed both
  cargo's build output and `tidepool-bench`'s own, a panicking suite's
  message included (ADR-0035). So
  keep secrets out of the repository's cargo files; cargo reads
  credentials from `CARGO_HOME`.
- Control characters in output (M0.10): nothing a check reads, a file
  name, a cargo target's name or a line cargo quotes, can start a line of
  its output, which GitHub Actions would read as a workflow command such
  as `::warning::`, or send an escape sequence to a terminal.
  - The characters escaped are the control characters (C0, DEL and C1),
    the line and paragraph separators (U+2028, U+2029) and the
    bidirectional controls (U+061C, U+200E, U+200F, U+202A–U+202E,
    U+2066–U+2069). Each is written as its Rust escape: `\n`, `\r`,
    `\u{1b}`, `\u{202e}`. A backslash and every other character are kept.
  - Wherever a path is shown, cargo's command lines included, all of them
    are escaped.
  - Every note, violation, `skipped:` reason and error a check prints gets
    the same, except newlines and tabs, which a message quoting cargo's
    error over several lines needs. Each line after the first starts with
    `  | `, which is not whitespace once GitHub Actions has trimmed the
    leading spaces it ignores. So every line starts with text `xtask`
    wrote.
  - Not covered by `xtask`: `ci`'s fmt, clippy and test steps (and
    `site`'s run of them), and
    `fuzz-smoke`'s tests, stream cargo's output as cargo prints it, and the
    code they build and test can print anything. `bench`'s child does the
    same with cargo's build output and `tidepool-bench`'s own, which
    escapes the names it reads from the baseline and the command line but
    not a panicking suite's message, printed by the panic hook as it is
    (ADR-0035). And the runner also reads the older `##[command]`
    form anywhere in a line, so a path in a check's message can still hold
    one (actions/runner v2.338.2, `src/Runner.Common/ActionCommand.cs`:
    `TryParseV2` trims leading whitespace and needs `::` at the start;
    `TryParse` looks for `##[` anywhere). In CI the gate's step pauses
    workflow commands, and so do the fuzz-smoke step and `pages.yml`'s
    build step, so none acts as
    one, though a paused line still shows in the log as printed (M0.11, "CI
    (GitHub Actions)"). Run locally, the streamed steps and `bench` can
    still send escape sequences to the terminal. What `fuzz-smoke` quotes of a child's
    line in its own messages is escaped, as any check's message is.
- When every violation `deps` found is an out-of-date lockfile (below), its
  last line is `deps: FAILED (N violations; only out-of-date lockfiles)`
  (`1 violation` when there is one). That line is built from the count and
  fixed text only, never from a violation's text, which may hold a path.
  The Claude Code hook (`.claude/hooks/deps-guard.sh`) relies on it. In
  `ci` the `deps` step line reads the same, but the summary does not
  repeat it.
- Named but not yet implemented: `tools` (M5.1), `theme` (M3.2), `golden`
  (M2.4) and `package` (M11.6, M12.4, M14.4).
  Each named one exits 1 and
  says it is not yet implemented, whatever other options it gets; only
  `--help`/`-h` (usage text, exit 0) and `--root` without a value (exit 2)
  are handled first. `ci` lists `tools` and `theme` as "not yet
  implemented". None is ever reported as a pass.
- `xtask` runs `cargo` itself, not through `tidepool-toolchain` (the
  exception named in `AGENTS.md` rule 2, ADR-0017): the cargo that
  started it (`CARGO`, if not empty), or else `cargo` on the `PATH`. Its
  children are `cargo metadata`, `cargo locate-project`, `ci`'s fmt,
  clippy and test steps, `fuzz-smoke`'s `cargo test` of each target
  (ADR-0027), which also gets the `TIDEPOOL_FUZZ_*` variables,
  `site`'s: the same steps as `ci`'s, run in-process, and `cargo run
  --quiet -p <package> --example <name>` of each example the page shows
  (ADR-0030), and `bench`'s `cargo run --release --quiet -p
  tidepool-bench -- <args>`, given every argument after `bench`
  unchanged, `--help` and `--root` included, and whose success is
  `xtask`'s (ADR-0035). It runs no
  other program; the parsers run in those test processes, and the
  examples and the suites in their own, never in
  `xtask`. Each child is an argv vector, never a shell, and runs in the
  root with standard input closed and `CARGO_NET_OFFLINE=true`. `cargo
  metadata`, `cargo locate-project` and `fuzz-smoke`'s tests, whose output
  `xtask` reads, also get `CARGO_TERM_COLOR=never`, which overrides the
  environment and `term.color` (`fuzz-smoke` reads only the tests' result
  lines and cargo's and libtest's fixed ones in what they print); the fmt,
  clippy and test steps keep cargo's own colour setting. If
  `CARGO_TARGET_DIR` (or else
  `CARGO_BUILD_TARGET_DIR`) is set and not empty, every child gets
  `CARGO_TARGET_DIR` made absolute against the directory `xtask` started in.
  This is provisional: `package` is specified to run `lipo`, `iconutil` and
  `rc.exe` as well (`docs/05-release.md`), which this exception does not
  cover.
- The Claude Code hook (`.claude/hooks/deps-guard.sh`) runs after an edit
  of a manifest, lockfile or cargo configuration file. It checks the
  checkout holding the edited file: a linked worktree of this repository
  inside the project, else the project. Its `PATH` keeps only absolute
  entries, and rustup may not install a toolchain (`RUSTUP_AUTO_INSTALL=0`):
  a toolchain that `rust-toolchain.toml` names but that is missing is
  reported instead.
  - Before it runs cargo, it reads the cargo configuration and the rustup
    toolchain files (`rust-toolchain`, `rust-toolchain.toml`) in that
    checkout and each directory above it, up to the project.
    Configuration may hold only `[alias]` tables; the rules for alias
    values are left to `deps`, which runs later. A toolchain file may hold
    only `[toolchain]` with a plain `channel` and `profile`, `components`
    and `targets`, or a legacy one-line toolchain name. A `path` there
    would run another cargo.
  - It never shows a value. A report gives the file and line, with the
    first part of a table header (`[patch...]`) or a plain key
    (`token = ...`). A line inside a multi-line string or array, including
    one opened after a header, is shown as `...`. Source lines that cargo
    quotes in its errors are left out, and the user information of every
    URL in what it prints, its own lines and `xtask`'s, is replaced by
    `...`.
  - If the root workspace's lockfile is out of date, it checks only what
    the root manifests declare (`cargo metadata --no-deps`) and whether
    `Cargo.lock` names a package source. Then it says how to update the
    lockfile offline and exits 2 without running `xtask`.
  - It builds `xtask` only if `cargo tree` (normal and build edges) shows
    every package it is built from to be a member of the root workspace.
    Each must be in its own directory, reached without a symbolic link and
    holding none (stricter than `deps`, on purpose). Every target's source
    file must lie inside it, and it may have no build script and no
    proc-macro target. So compiling runs no package's code.
  - It builds into the checkout's `target/deps-guard/`, whatever
    `CARGO_TARGET_DIR` or `build.target-dir` says, and refuses a symbolic
    link inside it, so another checkout's build cannot supply the binary.
    It removes the old binary before each build. It keeps a stamp of the
    sources' contents and starts the build afresh when the stamp changes,
    since cargo judges freshness by file times.
  - `xtask` then runs `deps` and `layers`. If `xtask` cannot be built, the
    hook runs `scripts/check-deps.sh` and exits 2 even when that passes,
    because that check is incomplete. A report over 60 lines is cut to its
    first 40 and last 5.
  - Not covered: a file path with a JSON escape in it is not decoded, so
    the project is checked rather than the worktree. On Windows every
    path has one, which M12 must fix.

**Files searched** (`deps`, `unsafe-audit` and `headers`)

- Everything under the root, including hidden directories, nested `.git`
  directories and submodules. Skipped: `.git` directly under the root, the
  `target` directory beside a `Cargo.toml` that `cargo locate-project
  --workspace` reports as a workspace root, and linked worktrees of the
  repository. A `target` beside any other manifest is searched.
- That `target` must be a real directory: a symbolic link there is reported
  like any other link. It is skipped only while cargo can load its
  workspace. If cargo cannot (an invalid manifest, say), `target` is
  searched like any other directory, and what cargo built there fails
  `deps`.
- Build output anywhere else in the repository is searched like source,
  whether `CARGO_TARGET_DIR`, `CARGO_BUILD_TARGET_DIR` or `build.target-dir`
  put it there, so the checks fail on what cargo built or the tests
  generated there; a note says so when the environment named it. Keep the
  build directory outside the repository or at `<root>/target`.
- A worktree is skipped only if it is registered both ways: its `.git` is a
  regular file (not a link) naming an entry in the repository's
  `.git/worktrees/` (found through `commondir` when the root is itself a
  worktree), and that entry's `gitdir` names it back. A note names each
  skipped worktree; run `xtask` in it to check it. If this checkout builds
  code from one, `cargo metadata` still shows a crate there. A root member
  there fails the audit's crate-root rule, and `layers` too when it has no
  row or an edge the matrix forbids. A non-member path package there fails
  `deps`. A worktree inside a member's package directory fails the audit.
- Symbolic links are never followed, and `xtask` itself never resolves
  one (M0.10): every link under the root is a violation in each check,
  whatever its name and wherever it leads, a dangling one included.
  Telling a link to a directory from one to a file would mean resolving it,
  which touches its target. On Windows, a link to a UNC path would make the
  OS contact that host.
  - A linked `Cargo.toml` is not a manifest to the walk, so the `target`
    beside it is searched, and cargo is not asked about its directory.
  - The files a check reads by a fixed name, `xtask/deps-allowlist.txt`
    for `deps`, `docs/01-architecture.md` for `layers`,
    `docs/roadmap.md`, the root `Cargo.toml`, `LICENSE` and `NOTICE`, and
    each member's `Cargo.toml`, `README.md`, `LICENSE` and `NOTICE` for
    `crates`, and `docs/features.jsonc` and `docs/roadmap.md` for
    `features`, are not opened when they, or a directory on the way to them,
    are links: the check fails, naming the link (for `crates`, a linked
    manifest, README or copy is one violation, and the check goes on).
  - Not covered: cargo follows links. `deps`, `layers`, `crates` and
    `unsafe-audit` run `cargo metadata`, which loads every workspace they
    find, and `cargo xtask` itself loads the root one before `xtask`
    starts. A linked member, path dependency, crate root or configuration
    file is resolved by cargo, and the paths cargo reports are resolved
    again by `deps`, `layers` and `unsafe-audit` (`crates` reads them only
    as paths under the root, refusing a link on the way). The link is still
    a violation.
  - The repository holds no links. Git for Windows checks them out as text
    files unless symbolic links are turned on (`core.symlinks`).
- `Cargo.toml`, `Cargo.lock` and `.rs` match in any case, since macOS and
  Windows ignore case. On Linux cargo cannot describe a workspace from a
  `cargo.toml`, so that workspace fails closed.
- Committed test data is checked as the real thing: a file named
  `Cargo.toml` or `Cargo.lock`, `config` or `config.toml` in a `.cargo`
  directory, a file ending in `.rs` (or, for `headers`, `.sh`, `.html`,
  `.css` or `.js`), or one
  named or starting like a compiled artifact (`deps`), in any case. Give samples another suffix
  (`.rs.txt`), or generate them at test time under `<root>/target` or a
  build directory outside the repository. xtask's fixtures use
  `CARGO_TARGET_TMPDIR`, each in a case directory whose `Cargo.toml` is a
  workspace with no members, so what lies above it never matters. Its unit
  tests use `<root>/target/xtask-unit-tmp/`.
- This covers planned test data too: TOML samples and fuzz seeds named
  `Cargo.toml` or `Cargo.lock`, lexer inputs and fuzz seeds named `.rs`,
  trust-test and cargo-task projects, and the Android and iOS sample apps.
- Not covered, in every check that uses this walk: races. A file swapped
  for a link or a FIFO between the walk and its open is followed, or
  blocks the open. A link committed to the repository cannot race.

**`deps`**

- Finds every `Cargo.toml` under the root and resolves the workspace each
  belongs to once, offline and from its lockfile as it is: `cargo metadata
  --format-version 1 --offline --locked`. It never rewrites a lockfile.
  When cargo cannot list a workspace's members (an invalid root manifest
  or configuration), each manifest is tried and reported on its own.
- Only the root workspace and `agents/` (created by M9.2) may exist. The
  members of `agents/` may be only `tp-agent-mock`, `tp-agent-local` and
  `tp-agent-claude`, each in `agents/<name>/`. A manifest under the root
  that belongs to any other workspace fails. This is provisional: approving
  register item #30 (`cargo fuzz` in a `fuzz/` workspace,
  `docs/02-dependency-policy.md`) would need this rule changed, and M9.2 may
  need more members in `agents/` (shared TAP code, a conformance suite).
- A resolved package with a `source` (registry or git) must be approved: an
  allowlist line names it at its exact version, and the source is
  crates.io.
- A path package (`source` null) must lie in the repository (links and `..`
  resolved) and be a member of the workspace whose graph it is in. Vendored
  or copied code that is not a member fails, listed or not: in an
  `exclude`d directory, in another workspace (even one of its own), or
  outside the repository. Another workspace's crates fail too. As coded,
  then, `agents/` cannot use `crates/`, and the root workspace cannot use
  `agents/`. This is provisional: whether the reference agents may use root
  crates such as `core` is an open question for M9.2.
- In the root workspace, vendored code that cargo makes a member passes
  `deps`. Cargo makes a path dependency inside the workspace directory a
  member unless it is `exclude`d. `layers` fails on such a member when it
  has no matrix row, as vendored third-party code has none, but `ci` runs
  `layers` only after clippy and test (`ci`, below). In `agents/` such a
  member fails `deps` unless it is a reference agent in its own directory.
- Every registry or git dependency a member declares, in any table and for
  any target, must be approved, from crates.io, with the exact requirement
  `=version`. An unapproved one fails even when `[patch]`, `[replace]` or
  `[source]` serves it from a directory.
- Every `Cargo.lock` under the root is also read as text, whether a
  workspace uses it or not. A `source = ` line fails unless it is in a
  `[[package]]` entry from crates.io whose name and version are approved;
  with the allowlist empty, any does. A quoted `"source"` or `'source'` key
  counts as one. A `source` key in a dotted key or inline table, and any
  quoted key with an escape, fail.
- Fails closed: a workspace cargo can't resolve this way (a crates.io
  package not available offline, a manifest in no workspace, a lockfile
  that needs updating) is a violation, because its graph was not checked.
  What its members declare is still checked (`--no-deps`): registry, git,
  outside and non-member path dependencies, and members outside the
  repository, are each reported. If those pass and cargo's error is that
  the lockfile is out of date, the message says so and gives an offline
  update that compiles nothing (`cargo metadata --offline --format-version
  1 --manifest-path <manifest>`); run `deps` again after it. Only that
  message counts as an out-of-date lockfile. If every violation `deps`
  found is one, the last line reads `deps: FAILED (N violations; only
  out-of-date lockfiles)`. Any other violation, anywhere, gives the plain
  form.
- A cargo configuration file (`config` or `config.toml` in a `.cargo`
  directory, anywhere under the root, in any case) may hold only comments
  and an `[alias]` table: bare names, each with a one-line `"..."` string
  or array of them, without escapes or `--config`. Any other line fails,
  since it could change what cargo builds or what the checks see. An
  allowed alias can still change what `cargo xtask` runs (another
  manifest, say), so alias changes need review. A value could be a secret
  such as a registry token, so a violation never shows one. It shows the
  first part of a table header when that is a bare name (`[registry]`,
  `[patch.…]`, otherwise `[…]`), or a key when it is a bare or dotted one
  (`token = …`). A line that continues a multi-line string, array or inline
  table, including one opened after a header, is shown as `…`. One UTF-8
  byte-order mark at the start of the file is skipped. Configuration outside the repository (parent
  directories, `CARGO_HOME`, `--config`) is not checked.
- A compiled artifact fails: a file named `.rlib`, `.rmeta`, `.a`, `.o`,
  `.obj`, `.so`, `.dylib`, `.dll`, `.lib`, `.exe` or `.wasm` (in any case),
  or one whose first bytes are ELF, Mach-O (thin, or a 32- or 64-bit
  universal header; a Java class shares the 32-bit one), PE (when its PE
  header lies in the first 1 KiB), an `ar` archive, rustc metadata,
  WebAssembly, or LLVM bitcode, raw or wrapped. A bare COFF object is known
  by name only. A file named like an artifact fails by name
  and is not opened. Files named like secrets (`.env`, `.env.*`,
  `keystore.properties`, `*.jks`, `*.keystore`, `*.p12`, `*.p8`, `*.pem`,
  `*.key`, `*.mobileprovision`, `*.provisionprofile`, in any case) are
  never opened. Of every other file it searches, `deps` reads the first
  1 KiB.
- A file it cannot open or read for that is a violation (it was not
  checked), and the check goes on. A directory it cannot list, or a
  lockfile, cargo configuration file or allowlist it cannot read (missing,
  over 16 MiB or not UTF-8), stops the check: `deps: FAILED: <reason>`.
- `xtask/deps-allowlist.txt` has one `name version ADR-NNNN` per line:
  three fields separated by whitespace, the last `ADR-` and four digits.
  Blank lines and `#` lines are ignored; any other line is a violation. One
  UTF-8 byte-order mark at the start of the file is skipped. A line
  approves that name at that version from crates.io only. The list is empty
  (ADR-0001). A missing allowlist fails the check, and so does a tree with
  no `Cargo.toml` anywhere under the root.

**`layers`**

- Reads the matrix from `docs/01-architecture.md` → "Dependency matrix" on
  every run. A cell is `—` (or `-`), `all crates above`, `any workspace
  crate`, or backticked names (`core` for `tidepool-core`, or an alias such
  as `app`), optionally followed by "(used only as a dev-dependency)".
  Anything else fails, naming the line.
- The document is read close to how GFM renders it, by these rules. Where
  they differ from a renderer, they mostly fail:
  - HTML comments and fenced code blocks (fences indented at most three
    columns) are not read as text. A line a comment touches is not read as
    text either, so a comment on a table row ends the table there, and the
    rows after it fail as a second table.
  - Only fenced code blocks and code spans count as code. An indented code
    block is read as text. A code span counts only if it closes on its
    line.
  - Outside code, the document may hold no raw HTML but comments, as a
    block or inline. A line whose first character after any indentation is
    `<` (other than a comment) counts as an HTML block and fails. So does
    HTML-like text in an indented code block, such as `<root>` or
    `Vec<u8>`.
  - It has one header row of the matrix's shape, with or without outer
    pipes, counting comments and code.
  - The section runs to the next line read as text that starts with `# `
    or `## ` (a setext or indented heading does not end it). Its table is
    the run of lines starting with `|` (not indented) from the first such
    line. The section has exactly one table, with a GFM delimiter row
    (cells such as `:-`, `---` or `-:`), and no other line of it that is
    read as text holds a `|`.
  - Two gaps are known, and each needs crafted text. A line without `|`
    directly under the table, which GFM renders as one more row, is not
    read and does not fail. A backslash before a backtick is not read as
    an escape, so `` \` `` still opens a code span here, and raw HTML after
    it is missed.
- Checks the root workspace only (`cargo metadata --offline --no-deps`); the
  matrix has no rows for `agents/`. Every member needs a row, and every row
  a member.
- Every declared dependency counts: normal, dev and build, for every target
  platform.
- A path dependency is matched to a member by its resolved directory, not
  its name. A path crate that is not a member fails, named by its resolved
  directory relative to the resolved workspace root
  (`vendor/tidepool-core`), or in full if it lies outside that root. A
  registry or git dependency named like a member counts as an edge to it,
  since a `[patch]` can serve the member under that name. Other registry
  and git dependencies are left to `deps`.
- A crate whose row says "(used only as a dev-dependency)" (today only
  `tidepool-testkit`) may be used only from `[dev-dependencies]`. The note
  grants no edge: a crate may use testkit only if its own row allows it.
  Today only `tidepool-bench` ("any workspace crate") does. This is
  provisional: the test plan has the tests of `toolchain`, `vcs`, `mobile`,
  `rpc` and `lang`, among others, use testkit's fakes and transcripts,
  which fails `layers` as the matrix stands. The matrix needs a decision
  before M5.1.

**`crates`**

- Checks the members of the root workspace (`cargo metadata --offline
  --no-deps`) against ADR-0023 and ADR-0028, each in the directory of its
  `Cargo.toml`.
  A member's manifest must be under the root: cargo's path and the root
  are compared with `.` and `..` taken out, as cargo writes the path, and
  no link resolved. One that is not is a violation and is not checked.
- Cargo loads every manifest first (`cargo metadata`). A manifest that is
  not TOML, or holds a value cargo rejects, in the root or any member,
  makes it fail, and then the check cannot run (below).
- Then each manifest is read whole, at most 1 MiB of UTF-8, and parsed
  with `tidepool_codec::toml` within limits of the check's own (32 levels
  deep, 64 KiB a string, 10,000 entries a table). One violation per rule
  broken:
  - `package.edition`, `package.version` and `package.license` must each
    be a table whose only key is `workspace`, set to `true`, which is what
    `edition.workspace = true` and `edition = { workspace = true }` both
    are. A literal value, a missing key, `workspace = false` or another key
    beside it fails.
  - `package.publish` must be `false`. Missing, `true`, a list of
    registries (`[]` included) and `publish.workspace = true` fail.
  - `[lints]` must be there and hold only `workspace = true`.
  - No target may set an `edition` of its own: an `edition` key in
    `[lib]`, or in any table of `[[bin]]`, `[[test]]`, `[[bench]]` or
    `[[example]]`, is one violation per target, named by its table and its
    `name` if it has one. Cargo 1.99 still accepts one, with a warning, and
    compiles that target under it, whatever the package inherits.
  - The root `Cargo.toml` must set `edition = "2024"` in
    `[workspace.package]`, the edition every member inherits. It is read
    once, also when the root is itself a member, so a problem with it is
    reported once.
  - A manifest that cargo loaded but this check cannot read or parse (a
    link, over 1 MiB, past the limits above) is one violation naming it,
    and the check goes on with the other members.
- A member's README is the `README.md` beside its `Cargo.toml`, that name
  exactly. Missing, a symbolic link or reached through one, not a regular
  file (checked before it is opened, so a FIFO is never opened), over
  1 MiB or not UTF-8 is one violation. Otherwise:
  - its first line, after one leading byte-order mark and one trailing CR,
    must be `# <package name>`. The violation says what the line should
    be, never what it is;
  - its sections are the lines `## Status`, `## Usage`, `## Documentation`,
    `## Rules`, `## Checks` and `## License`, exactly (after one trailing
    CR), that `layers`' reading of a document ("`layers`" above) takes for
    text: outside fenced code blocks, HTML blocks and HTML comments as that
    reading finds them, line by line and at the top level, which is close
    to a renderer's but not the same (Not covered, below). So a fence opens
    at most three columns in, with three or more backticks or tildes (and
    no backtick in a backtick fence's info string), and closes with at
    least as many of the same character, then only spaces and tabs; a
    block that never closes runs to the end; and a fence inside a comment
    or an HTML block opens nothing;
  - each section must be there once, and in that order. One violation for
    each section missing, each one there more than once, and each pair of
    sections that are neighbours in that order, once those missing or there
    more than once are left out, and stand the other way round (the
    violation says which must come before which). So a section moved far
    away is one violation, and the sections all the other way round are
    five;
  - a `## Features` line may be there, once, between `## Usage` and
    `## Documentation` (ADR-0028: only a crate with Cargo features has
    one, and none has today). Twice is one violation; standing before a
    `## Usage` or after a `## Documentation` that is there once is
    another. Other `## ` headings, ADR-0023's `## Design` among them, may
    stand anywhere;
  - every task ID it names, anywhere (in code too), must be a task in
    `docs/roadmap.md`. A task ID is `M`, ASCII digits, `.` and ASCII
    digits, each run of digits as long as it goes, with no ASCII letter,
    digit or `_` just before the `M`: `M1.23` is one ID, not `M1.2`, and
    `xM1.2` is none. Each unknown ID is one violation, once per README, in
    numeric order (`M1.9` before `M1.10`).
- A member's `LICENSE` and `NOTICE` are the files of those names beside
  its `Cargo.toml`, read as its README is (at most 1 MiB of UTF-8, a
  regular file, no link on the way), and each must be byte for byte the
  root's (ADR-0028). One violation per copy that is missing, cannot be read
  or differs; it names the copy and never shows a line of it or of the
  root's. The root's `LICENSE` and `NOTICE` are read once: one that is
  missing (`the workspace has no NOTICE at its root`) or cannot be read is
  one violation, and the members' copies of it are then read but not
  compared. A member at the root is its own copy.
- The tasks are the lines of `docs/roadmap.md` that start
  `- [ ] **M<n>.<m> — ` or `- [x] **M<n>.<m> — `, an em dash between the
  spaces. The roadmap is read as `layers` reads its document (at most
  4 MiB, no link on the way), and must be a regular file.
- If the roadmap cannot be read or has no task, or `cargo metadata` fails,
  the check cannot run: `crates: FAILED: <reason>`, where cargo's error is
  relayed without the source lines it quotes ("Secrets in messages").
- Violations name paths (escaped, as everywhere), package and target
  names, sections and task IDs. Notes give the number of members checked
  and of the roadmap's task IDs.
- Not covered:
  - whether what a README calls built is built, or whether the tasks it
    gives for it are ticked (ADR-0023's Consequences);
  - the rest of ADR-0028's shape: the paragraphs and the layer line before
    `## Status`, and what each section says; nor whether a README's Rust
    examples run as doc tests, which `cargo test` does only where the
    crate's `src/lib.rs` includes its README;
  - where that reading and a renderer differ, a section is hidden, and
    reported missing, or in one case seen though a browser hides it:
    - only a line that is itself a fence, at most three columns in, opens
      or closes one. A fence after `>` or a list marker on its line, or
      four or more columns in, is not seen. So when a list item opens its
      fence on the marker line (`- ```sh`), its closing fence opens one;
      and a fence that closes four or more columns in, or that its list
      item ends without closing, runs on. Each hides the sections after
      it;
    - every `<!--` outside a fence or an HTML block opens a comment that
      runs to the next `-->` after it, wherever the `<!--` is: in a code
      span, in indented code, after a backslash. `<!-->` and `<!--->` do
      not close. Each hides the sections up to the next `-->`;
    - every line whose first character after its indentation is `<`
      (other than a comment) opens an HTML block, as `layers` reads one,
      and a section in that block is not seen;
    - a `<!--` on a line of an HTML block is not followed, so a section
      after the block counts, though a browser shows it inside that
      comment;
  - the members of `agents/`, a workspace of its own from M9.2;
  - on a file system that ignores case (macOS and Windows by default),
    `readme.md` opens as `README.md`, and `license` as `LICENSE`, and
    passes;
  - races, as in "Files searched".

**`features`**

- Ported from Nadir's check (Nadir's ADR-0029); the rules are ADR-0030's.
  `docs/features.jsonc` lists Tidepool's features as the demo page shows
  them, each with the roadmap tasks it needs. The page marks a feature
  shipped when all of them are ticked, in progress when some are, and
  planned when none are.
- The tasks are the roadmap's task lines as `crates` reads them, with the
  same reader (`- [ ] **M<n>.<m> — ` and `- [x] **M<n>.<m> — `, above).
  A line that looks like a task's but that reader does not take (a list
  item, indented, quoted or numbered or not, with a checkbox of one
  character and the start of a task ID after it, such as `M5.3` or
  `m5.3`, outside a longer word) is a violation, since a task it misses
  would escape the check; so are two tasks with one ID. So is a line that
  still holds a CR once its LF or CR LF ending is taken off, when a part
  of it between CRs looks like a task's: a renderer starts a new line at
  that CR, where the reader does not, so the task a reader sees may be one
  the check misses.
- The file is read as JSONC (`tidepool_codec::json`): one object whose
  only member is `features`, an array of objects. Each has `name`, `area`
  and `summary`, strings that are not blank once trimmed and are one line
  of plain text, with no character that `xtask` escapes in what it prints
  (a control character, a line or paragraph separator or a bidirectional
  control: "Control characters in output", above), so the page shows
  each on one line; `tasks`, an array of at least one task ID; and
  `ticked`, an array of task IDs, left out when none was ticked. Any other
  member fails, and so does a member given twice, since Tidepool's JSON
  parser keeps both. The comment at the top of the file gives the areas
  and how to write a summary; the check does not read it.
- Violations: two features with one name; a feature that names a task
  twice, or one the roadmap does not have; a `ticked` that names a task
  twice, or one its feature's `tasks` does not; a feature whose `ticked`
  is not the tasks of its `tasks` that the roadmap ticks (the summary was
  written for other ticks: rewrite it, then set `ticked`); and a ticked
  task that no feature needs. So the task that ships something adds it to
  a feature, or describes a new one, and brings the summary up to date.
- Both files are read whole as UTF-8, each a regular file reached through
  no link ("Files searched"). A file that cannot be read stops the check:
  `features: FAILED: <reason>`.
- Violations quote feature names, task IDs and task titles, the name of a
  member it does not know, and the character a JSON parse error names,
  each escaped on one line, newlines and tabs too. Notes give how many
  features are shipped, in progress
  and planned, and how many of the roadmap's tasks are ticked.
- Not covered: whether a summary says what its ticked tasks built, which
  only review sees; the check sees only that `ticked` moved with the
  ticks.

**`unsafe-audit`**

- Reads every `.rs` file under the root with its own small lexer. The lexer
  tells code from comments and from string, raw string, byte, C string and
  character literals; it is not a parser. It never fails or panics: text
  that isn't valid Rust still gets an answer, and the compiler reports it.
  A raw identifier (`r#unsafe`) is a name, never a keyword. Because it reads
  text, it covers every `cfg` branch and code in macros.
- Violations:
  - `unsafe` in any form (block, `fn`, `impl`, `trait`, `extern`,
    `#[unsafe(...)]`) outside `crates/sys/` and `crates/platform/`.
  - In those two, an `unsafe` block or `unsafe impl` with no `//` comment
    starting `SAFETY:` between the end of the previous statement (`;`, `{`
    or `}`) and the keyword. Doc comments and `/* */` don't count.
  - `allow`, `expect` or `warn` of `unsafe_code` (also inside `cfg_attr`)
    anywhere but `crates/sys/src/lib.rs` and `crates/platform/src/lib.rs`.
    Only attributes in source are seen here. A member's `[lints]` table
    cannot relax `unsafe_code` either: `crates` fails one that is anything
    but `workspace = true`. No check reads the root's `[workspace.lints]`,
    whose `unsafe_code = "deny"` every member inherits, or `RUSTFLAGS`;
    review keeps both. So, while that table denies it, `unsafe`
    compiles only in the library targets of `sys` and `platform` (their
    `#[cfg(test)]` modules included). Their
    integration tests, benches, examples and binaries are crates of their
    own, under the workspace's `deny`. This is provisional: M2.2's Xvfb test
    reads pixels back with `XGetImage`, and where that FFI call may live is
    an open question.
  - A function written `pub` or `pub(...)` `unsafe fn` (also `const`,
    `async`, with an `extern` ABI, or in an `extern` block) with no
    `# Safety` line in its outer doc comments and no `# Safety` in its
    `#[doc = "..."]` attributes. An unmarked `pub fn` in an `unsafe extern`
    block is not checked.
  - An `extern` block outside `tidepool-sys`, or one in it not written
    `unsafe extern`.
  - In those two crates, a `#[macro_export]` `macro_rules!` (also through
    `cfg_attr`) with an `unsafe` block or `unsafe impl` that holds a
    metavariable other than `$crate`: in a calling crate, the caller's
    tokens would run as `unsafe` that neither its lint nor the audit sees.
- Fails closed on these ways of bringing in code it did not read:
  - `#[path = "..."]` or `include!(...)` (also in `cfg_attr`, as a raw
    identifier, or through `std::`), unless the argument is one plain or raw
    string literal without escapes, naming a relative `.rs` path inside the
    file's directory. No `:` anywhere. Each `/`-separated component is
    non-empty, doesn't start with `.`, doesn't end in `.` or a space, isn't
    `target` in any case, and has no `\` or `~`.
  - A crate root of a member of any workspace under the root (library,
    binary, test, example, bench or build script, as `cargo metadata
    --offline --no-deps` reports it) that is not a file the audit read, such
    as `[lib] path = "src/lib.txt"`. A workspace cargo can't describe
    offline is a violation.
  - A crate root of such a member, of any kind (build scripts included),
    that does not lie inside its package directory, both with links and
    `..` resolved, such as `[lib] path = "../../lib.rs"`. From outside, a
    plain `mod target;` could compile a directory the audit skips. The
    violation names the target, the crate root and the package directory.
  - A skipped linked worktree inside the package directory of a member of
    any workspace, since a plain `mod` could compile files from it.
  - A symbolic link (above), a file over 8 MiB, or a file that isn't UTF-8.
    A `.rs` file it cannot read stops the audit:
    `unsafe-audit: FAILED: <reason>`.
  - `crates/sys/src/lib.rs` or `crates/platform/src/lib.rs` not found,
    which means the root is wrong.
- Not covered: a workspace root's `target/` inside a member's package
  directory, as when one manifest holds both `[workspace]` and `[package]`.
  A crate root placed directly in that directory (`[lib] path = "lib.rs"`,
  or a `build.rs` beside the manifest) can reach `target/` through a plain
  `mod target;`, and the audit does not see those files. Tidepool's root
  manifest has no `[package]` today.
- Also not covered: unsafe attributes (`#[no_mangle]`, `#[export_name]`,
  `#[link_section]`) written without `unsafe`, as editions before 2024
  allow. Only the `unsafe_code` lint catches those. Members use edition
  2024 and `[lints] workspace = true`, and `cargo xtask crates` verifies
  both: each member inherits the workspace's edition, which the root must
  set to 2024, no target of a member sets an edition of its own, and each
  member inherits the workspace's lints, though no check reads what
  `[workspace.lints]` holds (ADR-0023, "`crates`" above).
- Notes list the `unsafe` uses per crate (review any growth), the files
  scanned and the crate roots checked.
- It does not check where `#[cfg(target_os)]` appears (`AGENTS.md` rule 5).
  No check does.

**`headers`**

- Source files, found as "Files searched" says: a Rust file (`.rs`), a
  shell script (`.sh`), a workflow (`.yml` or `.yaml` directly in
  `.github/workflows/`, where GitHub reads them), and an HTML (`.html`),
  CSS (`.css`) or JavaScript (`.js`) file, such as the demo page's in
  `site/` (ADR-0030, M0.14), the extension in any case. Configuration
  and data carry no header (ADR-0021): TOML, Markdown, JSON and JSONC
  (`docs/features.jsonc` among them), other YAML (`dependabot.yml`, the
  issue forms, YAML in a folder below `.github/workflows/`), test vectors,
  the Unicode data, `LICENSE` and `NOTICE`, and plain text that is served
  as it is, such as `site/llms.txt`.
- Each starts with the 13 lines of `LICENSE`'s appendix from its copyright
  line, word for word: `// ` before each in Rust and JavaScript, `# ` in
  scripts and workflows, and `//` or `#` alone on an empty one. In HTML
  and CSS they are one comment, as in Nadir's check: a line `<!--` (HTML)
  or `/*` (CSS), the 13 lines each indented by two spaces (an empty one
  left empty), then a line `-->` or `*/`. A blank line follows, unless the
  file ends there. The header starts on line 1, or on line 2 after a
  script's `#!` line or an HTML file's doctype (a first line that starts
  `<!doctype`, in any case). One leading UTF-8 byte-order mark is
  skipped, as `deps` skips one. CR LF line endings, as a Windows checkout
  may have, are accepted; lone CR ones are not. Only those first lines of
  a file are read as lines.
- A violation names the first line that is wrong and what it should be,
  never what it holds: `<file>:<line>: expected `<text>`, line N of the
  Apache-2.0 header in `LICENSE`'s appendix`; in HTML and CSS,
  `<file>:<line>: expected `<!--`, the comment that holds the Apache-2.0
  header` or `<file>:<line>: expected `-->`, the end of the comment that
  holds the Apache-2.0 header` (`/*` and `*/` in CSS); or
  `<file>:<line>: expected a blank line after the Apache-2.0 header`. A
  file over 8 MiB and a file that isn't UTF-8 are violations too, and so
  is every symbolic link (above). A file it cannot read stops the check: `headers: FAILED:
  <reason>`.
- Exempt by path (`EXEMPT` in `xtask/src/headers.rs`, ADR-0021):
  `crates/text/src/pos/grapheme/tables.rs`, which `ucd` generates and
  which keeps the Unicode data's notice instead (ADR-0018).
- Notes give the number of files whose header was checked (not those too
  large or not UTF-8) and the exempt files found, each with its reason.
- A unit test compares its header with `LICENSE`'s appendix, so a change
  to either fails until the other matches.
- Not covered: a new kind of source file (another extension) is unchecked
  until `kind` in `xtask/src/headers.rs` knows it. The third-party notices
  kept beside other material, such as the IETF Trust's for the RFC 8259
  examples in `crates/codec/tests/json.rs`, are not checked.

**`workflows`**

- Ported from Nadir's check (Nadir's ADR-0032). The rules are ADR-0022's,
  against the table `WORKFLOWS` in `xtask/src/workflows.rs`, whose rows
  "CI (GitHub Actions)" below lists. A workflow is matched to its row by
  its exact file name, so `CI.yml` has none. `pages.yml`'s row is
  ADR-0030's (M0.14); the release workflow adds its row with M14.4.
- The workflows are the `.yml` and `.yaml` files directly in
  `.github/workflows/`, the extension in any case, as `headers` finds them.
  A folder there, and a file with another extension, is not one: GitHub
  reads neither. The check reads only that directory; the links elsewhere
  are reported by `deps`, `unsafe-audit` and `headers`.
- Symbolic links are neither followed nor resolved, as in "Files searched"
  above. `.github` and `.github/workflows` must each be a real directory,
  checked shallowest first; and every link directly in `.github/workflows/`,
  whatever its name and wherever it leads, is a violation and is not read.
  So is an entry named like a workflow that is neither a file, a folder
  nor a link (a FIFO, say), which is not opened.
- Nothing to check fails, as `deps` fails on a tree with no `Cargo.toml`.
  `.github/workflows` missing (the violation asks whether the root is
  right), not a directory, reached through a link, or holding no workflow
  is a violation. So a check of the wrong directory never passes, and
  `xtask`'s own fixture repositories carry a `ci.yml` and a `codeql.yml`
  that keep the rules.
- Each workflow is read whole, within 1 MiB, and must be UTF-8; otherwise
  it is a violation and is not checked. A leading byte-order mark is
  skipped. Lines end at LF or CR LF. Any other line break, which a YAML
  reader also ends a line at (a lone CR, at the end of the file too; and,
  as GitHub's YAML 1.1 reader does, NEL, U+2028 and U+2029), is a
  violation naming its line, and the file is checked no further. A
  workflow it cannot read stops the check: `workflows: FAILED: <reason>`.
- It reads lines, not YAML (ADR-0022). A comment, from a `#` at a line's
  start or after a space or a tab, is left out. A key is found by its
  column, after a list item's dash, and read as YAML ends a plain implicit
  key: the text before the first `:` followed by a space, a tab or the
  line's end (or, for a quoted key, the `:` after its closing quote),
  trimmed, without one pair of matching quotes. So `uses :`, `uses\t:`,
  `"uses":` and `'permissions':` are the keys `uses` and `permissions`.
  The keys it reads by name are `on`, `jobs`, `permissions`, `uses`,
  `with` and `persist-credentials`.
- Where the lines would not say what YAML says, it fails closed. Beyond
  ADR-0022's words:
  - a local action (`./…`) fails like any other its row does not name,
    since the check does not read its steps (Nadir's check passes one);
  - a `uses:` with no action on its line fails;
  - in a job's `permissions:`, a level that is not `read`, `write` or
    `none` once unquoted fails, and so does a `write` in a `permissions:`
    block that is in no job; an empty top-level block fails;
  - a job is found at whatever indentation the file gives its jobs;
  - `persist-credentials: false` counts only as an input of the checkout
    step's `with:`, not in its `env:` or inside another input's value. A
    step starts at the nearest line above its `uses:` that is indented less
    than its keys (not a blank or comment line), so a step whose dash is
    alone on its line (`-`, `- # note`) is not given the step before's
    input; a `uses: actions/checkout` in no step fails. The step must have
    one `with:` and one `persist-credentials` input, reading `false` with
    no more-indented line going on with it: a YAML reader keeps the last of
    two equal keys, and GitHub rejects a workflow that has two;
  - the text `pull_request_target` fails wherever it is on a line before
    its comment, even in a string. A top-level key that is quoted or holds
    a `\` fails, since it could be `on`. In the top-level `on:` section, up
    to the next top-level key, a line whose text (its comment left out, as
    YAML reads one) holds a `\` fails, since an escape could spell the
    event, and so does a line on which a ` #` falls inside a quoted value,
    which this check would read as the start of a comment. A comment that
    names the event, there or elsewhere, is only words;
  - a quoted value, single or double, that does not end on its own line
    fails, since its next lines would be read as keys (`''` and `\"` are
    escapes, and the quote is looked for in the raw line, ` #` included).
    So does a flow collection (`[…]`, `{…}`) that does not end on its line;
  - a mapping written in flow style fails: `{…}` other than `{}`, and
    `key: value` (or `"key":value`) inside `[…]`. A flow sequence of
    scalars, such as `[main]`, is read;
  - an alias or anchor as a value fails: `key: *name`, `key: &name`,
    `- *name`, `<<: *name`, a value on the line after its key, and one
    inside `[…]`. YAML would copy a job, say, without the check crediting
    its permissions to it;
  - a tag (`!…`) or an anchor before one of the keys it reads, an explicit
    key (`? `), and a double-quoted key with an escape (`"u\x73es":`)
    fail;
  - a YAML document marker or directive fails (`---` after the document
    has started, `...`, `%YAML`); one `---` before the document starts it.
  These look at a node's start only, so the text of a `run:` script
  (`exec 2>&1`, `>&2`, `*[!0-9a-f]*`, `tr -d ' \n'`) passes. A script line
  that looks like one of them where a node starts, such as `key: *name`,
  fails.
- A violation names the file, and the line where it has one. What it
  quotes from the workflow is escaped like every line `xtask` prints
  ("Control characters in output" above). Notes give the number of
  workflows checked.
- Not read, but failed: a `uses:` or a `permissions:` key in a flow
  mapping (`- { uses: … }`, `job: { permissions: write-all }`), a value of
  either written as a block scalar (`uses: >-`, `permissions: |`), and the
  forms above. Text that looks like a key inside a block scalar, such as
  `uses:` in a `run:` script, is read as one, so it fails rather than
  passes.
- Not covered: the check is not a YAML parser, and a form that a YAML
  reader accepts beyond those above, and that this check reads otherwise,
  would not be seen; ADR-0022 accepts that cost of reading lines
  (`.claude/rules/ci-workflows.md` → "One key a line"). Nor are the steps
  of a local or composite action; whether each row's workflow exists,
  which a unit test checks for the real repository; and races, as in
  "Files searched".

**`ucd`**

- Inputs: the five files in `crates/text/ucd/17.0.0/` (ADR-0018). They are
  `GraphemeBreakProperty.txt`, `emoji-data.txt`,
  `DerivedCoreProperties.txt`, `GraphemeBreakTest.txt` (read only by
  `tidepool-text`'s tests) and `license.txt`, each pinned by its SHA-256 in
  `xtask/src/ucd.rs`. Before anything is parsed, each must be a regular
  file (a symbolic link is never followed), at most 4 MiB, UTF-8, and match
  its pin. Otherwise it is a violation, and nothing is parsed or written.
  `.gitattributes` keeps git from converting their line endings, which
  would break the pins.
- It packs three properties into one byte per code point, using the
  constants of `class` in `crates/text/src/pos/grapheme.rs`:
  - `Grapheme_Cluster_Break` in bits 0–3;
  - `Indic_Conjunct_Break`, from the `InCB` lines, in bits 4–5;
  - `Extended_Pictographic` in bit 6.
- Each file's first line and its `@missing` default (for `emoji-data.txt`,
  its `# Version:` line) must be the expected ones, and every data line
  must parse. No code point may get one property twice. Each
  `# Total code points: N` line must count the code points kept since the
  previous one, unless none were. Two more things are errors, because the
  grapheme engine relies on them: a code point that is both ZWJ and
  Extended_Pictographic, and a U+000A or U+000D that is not exactly LF or
  CR.
- Output: `crates/text/src/pos/grapheme/tables.rs`. It holds a header
  naming the three sources, their SHA-256s and the Unicode license, then
  `UNICODE_VERSION`, then the sorted ranges of code points whose
  properties are not all defaults, one per line. It has LF line endings
  and the same bytes on every run and OS.
- Without `--check`, it writes the file only when it differs: to a new
  `tables.rs.tmp` beside it (a leftover one is removed first), which is
  then renamed over it. A linked or non-regular output is a violation and is
  not replaced. With `--check` (the `ci` step), a missing, linked or
  different output is a violation that names the first different line.
- Directories: each directory below the repository root on the way to the
  inputs or the output must be a real directory. That is `crates`,
  `crates/text`, `crates/text/ucd`, `crates/text/ucd/17.0.0`,
  `crates/text/src`, `crates/text/src/pos` and
  `crates/text/src/pos/grapheme`. They are checked shallowest first,
  before anything is read.
  - A link is the violation `<dir> is a symbolic link, which ucd does not
    follow`; anything else that is not a directory is `<dir> is not a
    directory`. Either is reported once and stops the run.
  - Without `--check`, the output's directories are checked again just
    before writing. A missing one is made one level at a time, and each is
    checked again once made.
- It opens no sockets and starts no process. Its parser is not a fuzz
  target, because its inputs are pinned (ADR-0018).
- Not covered:
  - Races. std has no `openat`, so a directory swapped for a link between
    the check and the open is followed. A link committed to the repository
    cannot race.
  - Hard links are not detected.

**`ci`**

- Runs, in this order: `deps`, `cargo fmt --all -- --check`, `cargo clippy
  --workspace --all-targets -- -D warnings`, `cargo test --workspace`,
  `layers`, `crates`, `features`, `unsafe-audit`, `headers`, `workflows`,
  `ucd` (as `ucd --check`). Each step
  starts with a `== <step>: ...` heading; the cargo steps print cargo's own
  output. `cargo xtask site` runs the same steps, in-process, for the demo
  page ("Demo page", below).
- It keeps going after a failure, so one run reports every problem, with
  one exception. Once `deps` has failed or could not run, clippy and test
  are not run, because they would build and run the code `deps` rejected
  (build scripts, proc macros, tests). `fmt` only reads source, so it still
  runs. `layers`, `crates`, `features`, `unsafe-audit`, `headers`,
  `workflows` and `ucd` are not gates, and run after clippy and test. So code that `deps` accepts but they reject,
  such as a vendored crate that cargo made a member, is still compiled, and
  its build scripts and tests run, before they report it.
- The cargo steps are cargo children like the others: offline, with
  standard input closed. The test step's output passes through `xtask` as
  it arrives, so by default cargo and libtest print it without colour, and
  the two streams may interleave differently.
- A line of the test step's output, on either stream, that starts with
  `SKIPPED:` (after any indentation) is a skipped test. After the step each
  reason is printed as `skipped: <reason>` (up to 1000; the rest are
  counted), and the step line reads `test: ok (2 SKIPPED)` or `test:
  FAILED (1 violation; 2 SKIPPED)`. A skip does not fail `ci`; it is listed
  and counted, not passed. A test reports one by writing it straight to
  `std::io::stderr()`: libtest captures `println!`/`eprintln!` from a
  passing test, so `ci` never sees those. A write straight to
  `std::io::stdout()` is seen too, but libtest writes its own lines there,
  so such a write should start with a newline. Prefer standard error.
- It ends with a summary: each step's result and time (the test row with
  its `(N SKIPPED)`), "not run (deps failed)" where that applies, and
  `tools` and `theme` as "not yet implemented". The last line is `ci: ok:
  N of N steps passed` or `ci: FAILED: M of N steps failed: <steps>`, then
  `; not run: <steps>` if any, and `; not yet implemented: tools, theme`
  while those are pending. After skips, `(1 test SKIPPED)`, `(2 tests
  SKIPPED)` and so on follow `ok` or `FAILED`, as in `ci: ok (2 tests
  SKIPPED): 11 of 11 steps passed; not yet implemented: tools, theme`.
  Scripts should rely on the exit status, which is 0 only if every step
  passed.

## CI (GitHub Actions)

- `.github/workflows/ci.yml`: matrix Linux, macOS, Windows; `permissions:
  contents: read`; actions pinned by full commit SHA with the tag in a
  comment; cache keyed on the lockfile and toolchain; installs the
  toolchain and runs `cargo xtask ci`, then, on Linux only, `cargo xtask
  fuzz-smoke --secs 30` (30 s per target; it runs only if the gate
  passed), all three with workflow commands paused (below).
- **Workflow commands are paused** (M0.11) in the toolchain step, whose
  rustup reads `rust-toolchain.toml` and can quote it, and in the gate's
  and the fuzz-smoke steps, which run the repository's code; and so in
  `pages.yml`'s toolchain step and the step that reads the commit with
  `git log` and runs `cargo xtask site` ("Demo page", below). Each step
  makes a token from 16 random bytes, and fails before running anything
  if that is not 32 lowercase hex digits. It joins its
  stderr to stdout (`exec 2>&1`), prints `::stop-commands::<token>`, runs
  its commands, then prints `::<token>::`. In between, the runner acts on
  no workflow command they print, whether `::warning::`, `::add-mask::` or
  `##[error]` (actions/runner v2.338.2,
  `ActionCommandManager.TryProcessCommand`).
  - Stderr joins stdout, bash's own messages included, because the runner
    reads the two pipes on separate tasks and handles each batch's stderr
    lines first (`src/Runner.Sdk/ProcessInvoker.cs`). Apart, a line on
    stderr could be handled before the stop line or after the resume.
  - Only the token resumes them: `::<token>::` at a line's start, after
    any whitespace, or `##[<token>]` at a line's first `##[`, in any case.
    So each step makes its own, in a shell variable it unsets first and
    never exports, and the workflow's text never holds it. The runner masks
    it in the log, as it does any token over 6 characters.
  - A pause ends with its step, so a step that fails needs no resume. Each
    step gets its own handler (`src/Runner.Worker/ActionRunner.cs`), and
    each handler its own command manager (`Handlers/Handler.cs`, through
    `HostContext.CreateService`, which makes a new one on every call).
  - Not covered:
    - A paused line still goes to the log as printed, and the runner
      writes its own groups and messages as lines with the same prefixes.
      So in GitHub's log view a line starting `##[group]` or `##[endgroup]`
      may fold the lines after it, and one starting `##[error]` or
      `##[warning]` may look like the runner's (not checked against the
      viewer), though no annotation, mask or other command follows. The
      runner escapes only `##[start-action` and `##[end-action`
      (`Handlers/OutputManager.cs`).
    - Output from a process that outlives its step's commands can arrive
      after the resume, and is not paused.
    - The pause keeps text from acting as a command. It does not contain
      the code the gate, the fuzz-smoke step and `pages.yml`'s build step
      build and test, or a
      `path` toolchain that `rust-toolchain.toml` names, which run as the
      job. That code can
      write the files the runner reads after a step, even one that failed
      (`ActionRunner.cs`, in a `finally`): `$GITHUB_ENV` and `$GITHUB_PATH`
      reach the later steps, the actions' post steps among them, and
      `$GITHUB_OUTPUT` and `$GITHUB_STEP_SUMMARY` the step's outputs and
      the job's summary. And it can
      read the token from its parent shell: GitHub's hosted runners give
      the job passwordless `sudo` on Linux and macOS, and run it as an
      administrator on Windows (GitHub's docs, "GitHub-hosted runners" →
      "Administrative privileges").
- **Supply chain of CI itself** (ADR-0022, ADR-0030): each workflow uses
  only the actions its row allows (`ci.yml`: `actions/checkout`,
  `actions/cache`; `codeql.yml`: `actions/checkout`,
  `github/codeql-action/init` and `analyze`; `pages.yml`:
  `actions/checkout`, `actions/cache`, `actions/upload-pages-artifact` and
  `actions/deploy-pages`), each pinned by full commit SHA with its tag in
  a comment; top-level permissions are `{}` or `contents: read`, and only
  two jobs may write: CodeQL's analysis job, to `security-events`, and
  `pages.yml`'s deploy job, to `pages` and `id-token`; no
  `pull_request_target`; every checkout sets `persist-credentials: false`.
  A job may still be granted a read, such as `pages.yml`'s gate's
  `actions: read`. `cargo xtask workflows`
  checks this in `ci` ("`workflows`" above). `codeql.yml`'s pins, and how
  they were looked up, are in roadmap M0.8's note; `ci.yml`'s
  (`actions/checkout`, `actions/cache`) are in M0.4's; `pages.yml`'s are
  in M0.14's.
- **Code scanning** (`.github/workflows/codeql.yml`, ADR-0022): CodeQL
  scans `rust` and `actions` (the workflows) with `build-mode: none` and the
  `security-extended` queries, on pushes to `main`, on pull requests,
  weekly (Saturday, 09:35 UTC) and by hand. The token's top-level
  permissions are `{}`; its job, `analyze`, gets `contents: read` and
  `security-events: write`, to upload its results. The checkout sets
  `persist-credentials: false`, and there are no `run:` steps.
  - `build-mode: none` means no build step, not no cargo. In the analyze
    step, which holds that token, the Rust extractor of CodeQL CLI 2.27.3
    (the one codeql-action v4.39.1 pins) loads the workspace through
    rust-analyzer's project model (`ra_ap_*` 0.0.357): `cargo metadata`,
    with `--no-deps` and then in full, and, with no build-script command
    configured, `cargo check --workspace --keep-going` with
    `--compile-time-deps --all-targets` on cargo 1.89 and later, which
    compiles and runs build scripts and compiles proc macros (with their
    dependencies). Tidepool has no build script, proc macro or
    dependency, so that compiles nothing today. The job sets
    `CARGO_NET_OFFLINE`, as `ci.yml` does, so a registry or git
    dependency fails there instead of being fetched and built. Established
    from codeql's `rust/extractor/src/config.rs` at `codeql-cli/v2.27.3`,
    `ra_ap_project_model` 0.0.357 (`build_dependencies.rs`,
    `cargo_workspace.rs`), cargo's documentation of `--compile-time-deps`,
    and the 2026-11-02 run's log, which shows no build-script command, the
    toolchain `rust-toolchain.toml` pins, and 89 of 89 files extracted. That
    log does not show the cargo command lines.
  - CodeQL's `actions` queries catch, among others, injection, untrusted
    checkouts, cache and artifact poisoning, unpinned actions of owners
    other than `actions`, `github` and `advanced-security`, and a job with
    no `permissions:` block. GitHub's own actions pinned by tag, and
    `write` grants, are caught only by `cargo xtask workflows`.
  - It has `ci.yml`'s concurrency rule: a newer run for the same pull
    request, or the same branch, cancels the older one, so on `main` the
    pushes and the weekly run share a group.
- **Dependabot** (`.github/dependabot.yml`, ADR-0022) updates the actions'
  pins and tag comments: `github-actions` only, weekly, for releases at
  least 7 days old (`cooldown`), with the week's updates in one pull
  request whose commits start `ci`. It has no `cargo` entry.
- **Demo page** (`.github/workflows/pages.yml`, ADR-0030, M0.14):
  publishes <https://jabbott.dev/Tidepool/> through GitHub Pages, whose
  source must be "GitHub Actions" in the repository's settings. Until the
  user switches it, Pages' Jekyll build serves the root `README.md` there
  (`build_type` `legacy`, from `main`, `/`). Nadir's demo page
  (<https://jabbott.dev/Nadir/>, its ADR-0029) is the model.
  1. **When.** It runs after CI succeeds on a push to
     `jabbott-iii/Tidepool`'s `main` (`workflow_run`), or by hand from
     `main`. A first job, `gate`, which waits for nothing, stops there if
     `main` has moved past the commit: that commit's own run publishes, so
     the page never goes back to an older commit. A run by hand also stops
     there unless CI has a successful run of a push of that commit to
     `main` (`actions: read`). A run from a fork or a pull request never
     gets past it. Only then does a run join the queues below, so a run
     that will not publish never displaces one that will.
     - The build and the deploy each have a queue of their own: one runs
       at a time, and one that waits is replaced by the next one let
       through, a newer commit's. The build checks `main` again when it
       starts, since it may have waited.
     - The build and the deploy run only in the attempt whose gate let
       them through: a re-run of only the failed jobs, which keeps the
       gate's old answer, skips them. "Re-run all jobs", which asks the
       gate again, or a run by hand is how to retry. Each attempt uploads
       an artifact of its own, `github-pages-<attempt>`, since an artifact
       belongs to the whole run and `actions/deploy-pages` refuses two of
       one name (Nadir's ADR-0029).
     - When the newest commit fails CI, the page waits for the next one
       that passes.
  2. **The build**, on `ubuntu-latest` with `contents: read` and
     `CARGO_NET_OFFLINE: "true"`, checks out the commit CI passed, with
     `persist-credentials: false`, installs the pinned toolchain with
     rustup, and restores its cache, which only `main`'s runs write: a
     run on the default branch restores no pull request's cache (GitHub's
     docs, "Dependency caching"). Then one step, with workflow commands
     paused (above), reads the checked-out commit's hash (`%H`), committer
     date (`%cs`) and subject (`%s`) with `git log -1`, and runs `cargo
     xtask site` with them in `TIDEPOOL_SITE_COMMIT`, `TIDEPOOL_SITE_DATE`
     and `TIDEPOOL_SITE_SUBJECT`, set for that command alone. They never go
     through `$GITHUB_ENV` or `$GITHUB_OUTPUT`, where a subject could reach
     later steps. `actions/upload-pages-artifact` uploads `target/site`.
  3. **The deploy**, the only job with `pages: write` and `id-token:
     write`, publishes the artifact with `actions/deploy-pages`. It checks
     nothing out and runs none of the repository's code.

  **`cargo xtask site [--out DIR]`** builds the page into `target/site`,
  or `DIR`, made absolute as `--root` is. `DIR` must be new, empty, or
  made by an earlier run, which leaves the mark `.tidepool-site`, and a
  run removes only what an earlier one wrote (Nadir's `MARK` and
  `WRITTEN`), so `--out` can never empty a directory of anything else.
  `site` writes only files there, so a directory (not a link) at one of
  those names is not its own: `site` fails, naming it, before anything
  runs, and removes nothing. A link there is removed as a link, never
  followed. It runs no program but `cargo` and makes no network calls;
  the page loads jabbott.dev's files when it is viewed. In order:
  1. It checks `docs/features.jsonc` as `features` does ("`features`"
     above).
  2. It runs the gate in-process: the steps of `ci`, in its order, with
     the same rules and output. It records each step's verdict and time,
     and the tests passed, failed and ignored, summed from libtest's `test
     result:` lines, and the `SKIPPED:` lines, counted as `ci` counts
     them. It keeps no other line of the output.
  3. It runs each example the page shows, `cargo run --quiet -p <package>
     --example <name>` (a cargo child as above), and keeps its standard
     output: `tidepool-codec`'s `formats`
     (`crates/codec/examples/formats.rs`) and `tidepool-text`'s `editing`
     (`crates/text/examples/editing.rs`). An example runs on fixed inputs
     written in its source and prints only what follows from them, never
     the time, an absolute path or anything else of the machine or the
     environment, so its tab says the same wherever the page is built. An exit
     status other than 0, or output that is not UTF-8, fails the build.
  4. It fills `site/index.html` and `site/llms.txt`, whose placeholders
     are names in double braces, each defined in the comment at the top of
     `site/index.html`, and copies `site/tidepool.css` and
     `site/tidepool.js` beside them. Every value filled into the HTML is
     escaped for HTML; into `llms.txt`, its control characters are escaped
     as `xtask`'s messages escape them ("Control characters in output",
     above).

  If any of these fails, `site` fails and writes nothing, so the page
  from an earlier run stays as it was.

  **The commit's facts.** `xtask` starts only `cargo` (ADR-0017), so the
  workflow reads them with `git` and passes them in:

  | Variable | Holds | Valid |
  | :- | :- | :- |
  | `TIDEPOOL_SITE_COMMIT` | The commit's hash | 40 lowercase hex digits |
  | `TIDEPOOL_SITE_DATE` | Its committer date | `YYYY-MM-DD` |
  | `TIDEPOOL_SITE_SUBJECT` | Its subject line | One line, at most 200 characters once trimmed, no control characters (nor line or paragraph separators or bidirectional controls) |

  All three are set, or none is. One or two, or a value that is not
  valid, fails `site`, naming the variable and never showing the value
  unescaped. With none, a local build, `site` reads HEAD's hash from
  `.git` itself: `.git/HEAD`, then the loose ref it names or the ref's
  line in `packed-refs`, without following a link and without running
  `git`. The page then shows the hash, no date and no subject. The gate,
  the examples, the features and the counts are the working tree's, which
  may hold changes HEAD does not, so where a published page gives the
  subject, a local one says "A local build of the working tree at HEAD; it
  may hold changes HEAD does not.", and `llms.txt` says the same. It names
  who ran the gate and the examples as "a local build of the working
  tree", and marks the features from the ticks "at this build", not at the
  hash. Where HEAD cannot be read that way, as in a linked worktree, whose
  `.git` is a file, the page names the commit unknown and says "A local
  build of the working tree; its HEAD could not be read from `.git`.",
  and `site` prints why (ADR-0030). A blank subject, as `git commit
  --allow-empty-message` makes, is shown as "a commit with no subject".

  **The page.** Its main panel has three tabs: the gate (each step, its
  verdict and time, and the counts, the skips apart from the passes; a
  line saying where and by whom it ran, from `{{platform}}` and
  `{{runner}}`, which in a published page name the Pages job's Linux
  runner; and the caption's own sentence that the page is published only
  from a commit of `main` that passed CI on Linux, macOS and Windows), and
  what each example printed. Below them are the features from `docs/features.jsonc`, each marked
  shipped, in progress or planned by the roadmap's ticks. `site/llms.txt`
  says the same in plain text for language models, served as it is. The
  page loads jabbott.dev's own stylesheet
  (`https://jabbott.dev/assets/css/style.css`) and script
  (`https://jabbott.dev/assets/js/site.js`) and Google Fonts as that site
  does, and its header and footer repeat that site's, so a change to that
  site's markup may need `site/index.html` to follow. `site/tidepool.css`
  adds the page's own components with that site's tokens, and
  `site/tidepool.js` its own behaviour, such as the tabs. jabbott.dev's
  Deployments menu finds the page from GitHub by itself.

  **What it publishes.** Everything on the page is public. The gate's tab
  holds no line that cargo or a test printed, so a path, a user name or a
  value printed during the gate is never published, and the examples
  print only what their fixed inputs give. The subject is the one text
  from outside the code: checked, then escaped. Visitors' browsers fetch
  jabbott.dev's stylesheet and script and Google's fonts, with no
  integrity hashes, since the page follows the live files.

  Not covered: `cargo test` builds the examples but does not run them, so
  one that fails when run fails only the Pages build, after CI passed;
  and the build runs the gate a second time, on one OS, so the page shows
  that run's times and counts, not CI's.
- Goldens run on Linux CI only until M11/M12 add per-OS references for
  platform-specific scenes (the CPU rasterizer is deterministic, so shared
  scenes share references).
- Nightly (later): real-tool jobs. Android on Linux (SDK installed by
  Tidepool's own `toolchain` code, licenses accepted by a human-reviewed
  workflow input), iOS on macOS runners with Xcode.

## Unsafe and FFI review checklist

Use for every change to `sys` or `platform` (the `security-reviewer` agent
runs it).

1. Declarations live only in `sys`, each citing a header or doc URL.
2. Every `#[repr(C)]` type has `size_of`/`align_of` asserts per target.
3. One unsafe operation per `unsafe` block, each with `// SAFETY:`.
4. Every `pub unsafe fn` has a `# Safety` section.
5. Ownership of every returned pointer is documented and wrapped in RAII.
6. Every return code is checked and converted to a typed error.
7. Objective-C calls go through `msg_send!` with the exact signature.
8. Signal handlers do only async-signal-safe work (write to a self-pipe).
9. Child processes are spawned with an argv vector, never through a shell.
10. If a check can only run on another OS, the summary says which.
