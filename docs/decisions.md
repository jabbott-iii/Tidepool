# Architecture decision records

New ADRs are appended at the end using the template below; `/record-decision`
does this for you. Never edit an accepted ADR's decision. Supersede it with a
new ADR and set the old one's status to `Superseded by ADR-NNNN`.

Only the user accepts an ADR. `Proposed` ADRs are the design until the user
decides; the roadmap task that first depends on one asks for the decision.

```markdown
## ADR-NNNN — <title>

- **Status:** Proposed | Accepted | Superseded by ADR-NNNN
- **Date:** YYYY-MM-DD
- **Context:** <forces at play, constraints, what prompted this>
- **Decision:** <what we will do>
- **Consequences:** <good and bad results, follow-up work>
- **Alternatives considered:** <option — why not>
- **Revisit when:** <a concrete trigger>
```

---

## ADR-0001 — Zero third-party crates: Rust `std` plus hand-written OS bindings

- **Status:** Accepted
- **Date:** 2026-10-31
- **Context:** The user's rule: Tidepool "cannot have any dependencies
  outside of what is absolutely necessary."
- **Decision:**
  - The approved third-party crate set is empty.
  - OS libraries are reached through hand-written FFI in `tidepool-sys`.
  - Exceptions follow `docs/02-dependency-policy.md` and need user approval.
- **Consequences:**
  - We write the windowing, rendering, fonts, UI toolkit, text engine,
    parsers, protocols, terminal emulator and test harnesses.
  - No transitive supply-chain risk in the binary; small builds.
  - Much more code; the roadmap stages it so Tidepool is usable early.
- **Alternatives considered:** a curated small set (`winit`, `tiny-skia`,
  `ropey`, `regex`). Rejected under the user's rule.
- **Revisit when:** a switch trigger in the build-vs-buy register fires.

## ADR-0002 — Outside tools are managed by Tidepool, not bundled or rebuilt

- **Status:** Accepted
- **Date:** 2026-10-31
- **Context:** The user asked for rust-analyzer, git, the Android SDK and
  Xcode to be integrated into the IDE. Options were: manage them, bundle
  them where licenses allow, or rebuild them. Xcode can only come from
  Apple, and the Android SDK's license doesn't allow redistribution. The
  user chose "managed by Tidepool" on 2026-10-31.
- **Decision:**
  - `tidepool-toolchain` finds, installs, updates, configures and
    supervises each tool, through its official channel only.
  - Installs ask first, show the source, verify downloads against pinned
    SHA-256 values, show licenses for the user to accept, and never elevate
    silently: commands needing admin run in the integrated terminal when
    the user presses Run.
  - Each tool gets a Toolchains-panel row and, where useful, its own panel
    (SDK manager, devices, simulators, git).
  - Tools install to their standard locations so other tools share them.
- **Consequences:**
  - The zero-crate rule holds; tool updates and security fixes come from
    their vendors.
  - Tidepool must track tool output formats and versions (descriptors in
    `toolchains/`, fake-tool tests, gated real-tool tests).
  - First-run setup needs a network connection.
- **Alternatives considered:** bundling rust-analyzer and git (bigger
  download, we would own their security updates); rebuilding git and Rust
  analysis in-house (very large; impossible for the SDKs).
- **Revisit when:** a tool's official channel becomes unusable for
  managed installs, or users ask for an offline bundle.

## ADR-0003 — Native GUI first; the terminal UI follows as its own milestone

- **Status:** Accepted
- **Date:** 2026-10-31
- **Context:** The UI must match jabbott.dev (ADR-0004), which needs real
  fonts, translucency and cut corners; a terminal can only match colors.
  The user chose GUI first on 2026-10-31, then asked for vim support in the
  terminal UI, so the TUI is a planned milestone, not backlog.
- **Decision:**
  - The GUI is the default frontend and lands first (M2–M4).
  - The TUI lands in M7, rendering the same `ui` draw lists onto cells.
  - Features live in `editor`; frontends stay thin.
- **Consequences:** the first usable editor arrives later than a TUI-first
  plan would allow; the UI is built once and works in both.
- **Alternatives considered:** TUI first (faster to a usable editor, but the
  look would come late); both in parallel (twice the early work).
- **Revisit when:** the GUI path slips more than one milestone behind plan.

## ADR-0004 — The UI follows jabbott.dev's design language; Saira and JetBrains Mono are bundled

- **Status:** Accepted
- **Date:** 2026-10-31
- **Context:** The user asked for the UI to match or mimic jabbott.dev. The
  site is a dark "capsuleer UI": deep-space background, translucent hairline
  panels, teal signal accents, amber primary actions with cut corners,
  squared uppercase letter-spaced Saira type and JetBrains Mono for code.
  ADR-0001 and ADR-0009 rule out fetching fonts at runtime.
- **Decision:**
  - The default theme, `tidepool-dark`, uses the site's tokens and
    components as specified in `docs/subsystems/ui-and-theme.md`.
  - Static TrueType instances of Saira and JetBrains Mono ship in
    `assets/fonts/` with their SIL OFL 1.1 license files.
  - Text colors must meet WCAG AA contrast; `cargo xtask theme` checks it.
- **Consequences:** a distinctive, consistent look across the owner's
  projects; the font rasterizer must render both faces well; the binary
  grows by the font files (well within the size budget).
- **Alternatives considered:** a neutral theme with a jabbott.dev accent
  (loses the identity); system fonts only (inconsistent look).
- **Revisit when:** jabbott.dev's design changes; re-sync tokens then.

## ADR-0005 — Rust and Markdown are first-class before other languages

- **Status:** Accepted
- **Date:** 2026-10-31
- **Context:** The user set the initial focus on Rust and Markdown.
- **Decision:** hand-written Rust and Markdown lexers, rust-analyzer
  integration, cargo tasks and a Markdown preview land in M6; other
  languages come as language packs in M13.
- **Consequences:** the language-pack format is designed in M6 but filled
  out later.
- **Alternatives considered:** a generic grammar engine first. Slower to
  excellent Rust and Markdown support.
- **Revisit when:** the user prioritizes another language.

## ADR-0006 — Vim mode is a frontend-agnostic modal keymap engine in `editor`

- **Status:** Proposed
- **Date:** 2026-10-31
- **Context:** The user requires vim support in the terminal UI. Building
  it in the TUI alone would leave the GUI without it and duplicate the
  command layer.
- **Decision:**
  - `editor` has a keymap engine with modes from M4 (`Normal`, `Insert`,
    `Visual`, `VisualLine`, `VisualBlock`, `OperatorPending`, `Replace`,
    `CommandLine`); the default keymap uses only `Insert`.
  - Vim mode supports the subset in `docs/subsystems/vim-mode.md`: motions,
    operators, text objects, counts, registers, marks, dot-repeat, macros,
    search, and the listed ex commands. No Vimscript.
  - It works in both frontends via `editor.keymap = "vim"`; the first TUI
    launch offers to turn it on.
- **Consequences:** the command model must be modal-aware from M4; a vim
  conformance suite runs each supported command against fixtures.
- **Alternatives considered:** embed or drive Neovim as a child process
  (heavy, a managed-tool dependency for core editing); TUI-only vim mode.
- **Revisit when:** users need Vimscript plugins or `.vimrc` import beyond
  the mappings subset.

## ADR-0007 — Signing is delegated to `apksigner`, `jarsigner` and `codesign`

- **Status:** Proposed
- **Date:** 2026-10-31
- **Context:** Tidepool must sign `.apk` and `.aab` files and the iOS
  equivalents. Signing cryptography is a specialist domain with severe
  failure modes. Android's docs state `apksigner` cannot sign app bundles;
  `jarsigner` signs them.
- **Decision:**
  - APK: `zipalign -p 4`, then `apksigner sign`, then `apksigner verify`.
  - AAB: `bundletool build-bundle`, then `jarsigner`, then
    `jarsigner -verify`.
  - iOS: `codesign` with the identity in the Keychain, then
    `codesign --verify --deep --strict`.
  - Passwords never go on argv; they go through stdin, a per-child
    environment variable, or a 0600 file in a 0700 temp directory deleted
    after use. Tidepool never stores them in v1.
- **Consequences:** no in-house signing crypto; signing needs the JDK, SDK
  build-tools or Xcode installed (managed, ADR-0002).
- **Alternatives considered:** native APK Signature Scheme v2/v3 and JAR
  signing in Rust. Rejected: crypto risk, and it would need an approved
  exception or in-house RSA/ECDSA.
- **Revisit when:** a signing tool becomes unavailable on a supported host.

## ADR-0008 — iOS builds run on macOS: locally, or on a remote Mac over SSH

- **Status:** Proposed
- **Date:** 2026-10-31
- **Context:** Building and signing iOS apps needs Xcode's SDKs and tools,
  which run only on macOS; Apple's license terms restrict where the SDKs may
  be used. The owner develops on Linux.
- **Decision:** on macOS, run the pipeline locally. On Linux and Windows,
  sync the project to a configured Mac with `rsync` over `ssh`, run the same
  pipeline there, and copy artifacts back. `rcodesign` is optional for
  re-signing only.
- **Consequences:** iOS work needs access to a Mac; remote mode needs SSH
  keys the user manages.
- **Alternatives considered:** cross-compiling against copied SDKs on Linux
  (license and reliability problems).
- **Revisit when:** Apple changes its tooling or licensing.

## ADR-0009 — Language agents are external processes speaking TAP; the core binary opens no sockets

- **Status:** Proposed
- **Date:** 2026-10-31
- **Context:** AI features need models that live behind HTTPS APIs or local
  servers. TLS in-house is a specialist domain; an AI SDK is a dependency.
- **Decision:**
  - Agents are child processes speaking the Tidepool Agent Protocol (TAP):
    JSON-RPC 2.0 with LSP framing over stdio (`docs/subsystems/agents.md`).
  - The IDE assembles all context (with a deny-list and consent) and treats
    agent output as untrusted. Agent edits are proposals shown as diffs.
    Agents cannot run commands in TAP v1.
  - HTTPS anywhere in Tidepool goes through a `curl` child process.
- **Consequences:** the core binary has no network code to secure; agents
  can be written in any language; reference agents live in `agents/`.
- **Alternatives considered:** in-process HTTP/TLS (crypto exception
  needed); exposing Tidepool as an MCP server only (backlog item instead).
- **Revisit when:** a required agent feature can't be expressed in TAP.

## ADR-0010 — Linux GUI: X11 through `dlopen`'d libX11; Wayland through XWayland

- **Status:** Proposed
- **Date:** 2026-10-31
- **Context:** Nadir already ships hand-written, `dlopen`-loaded X11
  bindings that work; native Wayland needs xdg-shell and keymap handling
  equivalent to `libxkbcommon`.
