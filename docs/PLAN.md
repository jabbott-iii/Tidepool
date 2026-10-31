# Tidepool Development Plan

**Status:** Draft v0.1 (2026-10-31). Proposed, pending owner review.
**Owner:** Joseph Abbott III
**Related:** [`DEPENDENCIES.md`](DEPENDENCIES.md) (dependency policy and ledger), [`adr/`](adr/) (decision records), [`../CLAUDE.md`](../CLAUDE.md) (Claude Code working rules)

This plan describes what Tidepool is, how it is put together, and the order it is built in.
Milestone checklists (`- [ ]`) are the work queue used by the `/next-task` skill. Anything marked
**Decision** in §10 needs owner input before the milestone that depends on it starts.

---

## 1. Goals

Tidepool is an IDE for software developers, written in Rust, with no third-party dependencies
beyond what is strictly necessary to run on an operating system.

1. **Zero-dependency core.** Rust `std` plus hand-written OS bindings. No crates.io packages. See §2.
2. **Standard IDE features** for common languages and file formats (§5).
3. **Rust and Markdown first.** These two are first-class before anything else is broadened.
4. **Mobile packaging.** Build and sign Android `.apk` and `.aab`, and the iOS equivalents
   (`.app` / `.xcarchive` / `.ipa`) (§5.5, §5.6).
5. **AI integration and predictive text** through pluggable *language agents* (§5.7).

### Glossary

| Term | Meaning |
| --- | --- |
| **Language pack** | Data (not code) that teaches Tidepool a language: file associations, lexer rules, comments, brackets, indentation, and which LSP server, debug adapter, formatter, and agents to use. |
| **Language agent** | An external process that plugs into Tidepool over the Tidepool Agent Protocol (TAP) and provides AI features (inline completion, prediction, chat, edit proposals) for one or more languages. |
| **Frontend** | A presentation layer (terminal UI or native GUI) over the shared, frontend-agnostic core. |
| **Tier 1 / Tier 2 / Tier 3** | Dependency tiers defined in `DEPENDENCIES.md`: OS interfaces / external programs / third-party crates. |

## 2. Non-negotiable constraints

- **Dependencies:** Only Tier 0 (Rust toolchain and `std`), Tier 1 (OS interfaces via hand-written
  FFI, confined to `tp-sys`), and Tier 2 (external programs discovered at runtime and run as
  subprocesses, never linked or bundled). Tier 3 (third-party crates, including dev-, build-, and
  proc-macro dependencies) is forbidden unless an ADR is approved by the owner. Enforced by
  `scripts/check-deps.sh`, a Claude Code hook, and a deny rule on `cargo add`.
- **`unsafe` is confined** to `tp-sys`. All other crates use `#![forbid(unsafe_code)]`.
- **No in-house cryptography for signing in v1.** Signing is delegated to the platform's official
  tools (`apksigner`, `jarsigner`, `codesign`). See §8.
- **The core binary opens no network sockets in v1.** Network access, where needed, happens in
  external processes (language agents, `git`, `ssh`, build tools) the user controls.
- **Opening a folder never executes its code** without workspace trust (§8).
- **Compatibility:** once a file format, protocol (TAP), or settings key ships, changes must be
  backward-compatible or versioned.

## 3. Scope

### In scope for v1

- Editing: multi-cursor, undo/redo tree, search/replace (regex), code folding, bracket matching,
  auto-indent, large-file mode, encodings (UTF-8, UTF-16 with BOM), line-ending preservation.
- Workspace: file tree, fuzzy file finder, project-wide search, file watching, tabs/splits,
  command palette, keymaps, themes, settings, session restore.
- Languages: Rust and Markdown first-class (M3); language packs for common languages (M9).
- Semantic features via LSP; debugging via DAP (M9).
- Tasks: build/run/test with problem matching; integrated terminal; git via the `git` CLI.
- File formats: JSON, TOML, YAML, XML, plist, CSV, INI (view + validate); hex view for binaries;
  PNG view (GUI); archive inspection (ZIP, and therefore APK/AAB/IPA contents).
- Mobile: Android APK/AAB build + sign; iOS `.app`/`.xcarchive`/`.ipa` build + sign on macOS.
- AI: TAP, agent host, inline completion (ghost text), local predictive text, chat, reviewable
  edit proposals, reference agents.

### Out of scope for v1 (revisit later)

- Complex text shaping (Arabic, Indic scripts), bidirectional text, color emoji, full CJK IME.
  Monospace Latin/Unicode-without-shaping only in v1; documented as a known limitation.
- Extension marketplace, collaborative editing, remote development servers.
- Agents executing commands or tools inside the IDE (TAP v1 is read-context + propose-edits only).
- App store *upload* automation (Play Console, App Store Connect). v1 produces and verifies signed
  artifacts; upload can be added after.

## 4. Architecture

### 4.1 Process model

