# Roadmap

How to use this file:

- Work top to bottom. A task is **unblocked** when everything in its
  `Depends` line is checked.
- Tick a box only after its acceptance criteria are met **and** its
  verification commands were run and passed (see "Definition of done" in
  `AGENTS.md`).
- From M0.14, ticking a box also brings the task's feature in
  `docs/features.jsonc` up to date: its summary, and its `ticked` list.
  `cargo xtask features` fails until it is (ADR-0030).
- If a criterion needs an OS, tool or device you can't run, leave the box
  unchecked. Add an indented note saying exactly what remains:
  `  - ⏳ needs macOS: run the iOS sample through codesign --verify`.
- A `Decide first` line is a question for the user. Ask it before writing
  code for the task, and record the answer with `/record-decision`: accept
  the `Proposed` ADR it names, or add or extend one.
- **Task IDs never change.** Code, docs and commit messages cite them. A
  task added later takes the next free number in its milestone and is
  listed where it is to be done, so numbers are not always in order. A
  split task keeps its ID on the part that finishes what it promised, so
  every `Depends` line that names it stays true.
- Exit criteria for a milestone are checked by a human.
- OS order: **Linux → macOS → Windows** (ADR-0015).
- The roadmap was re-planned on 2026-11-02 from the state of the code
  (ADR-0024): larger tasks were split, missing work was added as tasks, and
  questions that finished tasks left open became `Decide first` lines.

---

## M0 — Foundation and guardrails

**Goal:** an empty but fully guarded workspace. The constraints are
enforced by tooling from day one.

- [x] **M0.1 — Workspace skeleton**
  - Depends: —
  - Do:
    - Create every crate in the workspace map (`AGENTS.md`) as an empty
      library or binary with a crate-level doc comment.
    - Root `Cargo.toml`: `[workspace]` (resolver 3), `[workspace.package]`
      (edition 2024, version 0.1.0, license Apache-2.0), and
      `[workspace.lints]` exactly as in `docs/01-architecture.md`.
    - `rust-toolchain.toml` pinning the current stable with `rustfmt` and
      `clippy`; record the version in a task note.
    - `clippy.toml`, `.cargo/config.toml` with the aliases.
    - `#![allow(unsafe_code)]` in the `sys` and `platform` roots only.
  - Accept:
    - `cargo build --workspace` and
      `cargo clippy --workspace --all-targets -- -D warnings` succeed.
    - `Cargo.lock` lists only workspace packages.
  - Verify: `cargo build --workspace`;
    `cargo clippy --workspace --all-targets -- -D warnings`;
    `sh scripts/check-deps.sh`
  - Note: toolchain pinned to 1.99.0 (rustc 1.99.0 b940084d7 2026-09-28;
    clippy 0.1.99; rustfmt 1.10.0-stable).

- [x] **M0.2 — core: errors, logging, test harness, hashing, CLI, time**
  - Depends: M0.1
  - Do: `error`, `log` (with redaction), `testing::{check, check_sized}`,
    `rng::Pcg32`, `hash::{fnv1a64, crc32, sha256}`, `cli`, `civil`, `geom`,
    `color`, `input`, `cancel`, per `docs/subsystems/core.md`. Port from
    Nadir's `nadir-core` where it fits.
  - Accept:
    - SHA-256 matches the FIPS 180-4 vectors; `crc32("123456789") ==
      0xCBF43926`.
    - A failing `check` prints a reproducible seed.
    - A registered secret never appears in any log sink.
  - Verify: `cargo test -p tidepool-core`

- [x] **M0.3 — xtask: deps, layers, unsafe-audit, ci**
  - Depends: M0.2, M1.4
  - Do:
    - `cargo xtask deps` reads `cargo metadata` with `codec::json`; fails on
      any non-null `source` not in `xtask/deps-allowlist.txt` (empty).
    - `cargo xtask layers` checks the matrix in `docs/01-architecture.md`
      (parse the table from the doc, so the doc is the source of truth).
    - `cargo xtask unsafe-audit` per `docs/04-quality.md`.
    - `cargo xtask ci` runs fmt check, clippy, tests and the checks above.
    - Switch `.claude/hooks/deps-guard.sh` to `cargo xtask deps`; keep
      `scripts/check-deps.sh` as a fallback when `xtask` doesn't build.
  - Accept: each check fails on a deliberately broken fixture workspace
    generated in a temp dir, and passes on the real workspace.
  - Verify: `cargo xtask ci`
  - Note (2026-11-01): `cargo xtask ci` passes locally: 6 of 6 steps, 566
    tests, 0 failed, 0 `SKIPPED:`. It also passes with the CI workflow's
    environment (`CARGO_TERM_COLOR=always`). The xtask suite passes with
    `CARGO_TARGET_DIR` outside the repository. Each check's fixtures are in
    `xtask/tests/fixtures.rs`, generated under `CARGO_TARGET_TMPDIR`; the
    rules are in `docs/04-quality.md` → "`xtask` checks".
    - The hook runs `xtask deps` and then `xtask layers` (layers goes beyond
      the task text). It builds `xtask` into `target/deps-guard/` and
      falls back to `scripts/check-deps.sh`.
    - `check-deps.sh` now resolves with `--locked`, so it fails on an out
      of date or missing lockfile instead of writing one.
    - Running cargo from `xtask` is ADR-0017, accepted on 2026-11-01 and
      named as the exception in `AGENTS.md` rule 2.

- [x] **M0.4 — CI workflow**
  - Depends: M0.3
  - Do: `.github/workflows/ci.yml` per `docs/04-quality.md`: matrix of
    3 OSes, `permissions: contents: read`, actions pinned by full SHA with
    the tag in a comment, cache keyed on lockfile + toolchain, runs
    `cargo xtask ci`.
  - Accept: valid YAML; no unpinned actions. If SHAs can't be looked up in
    this session, use `# TODO(pin): actions/checkout@vX` placeholders and
    list them for a human.
  - Verify: parse the YAML (state how)
  - Note (2026-11-01): parsed with PyYAML 6.0.1 (`python3 -I`,
    `yaml.safe_load`). Both actions are pinned by full SHA, checked with
    `git ls-remote` on 2026-11-01: `actions/checkout` v7.0.1 =
    `3d3c42e5aac5ba805825da76410c181273ba90b1`, `actions/cache` v6.1.0 =
    `55cc8345863c7cc4c66a329aec7e433d2d1c52a9`, both lightweight tags. The
    Linux fuzz-smoke step waits for M1.6 (a `TODO(M1.6)` in `ci.yml`).
  - ⏳ needs human: push, and confirm `cargo xtask ci` is green on
    `ubuntu-latest`, `macos-latest` and `windows-latest` (the M0 exit
    criterion). Nothing ran on macOS or Windows in this session.
  - Note (2026-11-08): at `adb0d3b`, Linux and macOS pass; Windows has
    failed since M1.3, which these fix (none run on Windows yet):
    - The checkout stopped at `fuzz/corpus/encoding/nul.bin`: Windows
      reserves `NUL` and the other device names whatever the extension.
      Renamed `nul-bytes.bin`, the same bytes; no other tracked path is
      a reserved name, ends in a dot or space, or differs only in case.
    - Before that, `coalescing_needs_the_kind_the_window_and_unmoved_cursors`
      failed: on Windows `Instant::checked_duration_since` counts an
      instant up to one tick of the performance counter later as no time
      (std 1.99.0, `sys/time/windows.rs`), so a commit 1 ns before the
      last one joined it. `History` now compares the instants, as its
      docs say ("not after it").
    - `cargo test` stops at its first failing binary, so the tests after
      that one had never run on Windows. A review of them found two that
      compared a path written with `/` to one Windows writes with `\`, in
      `core`'s fuzz tests; both now build the path as the engine does.
      `cargo clippy --workspace --all-targets --target
      x86_64-pc-windows-msvc -- -D warnings` passes.
    - At `d786f91`, Windows ran every test binary: all passed but
      `fixture_toml_is_refused_with_the_case_path_as_given_or_resolved`.
      Its link to a directory is a file link on Windows (`fs::soft_link`,
      as `xtask` has no platform `cfg`), which does not resolve ("Access is
      denied"), so the test now prints `SKIPPED:` there, as it does when no
      link can be made. The doc tests, which run last, have not run on
      Windows yet; a review of all 25 found nothing Windows-specific.

- [x] **M0.5 — Budgets and reference machine**
  - Depends: M0.1
  - Do: ask the user for the reference machine; record it and confirm or
    adjust every budget in `docs/00-overview.md`.
  - Accept: the budgets table says "confirmed" with the machine named.
  - Verify: doc review (human)
  - Note (2026-11-08): done. The user reviewed `docs/00-overview.md` →
    "Performance budgets" and approved the reference machine and every
    "Target (confirmed)" as written. The machine entry was checked against
    the development machine that day: `/sys/class/dmi/id` (MSI Pulse GL66
    11UGKV), `lscpu` (i7-11800H, 8 cores / 16 threads), `free` (15.3 GiB),
    `lsblk` (the root file system on the NVMe drive), `/etc/os-release`
    and `uname -r` (Pop!_OS 24.04 LTS, `7.2.6-76070206-generic`), `lspci`
    (Intel UHD and RTX 3070 Mobile), the panel's mode (1920×1080) and
    `XDG_CURRENT_DESKTOP` (COSMIC, on Wayland).

- [x] **M0.6 — Contribution workflow and crate READMEs**
  - Depends: M0.3
  - Do:
    - `CONTRIBUTING.md`: the workflow (an issue first), setup, commands,
      checks, conventions, docs, commits, coding agents, license.
    - `SECURITY.md` in place of GitHub's template: supported versions,
      private reporting, what to expect, scope.
    - `.github/pull_request_template.md`, and the issue form
      `.github/ISSUE_TEMPLATE/issue.yml` with a `config.yml` that turns
      blank issues off and links private vulnerability reporting.
    - A `README.md` in every workspace member, in the shape of ADR-0023.
  - Accept:
    - Both issue-template files parse as YAML, and the form has the
      top-level keys GitHub's issue forms require (`name`, `description`,
      `body`).
    - Every member has a README, and its status agrees with the ticks in
      this file.
    - Private vulnerability reporting is turned on in the repository's
      security settings, so the links in `SECURITY.md` and `config.yml`
      work (human).
    - The user has reviewed the texts (human).
  - Verify: parse the YAML (state how); doc review (human)
  - Note (2026-11-02): the files were written in the documentation rebuild
    of 2026-11-02 (ADR-0024).
  - Note (2026-11-08): done. The user reviewed and approved the texts as
    written (`CONTRIBUTING.md`, `SECURITY.md`, the pull request template,
    `issue.yml`, `config.yml` and the 26 members' READMEs), and accepted
    ADR-0023 and ADR-0024 as written.
    - **YAML:** `issue.yml` and `config.yml` parse with PyYAML 6.0.1
      (`python3 -I`, `yaml.safe_load`). `issue.yml` has `name`,
      `description` and `body`; `config.yml` sets `blank_issues_enabled:
      false` and links private vulnerability reporting.
    - **Private vulnerability reporting** is on: `gh api
      repos/jabbott-iii/Tidepool/private-vulnerability-reporting` returned
      `{"enabled":true}`.
    - **READMEs:** a script checked that every member has a `README.md`
      whose first line is `# <package name>`, and that each `## Status`
      names under "Built" only ticked tasks and under "Not yet" only
      unticked ones, all of them in this file. M0.9's `cargo xtask crates`
      makes the shape a check.

- [x] **M0.7 — License headers: `cargo xtask headers`; the RFC 8259 notice**
  - Depends: M0.3
  - Decide first:
    - accept ADR-0021 (license headers), which is `Proposed`;
    - whether to add a root `NOTICE` file, as Nadir has (ADR-0021 leaves
      it open).
  - Do:
    - Put ADR-0021's header on every source file it covers: today the Rust
      files, `scripts/check-deps.sh`, `.claude/hooks/deps-guard.sh` and the
      two workflows. The generated grapheme tables
      (`crates/text/src/pos/grapheme/tables.rs`) keep the Unicode
      license's notice instead and are exempt.
    - Put the IETF Trust's legend for the RFC 8259 examples beside them, in
      `crates/codec/tests/json.rs` (ADR-0021).
    - `cargo xtask headers`, run by `ci` after `unsafe-audit`, per
      `docs/04-quality.md`; a unit test compares its text with `LICENSE`'s
      appendix.
    - The comments above `#![allow(unsafe_code)]` in `crates/sys/src/lib.rs`
      and `crates/platform/src/lib.rs` still say `unsafe-audit` "(M0.3)
      will enforce" the rule; say that it does, while those lines move for
      the header.
  - Accept:
    - Fixture repositories fail for a missing header, an altered line, a
      header below other text and a missing blank line after it; a header
      after a `#!` line passes.
    - The real repository passes, and `cargo xtask ucd --check` still
      passes.
  - Verify: `cargo xtask headers`; `cargo xtask ci`
  - Note (2026-11-02): both `Decide first` questions are answered. The user
    accepted ADR-0021, and chose a root `NOTICE` (the project's name and
    copyright line), which ships beside `LICENSE` in each package
    (`docs/05-release.md`).
    - **One workflow, not two:** `.github/workflows/codeql.yml` is still
      GitHub's starter workflow word for word, so the user chose to exempt
      it by path until M0.8 rewrites it, rather than give it the header
      now (ADR-0021's Context; M0.8 has the step that ends the exemption).
      91 files got the header: the Rust files, both scripts and `ci.yml`.
    - **Verified:** `cargo xtask headers` passes (92 files checked, with
      `headers.rs` itself; the two exempt files noted), and `cargo xtask
      ci` passes: 8 of 8 steps, 747 tests, 0 failed, 0 `SKIPPED:`, with
      `ucd` (`--check`) ok. The fixtures are
      `headers_fails_on_each_kind_of_violation` and
      `headers_passes_on_the_unbroken_fixture_and_after_a_shebang` in
      `xtask/tests/fixtures.rs`; the real repository is checked by
      `the_real_repository_passes_deps_layers_unsafe_audit_and_headers`.
    - **Beyond the task text:** a leading byte-order mark is skipped; only
      files directly in `.github/workflows/` are workflows; and every
      symbolic link the walk reports fails, without resolving it again.
      The walk's own gaps (races, control characters in file names, a
      link resolved to tell a directory) are listed under "Files searched"
      in `docs/04-quality.md`.
    - Nothing ran on macOS or Windows (CR LF checkouts, case-insensitive
      file systems, links on Windows).
  - Note (2026-11-02, after the commit): of the walk's gaps named above,
    control characters in file names and links resolved to tell a
    directory are taken up by M0.10, which also changes the link rule:
    every symbolic link now fails, and none is resolved by `xtask`.

