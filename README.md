# Tidepool

A zero-dependency IDE for software developers, written in Rust. It has a
native GUI in the style of [jabbott.dev](https://jabbott.dev), a terminal UI
with vim-style modal editing, and first-class Rust and Markdown support.
It also builds and signs Android and iOS apps, and has AI features through
plugged-in language agents.

**Status:** design complete, no code yet. Work starts at milestone M0 in
`docs/roadmap.md`.

## How to use this repository with Claude Code

1. Start Claude Code in the repository root. It loads `CLAUDE.md`, which
   imports `AGENTS.md`.
2. Run `/next-task`. It picks up the first unblocked roadmap task and works
   through it, leaving each change uncommitted for you to review.

## What is in here

| Path | Contents |
| :- | :- |
| `AGENTS.md` | Project rules for any coding agent: scope, constraints, workspace map, commands, conventions, definition of done |
| `CLAUDE.md` | Imports `AGENTS.md`, plus Claude Code skills, subagents, rules and permissions |
| `docs/00-overview.md` | Goals, non-goals, budgets, assumptions, scope estimate, risks |
| `docs/01-architecture.md` | Crates, layers, dependency matrix, main loop, data flow, threading, frontends |
| `docs/02-dependency-policy.md` | The build-it-ourselves rule, managed tools, the exception process, the build-vs-buy register |
| `docs/03-platforms.md` | Linux, macOS and Windows specifics |
| `docs/04-quality.md` | Testing strategy, own harnesses, `xtask` checks, CI, FFI review checklist |
| `docs/05-release.md` | Profiles, packaging, signing Tidepool's releases, crash reports |
| `docs/subsystems/` | One spec per subsystem, including the jabbott.dev theme, vim mode, managed toolchains, mobile and agents |
| `docs/roadmap.md` | Milestones M0–M14 plus backlog, with checkbox tasks and acceptance criteria |
| `docs/decisions.md` | Architecture decision records (ADRs) |
| `.claude/` | Settings, hooks, subagents, path-scoped rules and skills |
| `scripts/check-deps.sh` | Interim dependency-policy check |

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

Apache-2.0. Bundled fonts are licensed under the SIL Open Font License 1.1.
