# Tidepool

A zero-dependency IDE for software developers, written in Rust. It is to
have a native GUI in the style of [jabbott.dev](https://jabbott.dev), a
terminal UI with vim-style modal editing, and first-class Rust and Markdown
support. It will also build and sign Android and iOS apps, and offer AI
features through plugged-in language agents.

**Status:** in development, at version 0.1, and still headless. The guarded
workspace and its checks, the contribution workflow, license headers, the
core library, JSON and TOML, the text engine's rope, positions,
selections, undo tree, encodings and atomic save, and a fuzzer for the
parsers are built;
nothing opens a window yet. [`docs/roadmap.md`](docs/roadmap.md) ticks each task as it is done,
and each crate's `README.md` says what it has.

## Build and check

You need [rustup](https://rustup.rs): `rust-toolchain.toml` pins the
toolchain and its components, and rustup installs them the first time you
run `cargo` here. Tidepool has no third-party crates, so nothing else is
downloaded.

```bash
cargo xtask ci        # the full gate: deps, fmt, clippy, tests, layers, crates, unsafe-audit, headers, workflows, ucd
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
| `docs/roadmap.md` | Milestones M0–M14 plus backlog, with checkbox tasks, acceptance criteria and verification commands |
| `docs/decisions.md` | Architecture decision records (ADRs) |
| `crates/`, `xtask/`, `bench/` | The workspace; each member has a `README.md`, and copies of `LICENSE` and `NOTICE` |
| `diagrams/` | C4, UML class, component and sequence, data-flow and state diagrams, in Mermaid |
| `.claude/` | Claude Code settings, hook, subagents, path-scoped rules and skills |
| `.github/` | CI and CodeQL workflows, Dependabot, the pull request template and issue form |
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

Apache-2.0: see [`LICENSE`](LICENSE) and [`NOTICE`](NOTICE). Bundled fonts (from M2.8) are
licensed under the SIL Open Font License 1.1, and the Unicode data under the
Unicode License v3 (`crates/text/ucd/17.0.0/license.txt`).
