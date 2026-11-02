# Roadmap

How to use this file:

- Work top to bottom. A task is **unblocked** when everything in its
  `Depends` line is checked.
- Tick a box only after its acceptance criteria are met **and** its
  verification commands were run and passed (see "Definition of done" in
  `AGENTS.md`).
- If a criterion needs an OS, tool or device you can't run, leave the box
  unchecked. Add an indented note saying exactly what remains:
  `  - ⏳ needs macOS: run the iOS sample through codesign --verify`.
- A `Decide first` line is a question for the user. Ask it before writing
  code for the task, and record the answer with `/record-decision`: accept
  the `Proposed` ADR it names, or add or extend one.
- **Task IDs never change.** Code, docs and commit messages cite them. A
  task added later takes the next free number in its milestone and is
  listed where it is to be done, so numbers are not always in order. A
  split task keeps its ID on the part that finishes what it promised, so
  every `Depends` line that names it stays true.
- Exit criteria for a milestone are checked by a human.
- OS order: **Linux → macOS → Windows** (ADR-0015).
- The roadmap was re-planned on 2026-11-02 from the state of the code
  (ADR-0024): larger tasks were split, missing work was added as tasks, and
  questions that finished tasks left open became `Decide first` lines.

---

## M0 — Foundation and guardrails

**Goal:** an empty but fully guarded workspace. The constraints are
enforced by tooling from day one.

- [x] **M0.1 — Workspace skeleton**
  - Depends: —
  - Do:
    - Create every crate in the workspace map (`AGENTS.md`) as an empty
      library or binary with a crate-level doc comment.
    - Root `Cargo.toml`: `[workspace]` (resolver 3), `[workspace.package]`
      (edition 2024, version 0.1.0, license Apache-2.0), and
      `[workspace.lints]` exactly as in `docs/01-architecture.md`.
    - `rust-toolchain.toml` pinning the current stable with `rustfmt` and
      `clippy`; record the version in a task note.
    - `clippy.toml`, `.cargo/config.toml` with the aliases.
    - `#![allow(unsafe_code)]` in the `sys` and `platform` roots only.
  - Accept:
    - `cargo build --workspace` and
      `cargo clippy --workspace --all-targets -- -D warnings` succeed.
    - `Cargo.lock` lists only workspace packages.
  - Verify: `cargo build --workspace`;
    `cargo clippy --workspace --all-targets -- -D warnings`;
    `sh scripts/check-deps.sh`
  - Note: toolchain pinned to 1.99.0 (rustc 1.99.0 b940084d7 2026-09-28;
    clippy 0.1.99; rustfmt 1.10.0-stable).

- [x] **M0.2 — core: errors, logging, test harness, hashing, CLI, time**
  - Depends: M0.1
  - Do: `error`, `log` (with redaction), `testing::{check, check_sized}`,
    `rng::Pcg32`, `hash::{fnv1a64, crc32, sha256}`, `cli`, `civil`, `geom`,
    `color`, `input`, `cancel`, per `docs/subsystems/core.md`. Port from
    Nadir's `nadir-core` where it fits.
  - Accept:
    - SHA-256 matches the FIPS 180-4 vectors; `crc32("123456789") ==
      0xCBF43926`.
    - A failing `check` prints a reproducible seed.
    - A registered secret never appears in any log sink.
  - Verify: `cargo test -p tidepool-core`

- [x] **M0.3 — xtask: deps, layers, unsafe-audit, ci**
  - Depends: M0.2, M1.4
  - Do:
    - `cargo xtask deps` reads `cargo metadata` with `codec::json`; fails on
      any non-null `source` not in `xtask/deps-allowlist.txt` (empty).
    - `cargo xtask layers` checks the matrix in `docs/01-architecture.md`
      (parse the table from the doc, so the doc is the source of truth).
    - `cargo xtask unsafe-audit` per `docs/04-quality.md`.
    - `cargo xtask ci` runs fmt check, clippy, tests and the checks above.
    - Switch `.claude/hooks/deps-guard.sh` to `cargo xtask deps`; keep
      `scripts/check-deps.sh` as a fallback when `xtask` doesn't build.
  - Accept: each check fails on a deliberately broken fixture workspace
    generated in a temp dir, and passes on the real workspace.
  - Verify: `cargo xtask ci`
  - Note (2026-11-01): `cargo xtask ci` passes locally: 6 of 6 steps, 566
    tests, 0 failed, 0 `SKIPPED:`. It also passes with the CI workflow's
    environment (`CARGO_TERM_COLOR=always`). The xtask suite passes with
    `CARGO_TARGET_DIR` outside the repository. Each check's fixtures are in
    `xtask/tests/fixtures.rs`, generated under `CARGO_TARGET_TMPDIR`; the
    rules are in `docs/04-quality.md` → "`xtask` checks".
    - The hook runs `xtask deps` and then `xtask layers` (layers goes beyond
      the task text). It builds `xtask` into `target/deps-guard/` and
      falls back to `scripts/check-deps.sh`.
    - `check-deps.sh` now resolves with `--locked`, so it fails on an out
      of date or missing lockfile instead of writing one.
    - Running cargo from `xtask` is ADR-0017, accepted on 2026-11-01 and
      named as the exception in `AGENTS.md` rule 2.

- [x] **M0.4 — CI workflow**
  - Depends: M0.3
  - Do: `.github/workflows/ci.yml` per `docs/04-quality.md`: matrix of
    3 OSes, `permissions: contents: read`, actions pinned by full SHA with
    the tag in a comment, cache keyed on lockfile + toolchain, runs
    `cargo xtask ci`.
  - Accept: valid YAML; no unpinned actions. If SHAs can't be looked up in
    this session, use `# TODO(pin): actions/checkout@vX` placeholders and
    list them for a human.
  - Verify: parse the YAML (state how)
  - Note (2026-11-01): parsed with PyYAML 6.0.1 (`python3 -I`,
    `yaml.safe_load`). Both actions are pinned by full SHA, checked with
    `git ls-remote` on 2026-11-01: `actions/checkout` v7.0.1 =
    `3d3c42e5aac5ba805825da76410c181273ba90b1`, `actions/cache` v6.1.0 =
    `55cc8345863c7cc4c66a329aec7e433d2d1c52a9`, both lightweight tags. The
    Linux fuzz-smoke step waits for M1.6 (a `TODO(M1.6)` in `ci.yml`).
  - ⏳ needs human: push, and confirm `cargo xtask ci` is green on
    `ubuntu-latest`, `macos-latest` and `windows-latest` (the M0 exit
    criterion). Nothing ran on macOS or Windows in this session.

- [ ] **M0.5 — Budgets and reference machine**
  - Depends: M0.1
  - Do: ask the user for the reference machine; record it and confirm or
    adjust every budget in `docs/00-overview.md`.
  - Accept: the budgets table says "confirmed" with the machine named.
  - Verify: doc review (human)
  - ⏳ needs human: review the reference machine and "Target (confirmed)"
    edit in `docs/00-overview.md` → "Performance budgets".

- [ ] **M0.6 — Contribution workflow and crate READMEs**
  - Depends: M0.3
  - Do:
    - `CONTRIBUTING.md`: the workflow (an issue first), setup, commands,
      checks, conventions, docs, commits, coding agents, license.
    - `SECURITY.md` in place of GitHub's template: supported versions,
      private reporting, what to expect, scope.
    - `.github/pull_request_template.md`, and the issue form
      `.github/ISSUE_TEMPLATE/issue.yml` with a `config.yml` that turns
      blank issues off and links private vulnerability reporting.
    - A `README.md` in every workspace member, in the shape of ADR-0023.
  - Accept:
    - Both issue-template files parse as YAML, and the form has the
      top-level keys GitHub's issue forms require (`name`, `description`,
      `body`).
    - Every member has a README, and its status agrees with the ticks in
      this file.
    - Private vulnerability reporting is turned on in the repository's
      security settings, so the links in `SECURITY.md` and `config.yml`
      work (human).
    - The user has reviewed the texts (human).
  - Verify: parse the YAML (state how); doc review (human)
  - Note (2026-11-02): the files were written in the documentation rebuild
    of 2026-11-02 (ADR-0024).
  - ⏳ needs human: review the texts; accept ADR-0023 (crate READMEs) and
    ADR-0024 (this re-plan), or say what to change; and turn on private
    vulnerability reporting for `jabbott-iii/Tidepool`, which `SECURITY.md`
    and the issue form link to.

