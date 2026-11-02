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
- A task that depends on a `Proposed` ADR asks the user to decide it first.
- Exit criteria for a milestone are checked by a human.
- OS order: **Linux → macOS → Windows** (ADR-0015).

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

**Exit criteria (M0):** `cargo xtask ci` is green locally and on all three
CI runners. `Cargo.lock` contains zero third-party packages.

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
  - Do: `encoding`, `file` (atomic save, preservation rules, large-file
    threshold, invalid-UTF-8 read-only).
  - Accept: unchanged files with BOM, CRLF, mixed endings and no trailing
    newline round-trip byte-for-byte; a crash between write and rename
    leaves the original intact (simulated).
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

- [ ] **M1.5 — codec: TOML with a preserving document model**
  - Depends: M1.4
  - Do: `codec::toml` per ADR-0013 (accepted 2026-11-01).
  - Accept: TOML 1.0 compliance cases; editing a value and writing back
    preserves comments, order and formatting elsewhere.
  - Verify: `cargo test -p tidepool-codec`

- [ ] **M1.6 — xtask: fuzz smoke**
  - Depends: M0.3, M1.4, M1.5
  - Do: `cargo xtask fuzz-smoke` per `docs/04-quality.md`; targets for JSON
    and TOML with seed corpora in `fuzz/corpus/`.
  - Accept: 60 s per target with no crash; a planted panic is found.
  - Verify: `cargo xtask fuzz-smoke --secs 60`
  - Note: also add the Linux fuzz-smoke step (30 s per target) that
    `docs/04-quality.md` → "CI" requires to `.github/workflows/ci.yml`;
    M0.4 left a `TODO(M1.6)` there.

**Exit criteria (M1):** the text engine and JSON/TOML pass their property
tests and fuzz smoke.

---

## M2 — A window and text on Linux

**Goal:** a Linux window that shows Saira and JetBrains Mono text, drawn by
our own rasterizer.

- [ ] **M2.1 — sys: dynlib, libc and X11 declarations**
  - Depends: M0.1
  - Do: `dynlib!`, `linux::libc`, `linux::x11` per
    `docs/subsystems/sys-and-platform.md` (ask the user to accept
    ADR-0010 first). Follow `/add-ffi-binding`.
  - Accept: layout asserts for every struct; each declaration cites its
    header.
  - Verify: `cargo build -p tidepool-sys`; `cargo xtask unsafe-audit`

- [ ] **M2.2 — platform: window, events, waker, present, clipboard (Linux)**
  - Depends: M2.1, M0.2
  - Do: `window`, `events` (poll on the X fd and a self-pipe), `surface`
    (`XPutImage` of damage rects), `clipboard`, `paths`.
  - Accept: under Xvfb, a test opens a window, presents a framebuffer,
    reads the pixels back with `XGetImage`, and a `Waker` from another
    thread interrupts `wait_events` within 5 ms.
  - Verify: `xvfb-run cargo test -p tidepool-platform`

- [ ] **M2.3 — gfx: rasterizer and backdrop**
  - Depends: M0.2
  - Do: `framebuffer`, `raster`, `blend`, `clip`, `image`, `damage`,
    `backdrop` per `docs/subsystems/rendering.md` (ask the user to accept
    ADR-0011 first).
  - Accept: golden frames for every primitive; output identical across two
    runs.
  - Verify: `cargo test -p tidepool-gfx`

- [ ] **M2.4 — codec: QOI; xtask: golden harness**
  - Depends: M2.3, M0.3
  - Do: `codec::qoi`; `cargo xtask golden [--bless]` per
    `docs/04-quality.md`.
  - Accept: a changed pixel fails with diff images in `target/golden/`.
  - Verify: `cargo xtask golden`

- [ ] **M2.5 — font: TrueType, rasterizer, atlas, layout; bundled fonts**
  - Depends: M2.3
  - Do:
    - `ttf`, `outline`, `atlas`, `layout`, `fallback` per
      `docs/subsystems/rendering.md`.
    - Add static Saira and JetBrains Mono `.ttf` files with their OFL
      license files to `assets/fonts/`; record versions and sources in
      `docs/02-dependency-policy.md` → Assets. The user downloads the files
      if this session can't.
  - Accept: golden glyph runs at the listed sizes; TrueType parser is a
    fuzz target; malformed fonts return errors.
  - Verify: `cargo test -p tidepool-font`; `cargo xtask golden`;
    `cargo xtask fuzz-smoke --target ttf --secs 60`

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
  - Depends: M3.1, M1.5
  - Do: `themes/tidepool-dark.toml` with the token table; `theme` module;
    `cargo xtask theme` contrast check.
  - Accept: the contrast table in the spec is reproduced exactly; a token
    change that breaks AA fails the check.
  - Verify: `cargo xtask theme`