- [x] **M0.8 — CI supply chain: CodeQL pinned, Dependabot for actions, `cargo xtask workflows`**
  - Depends: M0.4
  - Decide first: accept ADR-0022 (CodeQL, Dependabot and the workflow
    check), which is `Proposed`.
  - Do:
    - Rewrite `.github/workflows/codeql.yml` per ADR-0022: `actions/checkout`
      and `github/codeql-action/init` and `analyze` pinned by full commit
      SHA with the tag in a comment; top-level `permissions: {}`; the job
      gets `contents: read` and `security-events: write` only;
      `persist-credentials: false`; languages `rust` and `actions` with no
      build; the `security-extended` queries; `ci.yml`'s concurrency rule.
    - Replace `.github/dependabot.yml`, whose empty `package-ecosystem`
      updates nothing, with the weekly `github-actions` entry of ADR-0022.
    - `cargo xtask workflows`, run by `ci`, per `docs/04-quality.md` and
      its table of the actions each workflow may use.
    - Look up each SHA with `git ls-remote` and record it in a task note.
    - Give the rewritten `codeql.yml` ADR-0021's header, and remove its
      entry from `EXEMPT` in `xtask/src/headers.rs` and from the docs that
      name the exemption (`docs/04-quality.md` → "`headers`", `AGENTS.md` →
      "License header"). M0.7 exempted it while it was GitHub's template.
  - Accept:
    - Against a fixture copy of the `codeql.yml` committed on 2026-11-01
      (GitHub's template), the check reports each of its five problems:
      three actions pinned by tag, no top-level `permissions`, and a
      checkout without `persist-credentials: false`.
    - The rewritten workflows pass, and `dependabot.yml` parses.
  - Verify: `cargo xtask workflows`; `cargo xtask ci`; parse the YAML
    (state how)
  - Note (2026-11-03): the user accepted ADR-0022 as written, which
    answers the `Decide first` line.
  - Note (2026-11-03): the pins in `codeql.yml`, each looked up that day
    with `git ls-remote --tags https://github.com/<owner>/<repo>
    'refs/tags/<tag>' 'refs/tags/<tag>^{}'`:
    - `actions/checkout` v7.0.1: `3d3c42e5aac5ba805825da76410c181273ba90b1`,
      a lightweight tag (no `^{}` line), the same pin as `ci.yml`'s.
    - `github/codeql-action/init` and `analyze` v4.39.1:
      `9b3254085494b55a0f7c4c2b3d2d52cc712ce370`, the peeled `^{}` commit of
      an annotated tag whose tag object is
      `8498dc4c2b27cdd99baf0f87955b29ebbf30c78d`.
    - Not v4.39.2 (peeled `1fc58d04a4d4374f921bdac573c3811c240f9c26`): it
      was published on 2026-11-03, inside ADR-0022's 7-day cooldown, so
      Dependabot would not propose it yet either. v4.39.1 was published on
      2026-10-21. The dates are GitHub's release dates (`gh api
      repos/github/codeql-action/releases/tags/<tag>`, `published_at`).
  - Note (2026-11-03): `cargo xtask workflows` is ported from Nadir's
    check, and fails closed on more than ADR-0022 lists: on YAML it cannot
    read line by line, flow mappings included (`docs/04-quality.md` →
    "`workflows`"). The template fixture is GitHub's MIT-licensed text; its
    notice is in `xtask/tests/data/NOTICE.md`, and it has a row in
    `docs/02-dependency-policy.md` → "Assets".
    - **Verified** on Linux (toolchain 1.99.0): `cargo xtask workflows`
      passes (2 workflows), and `cargo xtask ci` passes: 9 of 9 steps, 791
      tests, 0 failed, 0 `SKIPPED:`. PyYAML 6.0.1 (`python3 -I`,
      `yaml.safe_load`) parses `codeql.yml`, `ci.yml` and
      `dependabot.yml`. The template's five problems are asserted by
      `workflows::tests::github_s_codeql_template_has_exactly_its_five_problems`
      and the end-to-end
      `workflows_reports_each_of_the_five_problems_of_github_s_codeql_template`.
    - A security review and a four-lens review, each finding checked by a
      skeptic, found YAML that led the line reader past a rule: a lone CR
      or a Unicode line break, `uses :`, a quoted value over several
      lines, aliases, escapes in `on:`, a repeated `persist-credentials`.
      Each now fails. Of the 116 variants the reviewers built, the 100 that
      break a rule or cannot be read fail and the 16 that keep the rules
      pass. 6,000 random mutations of the real workflows caused no panic
      or hang. 8,000 more, read again with PyYAML, found no workflow that
      passes while breaking a rule, except ones GitHub rejects as invalid
      (`jobs` not a mapping, a job-level `uses:` that names an action).
  - ⏳ needs human after the change: push, and confirm CodeQL's first run
    on `main` succeeds, and that its Rust leg, now with `CARGO_NET_OFFLINE`,
    still reports 0 Rust files extracted with errors (89 of 89 on
    2026-11-02, before the rewrite).

- [x] **M0.9 — `cargo xtask crates`: the members' manifests and READMEs**
  - Depends: M0.3, M0.6
  - Decide first: accept ADR-0023 (crate READMEs and the manifest check),
    which is `Proposed`, if M0.6 has not.
  - Do:
    - Add `publish = false` to every member's `[package]` that lacks it
      (all but `xtask`).
    - `cargo xtask crates`, run by `ci`, per ADR-0023 and
      `docs/04-quality.md`: every member inherits `edition`, `version` and
      `license` from the workspace, sets `publish = false` and
      `[lints] workspace = true`, and has a `README.md` whose first heading
      is its package name, with `## Status` and `## Design` sections that
      name only task IDs this file has.
  - Accept: fixtures fail for each rule; the real workspace passes.
  - Verify: `cargo xtask crates`; `cargo xtask ci`
  - Note (2026-11-08): the `Decide first` line is answered: the user
    accepted ADR-0023 as written in M0.6's review.
  - Note (2026-11-08): done. Every member but `xtask` got
    `publish = false`. The check is `xtask/src/crates.rs`, a step of `ci`
    after `layers`; its rules are in `docs/04-quality.md` → "`crates`".
    - **Beyond the Do line:** it also fails a root `[workspace.package]`
      edition other than `"2024"`, and a target (`[lib]`, `[[bin]]`,
      `[[test]]`, `[[bench]]`, `[[example]]`) that sets an `edition` of its
      own, which cargo 1.99 still accepts with only a warning. Together
      they make ADR-0023's "Manifests cannot drift from the workspace's
      edition" true. As ADR-0023 words it, a README's first line must be
      `# <package name>`, and its task IDs are checked wherever they are,
      not only in `## Status` and `## Design`.
    - **Verified** on rustc 1.99.0: `cargo xtask crates` prints "members
      checked: 26, task IDs in docs/roadmap.md: 105" and `crates: ok`;
      `cargo xtask ci` passes 10 of 10 steps, 1,045 tests, 0 failed, 0
      `SKIPPED:`. A fixture fails for each rule (the `crates_` tests in
      `xtask/tests/fixtures.rs`, beside the unit tests in
      `xtask/src/crates/tests.rs`), and
      `the_real_repository_s_crates_pass` runs it on this repository.
    - **Review:** two rounds, each finding checked by a skeptic. The first
      confirmed 15, all fixed, among them the target `edition`; the second
      confirmed 6 in the fixes, all in how a README is read. Sections are
      found as `layers` reads a document, and where that differs from a
      renderer is listed under "Not covered" there, rather than changing
      the reading `layers` relies on.
    - **Not run:** macOS and Windows, where a `readme.md` opens as
      `README.md` and passes (not covered); a FIFO named `README.md`,
      which `std` cannot make, so a directory stands in for "not a regular
      file".

- [x] **M0.10 — xtask: paths shown escaped; symbolic links never resolved**
  - Depends: M0.3, M0.7
  - Do:
    - Show every path with its control characters, line separators and
      bidirectional controls escaped (`walk::relative`, `walk::shown`,
      cargo's command lines in `cargo::display`).
    - Escape the same in every line a check prints and every error, but
      for the newlines and tabs of a message quoting cargo, and start each
      continued line with `  | ` (`check::printable`).
    - Report every symbolic link under the root, in `deps`, `unsafe-audit`
      and `headers`, without resolving it: no `fs::metadata` on a link, a
      linked `Cargo.toml` included.
    - Refuse to open the files a check reads by a fixed name (the
      allowlist, the dependency matrix) through a link
      (`walk::read_unlinked`).
    - Update `docs/04-quality.md` → "Command line and rules" and "Files
      searched", and remove the two "Not covered" bullets M0.7 added there
      for these gaps.
  - Accept:
    - A file whose name holds a newline, ESC or CR is shown escaped on one
      line, and no line of output starts with `::` (after leading
      whitespace) or repeats a verdict: unit tests, and end-to-end
      fixtures of `headers` on a forged file name and of `deps` and
      `unsafe-audit` on a forged directory name, which cargo's messages
      quote; skipped where the OS allows no such name.
    - A dangling link, a link to a directory and a link to a file of any
      name are each reported, by the walk and by `headers`; a linked
      `Cargo.toml` is not a manifest to the walk.
    - A linked allowlist or matrix fails `deps` or `layers`, naming the
      link, and nothing it leads to is printed.
    - The real repository passes every check.
  - Verify: `cargo test -p xtask`; `cargo xtask ci`
  - Note (2026-11-02): from the security review of M0.7, whose Low
    findings 1 and 3 these are. The user chose to report every link, since
    portable `std` cannot tell a link to a directory without resolving it,
    and to make this a task. A review of the first version found more
    routes: a linked `Cargo.toml` still resolved, a linked allowlist read
    and quoted, newlines through cargo's command lines, cargo target names
    and cargo's quoted errors, and the Unicode line separators and
    bidirectional controls; the `Do` lines above cover them.
    - **Verified:** `cargo test -p xtask` passes (216 unit tests, 59
      end-to-end), and `cargo xtask ci` passes: 8 of 8 steps, 755 tests,
      0 failed, 0 `SKIPPED:`. The new fixtures, which ran here on Linux,
      are `a_file_name_cannot_add_lines_to_the_output`,
      `a_directory_name_cannot_add_lines_through_cargo_s_messages` and
      `deps_and_layers_refuse_a_linked_input_without_reading_it` in
      `xtask/tests/fixtures.rs`.
    - **Not covered** (`docs/04-quality.md`): cargo follows links itself;
      the streamed fmt, clippy and test steps; and GitHub's older
      `##[command]` syntax, not checked against the runner.
      `::stop-commands::` in the workflow would cover the last two, as a
      CI change.
    - Nothing ran on macOS or Windows, where control characters cannot be
      in a file name and links need Developer Mode.
  - Note (2026-11-02, after the commit): of the gaps named above, the
    streamed steps and the `##[command]` syntax are taken up by M0.11,
    which reads in the runner's source how it finds workflow commands and
    pauses them in CI.

- [x] **M0.11 — CI: workflow commands paused in the toolchain and gate steps**
  - Depends: M0.4, M0.10
  - Do:
    - In `.github/workflows/ci.yml`, run the toolchain step's commands and
      `cargo xtask ci` each between GitHub's `::stop-commands::<token>` and
      `::<token>::`, after `exec 2>&1`. Each step makes its token from 16
      random bytes, in a variable it unsets first and never exports, and
      fails before running anything if the token is not 32 lowercase hex
      digits.
    - Settle from the runner's source how it reads workflow commands, and
      record it in `docs/04-quality.md` → "Command line and rules" and "CI
      (GitHub Actions)", where M0.10 left it unchecked.
    - Add the rule that keeps the pause to `.claude/rules/ci-workflows.md`.
  - Accept:
    - Each step's script, run by hand with bash against stand-ins for the
      programs it runs, prints the stop line first on stdout. When its
      commands pass, it prints the resume line last with the same 32-hex
      token and exits 0; when one fails, it exits with that status. A line
      a stand-in writes to stderr just before it exits comes out on stdout,
      before the resume line, and the step writes nothing to stderr, even
      when a stand-in is killed by a signal.
    - Two runs get different tokens. The token is not in the commands'
      environment, even when `token` is already exported. If `od` prints
      nothing, a short value, CR LF, or 32 characters that are not
      lowercase hex, or `/dev/urandom` is missing, the step fails and runs
      nothing.
    - `ci.yml` parses and keeps ADR-0021's header, its two pinned actions,
      `permissions: contents: read` and `persist-credentials: false`.
    - `cargo xtask ci` passes.
  - Verify: parse the YAML (state how); run each step's script with bash
    (state how); `cargo xtask ci`
  - Note (2026-11-02): from M0.10's "Not covered". The user chose a task of
    its own rather than a step in M0.8, and a rule in
    `.claude/rules/ci-workflows.md` rather than a check in M0.8's
    `cargo xtask workflows`, so ADR-0022 is unchanged. A security review and
    a three-lens review then found that the runner reads stdout and stderr
    apart, and that rustup quotes `rust-toolchain.toml`. The user chose to
    pause the toolchain step too.
    - Read in actions/runner v2.338.2:
      - The runner trims leading whitespace before it looks for `::`, and
        finds the older `##[command]` anywhere in a line
        (`src/Runner.Common/ActionCommand.cs`).
      - It reads stdout and stderr on separate tasks and handles each
        batch's stderr first (`src/Runner.Sdk/ProcessInvoker.cs`).
      - Each step gets its own command manager, so a pause ends with its
        step (`src/Runner.Worker/ActionRunner.cs`,
        `Handlers/Handler.cs`, `HostContext.CreateService`).
    - `actions/checkout` at the pinned commit adds its problem matcher and
      removes it in the same step (`src/main.ts`), so no matcher reads the
      paused output.
    - **Verified** on Linux (bash 5.2, toolchain 1.99.0): `ci.yml` parses
      with PyYAML 6.0.1 (`python3 -I`, `yaml.safe_load`), and both steps'
      scripts were taken from the parsed file and run with
      `bash --noprofile --norc -eo pipefail`, as GitHub runs them.
      - Against stand-ins for rustup, rustc and cargo: every Accept case
        above, a SIGSEGV included (exit 139, bash's report on stdout).
      - The gate step's script, running the real `cargo xtask ci`, passes:
        8 of 8 steps, 755 tests, 0 failed, 0 `SKIPPED:`, nothing on
        stderr, and the resume line last with the stop line's token.
      - The toolchain step ran only against stand-ins:
        `rustup set profile minimal` would change this machine's rustup
        settings.
  - ⏳ needs human: push, and confirm on `ubuntu-latest`, `macos-latest`
    and `windows-latest` that both steps pass, and that after the runner's
    `Run` group each step's output begins `::stop-commands::***` and ends
    `::***::` (the runner masks the token). `od`, `tr` and `/dev/urandom`
    ran only in Linux bash in this session, not in macOS's bash or Git
    Bash on Windows.

- [x] **M0.12 — docs: architecture diagrams in `diagrams/`**
  - Depends: M0.6
  - Decide first: what to draw while most crates are empty, and in what
    form (ADR-0029).
  - Do: per ADR-0029, in `diagrams/`: C4 containers and components; UML
    class diagrams of the built types; UML component diagrams; UML
    sequence diagrams; data-flow and state diagrams; a `README.md` that
    says what each file shows and how to keep it true. Say where the
    diagrams are in `AGENTS.md`, `docs/01-architecture.md` and the docs
    rules.
  - Accept:
    - Every Mermaid block parses.
    - Only what the code has is solid; each planned element names a task
      this file has, and a spec says it.
    - Every type in a class diagram exists under that name, and the crate
      graph is the dependency matrix's.
  - Verify: parse every Mermaid block (state how); doc review (human)
  - Note (2026-11-08): the user asked for the diagrams and answered the
    `Decide first` line: draw what is built and what is planned, planned
    parts dashed with their task, in Mermaid as Nadir does (ADR-0029).
  - Note (2026-11-08): written. Six files, 101 Mermaid blocks, drawn as
    of M0.13.
    - **Parsed:** every block parses and renders with Mermaid 11.4.1
      (`mermaid.parse` and `mermaid.render` on a local page in the
      desktop app's browser), which drew all 44 of Nadir's blocks and
      failed a planted broken one. Scaled to an 880 px column, the median
      block's text is 12.7 px and the smallest 7.9 px; none is wider than
      2,000 px (Nadir's median is 10.7 px).
    - **Review:** two rounds, each finding checked by a skeptic, which
      confirmed 27 errors and found 3 more plausible, all fixed: tasks tied to the wrong element (the
      vim modes, reload and merge), steps the code does not take (a failed
      directory sync drawn as a successful save, a `FAILED` drawn as
      `DIED`), M0.1's crate roots drawn planned, and statements about the
      matrix, the crates check and the CLI's errors.
    - ADR-0029 now says each file names the task it was drawn as of, not
      a commit: the files were drawn before the commit that holds them.
  - Note (2026-11-08): done. The user reviewed the diagrams and approved
    them.

- [x] **M0.13 — crates: READMEs in Nadir's shape; `LICENSE` and `NOTICE` in every member**
  - Depends: M0.9
  - Decide first: the READMEs' shape and how a crate carries its license
    (ADR-0028).
  - Do: per ADR-0028:
    - Rewrite every member's README in the new shape, with examples for
      the built APIs of `core`, `codec` and `text`, and the
      `ReadmeDoctests` item in each library's `src/lib.rs`.
    - Copy the root's `LICENSE` and `NOTICE` into every member.
    - `cargo xtask crates` checks the sections and their order, and the
      copies; update `docs/04-quality.md`, `AGENTS.md`, `CONTRIBUTING.md`
      and the docs rules.
  - Accept: fixtures fail for each new rule; the real workspace passes;
    the README examples run as doc tests in `cargo test`.
  - Verify: `cargo xtask crates`; `cargo xtask ci`
  - Note (2026-11-08): the user asked for the READMEs and licensing in
    Nadir's style and answered the `Decide first` line: Nadir's shape,
    keeping `## Rules` and `## Checks`, and copies of `LICENSE` and
    `NOTICE` in every crate (ADR-0028). The task touches all 26 members,
    as asked.
  - Note (2026-11-08): done. Every member's README is in ADR-0028's shape;
    the libraries' `src/lib.rs` have `ReadmeDoctests`, so the examples of
    `core` (4), `codec` (3) and `text` (3) run in `cargo test`; every
    member holds copies of `LICENSE` and `NOTICE` that `cmp` finds equal
    to the root's.
    - **Verified** on rustc 1.99.0: `cargo xtask crates` prints "members
      checked: 26" and `crates: ok`; `cargo xtask ci` passes 10 of 10
      steps, 1,067 tests, 0 failed, 0 `SKIPPED:`. A unit test and a
      fixture fail for each new rule (a section missing, twice or out of
      order, `## Features` out of place, a copy missing, linked or
      differing, the root's `LICENSE` or `NOTICE` missing), and a seeded
      property test checks the section rule against a model.
    - **Review:** the review of M0.12 also read the READMEs, the check and
      the docs; it confirmed six errors and found one more plausible, in
      who may use `core`, `codec` and `tidepool-testkit`, the CLI errors'
      wording, and two docs rules, all fixed.

- [ ] **M0.14 — site: the demo page at jabbott.dev/Tidepool/; `cargo xtask site` and `features`**
  - Depends: M0.4, M0.7, M0.8, M0.9, M0.11, M1.3, M1.4, M1.7
  - Decide first (ADR-0030):
    - what the page's main panel shows while nothing draws;
    - how the features are listed, and kept true as tasks are ticked;
    - when the page is published, and how, in place of the README that
      Pages serves at `/Tidepool/` today;
    - how the page learns the commit's date and subject while `xtask`
      starts only `cargo`;
    - accept ADR-0030, which is `Proposed`.
  - Do: per ADR-0030 and `docs/04-quality.md` → "Demo page":
    - `docs/features.jsonc`, with every ticked task in a feature, and
      `cargo xtask features`, a step of `ci` right after `crates`, ported
      from Nadir's `xtask/src/features.rs` and reading the tasks with
      `crates`' reader of this file. This task belongs to a feature too,
      and ticking it adds it to that feature's `ticked` in the same change.
    - The examples `crates/codec/examples/formats.rs` and
      `crates/text/examples/editing.rs`, each on fixed inputs.
    - `site/index.html`, `site/tidepool.css`, `site/tidepool.js` and
      `site/llms.txt`, in jabbott.dev's markup, loading that site's
      stylesheet and script.
    - `cargo xtask site [--out DIR]`, ported from Nadir's
      `xtask/src/site.rs` and `site/page.rs`, with the
      `TIDEPOOL_SITE_*` variables and the reading of `.git` for a local
      build.
    - `cargo xtask headers` checks `.html`, `.css` and `.js` files.
    - `.github/workflows/pages.yml`, and its row in `WORKFLOWS`
      (`xtask/src/workflows.rs`). Look up each pin with `git ls-remote`
      and record it in a task note.
    - The docs that name `xtask`'s commands, its children, the workflows
      and the header's file types: `AGENTS.md`, `CONTRIBUTING.md`,
      `README.md`, `SECURITY.md`, `.claude/rules/ci-workflows.md` and
      `docs-and-roadmap.md`, the READMEs of `xtask`, `tidepool-codec` and
      `tidepool-text`, and `diagrams/`.
  - Accept:
    - `cargo xtask site` builds the page into `target/site`, running no
      program but cargo and making no network calls. It fails, writing
      nothing, when the features, a gate step or an example fail, and it
      refuses an `--out` directory that is not new, empty or marked by an
      earlier run.
    - With the three `TIDEPOOL_SITE_*` variables, the page names the
      commit, its date and its subject; with none, it names HEAD's hash
      read from `.git` and says it is a local build; with one or two, or a
      value that is not valid, `site` fails naming the variable.
    - `cargo xtask features` is a step of `ci`, after `crates`. Fixtures
      fail for each of its rules; the real repository passes.
    - `headers` fixtures fail an HTML, a CSS and a JavaScript file without
      the header, and pass each with it, an HTML file's after a doctype.
    - `pages.yml` passes `cargo xtask workflows`, parses, and pauses
      workflow commands in every `run:` step that reads the checkout or
      runs its code.
    - The built page passes a look in a browser at desktop and phone
      widths, with jabbott.dev's stylesheet loaded (human).
    - The page is published at <https://jabbott.dev/Tidepool/> once the
      user has switched the Pages source (below).
  - Verify: `cargo xtask ci`; `cargo xtask site`
  - Note (2026-11-08): ADR-0030 answers the `Decide first` questions with
    the options the user chose that day, each the recommended one: tabs of
    the gate and two examples run on the commit; `docs/features.jsonc` and
    its check; `pages.yml` after CI on `main`, replacing the README; and
    the commit's facts passed in by the workflow, so `xtask` still starts
    only `cargo`.
  - Note (2026-11-08): the user accepted ADR-0030 as written, which
    answers the `Decide first` line.
  - Note (2026-11-08): `pages.yml`'s pins, each looked up with `git
    ls-remote --tags https://github.com/actions/<name>` (all four are
    lightweight tags, with no peeled `^{}` line), and the same as Nadir's:
    `actions/checkout` v7.0.1 `3d3c42e5aac5ba805825da76410c181273ba90b1`,
    `actions/cache` v6.1.0 `55cc8345863c7cc4c66a329aec7e433d2d1c52a9`,
    `actions/upload-pages-artifact` v5.0.0
    `fc324d3547104276b827a68afc52ff2a11cc49c9`, `actions/deploy-pages`
    v5.0.1 `368f82528645a54fb793d4d04e342629a3f51346`.
  - Note (2026-11-08): a build with the three variables set from `git log
    -1`, as `pages.yml` sets them, was looked at in a browser with
    jabbott.dev's stylesheet and script loaded, at 1400 × 900 and at
    375 × 812: no horizontal scroll at phone width, the tabs and the
    feature filters work, and the diagrams' text fits their boxes.
  - ⏳ needs the user: switch Settings → Pages → Source to GitHub Actions,
    push, and see the page at <https://jabbott.dev/Tidepool/>.

- [x] **M0.15 — unsafe-audit: doc tests forbid `unsafe_code`**
  - Depends: M0.3, M0.13
  - Decide first: how the gate catches `unsafe` in doc tests, which
    neither the workspace's `unsafe_code = "deny"` nor `unsafe-audit`
    reaches (`docs/04-quality.md` → `unsafe-audit`).
  - Do: give the crate root of every library target but `sys`'s and
    `platform`'s `#![doc(test(attr(forbid(unsafe_code))))]`, and make
    `cargo xtask unsafe-audit` fail a library crate root that lacks it,
    per ADR-0036.
  - Accept:
    - A generated workspace whose library carries the attribute fails
      `cargo test --doc` on an `unsafe` block in a README example and in a
      `///` example, and on a doc test's own `#![allow(unsafe_code)]`.
    - The audit fails a library crate root without the attribute, with it
      under `cfg_attr`, with other lints in the same `attr`, or as an
      outer attribute.
    - The audit passes the real workspace, and `sys`'s README example
      still runs.
  - Verify: `cargo test -p xtask`; `cargo xtask unsafe-audit`;
    `cargo xtask ci`
  - Note (2026-11-11): M2.1's review found the gap. The user answered the
    `Decide first` line by choosing the attribute over lexing doc tests in
    the audit, or doing both (ADR-0036).
  - Note (2026-11-11): done. The 22 library crate roots but `sys`'s and
    `platform`'s carry the attribute, and their READMEs' Rules say so. The
    audit counts every crate type of a `[lib]` target: Cargo 1.99.0 runs
    doc tests of `lib`, `rlib`, `proc-macro` and `["cdylib", "rlib"]`
    libraries, and none of one that is only `dylib`, `cdylib` or
    `staticlib`, but one more crate type would start them. The fixture
    test runs `cargo test --doc --offline` on a generated workspace twice:
    without the attribute its 7 doc tests pass, which is the gap, and with
    it the `unsafe` ones and the three `allow`s fail.
  - Note (2026-11-11): the review found that the audit's lexer read a
    shebang line as code, older than this task: `#!/x "` on line 1 opened
    a string that hid the rest of the file from every rule, while rustc
    compiled it (`headers` caught it in `ci`, as line 1 must be the license
    header). The lexer now skips a byte-order mark and a shebang line as
    rustc does, checked against rustc 1.99.0 in 16 cases, and reads a
    vertical tab as white space, which had let `unsafe\x0B{` escape the
    `// SAFETY:` rule. A crate root the audit cannot read is reported as
    that only.
  - Note (2026-11-11): a test of the attribute found two more ways past
    it, and the user chose ADR-0037. A proc-macro crate could make the
    `unsafe` keyword from a string, with a span the lint skips, so no
    workspace may have one. A doc test could pass `unsafe` to another
    crate's macro that writes the braces, so the audit now reads the
    doc-test code of every crate but `sys` and `platform` for the word:
    64 code blocks in the tree today. The same test found that the lexer
    read a combining mark after `unsafe_code` as a separator, and that the
    rule for exported macros in `sys` and `platform` saw only `unsafe`
    blocks and impls; both are fixed. All four ways fail the audit now.
  - Note (2026-11-11): a test of the doc reader against rustdoc found 18
    constructs it read differently. Each mattered only through an
    exported macro that writes braces after a caller's `unsafe`, so the
    user chose ADR-0038: that shape and macro-defining macros fail the
    audit, and the reader is a second layer, with its mechanical gaps
    fixed and its other limits written down.

**Exit criteria (M0):** `cargo xtask ci` is green locally and on all three
CI runners, with `headers`, `workflows`, `crates` and `features` among its
steps. `Cargo.lock` contains zero third-party packages. CodeQL has run on
`main` with pinned actions. The demo page at <https://jabbott.dev/Tidepool/>
is published from `main`.

---

## M1 — Text engine and formats (headless)

**Goal:** the data structures and parsers everything else stands on.

- [x] **M1.1 — text: rope and positions**
  - Depends: M0.2
  - Do: `rope`, `pos` (byte, char, line/column, UTF-16; grapheme
    boundaries from UCD tables generated by `xtask ucd`).
  - Accept: property test vs a `String` model with ≥ 10,000 random edit
    sequences; every conversion round-trips.
  - Verify: `cargo test -p tidepool-text`, `cargo test -p xtask`,
    `cargo xtask ucd --check`
  - Note: the Unicode 17.0.0 data is kept in `crates/text/ucd/` and
    `ucd --check` is a `ci` step (ADR-0018); positions follow ADR-0019.
    Every position is checked in tiny leaves; in real-size leaves, near
    every seam, on lines longer than a leaf, and sampled elsewhere. A
    grapheme query can still take 0.5–3 s on a crafted 100 MB line (one
    100 MB cluster, or a long run of marks or regional indicators), so M4
    must bound cursor motion or run it off the main thread.

- [x] **M1.2 — text: selections, transactions, undo tree**
  - Depends: M1.1
  - Do: `selection`, `transaction`, `history` per
    `docs/subsystems/text-and-formats.md`.
  - Accept: undo after any random transaction sequence restores the text
    exactly; redo branches are kept.
  - Verify: `cargo test -p tidepool-text`
  - Note: the edit and undo model follows ADR-0020.
    - **Acceptance tests:** a model that keeps every revision's text runs
      2,000 random sequences in tiny leaves, plus 40 at real sizes.
      - Each sequence mixes commits with every grouping, undo, redo,
        `jump_to`, saved markers and refused operations, with the apply
        path forced at random.
      - Every step's text is checked, hop by hop.
      - At the end, undo must give the first text, and every revision must
        be reachable with its exact text.
    - **Measured** (release; 10,000 lines of about 400 KB):
      - a 10,000-caret keystroke commits in about 1–2 ms, undo and redo
        about the same;
      - typing into 100 MB takes about 12–14 µs per character.
    - **Residual risk:** 10,000 carets on a 100 MB file take about 120 ms,
      over one frame. Each M1.1 in-leaf edit costs about 11 µs, because it
      recounts the leaf. M4.2 should batch such edits or make that edit
      cheaper.

- [x] **M1.3 — text: encodings and files**
  - Depends: M1.1
  - Do: `encoding` (UTF-8, UTF-16 LE/BE with BOM; invalid UTF-8 opens
    read-only and lossless) and `file` (load with the 16 MB large-file
    threshold; atomic save: a temp file in the same directory, fsync,
    rename, permissions kept; line endings and the trailing newline
    preserved), per `docs/subsystems/text-and-formats.md`.
  - Accept: unchanged files with BOM, CRLF, mixed endings and no trailing
    newline round-trip byte-for-byte; a crash between write and rename
    leaves the original intact (simulated); a file that is not valid UTF-8
    is never written back.
  - Verify: `cargo test -p tidepool-text`
  - Note (2026-11-04): saving follows ADR-0025, from the user's answers
    of 2026-11-03 and 2026-11-04. A save follows links to their target;
    the temp file of an existing file starts 0600 and loses setuid and
    setgid, through the one `cfg(unix)` in `text`'s library; what a
    rename changes is documented, and M4.3 checks it.
    - **Lossless and read-only:** each replaced run keeps its bytes, so
      `Decoded::original_bytes` rebuilds any input. A text with a
      replacement has no save format (`format()` is `None`). The large
      threshold is 16 MiB.
    - **Verified:** `cargo test -p tidepool-text` passes: 105 unit
      tests, and 23 in `tests/files.rs`, 9 of them Unix-only. The
      encoding properties run 11,150 cases against `from_utf8_lossy` and
      `decode_utf16`: lossless, the same however the input is split, and
      valid input re-encodes to itself. `cargo xtask ci` passes 9 of 9
      steps.
    - **Mutation checks:** without the setuid mask, the setuid test
      fails. The implementing agent disabled the split carry, and the
      split property failed.
    - **Review:** a security review, a decoder review and a review of the
      file semantics, tests and docs. A skeptic checked each finding, then
      a completeness critic looked for gaps. Fixed from it:
      - setuid and setgid survived a save, so root's save of a 4755 file
        was setuid-root;
      - a file system that cannot sync a directory (`EINVAL`, `ENOTSUP`)
        failed every save after the rename;
      - the unit tests' scratch directories now live at a path no one
        else can have made first;
      - the docs were wrong on a run's memory, on BOM-less formats, and
        on what a rename changes (the group, a Linux default ACL).
    - **Measured** (release, this machine, a scratch binary rather than
      the bench harness): decoding 99 MB of UTF-8 takes 0.30 s; loading
      99 MB takes 0.41 s, and saving it 0.17 s.
    - **Not run here:** macOS and Windows. CI runs these tests there on
      the next push (M0.4's ⏳). That includes renaming over a file that
      Windows has open, its read-only attribute, and macOS's
      `F_FULLFSYNC` on a directory.
    - **For later tasks:** M1.6 adds a fuzz target for
      `text::encoding::Decoder`. M4.3 checks the link count, owner and
      group before a save. M4.9 may clean the `.tidepool-save-*` files a
      crash leaves behind.
  - Note (2026-11-09): M1.8's security review found that on Windows a save
    through a directory link joined a relative link target onto the
    link's lexical parent, which Win32 reads as another place than the
    OS opens. Where Windows' path rules apply, `save` now joins it onto
    the canonical path of the link's directory, taken from the whole path
    in its verbatim form; Unix keeps the lexical join, which the kernel
    resolves. A target with a root or a drive that is not absolute is
    refused (ADR-0034).

- [x] **M1.4 — codec: JSON and JSONC**
  - Depends: M0.2
  - Do: `codec::json` parser and writers with `Limits`. Port from Nadir's
    `nadir-codec::json`.
  - Accept: RFC 8259 cases; limits enforced; no panics on malformed input.
  - Verify: `cargo test -p tidepool-codec`
  - Note: done ahead of M0.3, which needs it. The supported subset is in
    `docs/subsystems/text-and-formats.md` → `json`. Its fuzz target and
    seed corpus come with M1.6.

- [x] **M1.7 — codec: TOML 1.1 parser**
  - Depends: M1.4
  - Decide first: whether to vendor the cases of the `toml-test` suite as
    test-only fixtures, once their license is confirmed (as for the
    CommonMark examples in M6.5), or to write the compliance cases by hand.
  - Do: `codec::toml` parsing TOML 1.1 into a value tree with source spans,
    with `Limits` and errors that give the line and column (ADR-0013,
    ADR-0026). Vendor toml-test v2.2.0's 1.1 cases with their notice.
    Document the supported subset in `docs/subsystems/text-and-formats.md`
    → `toml`.
  - Accept: every valid and invalid case on toml-test's 1.1 list; limits
    enforced; no panics on malformed input.
  - Verify: `cargo test -p tidepool-codec`
  - Note (2026-11-04): the user answered the `Decide first` line: vendor
    toml-test (MIT, v2.2.0), and parse TOML 1.1, since Cargo has accepted it
    since cargo#16415. The roadmap first said TOML 1.0. Both answers are in
    ADR-0026.
  - Note (2026-11-04): the 1.1 list's 895 files and the list itself are in
    `crates/codec/tests/toml-test/`. Each one's git blob hash matched
    toml-test's v2.2.0 tree when it was copied. What the parser accepts,
    its value model and its limits are in the spec's `### toml` section.
    - **Verified:** `cargo test -p tidepool-codec` passes: 143 unit tests
      and the 8 tests of `tests/toml.rs`. Those run every case on the
      list: 214 valid cases parse to their expected values, and 467
      invalid ones fail. `cargo xtask ci` passes 9 of 9 steps.
    - **Review:** a robustness review (by `security-reviewer`), a
      conformance review and a review of the tests, spans and docs. A
      skeptic checked each finding, then a completeness critic looked for
      gaps.
      - About 9.6 million mutated and generated inputs caused no panic.
      - Nesting 200,000 deep overflowed no stack, on a 256 KiB thread.
      - Every adversarial shape took time linear in its size.
      - 180,000 TOML 1.0 documents compared against Python's `tomllib`
        differed only where the spec documents a choice, or where 1.1
        relaxes 1.0.
    - **Fixed from the review:**
      - the worst-case memory was about 80 times the text, not the 43
        times documented: tables now grow by a quarter, are shrunk when
        the parse ends, and index positions instead of copied names;
      - `max_number_bytes` did not count `inf` and `nan`;
      - five error kinds' docs did not match the code. A bare CR is now
        `BareCarriageReturn` wherever it appears;
      - the compliance test let a tree holding a key twice pass.

      After the fixes, the robustness harness ran 1.8 million more inputs
      with no problem, and its depth and timing checks passed again.
    - **Measured** (release, RSS on this machine): the worst shape is a
      long chain of one-entry tables (`k.b.b…`), at about 49 bytes of heap
      per byte of text, about 72 times the text with glibc. At the default
      64 MiB input that is about 4.5 GiB, so callers that read untrusted
      TOML should lower `max_input_bytes`. `Cargo.lock` takes about 7
      times its text. 100,000 keys parse in 0.03 s.
    - **Not run here:** macOS and Windows. CI runs these tests there on
      the next push (M0.4's ⏳).
    - **For later tasks:** M1.6 adds a `toml` fuzz target, seeded from the
      vendored valid cases. M1.5 decides whether its writer writes TOML 1.1
      syntax.

- [x] **M1.6 — xtask: fuzz smoke**
  - Depends: M0.3, M1.4, M1.7
  - Decide first (`docs/04-quality.md` → "Fuzz smoke" calls these
    provisional):
    - where the targets for parsers outside `xtask`'s matrix row (`core`,
      `codec`) live: a wider row for `xtask`, a dev-only target in each
      crate that the harness drives, or a separate dev-only member;
    - how to cap allocation, since a counting `GlobalAlloc` is `unsafe` and
      `unsafe` lives only in `sys` and `platform`, or whether each parser's
      `Limits` is enough;
    - whether `fuzz-smoke` runs anything but in-process code, which would
      extend ADR-0017.
  - Do:
    - `cargo xtask fuzz-smoke [--secs N] [--target T]` per
      `docs/04-quality.md`; targets for JSON, TOML and `text::encoding`'s
      decoder (M1.3's note) with seed corpora in `fuzz/corpus/`; a crash is
      saved to `fuzz/crashes/<target>/` and becomes a regression test.
    - Add the Linux fuzz-smoke step (30 s per target) that
      `docs/04-quality.md` → "CI" requires to `.github/workflows/ci.yml`;
      M0.4 left a `TODO(M1.6)` there.
  - Accept: 60 s per target with no crash; a planted panic is found.
  - Verify: `cargo xtask fuzz-smoke --secs 60`
  - Note (2026-11-07): the user answered the `Decide first` line, taking
    the recommended option each time. Targets live in each parser crate's
    `tests/fuzz.rs`, driven by an engine in `tidepool_core::testing`.
    Over-allocation is caught by small `Limits` and, on Linux, the peak
    memory. `fuzz-smoke`'s only child is `cargo test`, which extends
    ADR-0017. All three are in ADR-0027. After the review, the user chose
    to read the peak on a time budget and to check reserved memory
    (`VmPeak`) as well, and ADR-0027 records both.
  - Note (2026-11-08): done. The engine is `tidepool_core::testing::fuzz`;
    the targets `json` and `toml` are in `crates/codec/tests/fuzz.rs` and
    `encoding` in `crates/text/tests/fuzz.rs`, with 54 hand-made seeds in
    `fuzz/corpus/` (some sit exactly at each of `codec`'s small `Limits`)
    and `fuzz/NOTICE.md`. What each run does is in `docs/04-quality.md` →
    "Fuzz smoke".
    - **Verified:** `cargo xtask fuzz-smoke --secs 60` exits 0, 3 of 3
      targets clean (seed `0x0d7d112a4d244aad`): `json` 9,116,317 inputs
      (151,936 a second), `toml` 33,215,266 (553,548), `encoding`
      12,213,990 (203,565). Written memory rose at most 0.5 MiB of a
      16 MiB cap; reserved memory stayed at the 64 MiB glibc arena gap,
      of a 256 MiB cap. A panic planted in `json::parse_bytes` on
      `nullnull`, which no seed holds, was found in 30 s and saved as
      `fuzz/crashes/json/panic-<hash>`; plain `cargo test` then failed on
      that file, and passed on it once the plant was reverted. A planted
      untouched 272 MiB reservation was found by range and named and
      saved by its replay. `cargo xtask ci` passes 9 of 9 steps (1,005
      tests, none SKIPPED), as do core's and xtask's tests on one
      thread.
    - **Review:** five dimensions, `security-reviewer` among them, each
      finding checked by a skeptic; then three more rounds on the fixes.
      Among what was fixed:
      - a replay could delete the only copy of the input a death left;
      - the result line was missed on one test thread;
      - CI's colour made a build failure read as a death;
      - a symbolic link at `fuzz/` or below was followed;
      - `VmHWM` cannot see memory reserved and never written, hence
        `VmPeak`;
      - a race left the count of stuck threads up, which turned the
        memory checks off for the rest of the process. Before the fix,
        4 of 2,000 stress runs failed; after it, none of 6,400, and none
        of 1,680 more at a load average of about 300.
    - **Measured:** reading `/proc/self/status` after every input costs
      2.7 to 7 times the throughput, hence the time budget
      (`crates/core/src/testing/fuzz/memory.rs`). A replay adds about
      8 µs an input.
    - **Not run here:** macOS and Windows, where both memory checks skip
      and say so. CI's new fuzz step runs on the next push (M0.4's ⏳). A
      replay under a systemd memory limit was checked by a reviewer
      only.
    - **For later tasks:** every new parser adds a test to its crate's
      `tests/fuzz.rs`, seeds and a row in `TARGETS`
      (`.claude/rules/parsers.md`).

- [x] **M1.5 — codec: TOML with a preserving document model**
  - Depends: M1.7
  - Decide first: whether the writer may write TOML 1.1 syntax, which
    tools that know only TOML 1.0 cannot read (ADR-0026).
  - Do: the comment- and order-preserving document model of ADR-0013
    (accepted 2026-11-01) on top of M1.7's parser: read, change and add
    values, and write back.
  - Accept: TOML 1.1 compliance cases still pass; an unchanged document
    writes back byte-for-byte; editing a value and writing back preserves
    comments, order and formatting elsewhere.
  - Verify: `cargo test -p tidepool-codec`
  - Note (2026-11-08): the user chose TOML 1.0 syntax for the text the
    writer creates, which answers the `Decide first` line (ADR-0031).
  - Note (2026-11-08): done.
    - **Built:** `toml::Document` (`parse`, `parse_bytes`, `get`, `set`,
      `as_str`, `into_string`), the path type `Step`, `EditErrorKind` and
      `CodecError::TomlEdit`, and `Array::new`, `Table::new_inline` and
      `Table::insert` for building values. `set` plans one splice, parses
      the new text again with the document's `Limits` and compares the
      old and new trees along the path before keeping either; a refused
      edit leaves the document as it was. The design was merged from
      three independent ones; `docs/subsystems/text-and-formats.md` →
      "The document model" summarizes the rustdoc, which is authoritative.
    - **Verified:** `cargo test -p tidepool-codec` passes: 160 unit tests,
      6 in `tests/fuzz.rs`, 26 and 6 of JSON, 14 in `tests/toml.rs`, 27 in
      `tests/toml_document.rs` and 9 doc tests. All 214 valid toml-test
      cases write back byte for byte; every leaf (1,015) and every table
      (607) of them takes an edit, each checked against toml-test's
      expected JSON edited the same way. `cargo xtask fuzz-smoke --target
      toml --secs 60`: 17,985,186 inputs, no finding, +0.7 MiB of the
      16 MiB cap. `cargo xtask ci` passes 11 of 11 steps, with 1,170 tests
      passed in 65 binaries and none skipped.
    - **Review:** five lenses (placement, robustness by
      `security-reviewer`, ADR-0031's syntax, the re-parse check, docs and
      tests), a second agent reproducing or refuting each finding, then a
      completeness critic. No defect in what an edit writes: placement and
      created text were also checked against Python's `tomllib`, a TOML 1.0
      parser. Fixed from it: each of the re-parse check's comparisons, and
      the rule that two NaNs of one sign are the same data, now has a test
      that fails without it (six mutants, each killed); the fuzz target
      picks the table it edits by a hash of the input, so implicit tables,
      header blocks under other tables and the scan above the first
      header are fuzzed too.
    - **Not supported, for the task that needs it:** removing keys,
      appending to an array or adding a `[[...]]` element, and replacing a
      table under a header as a whole. A changed value is written afresh,
      so it loses its spelling (hex, underscores, left-out seconds), and a
      multi-line value it replaces becomes one line, without the comments
      inside it (ADR-0031). Each edit parses the whole text again and
      briefly holds two trees, so M4.7 should read settings with a lower
      `max_input_bytes`, about 1 MiB.
    - **Not run here:** macOS and Windows. CI runs these tests there on
      the next push.

- [x] **M1.8 — core: path helpers**
  - Depends: M0.2
  - Do: `core::path` per `docs/subsystems/core.md`: UTF-8 path helpers,
    canonicalization and the display of paths that are not UTF-8. M0.2's
    `Do` line left it out.
  - Accept: a path that is not UTF-8 displays without hiding which bytes it
    holds (tested on Unix with bytes built in the test); canonicalization
    is tested in a temp directory under `target/`.
  - Verify: `cargo test -p tidepool-core`
  - Note (2026-11-08): the user chose `\xNN` for each byte that is not
    UTF-8, with control characters escaped too, and `cfg(unix)` and
    `cfg(windows)` in `core::path`'s tests to build such paths (ADR-0032).
  - Note (2026-11-08): from the merged design, the user chose that
    `canonicalize` drops Windows' `\\?\` where Win32 reads the plain form
    back, that `resolve` and `under` are part of this task, and that the
    display also escapes bidirectional controls and line separators
    (ADR-0033).
  - Note (2026-11-09): done.
    - **Built:** `core::path`: `display` (`PathDisplay`), `to_utf8`,
      `canonicalize`, `resolve`, `under` and `PathError`, with no platform
      `cfg` in the library. The design was merged from three independent
      ones; `docs/subsystems/core.md` → "Public API names" summarizes the
      rustdoc.
    - **Verified:** `cargo test -p tidepool-core` passes: 276 unit tests,
      20 in `tests/path.rs`, 24 in `tests/log.rs`, 2 in `tests/seed.rs`,
      1 in `tests/fuzz_memory.rs` and 19 doc tests, with no `SKIPPED:`
      line on Linux. The unit test that builds a path that is not UTF-8
      from bytes runs here; canonicalization is tested in directories
      under `target/`. `cargo clippy` with `-D warnings` is clean for the
      Windows and macOS targets too, which compiles their `cfg` tests.
      `cargo xtask ci` passes 11 of 11 steps, with 1,211 tests passed in
      66 binaries and none skipped.
    - **Review:** two rounds. The first (six lenses, among them
      `security-reviewer` and one predicting the Windows and macOS CI
      runs) checked the display against an independent decoder over more
      than 300 million inputs and found nothing there. It found `resolve`
      cubic in time on `missing/..` pairs (a 16 KB path took 197 s), a
      Windows name such as `c:x` read as a drive, probes of plain prefixes
      that Win32 rewrites, a final separator lost, a race giving
      `InvalidInput`, the 40-link limit untested, and wrong docs;
      `resolve` was rewritten as one canonicalization and otherwise a walk
      from the root. The second found absolute link targets still costing
      about 30 times `canonicalize`, `\\.\` paths probed as Win32 rewrites
      them, empty link targets, and Windows' trimmed final separators; all
      fixed. A randomized checker against the kernel agrees on 45,000
      trees over three seeds, and 40 chained absolute links in a tree
      1,000 deep take 1.1 s where `canonicalize` takes 0.5 s.
    - **Not run here:** macOS and Windows, which CI runs on the next push.
      Only those runs confirm the Windows prefix rules (U7, U9, U10, U13,
      the trimmed names), the case and `/private` forms on macOS, and the
      loop errors' kinds. Expected `SKIPPED:` lines: on macOS, the name
      that is not UTF-8 (APFS refuses it) and the 2,000-pair timing test
      (`realpath` refuses paths over 1,024 bytes); on Windows, the cases
      that list a directory through a link.
    - **For later tasks:** `under` and `resolve` are checks, not a
      sandbox: a link changed after them escapes them, and on Windows
      canonicalizing through a link to `\\host\share` authenticates to that
      host. M4.7's security review weighs both. The review also found that
      on Windows `text::file::save` joins a relative link target onto the
      link's lexical parent, where the OS uses the link's own directory
      (reasoned from std's sources, not run).
  - Note (2026-11-20): macOS CI failed two of these tests on every run
    from this task's commit on: `a_final_separator_asks_for_a_directory`,
    since macOS's `realpath(3)` gives a file for `file.txt/`, and
    `absolute_links_into_a_deep_tree_cost_about_what_canonicalize_costs`,
    since it follows 33 links, not 40. ADR-0051 has
    `canonicalize` refuse the first, and the second test now follows as
    many links as the OS does. A macOS CI run has yet to show both pass.

- [x] **M1.9 — bench: harness, `cargo xtask bench` and the text suites**
  - Depends: M0.3, M1.2, M0.5
  - Decide first: extend ADR-0017 for the child `cargo xtask bench` starts
    (a release build and run of `tidepool-bench` through cargo).
  - Do:
    - `tidepool-bench` per `docs/04-quality.md` → "Benchmarks": warm-up,
      N samples, median and p95, a JSON baseline in `bench/baseline.json`
      written with `codec::json`.
    - `cargo xtask bench [filter]` (reports only): `bench` leaves `xtask`'s
      list of commands not yet implemented (`LATER` in
      `xtask/src/check.rs`), whose message for it still says it has no task.
    - Suites for what M1.1 and M1.2 measured by hand: rope edits, position
      conversions, a 10,000-caret keystroke on 400 KB and on 100 MB, typing
      into 100 MB, and grapheme queries on a crafted 100 MB line.
  - Accept: two runs on the reference machine agree within 10% on every
    median; the baseline is committed with the machine named.
  - Verify: `cargo xtask bench text`
  - Note (2026-11-10): ADR-0035 answers the `Decide first` line. The child
    is `cargo run --release --quiet -p tidepool-bench`; only `--save`
    writes the baseline, and it needs `--machine`; the file also records
    the OS, architecture, thread count and CPU model the harness reads.
  - Note (2026-11-10): done. On the reference machine (performance
    governor, on AC, load about 1.3 from the desktop), with both binaries
    built first, `cargo xtask bench text --save --machine "MSI Pulse GL66
    11UGKV, the reference machine of docs/00-overview.md"` and then
    `cargo xtask bench text` ran back to back, 50 s each. Every median of
    the second run was within −2.2% to +6.4% of the first's, which is the
    baseline. Medians of the baseline:
    - rope edits about 9.3–11.1 µs; position conversions 681–727 ns on
      100 MB;
    - a 10,000-caret keystroke 1.79 ms on 400 KB (undo 817 µs, redo
      972 µs) and 115 ms on 100 MB; typing into 100 MB 11.3 µs a char;
    - grapheme queries on the crafted 100 MB lines 524 ms to 1.03 s.
    These agree with M1.1's and M1.2's hand measurements. The review
    found that a suite run alone could swing 25–35% with the allocator's
    page faults, so a run first primes the allocator
    (`harness::prime_allocator`). After that, eight runs of
    `text/history/keystroke_10k_carets/400kb` alone had the same page
    faults (about 2,156) but medians of 1.71–2.10 ms, −4% to +17% of the
    baseline's, so two of them were marked slower; four kept on one core
    (`taskset -c 4`) held 1.94–2.00 ms. The spread follows the core the
    process lands on, not the allocator. Whole runs, as the baseline was
    recorded, agreed.

**Exit criteria (M1):** the text engine (files included) and JSON and TOML
(preserving writes included) pass their property tests and fuzz smoke, and
the text suites have a recorded baseline.

---

## M2 — A window and text on Linux

**Goal:** a Linux window that shows Saira and JetBrains Mono text, drawn by
our own rasterizer.

- [x] **M2.1 — sys: dynlib, libc and X11 declarations**
  - Depends: M0.1
  - Decide first: accept ADR-0010 (X11 through `dlopen`'d libX11; Wayland
    through XWayland), which is `Proposed`.
  - Do: `dynlib!`, `linux::libc`, `linux::x11` per
    `docs/subsystems/sys-and-platform.md`, declaring only what M2.2 calls;
    later tasks add what they call. Follow `/add-ffi-binding`, and port
    Nadir's X11 declarations where they fit, with a note.
  - Accept: layout asserts for every struct; each declaration cites its
    header.
  - Verify: `cargo build -p tidepool-sys`; `cargo xtask unsafe-audit`
  - Note (2026-11-10): the user accepted ADR-0010 as written, which
    answers the `Decide first` line.
  - Note (2026-11-10): done. `dynlib` is ported from nadir-sys; a table is
    written `struct Name: unsafe extern "C" { ... }`, which
    `unsafe-audit` reads as an `unsafe extern` block. `linux::libc` has the
    loader (no `dlclose`), `poll`, and `pipe2` with `O_CLOEXEC` and
    `O_NONBLOCK` for the waker's self-pipe: `std::io::pipe` cannot be made
    non-blocking without the variadic `fcntl`, and a socket pair would be
    a socket of Tidepool's own, against rule 8. `linux::x11` declares the
    types, events and the `Xlib` table of 31 functions M2.2's window,
    events and surface are to call. Typed text (`XOpenIM`, the variadic
    `XCreateIC`), the keyboard map and the waker's `write(2)` are left to
    M2.2, the clipboard's calls to M2.7.
  - Note (2026-11-10): a C program built from the declarations with gcc
    13.3.0 and `-Werror` against glibc 2.39 and libX11 1.8.7 headers on
    x86_64 checks all 191 layout assertions, 59 constants, 36 prototypes
    and 175 field and typedef types. The numbers hold on aarch64 Linux,
    whose clippy run evaluates the same assertions; `linux.rs` fails the
    build on any other Linux target. A test loads the whole
    `Xlib` table from the installed `libX11.so.6`, opening no display.

- [x] **M2.2 — platform: window, events, waker, present (Linux)**
  - Depends: M2.1, M0.2
  - Decide first: where the Xvfb test's read-back with `XGetImage` lives.
    `unsafe` compiles only in the library targets of `sys` and `platform`
    and in their doc tests (ADR-0036), so an integration test cannot make
    the call (`docs/04-quality.md` →
    `unsafe-audit`): a `#[cfg(test)]` test inside `platform`'s library, or
    a safe read-back function that `platform` exports for tests. Also,
    from M2.1's review (the Note below): whether Xlib's connection to the
    X server is `AGENTS.md` rule 8's one exception, and whether TCP
    displays are accepted; and whether the key map reads key codes or
    `XLookupKeysym`.
  - Do: `window`, `events` (poll on the X fd and a self-pipe), `surface`
    (`XPutImage` of damage rects), per `docs/03-platforms.md` → Linux.
  - Accept: under Xvfb, a test opens a window, presents a framebuffer,
    reads the pixels back with `XGetImage`, and a `Waker` from another
    thread interrupts `wait_events` within 5 ms.
  - Verify: `TIDEPOOL_IT_X11=1 xvfb-run cargo test -p tidepool-platform`;
    `cargo xtask unsafe-audit`
  - Note (2026-11-10): M2.1's review raised two more questions to answer
    before M2.2's code. First, `XOpenDisplay` connects a socket in the
    core binary: a Unix one for `:N`, falling back to TCP on localhost
    when that fails (`unix:N` never uses TCP), and TCP when `DISPLAY`
    names a host; it sends the Xauthority cookie for that display if the
    authority file has one. `AGENTS.md` rule 8 records no exception for
    the X connection that ADR-0010 implies: whether it is the rule's one
    exception, and whether TCP displays are accepted.
    Second, `docs/03-platforms.md` → Input maps keys with `XLookupKeysym`,
    but `core::input::Key` names a physical key (M0.2), which a key code
    gives (the evdev code plus 8 under XWayland and the evdev and libinput
    drivers) and a keysym does not: which source the key map uses.
  - Note (2026-11-16): ADR-0039 answers the `Decide first` line with the
    options the user chose that day, each the recommended one: the
    read-back is a `#[cfg(test)]` test in `platform`'s library; the X
    connection is rule 8's one exception, over a local Unix socket only
    (`:N` becomes `unix:N`, a `DISPLAY` naming a host is refused); the
    key map reads key codes; and the window tests run only with
    `TIDEPOOL_IT_X11=1`, which the `Verify` line now sets.
  - Note (2026-11-16): the user chose, while the task was prepared, that
    `present` takes a pixel view of `platform`'s own, since the matrix
    gives `platform` no edge to `gfx` (ADR-0040).
  - Note (2026-11-16): done. `platform` has one public API on every system:
    `window` (`Window::open`, title, device and logical size, scale,
    `CursorShape`, focus), `events` (`wait_events` with a deadline,
    `Event`, the `Waker`) and `surface` (`present` of a checked `Pixels`
    view and `DeviceRect` damage), with its `Error`; elsewhere
    `Window::open` fails with `Error::Unsupported`. On Linux every window
    is on one thread, X errors are recorded and logged instead of ending
    the process, keys come from key codes and text from
    `Xutf8LookupString` through Xlib's own input method. `sys` adds
    `write(2)`, `setlocale`, `linux::evdev`, the types for typed text,
    tables that take C's `...`, and a table of 41 functions; M2.1's
    `XSetIOErrorHandler` and `XAnyEvent` are gone, since nothing calls
    them.
  - Note (2026-11-16): checked. Under Xvfb (21.1.12, libX11 1.8.7,
    libxcb 1.15) the read-back compares every damaged and undamaged
    pixel exactly, and a `Waker` on another thread ends a wait within
    5 ms; a scratch run of 200 wakes measured a median of 26 µs and a
    maximum of 116 µs. `strace` of the test binary saw only `AF_UNIX`
    connections to the X server's socket, and none for a `DISPLAY` that
    names a host. A C program built with gcc 13.3.0 and `-Werror`
    against glibc 2.39, libX11 1.8.7 and the kernel's headers checks the
    new declarations: 260 static assertions, 14 prototypes and the 118
    evdev codes. clippy is clean for x86_64 and aarch64 Linux,
    `x86_64-apple-darwin` and `x86_64-pc-windows-msvc`; macOS and Windows
    were compiled, not run.
  - Note (2026-11-16): reviewed in two rounds, `security-reviewer` in
    each, every finding checked by a skeptic: 15 confirmed findings, 10
    issues once those two reviewers found alike are counted once. All
    are fixed but one, which is documented and narrowed. Among them: a client message's data is read only for
    format 32, the only one whose 40 bytes Xlib writes; `present` gives
    Xlib whole rows of the view, so no server's padding reads past it;
    one wait reports a wake and the server's events together; and the
    tests retry a connection that Xvfb's reset between them refuses.
    Left as documented risk (`docs/03-platforms.md` → Input → Cursors):
    libX11 loads libXcursor, whose theme any client of the display can
    point at files of its choosing, so a FIFO there blocks
    `Window::open`; every cursor is made there, so `set_cursor` reads no
    file.
  - Note (2026-11-20): ADR-0050 changes the name `platform`
    gives `XOpenDisplay` from `unix:N` to `unix/:N`, since libxcb 1.16
    and later read `unix:N` as the path of a socket file. The `DISPLAY`
    values accepted are unchanged.

- [ ] **M2.9 — ci: the window tests run under Xvfb on Linux**
  - Depends: M2.2, M0.4, M0.11
  - Decide first: whether `ci.yml`'s gate step runs `cargo xtask ci` under
    `xvfb-run -a` with `TIDEPOOL_IT_X11=1` on Linux, or a separate
    Linux-only step runs `cargo test -p tidepool-platform` that way after
    the gate; and whether `pages.yml`'s build does the same, so that the
    demo page counts the window tests. M2.2 left CI unchanged (ADR-0039).
  - Do: the change in `.github/workflows/ci.yml`, and in `pages.yml` if
    chosen, with workflow commands paused in every step it adds or
    changes; `docs/04-quality.md` → "CI",
    `docs/subsystems/sys-and-platform.md` → Tests, the `Checks` of
    `crates/platform/README.md`, and the docs that say CI skips the
    window tests.
  - Accept:
    - On the Linux runner the window tests run and pass, with no
      `SKIPPED:` line for them; on macOS and Windows nothing changes.
    - The runner image's `xvfb`, which depends on `xauth`, is used, with
      no install step.
    - The changed workflows pass `cargo xtask workflows` and parse.
    - The Linux job of a CI run on GitHub shows them run (human, after a
      push).
  - Verify: `cargo xtask workflows`; the Linux step's command run here;
    parse the changed workflows (state how)
  - Note (2026-11-16): ADR-0041 answers the `Decide first` line with the
    options the user chose that day, each the recommended one: the gate
    step runs `TIDEPOOL_IT_X11=1 xvfb-run -a cargo xtask ci` on Linux and
    `cargo xtask ci` elsewhere; `pages.yml` is unchanged.
  - Note (2026-11-17): M2.6 adds `gui`'s window test,
    `crates/gui/tests/window.rs`, which every system builds: the Linux
    job runs it with `platform`'s, and the macOS and Windows jobs print
    `SKIPPED:` for it, since nothing sets `TIDEPOOL_IT_X11` there.
  - Note (2026-11-20): every Linux run since then failed `gui`'s window
    test with `cannot connect to the X server at unix:99`, and `cargo
    test` stopped there before `platform`'s ran; the six runs before
    failed all of `platform`'s the same way. `ubuntu-latest`
    is now `ubuntu-26.04`, whose libxcb 1.17.0 reads `unix:N` as a socket
    file named `N` in the working directory, so `XOpenDisplay` failed
    before it opened a socket; this machine's libxcb 1.15 does not.
    ADR-0050 gives Xlib `unix/:N`. The workflow is unchanged.
  - ⏳ needs the user: push, and see the Linux job of `ci.yml` run the
    29 window tests, 28 of `platform`'s and `gui`'s one (its gate's
    summary reads `ci: ok` with no `SKIPPED` count), while the macOS and
    Windows jobs build only `gui`'s, which prints `SKIPPED:` there
    (M2.6).
  - Note (2026-11-22): M3.8 adds the binary's window test,
    `crates/app/tests/window.rs`, which every system builds as it builds
    `gui`'s: the Linux job runs 30, and the macOS and Windows jobs print
    `SKIPPED:` for both of theirs.

- [x] **M2.7 — platform: clipboard and paths (Linux)**
  - Depends: M2.2, M1.8
  - Do: `clipboard` (`CLIPBOARD` and `PRIMARY`, `UTF8_STRING`, `TARGETS`,
    `INCR`), `paths` and `dirs`, per `docs/03-platforms.md` → Linux.
  - Accept: under Xvfb, text copied in one window pastes in another, and a
    1 MiB paste goes through `INCR`; `paths` follows the XDG variables and
    their defaults.
  - Verify: `TIDEPOOL_IT_X11=1 xvfb-run cargo test -p tidepool-platform`;
    `cargo xtask unsafe-audit`
  - Note (2026-11-16): the `Verify` line sets `TIDEPOOL_IT_X11=1`, without
    which the window tests skip (ADR-0039).
  - Note (2026-11-17): the user chose, while the task was prepared, each
    option recommended (ADR-0042): the clipboard goes through the window's
    events (`set_selection`, `request_selection` and `Event::Pasted`), so
    the main thread never waits on another client; an owner serves
    `UTF8_STRING`, `TARGETS` and `TIMESTAMP`, and a paste is
    `UTF8_STRING` of at most 64 MiB; and logs live in
    `$XDG_DATA_HOME/tidepool/logs`.
  - Note (2026-11-17): done. `platform` has `clipboard` (`Selection`,
    `Window::set_selection`, `Window::request_selection`,
    `MAX_PASTE_BYTES`) with `Event::Pasted`, `paths` (`user_dirs`,
    `UserDirs`) and `dirs` (`android_sdk`, `rustup_home`), and the errors
    `NoHomeDirectory` and `Unsupported`, which now names what is missing.
    On Linux the clipboard is `linux::selection`, on each window's own
    connection, and the folders `linux::xdg`, ported in part from Nadir's.
    `sys` adds the property and selection events and their constants, and
    9 functions to Xlib's table, which has 50: `XMaxRequestSize`,
    `XDeleteProperty`, `XGetWindowProperty`, `XFree`,
    `XSetSelectionOwner`, `XGetSelectionOwner`, `XNextRequest`,
    `XConvertSelection` and `XSendEvent`.
  - Note (2026-11-17): checked. Under Xvfb 21.1.12 (libX11 1.8.7,
    libxcb 1.15) text copied in one window pastes in another, both
    selections and both ways, and a 1 MiB paste goes through `INCR` in
    whole parts and an empty one; the 121 tests of the library, 28 of
    them window tests, pass three runs in a row with no `SKIPPED`. With
    xclip 0.13 under Xvfb, xclip read a 1 MiB clipboard byte for byte, and
    `TARGETS` and `PRIMARY`, and a window pasted the 2,118,124 bytes xclip
    served by `INCR`. A C program built with gcc 13.3.0 and `-Werror`
    against libX11 1.8.7's headers checks the four new structs' sizes and
    every offset, 14 constants and the 9 prototypes. libX11's source could
    not be read (gitlab.freedesktop.org refuses automated reads), so a
    reviewer ran its `_XWireToEvent` on hand-built events: it writes every
    field of the four new event types that `decode` reads. clippy is
    clean for x86_64 and aarch64 Linux, `x86_64-apple-darwin` and
    `x86_64-pc-windows-msvc`, which were compiled, not run. The 71 Mermaid
    blocks of the three changed diagram files parse and render with
    Mermaid 11.4.1.
  - Note (2026-11-17): reviewed in three rounds, `security-reviewer` in
    each, every finding checked by two skeptics. Round one: 4 of 5
    confirmed and fixed (another client could stop every later copy by
    retyping the time stamp property, so it is written with a replace; an
    `INCR` paste had no overall deadline; a read relied on its caller; a
    paste's peak memory). Round two: 12 of 15 confirmed and fixed (floods
    of parts, requests or events held a wait for seconds, so a wait takes
    events in turns of 10 ms, reads a paste once a turn and serves at most
    64 MiB a second; a clear at the claim's own time gave a held selection
    up; a late answer was pasted for the next request, so each request
    has an answer property of its own; and 8 fixes to docs and
    declarations). Round three: 7 confirmed, 5 distinct, all fixed (a wake
    seen a turn late; a copy behind a flood returned before its claim; a
    stale write was taken for the claim's time, so the time stamp is told
    by its request number; the cap on transfers spent the serving budget;
    no test reached a resting answer slot). Each fix has a window test
    that fails with the fix taken out, but for the format-32 guard, which
    the old code also passed, and the wake after a drain, which a
    reviewer measured instead.
  - Note (2026-11-17): left open. `Event`'s derived `Debug` prints pasted
    text, as `InputEvent::Text`'s prints typed text: the skeptics split
    on it, and a redacting `Debug` would span `core` and `platform`. A
    late refusal of an earlier request names no property, so it can end
    the next paste of that selection with no text. Another client's server
    grab stalls the round trips to the server, as it stalls any Xlib call
    (ADR-0042).
  - Note (2026-11-17, after the tick): at the user's request, `Event`'s
    and `InputEvent`'s `Debug` now show a typed or pasted text's length,
    never the text, through `core::input::TextLength` (ADR-0043). The
    rest of the note above stays open. The window test
    `a_flood_of_events_holds_neither_a_wait_nor_a_wake` failed once in
    the gate under Xvfb: under load the server was still writing the
    events when the wait began. It now reads them all into Xlib's queue
    first, with a round trip, and passed the gate under Xvfb twice; it
    still fails without the drain's budget.

- [x] **M2.3 — gfx: rasterizer, blending, clipping, damage, backdrop**
  - Depends: M0.2
  - Decide first: accept ADR-0011 (CPU software rendering with damage
    tracking), which is `Proposed`.
  - Do: `framebuffer`, `raster`, `blend`, `clip`, `image`, `glyph`,
    `damage`, `backdrop` per `docs/subsystems/rendering.md`.
  - Accept: unit tests check exact pixels for every primitive; property
    tests: drawing inside the clip never writes outside it, and merged
    damage covers every changed pixel; the same draw list gives the same
    bytes on two runs. Golden frames follow in M2.4, with the harness.
  - Verify: `cargo test -p tidepool-gfx`
  - Note (2026-11-17): the user accepted ADR-0011 as written, which
    answers the `Decide first` line.
  - Note (2026-11-17): done. `gfx` has `framebuffer` (`Framebuffer`,
    which covers any rect of device space, at most `MAX_DIMENSION`,
    16,384 px, a side, and `View`), `clip` (`ClipStack`, `intersect`,
    `SPACE`), `blend` (`over`, `scale`, `composite`), `raster`
    (`fill_rect`, `hairline`, `border`, `brackets`, `polygon`, `line`,
    `cut_corner_rect`, `pill`), `image` (`copy`, `blit`, `blit_scaled`
    with `Filter`), `glyph` (`Mask`, `draw`), `damage` (`Damage`, at most
    8 rects, and `diff` by 16 px tiles), `backdrop` (`BackdropStyle` with
    the site's values, `Backdrop::ensure`, `base` and `blurred`) and
    `GfxError`. It is new, not ported: Nadir draws with OpenGL. Its one
    dependency is `core`. `polygon` and `line` are exact while their
    vertices stay within about 1e11 px of the framebuffer, as the docs
    and a test say; further out a slanted edge may come out wrong, but
    never outside the clip.
  - Note (2026-11-17): checked on rustc 1.99.0, Linux x86_64. `cargo test
    -p tidepool-gfx`: 73 unit tests, 6 property tests and the README's
    doc test pass. The unit tests check exact pixels for every primitive;
    the property tests check that no draw call writes outside its clip,
    whatever its geometry, that `diff` and merged `Damage` cover every
    changed pixel, and that a fixed draw list of every primitive, a layer,
    an image, a glyph mask and a backdrop gives the same bytes twice.
    `RUSTDOCFLAGS='-D warnings' cargo doc -p tidepool-gfx --no-deps` is
    clean, and so is clippy for `x86_64-apple-darwin`,
    `x86_64-pc-windows-msvc` and `aarch64-unknown-linux-gnu`, which were
    compiled, not run. That the bytes are the same on every OS rests on the
    arithmetic (integers, or IEEE 754 operations every CPU rounds the
    same); M2.4's goldens compare them across the CI runners.
  - Note (2026-11-17): `Backdrop::ensure` in a release build on the
    reference machine (M0.5), median of 5: 191 ms at 1920×1080 and scale
    1, 328 ms at 2560×1440 and scale 1, 370 ms at 2560×1440 and scale 2,
    772 ms at 3840×2160 and scale 2. The spec's raster benchmarks
    (`docs/subsystems/rendering.md` → Tests) are not part of this task
    and were not added.
  - Note (2026-11-17): reviewed in two rounds, each finding checked by
    two skeptics. Round one: 5 of 9 confirmed (one by one skeptic of
    two), all fixed: `Damage`'s equality read stale slots; mutants that
    passed every test showed exact tests missing for polygon edges across
    several cells, slanted lines and vertical bilinear scaling; and three
    rustdoc links did not resolve. Round two: 7 of 8 confirmed, all
    fixed: slanted polygons and lines with ends about 1e15 px away come
    out wrong, so the docs state the bound above; and 6 in docs and
    diagrams. A stress test of every public function with hostile input,
    about 3.8 million calls in debug and release, found no panic, hang,
    write outside the clip or unbounded allocation. The 94 Mermaid blocks
    of the four changed diagram files parse and render with Mermaid
    11.4.1.
  - Note (2026-11-22): ADR-0057 changed the backdrop after jabbott.dev's
    of 2026-11-23 (UTC): the purple and bronze glows in place of the teal
    and gold ones, no planet, and the site's bronze circuits and purple
    nerve cells, drawn still, from its `_includes/backdrop.html`, under
    limits on what a drawing may cost (`MAX_SHAPES` and the rest,
    `docs/subsystems/rendering.md`). The theme gained `bronze`,
    `bronze-hi`, `purple`, and `purple-hi`. `Backdrop::ensure` took 173
    and 177 ms at 1920×1080, scale 1, in a release build, against 193 and
    179 ms with the planet, alternating on a loaded machine. Two reviews
    found 5 Low issues, all fixed, and `gfx_backdrop` and
    `gfx_backdrop_scale_2` were blessed again once the owner had seen
    them.

- [x] **M2.4 — codec: QOI; xtask: golden harness; gfx goldens**
  - Depends: M2.3, M0.3, M1.6
  - Decide first: how `cargo xtask golden` draws its scenes. If it runs test
    binaries through cargo, that extends ADR-0017's list of `xtask`'s
    children.
  - Do: `codec::qoi` (port Nadir's) with a fuzz target; `cargo xtask golden
    [--bless]` per `docs/04-quality.md`; golden frames for every `gfx`
    primitive and the backdrop.
  - Accept: a changed pixel fails with diff images in `target/golden/`;
    QOI round-trips, and rejects malformed input without panicking.
  - Verify: `cargo xtask golden`; `cargo test -p tidepool-codec`
  - Note (2026-11-17): ADR-0044 answers the `Decide first` line with the
    options the user chose that day, each the recommended one: the scenes
    are tests in `crates/gfx/tests/golden.rs` that write PAM when
    `TIDEPOOL_GOLDEN_DIR` names a directory, and `cargo xtask golden`
    runs them with `cargo test` (one more child of ADR-0017's) and
    compares them with the QOI references; `golden` is a step of `ci`,
    after `test`, so every CI runner compares.
  - Note (2026-11-17): done. `codec::qoi` (`decode`, `encode`, `Header`,
    `Image`, `Channels`, `ColorSpace`), ported from Nadir, writes what
    `qoi.h` writes byte for byte and rejects what that decoder lets
    through (a run past the last pixel, chunks that end early, a wrong
    end marker, bytes after it); `Limits` gains `max_image_dimension`
    (16,384) and `max_decoded_bytes` (1 GiB), and `CodecError` `Qoi` and
    `QoiEncode`. Its fuzz target `qoi` has 17 seeds, `edge-*` ones at the
    small limits (64 px a side, 4 KiB of pixels). `cargo xtask golden
    [--bless <scene>...]` finds the drawing crates with `cargo metadata`
    (members with a `golden` test target, each with a row in `SCENES`),
    runs their scenes, compares each with `tests/golden/<scene>.qoi`
    within its crate's tolerance (0 for `gfx`), and writes `actual` and
    `diff` images of a failing scene; `--bless` rewrites only the scenes
    named. It is a step of `ci` after `test`. `crates/gfx/tests/golden.rs`
    has 14 scenes: each primitive, clipping, a layer, images, glyph masks,
    and the backdrop at 480 × 300, scale 1 and 2. The 14 references take
    96 KB. An independent QOI decoder (Python) read a reference back as
    exactly the bytes its scene drew.
  - Note (2026-11-17): checked on rustc 1.99.0, Linux x86_64. `cargo xtask
    golden`: 14 scenes ok, in about 0.8 s as a step of the gate. A scene
    changed by one column of pixels failed with 10 of 3,072 pixels and
    wrote `gfx_fill_rect.actual.qoi` and `gfx_fill_rect.diff.qoi`, the
    column in magenta; the scene was then restored. `cargo test -p
    tidepool-codec`: 277 tests pass, 21 of them QOI's vectors and
    property tests, and the README's and module's examples. `cargo xtask
    fuzz-smoke --target qoi --secs 30`: 13,710,316 inputs, no finding.
    `cargo xtask ci`: 12 of 12 steps pass, with 28 window tests skipped
    for want of `TIDEPOOL_IT_X11` (M2.9). Clippy is clean for
    `x86_64-apple-darwin`, `x86_64-pc-windows-msvc` and
    `aarch64-unknown-linux-gnu`, compiled, not run: that the scenes give
    the same bytes on macOS and Windows is checked by CI's gate on the
    next push (M0.4's ⏳). The 114 Mermaid blocks of `diagrams/` parse
    and render with Mermaid 11.4.1, which rejected a planted broken one.
  - Note (2026-11-17): reviewed. A security review of `golden` found 4 low
    findings, all fixed: images are created as new files and references
    renamed into place, so neither is written through a link or hard
    link; `TIDEPOOL_GOLDEN_DIR` is removed from every other cargo child;
    references are read one at a time; and a failed `--bless` write says
    which references were written. Two review rounds followed, each
    finding checked by two skeptics: round one, 15 of 18 confirmed (10
    after removing duplicates), all fixed: `--bless` skipped a scene that
    matched only within a tolerance above 0, a seed sat at a limit its
    doc said it was under, and 8 in docs; round two, 6 of 9 confirmed, all
    fixed: a temp file left by a failed write, and 5 in docs and diagrams.
    A separate check of the diagrams confirmed 3 of 5, all fixed.

- [x] **M2.8 — font: TrueType parser and outline rasterizer; bundled font files**
  - Depends: M0.2, M1.6
  - Do:
    - `ttf` and `outline` per `docs/subsystems/rendering.md`.
    - Add static Saira and JetBrains Mono `.ttf` files with their OFL
      license files to `assets/fonts/`; record versions and sources in
      `docs/02-dependency-policy.md` → Assets. The user downloads the files
      if this session can't.
  - Accept: both faces parse; malformed fonts return errors (fuzz target
    `ttf`); a glyph's coverage is identical across runs.
  - Verify: `cargo test -p tidepool-font`;
    `cargo xtask fuzz-smoke --target ttf --secs 60`
  - Note (2026-11-17): the user agreed to the download of the font files
    this session, fetched with `curl` from GitHub's raw files at a pinned
    commit and tag: Saira 1.101 (Omnibus-Type/Saira, `Saira/fonts/ttf/`
    at commit `c9d45ed`) Regular, Medium, SemiBold and Bold, and
    JetBrains Mono 2.304 (JetBrains/JetBrainsMono, `fonts/ttf/` at tag
    `v2.304`) Regular, Italic and SemiBold, each in `assets/fonts/` with
    its project's `OFL.txt`, unchanged and `-text` in `.gitattributes`.
    Both are SIL OFL 1.1 with no Reserved Font Name, as their `OFL.txt`
    and their `name` tables say. Versions, sources and SHA-256s are in
    `docs/02-dependency-policy.md` → Assets.
  - Note (2026-11-17): done. `ttf::Font::parse` takes the bytes, owned or
    `'static`, and checks against `font::Limits` everything its lookups
    read: the directory, `head`, `hhea`, `maxp`, `hmtx`, `loca`, one
    `cmap` subtable (format 12, else 4), and the optional `OS/2` (a
    68-byte version 0 too), `name`, `GPOS` and `kern`; CFF and
    collections are refused. A `Font` gives glyph ids, metrics, names, the
    `OS/2` style, kerning (`GPOS`'s `kern` feature of `latn`, else `DFLT`,
    else the first script: pair adjustment formats 1 and 2 and
    extensions; else the `kern` table) and outlines with composites
    resolved (offsets, scales, 2 × 2 matrices, point matching).
    `outline::rasterize` flattens to 1/32 px and accumulates exact-area
    coverage, as `gfx::raster`'s polygons do, into a `Bitmap` laid out as
    `gfx::glyph::Mask`. The limits' defaults: 64 MiB files, 65,536 points
    a glyph, composites 8 deep and 1,024 components, 512 kerning
    subtables kept, bitmaps 4,096 px a side, and 2^21 steps of work a
    glyph (under 100 ms, measured). `FontError` names the table or glyph
    and the byte offset; `ttf::ErrorKind` has 34 kinds and
    `outline::ErrorKind` 7, each reached by a hand-made font or outline in
    the tests. The fuzz target `ttf` has 18 hand-made seeds, `edge-*`
    ones at each of its small limits. Both faces parse, every glyph of the
    seven files reads and rasterizes, every composite is its components
    moved, and FNV-1a 64 hashes of each face's printable ASCII coverage at
    14 px (and Saira at 24 px, shifted 0.25 px) are pinned in
    `tests/bundled.rs`; FNV-1a, not SHA-256, which ADR-0016 keeps to
    downloads. In review (round 3), the kerning of every glyph pair of the
    four Saira faces matched an independent Python reader of `GPOS`, and
    that of every printable ASCII pair of 384 system TrueType faces, read
    through `latn`, matched HarfBuzz 8.3.0's, which a planted change of
    the class-0 rule made differ.
  - Note (2026-11-17): checked on rustc 1.99.0, Linux x86_64. `cargo test
    -p tidepool-font`: 127 tests pass (32 unit, 63 in `tests/ttf.rs`, 16
    in `tests/outline.rs`, 7 in `tests/bundled.rs`, 4 in `tests/fuzz.rs`,
    5 doc tests). `cargo xtask fuzz-smoke --target ttf --secs 60`:
    10,613,185 inputs, no finding, written memory +0.4 MiB of its 16 MiB
    cap, slowest input 0.59 ms. `cargo xtask ci`: 12 of 12 steps pass,
    with the 28 window tests skipped for want of `TIDEPOOL_IT_X11`
    (M2.9). Clippy is clean for `x86_64-apple-darwin`,
    `aarch64-apple-darwin`, `x86_64-pc-windows-msvc` and
    `aarch64-unknown-linux-gnu`, compiled, not run: that the pinned
    hashes hold on macOS and Windows is checked by CI's gate on the next
    push (M0.4's ⏳). The 115 Mermaid blocks of `diagrams/` parse and
    render with Mermaid 11.4.1, which rejected a planted broken one.
  - Note (2026-11-17): reviewed in four rounds, each finding checked by
    two skeptics, and all fixed. Round 1: 24 confirmed (20 after removing
    duplicates), among them `GPOS` checks quadratic in the table, composite
    points that could overflow `f32` under raised limits, `DFLT` read
    before `latn`, a 68-byte `OS/2` refused, a hostile glyph taking
    seconds to rasterize (hence `max_raster_work`), and test gaps. Round
    2: 23 confirmed (18): a padded `GPOS` held 30 times its size and made
    each kerning call slow (hence `max_kerning_subtables`), and a format 2
    pair with its second glyph in class 0 now gives way to the next
    subtable, as in HarfBuzz. Round 3: 14 confirmed (11): the `kern`
    table is now bounded by the same limit, the limit is checked before a
    subtable is, and the cost figures were re-measured. Round 4: 6
    confirmed (4): offsets kept from the table's start, so that a file
    over 4 GiB kerns right, an error message, and two tests.

- [x] **M2.5 — font: atlas, layout, fallback; glyph goldens**
  - Depends: M2.8, M2.4
  - Do: `atlas`, `layout`, `fallback` and `bundled` per
    `docs/subsystems/rendering.md`.
  - Accept: golden glyph runs in both faces at the listed sizes, at scale 1
    and 2, including uppercase letter-spaced labels.
  - Verify: `cargo test -p tidepool-font`; `cargo xtask golden`
  - Note (2026-11-17): done. `bundled` compiles the seven faces in with
    `include_bytes!` (ADR-0004's "the binary grows by the font files") and
    `bundled::load` parses them into a `fallback::Faces`, 0.04 ms in a
    release build. `Faces` holds at most 64 faces by `FaceId`; a character
    is drawn by the face asked for, then the chain (JetBrains Mono, then a
    system face the caller adds), then that face's `.notdef`, a box this
    crate makes when glyph 0 is empty, unreadable, flat, more than an em
    outside the em square, or cannot be rasterized. `layout::line`,
    `truncate` and `wrap` set text in a `Style` (face, size in 1/64 px,
    letter-spacing in ems, clamped to ±64, tab size) in device pixels:
    advances, kerning within a face, letter-spacing between glyphs, tab
    stops, an ellipsis, and breaks at LF, CR LF and CR (ADR-0019) and after
    spaces, which hang. `atlas::Atlas` keeps glyphs keyed by face, glyph,
    size and quarter pixel on shelves of one coverage image, laid out as
    `gfx::glyph::Mask`; when full it keeps the most recently used half and
    repacks; a glyph is at most half its shorter side, refused from its
    control box, and a failed key is remembered (up to 1,024). Filling a
    512 × 512 atlas with Saira's 95 printable ASCII glyphs at 14 px took
    0.27 ms in a release build (a probe, not a suite). The 8 golden scenes
    in `crates/font/tests/golden.rs` draw both faces at 11 to 24 px, the
    uppercase letter-spaced labels of the typography table with Saira's
    four weights, and fallback, truncation, wrapping and tabs, each at
    scale 1 and 2, through the atlas, blending the coverage themselves, as
    ADR-0044 left them to (no `font` → `gfx` edge); `SCENES` gains
    `tidepool-font` with a tolerance of 1. The user looked at the eight
    images and agreed to bless them; their references take 456 KB.
  - Note (2026-11-17): checked on rustc 1.99.0, Linux x86_64. `cargo test
    -p tidepool-font`: 174 tests pass (37 unit, 11 in `tests/atlas.rs`, 7
    in `tests/bundled.rs`, 4 in `tests/fuzz.rs`, 8 scenes in
    `tests/golden.rs`, 16 in `tests/layout.rs`, 16 in `tests/outline.rs`,
    63 in `tests/ttf.rs`, 12 doc tests). `cargo xtask golden`: 22 scenes
    ok, the 8 of `font` exactly. With the scenes' text color changed by 2,
    `font_labels` failed with 2,400 of 108,000 pixels and wrote `actual`
    and `diff` images; changed by 1, `font_saira` passed within the
    tolerance; then the scenes were restored. `cargo xtask
    fuzz-smoke --target ttf --secs 60`, its target now also setting text
    in each font that parses, through layout and an atlas: 4,570,005
    inputs, no finding, written memory +0.5 MiB of its 16 MiB cap, slowest
    input 0.66 ms. Clippy is clean for `x86_64-apple-darwin`,
    `aarch64-apple-darwin`, `x86_64-pc-windows-msvc` and
    `aarch64-unknown-linux-gnu`, compiled, not run: that the scenes give
    the same bytes on macOS and Windows is checked by CI's gate on the next
    push (M0.4's ⏳). The 116 Mermaid blocks of `diagrams/` parse and
    render with Mermaid 11.4.1, which rejected a planted broken one.
  - Note (2026-11-17): reviewed by `security-reviewer`: no panic, no
    `unsafe`, dependency or matrix change; 1 medium and 4 low findings,
    all fixed. Medium: a hostile system face's glyph was rasterized in
    full, up to 16 MiB and 2^21 steps, and refused or evicted again on
    every call; now the atlas caps glyphs at half its side before
    rasterizing and remembers failed keys. Low: a huge finite
    letter-spacing gave infinite positions (now clamped, and
    `Subpixel::split` keeps pixels within ±2^30); a face's own `.notdef`
    could fail to draw (now a box); `clear` left old coverage (now zeroed);
    and the fuzz target did not reach the new code (now it does). Not
    done: a benchmark suite for the atlas fill, which
    `docs/subsystems/rendering.md` lists and no task's `Do` names.
  - Note (2026-11-17): the atlas fill that `docs/subsystems/rendering.md`
    lists is `tidepool-bench`'s suite `font/atlas/fill_ascii/14px`: Saira
    Regular's 95 printable ASCII glyphs at 14 px into an empty 256 × 256
    atlas, each iteration from a new one, after an untimed check that the
    fill holds every glyph as `atlas::rasterize` draws it. On the
    reference machine, with other programs loading it (load average 3.3
    to 5.2, 14 GB of swap in use), `cargo xtask bench font` gave medians
    of 345 to 424 µs over nine runs, two of the slower ones (424 and 397
    µs) with p95s of 629 and 700 µs; `--save`, a run of `font` alone,
    recorded 345 µs (p95 361 µs, 481 samples), and the runs after it
    measured 355, 397 and 390 µs (+2.9%, +15.1% and +12.8%), so two of the
    three were marked. A 512 × 512 atlas, tried first, gave 369 to 442 µs
    over six runs under the same load; 256 × 256 holds all 95 glyphs with
    no eviction, which the suite checks.

- [x] **M2.6 — gui: first frame**
  - Depends: M2.2, M2.4, M2.5
  - Do: `gui` opens a window and draws the backdrop and a line of each
    face; `cargo tp` runs it.
  - Accept: cold start to first frame within the M0.5 budget on the
    reference machine; idle CPU ~0%.
  - Verify: `cargo tp -- --print-startup-time`; manual look
  - Note (2026-11-17): M2.3 measured `Backdrop::ensure` at 191 ms for
    1920×1080 at scale 1 in a release build on the reference machine
    (M2.3's notes), against the 300 ms budget from cold start to the
    first frame; how the first frame meets the budget with the backdrop
    is this task's to settle.
  - Note (2026-11-17): ADR-0045 settles it with the options the user
    chose that day, each the recommended one: `gui` builds the backdrop on
    the main thread once the window is exposed, at the size it has then;
    `tidepool-gfx` and `tidepool-font` are optimized (`opt-level = 3`) in
    the dev profile, where `Backdrop::ensure` took 3.9 s, so `cargo tp`
    meets the budget; the text's two colors are stand-ins for the `text`
    and `text-3` tokens until M3.2; and the frame shows a line of each of
    the seven bundled faces.
  - Note (2026-11-17): the `tp` alias was `run --package tidepool --`, so
    `cargo tp -- --print-startup-time`, as this task's `Verify` line and
    `AGENTS.md` write it, handed the program a `--` before the flag, which
    then read as a file. The user chose that day to drop the alias's `--`
    (`.cargo/config.toml`, `docs/01-architecture.md`), so `cargo tp --
    <arguments>` is plain `cargo run`.
  - Note (2026-11-17): done. `tidepool-gui` has `Gui` (`open`, `size` and
    `turn`, with its `Turn`), `first_frame::FirstFrame`, the stand-ins for
    `text` and `text-3` in `tokens`, checked against the spec's token
    table, and `GuiError`. The `tidepool` binary opens the window at
    `WindowConfig::default()`'s 1280 × 800 logical px and turns until it is
    asked to close; `--print-startup-time` prints `first frame: <ms> ms`,
    from the start of `main` to the return of the first `present`, and any
    other argument exits 2. COSMIC resizes the new window to 1232 × 662
    about 1 ms after its first expose, while the first frame is being
    drawn, so the frame is drawn twice at startup: at 1280 × 800,
    presented 117 to 181 ms after `main` began, and at 1232 × 662, 207 to
    274 ms (a scratch probe, three runs). A draft took the events already
    waiting before drawing; that missed the resize, since `wait_events`
    already returns everything waiting, and was removed.
  - Note (2026-11-17): checked on rustc 1.99.0, on the reference machine
    under other load (load average 3.7 to 5.7). `cargo tp --
    --print-startup-time`, a dev build with `gfx` and `font` optimized:
    120.4 to 188.7 ms over 10 runs of the final code, and 122.6 to
    165.9 ms over 10 runs before the review's fixes. A release build,
    timed by a script from its `exec` to the line it prints: 121.9 to
    179.6 ms over 10 runs, 0.9 to 1.3 ms of it before `main` and in the
    pipe; earlier, 116.4 to 201.8 ms from `main` over 10 runs. Each is
    within the 300 ms budget. Idle, an open window used 0 CPU ticks in
    10 s, its one thread asleep in `poll`. The user looked at the window
    that day and found it right, and its close button ended the process
    with status 0. `cargo xtask ci`: 12 of 12 steps pass, with 29 window
    tests skipped for want of `TIDEPOOL_IT_X11`; `TIDEPOOL_IT_X11=1
    xvfb-run -a cargo xtask ci`: 12 of 12, none skipped, `gui`'s window
    test included; its `golden` step passes with `gfx` and `font`
    optimized in the dev profile. `RUSTDOCFLAGS='-D warnings' cargo doc
    -p tidepool-gui -p tidepool --no-deps` is clean, and so is clippy for
    `x86_64-apple-darwin`, `x86_64-pc-windows-msvc` and
    `aarch64-unknown-linux-gnu`, compiled, not run.
  - Note (2026-11-17): reviewed by a subagent: no fault found in the
    turn's handling of events or the glyphs' placement. Fixed from its
    findings: `cargo fmt`, a rustdoc link to a private module, and a
    window over `gfx`'s 16,384 px a side ending the program (the frame is
    now clamped to it, and `present` clips); and tests added for the
    turn's events, a line's quarter-pixel placement, and the binary's
    command line run as a process; and the binary's messages escape
    bidirectional controls and line separators too, as `xtask`'s do.
  - Note (2026-11-17): the docs and diagrams this task changed were
    checked against the code by four readers, each finding put to two
    skeptics: 22 findings, none refuted by both, all fixed. Among them,
    stale sentences M2.6 made untrue (`diagrams/`' section 2 intro on who
    declares dependencies, the frontends' present tense, 8.1's backdrop
    built while opening, the macOS and Windows window tests, M2.9's
    count of 29 window tests), ADR-0045's text on the resize, now drawn
    on the next turn, and on literal colors, now `gui`'s only ones, which
    the user kept that day, and
    `site/llms.txt`'s layers missing M2.5. The 118 Mermaid blocks of
    `diagrams/` parse and render with Mermaid 11.4.1, which rejected a
    planted broken one.

**Exit criteria (M2):** `cargo tp` opens a Linux window with crisp Saira and
JetBrains Mono text over the jabbott.dev backdrop.

---

## M3 — UI toolkit in the jabbott.dev style

**Goal:** the shell of the IDE, looking like jabbott.dev.

- [x] **M3.1 — ui: tree, layout, focus, input routing, accessibility tree**
  - Depends: M0.2
  - Do: `tree`, `layout`, `focus`, `input`, `metrics`, `draw`, `a11y` per
    `docs/subsystems/ui-and-theme.md`.
  - Accept: layout property tests; every focusable node has a role and name.
  - Verify: `cargo test -p tidepool-ui`
  - Note (2026-11-17): the spec left open who runs a widget's code and
    what color a draw-list command carries. ADR-0046 records the options
    the user chose that day, each the recommended one: widgets live in
    the tree's nodes, which tracks their interaction state and routes
    events up through their ancestors; and draw lists carry resolved
    `core::Color`s.
  - Note (2026-11-18): done. `tidepool-ui` depends on `tidepool-core` and
    has `tree` (`Tree`, generational `NodeId`s, `Node`, `Widget`,
    `NodeState`, at most 256 levels and 2^20 nodes), `layout` (rows,
    columns, stacks, grids and splits; fixed, natural and filling lengths
    whose limits follow flexbox's freeze rule; sanitized values; exact
    containment in `f32`; scrolling, with `scroll_into_view` applied at
    the next layout), `focus` (the tab order and `:focus-visible`),
    `input` (`Tree::handle` and `hit_test`; Tab and the wheel as
    defaults; hover kept on the pointer as content moves), `metrics`
    (`TextMetrics`, `FixedMetrics`), `draw` (`DrawList`, `Command`,
    `Frame`: repaints only changed nodes, damage from their fragments,
    `None` when idle) and `a11y` (`Snapshot`); the tree refuses a
    focusable node without a role and a name. Choices the spec left open
    are in the modules' rustdoc: a click on a node that takes no focus
    leaves focus where it was; the root holds no widget; a painted text
    and a node's value show in `Debug` as their length (ADR-0043). No
    roadmap task builds the spec's `anim` module or the icon set that
    numbers `IconId`s.
  - Note (2026-11-18): reviewed by six reviewers, one a security reviewer,
    each finding put to two skeptics: 22 of 28 findings survived, 13
    distinct. All were fixed in three rounds, each checked by its own
    verifiers: `scroll_into_view` measured from stale bounds (three Tabs
    between frames scrolled the focus out of view), the fill freeze loop,
    a content-sized grid's width, a hidden root, the wheel starting from
    an unclamped offset, hover left on a row scrolled away, `Command`'s
    `Debug` printing text, NaN commands reaching the list but not the
    damage, the paint queue growing between frames, the measuring memo's
    size, a test that could not fail, and two docs. The docs and diagrams
    this task changed were checked against the code by four readers, each
    finding put to two skeptics: 21 of 28 survived, all fixed.
  - Note (2026-11-18): checked on rustc 1.99.0, Linux. `cargo test -p
    tidepool-ui`: 95 passed (35 unit, 3 a11y, 11 frame, 19 input, 22
    layout, 5 doc tests). `cargo xtask ci`: 12 of 12 steps pass, with the
    29 window tests skipped for want of `TIDEPOOL_IT_X11`;
    `TIDEPOOL_IT_X11=1 xvfb-run -a cargo xtask ci`: 12 of 12, none
    skipped. `RUSTDOCFLAGS='-D warnings' cargo doc -p tidepool-ui
    --no-deps` is clean. The 120 Mermaid blocks of `diagrams/` parse and
    render with Mermaid 11.17.2, which rejected a planted broken one.
    `ui` has no OS code, so nothing was built for macOS or Windows.

- [x] **M3.2 — ui: theme tokens and `xtask theme`**
  - Depends: M3.1, M1.7
  - Do: `themes/tidepool-dark.toml` with the token table; `theme` module;
    `cargo xtask theme` contrast check.
  - Accept: the contrast table in the spec is reproduced exactly; a token
    change that breaks AA fails the check.
  - Verify: `cargo xtask theme`
  - Note (2026-11-17): `tidepool-gui` holds stand-ins for the `text` and
    `text-3` tokens from M2.6 (ADR-0045), its only literal colors; this
    task replaces them with the theme's.
  - Note (2026-11-18): the task has no `Decide first` line, but the spec
    left open two questions that change the dependency matrix and a file
    format. ADR-0047 records the options the user chose that day, each the
    recommended one: `ui` reads theme files with `codec::toml`
    (`tidepool-ui`'s row is now `core`, `codec`), and a theme file lists,
    in `[contrast]`, the pairs it uses for text, which `cargo xtask theme`
    checks.
  - Note (2026-11-18): done. `themes/tidepool-dark.toml` (version 1) holds
    the spec's 23 color tokens, with their solid values and 256-color
    indices, `cut` and the two fonts, and three colors the contrast rules
    name, `on-amber`, `on-teal` and `comment`, which join the spec's token
    table; its `[contrast]` names the table's 7 rows and 4 columns, and 10
    `[[contrast.use]]` entries list 27 pairs, the editor's selection
    (`text` on `teal-glow` over `editor-bg`) among them. `ui::theme` reads
    it (`Theme`, `Token`, `TokenColor`, `ThemeError`, `ThemeErrorKind`;
    `UiError::Theme` and `ThemeSyntax`), refusing each rule's breach with
    the key and its line, and bundles it (`Theme::tidepool_dark`); it has
    a property test and a fuzz target, `theme`, with seeds in
    `fuzz/corpus/theme/` and `themes/` read in place (`-text` in
    `.gitattributes`). `cargo xtask theme`, the last of `ci`'s 13 steps,
    prints the spec's contrast table, every value the same to two
    decimals, which a test compares byte for byte, and fails a pair below
    its minimum; `gui`'s stand-ins are gone, and its first frame takes
    `text` and `text-3` from the theme. The component styles derived from
    tokens, and how a widget reaches the theme when it paints, are left to
    the controls (M3.6, M3.3), as ADR-0047 says.
  - Note (2026-11-18): found on the way: `core`'s source-over, which keeps
    alpha in 8 bits and rounds premultiplied channels, composites `panel`
    over `bg` as `#0e1213`, not the spec's `#0f1113`, on which `text` is
    16.71:1, not 16.79; `line`, `teal-wash` and `bar` are each a level off
    in a channel or two too. The check takes a written `solid` for a
    surface over `bg`, as the table's `(solid)` column says, and holds
    each within one level a channel of `core`'s composite (ADR-0047).
  - Note (2026-11-18): reviewed by four reviewers and a security reviewer,
    each finding of the four put to two skeptics: 10 of 23 findings were
    upheld by both and 4 by one, all fixed: a blank name or font
    accepted, a dotted key shown as a key of the format, follow-on
    violations after a color that does not read, and tests that could not
    fail (the selection's `over`, the solid's one-level limit, the
    unrounded comparison, parts of the name rule, the token set, rules of
    `[contrast]`), and three doc comments. The security review found two
    Low points and two nits, all fixed: `xtask theme` counted lines from
    the start of the file for each violation (a 1 MiB file with 120,450
    violations now takes 0.40 s, where a similar one took 54.9 s), and the
    reader had no fuzz target, which it now has.
  - Note (2026-11-18): checked on rustc 1.99.0, Linux. `cargo xtask
    theme`: ok, 27 of 27 pairs at their minimum or above, and the table
    the spec's byte for byte. Before the reviews' fixes, `cargo xtask ci`
    passed 13 of 13 steps, with the 29 window tests skipped for want of
    `TIDEPOOL_IT_X11`, and `TIDEPOOL_IT_X11=1 xvfb-run -a cargo xtask ci`
    13 of 13, none skipped. After them: `cargo test -p tidepool-ui` (42
    unit, 3 a11y, 11 frame, 2 fuzz, 19 input, 22 layout, 2 theme, 6 doc
    tests), `xtask`'s theme tests (12) and `tidepool-gui`'s unit tests
    (13) pass; clippy with `-D warnings` is clean; `cargo xtask
    fuzz-smoke --target theme` ran clean for 60 s twice, 562,108 inputs
    and, with the same seed, 1,831,565, the slowest 11.26 ms (the first
    run's 701.82 ms was a stall at a load average near 30, which the same
    inputs did not repeat). The 122 Mermaid blocks of `diagrams/` parse
    and render with Mermaid 11.17.2, which rejected a planted broken one.
    `tidepool-gui` takes 0.23 ms in a dev build to parse the bundled
    theme; the first frame under Xvfb took 108.6 to 168.4 ms over nine
    runs and 314.4 ms in one, at a load average near 5 while the reviewers
    built (the closing runs, below, measured it again).
  - Note (2026-11-18): the closing runs, on the final code. A first
    closing gate's `test` step stopped at a test of `xtask` whose expected
    list of fuzz targets lacked `theme`, then fixed; the disk had filled
    under other work on the machine. With space again: `cargo test
    --workspace`: 1,917 passed in 86 test binaries, the 29 window tests
    skipped; `cargo xtask ci`: 13 of 13 steps pass, with those 29
    skipped; `TIDEPOOL_IT_X11=1 xvfb-run -a cargo xtask ci`: 13 of 13,
    none skipped. `RUSTDOCFLAGS='-D warnings' cargo doc -p tidepool-ui -p
    tidepool-gui -p tidepool-gfx --no-deps` is clean. The first frame
    under Xvfb took 104.6 to 145.2 ms over 10 runs at a load average of
    4 to 5, within the 300 ms budget. `ui` has no OS code, so nothing was
    built for macOS or Windows.

- [x] **M3.5 — gui: draw-list painter and ui goldens**
  - Depends: M3.1, M2.6
  - Do: `gui` paints every `ui::draw` primitive with `gfx` and `font`, and
    implements `TextMetrics` with `font::layout`; a test binary renders
    `ui` scenes for `cargo xtask golden`.
  - Accept: a golden scene for each draw primitive at scale 1 and 2; a
    frame changes only pixels inside its damage.
  - Verify: `cargo xtask golden`; `cargo test -p tidepool-gui`
  - Note (2026-11-18): the task has no `Decide first` line, but no task
    builds the icon set that numbers `IconId`s, and an `ImageId` is a
    handle the frontend resolves. ADR-0048 records the options the user
    chose that day, each the recommended one: icons are outlines
    registered with the painter and filled by `font`'s rasterizer, and
    images are premultiplied framebuffers registered with it and scaled
    bilinearly.
  - Note (2026-11-18): done. `gui::paint::Painter` paints a `ui` frame
    into a framebuffer, its damage only (rounded out, at most 8 rects,
    cut to the target and a base it copies in first), with `gfx` and
    `font`: every command, clips, layers (each a framebuffer as large as
    what it inks, grown by at least its size, at most 8 nested), icons and
    images. A rect's edges round to the nearest device pixel, borders and
    bracket thickness snap as CSS snaps a border width, and each
    command's ink is clipped to its bounds cut to its clips, so a frame
    touches only its damage. Lines and cut corners are filled in 64 px
    tiles fixed in device space, as `gfx`'s path fill sums coverage from
    the corner of its clip, so painting the damage gives exactly what
    painting the whole frame gives. `gui::metrics::FontMetrics` is `ui`'s
    `TextMetrics` with `font::layout`, measured at the scale painted;
    `gui::icon::Icon` holds an outline in a square viewbox, y down.
    `ui`'s grid now places each column at the sum of the one before, as
    its rows were, so that grid cells share their edges at any scale (a
    12-cell grid 121 units wide left column 60 unpainted at scale 1).
    `crates/gui/tests/golden.rs` has 23 scenes, `ui_*`: each command,
    clips and layers at scale 1 and 2, and a mix at 1.5; `SCENES` gains
    `tidepool-gui` with a tolerance of 0. The user looked at the 23
    images and agreed to bless them; their references take 216 KB.
    `crates/gui/tests/paint.rs` paints random `ui` trees changed frame by
    frame at scales 1 to 2 and checks that each frame changes only its
    damage and that a shadow tree painted whole after every step, idle
    ones too, gives the same pixels; a second property aims slivers of
    anti-aliased ink past a clipping node's edge. All 600 seeds tried
    passed both; without the tiles, seeds 36, 109, 275 and 278 of the
    first failed. In a release build at 2560 × 1600, a full paint of 12
    small faded nodes took about 2 ms, where a layer as large as its clip
    had taken 328 ms.
  - Note (2026-11-18): reviewed in two rounds, each finding put to two
    skeptics. Round one, five reviewers (the painter, metrics and icons,
    the tests, a security review, the docs against the code): 15 of 18
    upheld, all fixed: seams where `ui` places neighbours (edges summed
    in `f64`), a layer the size of its clip, a 4,096 px icon drawing
    nothing, an icon's error reported as text's, a NaN cut drawing a
    rect, the damage grown for no reason, three tests that could not
    fail, and five docs. Round two checked the fixes, 13 of 14
    confirmed, and found 7 more upheld: grid seams, a layer copied once a
    row, painting the damage differing by a level from painting the whole
    on anti-aliased paths, a NaN text origin drawn at the window's edge,
    tiny icon viewboxes, and two weak tests; all fixed.
  - Note (2026-11-18): the docs and diagrams this task changed were
    checked against the code by four readers, each finding put to two
    skeptics: 16 of 24 upheld, all fixed (ADR-0048's account of the tiles
    and of what fails `paint`, the layers' size, `ui`'s draw docs, and
    golden passages and sources in the diagrams that still named `gfx`
    and `font` alone).
  - Note (2026-11-18): checked on rustc 1.99.0, Linux. `cargo xtask
    golden`: 45 scenes ok, the 23 of `gui` exactly. `cargo test -p
    tidepool-gui`: 53 unit tests, 23 scenes, 2 properties and 6 doc tests
    pass, its window test skipped for want of `TIDEPOOL_IT_X11`. `cargo
    xtask ci`: 13 of 13 steps pass, 2,031 tests, with the 29 window tests
    skipped; `TIDEPOOL_IT_X11=1 xvfb-run -a cargo xtask ci`: 13 of 13,
    none skipped. `RUSTDOCFLAGS='-D warnings' cargo doc -p tidepool-gui
    -p tidepool-ui --no-deps` is clean. The 123 Mermaid blocks of
    `diagrams/` parse and render with Mermaid 11.17.2, which rejected a
    planted broken one. `gui` has no OS code in the painter, so nothing
    was built for macOS or Windows: that the scenes give the same bytes
    there is checked by CI's gate on the next push (M0.4's ⏳).

- [x] **M3.6 — ui: controls**
  - Depends: M3.2, M3.5
  - Do: from the spec's Components table: buttons (ghost, teal, amber with
    cut corners, slate), chips, text input, search field, scrollbar, the
    focus outline, status dot, section title, eyebrow, kicker and
    breadcrumbs.
  - Accept: golden frames for every widget in each state (rest, hover,
    focus, active, disabled) at scale 1 and 2; each has a role and name in
    the accessibility tree.
  - Verify: `cargo xtask golden`; `cargo test -p tidepool-ui`
  - Note (2026-11-18): the task has no `Decide first` line, but ADR-0047
    left to the controls how a widget reaches the theme and the component
    styles, which change `ui`'s API, the theme file and the dependency
    matrix. ADR-0049 records the options the user chose that day, each
    the recommended one: the tree holds the theme; the colors the site's
    components use beyond its `:root` become eight theme tokens; `ui`
    gains `text` for editing by grapheme cluster (its row is now `core`,
    `codec`, `text`); and pressed and disabled looks, with no blurred
    glows.
  - Note (2026-11-18): done. `ui::widgets` has `Button` (ghost, teal,
    amber cut by the theme's `cut`, slate; 30 or 24 high), `Chip`,
    `TextInput` (a text input or the search field: one line of at most
    4,096 bytes moved in and deleted by grapheme cluster, with selection,
    a caret placed by the stops of the last paint, and a secret mode),
    `ScrollBar`, `focus_outline`, `StatusDot`, `SectionTitle`, `Eyebrow`,
    `Kicker`, and `Breadcrumbs`, which builds a row of `Crumb`s. Each
    control is a `Control`, which gives its node its role and the name it
    is given, changed with `widgets::update`. The tree holds an
    `Arc<Theme>` (`Tree::theme`, `set_theme`, `Theme::bundled`) that
    `PaintContext::theme` gives widgets; `EventContext` gains
    `set_semantics` and `focus_moves`; `UiError` gains `WrongWidget`; a
    fragment's painted bounds are cut to the clips it pushes. Buttons,
    chips and links activate on a click, on Enter going down, and on
    Space coming up after it went down on them, unless focus moved, the
    window lost it or another key went down between. The theme has 34
    color tokens and 40 text pairs, all at AA. `crates/gui/tests/
    controls/mod.rs`, a module of the `golden` target, paints 16 scenes,
    `ui_*`: a grid for each control, a row for each variant and a column
    for each state, every cell its own tree put in its state by real
    events, at scale 1 and 2. The user looked at two contact sheets of the
    16 images and agreed to bless them; their references take 424 KB.
    Besides `ui` and `gui`'s tests, the change touches `xtask`'s theme
    tests (the pairs they count), a doc comment of `gfx` and `text`'s
    README.
  - Note (2026-11-18): reviewed in four rounds, each finding of the last
    three put to two skeptics. A first reviewer found 6, all fixed: a
    click after an edit used caret stops of the old text and put the
    caret inside a char, so the next edit panicked; Space's typed space
    went to the owner; `widgets::update` and `Breadcrumbs::insert` left
    a blank name or a row behind on error; and a long field damaged past
    its clip. Four finders then found 17, 15 upheld by both skeptics, all
    fixed: Space activating on its key going down, so the space it typed
    reached a field the owner focused on activation; a release over
    another node activating the pressed control; Ctrl+Shift+A taken as
    Ctrl+A; the scrollbar overflowing on huge ranges; `teal-line`'s solid
    a level off; breadcrumbs leaving a separator behind; tests that
    could not fail or checked one control's colors only; and six docs.
    The fixes' check found 9 more upheld, all fixed: a Space release the
    platform reports after the window lost focus, or after focus left and
    came back, activating (the tree now counts focus moves), Shift
    calling Space off, four weak tests and four docs. A last check found
    3, fixed. Numpad keys with Num Lock off are a documented limit: a
    field keeps them as keys that type text, since `core`'s keys name
    places, not what Num Lock makes of them.
  - Note (2026-11-18): checked on rustc 1.99.0, Linux. `cargo xtask
    golden`: 61 scenes ok, the 16 new ones exactly. `cargo test -p
    tidepool-ui`: 42 unit tests, 3 a11y, 12 frame, 2 fuzz, 19 input, 22
    layout, 2 theme, 28 widgets and 15 doc tests pass; a property test of
    random edits, clicks and frames fails at its 49th case with the stale
    stops' fix reverted, and each fix of the reviews was reverted once to
    see its test fail. `cargo xtask theme`: 40 of 40 pairs at AA, the
    table the spec's. `cargo xtask fuzz-smoke --target theme`: 2,213,999
    inputs in 60 s, clean. `cargo xtask ci`: 13 of 13 steps pass, 2,101
    tests, with the 29 window tests skipped; `TIDEPOOL_IT_X11=1 xvfb-run
    -a cargo xtask ci`: 13 of 13, none skipped. `RUSTDOCFLAGS='-D
    warnings' cargo doc -p tidepool-ui -p tidepool-gui -p tidepool-gfx -p
    tidepool-text --no-deps` is clean. The 124 Mermaid blocks of
    `diagrams/` parse and render with Mermaid 11.17.2, loaded from
    jsdelivr in the app's browser, which rejected a planted broken one;
    the 14 SVGs of `site/index.html` parse as XML. `ui` has no OS code,
    so nothing was built for macOS or Windows: that the scenes give the
    same bytes there is checked by CI's gate on the next push (M0.4's ⏳).
  - Note (2026-11-22): ADR-0058 changed the buttons' look after
    jabbott.dev's header of 2026-11-23 (UTC): every kind is a tile with
    bracket corners in its accent (ghost and slate neutral, teal, amber),
    which lights when hovered, pressed, or focused, and rises only when
    hovered, and the amber primary
    no longer cuts its corners. The sizes stay 30 and 24. The theme gained
    eight tokens for the tiles' labels, lines, and glows. Two reviews found
    7 Low issues, all fixed; the main one, a button lifted by keyboard
    focus covering the one above where a click reached that one, made the
    lift hover's alone. The 12 scenes that show a button (`ui_buttons`,
    `ui_card`, `ui_modal`, `ui_palette`, `ui_toast`, `ui_tooltip`, and
    their `_scale_2`) were blessed again at the owner's word, once the
    fixes had landed.

- [x] **M3.7 — ui: collections**
  - Depends: M3.6
  - Do: from the spec's Components table: list, tree view, table, tab
    strip, splitter, card with its tab and code block. The list, tree
    view and table keep nodes only for the rows in view, over a model
    their owner provides (ADR-0052).
  - Accept: as M3.6, plus keyboard-navigation tests for the list, tree
    view and table, and a test that a collection of 1,000,000 rows keeps
    nodes only for the rows in view and a margin.
  - Verify: `cargo xtask golden`; `cargo test -p tidepool-ui`
  - Note (2026-11-20): split from M3.3 (ADR-0052), which keeps the
    overlays.
  - Note (2026-11-20): done. `ui::widgets` has:
    - `Collection`, a list, tree view or table (`CollectionKind`) over a
      model of `Rows` its owner provides, rows 20 high. It keeps nodes
      only for the rows on screen, cut to its clipping ancestors, and two
      either side, at most `MAX_VIEW_ROWS` (512) and two, in a body of
      fixed-height gaps that never scrolls, and asks the model for a row
      only as it comes into view.
    - A selection of one row moves with the arrow, page, Home and End
      keys and a press, and is scrolled into view the least, by the
      collection and by the scrollers around it. A tree view's Left and
      Right collapse, expand or move by level, as the WAI-ARIA tree
      pattern does.
    - A table names its rows by the cells its columns show, each capped
      at `MAX_CELL_BYTES`, and its column headers take focus and activate
      for the owner to sort.
    - `TabStrip`: one tab stop, wrapping arrows, Home, End, Delete and
      close controls, chrome or content labels, a track that scrolls to
      the selected tab. `Splitter`: the divider of a `Layout::Split`,
      dragged or stepped within the panes' minimums, keeping the ratio it
      was given. `Card`, with its right-edge tab. `CodeBlock`: spans in
      the theme's tokens, within line, byte and span caps, with a COPY
      button for its owner.
    - `ActionKind` gains `Select`, `Expand` and `Collapse`. `Tree::layout`
      binds these widgets until they settle, at most eight times, before
      it takes their reveals. The theme gains `table-head`, `row-hover`,
      `card-tab`, `on-card-tab` and `code-bar`, with 55 text pairs at AA.
      `crates/gui/tests/components/mod.rs` paints 14 scenes, `ui_list` to
      `ui_code_block_scale_2`, each a row for each variant and a column
      for each state, at scale 1 and 2. The user looked at a contact sheet
      of them and asked for them to be blessed; their references take
      1.3 MB.
  - Note (2026-11-20): reviewed in three rounds. Each round had a
    correctness reviewer and `security-reviewer`, and every finding was
    proven with a probe before it was fixed:
    - The list, tree view and table: 14 findings, 11 once those both
      reviewers found are counted once. The main one: inside a
      scrolling pane a collection kept a node for every row, up to
      50,005, a press could select a row other than the one drawn, and a
      64-column table filled the tree.
    - The tab strip, splitter, card and code block: 18 findings, 14 once
      counted once. Among
      them: a press on a tab removed before the next frame, a splitter
      that lost its ratio for good after a resize, and an uncapped code
      language.
    - The fixes of the second round: 4 findings and 2 nits, the main one
      a reveal lost when a split re-clamped in the same turn.
    All are fixed. Each fix was reverted once to see its test fail, and
    each review's probes were run again on the fixed code. Left as they
    are, the owner chose on 2026-11-21: a row's accessible name shows in
    full in `Semantics`'s `Debug`,
    as M3.1's names do, and `Collection`'s name and the M3.6 controls'
    names are not capped.
  - Note (2026-11-20): checked on rustc 1.99.0, Linux:
    - `cargo xtask golden`: 75 scenes ok, the 14 new ones blessed from
      images byte-identical to those the user looked at.
    - `cargo test -p tidepool-ui`: 63 unit tests, 3 a11y, 30
      collections, 28 components, 12 frame, 2 fuzz, 19 input, 22 layout,
      4 settling, 2 theme, 28 widgets and 27 doc tests pass.
    - `cargo xtask theme`: 55 of 55 pairs at AA.
    - `cargo xtask ci`: 13 of 13 steps pass, with the 29 window tests
      skipped; `TIDEPOOL_IT_X11=1 xvfb-run -a cargo xtask ci`: 13 of 13,
      none skipped. `RUSTDOCFLAGS='-D warnings' cargo doc -p tidepool-ui -p
      tidepool-gui --no-deps` is clean.
    - Nothing was built for macOS or Windows: `ui` and `gui` have no OS
      code, and CI's gate checks the scenes there on the next push.
  - Note (2026-11-22): M3.3's review found that a collection whose view
    lost a row as it got shorter kept the rows left at the heights the
    last layout pass gave them while the lost row still shared the space,
    shorter than `ROW_HEIGHT`. `bind_collection` now counts a removed row
    as a change, so the layout passes again and each row is
    `ROW_HEIGHT` high
    (`a_collection_that_loses_a_row_to_a_smaller_view_lays_its_rows_out_whole`
    in `crates/ui/tests/collections.rs`).

- [x] **M3.3 — ui: overlays**
  - Depends: M3.7
  - Do: the rest of the Components table: tooltip, toast, modal and the
    command palette, over a layer of overlays in the tree (ADR-0052).
  - Accept: as M3.6, plus keyboard-navigation tests for the palette, and
    tests that a modal makes the rest inert, keeps Tab within itself and
    gives focus back when it closes.
  - Verify: `cargo xtask golden`; `cargo test -p tidepool-ui`
  - Note (2026-11-20): ADR-0052 split this task, which was `ui:
    collections and overlays`: M3.7 builds the list, tree view, table,
    tab strip, splitter, card and code block; this task keeps the
    tooltip, toast, modal and palette. The palette shows what its owner
    ranks, and a tooltip's delay and a toast's timeout are the owner's
    timers.
  - Note (2026-11-22): built; the spec's `overlay`, `input`, `a11y` and
    `widgets` rows and its Tests have the details
    (`docs/subsystems/ui-and-theme.md`). `ui` has:
    - `overlay`: overlay roots over the main root's layer, at most
      `MAX_OVERLAYS` (64), each `Modal`, `Modeless`, or `PassThrough`
      (`OverlayKind`), placed by an `Anchor` in the window or on a `Side`
      of a node, with `Tree::open_overlay`, `close_overlay`, `overlays`,
      `overlay_kind`, `overlay_anchor`, `set_overlay_anchor`, `overlay_of`,
      `modal` and `inert`. An overlay that is not modal and is anchored to
      a node is hosted by that node's layer, so it is live or inert with
      it (ADR-0052).
    - The active modal (the topmost modal overlay that is shown) makes the
      layers below it inert (no hover, presses, events, focus, tab stops,
      or accessibility nodes), and keeps Tab within itself and the layers
      above it (ADR-0053). As it opens, it gives focus to its default: its
      root, which is not a tab stop (a dialog's), or the palette's field
      (ADR-0054). It keeps focus from resting on no node while that
      default can take it, and gives focus back when it closes
      (ADR-0052). What it keeps from the window comes back
      `Response::blocked`, as does a press after the active modal
      changes, closing and hiding included, until the pointer moves more
      than 4 device pixels (`PRESS_SLOP`; ADR-0055, ADR-0056).
    - In `widgets`: `Modal`, the dialog, put in a tree only by
      `Modal::open`; `Tooltip`, which the owner opens when its delay
      timer, following `Tree::tooltip_target`, fires, closed by Escape
      first, its text its node's accessible description; `Toast`, info,
      warning, or error, at most four at the bottom-right, of which only
      those shown after a modal are live above it, kept clear of it, with
      `Toast::held` and `Toast::waiting` for the owner's timeout
      (ADR-0055); and `Palette`, a modal search field over a windowed
      list of the items its owner ranks, which draws the characters
      `Row::matched` names in Saira Bold and `teal-hi` (`MATCH_TEXT`).
    - `AccessNode` gains `modal`, `description`, and `active`; a
      collection's rows show and name control, bidirectional formatting,
      and Unicode's default-ignorable characters as U+FFFD, which the
      dialog, tooltip, toast, and text input drop, all but keeping the
      flags of England, Scotland, and Wales whole (UTS #51's three RGI
      emoji tag sequences, an exception the owner asked for on
      2026-11-22); a button's label is cut
      to its bounds; the theme checks one more
      pair, `text` on `panel-solid`, 56 at AA.
      `crates/gui/tests/overlays/mod.rs` paints 8 scenes, `ui_modal`,
      `ui_tooltip`, `ui_toast`, `ui_palette`, and their `_scale_2`; and
      `tidepool-bench`, which now uses `tidepool-ui` and `tidepool-gui`,
      gains `ui/palette/repaint/4kib_64_matches`.
  - Note (2026-11-22): the owner chose the recommended option of each
    question that ADR-0053 to ADR-0056 record: on 2026-11-21,
    that focus lands on a modal dialog itself, that Tab reaches the
    overlays opened above it, that the window behind it is dimmed, and
    that its fill is opaque (ADR-0053); on 2026-11-22, that the palette's
    focus goes to its search field and the window behind it is dimmed
    (ADR-0054), and that a press waits for the pointer to move after a
    modal becomes active, and only the toasts shown after it are live
    above it, kept clear of it (ADR-0055), and that the press guard
    allows 4 device pixels of movement and applies when a modal closes
    too (ADR-0056). On 2026-11-22 the owner also
    chose how the palette shows what a query matched: the matched
    characters in Saira Bold and `teal-hi`.
  - Note (2026-11-22): reviewed in three rounds, each by a correctness
    reviewer and `security-reviewer`, every finding shown by a probe
    before it was fixed:
    - The overlay layer and the dialog: 10 findings. The main ones:
      modals closed out of order lost the node to give focus back to, and
      an overlay anchored to the inert window was live above the dialog.
      The review also found M3.7's row-layout bug noted there.
    - The tooltip, toast, and palette, with the first round's fixes: 13
      findings, plus 4 suspicions fixed as well. The main ones: a palette
      click ran an item the user never pressed when the items changed
      mid-click, a dialog opening under a resting pointer took a click
      aimed elsewhere (ADR-0055), and rows showed bidirectional
      formatting characters as written.
    - The second round's fixes: 14 findings, the main ones a toast's
      long action label painting across a dialog, and a dialog part
      made while the window was too short that still took focus.
    All are fixed. Each fix was reverted once to see its test fail, the
    reviews' probes were run again on the fixed code, and their random
    harnesses ran on new seeds: 1,000 seeds of the correctness harness
    (with ADR-0056's slop) and 10,000 of the security harness, 120
    steps each, with no invariant broken. Six more defects their
    harnesses found while fixing are fixed and tested too.
  - Note (2026-11-22): checked on rustc 1.99.0, Linux:
    - `cargo xtask golden`: 83 scenes ok. The 8 new ones were blessed
      from images byte-identical to those the owner looked at.
    - `cargo test -p tidepool-ui`: 75 unit tests, 3 a11y, 31
      collections, 28 components, 12 frame, 2 fuzz, 19 input, 22 layout,
      39 overlays, 58 popups, 4 settling, 2 theme, 29 widgets, 32 doc
      tests, and 1 doc test that must not compile pass.
    - `cargo xtask theme`: 56 of 56 pairs at AA.
    - `cargo xtask ci`: 13 of 13 steps pass, with the 29 window tests
      skipped; `TIDEPOOL_IT_X11=1 xvfb-run -a cargo xtask ci`: 13 of 13,
      none skipped. `RUSTDOCFLAGS='-D warnings' cargo doc -p tidepool-ui
      -p tidepool-gui -p tidepool-bench --no-deps` is clean.
    - Nothing was built for macOS or Windows: `ui` and `gui` have no OS
      code, and CI compares the scenes there.

- [x] **M3.8 — gui, app: the event-driven main loop**
  - Depends: M2.6, M2.7, M3.5
  - Do: per ADR-0012 and ADR-0059:
    - `gui`: `Gui` keeps a framebuffer of the window's size and a
      `Painter`; `Gui::render` lays a `ui` tree out at the window's
      logical size with the painter's `FontMetrics` at the window's
      scale, paints the frame's damage over the base and presents it
      with the window's exposes; a resize or a new base repaints the
      whole window. The backdrop is built on threads of its own, at
      most two at once, a waiting request replaced by a newer one and a
      stale result dropped, with the flat `bg` as the base until it
      arrives; the first present waits for it, at most
      `FIRST_FRAME_WAIT` (250 ms) after the window opened, and
      `Gui::next_deadline` gives the loop that deadline. `first_frame`
      builds the first frame's lines as a `ui` tree; `Gui::turn` and
      `Turn` go.
    - `app`: the loop of `docs/01-architecture.md`: wait on the window
      until the earliest deadline, input routed to the tree with
      `Tree::handle` first, a close request ends it, then render;
      `--print-startup-time` after the first present.
    - The docs that describe the first frame and the loop:
      `docs/01-architecture.md` (the loop and the threading model),
      `docs/subsystems/frontends.md`, the READMEs of `tidepool-gui` and
      `tidepool`, `AGENTS.md`, `README.md`, `diagrams/`,
      `docs/features.jsonc`.
  - Accept:
    - `cargo tp` shows the first frame's lines over the backdrop,
      painted by `Painter` from a `ui` tree; `--print-startup-time`
      stays within the 300 ms budget on the reference machine.
    - Nothing on the main thread builds a backdrop: a resize repaints
      the whole window over the flat `bg` at once, and over the
      backdrop when its build arrives; a stale build is dropped; at
      most two builds run at once; a failed build, or a thread that
      cannot start, leaves the flat `bg` and logs a warning (unit tests
      of the builder and the screen, headless).
    - HiDPI: at scales 1, 1.5 and 2 the tree is laid out at the window's
      logical size and painted at its device size (headless tests).
    - An idle window presents nothing, and the idle binary uses no CPU
      over a second (window tests under Xvfb).
    - A resize on the reference machine's desktop follows the drag,
      with the backdrop arriving after (human).
  - Verify: `cargo test -p tidepool-gui -p tidepool`; `TIDEPOOL_IT_X11=1
    xvfb-run -a cargo test -p tidepool-gui -p tidepool`; `cargo xtask
    ci`; `cargo tp -- --print-startup-time`
  - Note (2026-11-22): split from M3.4 (ADR-0059), which keeps the
    shell. The user accepted ADR-0012 that day, and chose the backdrop's
    builders on worker threads with the flat `bg` meanwhile.
  - Note (2026-11-22): built. `gui`'s `Gui` has `wait_events`, `render`,
    `next_deadline`, `scale` and `backdrop_ready`, over a private
    `Screen` (the framebuffer, the base, the `Painter` and the exposes,
    tested without a window) and `Builds` (the backdrop's scheduler,
    tested with a stand-in builder and no threads; `Threads` runs each
    build on a `tidepool-backdrop` thread, which reports through a drop
    guard, so a build that panics is logged and frees its place).
    `first_frame::tree` gives the seven lines as a `ui` tree; `Gui::turn`,
    `Turn` and `FirstFrame` are gone. The wait of the first present ends
    for good at the first render past it, so the loop never spins on a
    deadline that has passed. `app`'s loop routes input to the tree
    before rendering and stops at a close request. `cargo test -p
    tidepool-gui -p tidepool`: `gui`'s 71 unit, 61 golden, 2 paint, 1
    window and 6 doc tests, and `tidepool`'s 6 unit, 2 command-line and 1
    window test, the two window tests printing `SKIPPED:`; under
    `TIDEPOOL_IT_X11=1 xvfb-run -a`, the same, with both window tests run.
    Under Xvfb the idle binary used 0 ticks of processor time over a
    second, after about 30 at startup. The 132 Mermaid blocks of
    `diagrams/` parse and render with Mermaid 11.17.2 in the app's
    browser, which rejected a planted broken one and caught a `;` in a
    message of a new sequence diagram, since fixed.
  - Note (2026-11-22): `--print-startup-time` under Xvfb, five runs
    each: 292 to 327 ms in a dev build, 286 to 317 ms in a release
    build, at a load average of 41 on the reference machine's 16 threads
    from other programs (a planner at about 870% and a JVM at about 320%
    of a core). At that load the backdrop missed the 250 ms wait, so these
    times are the wait's, not the backdrop's. Ten minutes later, at a load
    average of 15 to 17, three runs each: 147 to 171 ms in a release build
    and 149 to 158 ms in a dev build, the backdrop in the first frame.
    `cargo xtask ci`, under `TIDEPOOL_IT_X11=1 xvfb-run -a`: 13 of 13
    steps, 2,414 tests passed, none skipped. `RUSTDOCFLAGS='-D warnings'
    cargo doc -p tidepool-gui -p tidepool --no-deps` is clean.
  - Note (2026-11-22): the user ran `cargo tp -- --print-startup-time`
    on the reference machine's desktop, resized the window, and reported
    that it looks right, which meets the two criteria left for a person;
    the time it printed there was not reported back.

- [x] **M3.4 — app: the shell**
  - Depends: M3.3, M3.8, M2.6, M2.7
  - Decide first (ADR-0059):
    - where the shell's views live: `editor`, where
      `docs/subsystems/editor.md` puts every panel, with its goldens
      drawn in `tidepool`'s `tests/golden.rs` (a new row of `xtask
      golden`'s `SCENES`), or `app`;
    - how the activity bar shows its views: icons drawn as outlines,
      Tidepool's first, or labels;
    - which settings the read-only loader reads before M4.7, and where
      it lives;
    - which commands the palette lists, and how they are ranked.
  - Do: title strip, activity bar, side panel, editor area, bottom
    panel, status bar, command palette; settings loading (read-only
    until M4.7).
  - Accept: shell golden at 1280×800 and 1920×1080; resize and HiDPI work;
    no redraw when idle.
  - Verify: `cargo xtask golden`; `xvfb-run cargo test -p tidepool`
  - Note (2026-11-17): from M2.6, `gui` rebuilds the backdrop on the main
    thread when the window is resized, 50 to 250 ms at the reference
    machine's sizes (ADR-0045), against the loop's rule of about 1 ms a
    handler; this task moves that work off the main thread or makes it
    fast enough.
  - Note (2026-11-22): ADR-0059 split this task, which was `app:
    event-driven main loop and shell`: M3.8 builds the loop and `gui`'s
    painting of `ui` frames, and builds the backdrop off the main thread,
    which answers the note above; this task keeps the shell. The user
    accepted ADR-0012 that day, which answers the `Decide first` line
    this task had; the line now holds the shell's questions.
  - Note (2026-11-22): ADR-0060 answers the `Decide first` line with the
    options the user chose that day, each the recommended one: the shell
    in `editor::shell`, its goldens drawn by `tidepool`'s
    `tests/golden.rs`; outline icons in `gui::icons`, registered through
    `Gui::set_icon`, so the task touches four crates (`editor`, `app`,
    `xtask` and `gui`), as the user accepted; `[ui] side_panel` and
    `bottom_panel` read by `app`; and the shell's commands ranked by a
    subsequence matcher.
  - Note (2026-11-22): built. `editor::shell` builds the shell in a `ui`
    tree: the title strip with the header's `teal` glow, the activity bar
    (`parts::ActivityButton`, each with a tooltip), the side and bottom
    panels beside splitters, either hidden by turning its split into a
    plain row or column, the editor area over the backdrop, the status bar
    on `bar`, and the palette, whose 13 commands `shell::commands::rank`
    ranks; Ctrl+Shift+P or F1, Ctrl+B and Ctrl+J are the shortcuts.
    `gui::icons` has the six icons and `Gui::set_icon` registers them,
    repainting only when one changes. `app` reads `[ui] side_panel` and
    `bottom_panel` (`settings.rs`, at most 1 MiB) and drives the shell
    from the loop, its tooltip timer among the deadlines. The theme lists
    `text-2` and `text-3` on `bar` (8.79:1 and 4.68:1), in its existing
    entries, so the reader's room for 990 more stays. The side and bottom
    panels draw no hairline of their own, as the splitter's rule divides
    them. Under Xvfb the first frame, now the shell, took 116 to 139 ms in
    a dev build and 98 to 194 ms in a release build, at a load average of
    about 10. The 133 Mermaid blocks of `diagrams/` parse and render with
    Mermaid 11.17.2, which caught two `;` in new sequence messages, since
    fixed, and rejected a planted broken one.
  - Note (2026-11-23): the user looked at the four scenes on 2026-11-22
    and had them blessed: `shell_1280x800`, `shell_1920x1080`,
    `shell_palette_1280x800` and `shell_960x600_scale_2`, 91 to 275 KB
    each. `TIDEPOOL_IT_X11=1 xvfb-run -a cargo xtask ci`: 13 of 13 steps,
    2,463 tests, none skipped, after a run whose only failure was `ui`'s
    timing test `a_huge_text_costs_what_a_short_one_does` at 567 ms
    against its 500 ms, at a load average of about 16 from other
    programs, which passed alone three times (0.24 to 0.29 s) and then in
    the full run; M3.4 changes no `ui` code. `RUSTDOCFLAGS='-D warnings'
    cargo doc -p tidepool-editor -p tidepool-gui -p tidepool --no-deps` is
    clean. Resizing and HiDPI are checked headlessly (the areas at three
    sizes and one smaller, the scale-2 scene) and by M3.8's resize path;
    the shell was not tried by hand on the reference machine's desktop.
  - Note (2026-11-22): M3.3 built what the shell's palette and popups
    need from `ui`. `widgets::Palette` shows the items its owner ranks,
    so the shell ranks its commands itself (ADR-0052). A tooltip's delay
    and a toast's timeout are timers of the loop's `next_deadline`, which
    follow `Tree::tooltip_target` and `Toast::held`. An event that comes
    back `Response::blocked` is one the loop runs no window shortcut for
    (ADR-0055), a press while the press guard holds after a modal opens
    or closes among them (ADR-0056). The guard's slop is in device pixels,
    so the loop lays the tree out with metrics whose `TextMetrics::scale`
    is the window's, as `gui::FontMetrics`'s is.

**Exit criteria (M3):** the empty Tidepool shell, side by side with
jabbott.dev, reads as the same design (human review).

---

## M4 — Editing Tidepool with Tidepool

**Goal:** dogfooding starts. Tidepool edits its own source on Linux.

- [x] **M4.1 — editor: editor view**
  - Depends: M3.4, M1.3, M1.9
  - Do: text rendering from the rope, cursor, selections, scrolling,
    gutter, current line, soft wrap.
  - Accept: scrolling budget met on a 10,000-line file; goldens.
  - Verify: `cargo xtask golden`; `cargo xtask bench editor`
  - Note (2026-11-23): ADR-0063 records the view's design and the
    options the user chose that day, each the recommended one: the
    binary keeps its welcome line until M4.3 opens files; soft wrap is
    off by default; a hidden character shows as a visible marker; and the
    current line's wash takes a new `current-line` token, so the task
    touches four crates (`editor`, `ui`, `tidepool` and
    `tidepool-bench`), as the user accepted.
  - Note (2026-11-23): built. `editor::view::EditorView` shows a rope
    and its selection in a `ui` tree: a gutter of line numbers, the text
    in JetBrains Mono 14 px / 1.5, selections in `teal-glow`, `teal`
    cursors while it has focus, the current line's wash, soft wrap, and
    two scrollbars, laying out only the lines on screen and cutting a
    line at 10,000 bytes (`MAX_LINE_BYTES`) with a `…`; `point_to_byte`,
    `byte_to_point` and `scroll_into_view` are ready for M4.2. `ui` gains
    the `current-line` token (`rgba(255,255,255,0.025)`, solid
    `#111213`, ANSI 233; 52 tokens), two contrast entries over
    `editor-bg` (`text` 16.31:1 and `text-2` 9.60:1, 70 pairs in all),
    and `widgets::is_hidden` and `widgets::flag_at`, made public.
    `tidepool`'s `tests/golden.rs` draws five scenes, which the user
    looked at and had blessed as they are on 2026-11-23:
    `editor_view_800x480`, `editor_view_wrap_800x480`,
    `editor_view_scrolled_800x480`, `editor_view_cut_800x480` and
    `editor_view_400x240_scale_2`, 19 to 125 KB each. In the first, the
    Wales flag on line 4 draws as seven `.notdef` boxes, since no bundled
    face has its glyphs. `xtask theme`'s room for more contrast entries
    now counts only `[[contrast.use]]` headers, and is one entry from its
    990 bound.
  - Note (2026-11-23): the scrolling budget, 16.7 ms a frame at 60 fps,
    is met. `tidepool-bench`'s `suites::editor` measures a wheel notch
    of three rows through a 10,000-line file at 1920 × 1080: handled,
    framed, and its damage painted with `gui`'s `Painter`. Pinned to one
    core with `taskset -c 4` on the idle reference machine, two runs
    agreed within 0.5%: with soft wrap off a 1.91 ms median and 2.05 ms
    p95; with it on, over a file whose 1,930 longest lines wrap, 3.80
    and 4.25 ms; and the first layout of a 10,000-byte line, 5.45 and
    5.69 ms. The Verify run, unpinned at a load average of about 6 from
    other programs, gave 2.14 and 2.35, 4.25 and 4.71, and 14.0 and
    14.7 ms. No baseline is recorded for the three suites:
    `--save` refuses a pinned run, which sees 1 thread against the
    baseline's 16 (ADR-0035), and other programs loaded the machine
    whenever it ran unpinned. Recording them later, on an idle machine,
    takes `cargo xtask bench editor --save --machine "MSI Pulse GL66
    11UGKV, the reference machine of docs/00-overview.md"`. The 134
    Mermaid blocks of `diagrams/` parse and render with Mermaid 11.17.2,
    which caught one `;` in a new sequence message, since fixed, and
    rejected a planted broken one. `cargo xtask ci`: 13 of 13 steps,
    2,509 tests, 30 skipped (the X11 tests, `TIDEPOOL_IT_X11` unset),
    after a run whose only failure was `codec`'s timing test
    `a_table_with_100000_keys_parses_in_linear_time` at 3.7 s against its
    2 s, at a load average of about 20 from other programs, which passed
    alone three times; M4.1 changes no `codec` code.
    `RUSTDOCFLAGS='-D warnings' cargo doc --no-deps` of `tidepool-editor`,
    `tidepool-ui`, `tidepool-bench` and `tidepool` is clean.

- [x] **M4.6 — editor: commands and the modal keymap engine**
  - Depends: M4.1, M1.7
  - Decide first: accept ADR-0006 (vim mode as a modal keymap engine in
    `editor`), which is `Proposed`.
  - Do: named commands with palette entries; the keymap engine with a mode
    stack and a key-sequence parser; keymaps as TOML, with user bindings
    over the defaults and conflicts listed; the default keymap (one mode).
  - Accept: command tests; a test keymap with two modes switches correctly;
    a conflicting binding is reported.
  - Verify: `cargo test -p tidepool-editor`
  - Note (2026-11-23): the user accepted ADR-0006 that day with `Search`
    added to its modes, which answers the `Decide first` line. ADR-0064
    records the design and the options the user chose, each the
    recommended one: the user's bindings in `settings.toml`'s `[keymap]`
    tables, read by the binary at start; keys in `ctrl+shift+p`
    notation; and a prefix conflict reported, the shorter binding
    winning, with a started sequence waiting for its next key without a
    timeout. The task touches three crates, `editor`, `tidepool` and
    `xtask` (the fuzz target `keymap`).
  - Note (2026-11-23): built. `editor::command` is the registry of named
    commands, each an id and its palette title, which the palette ranks;
    the shell's 13 commands take ids such as `view.toggleSidePanel`, and
    opening the palette is a 14th, `palette.open` ("Command Palette:
    Show"). `editor::keymap` reads keys in `ctrl+shift+p` notation and
    keymaps from TOML layers within its `Limits` (4,096 bindings a layer,
    8 steps, 32 modes), reports conflicts, overrides and errors, and its
    `Engine` keeps a stack of modes and a pending sequence. The default
    keymap, `keymaps/default.toml`, binds the four shortcuts the shell
    matched in code; the shell's goldens are unchanged. The binary reads
    `settings.toml`'s `[keymap]` tables as the user's layer and logs the
    first 64 problems. The `keymap` fuzz target reads any text as a
    keymap file, a user layer and keys, and drives an engine over each:
    1,079,262 inputs in 60 s, no findings.
  - Note (2026-11-23): the security review found no panic, injection, or
    way for a workspace to supply bindings, and two findings, both fixed
    with tests: entries of a user layer that were not modes counted
    toward no limit, so a 1 MiB `settings.toml` gave about 124,000
    problems and flooded the log (now every entry counts, a layer's
    problems are bounded by its limits, and the binary logs at most 64);
    and the fuzz target's driver could not press most chords (now it
    presses any key with any modifiers, and the keymap's own bindings).
    It also notes that a user's mode without a `mode.leave` binding
    leaves the keyboard without shortcuts until a restart, and that M4.7
    must not let an untrusted workspace's settings add a layer of
    bindings. The 135 Mermaid blocks of `diagrams/` parse and render with
    Mermaid 11.17.2, which rejected a planted broken one. `cargo xtask
    ci`: 13 of 13 steps, 2,563 tests, 30 skipped (the X11 tests,
    `TIDEPOOL_IT_X11` unset). `RUSTDOCFLAGS='-D warnings' cargo doc
    --no-deps -p tidepool-editor -p tidepool` is clean.

- [x] **M4.2 — editor: editing commands, multi-cursor, undo and clipboard**
  - Depends: M4.6, M1.2, M2.7
  - Do:
    - Motions by grapheme, word and line; the editing commands;
      multi-cursor through `Transaction::change_each`; undo and redo
      through `History`, one command one step and typing coalesced
      (ADR-0020); the clipboard through the frontend.
    - M1.1's risk: a grapheme query can take 0.5–3 s on a crafted 100 MB
      line. Bound cursor motion, or run it on a worker, so the main thread
      never waits.
    - M1.2's risk: 10,000 carets on a 100 MB file take about 120 ms, over
      one frame. Batch in-leaf edits or make them cheaper, measured with
      `cargo xtask bench`.
  - Accept: command tests; on the crafted line no keystroke holds the main
    thread longer than one 16 ms frame; a 10,000-caret keystroke on 100 MB
    fits in one frame, or an ADR records the measured exception.
  - Verify: `cargo test -p tidepool-editor`; `cargo xtask bench editor`
  - Note (2026-11-23): ADR-0065 records the design and the options the
    user chose that day, each the recommended one: the editor asks for
    the clipboard and its owner carries it out, `gui` and the binary
    joining in M4.3; Up and Down by line with a goal column, also with
    soft wrap on; long lines read in a window of `MAX_LINE_BYTES` either
    side of the cursor, and `text`'s in-leaf edit counted from the bytes
    it changes; and Visual Studio Code's Linux keys, Tab as 4 spaces. The
    task touches `editor`, `text`, `tidepool-bench`, and in `tidepool`
    one test's count of the default bindings.
  - Note (2026-11-23): built. `editor::edit::Editor` runs 40 commands
    (`edit::Command`, `editor.` ids) on every cursor through
    `change_each` and `History`: motions by cluster, word, line (a goal
    column), the line's and the text's ends and a page, each with a
    `…Select` twin; typing, deletions, new lines with the indentation,
    indent and outdent; one undo step a command, typing coalesced within
    1 s a word at a time; copy, cut and paste as `edit::Request`s, whole
    lines from carets and spread pastes; cursors added above and below;
    and the mouse. The shell's registry holds them after its 14, and
    `keymaps/default.toml` binds them (45 bindings); the palette lists
    what the shell runs, so its goldens are unchanged. In `text`, an
    in-leaf edit updates its leaf's summary from the bytes it changes, a
    change set of two or more in-leaf edits applies in one walk of the
    tree (`rope::tree::batch_edit`), and `change_each` does not look a
    cursor's own ends up twice; a test applies 2,000 change sets batched
    and one by one and compares them, and mutations that skip the CR
    check at a leaf's end or the summary update fail it. The property
    test of random editing found a bug before it was fixed: Shift+Tab
    read 4 bytes into a line that starts with a 3-byte char.
  - Note (2026-11-23): measured, release, on the reference machine while
    other programs loaded it (load averages from 4 to 50 on 16 threads;
    `top` showed Java builds, then niced `rustc` builds and a planner).
    `text/history/keystroke_10k_carets/100mb`: 114 ms median at a load of
    4 before the change, 13.4 ms (p95 14.2) after it at a load of 45;
    `text/rope/insert/400kb`: 9.3 µs in the baseline, 0.59 µs pinned to
    one core after. Most of what is left is copying the 10,000 leaves the
    keystroke touches, which the text before still holds: the batched
    apply took 8.6–10.1 ms where one replace per edit took 10.3–13.8 ms in
    the same process. `editor/keystroke/10k_carets_100mb_1080p`, the whole
    keystroke, its frames painted at 1080p and the 10,000 replaced leaves
    freed: 24.2 ms median (p95 27.0) at a load of 19 to 23, 24.4 ms
    (p95 47.8) at 45, and at a load of about 4, where the old code gave its
    baseline's 115 ms, 18.0, 18.7 and 18.8 ms (p95 19.1 to 25.9) in three
    runs and 18.7 ms (p95 19.5) pinned to one core: about 2 ms over a
    16.7 ms frame. The 21 `editor/crafted` suites, M1.1's three
    crafted 100 MB lines with seven keystrokes each, at a load of about
    20: the slowest, a deletion before the last regional indicator, 1.85
    ms median and 2.18 ms p95, so no keystroke there comes near a frame.
    The Verify run of `cargo xtask bench editor`, at a load of about 4:
    all 25 suites, the scroll suites as M4.1 measured them (2.03 ms a
    notch with soft wrap off, 4.01 ms on, 5.56 ms for the 10,000-byte
    line), the 10,000-caret keystroke 19.9 ms (p95 23.5), and the slowest
    crafted keystroke the same deletion, 631 µs (p95 785 µs). No suite is
    recorded in the baseline: the machine was never idle.
  - Note (2026-11-23): the security review found no panic from input and
    no way to break the rope's invariants, and showed the two skipped
    checks (`change_each`'s cursor ends, `EditorView::show`'s selection)
    redundant. Fixed from it, with tests: an edit during a mouse drag left
    the drag's anchor past the text's end, so the next move returned an
    error (a step now ends the drag and the double click's first press);
    the windowed read's bound is per cursor, as the docs now say; and the
    batch's internal `Edit` showed its text in `Debug` (now its length).
    Left for M4.3: nothing caps the rope a copy hands its owner, which a
    64 MiB paste at many cursors can make gigabytes long while sharing
    its nodes; turning it into the clipboard's text needs a cap there.
    The 136 Mermaid blocks of `diagrams/` parse and render with Mermaid
    11.17.2, which rejected a planted broken one. `cargo xtask fuzz-smoke
    --target keymap --secs 30`: 319,414 inputs, no findings.
    `RUSTDOCFLAGS='-D warnings' cargo doc --no-deps` of `tidepool-editor`,
    `tidepool-text`, `tidepool-bench` and `tidepool` is clean.
  - Note (2026-11-23): the 10,000-caret keystroke on 100 MB measures
    about 19 ms, over one frame. The user chose that day to record it as
    the measured exception the acceptance allows, rather than optimize
    first, which ADR-0066 does; so every criterion is met.

- [x] **M4.7 — workspace: folders, ignore rules, trust, settings**
  - Depends: M1.5, M1.8, M2.7
  - Decide first: accept ADR-0014 (workspaces are untrusted until the user
    trusts them), which is `Proposed`.
  - Do: `folder` (roots, `.tidepool/`, `.tidepoolignore` and `.gitignore`
    matching), `trust` (stored in user data) and `settings` (defaults →
    user → workspace, written back through M1.5's document model), per
    `docs/subsystems/editor.md`.
  - Accept: untrusted is the default, and a trust decision persists; saving
    settings keeps unknown keys and comments; an invalid value falls back
    to its default with a logged warning.
  - Verify: `cargo test -p tidepool-workspace`
  - Note (2026-11-23): the user accepted ADR-0014 that day, with a trust
    decision covering the folder trusted and every folder inside it,
    which answers the `Decide first` line. ADR-0067 records the design and
    the options the user chose, each the recommended one: grants that
    cover subfolders; a workspace's `.tidepool/settings.toml` applied only
    once the folder is trusted, its `[keymap]` included; ignore files in
    Git's syntax, in every folder; and the library with the binary's
    reading moved to it, the editor's settings left to M4.3 and reloading
    to M4.5. The task touches three crates, `workspace`, `tidepool` and
    `xtask` (the fuzz targets' rows), and the doc comments of `editor`
    and `codec` that named M4.7.
  - Note (2026-11-23): built. `folder` opens a root by its canonical path
    and reads `.gitignore`, `.tidepoolignore` and `.git/info/exclude` as
    gitignore(5) says, each pattern compiled to at most one 12-byte
    instruction a byte and matched by stepping a set of states, so
    `*a*a…b` on a run of `a`s takes the run's length times the pattern's;
    98 cases of `tests/folder.rs` hold the answers Git 2.43.0's `git
    check-ignore -q --no-index` gave that day, and the precedence tree's
    `.gitignore` answers were checked against it too. `trust` keeps
    grants in `trust.toml`; `settings` resolves a `Schema`'s settings
    from the defaults, the user's file and a trusted folder's, and writes
    a value back through `codec::toml::Document::set` and
    `text::file::save`. The binary reads `[ui]` and `[keymap]` through it.
    The fuzz targets `ignore`, which also compares the matcher with a
    port of Git's `wildmatch`, and `settings` found nothing, run again
    after the review's fixes: 376,215 and 3,108,378 inputs in 60 s each.
  - Note (2026-11-23): the security review found no critical or high
    issue, and one medium and four low ones, all fixed with tests, and
    ADR-0067 updated: ignore files had no bound in all, so two hostile
    1 MiB root files took 2.61 s for one lookup of depth 10 in a release
    build and a 1 MiB file of `[a]` held 22 MiB (now 256 KiB a file and
    1 MiB in all, and `**/**/` compiled as one `**/`: that lookup takes
    0.6 ms, the worst file holds 14.7 bytes a byte, and the hardest
    family found, `**/a/` groups filling the budget, 4.3 ms); ignore
    files were read through a linked folder (now never); a trusted
    folder's `.tidepool/settings.toml` linked to its own `Cargo.toml`
    let a setting rewrite it (now neither `.tidepool` nor the file may
    be a link); `trust` could save a grant the next load refuses (now it
    reads the text back first); and the trust gate was a convention
    (now `Settings::resolve` uses a workspace's file only from the
    constructors that ask a `TrustStore`). It leaves three risks to M4.3,
    in that task's note, and a pipe swapped in between the check and the
    open, which can still hold a worker.

- [x] **M4.10 — platform: file metadata and the process's user and group (Linux)**
  - Depends: M2.1, M2.2
  - Do: per ADR-0068 and ADR-0025: in `tidepool-sys`, `geteuid` and
    `getegid` from libc; in `tidepool-platform`, a file's link count,
    owner, group and permission bits, following symbolic links as a save
    does, and the owner and group that a file this process creates in a
    folder gets (the effective user; the folder's group when the folder
    has the setgid bit, else the effective group), with `Unsupported` on
    other systems until M11.8 and M12.2;
    `docs/subsystems/sys-and-platform.md` and both crates' READMEs.
  - Accept: a file with a second hard link reports two links; a file the
    test makes reports the process's owner and group, and the owner and
    group given for its folder are the ones it has; the setgid rule is
    tested; a missing file is an error; each declaration cites its
    header, and any new `unsafe` block has its `// SAFETY:` comment.
  - Verify: `cargo test -p tidepool-sys -p tidepool-platform`;
    `cargo xtask unsafe-audit`
  - Note (2026-11-23): built. `tidepool-sys` declares `geteuid` and
    `getegid` as `safe` functions, since they take nothing and cannot
    fail (getuid(2) and getgid(2), Linux man-pages 6.7), so no `unsafe`
    block is new, with `uid_t`, `gid_t` and `mode_t` (4 bytes each,
    asserted), `S_ISGID` and `ALLPERMS`, from glibc 2.39's x86_64 headers;
    aarch64's were not at hand, so its values are marked to verify there.
    `tidepool-platform`'s `file` has `metadata` (`Metadata`: links,
    `Owner`, permission bits) and `new_file_owner`, through `std`'s `stat`
    on Linux and `Unsupported` elsewhere. Eight tests in `linux::file`
    and two in `sys` ran here; the setgid folder was given another of
    this user's groups, and under `unshare --user --map-current-user`
    that test prints `SKIPPED:`. `TIDEPOOL_IT_X11=1 xvfb-run -a cargo
    test -p tidepool-sys -p tidepool-platform` passed with no `SKIPPED`;
    clippy is clean for x86_64 and aarch64 Linux, x86_64 macOS and
    x86_64 Windows, where the `Unsupported` test was compiled, not run.
    The 137 Mermaid blocks of `diagrams/` parse and render with Mermaid
    11.17.2 in headless Chrome, which rejected a planted broken one.
  - Note (2026-11-23): the security review found no critical or high
    issue. Its two medium ones concern M4.11's use: the folder to ask
    about is the one holding the file the save reaches after links (the
    README's example now asks about the canonical path's folder, and
    `new_file_owner` says so), and the rule misses NFS's root squashing
    and a user namespace's overflow ids (now disclosed, with an error or
    an overflow id meaning ask), for which it suggested checking the
    save's own temporary file before the rename; M4.11's `Decide first`
    line asks that. Its low ones were fixed: the setgid test skips in a
    user namespace instead of failing, the example handles a bare file
    name, the sticky bit is described rightly, a header claim is
    reworded, and the `sys` test removes its folder.

- [x] **M4.11 — workspace: buffers, the file tree model and session**
  - Depends: M4.10, M4.7, M1.2, M1.3, M1.6, M1.9
  - Decide first (ADR-0068, M4.10's security review): how a save tells
    that its rename would split a hard link or change the owner or group:
    by M4.10's rule (`platform::file::new_file_owner` for the folder of the
    file the save reaches after links, which `text::file` would have to
    expose, an error or an overflow id meaning ask), or by reading the
    save's own temporary file just before the rename, which is exact on
    every file system (NFS squashing and user namespaces included) and
    narrows the race, but needs `text::file`'s save in two steps, making
    `text` a fourth crate of the task.
  - Do: per ADR-0068 and `docs/subsystems/editor.md`:
    - `buffers`: open buffers keyed by canonical path, each a rope and
      its `History`, loaded and saved on workers through `text::file`,
      saves through `History::checkpoint` and `mark_saved`, each result
      carrying the buffer version it was made for, so that a stale one
      is dropped; a file of more than 16 MiB opens read-only until made
      editable; before a save over an existing file, its link count,
      owner and group through `platform` (M4.10), and when a rename would
      split a hard link or change the owner or group, the save stops for
      its owner to choose: write in place, save as a new file, or cancel
      (ADR-0025).
    - `files`: a folder's file tree model, read a folder at a time when
      asked, on workers, each entry with its ignore state from `folder`.
    - `session`: one file a folder in the user data folder's
      `sessions/`, named from the folder's canonical path, which it holds
      and loading checks; the open editors, the splits, and each
      editor's selection and scroll; read within limits, never
      panicking, with a fuzz target `session` (ADR-0027).
    - a `tidepool-bench` suite opening a 100 MB file.
  - Accept: opening, editing and saving files round-trips, headless; a
    100 MB file opens in large-file mode within the budget; saving a file
    with a second hard link, or one whose owner or group a save would
    change, stops for the choice first, and each answer does what it
    says; a session round-trips, and one that names another folder is
    not used.
  - Verify: `cargo test -p tidepool-workspace`; `cargo xtask bench
    workspace`; `cargo xtask fuzz-smoke --target session --secs 60`
  - Note (2026-11-23): ADR-0069 answers the `Decide first` line with the
    option the user chose that day, the recommended one: the save checks
    its own temporary file just before the rename. `text::file` gains a
    save in two public steps (`prepare`, then `PreparedSave::commit`) and
    `write_in_place`, so `text` is a fourth crate of this task.
  - Note (2026-11-23): built. `workspace::buffers` holds buffers by
    canonical path and hands out `LoadJob`s and `SaveJob`s for workers,
    whose results carry the buffer, its version and a ticket, so a stale
    one is `SaveReport::Stale`; a save in `SaveMode::Check` prepares its
    temporary file, reads it and the target through `platform::file`, and
    returns `SaveReport::Asks` with a `SaveQuestion` when the target has
    more than one link, its owner or group differ from the new file's,
    either is unknown or 65534, or a file appeared at a new file's path;
    `SaveMode::InPlace` and `SaveMode::NewFile` answer it. `files` reads a
    folder at a time (`FileTree`, `ReadDirJob`), at most 10,000 entries,
    each with its ignore state, never through a link. `session` keeps one
    TOML file a folder in `sessions/`, named by the SHA-256 of the
    folder's canonical path, refusing another folder's
    (`WorkspaceError::OtherFolder`), with splits at most 8 deep of 16
    panes, 64 groups, 1,024 editors and 1,024 cursors an editor, and the
    fuzz target `session` with `edge-*` seeds at those limits. Tests:
    `cargo test -p tidepool-workspace` passed (30 unit, 11 `buffers`, 6
    `files`, 9 `session`, 7 `fuzz`, and the earlier 10 `folder`, 15
    `settings`, 8 `trust` and 2 doc tests), and `cargo test -p
    tidepool-text` (37 `files` tests, with 6 new). The owner and group
    case is tested in a unit test whose metadata reader reports another
    group for the temporary file, since `workspace` may not use
    `cfg(unix)` to make a setgid folder; it was also run once by hand
    against a real setgid folder of another of this user's groups (107
    against the file's 1000), where the save asked and "write in place"
    kept the file's group and mode 0640. On systems whose metadata
    `platform` cannot read yet, every save over a file asks.
  - Note (2026-11-23): `cargo xtask bench workspace`:
    `workspace/buffers/open/100mb` at a median of 517 ms (p95 701 ms) at
    a load near 19, then 343 ms (p95 368 ms) and 336 ms (p95 399 ms) at a
    load of 2 to 4, the last saved to `bench/baseline.json` on the
    reference machine: within the 1 s budget. `cargo xtask fuzz-smoke
    --target session --secs 60` ran clean twice with seed
    `0xbb8aa7900f353951`: 2,012,557 inputs, then 2,128,726 on the final
    code, the slowest 7.80 ms (the first run's 652 ms, the 13 KB cursors
    seed, was a stall at a load near 16 that the same inputs did not
    repeat; that seed parses and writes in about 1.5 ms in the fuzz
    profile).
  - Note (2026-11-23): the security review found no critical or high
    issue. Fixed: its medium one, that `prepare` gave the temporary file
    the target's mode before the check while the file kept its own group,
    so that group could read the new text (it now stays 0600 until
    `commit`, ADR-0069 amended before its first commit, as the user
    chose); a save of a new file renaming over a file that appeared
    meanwhile (it asks); a clippy lint in a test; scratch folders of the
    new tests (now under `target/`) and of the benchmark (made anew with
    a name no other run has); and the file tree looks at the folders on
    its way again after listing. Left, as the user chose: tying an answer
    to the file asked about by its device and inode, in M4.3. Residual: a
    folder swapped for a link and back between the tree's two looks can
    still show another folder's names, and older tests (`folder.rs`,
    `trust.rs`, `settings.rs`) keep predictable scratch names in the
    system's temporary folder.

- [x] **M4.12 — platform: a file's device and inode; workspace: a save's answer tied to its file**
  - Depends: M4.11, M4.10
  - Do: per ADR-0070 and ADR-0069:
    - in `tidepool-platform`, the file's identity in `file::Metadata`:
      its device and inode number (`st_dev` and `st_ino`) and its birth
      time where the file system reports one, through `std`'s `stat` on
      Linux, `Unsupported` elsewhere as now; and `file::metadata_of`, the
      same for an open file;
    - in `tidepool-text`, `file::write_in_place_if`, a write in place
      that gives the file it opened to a caller's check before a byte is
      written;
    - in `tidepool-workspace`, answers bound to one question of one
      buffer and used once: `SaveMode::InPlace` and `SaveMode::NewFile`
      made only by the question, naming its buffer and save; the buffer
      keeping its last question until its next save takes it; another
      answer running as `SaveMode::Check`, and another buffer's an error;
      "write in place" checking the identity of the file it opened, and
      "save as a new file" asking the question again after `prepare` and
      renaming only when it is the one answered, or there is none; a look
      that failed never counting as knowing the file;
    - `docs/subsystems/sys-and-platform.md`,
      `docs/subsystems/text-and-formats.md`, `docs/subsystems/editor.md`,
      the three crates' READMEs and the diagrams that draw the save.
  - Accept: two hard links to one file report the same identity, and a
    copy, or a file deleted and made again at its path, another; an open
    file's identity stays its own when another file is renamed over its
    name; an answer to a question about one file, after its path was made
    to name another (renamed over, or deleted and made again), writes
    nothing into that other file and saves as a checked save would,
    asking again when that one needs it; an answer for the same file does
    what it says; an answer used a second time, or after another save,
    runs as a checked save, and one given to another buffer is refused; a
    group changed since the question makes "save as a new file" ask
    again; where the metadata cannot be read, an answer to a question
    that could not read it does what it says, and where a look failed,
    the answer asks again.
  - Verify: `cargo test -p tidepool-platform -p tidepool-text
    -p tidepool-workspace`
  - Note (2026-11-23): ADR-0070 records this task, split from M4.3. On
    the reference machine's ext4, a file deleted and made again got the
    same inode number in each of 2,000 tries and another birth time in
    each, so the user chose that day to put the birth time in the
    identity, and to let only a question make an answer.
  - Note (2026-11-24): built. `platform::file::Metadata` has `id:
    FileId` (`device`, `inode`, `born`), from `std`'s `stat`, and
    `file::metadata_of` reads an open file (`fstat`); `text::file` has
    `write_in_place_if` and `InPlace`, its check called with the opened
    file before a byte is written; `workspace::buffers` has
    `SaveMode::{NewFile, InPlace}(Answer)`, `Buffer::asked`,
    `WorkspaceError::AnswerToAnotherBuffer` and tickets from one counter
    for the process, and `SaveQuestion::of` is gone. `TIDEPOOL_IT_X11=1
    xvfb-run -a cargo test -p tidepool-platform -p tidepool-text -p
    tidepool-workspace` passed with no `SKIPPED` (platform 134 unit
    tests; text 112 unit and 39 `files`; workspace 35 unit and 15
    `buffers`, with the other suites and doc tests). Mutations were run
    by hand and restored: a check that accepts any opened file, one that
    takes any question as the one answered, one that accepts any ticket,
    and one that compares device and inode without the birth time each
    made tests fail. Clippy is clean for x86_64 Linux, aarch64 Linux,
    x86_64 macOS and x86_64 Windows, where the `Unsupported` paths were
    compiled, not run; an answer where nothing can be read is checked by
    unit tests with looks that report `Unsupported`.
  - Note (2026-11-24): two rounds of security review found nothing
    critical, high or medium. The first round's six low findings were
    fixed as the user chose (the opened file checked, adding `text`;
    answers bound to one buffer's question, used once) or as the review
    suggested (only `Unsupported` counts as not knowing; no public
    `SaveQuestion::of`; the docs give the window before a rename; the
    platform test skips rather than fails where birth times tie). The
    second round's three were fixed: tickets unique in the process, so
    another `Buffers`' answer answers nothing; the failed-look rule
    stated for each answer; stale doc text. Left, as before this task: a
    save as a new file looks before its commit's permission change, sync
    and rename, so a file renamed over the path during that sync is not
    seen; and a path swapped for a FIFO before `open` blocks a worker.

- [ ] **M4.13 — editor: explorer, open editors, tabs, splits, the trust prompt and the save's question**
  - Depends: M4.11, M4.7, M4.2
  - Do: per ADR-0070, ADR-0068 and `docs/subsystems/editor.md`:
    - in `tidepool-editor`, the explorer over `workspace::files`, every
      entry shown and ignored ones dimmed, in every folder, folders
      collapsed until opened; the open editors list; editors over
      `workspace::buffers`, a large or non-text file's shown read-only;
      editor groups side by side, at most 4, with `ui` splitters between
      them and a tab strip each, Ctrl+\ splitting the active editor to
      the right and Ctrl+1 to Ctrl+4 focusing a group; a session's
      layout restored into them, side by side in order, a fifth group's
      editors and later ones given to the fourth; the trust prompt, a
      modal dialog that says trusting a folder trusts every folder inside
      it, refuses `/`, the home folder and a folder that contains it, and
      warns, then allows, as ADR-0070 says; and the save's question as a
      modal dialog with its three answers (ADR-0068);
    - in `tidepool-workspace`, `trust`'s check of a folder from the
      metadata of it and every folder above it, and the store refusing
      to grant `/`, the home folder or a folder that contains it, a grant
      of one in `trust.toml` trusting nothing, with a warning;
    - golden scenes of the shell with the explorer, two groups, the trust
      prompt and the save's question, drawn by the binary's
      `tests/golden.rs`;
    - `docs/subsystems/editor.md`, both crates' READMEs and the diagrams
      that draw the views.
  - Accept: headless tests open a folder's entries in the explorer, with
    ignored ones dimmed and shown in trusted and untrusted folders;
    tabs open, switch and close; groups split, focus and close, at most
    4; a session's layout of stacked or more than 4 groups restores side
    by side; the trust prompt refuses and warns in each case ADR-0070
    names, and says that subfolders are trusted too; each of the save
    question's answers is reported to the owner; the golden scenes
    compare.
  - Verify: `cargo test -p tidepool-editor -p tidepool-workspace`;
    `cargo xtask golden`

- [ ] **M4.3 — app: folders, trust, settings, the clipboard and sessions in the window**
  - Depends: M4.12, M4.13, M4.11, M4.7, M4.2
  - Decide first (ADR-0068):
    - how a folder is opened: a command-line argument, a command that
      takes a typed path, or both (a native file dialog would need the
      desktop portal over D-Bus, a socket that rule 8 does not allow);
    - which folders the trust prompt warns about or refuses: `/`, the
      home folder, a folder whose parent others may write (M4.7's
      review);
    - whether the explorer shows the files that ignore files hide, in
      every folder or only in an untrusted one, so that `build.rs`,
      `.cargo/config.toml` and `.tidepool/settings.toml` cannot be
      hidden from a user deciding whether to trust it (M4.7's review);
    - what shape splits take (side by side only, or both ways) and how
      many editor groups there may be.
  - Do: per ADR-0070: in `tidepool-gui`, the window's clipboard and a
    waker that worker threads can call; in the binary, `tidepool
    [FOLDER]` opening one folder, with its `TrustStore` loaded from
    `trust.toml`, the trust prompt (M4.13) shown for an untrusted one,
    the editor's settings (soft wrap, ADR-0063; Tab's spaces, ADR-0065),
    the keymap's workspace layer from `Settings::table_layers`, settings
    resolved again when trust changes, the window's clipboard routed to
    the editor, worker threads running `workspace`'s jobs and the
    channel of their results to the loop, and the session restored when
    the folder opens; without the argument, `NO FOLDER` as now.
  - Accept: opening, editing and saving files round-trips in the window;
    untrusted state is the default; session restores; a large file opens
    read-only; the save warning shows before a save that would split a
    hard link or change the owner or group.
  - Verify: `cargo test -p tidepool-gui -p tidepool`;
    `TIDEPOOL_IT_X11=1 xvfb-run -a cargo test -p tidepool-gui -p tidepool`;
    `cargo xtask golden`
  - Note (2026-11-23): M4.7 leaves this task (ADR-0067): opening a folder
    in the binary, whose title strip says `NO FOLDER` until then, with
    its `TrustStore` loaded from `trust.toml` in the user data folder; the
    trust prompt, which must say that trusting a folder trusts every
    folder inside it; declaring and applying the editor's settings (soft
    wrap, ADR-0063; Tab's spaces, ADR-0065); and giving `editor::keymap` a
    trusted folder's `[keymap]` layer, from `Settings::table_layers`.
    M4.7's security review asks this task, besides: to warn about or
    refuse trusting `/`, the home folder, or a folder whose parent others
    may write (a grant is a path, so it outlives its folder and trusts
    what is later made there); to show the files an untrusted folder's
    ignore files hide, at least `build.rs`, `.cargo/config.toml` and
    `.tidepool/settings.toml`, so that they cannot be hidden from a user
    deciding whether to trust it; and to resolve the settings again when
    trust changes, since a `SettingsFile` read before a revoke stays
    usable.
  - Note (2026-11-23): ADR-0068 split this task, which was `workspace:
    buffers and session; editor: explorer, tabs, splits, trust prompt`:
    M4.10 gives `platform` the file metadata a save checks, and M4.11
    builds `workspace`'s `buffers`, the file tree model of `files` and
    `session`, with the 100 MB open budget; this task keeps the views
    and the binary, and its `Decide first` line holds their questions.
    The user chose that day that sessions live in the user data folder,
    that a save which would split a hard link or change the owner or
    group asks whether to write in place or save as a new file, and that
    a large file loads whole and opens read-only.
  - Note (2026-11-23): M4.11's security review leaves this task two
    things (ADR-0069). An answer to the save's question is not tied to the
    file it was asked about, so this task adds the device and inode number
    to `platform::file::Metadata` and to `workspace::buffers::SaveQuestion`,
    and before a save in `SaveMode::InPlace` or `SaveMode::NewFile` acts on
    an answer, compares them and asks again if the path names another file
    (the user chose that day to leave it here rather than add `platform`
    to M4.11). And a session's editors may name files outside the folder,
    so restoring one opens them as untrusted, as any file of an untrusted
    folder is.
  - Note (2026-11-23): ADR-0070 answers the `Decide first` line with the
    options the user chose that day, each the recommended one, and
    splits this task, which was `editor: explorer, open editors, tabs,
    splits, trust prompt; app: folders and sessions`, since it reached
    five crates: M4.12 ties a save's answer to its file by device and
    inode (`platform`, `workspace`), M4.13 builds the views (`editor`,
    `workspace`), and this task keeps `gui`'s clipboard and waker and the
    binary. A folder opens from the command line; the trust prompt
    refuses `/`, the home folder and a folder that contains it, and
    warns, then allows, when the folder or one above it may be written
    by others or by a group other than the process's effective group, or
    is owned by a user other than it and root; the explorer shows ignored
    entries dimmed in every folder; editor groups sit side by side, at
    most 4. The last three answers shape M4.13, and the device and inode
    of the save's question, from M4.11's review, is M4.12's.

- [ ] **M4.8 — regex: the engine and Unicode classes**
  - Depends: M0.2, M1.6
  - Decide first: Unicode general categories and scripts for the classes
    extend ADR-0018 (more of the official UCD files, and generated tables),
    which needs the user's approval to download, as ADR-0018 did.
  - Do: `tidepool-regex` per `docs/subsystems/text-and-formats.md`: parser,
    NFA, Pike VM, Unicode classes, simple case folding, literal-prefix
    acceleration; a fuzz target for compiling patterns.
  - Accept: regex conformance table; pathological patterns finish in linear
    time.
  - Verify: `cargo test -p tidepool-regex`;
    `cargo xtask fuzz-smoke --target regex --secs 60`

- [ ] **M4.4 — search: find and replace, project search, file finder**
  - Depends: M4.8, M4.3
  - Do: find/replace in a file; `search` and the project search panel on
    workers, with streaming results; the fuzzy file finder and its index in
    `files`.
  - Accept: project search budget met; results for an older buffer version
    are dropped.
  - Verify: `cargo test -p tidepool-workspace -p tidepool-editor`;
    `cargo xtask bench search`

- [ ] **M4.5 — platform: file watching (Linux)**
  - Depends: M2.2, M4.3
  - Do: inotify watcher with debounce and polling fallback.
  - Accept: external edits reload or prompt correctly; watcher limits
    produce the fix-it message.
  - Verify: `cargo test -p tidepool-platform -p tidepool-workspace`
  - Note (2026-11-23): reloading the settings when `settings.toml` or a
    folder's `.tidepool/settings.toml` changes, so that a binding takes
    effect without a restart (ADR-0064), waits for this task's watcher
    (ADR-0067).

- [ ] **M4.9 — app: crash reports and buffer recovery**
  - Depends: M3.4, M4.3
  - Do: the panic hook of `docs/05-release.md` → "Crash reporting":
    `crashes/<timestamp>.txt` with the version, OS, backtrace and the last
    200 log lines (redacted), unsaved buffers saved to `recovery/`, and
    recovery offered on the next start.
  - Accept: an injected panic writes a report holding no registered secret,
    and the unsaved buffers come back after a restart.
  - Verify: `cargo test -p tidepool`

**Exit criteria (M4):** the owner uses Tidepool for a day of edits to
Tidepool, and every bug found is filed.

---

## M5 — Managed toolchains and the terminal

**Goal:** Tidepool installs and drives rust-analyzer and git (ADR-0002).

- [ ] **M5.1 — toolchain: descriptors, detection, registry**
  - Depends: M1.7, M1.8
  - Decide first: the dependency matrix lets no crate but `tidepool-bench`
    use `tidepool-testkit`, yet the test plan has the tests of `toolchain`,
    `vcs`, `mobile`, `rpc` and `lang` use its fakes and transcripts
    (`docs/04-quality.md` → `layers`). Decide those edges before the first
    fake tool.
  - Do: `descriptor`, `detect`, `registry`; `toolchains/*.toml` for
    rustup, the Rust toolchain, rust-analyzer, git and curl;
    `cargo xtask tools`.
  - Accept: fake tools cover found, missing, outdated and broken states.
  - Verify: `cargo test -p tidepool-toolchain`; `cargo xtask tools`

- [ ] **M5.7 — platform: processes (Linux)**
  - Depends: M2.1
  - Do: `platform::process` per `docs/subsystems/sys-and-platform.md`:
    pipes with `O_CLOEXEC`, a process group per child, kill-tree and
    reaping; the declarations in `sys` through `/add-ffi-binding`.
  - Accept: a child's own children die with its tree; no zombie is left
    after drop; no file descriptor leaks into a child (tests).
  - Verify: `cargo test -p tidepool-platform`; `cargo xtask unsafe-audit`

- [ ] **M5.2 — toolchain: processes, sessions and secrets**
  - Depends: M5.1, M5.7
  - Do: `process`, `sessions`, `secret` per `docs/subsystems/toolchains.md`.
  - Accept: timeouts, cancellation and kill-tree work; no secret appears in
    argv, `Debug` or logs (test).
  - Verify: `cargo test -p tidepool-toolchain`

- [ ] **M5.3 — toolchain: downloads and install plans**
  - Depends: M5.2, M0.2
  - Do: `download` via `curl` (ADR-0016, accepted 2026-10-31);
    `install` plans with consent; licenses shown.
  - Accept: a checksum mismatch deletes the file and runs nothing; plans
    list every step before anything runs.
  - Verify: `cargo test -p tidepool-toolchain`

- [ ] **M5.8 — term: VT parser and grid**
  - Depends: M0.2, M1.6
  - Decide first: display width. A wide character (East Asian Wide, most
    emoji) takes two cells, in this grid and in the TUI (M7.1). Generating
    the table from `EastAsianWidth.txt` extends ADR-0018 and needs the
    user's approval to download, as ADR-0018 did. Decide also where the
    table lives: both `term` and `tui` may use `core` and `platform`, and
    `core` is where pure data such as this fits.
  - Do: the VT parser and grid of `docs/subsystems/toolchains.md` →
    `tidepool-term`: controls, CSI, SGR with truecolor, the OSC and DEC
    private modes listed, wide characters, scrollback, reflow, selection
    and search.
  - Accept: conformance against recorded byte streams from real programs;
    the parser is a fuzz target.
  - Verify: `cargo test -p tidepool-term`;
    `cargo xtask fuzz-smoke --target vt --secs 60`

- [ ] **M5.9 — platform: PTY (Linux)**
  - Depends: M5.7
  - Do: `platform::pty` per `docs/03-platforms.md` → Linux.
  - Accept: the PTY tests of `docs/subsystems/sys-and-platform.md`: spawn
    through the `Pty` API, check output, exit status and resize.
  - Verify: `cargo test -p tidepool-platform`; `cargo xtask unsafe-audit`

- [ ] **M5.4 — editor: integrated terminal (Linux)**
  - Depends: M5.8, M5.9, M3.4
  - Do: the terminal panel, on `tidepool-term` and `platform::pty`.
  - Accept: `vim -u NONE`, `less`, `htop` and `git add -p` run correctly in
    scripted snapshot tests.
  - Verify: `cargo test -p tidepool-term -p tidepool-editor`;
    `cargo xtask golden`

- [ ] **M5.5 — Rust toolchain and rust-analyzer installs; git**
  - Depends: M5.3, M5.4
  - Do: install/update flows for rustup, the toolchain and rust-analyzer;
    git detection and the package-manager plan run in the terminal.
  - Accept: on a clean Linux user account, Tidepool installs
    rust-analyzer with consent and detects git (gated real-tool test).
  - Verify: `TIDEPOOL_IT_RA=1 cargo test -p tidepool-toolchain`

- [ ] **M5.6 — editor: Toolchains panel and first-run checklist**
  - Depends: M5.5
  - Do: cards, status dots, actions, logs per the spec.
  - Accept: goldens for each state; actions run the right plans.
  - Verify: `cargo xtask golden`

**Exit criteria (M5):** a new user gets from a clean account to a working
Rust setup inside Tidepool without leaving it, and no install ran without a
confirmation.

---

## M6 — Rust and Markdown first-class

**Goal:** Tidepool is a good Rust and Markdown editor (ADR-0005).

- [ ] **M6.1 — rpc: JSON-RPC transport and LSP client; testkit fake server**
  - Depends: M1.4, M1.6, M5.1
  - Do: per `docs/subsystems/languages.md`, with the testkit edges M5.1
    decided.
  - Accept: transcript tests including cancellation, crash and malformed
    messages; framing is a fuzz target.
  - Verify: `cargo test -p tidepool-rpc -p tidepool-testkit`

- [ ] **M6.2 — syntax: Rust, Markdown and TOML highlighters; language packs; highlighting in the editor**
  - Depends: M1.1, M1.7, M4.1
  - Do: incremental lexers; the pack format; packs for Rust, Markdown and
    TOML; the editor view shows highlights, computed on a worker for
    buffers above 1 MB, with versioned results.
  - Accept: golden token streams; incremental equals full re-lex
    (property); goldens of highlighted Rust and Markdown.
  - Verify: `cargo test -p tidepool-syntax`; `cargo xtask golden`

- [ ] **M6.7 — lang: rust-analyzer sessions and diagnostics**
  - Depends: M6.1, M5.5, M4.3
  - Do: `registry`, `session` (`initialize` with `positionEncoding`,
    incremental sync, crash restart through `toolchain::sessions`), and
    `rust` (no build scripts or proc macros in untrusted workspaces,
    ADR-0014; `rust-toolchain.toml` respected); diagnostics in the gutter.
    Positions follow ADR-0019: the offset between CR and LF moves before it
    is sent, and positions are mapped where rust-analyzer's lines differ
    from Tidepool's (a lone CR).
  - Accept: fake-server suite for lifecycle, sync, cancellation, crash and
    malformed messages, with non-ASCII lines (combining marks, emoji, CJK)
    in UTF-8 and UTF-16; real rust-analyzer on Tidepool's workspace reports
    diagnostics (gated).
  - Verify: `cargo test -p tidepool-lang`;
    `TIDEPOOL_IT_RA=1 cargo test -p tidepool-lang`

- [ ] **M6.8 — lang: navigation and hover**
  - Depends: M6.7
  - Do: definition, declaration, type definition, implementation,
    references, hover, signature help, document and workspace symbols and
    inlay hints, with their popups and lists in the editor.
  - Accept: fake-server suite; real rust-analyzer resolves a known
    definition in Tidepool's workspace (gated).
  - Verify: `cargo test -p tidepool-lang -p tidepool-editor`;
    `TIDEPOOL_IT_RA=1 cargo test -p tidepool-lang`

- [ ] **M6.3 — lang: completion, rename, formatting, code actions, semantic tokens, folding**
  - Depends: M6.8
  - Do: the rest of the `features` table in `docs/subsystems/languages.md`;
    the completion popup; a rename or code action applies as one undo step.
  - Accept: fake-server suite; every feature in the `lang` table works with
    real rust-analyzer on Tidepool's workspace (gated).
  - Verify: `cargo test -p tidepool-lang -p tidepool-editor`;
    `TIDEPOOL_IT_RA=1 cargo test -p tidepool-lang`

- [ ] **M6.4 — lang: cargo tasks, problems panel, test discovery**
  - Depends: M6.7
  - Do: `lang::cargo`, Problems panel.
  - Accept: a build error appears with the right range, including on
    non-ASCII lines and after a lone CR.
  - Verify: `cargo test -p tidepool-lang`

- [ ] **M6.5 — markdown: CommonMark + GFM parser**
  - Depends: M0.2, M1.6
  - Decide first: vendoring the CommonMark spec examples as test-only
    fixtures, once the user confirms their license (CC-BY-SA 4.0, to be
    verified; `docs/02-dependency-policy.md` → Assets).
  - Do: per the spec; a fuzz target.
  - Accept: all core spec examples pass; GFM extension pass rate recorded.
  - Verify: `cargo test -p tidepool-markdown`

- [ ] **M6.6 — editor: Markdown preview, outline, link checks**
  - Depends: M6.5, M6.2
  - Do: preview in the `.prose` style; outline; relative-link and anchor
    diagnostics.
  - Accept: preview goldens match the spec's prose styles; broken links are
    reported.
  - Verify: `cargo xtask golden`; `cargo test -p tidepool-lang`

**Exit criteria (M6):** the owner does Tidepool's Rust and docs work in
Tidepool with rust-analyzer and the preview.

---

## M7 — Terminal UI and vim mode

**Goal:** `tidepool --tui` with vim-style modal editing (ADR-0003,
ADR-0006).

- [ ] **M7.1 — platform: terminal guard; tui: renderer and input decoder**
  - Depends: M3.4, M5.8
  - Do: per `docs/subsystems/frontends.md`, with M5.8's display widths.
  - Accept: TUI snapshots of the shell; decoder tables for xterm, kitty,
    tmux and the Linux console; terminal restored after an injected panic.
  - Verify: `cargo test -p tidepool-tui -p tidepool-platform`

- [ ] **M7.2 — vim mode: grammar, motions, operators, text objects**
  - Depends: M4.2
  - Do: modes, command grammar, the Motions, Operators, Text objects and
    Actions rows of `docs/subsystems/vim-mode.md`.
  - Accept: conformance fixtures for each command in those rows.
  - Verify: `cargo test -p tidepool-editor vim`

- [ ] **M7.3 — vim mode: visual modes, registers, marks, macros, dot-repeat**
  - Depends: M7.2
  - Do: the remaining rows except Ex commands. Vim's marks follow lines
    where Tidepool's positions follow bytes; if the conformance fixtures
    need Vim's rules, revisit ADR-0020 first.
  - Accept: conformance fixtures; the undo property test.
  - Verify: `cargo test -p tidepool-editor vim`

- [ ] **M7.4 — vim mode: search, ex commands, IDE bindings, settings**
  - Depends: M7.3, M4.4, M6.8
  - Do: `/ ? n N * #`, the listed ex commands, `gd gr K [d ]d`, leader
    mappings, `editor.keymap`, first-TUI-launch prompt.
  - Accept: conformance fixtures; with `TIDEPOOL_IT_VIM=1`, the checker
    agrees with `vim -u NONE -N` on the suite.
  - Verify: `cargo test -p tidepool-editor vim`;
    `TIDEPOOL_IT_VIM=1 cargo test -p tidepool-editor vim`

- [ ] **M7.5 — TUI end to end**
  - Depends: M7.1, M7.4
  - Do: run `tidepool --tui` inside a PTY in a test: open, edit in vim mode,
    save, quit; OSC 52 clipboard; cursor shapes.
  - Accept: the scripted session's final file and snapshots match.
  - Verify: `cargo test -p tidepool --test tui_session`

**Exit criteria (M7):** the owner can work in Tidepool over SSH in vim mode.

---

## M8 — Git

**Goal:** everyday git without leaving Tidepool.

- [ ] **M8.1 — vcs: status, diff hunks, gutter**
  - Depends: M5.2, M1.6
  - Do: porcelain v2 and diff parsing; gutter markers; fuzz targets.
  - Accept: fixture repos produce the expected states.
  - Verify: `cargo test -p tidepool-vcs`;
    `TIDEPOOL_IT_GIT=1 cargo test -p tidepool-vcs`

- [ ] **M8.2 — editor: source control panel**
  - Depends: M8.1, M4.3
  - Do: changes list, stage/unstage files and hunks, commit (message on
    stdin), branch switch, stash; all on explicit user action.
  - Accept: actions on fixture repos produce the expected `git` results.
  - Verify: `TIDEPOOL_IT_GIT=1 cargo test -p tidepool-editor scm`

- [ ] **M8.3 — editor: diff view, blame, history**
  - Depends: M8.1, M4.3
  - Do: side-by-side and inline diff; blame gutter; file history.
  - Accept: goldens; blame on a fixture matches `git blame`.
  - Verify: `cargo xtask golden`; `TIDEPOOL_IT_GIT=1 cargo test -p tidepool-vcs`

**Exit criteria (M8):** a week of Tidepool's own commits made from Tidepool's
git panel.

---

## M9 — AI and predictive text

**Goal:** language agents and offline prediction (ADR-0009).

- [ ] **M9.1 — TAP spec review**
  - Depends: M6.1
  - Do: review `docs/subsystems/agents.md` with the user; accept ADR-0009.
  - Accept: the user accepts the spec (recorded in the ADR).
  - Verify: doc review (human)

- [ ] **M9.2 — agents/: mock agent and conformance suite**
  - Depends: M9.1
  - Decide first (`docs/04-quality.md` → `deps` calls both provisional):
    whether the reference agents may use root crates such as `core`, which
    `cargo xtask deps` refuses today; and whether `agents/` needs members
    beyond the three agents, such as shared TAP code or the conformance
    suite.
  - Do: create the `agents/` workspace and `tp-agent-mock`; TAP suite.
  - Accept: the suite passes against the mock; `cargo xtask deps` covers
    `agents/`.
  - Verify: `cargo test --manifest-path agents/Cargo.toml`;
    `cargo xtask deps`

- [ ] **M9.3 — agent: host, consent, context assembly**
  - Depends: M9.2, M5.2, M4.3
  - Do: `manifest`, `host`, `tap`, `context`, `consent`.
  - Accept: deny-list tests; untrusted workspaces send only the focused
    buffer with consent; crash/restart test.
  - Verify: `cargo test -p tidepool-agent`

- [ ] **M9.4 — agent: predictive engine; editor: completion merge and ghost text**
  - Depends: M9.3, M6.3
  - Do: `predict`, `merge`; popup and ghost text.
  - Accept: the responsiveness suite with a 2 s mock agent stays within
    budget; stale results never show.
  - Verify: `cargo test -p tidepool-agent -p tidepool-editor`;
    `cargo xtask bench latency`

- [ ] **M9.5 — editor: chat panel and edit proposals**
  - Depends: M9.4
  - Do: chat with streaming; proposals as diffs; accept applies one undo
    step.
  - Accept: goldens; proposal accept/reject tests.
  - Verify: `cargo test -p tidepool-editor agent`; `cargo xtask golden`

- [ ] **M9.6 — agents/: local and Claude reference agents**
  - Depends: M9.2
  - Do: `tp-agent-local` (loopback only) and `tp-agent-claude` (curl, key
    via `--config -`), per the spec; check the current Anthropic API docs.
  - Accept: unit tests with recorded responses; no key in argv or logs;
    live test opt-in only.
  - Verify: `cargo test --manifest-path agents/Cargo.toml`

**Exit criteria (M9):** with the Claude agent installed, the owner gets
inline completions and reviewable edits in Rust and Markdown; with no agent,
local prediction still works.

---

## M10 — Android

**Goal:** build, sign, verify and run Android apps (ADR-0007).

- [ ] **M10.1 — toolchain: JDK, Android SDK, NDK, bundletool**
  - Depends: M5.3
  - Do: descriptors and flows; `sdkmanager --licenses` in the terminal for
    the user; re-check Android's current docs (including which AAPT2 to
    use) and record findings in task notes.
  - Accept: fake-tool tests; gated real install on Linux.
  - Verify: `cargo test -p tidepool-toolchain`;
    `TIDEPOOL_IT_ANDROID=1 cargo test -p tidepool-toolchain android`

- [ ] **M10.7 — codec: inflate, deflate and PNG**
  - Depends: M1.4, M1.6
  - Do: `inflate` and `png` (port Nadir's) and `deflate`, per
    `docs/subsystems/text-and-formats.md`; a fuzz target for each decoder.
  - Accept: hand-made inflate vectors (stored, fixed and dynamic blocks,
    back-references at the window edge); PNG decodes every color type and
    depth; deflate output inflates back to its input.
  - Verify: `cargo test -p tidepool-codec`; `cargo xtask fuzz-smoke --secs 60`

- [ ] **M10.2 — codec: ZIP and XML; mobile: artifact inspector**
  - Depends: M10.7
  - Do: `zip`, `xml`; `mobile::inspect`.
  - Accept: archives from Android tools read correctly; traversal rejected;
    fuzz targets.
  - Verify: `cargo test -p tidepool-codec -p tidepool-mobile`;
    `cargo xtask fuzz-smoke --secs 60`

- [ ] **M10.3 — mobile: keystores and signing**
  - Depends: M10.1
  - Decide first: accept ADR-0007 (signing delegated to `apksigner`,
    `jarsigner` and `codesign`), which is `Proposed`.
  - Do: `android::sign` per `docs/subsystems/mobile.md`.
  - Accept: no secret in any constructed argv (test); signed artifacts pass
    the verifiers (gated).
  - Verify: `cargo test -p tidepool-mobile`;
    `TIDEPOOL_IT_ANDROID=1 cargo test -p tidepool-mobile sign`

- [ ] **M10.4 — mobile: Gradle projects**
  - Depends: M10.3
  - Do: `android::gradle` in trusted workspaces; artifacts then Tidepool
    signing.
  - Accept: a sample Gradle app produces a signed APK and AAB that pass the
    verifiers (gated).
  - Verify: `TIDEPOOL_IT_ANDROID=1 cargo test -p tidepool-mobile gradle`

- [ ] **M10.5 — mobile: Gradle-free Java and Rust NativeActivity apps**
  - Depends: M10.2, M10.3
  - Do: `android::direct` for both samples.
  - Accept: both produce a signed APK and AAB that pass every verifier and
    install on an emulator (gated).
  - Verify: `TIDEPOOL_IT_ANDROID=1 cargo test -p tidepool-mobile direct`

- [ ] **M10.6 — editor: SDK manager, devices, logcat, build artifacts**
  - Depends: M10.1, M10.5, M4.3
  - Do: panels per `docs/subsystems/editor.md`.
  - Accept: goldens; device actions run the right commands.
  - Verify: `cargo xtask golden`; `cargo test -p tidepool-editor mobile`

**Exit criteria (M10):** from a clean account, the owner installs the SDK,
builds both samples, and runs them on an emulator, all inside Tidepool.

---

## M11 — macOS and iOS

**Goal:** Tidepool on macOS, and iOS artifacts (ADR-0008).

- [ ] **M11.7 — sys/platform (macOS): Objective-C glue, window, events, present**
  - Depends: M2.2, M2.7
  - Decide first: accept ADR-0015 (Linux, then macOS, then Windows), which
    is `Proposed`; the milestones before this one follow it already.
  - Do: per `docs/03-platforms.md` → macOS, through `/add-ffi-binding`;
    port Nadir's Objective-C patterns where they fit.
  - Accept: M2.2's window, event and waker tests pass on macOS.
  - Verify: `cargo test -p tidepool-platform` on macOS
    - ⏳ needs macOS

- [ ] **M11.8 — platform (macOS): clipboard, paths, processes, PTY, FSEvents**
  - Depends: M11.7, M5.9, M4.5
  - Do: per `docs/03-platforms.md` → macOS.
  - Accept: the Linux clipboard, path, process, PTY and watcher tests pass
    on macOS.
  - Verify: `cargo test -p tidepool-platform -p tidepool-workspace` on macOS
    - ⏳ needs macOS
  - Note (2026-11-23): this task also gives `platform`'s file metadata
    for saves on macOS, whose new files take their folder's group
    (ADR-0068, M4.10).

- [ ] **M11.1 — sys/platform: macOS**
  - Depends: M11.8
  - Do: whatever else `cargo xtask ci` needs on macOS.
  - Accept: the Linux platform tests pass on macOS.
  - Verify: `cargo xtask ci` on macOS
    - ⏳ needs macOS

- [ ] **M11.2 — toolchain: Xcode, CLT, simulators**
  - Depends: M11.1, M5.3
  - Do: detection, install guidance, license status; re-check Apple's
    current tools and record findings.
  - Accept: fake-tool tests; gated real detection on macOS.
  - Verify: `TIDEPOOL_IT_IOS=1 cargo test -p tidepool-toolchain ios`

- [ ] **M11.3 — mobile: identities, profiles, Xcode projects**
  - Depends: M11.2
  - Decide first: accept ADR-0008 (iOS builds on macOS, locally or on a
    remote Mac), which is `Proposed`.
  - Do: `ios::sign`, `ios::xcode`, plist writer.
  - Accept: a sample Xcode app produces development and App Store `.ipa`
    files that pass `codesign --verify --deep --strict` (gated).
  - Verify: `TIDEPOOL_IT_IOS=1 cargo test -p tidepool-mobile xcode`

- [ ] **M11.4 — mobile: Xcode-free Rust iOS apps**
  - Depends: M11.3, M10.2
  - Do: `ios::direct`.
  - Accept: the Rust sample builds, signs, packages and launches in the
    simulator (gated).
  - Verify: `TIDEPOOL_IT_IOS=1 cargo test -p tidepool-mobile direct_ios`

- [ ] **M11.5 — mobile: remote-Mac builds from Linux**
  - Depends: M11.4
  - Do: `ios::remote` over `ssh`/`rsync`.
  - Accept: from Linux, the samples produce the same artifacts as on the Mac.
  - Verify: gated test with `TIDEPOOL_REMOTE_MAC=<host>`

- [ ] **M11.6 — editor: simulators and signing panels; macOS package**
  - Depends: M11.3
  - Decide first: `package` runs `lipo` and `iconutil`, which ADR-0017's
    exception does not cover; extend it.
  - Do: panels; `xtask package macos` per `docs/05-release.md`.
  - Accept: goldens; the package launches.
  - Verify: `cargo xtask golden`; `cargo xtask package macos`

**Exit criteria (M11):** iOS samples build and sign on a Mac and from Linux.

---

## M12 — Windows

- [ ] **M12.1 — sys/platform: Win32 window, events, present, clipboard, DPI**
  - Depends: M4.5, M5.4
  - Do: per `docs/03-platforms.md` → Windows.
  - Accept: the Linux platform tests pass on Windows.
  - Verify: `cargo xtask ci` on Windows
    - ⏳ needs Windows

- [ ] **M12.2 — platform: ConPTY, ReadDirectoryChangesW, paths**
  - Depends: M12.1
  - Do: per `docs/03-platforms.md` → Windows.
  - Accept: the terminal snapshot tests and watcher tests pass on Windows.
  - Verify: `cargo test -p tidepool-platform -p tidepool-term` on Windows
  - Note (2026-11-23): this task also gives `platform`'s file metadata
    for saves on Windows: a file's link count, and whatever stands for
    its owner there (ADR-0068, M4.10).

- [ ] **M12.3 — toolchain: winget plans, rustup-init.exe, JDK and SDK paths**
  - Depends: M12.1, M5.3
  - Do: Windows install plans per `docs/subsystems/toolchains.md`.
  - Accept: fake-tool tests; gated real install of rust-analyzer.
  - Verify: `cargo test -p tidepool-toolchain` on Windows

- [ ] **M12.4 — Windows package**
  - Depends: M12.3
  - Decide first: `package` runs `rc.exe`, which ADR-0017's exception does
    not cover; extend it.
  - Do: `xtask package windows` per `docs/05-release.md`.
  - Accept: the package launches on a clean Windows account.
  - Verify: `cargo xtask package windows`; manual launch

- [ ] **M12.5 — Repository tooling on Windows**
  - Depends: M12.1
  - Do: the Claude Code hook (`.claude/hooks/deps-guard.sh`) does not decode
    JSON escapes in the file path it is given, so on Windows, where every
    path has one, it checks the project instead of the worktree
    (`docs/04-quality.md` → "Command line and rules"). Decode them, and
    check that every `xtask` check agrees on Windows paths and line endings.
  - Accept: an edit in a linked worktree on Windows is checked in that
    worktree; the `xtask` suite passes on Windows.
  - Verify: `cargo test -p xtask` on Windows
    - ⏳ needs Windows

**Exit criteria (M12):** M4–M10 exit criteria hold on Windows.

---

## M13 — Debugging and language expansion

- [ ] **M13.1 — rpc/lang: DAP client and Rust debugging**
  - Depends: M6.3, M5.3
  - Do: managed `lldb-dap` or CodeLLDB; breakpoints, stepping, stack,
    variables, watch, console.
  - Accept: a debug session on a sample binary hits a breakpoint and shows
    locals (gated).
  - Verify: `cargo test -p tidepool-lang dap`

- [ ] **M13.2 — syntax: declarative pack lexers**
  - Depends: M6.2, M4.8
  - Do: lexer rules in language packs, compiled to `tidepool-regex`.
  - Accept: a pack-defined lexer for TOML matches the hand-written one's
    golden tokens.
  - Verify: `cargo test -p tidepool-syntax`

- [ ] **M13.3 — language packs for common languages**
  - Depends: M13.2
  - Do: C/C++, Python, JavaScript/TypeScript, Go, Java, Kotlin, Swift,
    Shell, SQL, HTML/CSS, YAML, JSON, Dockerfile, Makefile; LSP mappings as
    managed tools.
  - Accept: highlight goldens per pack; documented server setup.
  - Verify: `cargo test -p tidepool-syntax packs`

- [ ] **M13.4 — editor: image and hex viewers**
  - Depends: M10.7, M4.3
  - Do: PNG files open in an image viewer and binary files in a hex view,
    per `docs/subsystems/editor.md` → "Editor view".
  - Accept: goldens; a malformed PNG shows an error, never a panic.
  - Verify: `cargo xtask golden`; `cargo test -p tidepool-editor`

---

## M15 — Lua plugins

**Goal:** users extend Tidepool with plugins written in Lua, a small
scripting language made to be embedded in other programs, run by
Tidepool's own interpreter. A plugin reaches only what the plugin API
gives it: it opens no file, socket, or program of its own, and it never
makes the main thread wait (ADR-0061).

Added on 2026-11-23 at the owner's request. M15 is the next free
milestone number. It is listed before M14 so that the first release has
plugins, and so that M14's fuzz campaign and security review cover them.

- [ ] **M15.1 — lua: lexer, parser, and compiler**
  - Depends: M0.2, M1.6
  - Decide first:
    - Accept ADR-0061 (plugins are Lua 5.5 scripts run by `tidepool-lua`,
      an interpreter of our own with no way to reach the OS, on a thread
      apart from the main thread), which is `Proposed`.
    - Vendoring the official Lua 5.5 test suite as test-only fixtures,
      once the user confirms its license (MIT, as lua.org/tests said on
      2026-11-23; `docs/02-dependency-policy.md` → Assets).
  - Do:
    - Create `tidepool-lua` (`crates/lua`, layer 1, `core` only) with its
      README, `LICENSE` and `NOTICE`; its rows in `AGENTS.md`'s workspace
      map and in `docs/01-architecture.md`'s layers and dependency
      matrix; the diagrams that draw them (ADR-0029); its path in
      `.claude/rules/parsers.md`; and its owner among the subagents in
      `CLAUDE.md`.
    - The lexer and parser for the whole language of the Lua 5.5
      reference manual, and the compiler to bytecode (the instructions
      M15.2's virtual machine runs), with `Limits` for the source's size,
      its nesting depth, and the manual's limits on a function's locals,
      upvalues (the outer variables a closure captures), and constants.
    - The fuzz target `lua-parse`, which parses and compiles.
  - Accept:
    - Every rule of the manual's complete syntax of Lua parses, with a
      test for each rule; a syntax error names its line and the token it
      found.
    - Nesting past its limit is an error, never a stack overflow, and no
      input panics.
  - Verify: `cargo test -p tidepool-lua`;
    `cargo xtask fuzz-smoke --target lua-parse --secs 60`
  - Note (2026-11-23): the user accepted ADR-0061 as written, which
    answers the first question of the `Decide first` line. The second,
    vendoring the official Lua 5.5 test suite, is still open.
  - Note (2026-11-23): the user chose to vendor the suite, which answers
    the second question. ADR-0062 records how: this task downloads
    `lua-5.5.1-tests.tar.gz` with the user's approval, checks its
    SHA-256, and copies its Lua files, unchanged and without its C
    sources, into `crates/lua/tests/lua-5.5.1-tests/`, with a
    `NOTICE.md`.

- [ ] **M15.2 — lua: virtual machine, garbage collector, and coroutines**
  - Depends: M15.1, M1.9
  - Do:
    - Per the manual: the values, tables with an array part and a hash
      part, metatables and metamethods, closures and their upvalues,
      integer and float arithmetic, `error`, `pcall` and `xpcall` with
      tracebacks, coroutines, and to-be-closed variables.
    - Calls from Lua to Lua that do not recurse on Rust's own stack, so
      deep recursion in a script cannot overflow it.
    - The garbage collector, in safe Rust (ADR-0061): objects live in an
      arena (a table of slots addressed by index), and an incremental
      mark-and-sweep collector frees, in bounded steps, every object that
      nothing reachable refers to, cycles included; weak tables and
      `__gc` finalizers.
    - The limits that stop any call cleanly: fuel (how many instructions
      a call may run before it is stopped), a memory cap per state, a
      call depth, and an interrupt that another thread can raise. A
      limit's error ends the whole call: `pcall` cannot catch it.
    - The fuzz target `lua-run`, which runs mutated programs under small
      limits, and a `tidepool-bench` suite (calls, table inserts, string
      building, and a full collection) with its baseline.
  - Accept:
    - The conformance scripts in `crates/lua/tests/` pass, each with its
      expected output recorded from the reference `lua` 5.5 interpreter,
      whose version the fixture names.
    - `while true do end` stops at its fuel limit, a script that
      allocates without end stops at its memory cap, and endless
      recursion stops at the call depth, each with the error the host
      sees.
    - A cycle of tables is collected: memory returns to its level before
      the cycle was made (property test).
  - Verify: `cargo test -p tidepool-lua`;
    `cargo xtask fuzz-smoke --target lua-run --secs 60`;
    `cargo xtask bench lua`

- [ ] **M15.3 — lua: the sandboxed standard library**
  - Depends: M15.2
  - Decide first: `os.clock` measures processor time and `os.date` local
    time, but `lua` may use only `core`, which reads neither. Decide
    whether `clock` counts the time since the state was made and `date`
    gives UTC only, or whether the host passes both in.
  - Do: the standard library ADR-0061 keeps: the basic functions without
    `dofile` and `loadfile`, and `load` for source text only; `string`,
    with Lua's patterns, and without `dump`; `table`, `math`, `utf8`, and
    `coroutine`; and of `os`, only `time`, `difftime`, `clock`, and
    `date`. There is no `io`, no `debug`, and no `package` loader:
    `require` is the plugin host's (M15.5). Pattern matching, which
    backtracks, charges fuel as it goes.
  - Accept:
    - Conformance scripts for each library pass, with their output
      recorded as in M15.2.
    - If M15.1 vendored the official suite: its files that need only the
      language and the kept library pass, and each file skipped is
      listed with its reason.
    - A test lists the globals of a new state and fails on any name
      outside ADR-0061's list.
    - A pattern crafted to backtrack stops at its fuel limit.
  - Verify: `cargo test -p tidepool-lua`;
    `cargo xtask fuzz-smoke --target lua-run --secs 60`

- [ ] **M15.4 — plugins: spec review**
  - Depends: M15.3, M4.3, M4.6
  - Decide first:
    - Which grants the first API has beyond ADR-0061's minimum, such as
      reading workspace files that are not open, and whether any grant
      reaches the files that the agents' deny-list keeps from them
      (`docs/subsystems/agents.md`).
    - The limits' values: fuel per call, memory per plugin, and call
      depth.
    - Whether a save waits for a plugin that asks to change the buffer
      first (trimming whitespace on save, say), and for how long.
  - Do: write `docs/subsystems/plugins.md` with the user: the plugin
    folder (`plugin.toml` and `init.lua`), where plugins load from and
    when they may run, enabling and the grants shown, the `tidepool`
    API's modules and events, errors and limits, and the plugin thread;
    add it to `AGENTS.md` → "Read before working".
  - Accept: the user accepts the spec, recorded in a task note, with an
    ADR for each decision it adds to ADR-0061.
  - Verify: doc review (human)

- [ ] **M15.5 — editor: plugin host**
  - Depends: M15.4, M4.7
  - Do:
    - `editor::plugins` per `docs/subsystems/plugins.md`, with `editor`'s
      edge to `lua` in the dependency matrix: manifests read with
      `codec::toml`; plugins found in the user config folder's `plugins/`
      and, in a trusted workspace only, in `.tidepool/plugins/`; each
      plugin off until the user enables it, asked once with its grants
      listed, and the choice kept in `settings.toml`.
    - The plugin thread, with its row in `docs/01-architecture.md` →
      "Threading model": it owns one Lua state per plugin and runs one
      call at a time. Plugin files are read on workers, and `require`
      loads only from the plugin's own folder.
    - A plugin that fails is reported in a toast and the log, and is
      turned off after the spec's number of failures.
    - A `security-reviewer` review of the sandbox, and the plugin host
      added to its mandatory areas (`CLAUDE.md`,
      `.claude/agents/security-reviewer.md`).
  - Accept:
    - A plugin in an untrusted workspace never runs, and no plugin runs
      before it is enabled.
    - A plugin stuck in `while true do end` is stopped at its fuel
      limit, and meanwhile no keystroke takes longer than one 16 ms frame
      (a `tidepool-bench` suite).
    - One plugin's error leaves the other plugins and Tidepool running.
  - Verify: `cargo test -p tidepool-editor plugins`;
    `cargo xtask bench plugins`

- [ ] **M15.6 — editor: the plugin API**
  - Depends: M15.5, M7.1
  - Do: the `tidepool` modules of `docs/subsystems/plugins.md`:
    - commands, with palette entries, and key bindings in a mode (M4.6);
    - buffers, read from snapshots and edited through transactions, each
      call's edits one undo step (ADR-0020), at byte offsets (ADR-0019);
      an edit made from an older version of the buffer is dropped, and
      the plugin is told;
    - events: a buffer opened, changed, or saved, and the mode changed;
    - toasts, status bar items, and palette pickers;
    - the plugin's own settings, and the log, redacted like every log;
    - two sample plugins: a word count in the status bar, and trimming
      trailing whitespace on save.
  - Accept:
    - A test plugin for each module passes.
    - An edit from Lua undoes as one step.
    - A call on a closed buffer is an error in Lua, never a panic.
    - The samples work in the GUI and the TUI (golden scenes and TUI
      snapshots).
  - Verify: `cargo test -p tidepool-editor plugins`; `cargo xtask golden`

- [ ] **M15.7 — editor: Plugins view**
  - Depends: M15.6
  - Do: a Plugins view in the activity bar, added to
    `docs/subsystems/editor.md`: each plugin with where it came from (the
    user's folder or the workspace's), its grants, its state, and its
    errors; enable, disable, and reload; and its log. No marketplace
    (`docs/00-overview.md` → Non-goals).
  - Accept: golden scenes of each state; reloading picks up an edited
    `init.lua` without a restart; disabling a plugin removes its commands
    and key bindings.
  - Verify: `cargo xtask golden`; `cargo test -p tidepool-editor plugins`

- [ ] **M15.8 — syntax/toolchain: Lua highlighting, lua-language-server, and the API's definitions**
  - Depends: M15.6, M13.2, M5.3
  - Do:
    - A Lua language pack, its lexer written as M13.2's rules.
    - lua-language-server as a managed tool, through `/add-managed-tool`,
      after checking its current docs and release channel.
    - Definition files for the `tidepool` API, in the annotation format
      lua-language-server reads, generated from the host's bindings so
      that they cannot drift from them; the pack points the server at
      them in a plugin's folder.
  - Accept: golden token streams for the pack; fake-tool tests for the
    server; a test generates the definitions again and fails when they
    differ from the committed ones.
  - Verify: `cargo test -p tidepool-syntax packs`;
    `cargo test -p tidepool-toolchain`;
    `cargo test -p tidepool-editor plugins`

**Exit criteria (M15):** the owner works on Tidepool with a plugin they
wrote, one that adds the license header (ADR-0021) to each new source
file in Tidepool's workspace, and the plugin can do nothing that its
manifest did not declare.

---

## M14 — Production hardening and release

- [ ] **M14.1 — fuzz campaign**
  - Depends: M10.2, M9.3, M15.3
  - Decide first: register item 30 (coverage-guided fuzzing) in
    `docs/02-dependency-policy.md`.
  - Do: nightly 30-minute fuzz smoke per target; review every `Limits`.
  - Verify: `cargo xtask fuzz-smoke --secs 1800` per target (nightly logs)

- [ ] **M14.2 — accessibility bridges**
  - Depends: M3.1, M11.1, M12.1
  - Do: AT-SPI, NSAccessibility and UI Automation from the `ui` tree.
  - Accept: a screen reader announces focus moves in the shell on each OS.
  - Verify: manual screen-reader checklist per OS

- [ ] **M14.3 — performance pass**
  - Depends: M9.4, M10.6
  - Do: measure every budget in `docs/00-overview.md` on the reference
    machine; fix violations or record exceptions with an ADR.
  - Verify: `cargo xtask bench`; measurement notes with numbers

- [ ] **M14.7 — security review pass**
  - Depends: M9.6, M10.6, M11.5, M12.3, M15.7
  - Do: `security-reviewer` reviews the whole tree against its checklist
    and the scope in `SECURITY.md`: workspace trust, installs and
    downloads, child processes, secrets, signing, agent context and
    consent, the plugin sandbox and its grants (ADR-0061), every parser's
    `Limits`, `unsafe`, and the CI workflows. Each
    finding is fixed, or recorded with an ADR.
  - Accept: the review and how each finding was resolved are recorded in a
    task note.
  - Verify: the review report (human sign-off)

- [ ] **M14.4 — packaging and release workflow**
  - Depends: M11.6, M12.4, M0.8
  - Do: `xtask package <os>`, the `tidepool --smoke-test` mode, and
    `.github/workflows/release.yml` per `docs/05-release.md`, with its row
    in `cargo xtask workflows`' table (ADR-0022); document secret names
    only.
  - Verify: YAML review; `cargo xtask workflows`; `cargo xtask package linux`

- [ ] **M14.5 — documentation**
  - Depends: M14.4
  - Do: user guide; `cargo doc --workspace --no-deps` with no warnings.
  - Verify: `cargo doc --workspace --no-deps`

- [ ] **M14.6 — release-candidate dry run**
  - Depends: M14.1–M14.5, M14.7
  - Do: the release checklist in `docs/05-release.md`, with evidence.
  - Verify: the completed checklist (human sign-off)

**Exit criteria (M14):** signed packages for all three OSes from a tag, and
the release checklist is complete.

---

## Backlog (each needs a trigger or a decision)

| Item | Trigger / reason |
| :- | :- |
| Native Wayland (xdg-shell, keymaps without `libxkbcommon`) | A target distro ships without XWayland by default (ADR-0010) |
| GPU rendering backend | Frame budget missed at 1440p after profiling (ADR-0011) |
| Complex text shaping, bidi, color emoji | A user need for shaped scripts or emoji in code |
| Full IME (XIM, `NSTextInputClient`, IMM32) | CJK input needed |
| `.vimrc` importer for simple `map` lines | Users ask to bring mappings (ADR-0006) |
| Tidepool as an MCP server for outside agents | A concrete agent integration needs it (ADR-0009) |
| Agent tools (run tests, search) with per-call approval | TAP v1 proposals prove too limiting |
| Plugins that run programs, through `toolchain` with consent | A plugin needs what only a program can give, and the API cannot (ADR-0061) |
| OS sandboxing for agents (seccomp/landlock, macOS sandbox) | Agents from third parties become common |
| Play Console and App Store Connect uploads | The owner ships an app through Tidepool |
| Offline toolchain bundle | Users without network on first run (ADR-0002) |
| Light theme | jabbott.dev adds one, or users ask |
| Remote development, collaborative editing | User demand |
| aarch64 Linux and Windows | Platform demand |
| MIT-SHM present on X11 | Present time shows in profiles |
| Undo history that survives a restart, and history pruning | Undo must persist, or history memory shows in profiles (ADR-0020) |
| `codec`: the YAML subset, CSV and INI (`docs/subsystems/text-and-formats.md`) | A viewer, a validator or a language pack needs to parse them, not only highlight them |
| A command that opens a folder from a typed path, and switching folders while one is open | Users ask to open a folder from inside Tidepool (ADR-0070) |
| Test coverage measured with Rust's own `llvm-tools` component, a floor that only rises, and mutation testing written here, as `.claude/rules/testing.md` asks; none runs today (ADR-0071) | The user decides how `xtask` or CI may run a program other than `cargo` (ADR-0017), or a review finds a change that no test catches |
| A check that fails when a benchmark goes over its budget, as `.claude/rules/testing.md` asks, where `cargo xtask bench` only marks a slower median (ADR-0035, ADR-0071) | A budget regression reaches `main` unnoticed, or a runner of the owner's own can measure on the reference machine |