```
tidepool  (one binary: Rust std + hand-written OS FFI)
├── frontends ── tp-tui (M2), tp-gui (M8)
├── tp-ui ────── layout, views, commands, keymaps, themes
├── core ─────── tp-workspace, tp-text, tp-syntax, tp-markdown, tp-formats, tp-regex
├── services ─── tp-rpc (LSP/DAP/TAP), tp-agent, tp-proc, tp-vcs, tp-mobile, tp-archive
└── tp-sys ───── the only crate allowed to use `unsafe`
      │
      ├── stdio JSON-RPC ──▶ LSP servers (rust-analyzer, …)
      ├── stdio JSON-RPC ──▶ DAP adapters (lldb-dap, …)
      ├── stdio JSON-RPC ──▶ language agents (TAP)
      └── argv + pipes ────▶ tools (cargo, git, Android SDK, JDK, Xcode, ssh, curl)
```

Everything outside the box is Tier 2: optional, user-installed, discovered at runtime, and features
degrade with a clear diagnostic when a tool is missing.

### 4.2 Threading and events

- One **UI thread** owns all editor state. No locks on the edit/render path.
- **Worker threads** for file I/O, search, indexing, and one reader thread per child-process pipe.
- Workers send messages to the UI thread over `std::sync::mpsc`. The UI thread is woken by a
  self-pipe (`poll` on stdin + pipe in the TUI) or a platform event (GUI).
- Cancellation via `Arc<AtomicBool>` tokens. Every long operation is cancellable.
- **No async runtime.** `std` threads and channels only.
- **Invariant:** the UI thread never blocks on I/O, a subprocess, an LSP server, or an agent.
  Tests in M2/M5 enforce this with deliberately slow fakes.

### 4.3 Crate layout (target)

Crates are created when their milestone starts, not up front. Merge crates whose boundary turns
out to be artificial. All crates are workspace members under `crates/`; reference agents live in a
separate workspace under `agents/` and are not linked into the IDE.

| Crate | Responsibility | Created in |
| --- | --- | --- |
| `tidepool` | Binary: CLI (std-only arg parsing), wiring, startup, session restore | M0 |
| `tp-base` | Errors, logging (to file), small collections, test PRNG, path/UTF helpers | M0 |
| `tp-text` | Rope buffer, positions (byte/char/line/UTF-16), selections, transactions, undo tree, encodings | M1 |
| `tp-formats` | JSON, TOML, XML, plist, then YAML, CSV, INI. Parsers with depth/size limits | M1 (JSON, TOML), M4 (rest) |
| `tp-sys` | Hand-written FFI: libc/POSIX, Linux, macOS (objc runtime, AppKit, CF), Win32. Only crate allowed `unsafe` | M2 |
| `tp-ui` | Frontend-agnostic UI model: layout tree, views, commands, keymaps, themes, draw list / cell grid | M2 |
| `tp-tui` | Terminal frontend: raw mode, input decoding, diffed rendering | M2 |
| `tp-workspace` | Project model, file tree, settings resolution, workspace trust, file watching, indexes | M2 (M4 watching) |
| `tp-syntax` | Highlighting framework, hand-written Rust and Markdown lexers, language-pack loader | M3 |
| `tp-markdown` | CommonMark + GFM parser to a document model for preview/outline/link checks | M3 |
| `tp-rpc` | JSON-RPC 2.0 transport (Content-Length framing) and LSP, DAP, TAP clients | M3 |
| `tp-proc` | Subprocess management (no shell), toolchain discovery, task runner, problem matchers | M3 |
| `tp-regex` | Linear-time regex engine (Pike VM), no backtracking | M4 |
| `tp-term` | VT/ANSI parser and terminal grid for the integrated terminal | M4 |
| `tp-vcs` | git via CLI (`status --porcelain=v2`, `diff`, `blame`) | M4 |
| `tp-agent` | Agent host, context assembly, consent, predictive-text engine | M5 |
| `tp-archive` | CRC-32, DEFLATE (inflate/deflate), ZIP read/write with alignment | M6 |
| `tp-mobile` | Android and iOS build/sign pipelines | M6, M7 |
| `tp-gfx` | Software rasterizer, TrueType parser/rasterizer, glyph cache, PNG decode | M8 |
| `tp-gui` | Window backends (X11 first), input, clipboard, HiDPI | M8 |
| `tp-testkit` | Fake LSP server, mock agent, fake toolchain scripts, golden-file helpers, fuzz/bench harness | M0 (grows) |

### 4.4 Protocols and file formats Tidepool defines

- **Settings:** TOML (`~/.config/tidepool/settings.toml`, `<workspace>/.tidepool/settings.toml`).
  Pending Decision D5.
- **Language packs:** TOML files under `languages/` (built-in) and user/workspace directories.
- **TAP (Tidepool Agent Protocol):** JSON-RPC 2.0 over stdio, LSP-shaped. Spec written in M5 at
  `docs/TAP.md`, versioned from v0.1.
- **Mobile build manifests:** `tidepool-android.toml` / `tidepool-ios.toml` for Gradle-free /
  Xcode-project-free builds (M6/M7).
- **`.tidepoolignore`:** gitignore syntax; excludes paths from indexing and from AI context.

## 5. Feature design notes

