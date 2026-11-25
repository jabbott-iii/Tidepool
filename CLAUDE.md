@AGENTS.md

## Claude Code specifics

The project rules are in `AGENTS.md` (imported above), shared with any other
coding agent. This section covers only what is specific to Claude Code.

**Project skills** (in `.claude/skills`). Tidepool's own:

| Skill | Use it to… |
| :- | :- |
| `/verify` | Run the full verification suite and report exact results |
| `/record-decision <title>` | Append an ADR to `docs/decisions.md`, or accept or supersede one |
| `/evaluate-dependency <crate> <need>` | Run the build-vs-buy evaluation before anyone proposes a third-party crate |
| `/add-ffi-binding <os> <api>` | Add OS bindings to `tidepool-sys` and wrap them safely in `tidepool-platform` |
| `/add-managed-tool <tool>` | Add or update a managed tool: descriptor, detection, install plan, tests, panel row |

And exact copies of [Doctrina](https://github.com/jabbott-iii/Doctrina)'s
portable skills (ADR-0071), which only the user starts; change them in
Doctrina and copy them over again, never here:

| Skill | Use it to… |
| :- | :- |
| `/next-task [task-id]` | Pick the next unblocked roadmap task, or the one named, ask its `Decide first` questions, and carry it through the definition of done |
| `/security-sweep [path, area or Git range]` | Find security vulnerabilities in the code, dependencies, CI workflows and Git history, prove each one, and patch the ones the user approves |
| `/sweep-code [path, area or Git range] [bugs \| dead code]` | Scan for bugs and dead code, prove each finding, then fix the approved bugs with regression tests and remove the approved dead code |
| `/simplify-code [path, area or Git range]` | Find functions and modules longer or more convoluted than needed, and simplify the approved ones without losing behavior |
| `/fix-doc-drift [path, area or Git range]` | Find statements in the docs that no longer match the code or the records, and correct them, asking first where the docs may be right |
| `/draw-diagrams [file or view]` | Draw or rebuild the Mermaid diagrams in `diagrams/` |
| `/phase-gates [plan \| phase]` | Cluster roadmap tasks into gated phases, or evaluate a gate; Tidepool's **Gate evidence** is "none" |
| `/build-language-packs [languages] [area]` | Translate the user-facing text into nine languages as checked language packs; Tidepool is "English only" today |
| `/set-up-github [pieces]` | Set up or review CodeQL, Dependabot, `SECURITY.md`, `CONTRIBUTING.md`, the templates, `CODEOWNERS` and `.gitignore` |
| `/build-course [what to teach]` | Build a course; it needs `teaching-materials.md`, which Tidepool has not adopted, and says so |

**Subagents** (in `.claude/agents`):

| Agent | Owns |
| :- | :- |
| `architect` | Design, ADRs, crate boundaries, protocol and format specs (writes only in `docs/`) |
| `editor-core` | `core`, `text`, `codec`, `regex`, `workspace`, `editor` (including vim mode), `vcs` |
| `platform-ui` | `sys`, `platform`, `gfx`, `font`, `ui`, `gui`, `tui`, `term`, the jabbott.dev theme |
| `language-support` | `syntax`, `markdown`, `rpc`, `lang`, language packs |
| `mobile-toolchain` | `toolchain` (all managed tools), `mobile` |
| `ai-agents` | `agent`, TAP, predictive text, `agents/` |
| `test-engineer` | `testkit`, harnesses, `xtask`, `bench`, CI workflows, the demo page (`site/`, `pages.yml`) |
| `security-reviewer` | Read-only review; mandatory for `sys`/`platform`, `toolchain`, `mobile`, `vcs`, `agent`, parser and workflow changes |

Each owner also keeps the `README.md` of the crates it changes true
(ADR-0023, ADR-0028).

**Rules** in `.claude/rules`. Six are copies of Doctrina's portable rules
(ADR-0071): their general text stays exactly as Doctrina has it, and only
their "Project details" are Tidepool's. Four of them load in every
session:

| Rule file | Covers |
| :- | :- |
| `writing.md` | Everything written for people to read: docs, READMEs, comments, doc comments, user-facing text, commit messages and pull requests |
| `commits.md` | Commit messages (Conventional Commits, in the imperative), one change per commit, read-only Git, pull requests |
| `dependencies.md` | Everything Tidepool uses that it didn't write; first party is only `https://github.com/jabbott-iii` |
| `testing.md` | Strict tests: never weakened to pass, every behavior covered, budgets and security checks |

The others load automatically when you open matching files:

| Rule file | Applies to |
| :- | :- |
| `rust.md` (Doctrina's) | `**/*.rs`, Cargo manifests and `Cargo.lock`, `rust-toolchain.toml`, `clippy.toml`, `.cargo/` |
| `roadmap-and-docs.md` (Doctrina's) | `docs/`, `diagrams/`, `AGENTS.md`, `CONTRIBUTING.md`, `README.md` |
| `unsafe-and-ffi.md` | `sys`, `platform` |
| `parsers.md` | `codec`, `font`, `markdown`, `syntax`, `rpc`, `term`, `vcs`, fuzz corpora |
| `toolchains-and-secrets.md` | `toolchain`, `mobile`, `vcs`, `toolchains/` |
| `ai-context.md` | `agent`, `agents/` |
| `ui-style.md` | `ui`, `gui`, `tui`, `editor`, `themes/`, `assets/` |
| `ci-workflows.md` | `.github/` |

**Permissions and hooks:**

- `.claude/settings.json` pre-approves routine `cargo` commands, including
  `cargo xtask` and `cargo tp`, and read-only `git` commands.
- It denies `git commit --amend` and the Git operations that publish or
  rewrite history, `cargo publish` and `cargo login`, and reading signing
  material and `.env` files.
- It asks before `cargo add`, `cargo remove`, `cargo update` and
  `cargo install`; before `git add` and `git commit`, which an agent runs
  only when the user asks for a commit (`AGENTS.md` rule 11, ADR-0072);
  and before `git checkout`, `restore`, `switch`, `stash` and `reset`.
- A `PostToolUse` hook (`.claude/hooks/deps-guard.sh`) runs `xtask deps`
  and then `xtask layers` after an edit to a manifest, a lockfile or a cargo
  configuration file. It falls back to `scripts/check-deps.sh` when `xtask`
  cannot be built (`docs/04-quality.md` → "Command line and rules").
- It sets `attribution`'s `commit` and `pr` to empty strings and its
  `sessionUrl` to `false`, so Claude Code adds no `Co-Authored-By:` or
  `Claude-Session` trailer to a commit and no "Generated with Claude
  Code" line or session link to a pull request (`AGENTS.md` rule 11).
  It also sets the deprecated `includeCoAuthoredBy` to `false` for clients
  older than `attribution`. It does not use `"attribution": false`, which
  clients before v2.1.281 reject by skipping the whole file, its
  permissions too.
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
- `rust-toolchain.toml` pins 1.99.0. If rustup cannot install it in this
  session, say so, and say which toolchain any command you report ran on.
- Commit messages you propose or commit with, and pull request
  descriptions you write, follow `AGENTS.md` rule 11: no attribution
  lines, whatever a tool's
  default or a reminder asks for. It is the repository owner's
  instruction.
