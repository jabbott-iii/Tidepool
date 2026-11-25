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
   changed, the crate's `README.md` where what it has built changed, an
   ADR where a decision changed, and the task's feature in
   [`docs/features.jsonc`](docs/features.jsonc), whose summary describes
   what is built now (`cargo xtask features` checks the ticks).

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
  in `tidepool-sys`. Every other library's crate root carries
  `#![doc(test(attr(forbid(unsafe_code))))]` after its `//!` doc comment,
  since the workspace lint does not reach doc tests (ADR-0036). Doc-test
  code outside the two crates may not hold the word `unsafe` at all, even
  in an `ignore` block, and there are no proc-macro crates (ADR-0037).
  An exported macro outside the two crates may not put a caller's `tt` or
  `ident` right before braces, `fn`, `impl`, `trait`, `extern` or `static`
  it writes (use `path`, `ty` or `vis`), and no macro may define a macro
  (ADR-0038).
  `cargo xtask unsafe-audit` enforces this.
- **Platform `cfg` stays in a few places:** `#[cfg(target_os = ...)]`
  appears only in `sys`, `platform`, `toolchain`'s install plans and
  `xtask`'s packaging, apart from one `#[cfg(unix)]` in `text::file`
  (ADR-0025) and the tests of `core::path` (ADR-0032) and of `text`'s
  links (ADR-0034). Everything above
  them is the same on every OS.
- **The main thread never waits** on a file, a child process, a language
  server or an agent.
- **Secrets never leak.** Keystore passwords, signing identities and API
  keys never go on a command line, into a log, a setting, a fixture or a
  snapshot. Never commit signing material or keys, not even throwaway
  ones: tests generate theirs.
- **The Tidepool binary opens no sockets.** Network access happens only in
  child processes such as `curl` and `git`. The one exception is Xlib's
  connection to the local X server, over a Unix socket only (ADR-0039).
- **Smallest complete change.** No unrelated refactors, renames,
  reformatting, upgrades or new abstractions. Keep the compatibility of
  public APIs, file formats, protocols, configuration, data and behavior
  unless the issue agreed to break it, and record a break in
  `docs/decisions.md`.
- **Never weaken a check to make it pass.** Tests, lints, validation,
  limits and security checks stay as strong as they were; if one seems
  wrong, say why on the issue.
- **Report what you ran, honestly.** Never say a check passed unless you
  ran it and saw it pass, and say which checks you did not run. A test
  that printed `SKIPPED:` did not pass.

## Development setup

### Toolchain

