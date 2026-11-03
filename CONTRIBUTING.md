# Contributing to Tidepool

Thanks for your interest in Tidepool, an IDE written in Rust with no
third-party crates. This guide covers how to propose a change, set up a
machine, check your work and open a pull request.

The project's rules are in [`AGENTS.md`](AGENTS.md), which every
contributor follows, person or coding agent. Where this guide and
`AGENTS.md` disagree, `AGENTS.md` wins. The design is in [`docs/`](docs):
start with [`docs/00-overview.md`](docs/00-overview.md), then the
`README.md` and the subsystem spec of the crate you are changing (the table
"Read before working" in `AGENTS.md` says which).

## Contribution workflow

1. **Open an issue first,** with the issue form: a bug, a proposal, a
   question, or a roadmap task you would like to take. A pull request with
   no issue will be declined, unless it fixes a typo or a broken link.
2. **If an issue already exists,** comment on it instead of opening
   another.
3. **Wait for a maintainer to confirm** the change on the issue before you
   open a pull request.
4. **Branch from `main` and make the change,** keeping to the one issue.
5. **Check your work** (see [Checks](#checks)), then open a pull request
   that links the issue. The template asks what you ran.
6. **CI must pass** on Linux, macOS and Windows. A maintainer reviews every
   pull request (see `CODEOWNERS`).

### Roadmap tasks

Most work is a task in [`docs/roadmap.md`](docs/roadmap.md). Each task
lists the tasks it depends on (`Depends`), what to do (`Do`), what must be
true when it is done (`Accept`) and how to check it (`Verify`). Some have a
`Decide first` line: a question the maintainer answers, on the issue,
before any code is written. Take the first unticked task whose
dependencies are all ticked, unless the issue agreed on another. A task is
done when:

1. every acceptance criterion is met, and its `Decide first` questions are
   answered and recorded in [`docs/decisions.md`](docs/decisions.md);
2. its `Verify` commands ran and passed, and the pull request reports each
   with its real result;
3. its checkbox is ticked, and only then;
4. the docs are up to date: the subsystem spec where behavior or design
   changed, the crate's `README.md` where what it has built changed, and an
   ADR where a decision changed.

A criterion that needs an OS, a tool or a device you do not have stays
unticked, with a `⏳ needs <OS or tool>: <the remaining check>` line under
the task.

Task IDs never change, so a commit or an issue can cite one safely. A task
added later takes the next free number in its milestone.

## Ground rules

These are the rules in `AGENTS.md` that a pull request is most often held
up on:

- **No third-party crates.** Do not add a crate from outside this workspace
  to any dependency table. If you think one is needed, say so on the issue,
  with the evaluation that
  [`docs/02-dependency-policy.md`](docs/02-dependency-policy.md) describes.
  `cargo xtask deps` enforces this.
- **Outside programs are managed tools.** Only `tidepool-toolchain` finds,
  installs or starts another program, with an argv vector and never a
  shell. Installs ask the user first, check a pinned SHA-256, and never
  elevate silently. Tidepool never bundles a tool's binary.
- **Layering is one-way.** A crate may depend only on the crates its row of
  the dependency matrix in
  [`docs/01-architecture.md`](docs/01-architecture.md) lists.
  `cargo xtask layers` enforces this.
- **`unsafe` lives in `tidepool-sys` and `tidepool-platform` only.** Each
  `unsafe` block has a `// SAFETY:` comment saying why it is sound, each
  `pub unsafe fn` a `# Safety` section, and raw OS declarations live only
  in `tidepool-sys`. `cargo xtask unsafe-audit` enforces this.
- **Platform `cfg` stays in a few places:** `#[cfg(target_os = ...)]`
  appears only in `sys`, `platform`, `toolchain`'s install plans and
  `xtask`'s packaging. Everything above them is the same on every OS.
- **The main thread never waits** on a file, a child process, a language
  server or an agent.
- **Secrets never leak.** Keystore passwords, signing identities and API
  keys never go on a command line, into a log, a setting, a fixture or a
  snapshot. Never commit signing material or keys, not even throwaway
  ones: tests generate theirs.
- **The Tidepool binary opens no sockets.** Network access happens only in
  child processes such as `curl` and `git`.
- **Smallest complete change.** No unrelated refactors, renames or
  reformatting. Keep public APIs, file formats and protocols unless the
  issue agreed to break one, and record a break in `docs/decisions.md`.
- **Report what you ran, honestly.** Never say a check passed unless you
  ran it and saw it pass, and say which checks you did not run. A test
  that printed `SKIPPED:` did not pass.

## Development setup

### Toolchain

- **Rust through [rustup](https://rustup.rs).** `rust-toolchain.toml` pins
  the version (1.99.0) and the `rustfmt` and `clippy` components; rustup
  installs them the first time you run `cargo` in the repository.
- **Git.**

Nothing else is downloaded: the workspace has no third-party crates, and
`cargo xtask` runs everything offline.

### Per system

| System | You need |
| :- | :- |
| Linux (x86_64) | Nothing more today. From M2.2, an X11 session or Wayland with XWayland to run the GUI, and `xvfb-run` (the `xvfb` package on Ubuntu) for the window tests. Tidepool loads `libX11` at run time, so no development packages are needed |
| macOS 13+ | The Xcode command-line tools (`xcode-select --install`), for the linker |
| Windows 10 1809+ (x86_64) | The MSVC build tools and the Windows SDK (Visual Studio Build Tools) |

Everything built so far is headless and is meant to build and test on all
three; CI's macOS and Windows runs are still to be confirmed (M0.4), and
the Claude Code hook is known not to handle Windows paths yet (M12.5). The
GUI comes to Linux first (M2), then macOS (M11) and Windows (M12).

Real-tool tests run only when you ask for them, with `TIDEPOOL_IT_RA=1`
(rust-analyzer), `TIDEPOOL_IT_GIT=1`, `TIDEPOOL_IT_ANDROID=1` or
`TIDEPOOL_IT_IOS=1`; otherwise they print `SKIPPED: <reason>`.

## Commands

Every check is a `cargo xtask` command, the same on Linux, macOS and
Windows, and the same commands CI runs:

| Command | What it does |
| :- | :- |
| `cargo xtask ci` | The whole pull request gate: `deps`, fmt check, clippy with `-D warnings`, tests, `layers`, `unsafe-audit`, `headers`, `workflows`, `ucd --check` |
| `cargo xtask deps` | Fails if a workspace resolves or declares a package from outside the repository that the policy does not approve |
| `cargo xtask layers` | Fails if a crate depends on one the dependency matrix forbids |
| `cargo xtask unsafe-audit` | Fails if `unsafe` appears outside the two allowed crates, or without its `SAFETY:` comment |
| `cargo xtask headers` | Fails if a source file does not start with the license header (below) |
| `cargo xtask workflows` | Fails if a CI workflow uses an action its row does not allow, pins one by anything but its commit SHA, grants a `write` its row does not allow or default permissions beyond `contents: read`, or is written in a form the check cannot read (`docs/04-quality.md` → "`workflows`") |
| `cargo xtask ucd [--check]` | Regenerates the grapheme tables from the Unicode data, or checks them |
| `cargo tp` | Runs Tidepool, which does nothing yet (M2.6 opens the first window) |

On the way, each with its roadmap task: `fuzz-smoke` (M1.6), `bench`
(M1.9), `golden` (M2.4), `theme` (M3.2), `tools` (M5.1), `package` (M11.6,
M12.4, M14.4), and the check `crates` (M0.9). `AGENTS.md` → "Commands" keeps the list.

## Checks

Run these before you open a pull request:

```bash
cargo fmt --all
cargo xtask ci
```

Then, as the change needs:

- **A parser** (`codec`, `font`, `markdown`, `syntax`, `rpc`, `term`,
  `vcs`): hand-made test vectors beside its tests, and from M1.6 a fuzz
  target with seeds in `fuzz/corpus/<target>/`, run with
  `cargo xtask fuzz-smoke --target <target>`.
- **Rendering or UI** (`gfx`, `font`, `ui`, `gui`, `tui`, `themes/`): from
  M2.4, `cargo xtask golden`. Re-bless a reference only when the change
  means to alter what is drawn, after you have looked at the diff images,
  and say why in the pull request.
- **Performance:** from M1.9, `cargo xtask bench <filter>`, with the numbers
  before and after in the pull request.
- **Managed tools, git or mobile:** the fake-tool tests, and the gated
  real-tool tests if you have the tool.
- **`.github/`:** parse the YAML you changed, and keep every action pinned
  by its full commit SHA with its tag in a comment
  ([`docs/04-quality.md`](docs/04-quality.md) → "CI").

CI runs `cargo xtask ci` on Linux, macOS and Windows. If your change touches
code for an OS you cannot run, say so in the pull request.

## Coding conventions

- **Edition and toolchain:** Rust 2024 on the pinned stable toolchain.
  `extern` blocks are written `unsafe extern "C" { ... }`.
- **Errors:** each crate has its own error enum implementing `Display` and
  `std::error::Error`. No `unwrap()` in library code; use `expect("why
  this cannot fail")` only for a real invariant.
- **Logging:** the `tidepool_core::log` macros, never `print!` or `eprint!`
  in a library crate. Command lines are logged through the redacting
  helper.
- **Text positions** are byte offsets, lines end at LF, CR LF or a lone CR,
  and a bad position is an error, never a panic (ADR-0019).
- **Docs:** every `pub` item has a doc comment.
- **Lints:** never weaken `[workspace.lints]`, `clippy.toml` or
  `unsafe_code`. An `#[allow(...)]` needs a comment saying why the lint is
  wrong there.
- **Parsers:** never panic on input, check every size against `Limits`
  before allocating, bound recursion, and give errors with the offset or
  line. A change to a format Tidepool writes bumps its version. See
  [`.claude/rules/parsers.md`](.claude/rules/parsers.md).
- **OS bindings:** declared in `tidepool-sys` with layout assertions and a
  citation of the official header, and wrapped safely in
  `tidepool-platform`. See
  [`.claude/rules/unsafe-and-ffi.md`](.claude/rules/unsafe-and-ffi.md).
- **UI:** colors, sizes and fonts come from theme tokens; chrome labels are
  uppercase and user content never is
  ([`docs/subsystems/ui-and-theme.md`](docs/subsystems/ui-and-theme.md)).

## Tests

- Unit tests live in a `#[cfg(test)] mod tests` beside the code, and
  integration tests in each crate's `tests/`.
- Add or update tests for every change in behavior. They must be
  deterministic: property tests use `tidepool_core::testing`, which prints
  the seed of a failure (`TIDEPOOL_SEED=0x…`).
- Never delete or `#[ignore]` a test, raise a tolerance, or re-bless a
  golden image to make a check pass. If a test is wrong, say why in the
  pull request.
- Committed test data must not look like the real thing to the checks: no
  file named `Cargo.toml`, `Cargo.lock` or `*.rs` in fixtures or corpora
  ([`docs/04-quality.md`](docs/04-quality.md) → "Files searched").

## Documentation

Update the docs in the same pull request when the change affects them:

- The subsystem spec in `docs/subsystems/` for a change in behavior or
  design.
- The crate's `README.md` when what it has built changes (ADR-0023).
- An ADR in `docs/decisions.md` for a decision made or changed. Use the
  template at the top of that file; to supersede an ADR, change only its
  `Status` line.
- `docs/roadmap.md` for a roadmap task: its tick, and a `Note:` with
  anything worth keeping, such as measured numbers or versions checked.
- `AGENTS.md` for a new command, crate or rule.
- This file, `SECURITY.md`, `.github/pull_request_template.md` and
  `.github/ISSUE_TEMPLATE/` when the workflow, the checks or the reporting
  process change.

Two documents are also read by the checks, so keep their shape:
`cargo xtask layers` reads the dependency matrix from
`docs/01-architecture.md`, and a test of `xtask` reads the table of
reference agents in `docs/subsystems/agents.md`.

## Commits and pull requests

- A commit's subject starts with the area it changes and names the roadmap
  task, as in `text: rope, positions and UAX #29 graphemes (M1.1)` or
  `docs: accept ADR-0017 (xtask runs cargo itself)`.
- Keep a pull request to one issue or task. Split unrelated work into
  issues of its own.
- Do not commit build output (`target/`) or generated files the docs do not
  ask for. The grapheme tables are generated and committed on purpose
  (`cargo xtask ucd`).

## Coding agents

The repository is set up for coding agents: `AGENTS.md` holds the rules for
any agent, and `CLAUDE.md` and `.claude/` add Claude Code's skills
(`/next-task`, `/verify`, `/record-decision`, `/evaluate-dependency`,
`/add-ffi-binding`, `/add-managed-tool`), subagents, path-scoped rules, a
dependency hook and permission guardrails.

- Agents never commit, push or rewrite history: they leave their changes
  uncommitted for a person to review, with a proposed commit message.
- You are responsible for every line you submit, whoever or whatever wrote
  it. Review an agent's change as you would anyone's, and run the checks
  yourself.

## Security issues

Do not report a vulnerability in an issue or pull request. Report it
privately, as [`SECURITY.md`](SECURITY.md) describes; that file also says
what is in scope.

## License

By contributing, you agree that your contributions are licensed under the
Apache License 2.0 (see [`LICENSE`](LICENSE), section 5).

Every source file (Rust, shell scripts, workflows) starts with the
license notice from the appendix of `LICENSE`, in that file's comment
syntax, followed by a blank line; in a script it goes after the `#!` line
(ADR-0021), and `cargo xtask headers` checks it. Material that is not the
project's own keeps its own license and notice beside it, as the Unicode
data does in `crates/text/ucd/17.0.0/license.txt` and the RFC 8259 examples
do in `crates/codec/tests/json.rs`. The files the check exempts are
listed by path in it, each with its reason (`docs/04-quality.md` →
"`headers`").
