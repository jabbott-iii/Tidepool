# 04 — Quality

## Test layers

| Layer | Where | What it proves |
| :- | :- | :- |
| Unit | `#[cfg(test)] mod tests` beside the code | Functions and types behave as documented |
| Property | Unit tests using `tidepool_core::testing` | Invariants hold for generated inputs (rope vs `String` model, position round-trips, undo/redo) |
| Integration | `crates/<name>/tests/` | A crate's public API works end to end |
| Golden frames | `tests/golden/*.qoi`, compared by `cargo xtask golden` | `gfx`, `font` and `ui` render what they rendered when blessed |
| TUI snapshots | `tests/golden/tui/*.txt` | The TUI's cell grid (text + style codes) for a scripted session |
| Protocol transcripts | `crates/testkit/transcripts/` | LSP, DAP and TAP clients send and accept exactly the recorded messages |
| Fake tools | `crates/testkit/fake-tools/` | `toolchain`, `vcs` and `mobile` handle real output formats and exit codes without the real tools |
| Real-tool | Gated by `TIDEPOOL_IT_RA`, `TIDEPOOL_IT_GIT`, `TIDEPOOL_IT_ANDROID`, `TIDEPOOL_IT_IOS` | Integration with the actual installed tool |
| Fuzz smoke | `cargo xtask fuzz-smoke` | No parser panics, hangs or over-allocates on mutated input |
| Responsiveness | `bench` latency suite | Keystroke-to-frame stays within budget with slow fakes attached |
| Vim conformance | `crates/editor/tests/vim/` | Each supported command in `docs/subsystems/vim-mode.md` behaves as in Vim on a fixture |

A gated test that can't run prints `SKIPPED: <reason>` and is never counted
as a pass.

## Own harnesses

### Property testing — `tidepool_core::testing`

- `check(name, cases, |rng| ...)` runs generated cases from a seeded
  `Pcg32`; `check_sized` adds size-directed shrinking.
- A failure prints the seed and the minimal case:
  `TIDEPOOL_SEED=0x… cargo test -p tidepool-text rope_matches_model`.
- Port the harness design from Nadir's `nadir-core::testing` rather than
  inventing a new one.

### Golden frames

- Scenes are drawn offscreen by test binaries into a framebuffer at a fixed
  size and scale, then encoded as QOI.
- `cargo xtask golden` compares them to `tests/golden/` with a per-scene
  tolerance (default: exact for `gfx`, ≤ 1/255 per channel for `font`
  coverage). On mismatch it writes `actual` and `diff` images to
  `target/golden/`.
- `--bless` replaces references only for the scenes named, and only after
  the user has reviewed the diff images.
- Theme changes (`themes/tidepool-dark.toml`) must re-bless with review.

### Fakes

- **Fake LSP server** (`testkit`): replays a transcript, can delay, drop,
  reorder or corrupt messages, and crash on cue.
- **Mock agent** (`agents/tp-agent-mock`): deterministic TAP responses,
  latency injection, malformed output modes.
- **Fake tools**: tiny Rust binaries in `testkit` named like the real tools
  (`git`, `rustup`, `sdkmanager`, `apksigner`, `xcodebuild`...) that print
  recorded output and exit codes, selected by putting their directory first
  on the `PATH` the test hands to `toolchain`.

### Fuzz smoke (stable Rust, no dependencies)

- `cargo xtask fuzz-smoke [--secs N] [--target T]` mutates each corpus in
  `fuzz/corpus/<target>/` (bit flips, splices, length edits, dictionary
  tokens) and runs the parser in-process. Provisional until M1.6: the
  parsers after QOI live in crates outside `xtask`'s matrix row, and an
  allocation cap needs a `GlobalAlloc`, which is `unsafe`
  (`cargo xtask fuzz-smoke` in the table below).
- Targets: JSON, TOML, XML, plist, YAML, CSV, INI, DEFLATE, ZIP, PNG, QOI,
  TrueType, Markdown, regex compile, VT parser, JSON-RPC framing, LSP
  messages, TAP messages, git porcelain v2, cargo JSON diagnostics, tool
  version strings.
- A crash is saved to `fuzz/crashes/<target>/` and becomes a regression
  test.

### Benchmarks — `tidepool-bench`

- Warm-up, N samples, median and p95, JSON baseline in `bench/baseline.json`.
- Suites: rope edits, position conversion, highlighting a 10k-line file,
  project search, frame build + raster at 1080p, glyph atlas fill, startup.
- A change that slows a suite by more than 10% against the baseline must
  say so and why.

### Responsiveness

- A scripted driver feeds keystrokes to `app` with a fake LSP server that
  waits 2 s per request and a mock agent that waits 2 s, and records
  keystroke-to-present time. Budget: p99 ≤ 16 ms (`docs/00-overview.md`).

## `xtask` checks