- **Decision:** follow Nadir's pattern (port its approach, not its crate).
  Native Wayland is backlog.
- **Consequences:** works on X11 and XWayland desktops; fractional scaling
  on Wayland is limited to what XWayland offers.
- **Alternatives considered:** the X11 wire protocol over a `std` Unix
  socket (no FFI at all, but more protocol code to write); native Wayland
  first.
- **Revisit when:** a target distro ships without XWayland by default.

## ADR-0011 — CPU software rendering with damage tracking; no GPU API in v1

- **Status:** Proposed
- **Date:** 2026-10-31
- **Context:** An IDE redraws small regions most of the time. GPU backends
  add three APIs' worth of FFI.
- **Decision:** `tidepool-gfx` rasterizes `ui` draw lists on the CPU into a
  framebuffer and redraws only damaged rectangles; `platform` presents them.
  Translucent panels are composited over a cached backdrop; blur is
  approximated once per backdrop change, not per frame.
- **Consequences:** deterministic goldens across OSes; one renderer;
  performance must be measured at 1440p and 4K.
- **Alternatives considered:** OpenGL 3.3 everywhere (as Nadir); Metal/D3D.
- **Revisit when:** the frame budget is missed at 1440p after profiling.

## ADR-0012 — Event-driven main loop on plain threads; no async runtime

- **Status:** Proposed
- **Date:** 2026-10-31
- **Context:** Tidepool waits on input, child-process pipes, file events
  and timers, and must never block typing.
- **Decision:** the loop in `docs/01-architecture.md`: block on frontend
  events with a deadline, wake from other threads through a `Waker`, handle
  input first, bounded message handling, render only on damage.
- **Consequences:** idle Tidepool uses no CPU; every slow operation lives
  on a worker or reader thread.
- **Alternatives considered:** a fixed-rate render loop (wastes CPU); an
  async runtime (a dependency, and more complexity than needed).
- **Revisit when:** profiling shows thread count or context switches as a
  bottleneck.

## ADR-0013 — Settings, keymaps, themes, language packs and tool descriptors are TOML

- **Status:** Accepted
- **Date:** 2026-10-31
- **Context:** Tidepool must parse TOML anyway (`Cargo.toml`). One
  human-edited format keeps the codec surface small.
- **Decision:** TOML with a version key for every user-editable file; JSON
  only where a protocol requires it.
- **Consequences:** `codec::toml` must preserve comments and key order on
  round-trip for files Tidepool writes back (settings).
- **Alternatives considered:** JSONC (as Nadir uses).
- **Revisit when:** comment-preserving TOML writes prove too costly.

## ADR-0014 — Workspaces are untrusted until the user trusts them

- **Status:** Proposed
- **Date:** 2026-10-31
- **Context:** Opening a Rust project with rust-analyzer can run its build
  scripts and proc macros; Gradle and `cargo` tasks run project code;
  agents could read the workspace.
- **Decision:** a new folder opens untrusted: no tasks, no build scripts or
  proc macros in rust-analyzer, no Gradle, and agents get only the open
  buffer with consent. One prompt trusts the folder; the decision is stored
  in user data.
- **Consequences:** slightly less help in a folder until it's trusted.
- **Alternatives considered:** trust by default (code execution on open).
- **Revisit when:** a sandboxing option makes untrusted execution safe.

## ADR-0015 — Platform order: Linux, then macOS, then Windows

- **Status:** Proposed
- **Date:** 2026-10-31
- **Context:** Development and CI run on Linux. iOS work needs macOS.
- **Decision:** Linux through M10; macOS in M11 with iOS; Windows in M12.
- **Consequences:** Windows users wait longest.
- **Alternatives considered:** Nadir's Linux → Windows → macOS order.
- **Revisit when:** Windows users become the priority.

## ADR-0016 — SHA-256 in `core` for download integrity only

- **Status:** Accepted
- **Date:** 2026-10-31
- **Context:** Managed installs download installers and jars (ADR-0002) and
  must verify them against pinned checksums.
- **Decision:** implement SHA-256 (FIPS 180-4) in `tidepool-core::hash`,
  tested against the NIST vectors, used only to compare a download with a
  pinned digest. No signatures, key handling or other crypto.
- **Consequences:** downloads are checked without a crate; anything beyond
  hashing still goes through ADR-0007's tools.
- **Alternatives considered:** shelling out to `sha256sum`/`shasum`
  (inconsistent across OSes); the `sha2` crate.
- **Revisit when:** a source publishes only signatures, not digests.

## ADR-0017 — `xtask` runs `cargo` itself, as a narrow exception to rule 2

- **Status:** Accepted; extended by ADR-0030 (`cargo xtask site`'s children: the gate's steps, run in-process, and `cargo run --example` of one example each of `tidepool-codec` and `tidepool-text`)
- **Date:** 2026-11-01
- **Context:** `AGENTS.md` rule 2 says only `tidepool-toolchain` finds,
  installs, updates or spawns outside programs. `cargo xtask ci` (M0.3)
  exists to run cargo: `cargo metadata` and `cargo locate-project` for
  `deps`, `layers` and `unsafe-audit`, and `cargo fmt`, `cargo clippy` and
  `cargo test` for the gate. The matrix lets `xtask` use only `core` and
  `codec` (`docs/01-architecture.md`), and `toolchain` does not exist
  before M5.1. The Claude Code hook also builds `xtask` before it runs, so
  everything `xtask` links is compiled before any check
  (`docs/04-quality.md` → "Command line and rules").
- **Decision:**
  - `xtask`, which is dev-only and never shipped, may spawn `cargo`
    directly with `std::process::Command`. That is the cargo running it
    (`CARGO`, if not empty), or else `cargo` on the `PATH`.
  - Its only children are `cargo metadata`, `cargo locate-project`, and
    `ci`'s fmt, clippy and test steps.
  - Every child is an argv vector, never a shell. Each runs in the root,
    with standard input closed and `CARGO_NET_OFFLINE=true`.
  - The Tidepool binary and every library crate still follow rule 2.
- **Consequences:**
  - `xtask` stays at `core` and `codec`, and its build stays small and
    fast for the hook.
  - The exception must be named in `AGENTS.md` rule 2 once accepted. That
    edit needs the user's approval, so it is not made here.
  - Any other program `xtask` runs needs this ADR extended first:
    `package`'s `lipo`, `iconutil` and `rc.exe` (`docs/05-release.md`,
    M11.6, M12.4, M14.4), and any child process `fuzz-smoke` uses (M1.6).
- **Alternatives considered:**
  - `xtask` depends on `tidepool-toolchain`. It does not exist until M5.1,
    it would need a matrix change, and it would put descriptors, detection
    and installs into what the hook compiles before checking.
  - A shell script for the gate. Rule 2 forbids a shell, and it would not
    be portable to Windows.
- **Revisit when:** `xtask` needs any program other than `cargo` (M1.6 or
  M11.6, whichever comes first), or `tidepool-toolchain` gains a process
  API that `xtask` could use within the matrix.

## ADR-0018 — Unicode data: the official UCD files are kept in the repository, and `xtask ucd` generates the tables

- **Status:** Accepted
- **Date:** 2026-11-01
- **Context:** `pos` finds extended grapheme cluster boundaries (UAX #29,
  roadmap M1.1). Those need three Unicode properties that `std` does not
  expose: `Grapheme_Cluster_Break`, `Extended_Pictographic` and
  `Indic_Conjunct_Break`. Rust 1.99's `std` uses Unicode 17.0.0
  (`char::UNICODE_VERSION`). Build scripts may not download anything
  (`docs/02-dependency-policy.md`), and `xtask` never calls the network
  (`docs/04-quality.md`). The user approved the download and this record
  on 2026-11-01.
- **Decision:**
  - The official Unicode 17.0.0 files are kept, unchanged, in
    `crates/text/ucd/17.0.0/`. They were downloaded once, on 2026-11-01,
    from `https://www.unicode.org/Public/17.0.0/`:
    - `ucd/auxiliary/GraphemeBreakProperty.txt`, `ucd/emoji/emoji-data.txt`
      and `ucd/DerivedCoreProperties.txt` are the property sources;
    - `ucd/auxiliary/GraphemeBreakTest.txt` is the conformance test, used
      only by tests;
    - `license.txt`, from `https://www.unicode.org/license.txt`, is the
      Unicode License v3.
  - `xtask` pins each file's SHA-256 and checks it before reading the
    file. `cargo xtask ucd` writes the generated tables into
    `tidepool-text`. `cargo xtask ucd --check`, a `ci` step, fails when the
    committed tables differ from what the files produce.
  - The files are an asset with a compatible license, listed in
    `docs/02-dependency-policy.md` → "Assets". They are data, not code, so
    they are not a dependency.
  - The generated tables ship in the binary, so `LICENSES.txt` carries the
    Unicode license next to the fonts' licenses.
  - The Unicode version follows `std`. A test fails when the tables'
    version differs from `char::UNICODE_VERSION`. A Rust upgrade that
    changes it adds the new version's files, moves the pins and regenerates
    the tables, as a change of its own.
- **Consequences:**
  - About 1.47 MB of text per Unicode version in the repository
    (`DerivedCoreProperties.txt` is 1.13 MB of it).
  - Segmentation is tested against every case in `GraphemeBreakTest.txt`.
  - The tables and `std`'s character properties always agree on the
    Unicode version.
- **Alternatives considered:**
  - Downloading the files at build time: build scripts may not download.
  - Hand-written tables: no reproducible link to the source data.
  - Keeping only the `Indic_Conjunct_Break` lines of
    `DerivedCoreProperties.txt` (tens of KB): the kept file could no longer
    be checked against the official one.
  - The user downloading the files on each machine: slower, and nothing
    gained.
- **Revisit when:** Rust's `char::UNICODE_VERSION` changes, or another
  crate needs Unicode data: general categories and scripts for `regex`
  (M4.4), or display width for the editor and terminal. Those add their
  files to this directory under the same rules.

## ADR-0019 — Text positions: byte offsets are canonical; lines end at LF, CR LF or CR; bad positions are errors

- **Status:** Accepted
- **Date:** 2026-11-01
- **Context:** M1.1 fixes the position model that `editor`, vim mode,
  `lang` (LSP), `syntax`, `regex` and `agent` all build on. LSP 3.17 says
  lines end at `\n`, `\r\n` or `\r`, lets client and server agree on
  `utf-8`, `utf-16` or `utf-32` columns, and forbids a position between `\r`
  and `\n`. rustc, rust-analyzer and git treat a lone CR as an ordinary
  character. Positions arrive from outside: LSP servers, agents, cargo's
  JSON and session files. The user accepted this record on 2026-11-01.
