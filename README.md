# Tidepool

A zero-dependency IDE for software developers, written in Rust. It is to
have a native GUI in the style of [jabbott.dev](https://jabbott.dev), a
terminal UI with vim-style modal editing, and first-class Rust and Markdown
support. It will also build and sign Android and iOS apps, offer AI
features through plugged-in language agents, and run plugins written in
Ferrolua, its author's scripting language derived from Lua 5.5.

**Status:** in development, at version 0.1. The guarded
workspace and its checks, the contribution workflow, license headers, the
core library, JSON, TOML and its document model, which writes edits
back, the text engine's rope, positions,
selections, undo tree, encodings and atomic save, a fuzzer for the
parsers, a benchmark harness with the text engine's suites, the
glyph atlas's, the command palette's, the editor view's, and opening a
100 MB file, and a
Linux window over X11, with its events, a waker, presenting what
changed and the clipboard, on hand-written libc and X11 declarations, the
XDG folders, the file metadata a save checks (links, owner and group), a
CPU rasterizer with jabbott.dev's backdrop, checked
against golden frames by a QOI codec and harness of our own, and a
TrueType parser, glyph rasterizer, glyph atlas and text layout, with
fallback, for the bundled Saira and JetBrains Mono, and the UI toolkit's
widget tree, with its layout, focus, input routing, draw lists and
accessibility tree, its theme of jabbott.dev's tokens, checked for
WCAG AA contrast, its controls (buttons, chips, text and search fields,
the scrollbar, status dots, titles and breadcrumbs), its collections
(lists, tree views and tables of any number of rows, tab strips,
splitters, cards and code blocks), its overlays (a modal dialog, which
keeps the rest of the window from taking input while it is open,
tooltips, toasts, and the command palette), the painter
that draws its frames into pixels, the changed parts only, and the
editor view, which draws a file's text with line numbers, cursors,
selections, the current line and soft wrap, laying out only the lines on
screen, named commands run by keys through a modal keymap engine,
from a default keymap and your bindings in `settings.toml`, and the
editing commands, which move, type, delete, indent, undo, copy and paste
at many cursors at once, and workspace folders, whose `.gitignore` and
`.tidepoolignore` files are read as Git reads them, workspace trust,
untrusted until you trust a folder, layered settings, checked and
written back with their comments kept, open buffers, loaded and saved on
worker threads, which ask before a save would split a hard link or change
a file's owner or group, and take an answer only for the file and the
question it was given for, the file tree, read a folder at a time, and each
folder's session in the user data folder, are built;
and
`cargo tp` opens a Linux window and runs the event-driven main loop,
which waits until something happens and draws only what changed: it shows
the empty shell over jabbott.dev's backdrop, built off the main thread,
with its activity bar, side and bottom panels, status bar and command
palette, and opens no folder or file yet, so it does not show the editor
view until M4.3.
[`docs/roadmap.md`](docs/roadmap.md) ticks each task as it is done, each
crate's `README.md` says what it has, and [`diagrams/`](diagrams/) draws
what is built and what is planned.

## Build and check

You need [rustup](https://rustup.rs): `rust-toolchain.toml` pins the
toolchain and its components, and rustup installs them the first time you
run `cargo` here. Tidepool has no third-party crates, so nothing else is
downloaded.

```bash
cargo xtask ci        # the full gate: deps, fmt, clippy, tests, golden, layers, crates, features, unsafe-audit, headers, workflows, ucd, theme
cargo test -p tidepool-text
```

[`CONTRIBUTING.md`](CONTRIBUTING.md) has the rest: the workflow, every
command, and what to run for each kind of change.

## Working with Claude Code

1. Start Claude Code in the repository root. It loads `CLAUDE.md`, which
   imports `AGENTS.md`.
2. Run `/next-task`. It picks the first unblocked roadmap task, asks any
   decision the task needs first, and works it through the definition of
   done, leaving the change uncommitted for you to review.

## What is in here

| Path | Contents |
| :- | :- |
| `AGENTS.md` | Project rules for any coding agent: status, constraints, workspace map, commands, conventions, definition of done |
| `CLAUDE.md` | Imports `AGENTS.md`, plus Claude Code skills, subagents, rules, permissions and the hook |
| `CONTRIBUTING.md` | How to propose, make and check a change |
| `SECURITY.md` | How to report a vulnerability, and what is in scope |
| `docs/00-overview.md` | Goals, non-goals, budgets and the reference machine, assumptions, scope estimate, risks |
| `docs/01-architecture.md` | Crates, layers, dependency matrix, main loop, data flow, threading, frontends |
| `docs/02-dependency-policy.md` | The build-it-ourselves rule, managed tools, assets, the exception process, the build-vs-buy register |
| `docs/03-platforms.md` | Linux, macOS and Windows specifics |
| `docs/04-quality.md` | Testing strategy, own harnesses, `xtask` checks, CI, FFI review checklist |
| `docs/05-release.md` | Profiles, packaging, signing Tidepool's releases, crash reports |
| `docs/subsystems/` | One spec per subsystem, including the jabbott.dev theme, vim mode, managed toolchains, mobile and agents |
| `docs/roadmap.md` | Milestones M0–M15 plus backlog, with checkbox tasks, acceptance criteria and verification commands |
| `docs/decisions.md` | Architecture decision records (ADRs) |
| `crates/`, `xtask/`, `bench/` | The workspace; each member has a `README.md`, and copies of `LICENSE` and `NOTICE` |
| `diagrams/` | C4, UML class, component and sequence, data-flow and state diagrams, in Mermaid |
| `.claude/` | Claude Code settings, hook, subagents, path-scoped rules and skills |
| `.github/` | CI, CodeQL and Pages workflows, Dependabot, the pull request template and issue form |
| `site/` | The demo page's template, stylesheet, script and `llms.txt`, which `cargo xtask site` fills (ADR-0030) |
| `scripts/check-deps.sh` | Fallback dependency check for the Claude Code hook |

## The constraints that shape everything

1. **Zero third-party crates** (ADR-0001). OS bindings, the rasterizer, the
   font renderer, the UI toolkit, parsers, protocols and the terminal
   emulator are all ours.
2. **Outside tools are managed, not bundled** (ADR-0002). rust-analyzer,
   git, the Android SDK and Xcode are installed and driven by Tidepool
   through their official channels.
3. **GUI first, then the terminal UI with vim mode** (ADR-0003, ADR-0006).
4. **One look:** jabbott.dev's design language (ADR-0004).

## License

Apache-2.0: see [`LICENSE`](LICENSE) and [`NOTICE`](NOTICE). The bundled fonts in
[`assets/fonts/`](assets/fonts/) are licensed under the SIL Open Font License 1.1 (each
directory's `OFL.txt`), and the Unicode data under the
Unicode License v3 (`crates/text/ucd/18.0.0/license.txt`).