| Command | Fails when |
| :- | :- |
| `cargo xtask ci` | `deps`, `cargo fmt --check`, clippy `-D warnings`, `cargo test`, `layers`, `unsafe-audit`, `headers` or `ucd` fails (`tools` and `theme` join once implemented, and `workflows` and `crates` with M0.8 and M0.9) |
| `cargo xtask deps` | A workspace resolves or declares a registry or git package that `xtask/deps-allowlist.txt` doesn't approve (or declares it without `=version`), a `Cargo.lock` has an unapproved `source`, a path package lies outside the repository or isn't a member of the workspace that uses it, a workspace other than the root one and `agents/` exists, a cargo config file holds more than aliases, the repository holds a compiled artifact or a file `deps` can't read, or cargo can't resolve a workspace offline |
| `cargo xtask layers` | A crate declares a workspace crate its matrix row doesn't allow (normal, dev or build dependency) or a path crate that isn't a member, members and matrix rows don't match, or `docs/01-architecture.md` breaks the rules below for how the matrix is written |
| `cargo xtask unsafe-audit` | `unsafe` outside `sys`/`platform`, a block or `unsafe impl` without `// SAFETY:`, the `unsafe_code` lint relaxed in source outside their crate roots, a `pub unsafe fn` without `# Safety`, an `extern` block outside `sys`, an exported macro in `sys`/`platform` that wraps its caller's tokens in `unsafe`, or one of the listed ways of bringing in code it did not read |
| `cargo xtask headers` | A source file (Rust, shell script, workflow) does not start with the Apache-2.0 header of `LICENSE`'s appendix in its comment syntax, then a blank line (after a script's `#!` line), unless it is exempt by path; or a source file is a symbolic link, over 8 MiB or not UTF-8 (ADR-0021) |
| `cargo xtask ucd [--check]` | A file in `crates/text/ucd/17.0.0/` is missing, a symbolic link, not a regular file, over 4 MiB, not UTF-8 or not matching its SHA-256 pin; a property file does not parse (a wrong first line, `@missing` default or `# Total code points:` count, an unknown value, a code point assigned twice); a code point is both ZWJ and Extended_Pictographic; or, with `--check` (the `ci` step), `crates/text/src/pos/grapheme/tables.rs` is missing, a link or not what the files generate. Without `--check` it writes that file when it changed |
| `cargo xtask tools` | Not yet implemented (M5.1). Planned: a `toolchains/*.toml` descriptor lacks a pinned version, HTTPS official source, SHA-256 (for downloads) or license |
| `cargo xtask theme` | Not yet implemented (M3.2). Planned: a theme token pair used for text falls below its contrast minimum (`docs/subsystems/ui-and-theme.md`) |
| `cargo xtask golden [--bless]` | Not yet implemented (M2.4). Planned: a golden frame or TUI snapshot differs |
| `cargo xtask fuzz-smoke` | Not yet implemented (M1.6). Planned: a fuzz target crashes, hangs or exceeds its allocation cap. Provisional: the parsers of the targets after QOI live in crates outside `xtask`'s matrix row, and counting allocations needs a `GlobalAlloc`, which is `unsafe`. M1.6 must settle both |
| `cargo xtask bench [filter]` | Not yet implemented (M1.9). Planned: reports only |
| `cargo xtask workflows` | Not a command yet (M0.8). Planned: a workflow in `.github/workflows/` that uses an action its row does not allow, pins one by anything but a full commit SHA with a `# v…` comment, gives its token more than its row allows, uses `pull_request_target`, or checks out without `persist-credentials: false` (ADR-0022) |
| `cargo xtask crates` | Not a command yet (M0.9). Planned: a member that does not inherit `edition`, `version` and `license`, lacks `publish = false` or `[lints] workspace = true`, or whose `README.md` is missing or not in the shape of ADR-0023 |

`xtask` never uploads, publishes or signs with real credentials. `deps`
never opens a file named like a secret (below); `unsafe-audit` reads every
`.rs` file, whatever the rest of its name, and `headers` every source file
it checks, though it never prints a line of one. It reads other files only
as far as a check needs: the first 1 KiB, to compare with compiled-code
signatures (none of it is kept or printed), or the whole of a file a check
parses. The one file it writes in the repository is the generated Unicode
tables, with `cargo xtask ucd` without `--check` (below). It opens no
sockets, and its cargo children run with
`CARGO_NET_OFFLINE=true`. The tests that `ci` runs inherit the environment,
so with a `TIDEPOOL_IT_*` variable set, a real-tool test may still reach the
network through its tool.

### Command line and rules

**Command line**

- `cargo xtask <command> [--root DIR]`. Every command takes no other
  argument, except `ucd`, which also accepts `--check` (once).
  `--root DIR` checks the repository
  at `DIR` instead of the one `xtask` was built from; xtask's own tests use
  it on generated fixtures. A relative `DIR` is made absolute against the
  directory `xtask` started in, before any check runs
  (`std::path::absolute`). Links are not resolved, and `..` is kept except
  on Windows. `--help`, `-h` or `help` lists the commands.
- Exit status: 0 if every check that ran passed; 1 if a check failed or
  could not run; 2 if the command line was not understood (no or unknown
  command, a stray argument, `--root` without a value or with an empty
  one).
- A check prints its notes, one `violation:` line per problem, then
  `<check>: ok` or `<check>: FAILED (N violations)`. A check that cannot run
  prints `<check>: FAILED: <reason>`. A violation or reason that quotes
  cargo's error goes on over the following lines, without a prefix. So for a
  check that ran, the verdict is always the last line; for one that could
  not run, the `FAILED:` line may not be.
- Secrets in messages (`AGENTS.md` rule 7): when `xtask` quotes the error
  of a cargo child it reads (`cargo metadata`, `cargo locate-project`), it
  leaves out the source lines cargo quotes (`  4 | token = "…"`). In those
  errors, and wherever it shows a package source, every URL has its user
  information replaced by `…` (`git+https://…@host/repo`). Not covered:
  cargo's message text can still name a value that is not a URL, such as a
  `rustc` path or an `include` file. And `ci`'s fmt, clippy and test steps
  stream cargo's output as cargo prints it. So keep secrets out of the
  repository's cargo files; cargo reads credentials from `CARGO_HOME`.