- **Decision:**
  - A position in a `Rope` is a byte offset; ranges are byte ranges. Char
    and UTF-16 offsets, and columns in bytes, UTF-16 units or chars
    (`Unit::{Byte, Utf16, Char}`, LSP's three encodings), are conversions,
    always given the unit explicitly.
  - Lines end at LF, CR LF or a lone CR. VT, FF, NEL, LS and PS are
    ordinary characters. A text has one more line than it has line breaks.
  - The offset between a CR and its LF is a valid position. It is on the
    line that CR LF ends, one column past the line's content.
  - Rope chunks never split a CR LF pair.
  - Edits, slices, iterator constructors and conversions return
    `Result<_, TextError>` and change nothing on error. Boundary and
    grapheme queries are total. Nothing panics on a bad position.
- **Consequences:**
  - A file with a lone CR shows a line break where rustc and rust-analyzer
    see none. `lang` maps positions between the two (M6.3, M6.4).
  - A malformed position from outside cannot crash the IDE.
  - LSP cannot express the CR|LF offset; `lang` moves it before sending.
- **Alternatives considered:**
  - LF-only lines (rustc, git): differs from LSP clients such as VS Code.
  - Char offsets as the canonical unit (as ropey does): every LSP and
    file-offset use would pay a conversion.
  - Panicking on bad positions, as `std` does for strings: an outside
    position could crash the IDE.
  - An `InsideCrlf` error for the CR|LF offset: byte to line/column would
    then fail on an offset that edits accept.
- **Revisit when:** a supported language server disagrees in practice, or
  vim mode needs another view of lines.

## ADR-0020 — Edits are canonical change sets of byte-range replacements holding rope text; undo is a revision tree that applies every step to the buffer's rope

- **Status:** Accepted
- **Date:** 2026-11-01
- **Context:** M1.2 fixes the edit and undo model that these build on:
  - `editor`: multi-cursor, clipboard, buffers;
  - vim mode: one undo step per typed command, Insert session or macro
    replay;
  - `lang`: LSP `TextEdit`s and incremental sync;
  - `syntax`: changed ranges;
  - `agent`: a proposal is one undo step.

  Untrusted LSP servers and agents send edits. Positions are byte offsets
  (ADR-0019). The main thread never waits (ADR-0012), so typing, 10,000
  cursors, long macros and 100 MB edits must stay cheap, and saves finish
  on workers. The user accepted this record on 2026-11-01.
- **Decision:**
  - **`ChangeSet`:** a sorted list of replacements in old-text byte offsets,
    each holding its new text as a `Rope`. Touching edits merge and no-ops
    vanish, so equality is structural.
    - Built from the text and edits in any order, with a stable sort.
    - Overlapping edits are an error.
    - Inserts at one offset keep their order.
    - A result over `isize::MAX` bytes is an error.
    - `invert` takes the text before.
  - **Mapping a position:**
    - kept text shifts;
    - the edges of a replaced range stick to the kept text beside them;
    - at a pure insert, or strictly inside a replaced range,
      `Assoc::Before`/`After` decides.
  - **`Selection`:** one or more `Cursor { anchor, head }`, sorted and
    merged. The primary survives.
    - Ranges merge when they share a byte.
    - A caret joins a range it is inside or at the edge of (the earlier
      one, where two touch).
    - Equal carets become one.
    - Touching ranges stay apart.
  - **`Transaction::change_each`** (multi-cursor):
    - merges overlapping deletions and identical edits from different
      cursors;
    - rejects other overlaps;
    - maps cursors through the unmerged edits, so seams are kept.
  - **`History`:** an arena tree.
    - It applies every commit, undo, redo and jump to the caller's rope,
      after checking by pointer identity that it is the tree the history
      last produced.
    - Each returns the text before and one change set per hop.
    - Grouping is `New`, `Command(GroupId)` (ids unique in the process) or
      `Coalesce { kind, window }` with an `Instant` the caller supplies.
    - An open revision keeps its change as a few pieces, merged like a
      binary counter.
    - Saves use `checkpoint()` (a sealed revision and its text), then
      `mark_saved(&Checkpoint)`.
    - Redo takes the child created or visited last; older branches stay
      reachable through `jump_to`.
    - No pruning in v1. Revision ids are opaque and the root is a method.
- **Consequences:**
  - Inverting and undoing a huge delete costs O(1) to O(log n) and shares
    memory.
  - An Insert session stays one replacement.
  - A group of K scattered edits costs O(K log K).
  - Every buffer edit must go through its history. After an outside edit,
    every call that takes the rope (`commit`, `undo`, `redo`, `jump_to`)
    is `HistoryMismatch` until the buffer restores `history.text()`.
  - `lang` sends changes last first, on `Step.before`.
  - Mapping through a composed set is exact only away from its
    replacements, so the history returns each hop separately.
  - Mapping through a built change set forgets the seams between touching
    edits.
  - An asynchronous edit in the middle of an Insert session splits it into
    two undo steps.
  - History memory grows with the session, deleted secrets included, until
    pruning is designed.
- **Alternatives considered:**
  - Retain/delete/insert lists as the stored form: mapping is O(k), and LSP
    needs a conversion. They are kept only as compose's internal view.
  - Keeping touching edits apart: no canonical form and no structural
    equality. `change_each` keeps seams instead.
  - Change sets that carry their deleted text and are checked by content:
    O(deleted) per check.
  - Checks by length or by size fingerprint: they miss outside edits that
    keep the size.
  - The caller applies edits and the history only records them: the text
    and the history can diverge.
  - Begin/end groups, or a bare `Join`: state can be left unbalanced, and
    unrelated asynchronous edits join a session.
  - Re-composing a whole group on every join: O(K²) for macros.
  - `mark_saved` by revision id: a snapshot of a still-open revision could
    be marked saved after more typing.
  - `String` texts: invert copies 100 MB.
  - A rope snapshot per revision: no mapping and no incremental sync.
  - Pruning now: no consumer needs it yet.
- **Revisit when:**
  - undo must persist across restarts (`Instant` cannot);
  - history memory shows up in profiles or in large-file editing;
  - a multi-cursor edit in M4.2's command tests or benchmarks takes more
    than one 16 ms frame;
  - collaborative editing, or rebasing agent proposals, needs transforms;
  - vim conformance needs Vim's line-based mark rules (M7.3).

## ADR-0021 — Every source file starts with the Apache-2.0 header of `LICENSE`'s appendix; `cargo xtask headers` checks it; the RFC 8259 examples keep the IETF Trust's notice

- **Status:** Accepted; extended by ADR-0030 (HTML, CSS and JavaScript files carry the header too, as the demo page's do)
- **Date:** 2026-11-02
- **Context:**
  - On 2026-11-01, asked which of Nadir's conventions Tidepool should
    adopt, the user chose a license header on every source file, enforced
    by a `cargo xtask headers` check. Nadir's ADR-0037 is the model; the
    details below adapt it, and the user reviews them with the change that
    records this ADR.
  - `LICENSE` is the Apache License 2.0. Its appendix gives the notice to
    attach, with the holder filled in: "Copyright 2026 Joseph Anthony
    Abbott III". No source file carries it yet.
  - A script cannot start with a comment: its `#!` line comes first.
  - `crates/text/src/pos/grapheme/tables.rs` is generated by
    `cargo xtask ucd` from the Unicode Character Database, and its header
    already names its sources and the Unicode License v3 (ADR-0018).
  - `crates/codec/tests/json/rfc8259_image.json` and
    `rfc8259_locations.json` are the examples of IETF RFC 8259, section 13,
    ported with Nadir's JSON tests. Under the IETF Trust's Legal
    Provisions they are Code Components under the Revised BSD License,
    whose notice must go with them (Nadir's ADR-0037). Nadir keeps that
    notice in its `tests/json.rs`; Tidepool's port does not have it: no
    file under `crates/` mentions the IETF.
  - The user accepted this record on 2026-11-02 (M0.7) with two answers:
    - The open question below: Tidepool gets a root `NOTICE`, as Nadir
      has, holding the project's name and copyright line and nothing else,
      which ships beside `LICENSE` in each package (`docs/05-release.md`).
    - An exception to "Files that carry it" below:
      `.github/workflows/codeql.yml`, still GitHub's starter workflow word
      for word and with no notice of its own, is exempt by path, beside
      the grapheme tables, until M0.8 rewrites it as the project's own and
      gives it the header.
- **Decision:**
  - **The header** is the appendix's notice, word for word, in the file's
    comment syntax, followed by one blank line. It goes on line 1, or
    straight after a `#!` line. In Rust:

    ```rust
    // Copyright 2026 Joseph Anthony Abbott III
    //
    // Licensed under the Apache License, Version 2.0 (the "License");
    // ...
    // limitations under the License.
    ```

  - **Files that carry it:** Rust (`.rs`), shell scripts (`.sh`), and the
    workflows in `.github/workflows/`. A new kind of source file joins the
    check when the first one appears.
  - **Files that do not:** configuration and data (TOML, Markdown, JSON and
    JSONC, YAML outside the workflows such as `dependabot.yml` and the issue
    forms, test vectors, fuzz corpora, the UCD files, fonts, goldens), and
    generated files that carry another license's notice. Today that is
    only the grapheme tables, which the check lists by path.
  - **`cargo xtask headers`** (M0.7) runs in `cargo xtask ci` after
    `unsafe-audit`. It fails a file whose header is missing, altered or
    out of place, naming the line and what it should be. A unit test
    compares its text with `LICENSE`'s appendix.
  - **New source files get the header now,** before M0.7 lands, so the
    check finds only the files that predate this record.
  - **The RFC 8259 examples** keep the IETF Trust's legend beside them, in
    `crates/codec/tests/json.rs`, as Nadir's tests do.
  - **Open:** whether Tidepool gets a root `NOTICE` file, as Nadir has. The
    license does not require one; M0.7 asks the user.
- **Consequences:**
  - Good: every source file says whose it is and under which license, and
    a file copied out of the repository keeps saying so; the check keeps
    the text the same everywhere; the RFC 8259 examples carry the notice
    their license asks for.
  - Bad: each Rust file grows by 14 lines and its lines move down, so
    M0.7's change touches every source file at once. A new kind of source
    file is unchecked until the check knows it.
- **Alternatives considered:**
  - An `SPDX-License-Identifier: Apache-2.0` line alone: shorter, but not
    the appendix's notice, and not what Nadir does.
  - The header on the generated tables too: the file would name two
    licenses for data that is Unicode's.
  - No headers: the license covers the repository, but a file copied out of
    it carries nothing.
- **Revisit when:** a file that is not the project's own enters the
  repository, or a contributor's file needs its own copyright line.

## ADR-0022 — CodeQL scans the Rust code and the workflows with pinned actions; Dependabot updates the pins; `cargo xtask workflows` enforces the CI supply-chain rules

- **Status:** Accepted; extended by ADR-0030 (the `pages.yml` row: GitHub's two Pages actions besides checkout and cache, and the deploy job's `pages` and `id-token` writes)
- **Date:** 2026-11-02
- **Context:**
  - `docs/04-quality.md` → "CI" says every action is pinned by its full
    commit SHA with its tag in a comment, and that the workflow's token
    gets `contents: read`. `ci.yml` (M0.4) follows that; nothing checks
    it.
  - On 2026-11-01 the user added `.github/workflows/codeql.yml` from
    GitHub's template. It uses `actions/checkout@v7`,
    `github/codeql-action/init@v4` and `github/codeql-action/analyze@v4` by
    tag, has no top-level `permissions`, grants its job `packages: read`
    and `actions: read` beside what it needs, and leaves the checkout's
    token in `.git/config` (no `persist-credentials: false`). The same day
    the user added `.github/dependabot.yml` from GitHub's starter file. Its
    `package-ecosystem` is empty, so it updates nothing.
  - On 2026-11-01 the user chose to adopt Nadir's workflow check. Nadir's
    ADR-0032 settled the same two template files there.
- **Decision:**
  - **CodeQL** is allowed, in `codeql.yml` only: `github/codeql-action/init`
    and `analyze`, pinned by SHA with the tag in a comment, as is its
    `actions/checkout`. It scans `rust` and `actions` (the workflows
    themselves) with no build, using the `security-extended` queries, on
    pushes to `main`, on pull requests, weekly and by hand. The workflow's
    top-level permissions are `{}`; its job has `contents: read` and
    `security-events: write`. A newer push to a pull request cancels its
    older run, as in `ci.yml`.
  - **Dependabot** watches `github-actions` only: weekly, for releases at
    least 7 days old (`cooldown`), with the week's updates in one pull
    request whose commits start `ci:`. It has no `cargo` entry: there are
    no crates to update, and `cargo xtask deps` fails on one.
  - **`cargo xtask workflows`** (M0.8) runs in `cargo xtask ci`. It reads
    each file in `.github/workflows/` line by line and fails:
    - a workflow with no row in its table of the actions each workflow may
      use and the jobs that may grant `write`, with which permissions;
    - an action not in its workflow's row, not pinned by 40 hex digits, or
      with no `# v…` comment on its line;
    - top-level permissions other than `{}` or `contents: read`, or none;
    - a job's `write` on a permission its row does not name, or permissions
      written on one line (`write-all`, `read-all`, `{ … }`), which it
      cannot read;
    - `pull_request_target`, which runs with the base repository's token;
    - an `actions/checkout` without `persist-credentials: false`.
  - **The table today:** `ci.yml` may use `actions/checkout` and
    `actions/cache`, and none of its jobs may write. `codeql.yml` may use
    `actions/checkout`, `github/codeql-action/init` and
    `github/codeql-action/analyze`, and its analysis job may write
    `security-events`. The release workflow (M14.4) adds its row.
- **Consequences:**
  - Good: the CI rules in `docs/04-quality.md` are checked on every push,
    as `deps` and `layers` check the dependency rules. Code scanning covers
    the Rust code and the workflows. Pins move forward by reviewed pull
    requests, a week after a release.
  - Bad: a line-based reading misses a `uses:` or a permission written in
    flow style or in a block scalar, so workflows are written one key a
    line. The `security-extended` queries raise more alerts to triage.
    CodeQL is one more action with a write permission, though only to code
    scanning. A new workflow must add its row to the table first.
- **Alternatives considered:**
  - zizmor or actionlint in CI: tools from outside the workspace,
    downloaded on every run. CodeQL's `actions` queries cover workflow
    injection and unpinned actions; the `xtask` check covers the project's
    own rules.
  - Parsing the workflows as YAML: the YAML subset in `tidepool-codec` is
    far off on the roadmap, and the check needs a few line shapes.
  - CodeQL's actions left pinned by tag: a moved tag would run new code with
    `security-events: write`, unreviewed.
- **Revisit when:** a workflow needs a flow-style `uses:` or a reusable
  workflow from another repository; the release workflow lands; CodeQL's
  alerts outgrow triage; or GitHub makes immutable actions the default,
  which would make a tag as safe as a SHA.

## ADR-0023 — Every workspace member has a README in one shape; `cargo xtask crates` checks the members' manifests and READMEs

- **Status:** Accepted; its README shape and its `## Status` and `## Design` rule are superseded by ADR-0028
- **Date:** 2026-11-02
- **Context:**
  - On 2026-11-01 the user chose per-crate READMEs: a `README.md` in each
    crate saying what it is, its status, and where its spec lives.
  - Nadir publishes its crates, so its READMEs are crates.io pages with
    tested examples (Nadir's ADR-0039). Tidepool's crates are the parts of
    one application and are not published; their readers are contributors
    and coding agents.
  - Each crate's `//!` doc says what it is and names its spec, but not what
    is built. On 2026-11-02, 22 of the 26 members were empty, and nothing in
    their folders said so.
  - `docs/04-quality.md` notes that members are expected to use edition
    2024 and `[lints] workspace = true`, and that no check verifies it. Only
    `xtask` sets `publish = false`; for the other members, only Claude
    Code's settings deny `cargo publish`.
- **Decision:**
  - **Every member has a `README.md`** in this shape:
    - `# <package name>`, then a paragraph: what it is, and how it fits in
      Tidepool;
    - its layer, and the workspace crates it may use, as the dependency
      matrix in `docs/01-architecture.md` gives them;
    - `## Status`: what is built, with the tasks that built it, and what is
      not yet, with the tasks that will build it;
    - `## Design`: its spec and the ADRs that shape it;
    - `## Rules`: whether it may hold `unsafe`, and the path-scoped rules in
      `.claude/rules/` that apply to it;
    - `## Checks`: the commands that test it;
    - `## License`.
  - **A task that changes what a crate has built updates its README** in
    the same change (`AGENTS.md` → "Definition of done").
  - **`cargo xtask crates`** (M0.9) runs in `cargo xtask ci`. It fails a
    member whose `[package]` does not inherit `edition`, `version` and
    `license` from the workspace, or lacks `publish = false` or
    `[lints] workspace = true`; or whose `README.md` is missing, does not
    start with `# <package name>`, lacks a `## Status` or `## Design`
    section, or names a task ID that `docs/roadmap.md` does not have.
  - No examples, doc tests or crates.io metadata: nothing is published.
- **Consequences:**
  - Good: whoever opens a crate's folder sees whether it is built and what
    to read. Manifests cannot drift from the workspace's edition and lints,
    and `cargo publish` fails on every member, inside Claude Code or not.
  - Bad: 26 READMEs to keep true as tasks land. The check verifies their
    shape and their task IDs, not whether what they call built is.
- **Alternatives considered:**
  - Nadir's crates.io shape with tested examples: a landing page for users
    of a published crate, which Tidepool's crates are not.
  - A status column in `AGENTS.md`'s workspace map: one more table to keep
    true, and not where someone browsing a crate looks.
  - The status in each crate's `//!` doc: a code change for every status
    change, and rustdoc is not where a contributor starts.
- **Revisit when:** a crate is published for use outside Tidepool (it then
  gets Nadir's crates.io shape), or the READMEs drift despite the check.

## ADR-0024 — The roadmap is re-planned from the state of the code on 2026-11-02; task IDs are stable

- **Status:** Accepted
- **Date:** 2026-11-02
- **Context:**
  - The roadmap, the docs and the Claude Code files were written on
    2026-10-31, before any code. By 2026-11-02, M0.1–M0.4, M1.1, M1.2 and
    M1.4 were done, `AGENTS.md` and `README.md` still said "design
    complete, no code yet", and the finished tasks had left open questions
    in their notes and in `docs/04-quality.md`.
  - On 2026-11-01 the user asked to rebuild the documentation, the Claude
    Code files and the roadmap, with Nadir's docs as the reference, and
    chose to refresh and re-plan rather than rewrite: keep the accepted
    ADRs and the notes on finished tasks, and split the unfinished
    milestones into finer tasks, as Nadir's roadmap has them. The splits
    and additions below are the plan the rebuild proposes; the user
    accepts it, or changes it, in review (M0.6).
  - Code cites task IDs: `xtask`'s list of commands not yet implemented
    (M1.6, M2.4, M3.2, M4.1, M5.1, M11.6, M12.4, M14.4), and comments in
    `ci.yml`, `text` and `codec`.
  - The plan itself had gaps:
    - M2.3 required golden frames, but the golden harness was M2.4, which
      depended on M2.3.
    - M5.2 depended on M2.2 for processes, but no task built
      `platform::process`.
    - `core::path` (`docs/subsystems/core.md`) was in no task's `Do` line.
    - The bench harness had no task, though M4.1 needs it, as `xtask` says.
    - Crash reports and buffer recovery (`docs/05-release.md`), the image
      and hex viewers (`docs/subsystems/editor.md`) and the `--smoke-test`
      mode were in no task.
    - Display width, which the terminal grid and the TUI need, was in no
      task, and needs Unicode data that ADR-0018 does not cover yet.
- **Decision:**
  - **Task IDs never change.** A new task takes the next free number in its
    milestone and is listed where it is to be done. A split task keeps its
    ID on the part that finishes what it promised, so every `Depends` line
    that names it stays true. The finished tasks and their notes are
    unchanged.
  - **Splits** (the new IDs come first, in work order):
    - M1.5: M1.7 the TOML parser, then M1.5 the preserving document model;
    - M2.2: M2.2 window, events, waker and present, then M2.7 clipboard and
      paths;
    - M2.3: its golden frames move to M2.4, with the harness;
    - M2.5: M2.8 the TrueType parser and rasterizer, with the font files,
      then M2.5 atlas, layout and fallback;
    - M3.3: M3.5 the painter and ui goldens, M3.6 the controls, then M3.3
      collections and overlays;
    - M4.2: M4.6 commands and the keymap engine, then M4.2 the editing
      commands;
    - M4.3: M4.7 folders, trust and settings, then M4.3 buffers, session
      and their panels;
    - M4.4: M4.8 the regex engine, then M4.4 search;
    - M5.4: M5.8 the VT parser and grid, M5.9 the PTY, then M5.4 the
      terminal panel;
    - M6.3: M6.7 sessions and diagnostics, M6.8 navigation and hover, then
      M6.3 the other features;
    - M10.2: M10.7 inflate, deflate and PNG, then M10.2 ZIP, XML and the
      inspector;
    - M11.1: M11.7 window and events, M11.8 the rest of `platform`, then
      M11.1 the macOS gate.
  - **Added tasks:** M0.6 the contribution workflow and crate READMEs; M0.7
    license headers (ADR-0021); M0.8 the CI supply chain (ADR-0022); M0.9
    `cargo xtask crates` (ADR-0023); M1.8 `core::path`; M1.9 the bench
    harness; M4.9 crash reports and recovery; M5.7 `platform::process`;
    M12.5 the repository's tooling on Windows; M13.4 the image and hex
    viewers; M14.7 a security review pass.
  - **`Depends` re-pointed to what a task needs:** M3.2 and M5.1 need only
    the TOML parser (M1.7); M5.2 needs M5.7, not M2.2; M6.2 needs the
    editor view (M4.1), not search; M6.4 needs M6.7; M7.4 needs navigation
    (M6.8); M13.2 needs the regex engine (M4.8); the editor panels of
    M8.2, M8.3 and M10.6 need the shell and buffers (M4.3); M14.4 needs the
    workflow check (M0.8); M1.9's baseline needs the reference machine
    (M0.5); and every task with a fuzz target depends on M1.6, directly or
    through another task.
  - **Open questions became `Decide first` lines** on the first task that
    needs the answer: the `Proposed` ADRs; how fuzz targets, allocation
    caps and children fit `xtask` (M1.6); ADR-0017's children for `bench`,
    `golden` and `package` (M1.9, M2.4, M11.6, M12.4); this ADR's three
    siblings, ADR-0021 to ADR-0023 (M0.7, M0.8, M0.9), and ADR-0015, the
    platform order, which no task asked about (M11.7); the Xvfb read-back
    (M2.2); the testkit edges (M5.1); Unicode data for regex classes and
    display width (M4.8, M5.8); the reference agents' workspace (M9.2);
    vendoring `toml-test` and the CommonMark examples (M1.7, M6.5); and a
    root `NOTICE` (M0.7).
  - **References that moved.** Where a reference named the part of a split
    task that now has a new ID, the docs that can change were updated:
    `docs/subsystems/vim-mode.md` (the keymap engine: M4.6, was M4.2) and
    `docs/subsystems/text-and-formats.md` (the CR|LF position in `lang`:
    M6.7, was M6.3). Accepted ADRs are not rewritten, so read their old
    references this way: ADR-0018's "`regex` (M4.4)" is now M4.8, and
    ADR-0019's "(M6.3, M6.4)" is now M6.7 and M6.4. Every other ID in code,
    docs and notes keeps its meaning.
  - The milestones' goals, the platform order (ADR-0015) and the GUI-first
    order (ADR-0003) do not change. Two milestones' exit criteria do: M0's
    add the new checks, and M1's add the text files, TOML's preserving
    writes and the bench baseline, which M1.3, M1.5 and M1.9 deliver.
- **Consequences:**
  - Good: tasks are small enough for a session each; a task asks the
    decisions it needs before its code; the IDs in code and notes keep
    their meaning, and the few doc references that moved are listed above.
  - Bad: 103 tasks instead of 77, and numbers within a milestone are no
    longer in order.
- **Alternatives considered:**
  - Renumbering every task in order: easier to read, but code, notes, ADRs
    and commit messages cite the old numbers.
  - Rewriting the roadmap from scratch: the user chose to keep the finished
    tasks and their notes.
- **Revisit when:** a milestone's tasks no longer fit a session each, or
  the plan needs another re-plan; record that as an ADR like this one.

## ADR-0025 — Saving follows symbolic links to their target; the temp file starts private and loses setuid and setgid through one `cfg(unix)` in `text::file`; what a rename changes is documented, and `buffers` checks it

- **Status:** Accepted
- **Date:** 2026-11-03
- **Context:**
  - M1.3 builds `text::file`'s atomic save: a temp file in the same
    directory, fsync, rename, permissions kept. `text` may use only `core`
    (dependency matrix), so it has `std` and nothing of `platform`.
  - A rename replaces whatever the path names. A path that is a symbolic
    link becomes a regular file and the link's target keeps its old
    content. The renamed file is a new file, so a hard link to the old one
    keeps the old content, and on Unix the owner and group become the
    saver's. `std` shows a link count or an owner only through
    `std::os::unix`.
  - `std` creates a file with mode 0666 minus the umask (usually 0644).
    Only `std::os::unix::fs::OpenOptionsExt::mode` sets the mode when the
    file is created. A temp file chmod'ed after it is created can be
    opened in that gap by another local user who can read the directory,
    and that descriptor reads whatever is written later: for a 0600 file,
    a leak of the text that rule 7 forbids.
  - `AGENTS.md` rule 5 keeps `#[cfg(target_os = ...)]` in `sys`,
    `platform`, `toolchain`'s install plans and `xtask`'s packaging.
  - The user answered three questions on 2026-11-03: save to the link's
    target (recommended); document what a rename changes and let
    `workspace::buffers` (M4.3) check it (recommended); one `cfg(unix)`
    for the temp file's mode (recommended).
  - M1.3's security review then showed that copying the target's whole
    mode keeps setuid and setgid on a file whose owner and group the
    rename changed: root saving a user's 4755 file made a setuid-root file
    of that user's bytes. A write in place would have cleared both bits,
    as the kernel does on an unprivileged write, and as `cp` does when it
    does not keep the owner. The review also found that a save changes the
    group even when the owner stays, and that on Linux the new file takes
    its directory's default POSIX ACL. On 2026-11-04 the user chose to
    clear the two bits in the same `cfg(unix)` (recommended), and to
    record that here before this ADR's first commit (recommended).
- **Decision:**
  - **Links are followed.** `file::save` follows the chain of symbolic
    links at the path, at most 40, to the file they name, and saves there:
    the temp file goes in the target's directory and is renamed over the
    target, so the links stay links. A link to nothing creates its target.
    A chain longer than 40 is an error.
  - **The temp file starts private.** When the target exists, `text::file`
    creates the temp file with mode 0600 on Unix, through `std`'s
    `OpenOptionsExt::mode` behind one `#[cfg(unix)]`, and gives it the
    target's permissions before the rename. A new file's temp file gets
    `std`'s default mode, which the new file would have anyway. Rule 5
    allows this one use, and `cfg(unix)` in `text`'s tests that check
    modes and links.
  - **Setuid and setgid are cleared.** The same `cfg(unix)` masks them out
    of the mode the temp file is given (`PermissionsExt`), so a save never
    carries them over to the saver's owner and group.
  - **A rename's other changes are documented, not detected.**
    `text::file`'s docs say that a save splits a hard link, gives the file
    the saver's owner and group on Unix (even when only the group differs)
    and the access list its directory passes on (a default POSIX ACL on
    Linux, the inherited ACL on Windows), and drops extended attributes.
    M4.3's `buffers`, which may use `platform`, checks the link count,
    owner and group before a save, and warns or writes in place.
- **Consequences:**
  - Dotfiles kept as links save as users expect. An untrusted workspace
    can hold a link to a file outside it: saving it writes there, as vim
    and VS Code do. The editor shows the path it opened, so what it
    writes is the file it showed.
  - No other process can open a temp file of an existing file before its
    mode is final.
  - A setuid or setgid file loses the bit when saved, as it would when
    written in place; it must be set again by hand.
  - Until M4.3, a save through `text::file` alone splits hard links and
    changes owners and groups without a word. Access lists are documented
    only: no task checks them.
  - Rule 5 has an exception outside the crates it names, in `AGENTS.md`
    and `CONTRIBUTING.md`.
- **Alternatives considered:**
  - Refusing to save through a link: safest, but surprising for dotfiles.
  - Renaming over the link: quietly turns it into a regular file.
  - `cfg(unix)` in `text` for the link count and owner, writing in place
    when a rename would change them: more OS code in `text`, which the
    user declined for now.
  - Always writing in place: keeps the file's identity, but a crash in
    the middle of a write corrupts the file.
  - Taking the temp-file creator from the caller, `platform`'s through
    `buffers`: no `cfg` in `text`, but the gap stays open until M4.3.
  - A chmod right after the file is created: the gap stays.
- **Revisit when:** `platform` grows file metadata that `text` could be
  given instead of the `cfg`; or users report a save that changed a file's
  owner, links or access list.

## ADR-0026 — `codec::toml` parses TOML 1.1; its compliance cases are toml-test v2.2.0's 1.1 list, vendored as test-only fixtures

- **Status:** Accepted; the question its Consequences leave to M1.5 is answered by ADR-0031: the writer writes TOML 1.0 syntax for the text it creates
- **Date:** 2026-11-04
- **Context:**
  - M1.7 builds `codec::toml`, the parser that settings, keymaps, themes,
    packs and tool descriptors (ADR-0013) and `Cargo.toml` go through. Its
    `Decide first` line asked whether to vendor the `toml-test` suite's
    cases, once their license was confirmed, or to write the compliance
    cases by hand.
  - `toml-lang/toml-test` is MIT-licensed ("Copyright (c) 2018 TOML
    authors", its `LICENSE`, read on 2026-11-04). Release v2.2.0 (commit
    `ce08da1ddb075d1c7596d663c7fcba9a2ae02c5c`, published 2026-04-30)
    lists each TOML version's cases: `tests/files-toml-1.1.0` names 895
    files, 106,344 bytes, with 214 valid documents (each with its expected
    value as tagged JSON) and 467 invalid ones.
  - TOML 1.1.0 was published on 2025-12-24. It adds newlines and trailing
    commas in inline tables, the `\xHH` and `\e` escapes, and optional
    seconds in times, and clarifies the rest. Cargo parses TOML 1.1 since
    cargo#16415 (merged 2025-12-29), so a `Cargo.toml` may use any of it,
    and a TOML 1.0 parser would reject that manifest.
  - On 2026-11-04 the user chose TOML 1.1 (recommended) and to vendor
    toml-test (recommended).
- **Decision:**
  - `codec::toml` parses TOML 1.1. Nothing that is valid TOML 1.0 is
    rejected, since 1.1 only relaxes 1.0. There is no strict 1.0 mode.
  - The files on toml-test v2.2.0's `files-toml-1.1.0` list, and the list
    itself, are copied unchanged into `crates/codec/tests/toml-test/`, with
    the MIT notice and their source beside them in a `NOTICE.md`, and an
    Assets row in `docs/02-dependency-policy.md`. They are test-only and
    never in a release binary. `.gitattributes` keeps their bytes as they
    are, since some cases are about CR, BOMs and invalid UTF-8.
  - The test reads each valid case's expected JSON with `codec::json` and
    compares it with the parsed value. toml-test's Go runner is not
    copied: its comparison rules are written again in the test.
- **Consequences:**
  - The parser is checked against the cases other TOML parsers use.
  - Every manifest Cargo accepts can be read.
  - About 900 small files in the repository. A newer toml-test release is
    taken by replacing the directory and its notice in one change.
  - M1.5's writer must decide whether it writes 1.1 syntax. A file that
    uses 1.1 syntax cannot be read by tools that only know TOML 1.0.
- **Alternatives considered:**
  - TOML 1.0, as the roadmap first said: rejects manifests Cargo accepts.
  - 1.1 with a strict 1.0 mode: more code and tests for a mode nothing
    needs yet.
  - Hand-written cases: no third-party data, but no outside check of our
    reading of the spec.
  - Only the cases taken from the spec's examples (about 100 files): far
    less coverage of edge cases.
- **Revisit when:** a TOML release after 1.1, or a toml-test release that
  changes the 1.1 list.

## ADR-0027 — Fuzz targets live in each parser crate's `tests/fuzz.rs`, driven by an engine in `core::testing`; `fuzz-smoke` runs them through `cargo test`; over-allocation is caught by `Limits` and the peak memory on Linux

- **Status:** Accepted
- **Date:** 2026-11-07
- **Context:**
  - M1.6 builds `cargo xtask fuzz-smoke` (`docs/04-quality.md` → "Fuzz
    smoke"), which that spec called provisional on three points that
    M1.6's `Decide first` line asked.
  - Its first targets are JSON and TOML in `codec`, and the decoder of
    `text::encoding`, which M1.3's note named. `text` is outside `xtask`'s
    matrix row (`core`, `codec`), and later targets are in `font`,
    `markdown`, `regex`, `term`, `rpc`, `vcs` and `toolchain`.
  - The Claude Code hook builds `xtask` before every manifest check
    (ADR-0017), so whatever `xtask` links compiles first.
  - Counting allocations needs a `GlobalAlloc`, which is `unsafe`, and
    `unsafe` lives only in `sys` and `platform`. Neither has code yet.
  - Nadir's fuzzer, in its `xtask`, catches panics and hangs, has no
    allocation cap, and reaches only `core` and `codec`, where all of
    Nadir's parsers are.
  - On 2026-11-07 the user chose: targets in each crate's tests
    (recommended); `Limits` plus the peak memory (recommended); and
    `cargo test` as `fuzz-smoke`'s only child (recommended).
  - M1.6's review then found two things. Reading `/proc/self/status`
    after every input, as first decided, cut throughput 2.7 to 7 times
    (`json` from 332,000 to 122,000 inputs a second, `toml` from 1,290,000
    to 173,000). And `VmHWM` counts only memory that was written:
    `Vec::with_capacity(1 GiB)`, `vec![0; 256 MiB]` and a 16 GiB
    reservation from a claimed length all went unseen, and Linux's
    overcommit let them succeed, though that is the bug
    `.claude/rules/parsers.md` means by "check before allocating". On
    2026-11-07 the user chose to read `VmHWM` on a time budget
    (recommended) and to check `VmPeak` as well (recommended), and this
    ADR was amended before its first commit to record both.
- **Decision:**
  - **Targets.** Each crate with a parser has a dev-only integration test,
    `tests/fuzz.rs`, with one test per target, named after it. The engine
    (mutations, catching panics, a watchdog for hangs, the memory check,
    saving crashes) is `tidepool_core::testing::fuzz`, beside the property
    harness, which every crate may use. No matrix row changes.
  - **Every `cargo test` replays.** A target's test runs its seeds
    (`fuzz/corpus/<target>/`, and data already in the repository that it
    names, read in place) and its saved crashes (`fuzz/crashes/<target>/`)
    unchanged, so a saved crash is a regression test. It mutates only when
    `fuzz-smoke` asks it to, through environment variables.
  - **`fuzz-smoke`'s only child is `cargo test`:** for each target,
    `cargo test` of that crate's `fuzz` test, filtered to the target, in an
    optimized profile with overflow checks and debug assertions on. The
    parsers run in that test binary. This extends ADR-0017's list of
    children by one cargo command, with the same rules: an argv vector, the
    root as its directory, standard input closed, `CARGO_NET_OFFLINE=true`.
    Nothing but cargo runs.
  - **Over-allocation.** Each parser runs with small `Limits`
    (`text::encoding`'s decoder takes none). On Linux, the engine reads
    `/proc/self/status` for two checks:
    - **Written memory** (`VmHWM`, the peak resident set): a rise past the
      target's cap is a finding. The file is read once enough of the
      target's time has passed since the last read for the peak to have
      crossed the cap at 1 MiB a microsecond, about 300 times the rate
      measured. Under that bound the input reported is still the one that
      took the peak past the cap, at a fraction of the cost of reading
      after every input.
    - **Reserved memory** (`VmPeak`, the peak address space, which counts
      memory reserved and never written), from the same read: a peak more
      than 256 MiB above the address space in use (`VmSize`) once the
      target's thread has made its first allocation, before any input, is
      a finding. (A baseline after the first input would hide a saved
      crash that runs first.) Thread arenas and stacks move it by tens of
      MiB, so it is checked only when `TIDEPOOL_FUZZ_SECS` is set, even to
      0: in the runs `fuzz-smoke` starts, and in the reproduce command of
      a saved reserved-memory crash, each one target to a process. Address space has no rate bound, so a run that reads
      on the budget names the inputs since the last read; `fuzz-smoke`
      then prints a replay, which reads after every input, and names and
      saves the one.

    Elsewhere both checks are skipped, and say so; CI's fuzz step runs on
    Linux. A run whose process dies (an allocation too big to reserve, a
    stack overflow) is reported with its seed. Since the inputs follow
    from the seed alone, a replay that saves each input before running it
    leaves the culprit on disk.
- **Consequences:**
  - A new parser adds a test to its own crate's `tests/fuzz.rs`, a seed
    directory, and a row in `fuzz-smoke`'s table. `.claude/rules/parsers.md`
    says so.
  - `xtask` stays at `core` and `codec`, and the hook stays fast.
  - Saved crashes cost nothing extra to keep as regression tests.
  - The memory checks are coarse: they see the whole process, they run
    only on Linux, the written-memory bound is an assumption measured on
    one machine, and an allocation that kills the process is found only by
    replaying it. A plain `cargo test` does not check reserved memory, so
    a saved reserved-memory crash is a regression test only in runs with
    `TIDEPOOL_FUZZ_SECS` set: `fuzz-smoke`'s, and its own reproduce
    command.
  - `fuzz-smoke` builds each parser crate's tests again in its own profile.
- **Alternatives considered:**
  - A dev-only fuzz member that may use any crate, run by `cargo run`: one
    place for every target, and room for an allocation counter from
    `platform`, at the cost of a new crate and its matrix row.
  - Widening `xtask`'s row to every crate with a parser: the hook would
    compile them all before each check.
  - A counting allocator in `platform`: precise, but only a binary that may
    use `platform` can install it, and it would be `platform`'s first
    `unsafe` code, ahead of M2.1.
  - `Limits` alone: the quality spec's "over-allocates" would go unchecked
    by `fuzz-smoke`.
- **Revisit when:** a parser's blow-up passes the coarse check and is found
  otherwise; or `platform` has a counting allocator for other reasons.

## ADR-0028 — Crate READMEs take Nadir's shape, keeping Tidepool's Rules and Checks, with examples that compile as doc tests; every member keeps copies of `LICENSE` and `NOTICE`; `cargo xtask crates` checks both

- **Status:** Accepted
- **Date:** 2026-11-08
- **Context:**
  - On 2026-11-08 the user asked for the crate READMEs and their licensing
    to follow Nadir (<https://github.com/jabbott-iii/Nadir>). Of the
    options put to them, they chose Nadir's README shape with Tidepool's
    `## Rules` and `## Checks` kept, and copies of `LICENSE` and `NOTICE`
    in every crate.
  - Nadir's crate READMEs are its crates.io pages (its ADR-0039): what the
    crate is and how it fits, its status, its usage with examples that are
    doc tests (a `#[cfg(doctest)]` item whose doc is the README), its
    features where it has some, links to its documentation, and "Apache
    License 2.0: see LICENSE and NOTICE". Each crate keeps byte-identical
    copies of the root's `LICENSE` and `NOTICE` (its ADR-0037), and its
    `cargo xtask crates` fails a missing or differing copy.
  - ADR-0023, accepted the same day in M0.6's review, gave the READMEs
    another shape (Status, Design, Rules, Checks, License) and chose
    against Nadir's, because Tidepool's crates are not published. M0.9 made
    every member `publish = false`, and its `crates` check requires
    `## Status` and `## Design`.
  - Nothing of Tidepool is published, so a crates.io dependency line or a
    docs.rs link would name pages that do not exist. Three libraries have
    APIs to show (`tidepool-core`, `tidepool-codec`, `tidepool-text`), and
    `xtask` has commands; `tidepool` builds a binary that does nothing.
- **Decision:**
  - **Every member's `README.md` has, in this order:**
    - `# <package name>`; what the crate is; how it fits in Tidepool, with
      a link to the repository; and its layer and the workspace crates it
      may use, as the dependency matrix gives them;
    - `## Status`: "Version 0.1, not published", then what is built, with
      the tasks that built it, and what is not yet, with the tasks that
      will build it (as ADR-0023 had it);
    - `## Usage`: for a library with a built API, the path dependency a
      workspace crate writes (only where its matrix row allows it), then
      examples that compile as doc tests; for a binary, its commands; for
      a crate with nothing to use yet, the task that brings its first API;
    - `## Features`, only when the crate has Cargo features (none has
      today);
    - `## Documentation`: its spec, the ADRs that shape it, and
      `cargo doc -p <package> --open` for its API reference. It takes the
      place of ADR-0023's `## Design`;
    - `## Rules` and `## Checks`, as ADR-0023 had them;
    - `## License`: "Apache License 2.0: see [LICENSE](LICENSE) and
      [NOTICE](NOTICE).", then any third-party material the crate holds
      and where its notice is.
  - **Examples are doc tests.** Each library member's `src/lib.rs` has
    Nadir's `#[cfg(doctest)] #[doc = include_str!("../README.md")] struct
    ReadmeDoctests;`, so `cargo test` compiles and runs the README's Rust
    blocks. A binary's README holds no Rust block, which nothing would test.
  - **Every member keeps copies of the root's `LICENSE` and `NOTICE`** in
    its folder, byte for byte.
  - **`cargo xtask crates`** keeps ADR-0023's manifest, first-line and
    task-ID rules. It fails a README without a `## Status`, `## Usage`,
    `## Documentation`, `## Rules`, `## Checks` or `## License` line, with
    one of them twice, or with them out of this order (a `## Features` may
    stand between Usage and Documentation); and a member whose `LICENSE` or
    `NOTICE` is missing, a link, not a regular file, or not the same bytes
    as the root's.
  - This supersedes ADR-0023's README shape and its `## Status` and
    `## Design` rule. The rest of ADR-0023 stands.
- **Consequences:**
  - Good: a crate's README shows how to use what it has built, and an
    example that stops compiling fails `cargo test`. Each crate's folder
    carries its license, as Nadir's do, and still does if a crate is ever
    packaged or published.
  - Bad: 52 more files; a change to `LICENSE` or `NOTICE` is 27 changes,
    which `crates` finds but does not make. The READMEs partly repeat the
    crate docs, and 22 of them have a `## Usage` that only says what comes.
- **Alternatives considered:**
  - Nadir's shape exactly, without Rules and Checks: the user kept them;
    they say which `.claude/rules/` apply and what to run.
  - ADR-0023's shape, with only the license copies added: not the style
    the user asked for.
  - Links to the root's `LICENSE` and `NOTICE` instead of copies: nothing
    to keep in step, but not Nadir's style, and a packaged crate would
    carry neither.
- **Revisit when:** a crate is published (it then gets Nadir's crates.io
  lines: a registry dependency and a docs.rs link), or the license changes.

## ADR-0029 — `diagrams/` holds the C4, UML, data-flow and state diagrams, in Mermaid; what the code has is drawn solid, what the roadmap plans dashed, with its task

- **Status:** Accepted
- **Date:** 2026-11-08
- **Context:**
  - On 2026-11-08 the user asked for a `diagrams/` folder at the root with
    C4 diagrams (the container and component levels), UML class, component
    and sequence diagrams, and data-flow and state diagrams, in Nadir's
    style. Nadir's `diagrams/` is Markdown with Mermaid blocks, which
    GitHub draws; each file names the commit it is drawn from, and what is
    planned is dashed and labelled.
  - 22 of the 26 members are empty, and most of Tidepool is the design in
    `docs/`. Asked what to draw, the user chose what is built and what is
    planned, the planned parts marked with their task.
  - Tidepool takes no third-party code (ADR-0001), and no tool to render
    diagrams is a managed tool.
- **Decision:**
  - **`diagrams/`** at the root holds `README.md` (what each file shows,
    how to read and keep them), `c4-architecture.md` (containers and
    components), `class-diagrams.md`, `component-diagrams.md`,
    `sequence-diagrams.md` and `data-flow-and-state.md`. Each names the
    roadmap task it was drawn as of, whose ID never changes (ADR-0024),
    and lists its sources.
  - **Mermaid in Markdown only**, no generated images. The C4 views are
    Mermaid flowcharts in C4's colours, as Nadir draws them, since
    Mermaid's own C4 diagrams are experimental.
  - **Built is solid; planned is dashed** and labelled with the roadmap
    task that builds it. A class diagram shows only types the code has,
    under their real names; planned types appear in the other diagrams,
    under the names their specs give.
  - **The code wins.** When a diagram and the code disagree, the code is
    right. A change to what a diagram draws (a crate's dependencies, a
    type drawn, a flow) updates the diagram in the same change, and a task
    that builds what a diagram draws as planned makes it solid.
- **Consequences:**
  - Good: one place to see how the parts fit, before most of them exist.
  - Bad: drawings of planned work go stale as specs change, and no check
    reads them; Mermaid has no true UML component notation, so component
    diagrams are flowcharts with ports drawn as nodes.
- **Alternatives considered:**
  - Images rendered by a tool (mermaid-cli, PlantUML): an outside program,
    and pictures that cannot be reviewed as text.
  - Only what is built: accurate, but most views would be empty.
  - Mermaid's `C4Container` and `C4Component` diagrams: experimental, and
    GitHub draws their layout poorly.
- **Revisit when:** a check can compare a diagram with the code (the crate
  graph with the dependency matrix, say), or GitHub stops drawing Mermaid.

## ADR-0030 — The demo page at jabbott.dev/Tidepool/ shows the commit's gate and two library examples; a features file covers the roadmap; `pages.yml` publishes it after CI passes on `main`

- **Status:** Accepted
- **Date:** 2026-11-08
- **Context:**
  - On 2026-11-08 the user asked for a deployed demo page in the style of
    <https://jabbott.dev/Nadir/>, which Nadir's ADR-0029 builds from the
    Nadir repository, with <https://jabbott.dev/> as the primary site and
    the source of its style, and for it to update by itself on push.
  - <https://jabbott.dev/Tidepool/> already serves the root `README.md`,
    rendered by GitHub Pages' Jekyll build: `gh api
    repos/jabbott-iii/Tidepool/pages` gave `build_type` `legacy` and the
    source `main`, `/`, on 2026-11-08. jabbott.dev's Deployments menu finds
    Pages sites from GitHub by itself, so it links that address already.
  - Tidepool is headless. Nothing draws before M2.6 (the GUI's first
    frame) or M7.1 (the TUI), so there is nothing to record as Nadir
    records its sandbox. What runs today is the gate (`cargo xtask ci`) and
    the libraries: JSON, JSONC and TOML 1.1 in `tidepool-codec`; the rope,
    positions, graphemes, selections, change sets, undo tree, encodings and
    atomic save in `tidepool-text`.
  - ADR-0017 lets `xtask` start only `cargo`, and lists its children
    (ADR-0027 added `fuzz-smoke`'s). Rule 2 leaves every other program,
    `git` among them, to `tidepool-toolchain`, which `xtask`'s matrix row
    (`core`, `codec`) does not allow. `xtask` makes no network calls
    (`docs/04-quality.md`).
  - ADR-0021 puts the license header on Rust, shell and workflow files: a
    new kind of source file joins the check when the first one appears.
  - ADR-0022's table has rows for `ci.yml` and `codeql.yml` only, and only
    CodeQL's analysis job may write. A Pages deployment from a workflow
    needs GitHub's `actions/upload-pages-artifact` and
    `actions/deploy-pages`, and a deploy job with at least `pages: write`
    and `id-token: write` (GitHub's docs, "Using custom workflows with
    GitHub Pages", read on 2026-11-08).
  - Nadir's ADR-0029 and its code are the model: `xtask/src/site.rs`,
    `site/page.rs` and `features.rs`, `docs/features.jsonc`, `site/` and
    `.github/workflows/pages.yml`. It is the owner's code, which Tidepool
    ports with a note (`docs/02-dependency-policy.md`).
  - The user made the four choices below on 2026-11-08, each the
    recommended option, in the session that asked for the page: the demo
    panel, the features, publishing, and the commit's facts. They reached
    this record through that session's design brief, so it stayed
    `Proposed` until the user accepted it as written, on 2026-11-08.
- **Decision:**
  - **The commit's real output, not a recording or a web build.** The
    page's main panel has three tabs, each made on the commit it names:
    - **The gate:** the steps of `cargo xtask ci` as `site` ran them, each
      with its verdict and time, and the tests passed, failed and ignored,
      summed from libtest's `test result:` lines, with the `SKIPPED:` count
      beside them and never among the passes (`AGENTS.md` rule 9). The page
      says where it ran: on the Pages job's Linux runner, after CI passed
      on Linux, macOS and Windows.
    - **`tidepool-codec`:** what its example `formats`
      (`crates/codec/examples/formats.rs`) prints.
    - **`tidepool-text`:** what its example `editing`
      (`crates/text/examples/editing.rs`) prints.

    Each example runs on fixed inputs written in its source, and prints
    only what follows from them: nothing of the clock, the machine or the
    environment, such as an absolute path. So their tabs say the same
    wherever the page is built.
  - **`cargo xtask site [--out DIR]`** builds the page into `target/site`,
    or into `DIR`, which must be new, empty, or marked by an earlier run
    with the file `.tidepool-site`. A run removes only what an earlier one
    wrote (Nadir's `MARK` and `WRITTEN`). It:
    1. checks `docs/features.jsonc` as `cargo xtask features` does;
    2. runs the gate in-process, the same steps `cargo xtask ci` runs,
       recording each step's verdict and time and the counts above;
    3. runs each example with `cargo run --quiet -p <package> --example
       <name>`, reading its standard output, and fails on an exit status
       other than 0 or output that is not UTF-8;
    4. fills `site/index.html` and `site/llms.txt`, whose placeholders are
       names in double braces that the comment at the top of
       `site/index.html` defines, and copies `site/tidepool.css` and
       `site/tidepool.js` beside them.

    It fails, writing nothing, when the output directory or one of the
    variables below is not valid, or the features, a gate step or an
    example fail. It starts no program but `cargo`, makes no network
    calls, and reads and writes only the workspace and its output
    directory. What it fills into the page is escaped for HTML, and what it
    fills into `llms.txt` has its control characters escaped as `xtask`
    escapes its messages.
  - **The commit's facts** (`xtask` still starts only `cargo`):
    `pages.yml` reads the commit's hash, date and subject with `git log -1`
    and passes them to `cargo xtask site` in three variables:
    `TIDEPOOL_SITE_COMMIT` (40 lowercase hex digits),
    `TIDEPOOL_SITE_DATE` (the committer date, `YYYY-MM-DD`) and
    `TIDEPOOL_SITE_SUBJECT` (one line, at most 200 characters once
    trimmed, with no control characters, line or paragraph separators or
    bidirectional controls).
    - All three or none. `site` checks each, and fails naming the variable
      that is wrong, never echoing its value unescaped.
    - With none, a local build, `site` reads HEAD's hash from `.git`
      itself: `HEAD`, then the loose ref or `packed-refs`. It never runs
      `git`. The page then shows no date or subject, and says it is a
      local build.
    - Where HEAD cannot be read that way, as in a linked worktree, whose
      `.git` is a file, the page says the commit is unknown and `site`
      prints why, rather than failing, since a local build is never
      published.
  - **`docs/features.jsonc`** lists the features in Nadir's format,
    `{"features": [{"name", "area", "summary", "tasks", "ticked"}]}`, with
    the comment that explains it at the top. Its areas are Quality, Core,
    Text, Formats, Platform, Rendering, UI, Editor, Languages, Toolchains,
    Terminal, Git, AI, Mobile and Release. The page marks each feature
    shipped, in progress or planned from the roadmap's ticks.
    - Each feature lists, as `ticked`, the tasks that were ticked when its
      summary was written.
    - **`cargo xtask features`**, a step of `ci` right after `crates`, is
      Nadir's check ported. It fails when a ticked task belongs to no
      feature, a feature names a task the roadmap lacks, or a feature's
      `ticked` is not what the roadmap ticks: ticking a task means bringing
      its feature's summary up to date.
    - It reads the roadmap's tasks with `crates`' reader
      (`xtask/src/crates.rs`), not a second one.
  - **The page's files** are `site/index.html` (the template),
    `site/tidepool.css`, `site/tidepool.js`, and `site/llms.txt`, a
    template of the same in plain text for language models, served as it
    is and so without a header. The page loads jabbott.dev's live
    stylesheet (`https://jabbott.dev/assets/css/style.css`) and script
    (`https://jabbott.dev/assets/js/site.js`) and Google Fonts, as Nadir's
    does, and its header and footer repeat that site's. `tidepool.css` adds
    the page's own components with that site's tokens. ADR-0004's bundled
    fonts are the IDE's; the page is a page of jabbott.dev, in its fonts.
  - **`.github/workflows/pages.yml`** has Nadir's three jobs, under
    Tidepool's CI rules:
    - It runs after CI succeeds on a push to `jabbott-iii/Tidepool`'s
      `main` (`workflow_run`), or by hand from `main`, and publishes only
      `main`'s newest commit.
    - A gate job, which waits in no queue, skips a commit `main` has moved
      past. A run by hand publishes only a commit whose push to `main` CI
      passed; its gate has `actions: read` to find that run.
    - The build and the deploy each wait in a queue of their own, so a run
      that will not publish never displaces one that will. They run only
      in the attempt whose gate let them through, and each attempt uploads
      an artifact of its own, as in Nadir's.
    - The build job, on `ubuntu-latest` with `contents: read`, checks
      `main` again, checks out the commit with `persist-credentials:
      false`, and runs `cargo xtask site` with `CARGO_NET_OFFLINE: "true"`
      and the three variables set inline for that command only, never
      through `$GITHUB_ENV`. It uploads `target/site`.
    - Every `run:` step that reads the checkout or runs its code pauses
      workflow commands with a fresh token (M0.11).
    - Only the deploy job writes, to `pages` and `id-token`. It checks
      nothing out and runs none of the repository's code.
    - Top-level permissions are `{}`. Each action is pinned by its commit
      SHA, with its tag in a comment.
    - It replaces the README that Jekyll serves at `/Tidepool/`. The user
      switches Settings → Pages → Source to "GitHub Actions".
  - **Trust boundaries.**
    - Whatever the page shows, it shows to everyone. The gate's tab gives
      verdicts, times and counts, never a line that cargo or a test
      printed, so no path, name or value printed during the gate is
      published; the examples print only what their fixed inputs give.
      The subject is the one text from outside the code that reaches the
      page: checked, then escaped like everything filled in.
    - The build runs `main`'s code, which CI has just run, with a token
      that can only read the repository and is not left in `.git/config`.
      The cache it restores is `main`'s: a run on the default branch
      restores no pull request's cache (GitHub's docs, "Dependency
      caching", read on 2026-11-08). No run from a fork or a pull request
      reaches the build, since the gate takes only a successful CI run of a
      push to this repository's `main`.
    - The deploy job's write tokens never meet the repository's code.
    - Visitors' browsers fetch jabbott.dev's stylesheet and script and
      Google's fonts, with no integrity hashes, since the page follows the
      live files.
  - **This extends three ADRs:**
    - ADR-0017: `site`'s children are the gate's (`cargo metadata`, `cargo
      locate-project`, and the fmt, clippy and test steps), and `cargo run
      --quiet -p tidepool-codec --example formats` and `cargo run --quiet
      -p tidepool-text --example editing`. Each is an argv vector, run in
      the root, offline, with standard input closed. Nothing but cargo
      runs.
    - ADR-0021: `headers` also checks `.html` files (an HTML comment, after
      a `<!doctype html>` line if there is one), `.css` files (a `/* … */`
      comment) and `.js` files (`//` lines), as Nadir's check does.
      `site/llms.txt` and `docs/features.jsonc` are data, with no header.
    - ADR-0022: `pages.yml` gets a row in `WORKFLOWS`: it may use
      `actions/checkout`, `actions/cache`, `actions/upload-pages-artifact`
      and `actions/deploy-pages`, and its deploy job may write `pages` and
      `id-token`.
  - Roadmap M0.14 is the task.
- **Consequences:**
  - Good:
    - The page shows what the commit does, made on that commit: the gate's
      verdicts and counts, and the libraries' own output. Only the
      features' summaries are written by hand, and the check keeps them in
      step with the ticks.
    - Every ticked task describes what it ships, for the page and for
      anyone reading `docs/features.jsonc`.
    - No crate is added, `xtask` still starts only `cargo`, and nothing in
      the libraries is there for the web.
    - The address stays the same, so jabbott.dev's Deployments menu keeps
      linking it.
  - Bad:
    - A publishing job holds write tokens, through two more actions.
    - The build job runs the whole gate again, after CI has, so a push to
      `main` costs a second Linux run of it before the page changes.
    - An example that fails when run, rather than when built, fails only
      the Pages build: `cargo test` builds the examples but does not run
      them.
    - The page depends on jabbott.dev's markup and class names, and on
      Google Fonts when it is viewed.
    - `/Tidepool/` no longer shows the README; GitHub's repository page
      still does.
    - When the newest commit fails CI, the page waits for the next one
      that passes.
    - A page built locally names its commit but not its date or subject.
    - `docs/features.jsonc` is one more place that holds status, which
      `features` checks against the ticks but not against the code.
- **Alternatives considered:**
  - A WASM build that runs in the browser: it needs a web platform layer,
    a canvas and the browser's event loop, and there is nothing to draw
    yet.
  - Recording the TUI or the GUI, as Nadir records its sandbox: neither
    exists before M7.1 and M2.6.
  - Only the gate's run: it shows the checks, not what the libraries do.
  - No demo panel, only the features: the page would show nothing the
    commit did.
  - Features made from the roadmap's milestones or task titles: they read
    as a plan, not as what is built.
  - Letting `xtask` run `git` for the commit's facts: rejected. Rule 2
    stays as ADR-0017 left it; `git` is a managed tool, and `toolchain`
    is outside `xtask`'s matrix row.
  - Publishing on every push: a push that fails CI, or to another branch,
    would replace the page, and the page could go back to an older commit.
  - Pushing the page to a `gh-pages` branch with `contents: write`: that
    token could write any branch, where the Pages deployment's can write
    only Pages.
  - A copy of jabbott.dev's stylesheet in the repository: it would not
    follow that site's changes.
  - Keeping the README at `/Tidepool/` and the demo at a subpath: Pages
    serves one source a repository, so the workflow would have to render
    the Markdown too.
  - Downloading CI's results as an artifact instead of running the gate
    again in the Pages job: one more action, and a page built from another
    run's files, which a later job then trusts; the second run costs only
    time.
- **Revisit when:** the TUI (M7.1) or the GUI (M2.6) can be recorded; a
  WASM or web build becomes a goal; jabbott.dev changes its markup or
  class names; the Pages job takes more than 15 minutes; or DEFLATE in
  `codec` (M10.7) lets `xtask` read the commit's objects itself.

## ADR-0031 — `codec::toml`'s document writer writes TOML 1.0 syntax for the text it creates, and keeps the text it does not change byte for byte

- **Status:** Accepted
- **Date:** 2026-11-08
- **Context:**
  - M1.5 builds the comment- and order-preserving document model that
    ADR-0013 calls for, on M1.7's parser, which reads TOML 1.1 (ADR-0026).
    Settings are its first user: M4.7 writes them back through it, and
    saving them keeps unknown keys and comments.
  - ADR-0026's Consequences left M1.5 one question: whether the writer
    may write TOML 1.1 syntax, which a reader that knows only TOML 1.0
    rejects. TOML 1.1 adds four such forms: line breaks, comments and a
    trailing comma in an inline table; the `\xHH` and `\e` escapes; and
    times without seconds. Each value they spell has a 1.0 spelling too:
    an inline table on one line, `é` and `\u001B`, and `:00` for the
    seconds.
  - The parser does not record which syntax a value was written in
    (`docs/subsystems/text-and-formats.md` → `toml`: not whether seconds
    were written, for one).
  - On 2026-11-08 the user chose "1.0 for new text" (the recommended
    option) over "1.1 where shorter" and "follow the document".
- **Decision:**
  - The document model reads what the parser reads: all of TOML 1.1.
  - An unchanged document writes back byte for byte. An edit rewrites only
    the bytes of the value it changes or the entry or table it adds;
    comments, blank lines, order, quoting, layout and line endings
    elsewhere stay as they were.
  - Every piece of text the writer creates is TOML 1.0:
    - a string escapes a control character with one of 1.0's short
      escapes (`\b`, `\t`, `\n`, `\f`, `\r`) or `\uXXXX`, never `\e` or
      `\xHH`;
    - a time, local date-time or offset date-time is written with its
      seconds;
    - an inline table is written on one line, with no comment, line
      break or trailing comma.
  - So a document that is TOML 1.0 stays TOML 1.0 after any edit, and 1.1
    syntax is in a file only where a person wrote it. An edit inside text
    that already uses 1.1 syntax, such as an inline table laid over
    several lines, keeps that text's layout: it adds no 1.1 syntax to a
    1.0 document, and it does not take any out of a 1.1 one.
  - The tests check each of the four forms in what the writer creates,
    and that every valid case of toml-test's 1.1 list (ADR-0026) writes
    back byte for byte when nothing is changed.
- **Consequences:**
  - A settings file Tidepool edits stays readable by tools that know only
    TOML 1.0, and what a person wrote is never reformatted.
  - A few values take more text: `\u001B` (6 bytes) for `\e` (2), and
    `07:30:00` for `07:30`. A time the writer rewrites gains `:00` even
    where a person left the seconds out, since the parser does not record
    that; only a value an edit changes is rewritten.
  - An inline table too long for one line is the caller's to write as a
    table under a header instead.
- **Alternatives considered:**
  - 1.1 where shorter (`\e`, `\xHH`, no zero seconds, inline tables over
    several lines): a file could stop parsing in a 1.0 tool after an edit
    whose syntax the person never saw.
  - Follow the document, writing 1.1 only into a file that already uses
    it: needs a detector of 1.1 syntax, which the parser does not record,
    and tests of its own, for a few bytes.
  - A strict 1.0 writer that also rewrites a person's 1.1 syntax into 1.0:
    breaks the byte-for-byte promise for text the edit did not touch.
- **Revisit when:** a TOML release after 1.1 (ADR-0026's trigger), or a
  report that a TOML file Tidepool wrote back was rejected by a TOML tool.
