# Tidepool — zero-dependency IDE (Rust; Linux, macOS, Windows)

**Status:** in development, at version 0.1: `cargo tp` opens a window on
Linux and shows its first frame, jabbott.dev's backdrop and a line in each
bundled face, and does nothing else yet (M2.6). `docs/roadmap.md` ticks
each task once it is done and verified.

- **Built:** the guarded workspace and its checks (M0.1–M0.4,
  M0.9–M0.11), with doc tests that forbid `unsafe` outside `sys` and
  `platform` (M0.15), the performance budgets, confirmed on the reference machine
  (M0.5), the contribution workflow and a README in every member (M0.6),
  in Nadir's shape with copies of `LICENSE` and `NOTICE` (M0.13),
  diagrams of the design and the code in `diagrams/` (M0.12),
  the CI supply chain and its check (M0.8), license headers on the
  source files and their check (M0.7), the foundations in `tidepool-core`
  (M0.2) and its path helpers (M1.8), JSON and JSONC (M1.4), TOML 1.1 (M1.7) and its document model,
  which writes edits back and keeps the rest (M1.5), in `tidepool-codec`,
  and in `tidepool-text` the rope, positions and grapheme boundaries
  (M1.1), selections, change sets and the undo tree (M1.2), and encodings,
  loading and the atomic save (M1.3); the fuzzer, with `cargo xtask
  fuzz-smoke` and targets for JSON, TOML and the text decoder (M1.6);
  the benchmark harness, with `cargo xtask bench`, the text engine's
  suites and their baseline on the reference machine (M1.9); and in
  `tidepool-sys` the first declarations for the Linux window, its events
  and its presentation: the loader, `poll` and `pipe2` from libc, and
  Xlib's types and a table of its functions to be loaded at run time
  (M2.1); and in `tidepool-platform`, on Linux through X11, a window, its
  events with key codes, typed text and a waker other threads can use,
  and presenting the damaged parts of a frame, over a local Unix socket
  only (M2.2, ADR-0039), with what that needs added to `tidepool-sys`;
  and the clipboard's `CLIPBOARD` and `PRIMARY` selections through the
  window's events, and the user's folders from the XDG variables and the
  tools' from their own (`ANDROID_HOME`, `RUSTUP_HOME`) (M2.7, ADR-0042);
  and in `tidepool-gfx` the CPU rasterizer: a premultiplied BGRA8
  framebuffer, rects, hairlines, borders, brackets and anti-aliased
  polygons, lines, cut corners and pills, blending and layer opacity,
  clipping, image and glyph-mask blits, damage merging and frame diffing,
  and jabbott.dev's backdrop with its blurred copy (M2.3, ADR-0011);
  and QOI in `tidepool-codec`, with its fuzz target, and the golden
  harness: `cargo xtask golden`, a step of `ci`, comparing golden frames
  of every `gfx` primitive and the backdrop with QOI references (M2.4,
  ADR-0044); and in `tidepool-font` the TrueType parser, with glyph ids,
  metrics, names, kerning and composite glyphs checked against its
  limits, and the glyph rasterizer's exact-area coverage, with its fuzz
  target and the bundled Saira and JetBrains Mono files in `assets/fonts/`
  (M2.8), and those faces compiled in, the fallback chain, text layout
  (kerning, letter-spacing, tabs, ellipsis and wrapping) and the glyph
  atlas, with golden scenes of both faces at scale 1 and 2 (M2.5), and a
  `tidepool-bench` suite filling the atlas, with its baseline (M2.5); and
  the first frame, in `tidepool-gui` and the `tidepool` binary: `cargo tp`
  opens a window on Linux and draws the backdrop and a line in each
  bundled face within the startup budget, and `--print-startup-time`
  prints the time it took (M2.6, ADR-0045); and in `tidepool-ui` the
  retained widget tree, with at most one widget in each node and the
  interaction state the tree tracks for it, layout in logical units, focus with `:focus-visible` and
  the tab order, input routed to the focused node or the one under the
  pointer and up through its ancestors, `TextMetrics`, draw lists in
  resolved colors with each frame's damage, and the accessibility tree, in
  which every focusable node has a role and a name (M3.1, ADR-0046); and
  the theme: `themes/tidepool-dark.toml`, jabbott.dev's tokens, read by
  `tidepool-ui`'s `theme` with `codec::toml` and bundled, with a fuzz
  target, which gives the first frame its two text colors, and `cargo xtask
  theme`, a step of `ci`, which
  prints its contrast table and fails when a pair it uses for text falls
  below WCAG AA (M3.2, ADR-0047); and in `tidepool-gui` the draw-list
  painter, `Painter`, which paints `ui`'s frames into a framebuffer with
  `gfx` and `font`, their damage only: every draw command, clips, layers,
  icons (outlines it fills with `font`'s rasterizer) and images
  registered with it; `FontMetrics`, `ui`'s `TextMetrics` with
  `font::layout`; and 23 golden scenes of `ui` trees it paints, compared
  exactly, with property tests that a frame changes only its damage
  (M3.5, ADR-0048); and in `tidepool-ui` the controls, `widgets`: buttons
  (ghost, teal, amber with cut corners, slate), chips, the text input and
  search field, edited by grapheme cluster with `tidepool-text`, the
  scrollbar, the focus outline, the status dot, the section title,
  eyebrow, kicker and breadcrumbs, the controls giving their nodes
  their role and name and painting from the theme the tree now holds, with eight more
  tokens for the site's component colors, and 16 golden scenes of them
  in each state that `gui` paints (M3.6, ADR-0049); and in `widgets` the
  collections: the list, tree view and table over a model of rows their
  owner provides, keeping nodes only for the rows on screen, with a
  selection the keyboard moves and scrolls into view, the tab strip, the
  splitter, the card with its tab and the code block, with five more
  tokens and 14 golden scenes of them in each state (M3.7, ADR-0052);
  and in `ui` the overlays: a layer of them over the tree's main root,
  where a modal one makes the rest of the window inert (no hover,
  presses, or focus), keeps Tab within itself, and gives focus back when
  it closes, with the modal dialog, the tooltip, the toast, and the
  command palette, which shows the items its owner ranks, 8 golden
  scenes of them in each state, and a `tidepool-bench` suite repainting
  the palette (M3.3, ADR-0052 to ADR-0056). The window still shows the
  first frame until the shell (M3.4).
- **Not built:** everything else. 14 of the 26 workspace members are empty
  crates with a doc comment. Each crate's `README.md` says what it has and
  which tasks add the rest.
- **Next:** the first unticked task whose dependencies are all ticked,
  skipping one whose remaining work needs an OS, tool or person this
  session does not have (`/next-task` picks it). A task's `Decide first`
  line is asked before its code.

This document also describes what is not built yet. Some of what it
mentions does not exist, so check before assuming a crate, module, command
or file is there.

## What we are building

- An IDE with a native GUI in jabbott.dev's design language, and a terminal
  UI with vim-style modal editing.
- **First-class first:** Rust (rust-analyzer, cargo) and Markdown (preview,
  outline, link checks). Other languages follow as language packs.
- **Managed toolchains:** rust-analyzer, git, the JDK, the Android SDK/NDK,
  bundletool and Xcode are installed, updated, configured and driven by
  Tidepool through their official channels (ADR-0002).
- **Mobile:** build, sign and verify Android `.apk`/`.aab` and iOS
  `.app`/`.xcarchive`/`.ipa` (iOS on macOS or a remote Mac).
- **AI:** language agents over the Tidepool Agent Protocol, plus offline
  predictive text.
- **Targets:** Linux x86_64 (X11/XWayland), macOS 13+ (universal), Windows
  10 1809+ x86_64. Development order Linux → macOS → Windows.
- **Built from the ground up:** zero third-party crates. We write our own OS
  bindings, rasterizer, font renderer, UI toolkit, text engine, parsers,
  protocols and terminal emulator.

## Read before working

| If you are touching… | Read first |
| :- | :- |
| Anything | `docs/00-overview.md`, then `docs/01-architecture.md`, then the crate's `README.md` |
| `Cargo.toml`, `Cargo.lock`, a new crate | `docs/02-dependency-policy.md` |
| `crates/core` | `docs/subsystems/core.md` |
| `crates/sys`, `crates/platform` | `docs/subsystems/sys-and-platform.md`, `docs/03-platforms.md` |
| `crates/text`, `crates/codec`, `crates/regex` | `docs/subsystems/text-and-formats.md` |
| `crates/gfx`, `crates/font`, `assets/fonts` | `docs/subsystems/rendering.md` |
| `crates/ui`, `themes/` | `docs/subsystems/ui-and-theme.md` |
| `crates/gui`, `crates/tui` | `docs/subsystems/frontends.md` |
| `crates/editor`, `crates/workspace` | `docs/subsystems/editor.md` |
| Vim mode (`crates/editor/src/vim`) | `docs/subsystems/vim-mode.md` |
| `crates/syntax`, `crates/markdown`, `crates/rpc`, `crates/lang`, `languages/` | `docs/subsystems/languages.md` |
| `crates/toolchain`, `crates/vcs`, `crates/term`, `toolchains/` | `docs/subsystems/toolchains.md` |
| `crates/mobile` | `docs/subsystems/mobile.md` |
| `crates/agent`, `agents/` | `docs/subsystems/agents.md` |
| Tests, CI, `xtask` | `docs/04-quality.md` |
| `.github/` (workflows, Dependabot, templates) | `docs/04-quality.md` ("CI"), ADR-0022 |
| A crate's `README.md` | ADR-0023, ADR-0028 |
| `diagrams/` | `diagrams/README.md`, ADR-0029 |
| The demo page: `site/`, `docs/features.jsonc`, `.github/workflows/pages.yml` | `docs/04-quality.md` ("Demo page"), ADR-0030 |
| `CONTRIBUTING.md`, `SECURITY.md` | This file; they summarize it for people |
| Packaging Tidepool itself | `docs/05-release.md` |
| `docs/roadmap.md` | Its "How to use this file", ADR-0024 |
| Why something is the way it is | `docs/decisions.md` |

## Working with the owner

These are the repository owner's instructions to any agent; the rules
below hold them too.

- Work as a senior engineer pairing with the owner: explain significant
  decisions briefly, with the risks and assumptions behind them, and help
  the owner review. Decisions stay the owner's: a task's `Decide first`
  questions are asked, and only the owner accepts an ADR.
- Before a non-trivial change, read the code, configuration, tests and
  docs it touches, and the patterns near it.
- Every piece of work, a roadmap task or not, ends with the summary that
  item 5 of "Definition of done" lists.

## Non-negotiable rules

1. **No third-party crates.** Do not add any crate outside this workspace to
   any `[dependencies]`, `[dev-dependencies]` or `[build-dependencies]`
   table, unless `docs/02-dependency-policy.md` lists it as `Approved`. If
   you believe one is needed, stop, run `/evaluate-dependency`, and ask the
   user. `cargo xtask deps` enforces this, and the Claude Code hook runs it
   after every manifest edit.
2. **Outside programs go through `toolchain`.** Only `tidepool-toolchain`
   finds, installs, updates or spawns external programs. The one exception
   is the dev-only `xtask`, which runs `cargo` itself and nothing else
   (ADR-0017). Spawn with an argv vector, never through a shell. Installs
   ask first, verify downloads against pinned SHA-256s, show licenses to
   the user, and never elevate silently. Never bundle a tool's binary
   (ADR-0002).
3. **Layering is one-way.** A crate may depend only on crates listed for it
   in `docs/01-architecture.md` → "Dependency matrix". `cargo xtask layers`
   enforces this, reading the matrix from that document.
4. **`unsafe` lives in two crates only:** `tidepool-sys` and
   `tidepool-platform`. Every other crate inherits `unsafe_code = "deny"`.
   - Every `unsafe` block carries a `// SAFETY:` comment.
   - Every `pub unsafe fn` has a `# Safety` doc section.
   - Raw OS declarations live only in `tidepool-sys`.
   - Every other library's crate root forbids `unsafe_code` in its doc
     tests, which the workspace lint does not reach:
     `#![doc(test(attr(forbid(unsafe_code))))]` (ADR-0036). Their doc-test
     code may not hold the word `unsafe` at all, and there are no
     proc-macro crates (ADR-0037). An exported macro outside the two
     crates may not put a caller's `tt` or `ident` right before braces,
     `fn`, `impl`, `trait`, `extern` or `static` it writes, and no macro
     may define a macro (ADR-0038).
   - `cargo xtask unsafe-audit` enforces this.
5. **Platform `cfg` lives in few places:** `#[cfg(target_os = ...)]`
   appears only in `sys`, `platform`, `toolchain` install plans and `xtask`
   packaging. The one exception is a `#[cfg(unix)]` in `text::file` that
   creates the save's temp file private and clears setuid and setgid, plus
   `cfg(unix)` in `text`'s tests of modes and links (ADR-0025) and
   `cfg(windows)` in its tests that make links to directories (ADR-0034),
   and
   `cfg(unix)` and `cfg(windows)` in `core::path`'s tests, which build
   paths that are not UTF-8 (ADR-0032). No check enforces this yet; review
   does.
6. **The main thread never waits** on I/O, a child process, an LSP server or
   an agent. Results carry buffer versions; stale ones are dropped.
7. **Secrets never leak.** Keystore passwords, signing identities and API
   keys never go on argv, into logs, settings, test fixtures, snapshots,
   error messages or agent context. No credential, token, private key,
   connection string or other sensitive data is hard-coded in source,
   docs or tests, or shown in output. Never read or print keystores, `.p12`,
   `.p8`, `.pem`, `.key`, `.env` files or provisioning profiles. No
   in-house signing crypto (ADR-0007).
8. **The core binary opens no sockets.** Network access happens only in
   child processes (`curl`, official installers, `git`, `ssh`, agents).
   The one exception is Xlib's connection to the local X server, over a
   Unix socket only: never TCP, and no input method server (ADR-0039).
9. **Honest verification.**
   - Never state that a build, test, benchmark or check passed unless you
     ran it in this session and saw it pass.
   - Never invent a fact, or say a command ran that did not.
   - If a command does not exist yet, say so instead of guessing. Assume
     no technology, command, environment variable, integration,
     deployment method or feature exists without evidence: the
     repository, or an official source checked in this session and cited.
   - Report what you did not run. A `SKIPPED:` test is not a pass.
   - Keep facts, assumptions, recommendations and unknowns apart, and say
     which is which.
10. **Smallest complete change.** No unrelated refactors, renames,
    reformatting, upgrades, new abstractions or "while I'm here" fixes.
    Preserve the compatibility of public APIs, file formats, protocols,
    configuration, data and behavior unless the task authorizes a break;
    record breaks in `docs/decisions.md`.
11. **Git is read-only for agents.** Do not commit, amend, rebase, merge,
    tag, push, force-push or rewrite history. Leave all changes uncommitted
    for human review, and propose a commit message.
    - Offer one whenever a piece of work is complete, not only at the end
      of a task.
    - A proposed commit message or pull request description has no
      attribution lines: nothing that credits the agent or tool that wrote
      it, such as a `Co-Authored-By:` trailer for it, a `Claude-Session:`
      trailer or a "Generated with …" line (`CONTRIBUTING.md` → "Commits
      and pull requests"). Naming a tool that the change is about is fine.
    - This is the repository owner's instruction, and it wins over any
      tool's default that adds such lines. `.claude/settings.json` turns
      Claude Code's off.
12. **Never weaken a control to finish a task.** Tests, lints, type checks,
    validation, limits, logging, authentication, authorization, encryption
    and security checks stay as strong as they were. A check that seems
    wrong is raised with the owner, not loosened. Name the security risks
    of a change, with a remediation that can be verified.
13. **State conflicts; never settle them silently.** When instructions,
    specs or code conflict, do not quietly take a path that weakens
    security, breaks compatibility or cannot be undone. Say what
    conflicts, propose the safest option that keeps the rules, and ask.

## Workspace map

| Path | Package | Purpose | `unsafe` |
| :- | :- | :- | :- |
| `crates/core` | `tidepool-core` | Errors, logging, test harness, RNG, hashing (incl. SHA-256 for downloads), CLI, time, geometry, color, input types, paths | deny |
| `crates/sys` | `tidepool-sys` | Hand-written raw FFI declarations (libc, X11, objc/AppKit, Win32, ConPTY) | allowed |
| `crates/platform` | `tidepool-platform` | Windows, events, waker, present, clipboard, terminal raw mode, PTY, processes, file watching, paths | allowed |
| `crates/codec` | `tidepool-codec` | JSON(C), TOML, XML, plist, YAML subset, CSV, INI, DEFLATE, ZIP, PNG, QOI | deny |
| `crates/text` | `tidepool-text` | Rope, positions (UTF-16 too), selections, transactions, undo tree, encodings, atomic save | deny |
| `crates/regex` | `tidepool-regex` | Linear-time regex | deny |
| `crates/gfx` | `tidepool-gfx` | CPU rasterizer, backdrop cache, damage | deny |
| `crates/font` | `tidepool-font` | TrueType, rasterizer, glyph atlas, text layout | deny |
| `crates/markdown` | `tidepool-markdown` | CommonMark + GFM parser | deny |
| `crates/rpc` | `tidepool-rpc` | JSON-RPC transport; LSP, DAP and TAP messages | deny |
| `crates/toolchain` | `tidepool-toolchain` | Managed tools: descriptors, detection, installs, downloads, processes, sessions, secrets | deny |
| `crates/syntax` | `tidepool-syntax` | Lexers (Rust, Markdown, TOML), language packs, brackets, indent | deny |
| `crates/term` | `tidepool-term` | VT parser and terminal grid | deny |
| `crates/workspace` | `tidepool-workspace` | Folders, trust, settings, files, watching, search, buffers, session | deny |
| `crates/ui` | `tidepool-ui` | Widget tree, layout, focus, theme (jabbott.dev), draw lists, accessibility tree | deny |
| `crates/lang` | `tidepool-lang` | Language sessions, LSP features, cargo tasks, Markdown services, DAP | deny |
| `crates/vcs` | `tidepool-vcs` | git over the managed CLI | deny |
| `crates/agent` | `tidepool-agent` | Agent host, TAP, context and consent, predictive text | deny |
| `crates/mobile` | `tidepool-mobile` | Android and iOS build, sign, verify, devices | deny |
| `crates/editor` | `tidepool-editor` | Editor view, modal keymaps and vim mode, all panels | deny |
| `crates/gui` | `tidepool-gui` | GUI frontend | deny |
| `crates/tui` | `tidepool-tui` | Terminal frontend | deny |
| `crates/app` | `tidepool` | The binary: main loop, wiring, startup, crash handling | deny |
| `crates/testkit` | `tidepool-testkit` | Fake LSP server, fake tools, transcripts (dev-only) | deny |
| `xtask` | `xtask` | CI checks, golden frames, Unicode tables, fuzz smoke, the demo page's build, running the benchmarks, theme contrast; later packaging | deny |
| `bench` | `tidepool-bench` | Own benchmark harness and suites | deny |
| `agents/` | (separate workspace, from M9.2) | Reference language agents: mock, local, Claude | deny |

## Commands

These exist now:

| Command | What it does |
| :- | :- |
| `cargo xtask ci` | The gate, in order: `deps`, `cargo fmt --check`, clippy (`-D warnings`), `cargo test`, `golden`, `layers`, `crates`, `features`, `unsafe-audit`, `headers`, `workflows`, `ucd --check`, `theme`. It ends with a summary; `tools` shows as "not yet implemented" |
| `cargo xtask deps` / `layers` / `crates` / `features` / `unsafe-audit` / `headers` / `workflows` / `theme` | One check each (`docs/04-quality.md` → "`xtask` checks"); `crates` checks the members' manifests, READMEs and copies of `LICENSE` and `NOTICE` (ADR-0023, ADR-0028); `features` checks that every ticked roadmap task belongs to a feature in `docs/features.jsonc`, and that each feature's `ticked` is what the roadmap ticks (ADR-0030); `theme` prints each theme's contrast table and checks that every pair `themes/*.toml` uses for text meets WCAG AA (ADR-0047) |
| `cargo xtask golden [--bless <scene>...]` | Draws the golden scenes (`cargo test` of each drawing crate's `tests/golden.rs`) and compares each with `tests/golden/<scene>.qoi`, writing `actual` and `diff` images of a failing one to `target/golden/`; `--bless` writes the references of the scenes named, after their images were looked at. A step of `ci`, without `--bless` (ADR-0044, `docs/04-quality.md` → "Golden frames") |
| `cargo xtask ucd [--check]` | Regenerates the grapheme tables from the pinned Unicode data, or with `--check` verifies them (ADR-0018) |
| `cargo xtask fuzz-smoke [--secs N] [--target T] [--seed S] [--replay]` | Runs each fuzz target (or `T`) with mutated inputs for `N` seconds, 60 by default; fails on a panic, a hang, over-allocation or a process that died, and prints the replay command for a death (ADR-0027, `docs/04-quality.md` → "Fuzz smoke") |
| `cargo xtask bench [FILTER] [--save --machine NAME]` | Builds `tidepool-bench` in the release profile and runs the suites whose names contain `FILTER`, reporting each median and p95 against `bench/baseline.json` where that was recorded on a machine like this one; a median more than 10% slower is marked, and never fails the run. `--save` writes the measured suites into the baseline, naming the machine. Not a step of `ci` (ADR-0035) |
| `cargo xtask site [--out DIR]` | Builds the demo page into `target/site`, or a new, empty or earlier `DIR`: runs the gate and the `tidepool-codec` and `tidepool-text` examples, and fills `site/`, starting only `cargo` and with no network. `pages.yml` publishes it at https://jabbott.dev/Tidepool/ after CI passes on `main` (ADR-0030) |
| `cargo xtask --help` | Lists the commands |
| `sh scripts/check-deps.sh` | The narrower dependency check the Claude Code hook falls back to when `xtask` cannot be built |
| `cargo tp` | Builds and runs the `tidepool` binary, which opens a window on Linux and shows the first frame until it is closed (M2.6); `cargo tp -- --print-startup-time` also prints the time from the start of `main` to that frame (ADR-0045) |

Planned. Until its task lands, each exits 1 with "not yet implemented":

| Command | Task |
| :- | :- |
| `cargo xtask tools` | M5.1 |
| `cargo xtask package <os>` | M11.6, M12.4, M14.4 |

`cargo tp -- --tui` starts the terminal UI from M7.1; until then the binary
refuses every argument but `--print-startup-time` (a `--` only ends the
options), and exits 2.

`xtask` never uploads, publishes, signs with real credentials, reads
secrets or opens a socket (`docs/04-quality.md`).

## Conventions

- **Toolchain and edition:** Rust edition 2024, stable pinned in
  `rust-toolchain.toml` (1.99.0). `extern` blocks are
  `unsafe extern "C" { ... }`.
- **Errors:** each crate has its own error enum implementing `Display` and
  `std::error::Error`. No `unwrap()` in library code; `expect("why")` only
  for real invariants.
- **Logging:** `tidepool_core::log` macros; `print!`/`eprint!` are denied in
  library crates. Log command lines through the redacting helper.
- **Parsers:** every parser takes `Limits`, never panics on input, and has a
  fuzz target: a test in its crate's `tests/fuzz.rs`, seeds in
  `fuzz/corpus/<target>/` and a row in `fuzz-smoke`'s table (ADR-0027).
- **Text positions** are byte offsets; lines end at LF, CR LF or a lone CR;
  a bad position is an error, never a panic (ADR-0019). Every buffer edit
  goes through its `History` (ADR-0020).
- **UI:** widgets use theme tokens (`tidepool_ui::theme`, ADR-0047),
  never literal colors; chrome labels are uppercase, user content never
  is.
- **Docs:** every `pub` item has a doc comment.
- **License header:** every source file (Rust, shell scripts, workflows,
  and the demo page's HTML, CSS and JavaScript) starts with the Apache-2.0 notice from `LICENSE`'s appendix, in its
  comment syntax, then a blank line; after a `#!` line in a script, and
  after the doctype in an HTML file (ADR-0021, ADR-0030). `cargo xtask headers` checks it, and lists the file it
  exempts by path: the generated grapheme tables, which keep the Unicode
  data's notice instead. The root `NOTICE` holds the project's name and
  copyright line.
- **Crate READMEs:** every member has a `README.md` in Nadir's shape
  (ADR-0023, ADR-0028): `# <package name>`, what it is, how it fits and
  its layer, then `## Status`, `## Usage`, `## Documentation`, `## Rules`,
  `## Checks` and `## License`, in that order. A library's Rust examples
  run as doc tests through the `ReadmeDoctests` item in its `src/lib.rs`;
  a binary's README has no Rust block. Every member also keeps
  byte-for-byte copies of the root's `LICENSE` and `NOTICE`. A change to
  what a crate has built updates its README.
- **Tests:** unit tests beside the code; integration tests in each crate's
  `tests/`; goldens in `tests/golden/`; real-tool tests gated by
  `TIDEPOOL_IT_*` and printing `SKIPPED: <reason>` otherwise. Committed
  test data never carries a name the checks read as real (`Cargo.toml`,
  `.rs`; `docs/04-quality.md` → "Files searched").

## Definition of done for a roadmap task

1. Every acceptance criterion for the task in `docs/roadmap.md` is met, and
   its `Decide first` questions were answered by the user and recorded.
2. The task's verification commands were run. Report each command and its
   real result.
3. The checkbox is ticked only after steps 1 and 2.
4. Docs are updated where behavior or design changed: the subsystem spec,
   the crate's `README.md` status, the diagrams that draw it (`diagrams/`,
   ADR-0029), and this file for a new command, crate or rule. If a decision changed, add an ADR in `docs/decisions.md`
   (`/record-decision`). A ticked task belongs to a feature in
   `docs/features.jsonc`, which the demo page lists: add it to one, or
   describe a new one, rewrite that feature's summary for what is built
   now, and update its `ticked` (`cargo xtask features` checks).
5. The final message summarizes:
   - changes and files affected
   - validation performed
   - checks not run, and why
   - assumptions
   - unresolved issues and residual risks
   - a proposed commit message, with no attribution lines (rule 11)