- When every violation `deps` found is an out-of-date lockfile (below), its
  last line is `deps: FAILED (N violations; only out-of-date lockfiles)`
  (`1 violation` when there is one). That line is built from the count and
  fixed text only, never from a violation's text, which may hold a path.
  The Claude Code hook (`.claude/hooks/deps-guard.sh`) relies on it. In
  `ci` the `deps` step line reads the same, but the summary does not
  repeat it.
- Named but not yet implemented: `tools` (M5.1), `theme` (M3.2), `golden`
  (M2.4), `fuzz-smoke` (M1.6), `bench` (M1.9; its message still says it
  has no task, which M1.9 corrects) and `package` (M11.6, M12.4, M14.4).
  `workflows` and `crates` (M0.8, M0.9) are not named yet, so they exit
  2 as unknown commands. Each named one exits 1 and
  says it is not yet implemented, whatever other options it gets; only
  `--help`/`-h` (usage text, exit 0) and `--root` without a value (exit 2)
  are handled first. `ci` lists `tools` and `theme` as "not yet
  implemented". None is ever reported as a pass.
- `xtask` runs `cargo` itself, not through `tidepool-toolchain` (the
  exception named in `AGENTS.md` rule 2, ADR-0017): the cargo that
  started it (`CARGO`, if not empty), or else `cargo` on the `PATH`. Its
  children are `cargo metadata`, `cargo locate-project` and `ci`'s fmt,
  clippy and test steps. It runs no other program. Each child is an argv
  vector, never a shell, and runs in the root with standard input closed and
  `CARGO_NET_OFFLINE=true`. `cargo metadata` and `cargo locate-project`,
  whose output `xtask` reads, also get `CARGO_TERM_COLOR=never`, which
  overrides the environment and `term.color`; the fmt, clippy and test steps
  keep cargo's own colour setting. If `CARGO_TARGET_DIR` (or else
  `CARGO_BUILD_TARGET_DIR`) is set and not empty, every child gets
  `CARGO_TARGET_DIR` made absolute against the directory `xtask` started in.
  This is provisional: `package` is specified to run `lipo`, `iconutil` and
  `rc.exe` as well (`docs/05-release.md`), which this exception does not
  cover.
- The Claude Code hook (`.claude/hooks/deps-guard.sh`) runs after an edit
  of a manifest, lockfile or cargo configuration file. It checks the
  checkout holding the edited file: a linked worktree of this repository
  inside the project, else the project. Its `PATH` keeps only absolute
  entries, and rustup may not install a toolchain (`RUSTUP_AUTO_INSTALL=0`):
  a toolchain that `rust-toolchain.toml` names but that is missing is
  reported instead.
  - Before it runs cargo, it reads the cargo configuration and the rustup
    toolchain files (`rust-toolchain`, `rust-toolchain.toml`) in that
    checkout and each directory above it, up to the project.
    Configuration may hold only `[alias]` tables; the rules for alias
    values are left to `deps`, which runs later. A toolchain file may hold
    only `[toolchain]` with a plain `channel` and `profile`, `components`
    and `targets`, or a legacy one-line toolchain name. A `path` there
    would run another cargo.
  - It never shows a value. A report gives the file and line, with the
    first part of a table header (`[patch...]`) or a plain key
    (`token = ...`). A line inside a multi-line string or array, including
    one opened after a header, is shown as `...`. Source lines that cargo
    quotes in its errors are left out, and the user information of every
    URL in what it prints, its own lines and `xtask`'s, is replaced by
    `...`.
  - If the root workspace's lockfile is out of date, it checks only what
    the root manifests declare (`cargo metadata --no-deps`) and whether
    `Cargo.lock` names a package source. Then it says how to update the
    lockfile offline and exits 2 without running `xtask`.
  - It builds `xtask` only if `cargo tree` (normal and build edges) shows
    every package it is built from to be a member of the root workspace.
    Each must be in its own directory, reached without a symbolic link and
    holding none (stricter than `deps`, on purpose). Every target's source
    file must lie inside it, and it may have no build script and no
    proc-macro target. So compiling runs no package's code.
  - It builds into the checkout's `target/deps-guard/`, whatever
    `CARGO_TARGET_DIR` or `build.target-dir` says, and refuses a symbolic
    link inside it, so another checkout's build cannot supply the binary.
    It removes the old binary before each build. It keeps a stamp of the
    sources' contents and starts the build afresh when the stamp changes,
    since cargo judges freshness by file times.
  - `xtask` then runs `deps` and `layers`. If `xtask` cannot be built, the
    hook runs `scripts/check-deps.sh` and exits 2 even when that passes,
    because that check is incomplete. A report over 60 lines is cut to its
    first 40 and last 5.
  - Not covered: a file path with a JSON escape in it is not decoded, so
    the project is checked rather than the worktree. On Windows every
    path has one, which M12 must fix.

**Files searched** (`deps`, `unsafe-audit` and `headers`)

- Everything under the root, including hidden directories, nested `.git`
  directories and submodules. Skipped: `.git` directly under the root, the
  `target` directory beside a `Cargo.toml` that `cargo locate-project
  --workspace` reports as a workspace root, and linked worktrees of the
  repository. A `target` beside any other manifest is searched.