- [ ] **M3.3 — ui: widgets**
  - Depends: M3.2
  - Do: every widget in the spec's Components table, in each state.
  - Accept: golden frames per widget and state at scale 1 and 2 (rendered
    through `gui`'s painter in test binaries).
  - Verify: `cargo xtask golden`

- [ ] **M3.4 — app: event-driven main loop and shell**
  - Depends: M3.3, M2.6
  - Do: the loop in `docs/01-architecture.md` (ask the user to accept
    ADR-0012 first); title strip, activity bar, side panel, editor area,
    bottom panel, status bar, command palette; settings loading.
  - Accept: shell golden at 1280×800 and 1920×1080; resize and HiDPI work;
    no redraw when idle.
  - Verify: `cargo xtask golden`; `xvfb-run cargo test -p tidepool`

**Exit criteria (M3):** the empty Tidepool shell, side by side with
jabbott.dev, reads as the same design (human review).

---

## M4 — Editing Tidepool with Tidepool

**Goal:** dogfooding starts. Tidepool edits its own source on Linux.

- [ ] **M4.1 — editor: editor view**
  - Depends: M3.4, M1.3
  - Do: text rendering from the rope, cursor, selections, scrolling,
    gutter, current line, soft wrap.
  - Accept: scrolling budget met on a 10,000-line file; goldens.
  - Verify: `cargo xtask golden`; `cargo xtask bench editor`

- [ ] **M4.2 — editor: commands and the modal keymap engine**
  - Depends: M4.1, M1.2
  - Do: named commands, palette entries, default keymap, multi-cursor,
    undo/redo, clipboard; the engine supports a mode stack (ask the user to
    accept ADR-0006 first).
  - Accept: command tests; a test keymap with two modes switches correctly.
  - Verify: `cargo test -p tidepool-editor`

- [ ] **M4.3 — workspace: folders, trust, settings, files, buffers, session**
  - Depends: M1.5, M4.2
  - Do: per `docs/subsystems/editor.md` (ask the user to accept ADR-0014
    first). Explorer, tabs, splits, trust prompt, session restore.
  - Accept: opening, editing and saving files round-trips; untrusted state
    is the default; session restores.
  - Verify: `cargo test -p tidepool-workspace -p tidepool-editor`

- [ ] **M4.4 — regex and search**
  - Depends: M0.2, M4.3
  - Do: `tidepool-regex`; find/replace in file; project search panel on
    workers; fuzzy file finder.
  - Accept: regex conformance; linear time on pathological patterns;
    project search budget met.
  - Verify: `cargo test -p tidepool-regex -p tidepool-workspace`;
    `cargo xtask bench search`

- [ ] **M4.5 — platform: file watching (Linux)**
  - Depends: M2.2, M4.3
  - Do: inotify watcher with debounce and polling fallback.
  - Accept: external edits reload or prompt correctly; watcher limits
    produce the fix-it message.
  - Verify: `cargo test -p tidepool-platform -p tidepool-workspace`

**Exit criteria (M4):** the owner uses Tidepool for a day of edits to
Tidepool, and every bug found is filed.

---

## M5 — Managed toolchains and the terminal

**Goal:** Tidepool installs and drives rust-analyzer and git (ADR-0002).

- [ ] **M5.1 — toolchain: descriptors, detection, registry**
  - Depends: M1.5
  - Do: `descriptor`, `detect`, `registry`; `toolchains/*.toml` for
    rustup, the Rust toolchain, rust-analyzer, git and curl;
    `cargo xtask tools`.
  - Accept: fake tools cover found, missing, outdated and broken states.
  - Verify: `cargo test -p tidepool-toolchain`; `cargo xtask tools`

- [ ] **M5.2 — toolchain: processes, sessions and secrets**
  - Depends: M5.1, M2.2
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

- [ ] **M5.4 — term and pty: integrated terminal (Linux)**
  - Depends: M2.2, M3.4
  - Do: `platform::pty`, `tidepool-term`, terminal panel.
  - Accept: `vim -u NONE`, `less`, `htop` and `git add -p` run correctly in
    scripted snapshot tests.
  - Verify: `cargo test -p tidepool-term`; `cargo xtask golden`

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
  - Depends: M1.4
  - Do: per `docs/subsystems/languages.md`.
  - Accept: transcript tests including cancellation, crash and malformed
    messages; framing is a fuzz target.
  - Verify: `cargo test -p tidepool-rpc -p tidepool-testkit`

- [ ] **M6.2 — syntax: Rust, Markdown and TOML highlighters; language packs**
  - Depends: M1.1, M4.4
  - Do: incremental lexers; pack format; packs for Rust, Markdown, TOML.
  - Accept: golden token streams; incremental equals full re-lex.
  - Verify: `cargo test -p tidepool-syntax`

- [ ] **M6.3 — lang: rust-analyzer sessions and features**
  - Depends: M6.1, M5.5, M4.3
  - Do: every feature in the `lang` table; trust rules; UTF-16 handling.
  - Accept: fake-server suite; real rust-analyzer on Tidepool's workspace
    shows diagnostics and go-to-definition.
  - Verify: `cargo test -p tidepool-lang`;
    `TIDEPOOL_IT_RA=1 cargo test -p tidepool-lang`

- [ ] **M6.4 — lang: cargo tasks, problems panel, test discovery**
  - Depends: M6.3
  - Do: `lang::cargo`, Problems panel.
  - Accept: a build error appears with the right range, including non-ASCII
    lines.
  - Verify: `cargo test -p tidepool-lang`

- [ ] **M6.5 — markdown: CommonMark + GFM parser**
  - Depends: M0.2
  - Do: per the spec; vendor the CommonMark spec examples as test-only
    fixtures with their license after the user confirms the license.
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
  - Depends: M3.4
  - Do: per `docs/subsystems/frontends.md`.
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
  - Do: the remaining rows except Ex commands.
  - Accept: conformance fixtures; the undo property test.
  - Verify: `cargo test -p tidepool-editor vim`

- [ ] **M7.4 — vim mode: search, ex commands, IDE bindings, settings**
  - Depends: M7.3, M4.4, M6.3
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
  - Depends: M5.2
  - Do: porcelain v2 and diff parsing; gutter markers; fuzz targets.
  - Accept: fixture repos produce the expected states.
  - Verify: `cargo test -p tidepool-vcs`;
    `TIDEPOOL_IT_GIT=1 cargo test -p tidepool-vcs`

- [ ] **M8.2 — editor: source control panel**
  - Depends: M8.1
  - Do: changes list, stage/unstage files and hunks, commit (message on
    stdin), branch switch, stash; all on explicit user action.
  - Accept: actions on fixture repos produce the expected `git` results.
  - Verify: `TIDEPOOL_IT_GIT=1 cargo test -p tidepool-editor scm`

- [ ] **M8.3 — editor: diff view, blame, history**
  - Depends: M8.1
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
  - Do: create the `agents/` workspace and `tp-agent-mock`; TAP suite.
  - Accept: the suite passes against the mock; `check-deps` covers `agents/`.
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

- [ ] **M10.2 — codec: DEFLATE, ZIP, XML; mobile: artifact inspector**
  - Depends: M1.4
  - Do: `inflate`, `deflate`, `zip`, `xml`, `png`; `mobile::inspect`.
  - Accept: archives from Android tools read correctly; traversal rejected;
    fuzz targets.
  - Verify: `cargo test -p tidepool-codec -p tidepool-mobile`;
    `cargo xtask fuzz-smoke --secs 60`

- [ ] **M10.3 — mobile: keystores and signing**
  - Depends: M10.1
  - Do: `android::sign` per `docs/subsystems/mobile.md` (ask the user to
    accept ADR-0007 first).
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
  - Depends: M10.1, M10.5
  - Do: panels per `docs/subsystems/editor.md`.
  - Accept: goldens; device actions run the right commands.
  - Verify: `cargo xtask golden`; `cargo test -p tidepool-editor mobile`

**Exit criteria (M10):** from a clean account, the owner installs the SDK,
builds both samples, and runs them on an emulator, all inside Tidepool.

---

## M11 — macOS and iOS

**Goal:** Tidepool on macOS, and iOS artifacts (ADR-0008).

- [ ] **M11.1 — sys/platform: macOS**
  - Depends: M4.5, M5.4
  - Do: objc glue, window, events, present, clipboard, PTY, FSEvents,
    paths, per `docs/03-platforms.md`.
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
  - Do: `xtask package windows` per `docs/05-release.md`.
  - Accept: the package launches on a clean Windows account.
  - Verify: `cargo xtask package windows`; manual launch

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
  - Depends: M6.2, M4.4
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

---

## M14 — Production hardening and release

- [ ] **M14.1 — fuzz campaign**
  - Depends: M10.2, M9.3
  - Do: nightly 30-minute fuzz smoke per target; review every `Limits`;
    ask the user to decide register item 30 (coverage-guided fuzzing).
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

- [ ] **M14.4 — packaging and release workflow**
  - Depends: M11.6, M12.4
  - Do: `xtask package <os>` and `.github/workflows/release.yml` per
    `docs/05-release.md`; document secret names only.
  - Verify: YAML review; `cargo xtask package linux`

- [ ] **M14.5 — documentation**
  - Depends: M14.4
  - Do: user guide; `cargo doc --workspace --no-deps` with no warnings.
  - Verify: `cargo doc --workspace --no-deps`

- [ ] **M14.6 — release-candidate dry run**
  - Depends: M14.1–M14.5
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