### 5.1 Editor engine (`tp-text`)

- Rope (B-tree of UTF-8 chunks): O(log n) insert/delete, line and char indexing.
- Position conversions between byte, char, line/column, and **UTF-16 code units** (LSP's default
  position encoding; negotiate `utf-8` via `positionEncoding` when the server supports it).
- Transactions group edits; the undo tree keeps branches; undo groups by time and by command.
- Saving is atomic: write temp file in the same directory, `fsync`, rename; preserve permissions.
- Lossless round-trip: unchanged files save byte-for-byte (BOM, CRLF, trailing newline state).
- Invalid UTF-8 opens in a read-only lossless view rather than being silently rewritten.
- Large-file mode above a threshold: no highlighting/LSP, chunked loading.

### 5.2 Frontends

- **TUI first (recommended, Decision D2).** Reaches a usable, dogfoodable editor quickly with only
  libc FFI (termios, `poll`, `ioctl` window size, `SIGWINCH`). Input: CSI/SS3 keys, bracketed
  paste, SGR mouse, optional kitty keyboard protocol. Diffed rendering; terminal state is restored
  on panic and on `SIGTERM`/`SIGHUP`.
- **Native GUI (M8).** Linux first via the X11 protocol spoken directly over a `std` Unix socket
  (no Xlib; works on Wayland desktops through XWayland), software rendering, an in-house TrueType
  rasterizer and glyph atlas. Then macOS (AppKit through the Objective-C runtime via FFI) and
  Windows (Win32/GDI). Native Wayland later (needs keymap handling equivalent to xkbcommon).
  Decisions D3, D4, D10.
- Both frontends consume the same `tp-ui` draw model, so features land once.

### 5.3 Language support

**Rust (M3):**
- Hand-written incremental lexer/highlighter (raw strings, nested block comments, lifetimes vs char
  literals, attributes, macros), bracket matching, indentation rules.
- rust-analyzer over LSP: incremental sync, diagnostics, completion, hover, signature help,
  go to definition/declaration/type/implementation, references, rename, formatting, code actions,
  inlay hints, semantic tokens, document/workspace symbols.
- Cargo awareness via `tp-formats` TOML: workspace members, targets, features.
- Tasks: `cargo check/build/test/run/clippy/fmt`, with `--message-format=json` parsed (own JSON
  parser) into the problems panel. Test discovery via `cargo test -- --list`.

**Markdown (M3):**
- CommonMark parser plus GFM extensions (tables, task lists, strikethrough, autolinks).
- Highlighting, outline, live preview (styled text in TUI, rich render in GUI), fenced-code
  highlighting through `tp-syntax`, relative-link and anchor checking, table formatting,
  word count. An external Markdown LSP is optional, not required.

**Other languages (M9):** language packs (TOML) with declarative lexer rules on `tp-regex`, plus
mappings to commonly used LSP servers (for example clangd, gopls, pyright, sourcekit-lsp,
typescript-language-server). Exact server choices are confirmed when M9 starts.

**File formats:** see §3. Every parser has depth, size, and expansion-ratio limits and is fuzzed.

### 5.4 Build, tasks, terminal, git

- `tp-proc` runs programs with `std::process::Command` and an argument vector. **Never through a
  shell**, never by interpolating strings into a command line.
- Toolchain discovery reports exactly what was found, where, and which version.
- Integrated terminal: PTY via `openpty`/`forkpty` (Linux/macOS) and ConPTY (Windows) in `tp-sys`.
- git through the CLI only (`--porcelain=v2`, `-z` output). Tidepool's git UI may create commits
  *for the user, on the user's explicit action*; Claude Code agents never commit (see `CLAUDE.md`).

### 5.5 Android: APK and AAB

**Toolchain (Tier 2, user-installed):** JDK (`keytool`, `jarsigner`, `javac`), Android SDK
build-tools (`aapt2`, `d8`, `zipalign`, `apksigner`), platform `android.jar`, NDK (for Rust/C
native code), `bundletool`, `adb`. Discovered via `ANDROID_HOME`/`ANDROID_SDK_ROOT`,
`ANDROID_NDK_HOME`, `JAVA_HOME`, and settings. Tidepool never downloads toolchains on its own.