- That `target` must be a real directory: a symbolic link there is reported
  like any other link. It is skipped only while cargo can load its
  workspace. If cargo cannot (an invalid manifest, say), `target` is
  searched like any other directory, and what cargo built there fails
  `deps`.
- Build output anywhere else in the repository is searched like source,
  whether `CARGO_TARGET_DIR`, `CARGO_BUILD_TARGET_DIR` or `build.target-dir`
  put it there, so the checks fail on what cargo built or the tests
  generated there; a note says so when the environment named it. Keep the
  build directory outside the repository or at `<root>/target`.
- A worktree is skipped only if it is registered both ways: its `.git` is a
  regular file (not a link) naming an entry in the repository's
  `.git/worktrees/` (found through `commondir` when the root is itself a
  worktree), and that entry's `gitdir` names it back. A note names each
  skipped worktree; run `xtask` in it to check it. If this checkout builds
  code from one, `cargo metadata` still shows a crate there. A root member
  there fails the audit's crate-root rule, and `layers` too when it has no
  row or an edge the matrix forbids. A non-member path package there fails
  `deps`. A worktree inside a member's package directory fails the audit.
- Symbolic links are never followed. A link to a directory is a violation,
  and so is a link named like a file the check looks for: for `deps`, a
  manifest, lockfile, cargo configuration file or compiled artifact; for the
  audit, a `.rs` file or a manifest; for `headers`, a source file.
- `Cargo.toml`, `Cargo.lock` and `.rs` match in any case, since macOS and
  Windows ignore case. On Linux cargo cannot describe a workspace from a
  `cargo.toml`, so that workspace fails closed.
- Committed test data is checked as the real thing: a file named
  `Cargo.toml` or `Cargo.lock`, `config` or `config.toml` in a `.cargo`
  directory, a file ending in `.rs` (or, for `headers`, `.sh`), or one
  named or starting like a compiled artifact (`deps`), in any case. Give samples another suffix
  (`.rs.txt`), or generate them at test time under `<root>/target` or a
  build directory outside the repository. xtask's fixtures use
  `CARGO_TARGET_TMPDIR`, each in a case directory whose `Cargo.toml` is a
  workspace with no members, so what lies above it never matters. Its unit
  tests use `<root>/target/xtask-unit-tmp/`.
- This covers planned test data too: TOML samples and fuzz seeds named
  `Cargo.toml` or `Cargo.lock`, lexer inputs and fuzz seeds named `.rs`,
  trust-test and cargo-task projects, and the Android and iOS sample apps.
- Not covered, in every check that uses this walk:
  - Races. A file swapped for a link or a FIFO between the walk and its
    open is followed, or blocks the open. A link committed to the
    repository cannot race.
  - Control characters in file names. Paths are printed as they are, so a
    committed file whose name holds a newline or an escape sequence can
    add lines to the output, such as a GitHub Actions workflow command or
    a fake verdict line, or send escape sequences to a terminal. The exit
    status and the real verdict line stay right.
  - To tell whether a link leads to a directory, the walk resolves it
    (`fs::metadata`), which touches its target. On Windows a link to a UNC
    path would make the OS contact that host. A link's contents are never
    read.

**`deps`**

- Finds every `Cargo.toml` under the root and resolves the workspace each
  belongs to once, offline and from its lockfile as it is: `cargo metadata
  --format-version 1 --offline --locked`. It never rewrites a lockfile.
  When cargo cannot list a workspace's members (an invalid root manifest
  or configuration), each manifest is tried and reported on its own.
- Only the root workspace and `agents/` (created by M9.2) may exist. The
  members of `agents/` may be only `tp-agent-mock`, `tp-agent-local` and
  `tp-agent-claude`, each in `agents/<name>/`. A manifest under the root
  that belongs to any other workspace fails. This is provisional: approving
  register item #30 (`cargo fuzz` in a `fuzz/` workspace,
  `docs/02-dependency-policy.md`) would need this rule changed, and M9.2 may
  need more members in `agents/` (shared TAP code, a conformance suite).
- A resolved package with a `source` (registry or git) must be approved: an
  allowlist line names it at its exact version, and the source is
  crates.io.
- A path package (`source` null) must lie in the repository (links and `..`
  resolved) and be a member of the workspace whose graph it is in. Vendored
  or copied code that is not a member fails, listed or not: in an
  `exclude`d directory, in another workspace (even one of its own), or
  outside the repository. Another workspace's crates fail too. As coded,
  then, `agents/` cannot use `crates/`, and the root workspace cannot use
  `agents/`. This is provisional: whether the reference agents may use root
  crates such as `core` is an open question for M9.2.
- In the root workspace, vendored code that cargo makes a member passes
  `deps`. Cargo makes a path dependency inside the workspace directory a
  member unless it is `exclude`d. `layers` fails on such a member when it
  has no matrix row, as vendored third-party code has none, but `ci` runs
  `layers` only after clippy and test (`ci`, below). In `agents/` such a
  member fails `deps` unless it is a reference agent in its own directory.
- Every registry or git dependency a member declares, in any table and for
  any target, must be approved, from crates.io, with the exact requirement
  `=version`. An unapproved one fails even when `[patch]`, `[replace]` or
  `[source]` serves it from a directory.
- Every `Cargo.lock` under the root is also read as text, whether a
  workspace uses it or not. A `source = ` line fails unless it is in a
  `[[package]]` entry from crates.io whose name and version are approved;
  with the allowlist empty, any does. A quoted `"source"` or `'source'` key
  counts as one. A `source` key in a dotted key or inline table, and any
  quoted key with an escape, fail.