- [ ] **M0.7 — License headers: `cargo xtask headers`; the RFC 8259 notice**
  - Depends: M0.3
  - Decide first:
    - accept ADR-0021 (license headers), which is `Proposed`;
    - whether to add a root `NOTICE` file, as Nadir has (ADR-0021 leaves
      it open).
  - Do:
    - Put ADR-0021's header on every source file it covers: today the Rust
      files, `scripts/check-deps.sh`, `.claude/hooks/deps-guard.sh` and the
      two workflows. The generated grapheme tables
      (`crates/text/src/pos/grapheme/tables.rs`) keep the Unicode
      license's notice instead and are exempt.
    - Put the IETF Trust's legend for the RFC 8259 examples beside them, in
      `crates/codec/tests/json.rs` (ADR-0021).
    - `cargo xtask headers`, run by `ci` after `unsafe-audit`, per
      `docs/04-quality.md`; a unit test compares its text with `LICENSE`'s
      appendix.
    - The comments above `#![allow(unsafe_code)]` in `crates/sys/src/lib.rs`
      and `crates/platform/src/lib.rs` still say `unsafe-audit` "(M0.3)
      will enforce" the rule; say that it does, while those lines move for
      the header.
  - Accept:
    - Fixture repositories fail for a missing header, an altered line, a
      header below other text and a missing blank line after it; a header
      after a `#!` line passes.
    - The real repository passes, and `cargo xtask ucd --check` still
      passes.
  - Verify: `cargo xtask headers`; `cargo xtask ci`

- [ ] **M0.8 — CI supply chain: CodeQL pinned, Dependabot for actions, `cargo xtask workflows`**
  - Depends: M0.4
  - Decide first: accept ADR-0022 (CodeQL, Dependabot and the workflow
    check), which is `Proposed`.
  - Do:
    - Rewrite `.github/workflows/codeql.yml` per ADR-0022: `actions/checkout`
      and `github/codeql-action/init` and `analyze` pinned by full commit
      SHA with the tag in a comment; top-level `permissions: {}`; the job
      gets `contents: read` and `security-events: write` only;
      `persist-credentials: false`; languages `rust` and `actions` with no
      build; the `security-extended` queries; `ci.yml`'s concurrency rule.
    - Replace `.github/dependabot.yml`, whose empty `package-ecosystem`
      updates nothing, with the weekly `github-actions` entry of ADR-0022.
    - `cargo xtask workflows`, run by `ci`, per `docs/04-quality.md` and
      its table of the actions each workflow may use.
    - Look up each SHA with `git ls-remote` and record it in a task note.
  - Accept:
    - Against a fixture copy of the `codeql.yml` committed on 2026-11-01
      (GitHub's template), the check reports each of its five problems:
      three actions pinned by tag, no top-level `permissions`, and a
      checkout without `persist-credentials: false`.
    - The rewritten workflows pass, and `dependabot.yml` parses.
  - Verify: `cargo xtask workflows`; `cargo xtask ci`; parse the YAML
    (state how)
  - ⏳ needs human after the change: push, and confirm CodeQL's first run
    on `main` succeeds.

- [ ] **M0.9 — `cargo xtask crates`: the members' manifests and READMEs**
  - Depends: M0.3, M0.6
  - Decide first: accept ADR-0023 (crate READMEs and the manifest check),
    which is `Proposed`, if M0.6 has not.
  - Do:
    - Add `publish = false` to every member's `[package]` that lacks it
      (all but `xtask`).
    - `cargo xtask crates`, run by `ci`, per ADR-0023 and
      `docs/04-quality.md`: every member inherits `edition`, `version` and
      `license` from the workspace, sets `publish = false` and
      `[lints] workspace = true`, and has a `README.md` whose first heading
      is its package name, with `## Status` and `## Design` sections that
      name only task IDs this file has.
  - Accept: fixtures fail for each rule; the real workspace passes.
  - Verify: `cargo xtask crates`; `cargo xtask ci`

**Exit criteria (M0):** `cargo xtask ci` is green locally and on all three
CI runners, with `headers`, `workflows` and `crates` among its steps.
`Cargo.lock` contains zero third-party packages. CodeQL has run on `main`
with pinned actions.

---

## M1 — Text engine and formats (headless)

**Goal:** the data structures and parsers everything else stands on.

- [x] **M1.1 — text: rope and positions**
  - Depends: M0.2
  - Do: `rope`, `pos` (byte, char, line/column, UTF-16; grapheme
    boundaries from UCD tables generated by `xtask ucd`).
  - Accept: property test vs a `String` model with ≥ 10,000 random edit
    sequences; every conversion round-trips.
  - Verify: `cargo test -p tidepool-text`, `cargo test -p xtask`,
    `cargo xtask ucd --check`
  - Note: the Unicode 17.0.0 data is kept in `crates/text/ucd/` and
    `ucd --check` is a `ci` step (ADR-0018); positions follow ADR-0019.
    Every position is checked in tiny leaves; in real-size leaves, near
    every seam, on lines longer than a leaf, and sampled elsewhere. A
    grapheme query can still take 0.5–3 s on a crafted 100 MB line (one
    100 MB cluster, or a long run of marks or regional indicators), so M4
    must bound cursor motion or run it off the main thread.

- [x] **M1.2 — text: selections, transactions, undo tree**
  - Depends: M1.1
  - Do: `selection`, `transaction`, `history` per
    `docs/subsystems/text-and-formats.md`.
  - Accept: undo after any random transaction sequence restores the text
    exactly; redo branches are kept.
  - Verify: `cargo test -p tidepool-text`
  - Note: the edit and undo model follows ADR-0020.
    - **Acceptance tests:** a model that keeps every revision's text runs
      2,000 random sequences in tiny leaves, plus 40 at real sizes.
      - Each sequence mixes commits with every grouping, undo, redo,
        `jump_to`, saved markers and refused operations, with the apply
        path forced at random.
      - Every step's text is checked, hop by hop.
      - At the end, undo must give the first text, and every revision must
        be reachable with its exact text.
    - **Measured** (release; 10,000 lines of about 400 KB):
      - a 10,000-caret keystroke commits in about 1–2 ms, undo and redo
        about the same;
      - typing into 100 MB takes about 12–14 µs per character.
    - **Residual risk:** 10,000 carets on a 100 MB file take about 120 ms,
      over one frame. Each M1.1 in-leaf edit costs about 11 µs, because it
      recounts the leaf. M4.2 should batch such edits or make that edit
      cheaper.

- [ ] **M1.3 — text: encodings and files**
  - Depends: M1.1
  - Do: `encoding` (UTF-8, UTF-16 LE/BE with BOM; invalid UTF-8 opens
    read-only and lossless) and `file` (load with the 16 MB large-file
    threshold; atomic save: a temp file in the same directory, fsync,
    rename, permissions kept; line endings and the trailing newline
    preserved), per `docs/subsystems/text-and-formats.md`.
  - Accept: unchanged files with BOM, CRLF, mixed endings and no trailing
    newline round-trip byte-for-byte; a crash between write and rename
    leaves the original intact (simulated); a file that is not valid UTF-8
    is never written back.
  - Verify: `cargo test -p tidepool-text`

- [x] **M1.4 — codec: JSON and JSONC**
  - Depends: M0.2
  - Do: `codec::json` parser and writers with `Limits`. Port from Nadir's
    `nadir-codec::json`.
  - Accept: RFC 8259 cases; limits enforced; no panics on malformed input.
  - Verify: `cargo test -p tidepool-codec`
  - Note: done ahead of M0.3, which needs it. The supported subset is in
    `docs/subsystems/text-and-formats.md` → `json`. Its fuzz target and
    seed corpus come with M1.6.

- [ ] **M1.7 — codec: TOML 1.0 parser**
  - Depends: M1.4
  - Decide first: whether to vendor the cases of the `toml-test` suite as
    test-only fixtures, once their license is confirmed (as for the
    CommonMark examples in M6.5), or to write the compliance cases by hand.
  - Do: `codec::toml` parsing into a value tree with source spans, with
    `Limits` and errors that give the line and column (ADR-0013). Document
    the supported subset in `docs/subsystems/text-and-formats.md` → `toml`.
  - Accept: TOML 1.0 valid and invalid cases; limits enforced; no panics on
    malformed input.
  - Verify: `cargo test -p tidepool-codec`

- [ ] **M1.6 — xtask: fuzz smoke**
  - Depends: M0.3, M1.4, M1.7
  - Decide first (`docs/04-quality.md` → "Fuzz smoke" calls these
    provisional):
    - where the targets for parsers outside `xtask`'s matrix row (`core`,
      `codec`) live: a wider row for `xtask`, a dev-only target in each
      crate that the harness drives, or a separate dev-only member;
    - how to cap allocation, since a counting `GlobalAlloc` is `unsafe` and
      `unsafe` lives only in `sys` and `platform`, or whether each parser's
      `Limits` is enough;
    - whether `fuzz-smoke` runs anything but in-process code, which would
      extend ADR-0017.
  - Do:
    - `cargo xtask fuzz-smoke [--secs N] [--target T]` per
      `docs/04-quality.md`; targets for JSON and TOML with seed corpora in
      `fuzz/corpus/`; a crash is saved to `fuzz/crashes/<target>/` and
      becomes a regression test.
    - Add the Linux fuzz-smoke step (30 s per target) that
      `docs/04-quality.md` → "CI" requires to `.github/workflows/ci.yml`;
      M0.4 left a `TODO(M1.6)` there.
  - Accept: 60 s per target with no crash; a planted panic is found.
  - Verify: `cargo xtask fuzz-smoke --secs 60`

