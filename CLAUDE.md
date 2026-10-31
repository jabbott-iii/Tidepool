# Tidepool — Claude Code project instructions

Tidepool is a zero-dependency IDE written in Rust. Initial focus: Rust and Markdown. Later:
Android (`.apk`/`.aab`) and iOS (`.app`/`.xcarchive`/`.ipa`) build and signing, AI integration
through pluggable language agents, and a native GUI.

- Plan and milestones: `docs/PLAN.md` (read the relevant milestone before starting work)
- Dependency policy and ledger: `docs/DEPENDENCIES.md`
- Decision records: `docs/adr/` (template: `docs/adr/0000-template.md`)

## Hard rules

1. **No third-party crates.** Only Rust `std`/`core`/`alloc`. Never run `cargo add`, never add a
   `[dependencies]`, `[dev-dependencies]`, `[build-dependencies]`, or `[workspace.dependencies]`
   entry that is not a `path` dependency inside this repo. If something seems to need a crate,
   stop and propose an ADR instead. `sh scripts/check-deps.sh` must pass.
2. **`unsafe` only in `crates/tp-sys`.** Every other crate has `#![forbid(unsafe_code)]`. Every
   `unsafe` block gets a `// SAFETY:` comment stating the invariants it relies on.
3. **OS bindings are hand-written** in `tp-sys`. No `-sys` crates, no `bindgen`, no `build.rs`
   that compiles C. New linked libraries and new external programs get a row in
   `docs/DEPENDENCIES.md` in the same change.
4. **External programs** (cargo, rust-analyzer, git, Android SDK, JDK, Xcode, ssh, curl) are run
   with `std::process::Command` and an argv vector. Never through a shell, never by building a
   command string. Missing tools produce a clear diagnostic, not a panic.
5. **Secrets** (keystore passwords, signing identities, API keys) never go on argv, into logs,
   settings files, test snapshots, error messages, or AI agent context. Never read or print the
   contents of keystores, `.p12`/`.p8`/`.pem`/`.key` files, `.env` files, or provisioning profiles.
6. **No in-house cryptography for signing.** Signing is delegated to `apksigner`, `jarsigner`,
   and `codesign`. Proposing otherwise requires an ADR.
7. **The core binary opens no network sockets** in v1. Network access belongs in external
   processes (agents, git, ssh).
8. **Do not weaken checks** to make something pass: no `#[allow]` to silence a real warning, no
   `#[ignore]`d or deleted tests, no skipped verification. Fix the cause or report it.
9. **Git:** do not commit, amend, rebase, reset, tag, merge, push, or otherwise change history.
   Leave all changes uncommitted for the owner to review. At the end of a unit of work, propose
   a commit message (Conventional Commits style) in your reply.

## Commands

The Cargo workspace is created in milestone M0. Until then there is nothing to build.

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
sh scripts/check-deps.sh        # dependency policy; fails closed
sh scripts/ci.sh                # all of the above (created in M0)
```

Real-toolchain integration tests are opt-in: `TIDEPOOL_IT_ANDROID=1`, `TIDEPOOL_IT_IOS=1`,
`TIDEPOOL_IT_RA=1` (rust-analyzer). When a gated test cannot run it must print
`SKIPPED: <reason>`; never report a skipped test as passed.

## Architecture in one screen

- One UI thread owns editor state; worker threads do I/O; communication via `std::sync::mpsc`.
  The UI thread never blocks on I/O, subprocesses, LSP servers, or agents. No async runtime.
- Frontends (`tp-tui` first, `tp-gui` later) render the same `tp-ui` draw model.
- LSP, DAP, and the Tidepool Agent Protocol (TAP) share one JSON-RPC transport in `tp-rpc`.
- Crates are created when their milestone starts. See the crate table in `docs/PLAN.md` §4.3.

## Conventions

- Rust edition 2024. Library crates return `Result` with crate-specific error enums; no
  `unwrap()`/`expect()` on data that comes from files, processes, or the network.
- Parsers of untrusted input (every file format, every protocol) enforce depth/size limits and
  never panic on malformed input. Add a fuzz target for every new parser.
- Tests use only `std` and `crates/tp-testkit` (seeded PRNG, golden files, fakes). Property tests
  print the failing seed.
- Public items get doc comments. Keep modules small; prefer plain functions and data over traits
  until a second implementation exists.
- Prefer the smallest complete change. No unrelated refactors, renames, or reformatting.

## How to work here

1. Read the relevant part of `docs/PLAN.md` and the code you are about to change before editing.
2. For non-trivial design choices, use the `architect` agent and record decisions with `/adr`.
   Only the owner marks an ADR as Accepted.
3. Implement with the agent that owns the area (see table below). Keep changes inside the
   current milestone's scope.
4. Use `security-reviewer` for any change touching `tp-sys`, subprocess spawning, signing,
   secrets, parsers of untrusted input, workspace trust, or AI context assembly.
5. Run `/verify`. Report exactly which commands ran and their results; say what was not run.
6. Tick the checklist item in `docs/PLAN.md` only when its acceptance evidence exists.
7. Finish with: summary, files changed, validation run, checks not run, assumptions, open
   issues, residual risks, and a proposed commit message.

| Agent | Owns |
| --- | --- |
| `architect` | Design, ADRs, crate boundaries, protocol specs, dependency decisions (writes only in `docs/`) |
| `editor-core` | `tp-base`, `tp-text`, `tp-formats`, `tp-workspace`, `tp-regex` |
| `platform-ui` | `tp-sys`, `tp-ui`, `tp-tui`, `tp-term`, `tp-gfx`, `tp-gui` |
| `language-support` | `tp-syntax`, `tp-markdown`, `tp-rpc`, language packs |
| `mobile-toolchain` | `tp-proc` toolchain discovery, `tp-archive`, `tp-mobile` |
| `ai-agents` | TAP, `tp-agent`, predictive text, `agents/` |
| `test-engineer` | `tp-testkit`, test suites, fuzz/bench harnesses, `scripts/ci.sh` |
| `security-reviewer` | Read-only security review |