- Fails closed: a workspace cargo can't resolve this way (a crates.io
  package not available offline, a manifest in no workspace, a lockfile
  that needs updating) is a violation, because its graph was not checked.
  What its members declare is still checked (`--no-deps`): registry, git,
  outside and non-member path dependencies, and members outside the
  repository, are each reported. If those pass and cargo's error is that
  the lockfile is out of date, the message says so and gives an offline
  update that compiles nothing (`cargo metadata --offline --format-version
  1 --manifest-path <manifest>`); run `deps` again after it. Only that
  message counts as an out-of-date lockfile. If every violation `deps`
  found is one, the last line reads `deps: FAILED (N violations; only
  out-of-date lockfiles)`. Any other violation, anywhere, gives the plain
  form.
- A cargo configuration file (`config` or `config.toml` in a `.cargo`
  directory, anywhere under the root, in any case) may hold only comments
  and an `[alias]` table: bare names, each with a one-line `"..."` string
  or array of them, without escapes or `--config`. Any other line fails,
  since it could change what cargo builds or what the checks see. An
  allowed alias can still change what `cargo xtask` runs (another
  manifest, say), so alias changes need review. A value could be a secret
  such as a registry token, so a violation never shows one. It shows the
  first part of a table header when that is a bare name (`[registry]`,
  `[patch.…]`, otherwise `[…]`), or a key when it is a bare or dotted one
  (`token = …`). A line that continues a multi-line string, array or inline
  table, including one opened after a header, is shown as `…`. One UTF-8
  byte-order mark at the start of the file is skipped. Configuration outside the repository (parent
  directories, `CARGO_HOME`, `--config`) is not checked.
- A compiled artifact fails: a file named `.rlib`, `.rmeta`, `.a`, `.o`,
  `.obj`, `.so`, `.dylib`, `.dll`, `.lib`, `.exe` or `.wasm` (in any case),
  or one whose first bytes are ELF, Mach-O (thin, or a 32- or 64-bit
  universal header; a Java class shares the 32-bit one), PE (when its PE
  header lies in the first 1 KiB), an `ar` archive, rustc metadata,
  WebAssembly, or LLVM bitcode, raw or wrapped. A bare COFF object is known
  by name only. A file named like an artifact fails by name
  and is not opened. Files named like secrets (`.env`, `.env.*`,
  `keystore.properties`, `*.jks`, `*.keystore`, `*.p12`, `*.p8`, `*.pem`,
  `*.key`, `*.mobileprovision`, `*.provisionprofile`, in any case) are
  never opened. Of every other file it searches, `deps` reads the first
  1 KiB.
- A file it cannot open or read for that is a violation (it was not
  checked), and the check goes on. A directory it cannot list, or a
  lockfile, cargo configuration file or allowlist it cannot read (missing,
  over 16 MiB or not UTF-8), stops the check: `deps: FAILED: <reason>`.
- `xtask/deps-allowlist.txt` has one `name version ADR-NNNN` per line:
  three fields separated by whitespace, the last `ADR-` and four digits.
  Blank lines and `#` lines are ignored; any other line is a violation. One
  UTF-8 byte-order mark at the start of the file is skipped. A line
  approves that name at that version from crates.io only. The list is empty
  (ADR-0001). A missing allowlist fails the check, and so does a tree with
  no `Cargo.toml` anywhere under the root.

**`layers`**

- Reads the matrix from `docs/01-architecture.md` → "Dependency matrix" on
  every run. A cell is `—` (or `-`), `all crates above`, `any workspace
  crate`, or backticked names (`core` for `tidepool-core`, or an alias such
  as `app`), optionally followed by "(used only as a dev-dependency)".
  Anything else fails, naming the line.
- The document is read close to how GFM renders it, by these rules. Where
  they differ from a renderer, they mostly fail:
  - HTML comments and fenced code blocks (fences indented at most three
    columns) are not read as text. A line a comment touches is not read as
    text either, so a comment on a table row ends the table there, and the
    rows after it fail as a second table.
  - Only fenced code blocks and code spans count as code. An indented code
    block is read as text. A code span counts only if it closes on its
    line.
  - Outside code, the document may hold no raw HTML but comments, as a
    block or inline. A line whose first character after any indentation is
    `<` (other than a comment) counts as an HTML block and fails. So does
    HTML-like text in an indented code block, such as `<root>` or
    `Vec<u8>`.
  - It has one header row of the matrix's shape, with or without outer
    pipes, counting comments and code.
  - The section runs to the next line read as text that starts with `# `
    or `## ` (a setext or indented heading does not end it). Its table is
    the run of lines starting with `|` (not indented) from the first such
    line. The section has exactly one table, with a GFM delimiter row
    (cells such as `:-`, `---` or `-:`), and no other line of it that is
    read as text holds a `|`.
  - Two gaps are known, and each needs crafted text. A line without `|`
    directly under the table, which GFM renders as one more row, is not
    read and does not fail. A backslash before a backtick is not read as
    an escape, so `` \` `` still opens a code span here, and raw HTML after
    it is missed.
- Checks the root workspace only (`cargo metadata --offline --no-deps`); the
  matrix has no rows for `agents/`. Every member needs a row, and every row
  a member.
- Every declared dependency counts: normal, dev and build, for every target
  platform.
- A path dependency is matched to a member by its resolved directory, not
  its name. A path crate that is not a member fails, named by its resolved
  directory relative to the resolved workspace root
  (`vendor/tidepool-core`), or in full if it lies outside that root. A
  registry or git dependency named like a member counts as an edge to it,
  since a `[patch]` can serve the member under that name. Other registry
  and git dependencies are left to `deps`.
- A crate whose row says "(used only as a dev-dependency)" (today only
  `tidepool-testkit`) may be used only from `[dev-dependencies]`. The note
  grants no edge: a crate may use testkit only if its own row allows it.
  Today only `tidepool-bench` ("any workspace crate") does. This is
  provisional: the test plan has the tests of `toolchain`, `vcs`, `mobile`,
  `rpc` and `lang`, among others, use testkit's fakes and transcripts,
  which fails `layers` as the matrix stands. The matrix needs a decision
  before M5.1.

**`unsafe-audit`**

- Reads every `.rs` file under the root with its own small lexer. The lexer
  tells code from comments and from string, raw string, byte, C string and
  character literals; it is not a parser. It never fails or panics: text
  that isn't valid Rust still gets an answer, and the compiler reports it.
  A raw identifier (`r#unsafe`) is a name, never a keyword. Because it reads
  text, it covers every `cfg` branch and code in macros.