- [ ] **M1.5 — codec: TOML with a preserving document model**
  - Depends: M1.7
  - Do: the comment- and order-preserving document model of ADR-0013
    (accepted 2026-11-01) on top of M1.7's parser: read, change and add
    values, and write back.
  - Accept: TOML 1.0 compliance cases still pass; an unchanged document
    writes back byte-for-byte; editing a value and writing back preserves
    comments, order and formatting elsewhere.
  - Verify: `cargo test -p tidepool-codec`

- [ ] **M1.8 — core: path helpers**
  - Depends: M0.2
  - Do: `core::path` per `docs/subsystems/core.md`: UTF-8 path helpers,
    canonicalization and the display of paths that are not UTF-8. M0.2's
    `Do` line left it out.
  - Accept: a path that is not UTF-8 displays without hiding which bytes it
    holds (tested on Unix with bytes built in the test); canonicalization
    is tested in a temp directory under `target/`.
  - Verify: `cargo test -p tidepool-core`

- [ ] **M1.9 — bench: harness, `cargo xtask bench` and the text suites**
  - Depends: M0.3, M1.2, M0.5
  - Decide first: extend ADR-0017 for the child `cargo xtask bench` starts
    (a release build and run of `tidepool-bench` through cargo).
  - Do:
    - `tidepool-bench` per `docs/04-quality.md` → "Benchmarks": warm-up,
      N samples, median and p95, a JSON baseline in `bench/baseline.json`
      written with `codec::json`.
    - `cargo xtask bench [filter]` (reports only): `bench` leaves `xtask`'s
      list of commands not yet implemented (`LATER` in
      `xtask/src/check.rs`), whose message for it still says it has no task.
    - Suites for what M1.1 and M1.2 measured by hand: rope edits, position
      conversions, a 10,000-caret keystroke on 400 KB and on 100 MB, typing
      into 100 MB, and grapheme queries on a crafted 100 MB line.
  - Accept: two runs on the reference machine agree within 10% on every
    median; the baseline is committed with the machine named.
  - Verify: `cargo xtask bench text`

**Exit criteria (M1):** the text engine (files included) and JSON and TOML
(preserving writes included) pass their property tests and fuzz smoke, and
the text suites have a recorded baseline.

---

## M2 — A window and text on Linux

**Goal:** a Linux window that shows Saira and JetBrains Mono text, drawn by
our own rasterizer.

- [ ] **M2.1 — sys: dynlib, libc and X11 declarations**
  - Depends: M0.1
  - Decide first: accept ADR-0010 (X11 through `dlopen`'d libX11; Wayland
    through XWayland), which is `Proposed`.
  - Do: `dynlib!`, `linux::libc`, `linux::x11` per
    `docs/subsystems/sys-and-platform.md`, declaring only what M2.2 calls;
    later tasks add what they call. Follow `/add-ffi-binding`, and port
    Nadir's X11 declarations where they fit, with a note.
  - Accept: layout asserts for every struct; each declaration cites its
    header.
  - Verify: `cargo build -p tidepool-sys`; `cargo xtask unsafe-audit`

- [ ] **M2.2 — platform: window, events, waker, present (Linux)**
  - Depends: M2.1, M0.2
  - Decide first: where the Xvfb test's read-back with `XGetImage` lives.
    `unsafe` compiles only in the library targets of `sys` and `platform`,
    so an integration test cannot make the call (`docs/04-quality.md` →
    `unsafe-audit`): a `#[cfg(test)]` test inside `platform`'s library, or
    a safe read-back function that `platform` exports for tests.
  - Do: `window`, `events` (poll on the X fd and a self-pipe), `surface`
    (`XPutImage` of damage rects), per `docs/03-platforms.md` → Linux.
  - Accept: under Xvfb, a test opens a window, presents a framebuffer,
    reads the pixels back with `XGetImage`, and a `Waker` from another
    thread interrupts `wait_events` within 5 ms.
  - Verify: `xvfb-run cargo test -p tidepool-platform`;
    `cargo xtask unsafe-audit`

- [ ] **M2.7 — platform: clipboard and paths (Linux)**
  - Depends: M2.2, M1.8
  - Do: `clipboard` (`CLIPBOARD` and `PRIMARY`, `UTF8_STRING`, `TARGETS`,
    `INCR`), `paths` and `dirs`, per `docs/03-platforms.md` → Linux.
  - Accept: under Xvfb, text copied in one window pastes in another, and a
    1 MiB paste goes through `INCR`; `paths` follows the XDG variables and
    their defaults.
  - Verify: `xvfb-run cargo test -p tidepool-platform`;
    `cargo xtask unsafe-audit`

- [ ] **M2.3 — gfx: rasterizer, blending, clipping, damage, backdrop**
  - Depends: M0.2
  - Decide first: accept ADR-0011 (CPU software rendering with damage
    tracking), which is `Proposed`.
  - Do: `framebuffer`, `raster`, `blend`, `clip`, `image`, `glyph`,
    `damage`, `backdrop` per `docs/subsystems/rendering.md`.
  - Accept: unit tests check exact pixels for every primitive; property
    tests: drawing inside the clip never writes outside it, and merged
    damage covers every changed pixel; the same draw list gives the same
    bytes on two runs. Golden frames follow in M2.4, with the harness.
  - Verify: `cargo test -p tidepool-gfx`

