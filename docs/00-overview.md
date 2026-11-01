# 00 — Overview

## Purpose

Tidepool is an IDE for software developers, written in Rust, with no
third-party crates. It runs on Linux, macOS and Windows as a native GUI, and
in a terminal as a TUI with vim-style modal editing.

Rust and Markdown are first-class from the start (ADR-0005). Tidepool also
builds and signs Android (`.apk`, `.aab`) and iOS (`.app`, `.xcarchive`,
`.ipa`) apps, and it has AI features through plugged-in language agents.

Tidepool is built to be used on itself. The rule is: **a feature is not done
until Tidepool's own development uses it.** Dogfooding starts at M4.

## Goals

1. A fast, dependable editor for Rust and Markdown, with semantic features
   through rust-analyzer and a Markdown preview in the jabbott.dev look.
2. Own the whole stack above the OS: windowing, rendering, fonts, text,
   parsers, protocols. Any bug can be understood and fixed in-house
   (ADR-0001).
3. **Managed toolchains:** rust-analyzer, git, the Android SDK and Xcode
   are installed, updated, configured and driven by Tidepool, through each
   tool's official channel, with panels in the IDE (ADR-0002).
4. Build, sign and verify Android and iOS artifacts from the IDE.
5. AI completion, chat and edit proposals through language agents, plus
   offline predictive text that works with no agent installed.
6. One visual identity: jabbott.dev's dark "capsuleer" design language
   (ADR-0004, `docs/subsystems/ui-and-theme.md`).
7. Small enough for one engineer and a coding agent to hold in their heads.
   As an estimate, about 60–80k lines of Rust at M14.

## Non-goals

- Bundling or rebuilding outside tools (ADR-0002). Tidepool drives
  rust-analyzer, git, the SDKs and Xcode; it does not ship or reimplement
  them.
- Running Apple SDKs anywhere but macOS (ADR-0008).
- Complex text shaping (Arabic, Indic scripts), bidirectional text, color
  emoji and full CJK input in v1. Backlog.
- An extension marketplace, collaborative editing, remote development
  servers. Backlog.
- Agents that run commands or tools inside the IDE. Agents read context and
  propose edits only (ADR-0009).
- App store upload automation (Play Console, App Store Connect). Tidepool
  produces and verifies signed artifacts. Backlog.
- Vimscript, or loading a user's `.vimrc` as-is. Vim mode is a defined
  subset (ADR-0006).

## Performance budgets

These are the numbers we test against. The user confirmed every budget as
written on 2026-10-31 (M0.5).

**Reference machine** (the main development machine): MSI Pulse GL66
11UGKV laptop; Intel Core i7-11800H, 8 cores / 16 threads; 16 GB RAM
(15.3 GiB usable); NVMe SSD; Pop!_OS 24.04 LTS, kernel
`7.2.6-76070206-generic`; built-in 1920×1080 panel on Intel UHD Graphics
(an NVIDIA GeForce RTX 3070 Mobile is also present); COSMIC Wayland
session, so the GUI runs through XWayland (ADR-0010).

| Budget | Target (confirmed) |
| :- | :- |
| Cold start to first frame (GUI) | ≤ 300 ms |
| Cold start to first frame (TUI) | ≤ 150 ms |
| Keystroke to pixels | p99 ≤ 16 ms at 1920×1080 on the reference machine |
| Scrolling | 60 fps through a 10,000-line Rust file at 1080p |
| Open a file | 10 MB in ≤ 200 ms; 100 MB in ≤ 1 s (large-file mode above 16 MB) |
| Project search | Tidepool's repository in ≤ 300 ms, warm |
| Idle | No redraw and ~0% CPU when nothing changes |
| Slow LSP server or agent | Adds 0 ms to keystroke latency (UI thread never waits) |
| Memory | ≤ 200 MB resident with Tidepool's workspace open, excluding external tools |
| Shipped binary | ≤ 25 MB per platform, including bundled fonts |

## Assumptions

Revisit these if any turns out to be wrong.

- One engineer, with Claude Code doing much of the implementation under
  review.