- Violations:
  - `unsafe` in any form (block, `fn`, `impl`, `trait`, `extern`,
    `#[unsafe(...)]`) outside `crates/sys/` and `crates/platform/`.
  - In those two, an `unsafe` block or `unsafe impl` with no `//` comment
    starting `SAFETY:` between the end of the previous statement (`;`, `{`
    or `}`) and the keyword. Doc comments and `/* */` don't count.
  - `allow`, `expect` or `warn` of `unsafe_code` (also inside `cfg_attr`)
    anywhere but `crates/sys/src/lib.rs` and `crates/platform/src/lib.rs`.
    Only attributes in source are seen; no check reads a member's `[lints]`
    table or `RUSTFLAGS`. So `unsafe` compiles only in the library targets
    of `sys` and `platform` (their `#[cfg(test)]` modules included). Their
    integration tests, benches, examples and binaries are crates of their
    own, under the workspace's `deny`. This is provisional: M2.2's Xvfb test
    reads pixels back with `XGetImage`, and where that FFI call may live is
    an open question.
  - A function written `pub` or `pub(...)` `unsafe fn` (also `const`,
    `async`, with an `extern` ABI, or in an `extern` block) with no
    `# Safety` line in its outer doc comments and no `# Safety` in its
    `#[doc = "..."]` attributes. An unmarked `pub fn` in an `unsafe extern`
    block is not checked.
  - An `extern` block outside `tidepool-sys`, or one in it not written
    `unsafe extern`.
  - In those two crates, a `#[macro_export]` `macro_rules!` (also through
    `cfg_attr`) with an `unsafe` block or `unsafe impl` that holds a
    metavariable other than `$crate`: in a calling crate, the caller's
    tokens would run as `unsafe` that neither its lint nor the audit sees.
- Fails closed on these ways of bringing in code it did not read:
  - `#[path = "..."]` or `include!(...)` (also in `cfg_attr`, as a raw
    identifier, or through `std::`), unless the argument is one plain or raw
    string literal without escapes, naming a relative `.rs` path inside the
    file's directory. No `:` anywhere. Each `/`-separated component is
    non-empty, doesn't start with `.`, doesn't end in `.` or a space, isn't
    `target` in any case, and has no `\` or `~`.
  - A crate root of a member of any workspace under the root (library,
    binary, test, example, bench or build script, as `cargo metadata
    --offline --no-deps` reports it) that is not a file the audit read, such
    as `[lib] path = "src/lib.txt"`. A workspace cargo can't describe
    offline is a violation.
  - A crate root of such a member, of any kind (build scripts included),
    that does not lie inside its package directory, both with links and
    `..` resolved, such as `[lib] path = "../../lib.rs"`. From outside, a
    plain `mod target;` could compile a directory the audit skips. The
    violation names the target, the crate root and the package directory.
  - A skipped linked worktree inside the package directory of a member of
    any workspace, since a plain `mod` could compile files from it.
  - A symbolic link (above), a file over 8 MiB, or a file that isn't UTF-8.
    A `.rs` file it cannot read stops the audit:
    `unsafe-audit: FAILED: <reason>`.
  - `crates/sys/src/lib.rs` or `crates/platform/src/lib.rs` not found,
    which means the root is wrong.
- Not covered: a workspace root's `target/` inside a member's package
  directory, as when one manifest holds both `[workspace]` and `[package]`.
  A crate root placed directly in that directory (`[lib] path = "lib.rs"`,
  or a `build.rs` beside the manifest) can reach `target/` through a plain
  `mod target;`, and the audit does not see those files. Tidepool's root
  manifest has no `[package]` today.
- Also not covered: unsafe attributes (`#[no_mangle]`, `#[export_name]`,
  `#[link_section]`) written without `unsafe`, as editions before 2024
  allow. Only the `unsafe_code` lint catches those. Members are expected to
  use edition 2024 and `[lints] workspace = true`, which no check verifies
  until `cargo xtask crates` (M0.9, ADR-0023).
- Notes list the `unsafe` uses per crate (review any growth), the files
  scanned and the crate roots checked.
- It does not check where `#[cfg(target_os)]` appears (`AGENTS.md` rule 5).
  No check does.

**`headers`**