- [ ] **M2.4 — codec: QOI; xtask: golden harness; gfx goldens**
  - Depends: M2.3, M0.3, M1.6
  - Decide first: how `cargo xtask golden` draws its scenes. If it runs test
    binaries through cargo, that extends ADR-0017's list of `xtask`'s
    children.
  - Do: `codec::qoi` (port Nadir's) with a fuzz target; `cargo xtask golden
    [--bless]` per `docs/04-quality.md`; golden frames for every `gfx`
    primitive and the backdrop.
  - Accept: a changed pixel fails with diff images in `target/golden/`;
    QOI round-trips, and rejects malformed input without panicking.
  - Verify: `cargo xtask golden`; `cargo test -p tidepool-codec`

- [ ] **M2.8 — font: TrueType parser and outline rasterizer; bundled font files**
  - Depends: M0.2, M1.6
  - Do:
    - `ttf` and `outline` per `docs/subsystems/rendering.md`.
    - Add static Saira and JetBrains Mono `.ttf` files with their OFL
      license files to `assets/fonts/`; record versions and sources in
      `docs/02-dependency-policy.md` → Assets. The user downloads the files
      if this session can't.
  - Accept: both faces parse; malformed fonts return errors (fuzz target
    `ttf`); a glyph's coverage is identical across runs.
  - Verify: `cargo test -p tidepool-font`;
    `cargo xtask fuzz-smoke --target ttf --secs 60`

- [ ] **M2.5 — font: atlas, layout, fallback; glyph goldens**
  - Depends: M2.8, M2.4
  - Do: `atlas`, `layout`, `fallback` and `bundled` per
    `docs/subsystems/rendering.md`.
  - Accept: golden glyph runs in both faces at the listed sizes, at scale 1
    and 2, including uppercase letter-spaced labels.
  - Verify: `cargo test -p tidepool-font`; `cargo xtask golden`

- [ ] **M2.6 — gui: first frame**
  - Depends: M2.2, M2.4, M2.5
  - Do: `gui` opens a window and draws the backdrop and a line of each
    face; `cargo tp` runs it.
  - Accept: cold start to first frame within the M0.5 budget on the
    reference machine; idle CPU ~0%.
  - Verify: `cargo tp -- --print-startup-time`; manual look

**Exit criteria (M2):** `cargo tp` opens a Linux window with crisp Saira and
JetBrains Mono text over the jabbott.dev backdrop.

---

## M3 — UI toolkit in the jabbott.dev style

**Goal:** the shell of the IDE, looking like jabbott.dev.

- [ ] **M3.1 — ui: tree, layout, focus, input routing, accessibility tree**
  - Depends: M0.2
  - Do: `tree`, `layout`, `focus`, `input`, `metrics`, `draw`, `a11y` per
    `docs/subsystems/ui-and-theme.md`.
  - Accept: layout property tests; every focusable node has a role and name.
  - Verify: `cargo test -p tidepool-ui`

- [ ] **M3.2 — ui: theme tokens and `xtask theme`**
  - Depends: M3.1, M1.7
  - Do: `themes/tidepool-dark.toml` with the token table; `theme` module;
    `cargo xtask theme` contrast check.
  - Accept: the contrast table in the spec is reproduced exactly; a token
    change that breaks AA fails the check.
  - Verify: `cargo xtask theme`

- [ ] **M3.5 — gui: draw-list painter and ui goldens**
  - Depends: M3.1, M2.6
  - Do: `gui` paints every `ui::draw` primitive with `gfx` and `font`, and
    implements `TextMetrics` with `font::layout`; a test binary renders
    `ui` scenes for `cargo xtask golden`.
  - Accept: a golden scene for each draw primitive at scale 1 and 2; a
    frame changes only pixels inside its damage.
  - Verify: `cargo xtask golden`; `cargo test -p tidepool-gui`

- [ ] **M3.6 — ui: controls**
  - Depends: M3.2, M3.5
  - Do: from the spec's Components table: buttons (ghost, teal, amber with
    cut corners, slate), chips, text input, search field, scrollbar, the
    focus outline, status dot, section title, eyebrow, kicker and
    breadcrumbs.
  - Accept: golden frames for every widget in each state (rest, hover,
    focus, active, disabled) at scale 1 and 2; each has a role and name in
    the accessibility tree.
  - Verify: `cargo xtask golden`; `cargo test -p tidepool-ui`

- [ ] **M3.3 — ui: collections and overlays**
  - Depends: M3.6
  - Do: the rest of the Components table: list, tree view, table, tab
    strip, splitter, card with its tab, code block, tooltip, toast, modal
    and the command palette.
  - Accept: as M3.6, plus keyboard-navigation tests for the list, tree
    view, table and palette.
  - Verify: `cargo xtask golden`; `cargo test -p tidepool-ui`

- [ ] **M3.4 — app: event-driven main loop and shell**
  - Depends: M3.3, M2.6, M2.7
  - Decide first: accept ADR-0012 (event-driven main loop on plain
    threads), which is `Proposed`.
  - Do: the loop in `docs/01-architecture.md`; title strip, activity bar,
    side panel, editor area, bottom panel, status bar, command palette;
    settings loading (read-only until M4.7).
  - Accept: shell golden at 1280×800 and 1920×1080; resize and HiDPI work;
    no redraw when idle.
  - Verify: `cargo xtask golden`; `xvfb-run cargo test -p tidepool`

**Exit criteria (M3):** the empty Tidepool shell, side by side with
jabbott.dev, reads as the same design (human review).

---

## M4 — Editing Tidepool with Tidepool

**Goal:** dogfooding starts. Tidepool edits its own source on Linux.

- [ ] **M4.1 — editor: editor view**
  - Depends: M3.4, M1.3, M1.9
  - Do: text rendering from the rope, cursor, selections, scrolling,
    gutter, current line, soft wrap.
  - Accept: scrolling budget met on a 10,000-line file; goldens.
  - Verify: `cargo xtask golden`; `cargo xtask bench editor`

- [ ] **M4.6 — editor: commands and the modal keymap engine**
  - Depends: M4.1, M1.7
  - Decide first: accept ADR-0006 (vim mode as a modal keymap engine in
    `editor`), which is `Proposed`.
  - Do: named commands with palette entries; the keymap engine with a mode
    stack and a key-sequence parser; keymaps as TOML, with user bindings
    over the defaults and conflicts listed; the default keymap (one mode).
  - Accept: command tests; a test keymap with two modes switches correctly;
    a conflicting binding is reported.
  - Verify: `cargo test -p tidepool-editor`

- [ ] **M4.2 — editor: editing commands, multi-cursor, undo and clipboard**
  - Depends: M4.6, M1.2, M2.7
  - Do:
    - Motions by grapheme, word and line; the editing commands;
      multi-cursor through `Transaction::change_each`; undo and redo
      through `History`, one command one step and typing coalesced
      (ADR-0020); the clipboard through the frontend.
    - M1.1's risk: a grapheme query can take 0.5–3 s on a crafted 100 MB
      line. Bound cursor motion, or run it on a worker, so the main thread
      never waits.
    - M1.2's risk: 10,000 carets on a 100 MB file take about 120 ms, over
      one frame. Batch in-leaf edits or make them cheaper, measured with
      `cargo xtask bench`.
  - Accept: command tests; on the crafted line no keystroke holds the main
    thread longer than one 16 ms frame; a 10,000-caret keystroke on 100 MB
    fits in one frame, or an ADR records the measured exception.
  - Verify: `cargo test -p tidepool-editor`; `cargo xtask bench editor`

- [ ] **M4.7 — workspace: folders, ignore rules, trust, settings**
  - Depends: M1.5, M1.8, M2.7
  - Decide first: accept ADR-0014 (workspaces are untrusted until the user
    trusts them), which is `Proposed`.
  - Do: `folder` (roots, `.tidepool/`, `.tidepoolignore` and `.gitignore`
    matching), `trust` (stored in user data) and `settings` (defaults →
    user → workspace, written back through M1.5's document model), per
    `docs/subsystems/editor.md`.
  - Accept: untrusted is the default, and a trust decision persists; saving
    settings keeps unknown keys and comments; an invalid value falls back
    to its default with a logged warning.
  - Verify: `cargo test -p tidepool-workspace`

- [ ] **M4.3 — workspace: buffers and session; editor: explorer, tabs, splits, trust prompt**
  - Depends: M4.7, M4.2
  - Do: `buffers` (keyed by canonical path; loaded and saved on workers
    through `text::file`, saves through `History::checkpoint` and
    `mark_saved`; large-file mode above 16 MB), `session`, and the file tree
    model of `files`; explorer, open editors, tabs, splits and the trust
    prompt.
  - Accept: opening, editing and saving files round-trips; untrusted state
    is the default; session restores; a 100 MB file opens in large-file
    mode within the budget.
  - Verify: `cargo test -p tidepool-workspace -p tidepool-editor`

- [ ] **M4.8 — regex: the engine and Unicode classes**
  - Depends: M0.2, M1.6
  - Decide first: Unicode general categories and scripts for the classes
    extend ADR-0018 (more of the official UCD files, and generated tables),
    which needs the user's approval to download, as ADR-0018 did.
  - Do: `tidepool-regex` per `docs/subsystems/text-and-formats.md`: parser,
    NFA, Pike VM, Unicode classes, simple case folding, literal-prefix
    acceleration; a fuzz target for compiling patterns.
  - Accept: regex conformance table; pathological patterns finish in linear
    time.
  - Verify: `cargo test -p tidepool-regex`;
    `cargo xtask fuzz-smoke --target regex --secs 60`

- [ ] **M4.4 — search: find and replace, project search, file finder**
  - Depends: M4.8, M4.3
  - Do: find/replace in a file; `search` and the project search panel on
    workers, with streaming results; the fuzzy file finder and its index in
    `files`.
  - Accept: project search budget met; results for an older buffer version
    are dropped.
  - Verify: `cargo test -p tidepool-workspace -p tidepool-editor`;
    `cargo xtask bench search`

- [ ] **M4.5 — platform: file watching (Linux)**
  - Depends: M2.2, M4.3
  - Do: inotify watcher with debounce and polling fallback.
  - Accept: external edits reload or prompt correctly; watcher limits
    produce the fix-it message.
  - Verify: `cargo test -p tidepool-platform -p tidepool-workspace`

- [ ] **M4.9 — app: crash reports and buffer recovery**
  - Depends: M3.4, M4.3
  - Do: the panic hook of `docs/05-release.md` → "Crash reporting":
    `crashes/<timestamp>.txt` with the version, OS, backtrace and the last
    200 log lines (redacted), unsaved buffers saved to `recovery/`, and
    recovery offered on the next start.
  - Accept: an injected panic writes a report holding no registered secret,
    and the unsaved buffers come back after a restart.
  - Verify: `cargo test -p tidepool`

**Exit criteria (M4):** the owner uses Tidepool for a day of edits to
Tidepool, and every bug found is filed.

---

## M5 — Managed toolchains and the terminal

**Goal:** Tidepool installs and drives rust-analyzer and git (ADR-0002).

- [ ] **M5.1 — toolchain: descriptors, detection, registry**
  - Depends: M1.7, M1.8
  - Decide first: the dependency matrix lets no crate but `tidepool-bench`
    use `tidepool-testkit`, yet the test plan has the tests of `toolchain`,
    `vcs`, `mobile`, `rpc` and `lang` use its fakes and transcripts
    (`docs/04-quality.md` → `layers`). Decide those edges before the first
    fake tool.
  - Do: `descriptor`, `detect`, `registry`; `toolchains/*.toml` for
    rustup, the Rust toolchain, rust-analyzer, git and curl;
    `cargo xtask tools`.
  - Accept: fake tools cover found, missing, outdated and broken states.
  - Verify: `cargo test -p tidepool-toolchain`; `cargo xtask tools`

- [ ] **M5.7 — platform: processes (Linux)**
  - Depends: M2.1
  - Do: `platform::process` per `docs/subsystems/sys-and-platform.md`:
    pipes with `O_CLOEXEC`, a process group per child, kill-tree and
    reaping; the declarations in `sys` through `/add-ffi-binding`.
  - Accept: a child's own children die with its tree; no zombie is left
    after drop; no file descriptor leaks into a child (tests).
  - Verify: `cargo test -p tidepool-platform`; `cargo xtask unsafe-audit`

- [ ] **M5.2 — toolchain: processes, sessions and secrets**
  - Depends: M5.1, M5.7
  - Do: `process`, `sessions`, `secret` per `docs/subsystems/toolchains.md`.
  - Accept: timeouts, cancellation and kill-tree work; no secret appears in
    argv, `Debug` or logs (test).
  - Verify: `cargo test -p tidepool-toolchain`

- [ ] **M5.3 — toolchain: downloads and install plans**
  - Depends: M5.2, M0.2
  - Do: `download` via `curl` (ADR-0016, accepted 2026-10-31);
    `install` plans with consent; licenses shown.
  - Accept: a checksum mismatch deletes the file and runs nothing; plans
    list every step before anything runs.
  - Verify: `cargo test -p tidepool-toolchain`

- [ ] **M5.8 — term: VT parser and grid**
  - Depends: M0.2, M1.6
  - Decide first: display width. A wide character (East Asian Wide, most
    emoji) takes two cells, in this grid and in the TUI (M7.1). Generating
    the table from `EastAsianWidth.txt` extends ADR-0018 and needs the
    user's approval to download, as ADR-0018 did. Decide also where the
    table lives: both `term` and `tui` may use `core` and `platform`, and
    `core` is where pure data such as this fits.
  - Do: the VT parser and grid of `docs/subsystems/toolchains.md` →
    `tidepool-term`: controls, CSI, SGR with truecolor, the OSC and DEC
    private modes listed, wide characters, scrollback, reflow, selection
    and search.
  - Accept: conformance against recorded byte streams from real programs;
    the parser is a fuzz target.
  - Verify: `cargo test -p tidepool-term`;
    `cargo xtask fuzz-smoke --target vt --secs 60`

- [ ] **M5.9 — platform: PTY (Linux)**
  - Depends: M5.7
  - Do: `platform::pty` per `docs/03-platforms.md` → Linux.
  - Accept: the PTY tests of `docs/subsystems/sys-and-platform.md`: spawn
    through the `Pty` API, check output, exit status and resize.
  - Verify: `cargo test -p tidepool-platform`; `cargo xtask unsafe-audit`

- [ ] **M5.4 — editor: integrated terminal (Linux)**
  - Depends: M5.8, M5.9, M3.4
  - Do: the terminal panel, on `tidepool-term` and `platform::pty`.
  - Accept: `vim -u NONE`, `less`, `htop` and `git add -p` run correctly in
    scripted snapshot tests.
  - Verify: `cargo test -p tidepool-term -p tidepool-editor`;
    `cargo xtask golden`

- [ ] **M5.5 — Rust toolchain and rust-analyzer installs; git**
  - Depends: M5.3, M5.4
  - Do: install/update flows for rustup, the toolchain and rust-analyzer;
    git detection and the package-manager plan run in the terminal.
  - Accept: on a clean Linux user account, Tidepool installs
    rust-analyzer with consent and detects git (gated real-tool test).
  - Verify: `TIDEPOOL_IT_RA=1 cargo test -p tidepool-toolchain`

- [ ] **M5.6 — editor: Toolchains panel and first-run checklist**
  - Depends: M5.5
  - Do: cards, status dots, actions, logs per the spec.
  - Accept: goldens for each state; actions run the right plans.
  - Verify: `cargo xtask golden`

**Exit criteria (M5):** a new user gets from a clean account to a working
Rust setup inside Tidepool without leaving it, and no install ran without a
confirmation.

---

## M6 — Rust and Markdown first-class

**Goal:** Tidepool is a good Rust and Markdown editor (ADR-0005).

- [ ] **M6.1 — rpc: JSON-RPC transport and LSP client; testkit fake server**
  - Depends: M1.4, M1.6, M5.1
  - Do: per `docs/subsystems/languages.md`, with the testkit edges M5.1
    decided.
  - Accept: transcript tests including cancellation, crash and malformed
    messages; framing is a fuzz target.
  - Verify: `cargo test -p tidepool-rpc -p tidepool-testkit`

- [ ] **M6.2 — syntax: Rust, Markdown and TOML highlighters; language packs; highlighting in the editor**
  - Depends: M1.1, M1.7, M4.1
  - Do: incremental lexers; the pack format; packs for Rust, Markdown and
    TOML; the editor view shows highlights, computed on a worker for
    buffers above 1 MB, with versioned results.
  - Accept: golden token streams; incremental equals full re-lex
    (property); goldens of highlighted Rust and Markdown.
  - Verify: `cargo test -p tidepool-syntax`; `cargo xtask golden`

- [ ] **M6.7 — lang: rust-analyzer sessions and diagnostics**
  - Depends: M6.1, M5.5, M4.3
  - Do: `registry`, `session` (`initialize` with `positionEncoding`,
    incremental sync, crash restart through `toolchain::sessions`), and
    `rust` (no build scripts or proc macros in untrusted workspaces,
    ADR-0014; `rust-toolchain.toml` respected); diagnostics in the gutter.
    Positions follow ADR-0019: the offset between CR and LF moves before it
    is sent, and positions are mapped where rust-analyzer's lines differ
    from Tidepool's (a lone CR).
  - Accept: fake-server suite for lifecycle, sync, cancellation, crash and
    malformed messages, with non-ASCII lines (combining marks, emoji, CJK)
    in UTF-8 and UTF-16; real rust-analyzer on Tidepool's workspace reports
    diagnostics (gated).
  - Verify: `cargo test -p tidepool-lang`;
    `TIDEPOOL_IT_RA=1 cargo test -p tidepool-lang`

- [ ] **M6.8 — lang: navigation and hover**
  - Depends: M6.7
  - Do: definition, declaration, type definition, implementation,
    references, hover, signature help, document and workspace symbols and
    inlay hints, with their popups and lists in the editor.
  - Accept: fake-server suite; real rust-analyzer resolves a known
    definition in Tidepool's workspace (gated).
  - Verify: `cargo test -p tidepool-lang -p tidepool-editor`;
    `TIDEPOOL_IT_RA=1 cargo test -p tidepool-lang`

- [ ] **M6.3 — lang: completion, rename, formatting, code actions, semantic tokens, folding**
  - Depends: M6.8
  - Do: the rest of the `features` table in `docs/subsystems/languages.md`;
    the completion popup; a rename or code action applies as one undo step.
  - Accept: fake-server suite; every feature in the `lang` table works with
    real rust-analyzer on Tidepool's workspace (gated).
  - Verify: `cargo test -p tidepool-lang -p tidepool-editor`;
    `TIDEPOOL_IT_RA=1 cargo test -p tidepool-lang`

- [ ] **M6.4 — lang: cargo tasks, problems panel, test discovery**
  - Depends: M6.7
  - Do: `lang::cargo`, Problems panel.
  - Accept: a build error appears with the right range, including on
    non-ASCII lines and after a lone CR.
  - Verify: `cargo test -p tidepool-lang`

- [ ] **M6.5 — markdown: CommonMark + GFM parser**
  - Depends: M0.2, M1.6
  - Decide first: vendoring the CommonMark spec examples as test-only
    fixtures, once the user confirms their license (CC-BY-SA 4.0, to be
    verified; `docs/02-dependency-policy.md` → Assets).
  - Do: per the spec; a fuzz target.
  - Accept: all core spec examples pass; GFM extension pass rate recorded.
  - Verify: `cargo test -p tidepool-markdown`

- [ ] **M6.6 — editor: Markdown preview, outline, link checks**
  - Depends: M6.5, M6.2
  - Do: preview in the `.prose` style; outline; relative-link and anchor
    diagnostics.
  - Accept: preview goldens match the spec's prose styles; broken links are
    reported.
  - Verify: `cargo xtask golden`; `cargo test -p tidepool-lang`

**Exit criteria (M6):** the owner does Tidepool's Rust and docs work in
Tidepool with rust-analyzer and the preview.

---

## M7 — Terminal UI and vim mode

**Goal:** `tidepool --tui` with vim-style modal editing (ADR-0003,
ADR-0006).

- [ ] **M7.1 — platform: terminal guard; tui: renderer and input decoder**
  - Depends: M3.4, M5.8
  - Do: per `docs/subsystems/frontends.md`, with M5.8's display widths.
  - Accept: TUI snapshots of the shell; decoder tables for xterm, kitty,
    tmux and the Linux console; terminal restored after an injected panic.
  - Verify: `cargo test -p tidepool-tui -p tidepool-platform`

- [ ] **M7.2 — vim mode: grammar, motions, operators, text objects**
  - Depends: M4.2
  - Do: modes, command grammar, the Motions, Operators, Text objects and
    Actions rows of `docs/subsystems/vim-mode.md`.
  - Accept: conformance fixtures for each command in those rows.
  - Verify: `cargo test -p tidepool-editor vim`

- [ ] **M7.3 — vim mode: visual modes, registers, marks, macros, dot-repeat**
  - Depends: M7.2
  - Do: the remaining rows except Ex commands. Vim's marks follow lines
    where Tidepool's positions follow bytes; if the conformance fixtures
    need Vim's rules, revisit ADR-0020 first.
  - Accept: conformance fixtures; the undo property test.
  - Verify: `cargo test -p tidepool-editor vim`

- [ ] **M7.4 — vim mode: search, ex commands, IDE bindings, settings**
  - Depends: M7.3, M4.4, M6.8
  - Do: `/ ? n N * #`, the listed ex commands, `gd gr K [d ]d`, leader
    mappings, `editor.keymap`, first-TUI-launch prompt.
  - Accept: conformance fixtures; with `TIDEPOOL_IT_VIM=1`, the checker
    agrees with `vim -u NONE -N` on the suite.
  - Verify: `cargo test -p tidepool-editor vim`;
    `TIDEPOOL_IT_VIM=1 cargo test -p tidepool-editor vim`

- [ ] **M7.5 — TUI end to end**
  - Depends: M7.1, M7.4
  - Do: run `tidepool --tui` inside a PTY in a test: open, edit in vim mode,
    save, quit; OSC 52 clipboard; cursor shapes.
  - Accept: the scripted session's final file and snapshots match.
  - Verify: `cargo test -p tidepool --test tui_session`

**Exit criteria (M7):** the owner can work in Tidepool over SSH in vim mode.

---

## M8 — Git

**Goal:** everyday git without leaving Tidepool.

- [ ] **M8.1 — vcs: status, diff hunks, gutter**
  - Depends: M5.2, M1.6
  - Do: porcelain v2 and diff parsing; gutter markers; fuzz targets.
  - Accept: fixture repos produce the expected states.
  - Verify: `cargo test -p tidepool-vcs`;
    `TIDEPOOL_IT_GIT=1 cargo test -p tidepool-vcs`

- [ ] **M8.2 — editor: source control panel**
  - Depends: M8.1, M4.3
  - Do: changes list, stage/unstage files and hunks, commit (message on
    stdin), branch switch, stash; all on explicit user action.
  - Accept: actions on fixture repos produce the expected `git` results.
  - Verify: `TIDEPOOL_IT_GIT=1 cargo test -p tidepool-editor scm`

- [ ] **M8.3 — editor: diff view, blame, history**
  - Depends: M8.1, M4.3
  - Do: side-by-side and inline diff; blame gutter; file history.
  - Accept: goldens; blame on a fixture matches `git blame`.
  - Verify: `cargo xtask golden`; `TIDEPOOL_IT_GIT=1 cargo test -p tidepool-vcs`

**Exit criteria (M8):** a week of Tidepool's own commits made from Tidepool's
git panel.

---

## M9 — AI and predictive text

**Goal:** language agents and offline prediction (ADR-0009).

- [ ] **M9.1 — TAP spec review**
  - Depends: M6.1
  - Do: review `docs/subsystems/agents.md` with the user; accept ADR-0009.
  - Accept: the user accepts the spec (recorded in the ADR).
  - Verify: doc review (human)

- [ ] **M9.2 — agents/: mock agent and conformance suite**
  - Depends: M9.1
  - Decide first (`docs/04-quality.md` → `deps` calls both provisional):
    whether the reference agents may use root crates such as `core`, which
    `cargo xtask deps` refuses today; and whether `agents/` needs members
    beyond the three agents, such as shared TAP code or the conformance
    suite.
  - Do: create the `agents/` workspace and `tp-agent-mock`; TAP suite.
  - Accept: the suite passes against the mock; `cargo xtask deps` covers
    `agents/`.
  - Verify: `cargo test --manifest-path agents/Cargo.toml`;
    `cargo xtask deps`

- [ ] **M9.3 — agent: host, consent, context assembly**
  - Depends: M9.2, M5.2, M4.3
  - Do: `manifest`, `host`, `tap`, `context`, `consent`.
  - Accept: deny-list tests; untrusted workspaces send only the focused
    buffer with consent; crash/restart test.
  - Verify: `cargo test -p tidepool-agent`

- [ ] **M9.4 — agent: predictive engine; editor: completion merge and ghost text**
  - Depends: M9.3, M6.3
  - Do: `predict`, `merge`; popup and ghost text.
  - Accept: the responsiveness suite with a 2 s mock agent stays within
    budget; stale results never show.
  - Verify: `cargo test -p tidepool-agent -p tidepool-editor`;
    `cargo xtask bench latency`

- [ ] **M9.5 — editor: chat panel and edit proposals**
  - Depends: M9.4
  - Do: chat with streaming; proposals as diffs; accept applies one undo
    step.
  - Accept: goldens; proposal accept/reject tests.
  - Verify: `cargo test -p tidepool-editor agent`; `cargo xtask golden`

- [ ] **M9.6 — agents/: local and Claude reference agents**
  - Depends: M9.2
  - Do: `tp-agent-local` (loopback only) and `tp-agent-claude` (curl, key
    via `--config -`), per the spec; check the current Anthropic API docs.
  - Accept: unit tests with recorded responses; no key in argv or logs;
    live test opt-in only.
  - Verify: `cargo test --manifest-path agents/Cargo.toml`

**Exit criteria (M9):** with the Claude agent installed, the owner gets
inline completions and reviewable edits in Rust and Markdown; with no agent,
local prediction still works.

---

## M10 — Android

**Goal:** build, sign, verify and run Android apps (ADR-0007).

- [ ] **M10.1 — toolchain: JDK, Android SDK, NDK, bundletool**
  - Depends: M5.3
  - Do: descriptors and flows; `sdkmanager --licenses` in the terminal for
    the user; re-check Android's current docs (including which AAPT2 to
    use) and record findings in task notes.
  - Accept: fake-tool tests; gated real install on Linux.
  - Verify: `cargo test -p tidepool-toolchain`;
    `TIDEPOOL_IT_ANDROID=1 cargo test -p tidepool-toolchain android`

- [ ] **M10.7 — codec: inflate, deflate and PNG**
  - Depends: M1.4, M1.6
  - Do: `inflate` and `png` (port Nadir's) and `deflate`, per
    `docs/subsystems/text-and-formats.md`; a fuzz target for each decoder.
  - Accept: hand-made inflate vectors (stored, fixed and dynamic blocks,
    back-references at the window edge); PNG decodes every color type and
    depth; deflate output inflates back to its input.
  - Verify: `cargo test -p tidepool-codec`; `cargo xtask fuzz-smoke --secs 60`

- [ ] **M10.2 — codec: ZIP and XML; mobile: artifact inspector**
  - Depends: M10.7
  - Do: `zip`, `xml`; `mobile::inspect`.
  - Accept: archives from Android tools read correctly; traversal rejected;
    fuzz targets.
  - Verify: `cargo test -p tidepool-codec -p tidepool-mobile`;
    `cargo xtask fuzz-smoke --secs 60`

- [ ] **M10.3 — mobile: keystores and signing**
  - Depends: M10.1
  - Decide first: accept ADR-0007 (signing delegated to `apksigner`,
    `jarsigner` and `codesign`), which is `Proposed`.
  - Do: `android::sign` per `docs/subsystems/mobile.md`.
  - Accept: no secret in any constructed argv (test); signed artifacts pass
    the verifiers (gated).
  - Verify: `cargo test -p tidepool-mobile`;
    `TIDEPOOL_IT_ANDROID=1 cargo test -p tidepool-mobile sign`

- [ ] **M10.4 — mobile: Gradle projects**
  - Depends: M10.3
  - Do: `android::gradle` in trusted workspaces; artifacts then Tidepool
    signing.
  - Accept: a sample Gradle app produces a signed APK and AAB that pass the
    verifiers (gated).
  - Verify: `TIDEPOOL_IT_ANDROID=1 cargo test -p tidepool-mobile gradle`

- [ ] **M10.5 — mobile: Gradle-free Java and Rust NativeActivity apps**
  - Depends: M10.2, M10.3
  - Do: `android::direct` for both samples.
  - Accept: both produce a signed APK and AAB that pass every verifier and
    install on an emulator (gated).
  - Verify: `TIDEPOOL_IT_ANDROID=1 cargo test -p tidepool-mobile direct`

- [ ] **M10.6 — editor: SDK manager, devices, logcat, build artifacts**
  - Depends: M10.1, M10.5, M4.3
  - Do: panels per `docs/subsystems/editor.md`.
  - Accept: goldens; device actions run the right commands.
  - Verify: `cargo xtask golden`; `cargo test -p tidepool-editor mobile`

**Exit criteria (M10):** from a clean account, the owner installs the SDK,
builds both samples, and runs them on an emulator, all inside Tidepool.

---

## M11 — macOS and iOS

**Goal:** Tidepool on macOS, and iOS artifacts (ADR-0008).

- [ ] **M11.7 — sys/platform (macOS): Objective-C glue, window, events, present**
  - Depends: M2.2, M2.7
  - Decide first: accept ADR-0015 (Linux, then macOS, then Windows), which
    is `Proposed`; the milestones before this one follow it already.
  - Do: per `docs/03-platforms.md` → macOS, through `/add-ffi-binding`;
    port Nadir's Objective-C patterns where they fit.
  - Accept: M2.2's window, event and waker tests pass on macOS.
  - Verify: `cargo test -p tidepool-platform` on macOS
    - ⏳ needs macOS

- [ ] **M11.8 — platform (macOS): clipboard, paths, processes, PTY, FSEvents**
  - Depends: M11.7, M5.9, M4.5
  - Do: per `docs/03-platforms.md` → macOS.
  - Accept: the Linux clipboard, path, process, PTY and watcher tests pass
    on macOS.
  - Verify: `cargo test -p tidepool-platform -p tidepool-workspace` on macOS
    - ⏳ needs macOS

- [ ] **M11.1 — sys/platform: macOS**
  - Depends: M11.8
  - Do: whatever else `cargo xtask ci` needs on macOS.
  - Accept: the Linux platform tests pass on macOS.
  - Verify: `cargo xtask ci` on macOS
    - ⏳ needs macOS

- [ ] **M11.2 — toolchain: Xcode, CLT, simulators**
  - Depends: M11.1, M5.3
  - Do: detection, install guidance, license status; re-check Apple's
    current tools and record findings.
  - Accept: fake-tool tests; gated real detection on macOS.
  - Verify: `TIDEPOOL_IT_IOS=1 cargo test -p tidepool-toolchain ios`

- [ ] **M11.3 — mobile: identities, profiles, Xcode projects**
  - Depends: M11.2
  - Decide first: accept ADR-0008 (iOS builds on macOS, locally or on a
    remote Mac), which is `Proposed`.
  - Do: `ios::sign`, `ios::xcode`, plist writer.
  - Accept: a sample Xcode app produces development and App Store `.ipa`
    files that pass `codesign --verify --deep --strict` (gated).
  - Verify: `TIDEPOOL_IT_IOS=1 cargo test -p tidepool-mobile xcode`

- [ ] **M11.4 — mobile: Xcode-free Rust iOS apps**
  - Depends: M11.3, M10.2
  - Do: `ios::direct`.
  - Accept: the Rust sample builds, signs, packages and launches in the
    simulator (gated).
  - Verify: `TIDEPOOL_IT_IOS=1 cargo test -p tidepool-mobile direct_ios`

- [ ] **M11.5 — mobile: remote-Mac builds from Linux**
  - Depends: M11.4
  - Do: `ios::remote` over `ssh`/`rsync`.
  - Accept: from Linux, the samples produce the same artifacts as on the Mac.
  - Verify: gated test with `TIDEPOOL_REMOTE_MAC=<host>`

- [ ] **M11.6 — editor: simulators and signing panels; macOS package**
  - Depends: M11.3
  - Decide first: `package` runs `lipo` and `iconutil`, which ADR-0017's
    exception does not cover; extend it.
  - Do: panels; `xtask package macos` per `docs/05-release.md`.
  - Accept: goldens; the package launches.
  - Verify: `cargo xtask golden`; `cargo xtask package macos`

**Exit criteria (M11):** iOS samples build and sign on a Mac and from Linux.

---

## M12 — Windows

- [ ] **M12.1 — sys/platform: Win32 window, events, present, clipboard, DPI**
  - Depends: M4.5, M5.4
  - Do: per `docs/03-platforms.md` → Windows.
  - Accept: the Linux platform tests pass on Windows.
  - Verify: `cargo xtask ci` on Windows
    - ⏳ needs Windows

- [ ] **M12.2 — platform: ConPTY, ReadDirectoryChangesW, paths**
  - Depends: M12.1
  - Do: per `docs/03-platforms.md` → Windows.
  - Accept: the terminal snapshot tests and watcher tests pass on Windows.
  - Verify: `cargo test -p tidepool-platform -p tidepool-term` on Windows

- [ ] **M12.3 — toolchain: winget plans, rustup-init.exe, JDK and SDK paths**
  - Depends: M12.1, M5.3
  - Do: Windows install plans per `docs/subsystems/toolchains.md`.
  - Accept: fake-tool tests; gated real install of rust-analyzer.
  - Verify: `cargo test -p tidepool-toolchain` on Windows

- [ ] **M12.4 — Windows package**
  - Depends: M12.3
  - Decide first: `package` runs `rc.exe`, which ADR-0017's exception does
    not cover; extend it.
  - Do: `xtask package windows` per `docs/05-release.md`.
  - Accept: the package launches on a clean Windows account.
  - Verify: `cargo xtask package windows`; manual launch

- [ ] **M12.5 — Repository tooling on Windows**
  - Depends: M12.1
  - Do: the Claude Code hook (`.claude/hooks/deps-guard.sh`) does not decode
    JSON escapes in the file path it is given, so on Windows, where every
    path has one, it checks the project instead of the worktree
    (`docs/04-quality.md` → "Command line and rules"). Decode them, and
    check that every `xtask` check agrees on Windows paths and line endings.
  - Accept: an edit in a linked worktree on Windows is checked in that
    worktree; the `xtask` suite passes on Windows.
  - Verify: `cargo test -p xtask` on Windows
    - ⏳ needs Windows

**Exit criteria (M12):** M4–M10 exit criteria hold on Windows.

---

## M13 — Debugging and language expansion

- [ ] **M13.1 — rpc/lang: DAP client and Rust debugging**
  - Depends: M6.3, M5.3
  - Do: managed `lldb-dap` or CodeLLDB; breakpoints, stepping, stack,
    variables, watch, console.
  - Accept: a debug session on a sample binary hits a breakpoint and shows
    locals (gated).
  - Verify: `cargo test -p tidepool-lang dap`

- [ ] **M13.2 — syntax: declarative pack lexers**
  - Depends: M6.2, M4.8
  - Do: lexer rules in language packs, compiled to `tidepool-regex`.
  - Accept: a pack-defined lexer for TOML matches the hand-written one's
    golden tokens.
  - Verify: `cargo test -p tidepool-syntax`

- [ ] **M13.3 — language packs for common languages**
  - Depends: M13.2
  - Do: C/C++, Python, JavaScript/TypeScript, Go, Java, Kotlin, Swift,
    Shell, SQL, HTML/CSS, YAML, JSON, Dockerfile, Makefile; LSP mappings as
    managed tools.
  - Accept: highlight goldens per pack; documented server setup.
  - Verify: `cargo test -p tidepool-syntax packs`

- [ ] **M13.4 — editor: image and hex viewers**
  - Depends: M10.7, M4.3
  - Do: PNG files open in an image viewer and binary files in a hex view,
    per `docs/subsystems/editor.md` → "Editor view".
  - Accept: goldens; a malformed PNG shows an error, never a panic.
  - Verify: `cargo xtask golden`; `cargo test -p tidepool-editor`

---

## M14 — Production hardening and release

- [ ] **M14.1 — fuzz campaign**
  - Depends: M10.2, M9.3
  - Decide first: register item 30 (coverage-guided fuzzing) in
    `docs/02-dependency-policy.md`.
  - Do: nightly 30-minute fuzz smoke per target; review every `Limits`.
  - Verify: `cargo xtask fuzz-smoke --secs 1800` per target (nightly logs)

- [ ] **M14.2 — accessibility bridges**
  - Depends: M3.1, M11.1, M12.1
  - Do: AT-SPI, NSAccessibility and UI Automation from the `ui` tree.
  - Accept: a screen reader announces focus moves in the shell on each OS.
  - Verify: manual screen-reader checklist per OS

- [ ] **M14.3 — performance pass**
  - Depends: M9.4, M10.6
  - Do: measure every budget in `docs/00-overview.md` on the reference
    machine; fix violations or record exceptions with an ADR.
  - Verify: `cargo xtask bench`; measurement notes with numbers

- [ ] **M14.7 — security review pass**
  - Depends: M9.6, M10.6, M11.5, M12.3
  - Do: `security-reviewer` reviews the whole tree against its checklist
    and the scope in `SECURITY.md`: workspace trust, installs and
    downloads, child processes, secrets, signing, agent context and
    consent, every parser's `Limits`, `unsafe`, and the CI workflows. Each
    finding is fixed, or recorded with an ADR.
  - Accept: the review and how each finding was resolved are recorded in a
    task note.
  - Verify: the review report (human sign-off)

- [ ] **M14.4 — packaging and release workflow**
  - Depends: M11.6, M12.4, M0.8
  - Do: `xtask package <os>`, the `tidepool --smoke-test` mode, and
    `.github/workflows/release.yml` per `docs/05-release.md`, with its row
    in `cargo xtask workflows`' table (ADR-0022); document secret names
    only.
  - Verify: YAML review; `cargo xtask workflows`; `cargo xtask package linux`

- [ ] **M14.5 — documentation**
  - Depends: M14.4
  - Do: user guide; `cargo doc --workspace --no-deps` with no warnings.
  - Verify: `cargo doc --workspace --no-deps`

- [ ] **M14.6 — release-candidate dry run**
  - Depends: M14.1–M14.5, M14.7
  - Do: the release checklist in `docs/05-release.md`, with evidence.
  - Verify: the completed checklist (human sign-off)

**Exit criteria (M14):** signed packages for all three OSes from a tag, and
the release checklist is complete.

---

## Backlog (each needs a trigger or a decision)

| Item | Trigger / reason |
| :- | :- |
| Native Wayland (xdg-shell, keymaps without `libxkbcommon`) | A target distro ships without XWayland by default (ADR-0010) |
| GPU rendering backend | Frame budget missed at 1440p after profiling (ADR-0011) |
| Complex text shaping, bidi, color emoji | A user need for shaped scripts or emoji in code |
| Full IME (XIM, `NSTextInputClient`, IMM32) | CJK input needed |
| `.vimrc` importer for simple `map` lines | Users ask to bring mappings (ADR-0006) |
| Tidepool as an MCP server for outside agents | A concrete agent integration needs it (ADR-0009) |
| Agent tools (run tests, search) with per-call approval | TAP v1 proposals prove too limiting |
| OS sandboxing for agents (seccomp/landlock, macOS sandbox) | Agents from third parties become common |
| Play Console and App Store Connect uploads | The owner ships an app through Tidepool |
| Offline toolchain bundle | Users without network on first run (ADR-0002) |
| Light theme | jabbott.dev adds one, or users ask |
| Remote development, collaborative editing | User demand |
| aarch64 Linux and Windows | Platform demand |
| MIT-SHM present on X11 | Present time shows in profiles |
| Undo history that survives a restart, and history pruning | Undo must persist, or history memory shows in profiles (ADR-0020) |
| `codec`: the YAML subset, CSV and INI (`docs/subsystems/text-and-formats.md`) | A viewer, a validator or a language pack needs to parse them, not only highlight them |
