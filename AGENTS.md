# Tidepool — zero-dependency IDE (Rust; Linux, macOS, Windows)

**Status:** in development, at version 0.1, and still headless: nothing
opens a window yet. `docs/roadmap.md` ticks each task once it is done and
verified.

- **Built:** the guarded workspace and its checks (M0.1–M0.4, M0.10,
  M0.11), the CI supply chain and its check (M0.8), license headers on the
  source files and their check (M0.7), the foundations in `tidepool-core`
  (M0.2), JSON and JSONC (M1.4) and TOML 1.1 (M1.7) in `tidepool-codec`,
  and in `tidepool-text` the rope, positions and grapheme boundaries
  (M1.1), selections, change sets and the undo tree (M1.2), and encodings,
  loading and the atomic save (M1.3).
- **Not built:** everything else. 22 of the 26 workspace members are empty
  crates with a doc comment; `tidepool` (`cargo tp`) builds a binary that
  does nothing. Each crate's `README.md` says what it has and which tasks
  add the rest.
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
| A crate's `README.md` | ADR-0023 |
| `CONTRIBUTING.md`, `SECURITY.md` | This file; they summarize it for people |
| Packaging Tidepool itself | `docs/05-release.md` |
| `docs/roadmap.md` | Its "How to use this file", ADR-0024 |
| Why something is the way it is | `docs/decisions.md` |

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
   - `cargo xtask unsafe-audit` enforces this.
5. **Platform `cfg` lives in few places:** `#[cfg(target_os = ...)]`
   appears only in `sys`, `platform`, `toolchain` install plans and `xtask`
   packaging. The one exception is a `#[cfg(unix)]` in `text::file` that
   creates the save's temp file private and clears setuid and setgid, plus
   `cfg(unix)` in `text`'s tests of modes and links (ADR-0025). No check
   enforces this yet; review does.
6. **The main thread never waits** on I/O, a child process, an LSP server or
   an agent. Results carry buffer versions; stale ones are dropped.
7. **Secrets never leak.** Keystore passwords, signing identities and API
   keys never go on argv, into logs, settings, test fixtures, snapshots,
   error messages or agent context. Never read or print keystores, `.p12`,
   `.p8`, `.pem`, `.key`, `.env` files or provisioning profiles. No
   in-house signing crypto (ADR-0007).
8. **The core binary opens no sockets.** Network access happens only in
   child processes (`curl`, official installers, `git`, `ssh`, agents).
9. **Honest verification.**
   - Never state that a build, test, benchmark or check passed unless you
     ran it in this session and saw it pass.
   - If a command does not exist yet, say so instead of guessing.
   - Report what you did not run. A `SKIPPED:` test is not a pass.
10. **Smallest complete change.** No unrelated refactors, renames,
    reformatting or "while I'm here" fixes. Preserve public APIs, file
    formats and protocols unless the task authorizes a break; record breaks
    in `docs/decisions.md`.
11. **Git is read-only for agents.** Do not commit, amend, rebase, merge,
    tag, push, force-push or rewrite history. Leave all changes uncommitted
    for human review, and propose a commit message.

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
| `xtask` | `xtask` | CI checks, Unicode tables; later goldens, fuzz smoke, theme contrast, packaging | deny |
| `bench` | `tidepool-bench` | Own benchmark harness and suites | deny |
| `agents/` | (separate workspace, from M9.2) | Reference language agents: mock, local, Claude | deny |

## Commands

These exist now:

| Command | What it does |
| :- | :- |
| `cargo xtask ci` | The gate, in order: `deps`, `cargo fmt --check`, clippy (`-D warnings`), `cargo test`, `layers`, `unsafe-audit`, `headers`, `workflows`, `ucd --check`. It ends with a summary; `tools` and `theme` show as "not yet implemented" |
| `cargo xtask deps` / `layers` / `unsafe-audit` / `headers` / `workflows` | One check each (`docs/04-quality.md` → "`xtask` checks") |
| `cargo xtask ucd [--check]` | Regenerates the grapheme tables from the pinned Unicode data, or with `--check` verifies them (ADR-0018) |
| `cargo xtask --help` | Lists the commands |
| `sh scripts/check-deps.sh` | The narrower dependency check the Claude Code hook falls back to when `xtask` cannot be built |
| `cargo tp` | Builds and runs the `tidepool` binary, which does nothing until M2.6 |

Planned. Until its task lands, each of the first six exits 1 with "not
yet implemented" (`bench`'s message still says it has no task; M1.9
corrects it), and the last is not a command yet and exits 2:

| Command | Task |
| :- | :- |
| `cargo xtask fuzz-smoke [--secs N] [--target T]` | M1.6 |
| `cargo xtask bench [filter]` | M1.9 |
| `cargo xtask golden [--bless]` | M2.4 |
| `cargo xtask theme` | M3.2 |
| `cargo xtask tools` | M5.1 |
| `cargo xtask package <os>` | M11.6, M12.4, M14.4 |
| `cargo xtask crates` | M0.9 (ADR-0023) |

`cargo tp -- --tui` starts the terminal UI from M7.1; until then the binary
ignores its arguments and exits 0.

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
  fuzz target (from M1.6).
- **Text positions** are byte offsets; lines end at LF, CR LF or a lone CR;
  a bad position is an error, never a panic (ADR-0019). Every buffer edit
  goes through its `History` (ADR-0020).
- **UI:** widgets use theme tokens, never literal colors; chrome labels are
  uppercase, user content never is.
- **Docs:** every `pub` item has a doc comment.
- **License header:** every source file (Rust, shell scripts, workflows)
  starts with the Apache-2.0 notice from `LICENSE`'s appendix, in its
  comment syntax, then a blank line; after a `#!` line in a script
  (ADR-0021). `cargo xtask headers` checks it, and lists the file it
  exempts by path: the generated grapheme tables, which keep the Unicode
  data's notice instead. The root `NOTICE` holds the project's name and
  copyright line.
- **Crate READMEs:** every member has a `README.md` saying what it is, its
  status, its spec and its rules (ADR-0023). A change to what a crate has
  built updates its README.
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
   the crate's `README.md` status, and this file for a new command, crate or
   rule. If a decision changed, add an ADR in `docs/decisions.md`
   (`/record-decision`).
5. The final message summarizes:
   - changes and files affected
   - validation performed
   - checks not run, and why
   - assumptions
   - unresolved issues and residual risks
   - a proposed commit message