**Project modes:**
1. **Gradle projects** (most existing apps): run the project's wrapper (`./gradlew
   :app:assembleRelease`, `:app:bundleRelease`), locate outputs, then sign with Tidepool's key
   management so signing works the same in every mode.
2. **Gradle-free projects** (`tidepool-android.toml`), including Rust `NativeActivity` apps:
   - `aapt2 compile` → `aapt2 link` (add `--proto-format` for bundles) → `javac`/`kotlinc` (if any
     JVM code) → `d8` → `cargo build --target <android-triple>` with the NDK linker → package.
   - **APK:** zip → `zipalign -p 4` → `apksigner sign` → `apksigner verify`.
   - **AAB:** module zip (`manifest/`, `dex/`, `res/`, `lib/`, `assets/`, `root/`,
     `resources.pb`) → `bundletool build-bundle` → `jarsigner` → `jarsigner -verify`.
     `apksigner` cannot sign app bundles.
   - Test installs from an AAB via `bundletool build-apks` (signed) and `bundletool install-apks`.
   - Android's docs caution that the AAPT2 bundled in build-tools may not support `bundletool`;
     confirm the correct AAPT2 source when M6 starts.

**Signing and key handling:**
- Keystore creation via `keytool -genkeypair`; keystore path and alias stored in project settings;
  **passwords never stored** by Tidepool in v1 (prompted per session, held in memory only).
- Passwords never on argv (visible to other local processes). Use `apksigner --ks-pass stdin` or
  `env:`, `jarsigner -storepass:env`, and for `bundletool` (which accepts only `pass:` or `file:`)
  a `file:` in a private temp directory with mode 0600 that is deleted after the call.
- Keystores are always excluded from AI context, search indexes, and logs.
- UI explains Play App Signing (upload key vs app signing key) and warns to back up keystores.
- Later option (ADR required): native ZIP + alignment in `tp-archive` (no crypto, verified with
  `zipalign -c`). Native signing is not planned.

### 5.6 iOS: the equivalent artifacts

| Purpose | Android | iOS equivalent |
| --- | --- | --- |
| Install on your own device | Debug APK (debug key) | Development-signed `.app`/`.ipa` (development certificate + development provisioning profile) |
| Direct distribution | Release APK | Ad hoc (registered devices) or Enterprise `.ipa` |
| Store submission | `.aab` (Play generates per-device APKs) | `.xcarchive` exported to an App Store `.ipa` (App Store Connect does app thinning) |
| Signing identity | Keystore + key alias | Certificate + private key (Keychain) + provisioning profile + entitlements |
| Verification | `apksigner verify`, `jarsigner -verify` | `codesign --verify --deep --strict`, profile decode via `security cms -D` |

**Host requirement:** building and signing iOS apps requires macOS with Xcode (SDKs, `xcodebuild`,
`codesign`, `xcrun`). On Linux or Windows, Tidepool dispatches iOS builds to a **remote Mac over
`ssh`** (and `rsync` if present) and copies artifacts back. Apple's SDK license terms restrict
where the SDKs may be used; the plan assumes a Mac (local or remote) and does not attempt to run
Apple SDKs elsewhere. `rcodesign` (open source, can sign Apple binaries on Linux) is an optional
Tier 2 tool for re-signing only; it does not remove the need for the SDK to compile. Decision D7.

**Project modes:**
1. **Xcode projects/workspaces:** `xcodebuild archive` → `xcodebuild -exportArchive` with an
   `ExportOptions.plist` generated by `tp-formats` (plist writer) for the chosen method.
2. **Xcode-free Rust apps** (`tidepool-ios.toml`): `cargo build --target aarch64-apple-ios`
   (and the simulator target) → assemble the `.app` bundle (generated `Info.plist`, executable,
   resources) → embed the provisioning profile → derive entitlements from the profile →
   `codesign` → zip `Payload/<App>.app` into an `.ipa` (in-house ZIP writer).

Signing identities stay in the macOS Keychain and are referenced by name/hash; Tidepool never
exports private keys. App Store Connect API keys (`.p8`) are secrets (same rules as keystores).

### 5.7 AI integration and predictive text

**Language agents** are external processes speaking **TAP** over stdio. Keeping them out of
process means the IDE core needs no TLS, no HTTP client, and no AI SDK, and agents can be written
in any language. One plug-in surface per language: a language pack lists its LSP server, debug
adapter, and agents together.

**TAP v0.1 (spec in M5):**
- JSON-RPC 2.0 with LSP's Content-Length framing (shares `tp-rpc` transport).
- Lifecycle: `initialize` (capability negotiation, protocol version) / `initialized` / `shutdown` /
  `exit`; document sync mirroring LSP's incremental `didOpen`/`didChange`/`didClose`.
- `tap/inlineCompletion` (ghost text; modeled on LSP's inline completion request), `tap/predict`
  (short next-word/phrase predictions, mainly for prose and Markdown), `tap/chat` with streamed
  `tap/chatDelta` notifications, `tap/proposeEdits` returning an LSP-shaped `WorkspaceEdit`,
  `$/cancelRequest`.
- **Edits are proposals.** Tidepool shows a diff and applies nothing without the user accepting.
- No command or tool execution capability in v1.

**Agent manifest** (`agent.toml`): id, version, command + args, languages (globs), requested
capabilities (`context.openBuffers`, `context.workspaceRead`, `edits.propose`), and a declared
`network` flag shown to the user. Capabilities are enforced by what the IDE sends; OS-level
sandboxing (seccomp/landlock, macOS sandbox) is a later hardening item.

**Context assembly is done by the IDE, not the agent:** prefix/suffix windows around the cursor,
open buffers if granted, size caps, `.tidepoolignore`, and a built-in secret deny-list (`.env*`,
`*.pem`, `*.key`, `*.jks`, `*.keystore`, `*.p12`, `*.p8`, `*.mobileprovision`, `id_*`,
`keystore.properties`). Per-workspace consent: once / always / never.

**Predictive text engine (`tp-agent`, in process, offline):** merges three sources:
1. LSP completion items;
2. a local model: per-language token trigram and identifier frequencies from open buffers and the
   workspace index, recency-weighted; word n-grams for Markdown/prose;
3. agent inline completions.
Popup list for 1–2, ghost text for 3 (or the top local prediction). Debounced (configurable),
cancelled on every keystroke, cached by prefix, with per-agent timeouts and concurrency limits.
Works with zero agents installed.

**Reference agents** (separate `agents/` workspace, also std-only):
- `tp-agent-mock`: deterministic test agent (fixtures, latency injection, malformed-output modes).
- `tp-agent-local`: talks to a user-run local model server over plain HTTP on loopback
  (`std::net::TcpStream`; no TLS needed for 127.0.0.1).
- `tp-agent-claude`: calls the Anthropic Messages API through the system `curl` (HTTPS by curl;
  API key read from an environment variable and passed to curl via `--config -` on stdin so it
  never appears on argv). Streaming responses parsed in-house. Decision D8.

## 6. Milestones

Tracks: **A** core/editor (M0–M4, M9), **B** AI (M5), **C** mobile (M6–M7), **D** GUI (M8),
**E** hardening (M10). C can start after M3; D can start after M2's draw model is stable.

Each milestone ends with `/verify` passing and its acceptance criteria demonstrated with evidence
(commands run, outputs observed). Performance numbers are targets to confirm in M0 (Decision D13).

### M0 — Foundations and guardrails
- [ ] Cargo workspace: edition 2024, resolver 3, `[workspace.lints]` (`unsafe_code = "forbid"`
      except `tp-sys`; `clippy::undocumented_unsafe_blocks = "deny"`), `rust-toolchain.toml` (D11)
- [ ] `tidepool` binary with std-only `--help` / `--version`; `tp-base` (error type, file logger)
- [ ] `tp-testkit` skeleton: golden-file helper, seeded PRNG, std-only bench harness
- [ ] `scripts/ci.sh` running fmt check, clippy `-D warnings`, tests, `check-deps.sh`
- [ ] `check-deps.sh` fixture tests: passes on std-only workspace; fails on registry and git deps
      (fixtures generated in a temp directory at test time, never committed)
- [ ] ADRs written for accepted decisions D1–D5, D10–D13
- **Acceptance:** `sh scripts/ci.sh` passes; `Cargo.lock` has no `source =` entries;
  `check-deps.sh` demonstrably fails on a fixture with an external crate.
- **Agents:** architect, editor-core, test-engineer, security-reviewer (lint/unsafe policy).

### M1 — Text engine (headless)
- [ ] Rope with O(log n) edits and line index
- [ ] Byte/char/line/UTF-16 position conversions
- [ ] Selections and multi-cursor edits; transactions; undo tree with grouping
- [ ] File load/save: atomic save, BOM/CRLF/trailing-newline preservation, UTF-16 with BOM,
      invalid-UTF-8 read-only view, external-change detection
- [ ] `tp-formats`: JSON (for LSP) and TOML (for settings and Cargo.toml), with limits
- **Acceptance:** property tests (≥10,000 random edit sequences vs a `String` model) pass; unchanged
  files round-trip byte-for-byte; JSON/TOML conformance fixtures pass; benchmarks recorded.
- **Agents:** editor-core, test-engineer.

### M2 — Terminal frontend (first usable editor)
- [ ] `tp-sys` libc FFI: termios raw mode, `poll`, window size, signals (every block has `// SAFETY:`)
- [ ] `tp-ui`: layout tree, editor view, tabs, splits, file tree, status bar, command palette,
      keymaps, themes