- Source files, found as "Files searched" says: a Rust file (`.rs`), a
  shell script (`.sh`) and a workflow (`.yml` or `.yaml` directly in
  `.github/workflows/`, where GitHub reads them), the extension in any
  case. Configuration and data carry no header (ADR-0021): TOML, Markdown,
  JSON and JSONC, other YAML (`dependabot.yml`, the issue forms, YAML in a
  folder below `.github/workflows/`), test vectors, the Unicode data,
  `LICENSE` and `NOTICE`.
- Each starts with the 13 lines of `LICENSE`'s appendix from its copyright
  line, word for word: `// ` before each in Rust, `# ` in scripts and
  workflows, and `//` or `#` alone on an empty one. A blank line follows,
  unless the file ends there. The header starts on line 1, or in a script
  on line 2 after a `#!` line. One leading UTF-8 byte-order mark is
  skipped, as `deps` skips one. CR LF line endings, as a Windows checkout
  may have, are accepted; lone CR ones are not. Only those first lines of
  a file are read as lines.
- A violation names the first line that is wrong and what it should be,
  never what it holds: `<file>:<line>: expected `<text>`, line N of the
  Apache-2.0 header in `LICENSE`'s appendix`, or `<file>:<line>: expected
  a blank line after the Apache-2.0 header`. A file over 8 MiB and a file
  that isn't UTF-8 are violations too. So is every symbolic link the walk
  reports (above): one named `.rs`, `.sh`, `.yml` or `.yaml`, wherever it
  is, or one to a directory. The check does not resolve a link again to
  tell a YAML file outside the workflows from a workflow. A file it cannot
  read stops the check: `headers: FAILED: <reason>`.
- Exempt by path (`EXEMPT` in `xtask/src/headers.rs`, ADR-0021):
  - `crates/text/src/pos/grapheme/tables.rs`, which `ucd` generates and
    which keeps the Unicode data's notice instead (ADR-0018);
  - `.github/workflows/codeql.yml`, GitHub's starter workflow, which
    carries no notice. It is exempt, at the user's choice, until M0.8
    rewrites it as the project's own.
- Notes give the number of files whose header was checked (not those too
  large or not UTF-8) and the exempt files found, each with its reason.
- A unit test compares its header with `LICENSE`'s appendix, so a change
  to either fails until the other matches.
- Not covered: a new kind of source file (another extension) is unchecked
  until `kind` in `xtask/src/headers.rs` knows it. The third-party notices
  kept beside other material, such as the IETF Trust's for the RFC 8259
  examples in `crates/codec/tests/json.rs`, are not checked.

**`ucd`**

- Inputs: the five files in `crates/text/ucd/17.0.0/` (ADR-0018). They are
  `GraphemeBreakProperty.txt`, `emoji-data.txt`,
  `DerivedCoreProperties.txt`, `GraphemeBreakTest.txt` (read only by
  `tidepool-text`'s tests) and `license.txt`, each pinned by its SHA-256 in
  `xtask/src/ucd.rs`. Before anything is parsed, each must be a regular
  file (a symbolic link is never followed), at most 4 MiB, UTF-8, and match
  its pin. Otherwise it is a violation, and nothing is parsed or written.
  `.gitattributes` keeps git from converting their line endings, which
  would break the pins.
- It packs three properties into one byte per code point, using the
  constants of `class` in `crates/text/src/pos/grapheme.rs`:
  - `Grapheme_Cluster_Break` in bits 0–3;
  - `Indic_Conjunct_Break`, from the `InCB` lines, in bits 4–5;
  - `Extended_Pictographic` in bit 6.
- Each file's first line and its `@missing` default (for `emoji-data.txt`,
  its `# Version:` line) must be the expected ones, and every data line
  must parse. No code point may get one property twice. Each
  `# Total code points: N` line must count the code points kept since the
  previous one, unless none were. Two more things are errors, because the
  grapheme engine relies on them: a code point that is both ZWJ and
  Extended_Pictographic, and a U+000A or U+000D that is not exactly LF or
  CR.
- Output: `crates/text/src/pos/grapheme/tables.rs`. It holds a header
  naming the three sources, their SHA-256s and the Unicode license, then
  `UNICODE_VERSION`, then the sorted ranges of code points whose
  properties are not all defaults, one per line. It has LF line endings
  and the same bytes on every run and OS.
- Without `--check`, it writes the file only when it differs: to a new
  `tables.rs.tmp` beside it (a leftover one is removed first), which is
  then renamed over it. A linked or non-regular output is a violation and is
  not replaced. With `--check` (the `ci` step), a missing, linked or
  different output is a violation that names the first different line.
- Directories: each directory below the repository root on the way to the
  inputs or the output must be a real directory. That is `crates`,
  `crates/text`, `crates/text/ucd`, `crates/text/ucd/17.0.0`,
  `crates/text/src`, `crates/text/src/pos` and
  `crates/text/src/pos/grapheme`. They are checked shallowest first,
  before anything is read.
  - A link is the violation `<dir> is a symbolic link, which ucd does not
    follow`; anything else that is not a directory is `<dir> is not a
    directory`. Either is reported once and stops the run.
  - Without `--check`, the output's directories are checked again just
    before writing. A missing one is made one level at a time, and each is
    checked again once made.
- It opens no sockets and starts no process. Its parser is not a fuzz
  target, because its inputs are pinned (ADR-0018).
- Not covered:
  - Races. std has no `openat`, so a directory swapped for a link between
    the check and the open is followed. A link committed to the repository
    cannot race.
  - Hard links are not detected.

**`ci`**

