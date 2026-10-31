@AGENTS.md

## Claude Code specifics

The project rules are in `AGENTS.md` (imported above), shared with any other
coding agent. This section covers only what is specific to Claude Code.

**Project skills** (in `.claude/skills`):

| Skill | Use it to… |
| :- | :- |
| `/next-task [task-id]` | Pick the next unblocked roadmap task, or the one named, and carry it through the definition of done |
| `/verify` | Run the full verification suite and report exact results |
| `/record-decision <title>` | Append an ADR to `docs/decisions.md` |
| `/evaluate-dependency <crate> <need>` | Run the build-vs-buy evaluation before anyone proposes a third-party crate |
| `/add-ffi-binding <os> <api>` | Add OS bindings to `tidepool-sys` and wrap them safely in `tidepool-platform` |
| `/add-managed-tool <tool>` | Add or update a managed tool: descriptor, detection, install plan, tests, panel row |

**Subagents** (in `.claude/agents`):

| Agent | Owns |
| :- | :- |
| `architect` | Design, ADRs, crate boundaries, protocol and format specs (writes only in `docs/`) |
| `editor-core` | `core`, `text`, `codec`, `regex`, `workspace`, `editor` (including vim mode), `vcs` |
| `platform-ui` | `sys`, `platform`, `gfx`, `font`, `ui`, `gui`, `tui`, `term`, the jabbott.dev theme |
| `language-support` | `syntax`, `markdown`, `rpc`, `lang`, language packs |
| `mobile-toolchain` | `toolchain` (all managed tools), `mobile` |
| `ai-agents` | `agent`, TAP, predictive text, `agents/` |
| `test-engineer` | `testkit`, harnesses, `xtask`, `bench`, CI |
| `security-reviewer` | Read-only review; mandatory for `sys`/`platform`, `toolchain`, `mobile`, `agent` and parser changes |

**Path-scoped rules** in `.claude/rules` load automatically when you open
matching files:

| Rule file | Applies to |
| :- | :- |
| `dependencies.md` | Cargo manifests, `.cargo/`, `xtask`, `rust-toolchain.toml` |
| `unsafe-and-ffi.md` | `sys`, `platform` |
| `parsers.md` | `codec`, `font`, `markdown`, `syntax`, `rpc`, `term`, `vcs`, fuzz corpora |
| `toolchains-and-secrets.md` | `toolchain`, `mobile`, `vcs`, `toolchains/` |
| `ai-context.md` | `agent`, `agents/` |
| `ui-style.md` | `ui`, `gui`, `tui`, `editor`, `themes/`, `assets/` |

**Permissions and hooks:**

- `.claude/settings.json` pre-approves routine `cargo` commands and read-only
  `git` commands.
- It denies Git operations that publish or finalize changes, `cargo
  publish` and `cargo login`, and reading signing material and `.env` files.
- It asks before `cargo add`, `cargo remove`, `cargo update` and
  `cargo install`.
- A `PostToolUse` hook runs the dependency check after edits that touch a
  `Cargo.toml` or `Cargo.lock`.
- These are guardrails, not a security boundary. The rules in `AGENTS.md`
  still apply when a command slips past a pattern.

**Working style:**

- Use the task list for any roadmap task with more than two steps. Make the
  last item "verify", and run the task's verification commands for it.
- Most tasks touch one or two crates. If a change spreads across more than
  three crates, stop and say so: the task is probably mis-scoped.
- When an acceptance criterion needs an OS, tool or device this session
  can't run (Claude Code sessions are usually Linux without the Android SDK
  or Xcode), implement it and say plainly which checks still need it.