- [ ] `tp-tui`: input decoding, diffed rendering, panic/signal-safe terminal restore
- [ ] `tp-workspace`: open folder, settings (defaults → user → workspace), session restore,
      workspace-trust prompt (untrusted by default)
- [ ] In-file find/replace (plain text)
- **Acceptance:** Tidepool is used to edit Tidepool's own source (dogfooding); render snapshot
  tests pass; terminal is restored after an injected panic; UI stays responsive (frame within the
  latency target) while a fake worker blocks for 2 s.
- **Agents:** platform-ui, editor-core, security-reviewer (FFI), test-engineer.

### M3 — Rust and Markdown first-class
- [ ] `tp-rpc`: JSON-RPC transport; LSP client (lifecycle, incremental sync, position encoding)
- [ ] Fake LSP server in `tp-testkit` for deterministic tests
- [ ] rust-analyzer integration: diagnostics, completion, hover, signature help, goto (def, decl,
      type, impl), references, rename, format, code actions, inlay hints, semantic tokens, symbols
- [ ] rust-analyzer in untrusted workspaces: build scripts and proc macros disabled
- [ ] `tp-syntax`: Rust lexer/highlighter, bracket matching, indentation
- [ ] `tp-proc`: toolchain discovery, task runner, cargo JSON diagnostics → problems panel,
      test discovery
