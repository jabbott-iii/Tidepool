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
| Fuzz smoke | `cargo xtask fuzz-smoke` | No parser panics, hangs or over-allocates on mutated input |
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

- `cargo xtask fuzz-smoke [--secs N] [--target T]` mutates each corpus in
  `fuzz/corpus/<target>/` (bit flips, splices, length edits, dictionary
  tokens) and runs the parser in-process.
- Targets: JSON, TOML, XML, plist, YAML, CSV, INI, DEFLATE, ZIP, PNG, QOI,
  TrueType, Markdown, regex compile, VT parser, JSON-RPC framing, LSP
  messages, TAP messages, git porcelain v2, cargo JSON diagnostics, tool
  version strings.
- A crash is saved to `fuzz/crashes/<target>/` and becomes a regression
  test.

### Benchmarks — `tidepool-bench`

- Warm-up, N samples, median and p95, JSON baseline in `bench/baseline.json`.
- Suites: rope edits, position conversion, highlighting a 10k-line file,
  project search, frame build + raster at 1080p, glyph atlas fill, startup.
- A change that slows a suite by more than 10% against the baseline must
  say so and why.

### Responsiveness

- A scripted driver feeds keystrokes to `app` with a fake LSP server that
  waits 2 s per request and a mock agent that waits 2 s, and records
  keystroke-to-present time. Budget: p99 ≤ 16 ms (`docs/00-overview.md`).

## `xtask` checks

| Command | Fails when |
| :- | :- |
| `cargo xtask ci` | Any of the checks below, `cargo fmt --check`, clippy `-D warnings`, or tests fail |
| `cargo xtask deps` | `Cargo.lock` (any workspace) has a non-workspace package not in `xtask/deps-allowlist.txt` |
| `cargo xtask layers` | A crate depends on a crate its matrix row doesn't allow |
| `cargo xtask unsafe-audit` | `unsafe` outside `sys`/`platform`, a block without `// SAFETY:`, or `#![allow(unsafe_code)]` elsewhere |
| `cargo xtask tools` | A `toolchains/*.toml` descriptor lacks a pinned version, HTTPS official source, SHA-256 (for downloads) or license |
| `cargo xtask theme` | A theme token pair used for text falls below its contrast minimum (`docs/subsystems/ui-and-theme.md`) |
| `cargo xtask golden [--bless]` | A golden frame or TUI snapshot differs |
| `cargo xtask fuzz-smoke` | A fuzz target crashes, hangs or exceeds its allocation cap |
| `cargo xtask bench [filter]` | (Reports only) |

`xtask` never uploads, publishes, signs with real credentials, reads
secrets or calls the network.

## CI (GitHub Actions)

- `.github/workflows/ci.yml`: matrix Linux, macOS, Windows; `permissions:
  contents: read`; actions pinned by full commit SHA with the tag in a
  comment; cache keyed on the lockfile and toolchain; runs `cargo xtask ci`
  plus a Linux fuzz-smoke step (30 s per target).
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
