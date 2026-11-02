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

- **Status:** Accepted
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