- [ ] `tp-markdown`: CommonMark + GFM parser; highlighting; outline; TUI preview; link checker
- **Acceptance:** CommonMark spec examples pass (target 100% core spec; GFM extension tests
  tracked) (test data licensing: D12); LSP client passes fake-server suite including non-ASCII
  UTF-16 positions; with rust-analyzer installed, an integration test on Tidepool's own workspace
  shows diagnostics and working go-to-definition (skipped with an explicit `SKIPPED:` reason when
  rust-analyzer is absent); `cargo build` errors appear in the problems panel with correct ranges.
- **Agents:** language-support, editor-core, test-engineer, security-reviewer (subprocess, trust).

### M4 — Workspace essentials
- [ ] `tp-regex` (linear time; Unicode classes subset) + project-wide search/replace with preview
- [ ] Fuzzy file finder; file watching (inotify, FSEvents, ReadDirectoryChangesW via `tp-sys`)
- [ ] `tp-vcs`: status, gutter diff, stage/unstage hunks, user-initiated commit UI, blame, branches
- [ ] `tp-term` + PTY: integrated terminal with scrollback
- [ ] `tp-formats`: YAML, XML, plist, CSV, INI view + validate; hex viewer
- **Acceptance:** regex conformance suite passes and pathological patterns run in linear time;
  search across Tidepool repo meets target; terminal runs an interactive shell, `vim`, and `htop`
  correctly in snapshot tests; git features verified against fixture repos.
- **Agents:** editor-core, platform-ui, language-support, test-engineer.

### M5 — AI integration and predictive text
- [ ] `docs/TAP.md` v0.1 spec (reviewed by architect + security-reviewer before code)
- [ ] `tp-agent-mock` and a TAP conformance suite
- [ ] `tp-agent` host: manifest loading, spawn/supervise/restart, capability negotiation,
      cancellation, timeouts, consent UI, context assembly with secret deny-list
- [ ] Inline completion (ghost text) end-to-end; chat panel; edit proposals as reviewable diffs
- [ ] Local predictive engine (n-gram + identifier frequency) for Rust and Markdown
- [ ] Reference agents: `tp-agent-local` (loopback HTTP) and `tp-agent-claude` (curl), opt-in
- **Acceptance:** with a mock agent that sleeps 2 s, keystroke-to-frame latency stays within
  target; stale completions are never shown after further typing; unit tests prove deny-listed
  files never enter agent context; the local predictor works with no agents and no network;
  agent crash/restart does not lose editor state; no API key appears in argv, logs, or settings.
- **Agents:** ai-agents, architect, security-reviewer, test-engineer.

### M6 — Android packaging and signing
- [ ] Toolchain discovery and diagnostics for JDK, SDK build-tools, platforms, NDK, bundletool, adb
- [ ] `tp-archive`: CRC-32, DEFLATE, ZIP read/write (APK/AAB inspection view)
- [ ] Gradle-project mode: assemble/bundle via wrapper, then Tidepool signing
- [ ] Gradle-free mode (`tidepool-android.toml`): Java sample and Rust `NativeActivity` sample
- [ ] Keystore creation, alias selection, password prompts (no persistence), Play App Signing notes
- [ ] APK: zipalign → apksigner sign → verify; AAB: build-bundle → jarsigner → verify;
      `bundletool build-apks` + `install-apks` for device testing
- **Acceptance:** for each sample, Tidepool produces a signed APK that passes
  `apksigner verify --verbose` and `zipalign -c -p 4`, and installs on an emulator via `adb`; an
  AAB that passes `jarsigner -verify` and `bundletool validate` and installs via `build-apks`;
  a test asserts that no constructed command line contains a password; missing tools produce
  actionable diagnostics. Real-toolchain tests gated by `TIDEPOOL_IT_ANDROID=1`.
- **Agents:** mobile-toolchain, security-reviewer (mandatory), test-engineer.

### M7 — iOS packaging and signing
- [ ] Toolchain discovery on macOS (Xcode, `xcodebuild`, `xcrun`, `codesign`, `security`, `simctl`)
- [ ] Signing identity and provisioning profile discovery (decode profiles; match bundle ID,
      team, certificates, entitlements, expiry)
- [ ] Xcode-project mode: archive + export with generated `ExportOptions.plist`
- [ ] Xcode-free Rust app mode (`tidepool-ios.toml`): `.app` assembly, signing, `.ipa` packaging
- [ ] Remote-Mac mode over `ssh` from Linux/Windows, with artifact retrieval
- **Acceptance:** on a macOS host, both samples produce development and App Store `.ipa` files
  that pass `codesign --verify --deep --strict`; the simulator build launches in the simulator;
  from Linux, remote mode produces the same artifacts; with no Mac configured, the UI explains the
  requirement. Real-toolchain tests gated by `TIDEPOOL_IT_IOS=1`.
- **Agents:** mobile-toolchain, security-reviewer (mandatory), test-engineer.

### M8 — Native GUI
- [ ] `tp-gfx`: software rasterizer, TrueType parser and anti-aliased rasterizer, glyph atlas,
      PNG decode (uses `tp-archive` inflate)