- Developers have a network connection when they first install toolchains.
  Editing works fully offline.
- The user has, or can get, an administrator account for installs that need
  one (git from a package manager, Xcode). Tidepool never elevates silently.
- A Mac is available (local or over SSH) for iOS work (ADR-0008).
- Desktop machines have at least 8 GB RAM and an SSD.
- X11 or XWayland is available on Linux desktops (ADR-0010).

## Scope estimate

Sizes are rough line-count estimates for planning, not commitments.
S ≈ <1k lines, M ≈ 1–3k, L ≈ 3–6k, XL ≈ 6k+.

| Area | Size | Why |
| :- | :- | :- |
| `sys` bindings (all OSes) | L | libc, X11, objc/AppKit, Win32, mostly mechanical, each with layout tests |
| `platform` Linux / macOS / Windows | L each | Windows, input, clipboard, DPI, PTY, file watching, processes |
| `gfx` | M | CPU rasterizer: rects, cut corners, lines, alpha, clipping, images, damage |
| `font` | L | TrueType parsing, outline rasterization, glyph atlas, kerning, fallback |
| `ui` | L | Widget tree, layout, focus, input routing, theme, draw lists |
| `text` | M | Rope, positions, selections, undo tree, encodings |
| `codec` | L | JSON, TOML, XML, plist, YAML, CSV, INI, DEFLATE, ZIP, PNG, QOI |
| `regex` | M | Linear-time engine |
| `syntax` + `markdown` | M + M | Rust and Markdown lexers, language packs, CommonMark + GFM |
| `rpc` + `lang` | M + L | JSON-RPC, LSP/DAP clients, language sessions, cargo tasks |
| `toolchain` | L | Tool registry, detection, installs, updates, supervision, downloads |
| `vcs`, `term` | M each | git over the CLI; VT parser and PTY terminal |
| `editor` | XL | Editor view, vim mode, panels: files, search, problems, git, toolchains, devices, chat |
| `agent` | M | TAP host, context assembly, consent, predictive engine |
| `mobile` | L | Android and iOS pipelines, signing orchestration |
| `gui`, `tui` | M each | Frontends over the same `ui` draw lists |
| `xtask` + harnesses | M | CI checks, goldens, fuzz smoke, benches |

The biggest risk is **breadth**: three windowing systems, a font
rasterizer, a UI toolkit, a terminal emulator and two mobile toolchains, all
without crates. The roadmap lands Linux first, makes Tidepool usable on
itself early (M4), and adds one capability area per milestone.

## Top risks and mitigations

| Risk | Mitigation |
| :- | :- |
| Hand-written FFI has ABI or layout bugs | Declarations only in `sys`, with `size_of`/`align_of` asserts from official headers; safe wrappers in `platform`; `/add-ffi-binding` |
| Text rendering quality (no hinting, no shaping) | Grayscale anti-aliasing tuned for JetBrains Mono and Saira at UI sizes; golden tests; shaping is backlog with a trigger |
| Accessibility needs per-OS screen-reader APIs | `ui` keeps an accessibility tree from M3 (stable IDs, roles, labels); platform bridges in M14 |
| Managed installs are a supply-chain path | Official channels only, user consent, pinned versions and SHA-256 checks, licenses shown to the user, never silent elevation (ADR-0002, ADR-0016) |
| Opening an untrusted project runs its code (build scripts, proc macros, Gradle) | Workspace trust, untrusted by default (ADR-0014) |
| iOS needs macOS and Xcode | Remote-Mac builds over SSH; clear messages when no Mac is configured (ADR-0008) |
| Android and Apple tooling changes | Version detection, gated real-tool tests, re-check official docs at the start of M10 and M11 |
| AI leaks secrets or applies bad edits | IDE-side context assembly with a deny-list, consent, edits as reviewable proposals (ADR-0009) |
| Vim mode scope creep | A defined subset with a conformance table; Vimscript is a non-goal (ADR-0006) |
| One maintainer | Small tasks with acceptance criteria and verification commands; Claude Code agents with narrow briefs |