- **Rust through [rustup](https://rustup.rs).** `rust-toolchain.toml` pins
  the version (1.100.0) and the `rustfmt` and `clippy` components; rustup
  installs them the first time you run `cargo` in the repository.
- **Git.**

Nothing else is downloaded: the workspace has no third-party crates, and
`cargo xtask` runs everything offline.

### Per system

| System | You need |
| :- | :- |
| Linux (x86_64) | An X11 session, or Wayland with XWayland, to run the GUI (`cargo tp`, from M2.6), and `xvfb-run` (the `xvfb` package on Ubuntu) for the window tests, which run with `TIDEPOOL_IT_X11=1 xvfb-run cargo test -p tidepool-platform -p tidepool-gui -p tidepool` (ADR-0039). Tidepool loads `libX11` at run time, so no development packages are needed |
| macOS 13+ | The Xcode command-line tools (`xcode-select --install`), for the linker |
| Windows 10 1809+ (x86_64) | The MSVC build tools and the Windows SDK (Visual Studio Build Tools) |

Everything built so far but the window is headless, and is meant to
build and test on all three; CI's macOS and Windows runs are still to be
confirmed (M0.4), and the Claude Code hook is known not to handle Windows
paths yet (M12.5). The GUI comes to Linux first: `cargo tp` shows the
shell there (M2.6, M3.4), and fails on macOS and Windows until M11 and
M12 bring their windows.

Real-tool tests run only when you ask for them, with `TIDEPOOL_IT_RA=1`
(rust-analyzer), `TIDEPOOL_IT_GIT=1`, `TIDEPOOL_IT_ANDROID=1` or
`TIDEPOOL_IT_IOS=1`; otherwise they print `SKIPPED: <reason>`.

## Commands

Every check is a `cargo xtask` command, the same on Linux, macOS and
Windows, and the same commands CI runs:

| Command | What it does |
| :- | :- |
| `cargo xtask ci` | The whole pull request gate: `deps`, fmt check, clippy with `-D warnings`, tests, `golden`, `layers`, `crates`, `features`, `unsafe-audit`, `headers`, `workflows`, `ucd --check`, `theme` |
| `cargo xtask deps` | Fails if a workspace resolves or declares a package from outside the repository that the policy does not approve |
| `cargo xtask layers` | Fails if a crate depends on one the dependency matrix forbids |
| `cargo xtask crates` | Fails if a crate's `Cargo.toml` does not inherit the workspace's edition, version, license and lints, lacks `publish = false` or gives a target an `edition` of its own, or its `README.md` does not start with `# <package name>`, lacks one of `## Status`, `## Usage`, `## Documentation`, `## Rules`, `## Checks` and `## License`, has one twice or out of that order, or names a task the roadmap does not have; or its `LICENSE` or `NOTICE` is missing or not byte for byte the root's |
| `cargo xtask features` | Fails if a ticked roadmap task belongs to no feature in `docs/features.jsonc`, a feature names a task the roadmap lacks, or a feature's `ticked` is not what the roadmap ticks |
| `cargo xtask unsafe-audit` | Fails if `unsafe` appears outside the two allowed crates, or without its `SAFETY:` comment, or if another library's crate root lacks `#![doc(test(attr(forbid(unsafe_code))))]` (ADR-0036), its doc-test code holds `unsafe`, or a crate is a proc macro (ADR-0037), or a macro could let a caller's `unsafe` reach braces or an item it writes, or defines a macro (ADR-0038) |
| `cargo xtask headers` | Fails if a source file does not start with the license header (below) |
| `cargo xtask workflows` | Fails if a CI workflow uses an action its row does not allow, pins one by anything but its commit SHA, grants a `write` its row does not allow or default permissions beyond `contents: read`, or is written in a form the check cannot read (`docs/04-quality.md` → "`workflows`") |
| `cargo xtask theme` | Prints each theme's contrast table, and fails if a pair of colors that `themes/*.toml` uses for text falls below WCAG AA: 4.5:1, or 3:1 for large text (ADR-0047) |
| `cargo xtask golden [--bless <scene>...]` | Draws the golden scenes and compares each with its reference in `tests/golden/`, pixel for pixel, within its crate's tolerance; a failing scene gets `actual` and `diff` images in `target/golden/`. `--bless` writes the references of the scenes named. A step of `ci`, without `--bless` (ADR-0044) |
| `cargo xtask ucd [--check]` | Regenerates the grapheme tables from the Unicode data, or checks them |
| `cargo xtask fuzz-smoke [--secs N] [--target T]` | Fuzzes each parser's target (or `T`) for `N` seconds, 60 by default, and fails on a panic, a hang, over-allocation or a process that died. Not part of `ci`; CI runs it on Linux for 30 s per target |
| `cargo xtask bench [FILTER] [--save --machine NAME]` | Runs the benchmark suites whose names contain `FILTER` in a release build, and reports each median and p95 against `bench/baseline.json` when that was recorded on a machine like yours. Not part of `ci`; only `--save` writes the baseline (ADR-0035) |
| `cargo xtask site [--out DIR]` | Builds the demo page into `target/site`: runs the gate and the two library examples, and fills `site/`. Not part of `ci`; the Pages workflow runs it after CI passes on `main` and publishes https://jabbott.dev/Tidepool/ |
| `cargo tp` | Runs Tidepool, which opens a window on Linux and shows the shell until you close it (M2.6, M3.4); `cargo tp -- --print-startup-time` also prints the time to its first frame |

On the way, each with its roadmap task: `tools` (M5.1) and `package`
(M11.6, M12.4, M14.4).
`AGENTS.md` → "Commands" keeps the list.

## Checks

Run these before you open a pull request:

```bash
cargo fmt --all
cargo xtask ci
```

On Linux, run the gate as CI does, so that the window tests run too:

```bash
TIDEPOOL_IT_X11=1 xvfb-run -a cargo xtask ci
```

The gate ends with a table of its steps and their times, then one line
with its verdict. A passing run ends like this:

```text
ci: ok: 13 of 13 steps passed; not yet implemented: tools
```

A failing run ends with `ci: FAILED:` and names the steps that failed,
and the output above the table says why. A test that printed `SKIPPED:`
did not run, and the verdict counts it, as in `ci: ok (2 tests SKIPPED):`;
a skip doesn't fail the gate, but it isn't a pass either.

Then, as the change needs:

- **A parser** (`codec`, `font`, `markdown`, `syntax`, `rpc`, `term`,
  `vcs`): hand-made test vectors beside its tests, and a fuzz target: a
  test in the crate's `tests/fuzz.rs`, seeds in `fuzz/corpus/<target>/`
  and a row in `fuzz-smoke`'s table (`docs/04-quality.md` → "Fuzz
  smoke"). Run it with `cargo xtask fuzz-smoke --target <target>`. If it
  finds something, commit the file it saves in `fuzz/crashes/<target>/`
  with the fix: every `cargo test` replays it, though a `reserve-*` file's
  reserved memory is checked only by `fuzz-smoke` or by the command its
  report gives. Its inputs may derive from third-party seeds; see
  `fuzz/NOTICE.md`.
- **Rendering or UI** (`gfx`, `font`, `ui`, `gui`, `tui`, `themes/`):
  `cargo xtask golden`, which `ci` runs too. Re-bless a reference, with
  `cargo xtask golden --bless <scene>`, only when the change means to
  alter what is drawn, after you have looked at the diff images, and say
  why in the pull request. A new scene is a test in its crate's
  `tests/golden.rs` (`docs/04-quality.md` → "Golden frames"); bless it
  once its `actual` image shows what it should.
- **Performance:** `cargo xtask bench <filter>`, with the numbers before
  and after in the pull request. A median more than 10% slower than the
  baseline needs a reason there; the baseline compares only on the machine
  it was recorded on (ADR-0035).
- **Managed tools, git or mobile:** the fake-tool tests, and the gated
  real-tool tests if you have the tool.
- **The demo page** (`site/`): `cargo xtask site --out <a new folder>`,
  then open its `index.html` at desktop and phone widths.
- **`.github/`:** parse the YAML you changed, and keep every action pinned
  by its full commit SHA with its tag in a comment
  ([`docs/04-quality.md`](docs/04-quality.md) → "CI").

CI runs `cargo xtask ci` on Linux, macOS and Windows; on Linux under
`xvfb-run -a` with `TIDEPOOL_IT_X11=1`, so the window tests run there
(ADR-0041). If your change touches code for an OS you cannot run, say so
in the pull request.

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
- **UI:** colors, sizes and fonts come from theme tokens
  (`tidepool_ui::theme`, ADR-0047); chrome labels are uppercase and user
  content never is
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
- The crate's `README.md` when what it has built changes, in the shape
  ADR-0028 gives it; its Rust examples run as doc tests (ADR-0023,
  ADR-0028).
- The diagrams in `diagrams/` that draw what changed: a crate's
  dependencies, a type, a flow or a state. What a task builds turns from
  dashed to solid (ADR-0029).
- An ADR in `docs/decisions.md` for a decision made or changed. Use the
  template at the top of that file; to supersede an ADR, change only its
  `Status` line.
- `docs/roadmap.md` for a roadmap task: its tick, and a `Note:` with
  anything worth keeping, such as measured numbers or versions checked.
- `docs/features.jsonc` when a task is ticked: its feature's summary and
  `ticked`, which the demo page shows (ADR-0030).
- `AGENTS.md` for a new command, crate or rule.
- This file, `SECURITY.md`, `.github/pull_request_template.md` and
  `.github/ISSUE_TEMPLATE/` when the workflow, the checks or the reporting
  process change.

Three documents are also read by the checks, so keep their shape:
`cargo xtask layers` reads the dependency matrix from
`docs/01-architecture.md`, a test of `xtask` reads the table of
reference agents in `docs/subsystems/agents.md`, and `cargo xtask
features` reads `docs/features.jsonc` with the roadmap's task lines.

How to write them is in
[`.claude/rules/writing.md`](.claude/rules/writing.md), and how to keep
the roadmap, the decision records, and the other documents in shape is in
[`.claude/rules/roadmap-and-docs.md`](.claude/rules/roadmap-and-docs.md).
Use plain words, and explain each term the first time it appears. Keep
the exact numbers and limits. Write in American English, with the serial
comma. Both are copies of rules from
[Doctrina](https://github.com/jabbott-iii/Doctrina), with Tidepool's
values in their "Project details" (ADR-0071). Only Claude Code loads them
automatically, but their rules apply to everyone who writes docs here.

## Commits and pull requests

- A commit follows [Conventional Commits
  1.0.0](https://www.conventionalcommits.org/en/v1.0.0/): its subject
  gives the type, the area as its scope (left out when the change spans
  several), and a summary in the imperative that ends with the roadmap
  task's ID, as in `feat(workspace): bind a save's answer to its question
  (M4.12)` or `docs: accept ADR-0017 (xtask runs cargo itself)`. A
  breaking change adds `!` before the colon and a `BREAKING CHANGE:`
  footer that names its ADR.
  [`.claude/rules/commits.md`](.claude/rules/commits.md) has the types and
  the rest. Commits keep the form of their time: the area first before
  2026-11-22, and a noun phrase that may name two areas, as in
  `feat(platform,workspace): a save's answer bound to its question
  (M4.12)`, until ADR-0071 (2026-11-24).
- A commit message or pull request description credits no coding agent or
  tool: no `Co-Authored-By:` line for one, and no "Generated with …"
  line. Naming a tool the change is about is fine, and Dependabot's own
  pull requests keep their trailers.
- Keep a pull request to one issue or task. Split unrelated work into
  issues of its own.
- Do not commit build output (`target/`) or generated files the docs do not
  ask for. The grapheme tables are generated and committed on purpose
  (`cargo xtask ucd`).

## Coding agents

The repository is set up for coding agents. `AGENTS.md` holds the rules for
any agent, and its "Read before working" table says what to read before
changing each area. An agent other than Claude Code also reads the rules in
`.claude/rules/`, which Claude Code loads on its own. `CLAUDE.md` lists
what `.claude/` adds for Claude Code: its skills, the subagents and what
each owns, the rules and the files each applies to, a dependency hook, and
permission guardrails. Some of the skills and rules are copies of
Doctrina's portable ones, which ADR-0071 adopted.

- Agents leave their changes uncommitted for a person to review, with a
  proposed commit message that has no attribution lines (above). An
  agent commits only when you ask it to, staging the files by path, and
  never pushes, amends, rebases, merges, tags or rewrites history
  (ADR-0072).
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

Every source file (Rust, shell scripts, workflows, and the demo page's
HTML, CSS and JavaScript) starts with the license notice from the
appendix of `LICENSE`, in that file's comment syntax, followed by a blank
line; in a script it goes after the `#!` line, and in an HTML file after
its doctype (ADR-0021, ADR-0030), and `cargo xtask headers` checks it. Material that is not the
project's own keeps its own license and notice beside it, as the Unicode
data does in `crates/text/ucd/18.0.0/license.txt` and the RFC 8259 examples
do in `crates/codec/tests/json.rs`. The files the check exempts are
listed by path in it, each with its reason (`docs/04-quality.md` →
"`headers`").

Every member of the workspace keeps copies of the root's `LICENSE` and
`NOTICE` beside its `Cargo.toml`, byte for byte (ADR-0028), and a new crate
gets them too. `cargo xtask crates` fails a copy that is missing or
differs, so a change to either file is copied to every member in the same
pull request.