- [ ] `tp-gui` Linux: X11 protocol over Unix socket (auth via Xauthority), window, input,
      clipboard, HiDPI; MIT-SHM later
- [ ] GUI Markdown rich preview; image viewer
- [ ] macOS backend (AppKit via objc runtime FFI); Windows backend (Win32/GDI)
- **Acceptance:** smooth scrolling of a 10,000-line file at 1080p within the frame target on the
  reference machine; font rasterization golden tests; feature parity with the TUI on Linux;
  fuzzed TrueType and PNG parsers.
- **Agents:** platform-ui, security-reviewer (FFI, parsers), test-engineer.

### M9 — Debugging and language expansion
- [ ] DAP client; Rust debugging through an installed adapter (e.g. lldb-dap or CodeLLDB):
      breakpoints, step, call stack, variables, watch, debug console
- [ ] Declarative language-pack lexers on `tp-regex`
- [ ] Language packs for common languages (C/C++, Python, JavaScript/TypeScript, Go, Java,
      Kotlin, Swift, Shell, SQL, HTML/CSS, YAML, Dockerfile, Makefile) with LSP mappings
- **Acceptance:** debug session on a sample Rust binary hits breakpoints and shows locals; each
  pack has highlight golden tests and documented LSP setup.
- **Agents:** language-support, test-engineer.

### M10 — Hardening and release
- [ ] Fuzz campaigns for every parser (JSON, TOML, YAML, XML, plist, Markdown, regex, ZIP,
      DEFLATE, PNG, TrueType, VT, LSP/DAP/TAP framing)
- [ ] Accessibility plan and first implementation (screen-reader APIs per platform)
- [ ] Packaging Tidepool itself (Linux tarball/.deb, macOS signed/notarized app, Windows installer)
- [ ] Optional OS sandboxing for agents; performance pass; user documentation
- **Agents:** security-reviewer, test-engineer, platform-ui, architect.

## 7. Testing and verification

- **Unit tests** in each crate; **integration tests** in `crates/*/tests/`.
- **Golden files** for render grids, token streams, Markdown output, and protocol transcripts.
- **Property tests** with the seeded PRNG in `tp-testkit` (failing seed is printed and replayable).
- **Fuzzing** with a std-only harness: `cargo run -p tp-testkit --bin fuzz -- <target> --seed S
  --iterations N`, with a checked-in corpus. (`cargo-fuzz` needs `libfuzzer-sys`, a Tier 3 crate.)
- **Fakes:** fake LSP server, mock agent, fake toolchain scripts that mimic tool output and exit
  codes, so most tests run with no external tools installed.
- **Real-tool integration tests** gated by environment variables and reported as `SKIPPED: <reason>`
  when unavailable. A skipped test is never reported as passed.
- **Miri** (rustup component) for crates without FFI where practical.
- **Definition of done** for any change: fmt, clippy `-D warnings`, tests, `check-deps.sh` all pass,
  plus the milestone's acceptance evidence.

## 8. Security model

| Area | Rule |
| --- | --- |
| `unsafe` / FFI | Only in `tp-sys`; every block has a `// SAFETY:` comment stating the invariants; small, reviewed wrappers expose safe APIs. |
| Untrusted files | Every opened file is untrusted. Parsers enforce limits (nesting depth, size, decompression ratio) and are fuzzed. No panics on malformed input. |
| Workspace trust | New folders are untrusted: no tasks, no build scripts, rust-analyzer without build scripts/proc macros, no agents with workspace read, until the user trusts the folder. (rust-analyzer and Gradle can execute project code.) |
| Subprocesses | `Command` + argv only, never a shell; minimal environment for agents; child processes killed on exit. |
| Secrets | Signing passwords and API keys never on argv, in logs, settings, crash output, or AI context. Prompted and held in memory; temp files 0600 in a private directory, deleted after use. |
| Signing | Delegated to `apksigner`, `jarsigner`, `codesign`. No in-house signing crypto without an approved ADR, test vectors, independent verification, and side-channel review. |
| AI | Consent per workspace; IDE-side context assembly with deny-list; edits are reviewable proposals; agents cannot execute commands in v1; repository content can contain prompt injection, so agent output is treated as untrusted. |
| Network | Core binary opens no sockets in v1 (verifiable). Network happens in user-controlled external processes. |
| Regex | Linear-time engine; no backtracking (no ReDoS). |

## 9. Risk register