- Runs, in this order: `deps`, `cargo fmt --all -- --check`, `cargo clippy
  --workspace --all-targets -- -D warnings`, `cargo test --workspace`,
  `layers`, `unsafe-audit`, `headers`, `ucd` (as `ucd --check`). Each step
  starts with a `== <step>: ...` heading; the cargo steps print cargo's own
  output.
- It keeps going after a failure, so one run reports every problem, with
  one exception. Once `deps` has failed or could not run, clippy and test
  are not run, because they would build and run the code `deps` rejected
  (build scripts, proc macros, tests). `fmt` only reads source, so it still
  runs. `layers`, `unsafe-audit`, `headers` and `ucd` are not gates, and
  run after clippy and test. So code that `deps` accepts but they reject,
  such as a vendored crate that cargo made a member, is still compiled, and
  its build scripts and tests run, before they report it.
- The cargo steps are cargo children like the others: offline, with
  standard input closed. The test step's output passes through `xtask` as
  it arrives, so by default cargo and libtest print it without colour, and
  the two streams may interleave differently.
- A line of the test step's output, on either stream, that starts with
  `SKIPPED:` (after any indentation) is a skipped test. After the step each
  reason is printed as `skipped: <reason>` (up to 1000; the rest are
  counted), and the step line reads `test: ok (2 SKIPPED)` or `test:
  FAILED (1 violation; 2 SKIPPED)`. A skip does not fail `ci`; it is listed
  and counted, not passed. A test reports one by writing it straight to
  `std::io::stderr()`: libtest captures `println!`/`eprintln!` from a
  passing test, so `ci` never sees those. A write straight to
  `std::io::stdout()` is seen too, but libtest writes its own lines there,
  so such a write should start with a newline. Prefer standard error.
- It ends with a summary: each step's result and time (the test row with
  its `(N SKIPPED)`), "not run (deps failed)" where that applies, and
  `tools` and `theme` as "not yet implemented". The last line is `ci: ok:
  N of N steps passed` or `ci: FAILED: M of N steps failed: <steps>`, then
  `; not run: <steps>` if any, and `; not yet implemented: tools, theme`
  while those are pending. After skips, `(1 test SKIPPED)`, `(2 tests
  SKIPPED)` and so on follow `ok` or `FAILED`, as in `ci: ok (2 tests
  SKIPPED): 8 of 8 steps passed; not yet implemented: tools, theme`.
  Scripts should rely on the exit status, which is 0 only if every step
  passed.

## CI (GitHub Actions)

- `.github/workflows/ci.yml`: matrix Linux, macOS, Windows; `permissions:
  contents: read`; actions pinned by full commit SHA with the tag in a
  comment; cache keyed on the lockfile and toolchain; runs `cargo xtask ci`
  plus, from M1.6, a Linux fuzz-smoke step (30 s per target).
- **Supply chain of CI itself** (ADR-0022): each workflow uses only the
  actions its row allows (`ci.yml`: `actions/checkout`, `actions/cache`;
  `codeql.yml`: `actions/checkout`, `github/codeql-action/init` and
  `analyze`), each pinned by full commit SHA with its tag in a comment;
  top-level permissions are `{}` or `contents: read`, and only CodeQL's
  analysis job may write, to `security-events`; no `pull_request_target`;
  every checkout sets `persist-credentials: false`. `cargo xtask workflows`
  checks this from M0.8.
- **Code scanning** (`.github/workflows/codeql.yml`): the file committed on
  2026-11-01 is GitHub's template. Today it scans `rust` and `actions` with
  no build, on pushes to `main`, pull requests to `main` and weekly, with the
  default queries. It falls short of these rules: its three actions are
  pinned by tag, it has no top-level `permissions`, its job also gets
  `packages: read` and `actions: read`, and its checkout keeps its
  credentials. ADR-0022 sets the target that M0.8 brings it to: actions
  pinned by SHA, `permissions: {}` with the job's `contents: read` and
  `security-events: write`, `persist-credentials: false`, the
  `security-extended` queries, a run by hand, and `ci.yml`'s concurrency
  rule.
- **Dependabot** (`.github/dependabot.yml`) is to update the actions' pins
  weekly (ADR-0022). The file committed on 2026-11-01 is GitHub's starter
  file with an empty `package-ecosystem`, which updates nothing; M0.8
  replaces it.
- Goldens run on Linux CI only until M11/M12 add per-OS references for
  platform-specific scenes (the CPU rasterizer is deterministic, so shared
  scenes share references).
- Nightly (later): real-tool jobs. Android on Linux (SDK installed by
  Tidepool's own `toolchain` code, licenses accepted by a human-reviewed
  workflow input), iOS on macOS runners with Xcode.

## Unsafe and FFI review checklist

Use for every change to `sys` or `platform` (the `security-reviewer` agent
runs it).

1. Declarations live only in `sys`, each citing a header or doc URL.
2. Every `#[repr(C)]` type has `size_of`/`align_of` asserts per target.
3. One unsafe operation per `unsafe` block, each with `// SAFETY:`.
4. Every `pub unsafe fn` has a `# Safety` section.
5. Ownership of every returned pointer is documented and wrapped in RAII.
6. Every return code is checked and converted to a typed error.
7. Objective-C calls go through `msg_send!` with the exact signature.
8. Signal handlers do only async-signal-safe work (write to a self-pipe).
9. Child processes are spawned with an argv vector, never through a shell.
10. If a check can only run on another OS, the summary says which.