| # | Risk | Impact | Mitigation |
| --- | --- | --- | --- |
| R1 | Zero-dependency scope (GUI, fonts, regex, parsers, protocols) is very large for a small team | Schedule | TUI first; strict milestone acceptance; defer complex text; push heavy features into external processes |
| R2 | Complex text (shaping, bidi, emoji, IME) | Some users can't use the GUI fully | Documented v1 limitation; revisit with an ADR (platform text APIs via FFI are Tier 1 on macOS/Windows) |
| R3 | Accessibility requires platform APIs | Excludes screen-reader users | Planned in M10; design `tp-ui` with an accessibility tree from M2 so it can be exposed later |
| R4 | iOS requires macOS + Xcode; Apple licensing | No local iOS builds on Linux | Remote-Mac mode; clear UI messaging; no attempt to run Apple SDKs elsewhere |
| R5 | Android toolchain churn (AAPT2 variants, bundletool versions, target API rules) | Broken builds | Version detection, gated real-tool tests, confirm current requirements when M6 starts |
| R6 | Unsound hand-written FFI | Memory safety bugs | Confinement in `tp-sys`, SAFETY comments, mandatory security review, small surface |
| R7 | Opening untrusted projects executes code (build scripts, proc macros, Gradle) | Code execution | Workspace trust (§8) |
| R8 | AI data leakage or prompt injection | Secret exposure, bad edits | Deny-list, consent, proposals-only edits, no command execution |
| R9 | License compatibility of bundled assets and test data | Legal | Ledger records license and source for every asset; review before adding |
| R10 | Single-maintainer bandwidth | Schedule | Small milestones, Claude Code agents with clear briefs, acceptance criteria as the gate |

## 10. Decisions needed (owner input)

Each accepted decision becomes an ADR in `docs/adr/` (use `/adr`).

| ID | Decision | Recommendation | Needed by |
| --- | --- | --- | --- |
| D1 | Adopt the dependency tiers in `DEPENDENCIES.md` | Accept as written | M0 |
| D2 | First frontend | TUI first, GUI in M8 | M0 |
| D3 | Linux GUI backend | X11 protocol over `std` Unix socket (no Xlib); native Wayland later | M8 |
| D4 | Fonts | Bundle one SIL OFL monospace font (license-compatible) and allow system fonts | M8 |
| D5 | Settings format | TOML (parser is needed for Cargo.toml anyway) | M0 |
| D6 | Android signing | Delegate to `apksigner`/`jarsigner`; no native signing | M6 |
| D7 | iOS builds | macOS host, plus remote Mac over `ssh` from Linux; `rcodesign` optional | M7 |
| D8 | Where AI network code lives | Out of process; reference Claude agent uses system `curl` | M5 |
| D9 | Agent protocol | Own LSP-shaped TAP; consider exposing an MCP server later | M5 |
| D10 | Platform order | Linux → macOS → Windows | M0 |
| D11 | Toolchain pinning | Pin a stable Rust in `rust-toolchain.toml`, bump deliberately | M0 |
| D12 | CommonMark spec test data (published under CC-BY-SA 4.0; verify) | Vendor as test-only fixtures with license notice, excluded from binaries | M3 |
| D13 | Performance targets | Confirm: TUI first frame < 150 ms; keystroke-to-frame p99 < 16 ms; 100 MB file opens < 1 s | M0 |
| D14 | CI | Add a hosted CI workflow running `scripts/ci.sh` on Linux/macOS/Windows | M0 |

## 11. Working with Claude Code

Agent files live in `.claude/`. See `CLAUDE.md` for the rules every session follows.

| Agent | Use for |
| --- | --- |
| `architect` | Designs, ADRs, crate boundaries, dependency-policy decisions, protocol specs. Read-mostly; writes only under `docs/`. |
| `editor-core` | `tp-text`, `tp-formats`, `tp-workspace`, `tp-regex`, `tp-base`. |
| `platform-ui` | `tp-sys`, `tp-ui`, `tp-tui`, `tp-term`, `tp-gfx`, `tp-gui`. |
| `language-support` | `tp-syntax`, `tp-markdown`, `tp-rpc` (LSP/DAP), language packs, Rust and Markdown features. |
| `mobile-toolchain` | `tp-proc` toolchain discovery, `tp-archive`, `tp-mobile` (Android + iOS). |
| `ai-agents` | TAP spec, `tp-agent`, predictive engine, reference agents. |
| `test-engineer` | Tests, fakes, golden files, fuzz and bench harnesses, `scripts/ci.sh`. |
| `security-reviewer` | Read-only review of `unsafe`/FFI, subprocesses, signing, secrets, parsers, AI context. Mandatory for M6, M7, and any `tp-sys` change. |

Skills: `/verify` (all checks), `/next-task` (take the next unchecked item from this plan),
`/adr <title>` (draft a decision record).

Typical loop: `/next-task` → architect for anything non-trivial → implementer agent →
test-engineer → security-reviewer when the change touches a §8 area → `/verify` → Claude proposes
a commit message; the owner reviews and commits.

## 12. References

- Android: [bundletool](https://developer.android.com/tools/bundletool),
  [Build your app from the command line](https://developer.android.com/build/building-cmdline)
- Apple signing on non-Apple hosts: [apple-codesign / rcodesign](https://gregoryszorc.com/docs/apple-codesign/0.12.0/index.html)
- Claude Code: [subagents](https://code.claude.com/docs/en/sub-agents.md),
  [skills](https://code.claude.com/docs/en/skills.md),
  [settings](https://code.claude.com/docs/en/settings.md),
  [permissions](https://code.claude.com/docs/en/permissions.md),
  [hooks](https://code.claude.com/docs/en/hooks.md)
