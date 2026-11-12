// Copyright 2026 Joseph Anthony Abbott III
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! End-to-end tests of the `xtask` binary on fixture repositories that are
//! generated in a temporary directory at test time (never committed, so the
//! real repository never contains a broken workspace).
//!
//! Each check must fail on a deliberately broken fixture and pass on the
//! fixture before it was broken and on the real repository. Where a test
//! says a fixture fails for one reason only, it asserts the whole list of
//! violations (or that the other checks pass); where one change breaks a
//! fixture in two ways, it asserts both.
//!
//! Fixtures live under cargo's `CARGO_TARGET_TMPDIR`, in a directory unique
//! to this process and test, removed when the test ends. That case
//! directory is a workspace root with no members ([`Fixture::empty`]), so
//! what lies above it never changes a result: the build directory may be
//! inside the real repository or outside any workspace. Every cargo
//! command runs offline. The `xtask` under test does not inherit
//! `CARGO_NET_OFFLINE` from these tests, so it must set it for its own
//! cargo children.
//!
//! A path in a fixture's TOML is relative and written with `/`, never an
//! absolute OS path spliced in: a Windows path (`D:\a\...`, or the
//! `\\?\D:\...` form `fs::canonicalize` gives there) is full of invalid
//! TOML escapes, and a resolved path differs from the one cargo sees when a
//! link leads to the build directory. [`Fixture::write`] enforces this.

use std::ffi::OsString;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The `xtask` binary under test.
const XTASK: &str = env!("CARGO_BIN_EXE_xtask");

/// The real allowlist, which every fixture copies so that its format is
/// the one exercised.
const ALLOWLIST: &str = include_str!("../deps-allowlist.txt");

/// A reduced dependency matrix for the fixture's four crates, in the
/// format of `docs/01-architecture.md`.
const MATRIX: &str = "# 01 — Architecture (fixture)\n\
    \n\
    ## Dependency matrix\n\
    \n\
    | Crate | Allowed workspace dependencies |\n\
    | :- | :- |\n\
    | `tidepool-core` | — |\n\
    | `tidepool-sys` | — |\n\
    | `tidepool-platform` | `core`, `sys` |\n\
    | `tidepool-text` | `core` |\n\
    \n\
    ## Workspace layout\n";

/// A small roadmap for the fixture: the task IDs its READMEs may name
/// ([`readme_of`]), in the format of `docs/roadmap.md`.
const ROADMAP: &str = "# Roadmap (fixture)\n\
    \n\
    - [x] **M0.1 — fixture: the first task**\n\
    - [ ] **M0.2 — fixture: the second task**\n";

/// The fixture's features: one for each task of [`ROADMAP`], in the format
/// of `docs/features.jsonc`, so that `features` passes.
const FEATURES: &str = "// The fixture's features.\n\
    {\n\
      \"features\": [\n\
        { \"name\": \"The first\", \"area\": \"Quality\", \"summary\": \"It is built.\", \
          \"tasks\": [\"M0.1\"], \"ticked\": [\"M0.1\"] },\n\
        { \"name\": \"The second\", \"area\": \"Quality\", \"summary\": \"It will be built.\", \
          \"tasks\": [\"M0.2\"] }\n\
      ]\n\
    }\n";

/// The README of the fixture's package `name`, in the shape `crates`
/// checks (ADR-0023, ADR-0028), naming the tasks of [`ROADMAP`].
fn readme_of(name: &str) -> String {
    format!(
        "# {name}\n\
         \n\
         A crate of the fixture.\n\
         \n\
         ## Status\n\
         \n\
         Version 0.1, not published. Built: nothing — M0.1. Not yet: the rest — M0.2.\n\
         \n\
         ## Usage\n\
         \n\
         Nothing to use yet: its first API lands in M0.2.\n\
         \n\
         ## Documentation\n\
         \n\
         - API reference: `cargo doc -p {name} --open`\n\
         \n\
         ## Rules\n\
         \n\
         None.\n\
         \n\
         ## Checks\n\
         \n\
         `cargo test -p {name}`\n\
         \n\
         ## License\n\
         \n\
         Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).\n"
    )
}

/// The crate root of the fixture's `tidepool-text`, after its header. Like
/// every library's but sys's and platform's, it forbids `unsafe` in its doc
/// tests, as `unsafe-audit` requires (ADR-0036).
const TEXT_LIB: &str = "//! Text.\n#![doc(test(attr(forbid(unsafe_code))))]\n\n\
    /// Says hello.\npub fn hello() -> &'static str {\n    \"hello\"\n}\n";

/// The real `LICENSE` and `NOTICE`, which the fixture keeps at its root and
/// a copy of in each member (ADR-0028), as the repository does.
const LICENSE: &str = include_str!("../../LICENSE");
const NOTICE: &str = include_str!("../../NOTICE");

/// The files `ci`'s `ucd` step checks: the pinned Unicode data and the
/// tables generated from it, copied from the real repository by
/// [`Fixture::with_ucd`].
const UCD_FILES: [&str; 6] = [
    "crates/text/ucd/17.0.0/GraphemeBreakProperty.txt",
    "crates/text/ucd/17.0.0/emoji-data.txt",
    "crates/text/ucd/17.0.0/DerivedCoreProperties.txt",
    "crates/text/ucd/17.0.0/GraphemeBreakTest.txt",
    "crates/text/ucd/17.0.0/license.txt",
    "crates/text/src/pos/grapheme/tables.rs",
];

/// The manifest of every case directory, outside the repository: a
/// workspace root with no members, which encloses the repository and
/// anything "outside" it ([`Fixture::empty`]).
const ENCLOSING: &str = "# Encloses the fixture, so that what lies above it never matters.\n\
    [workspace]\n\
    resolver = \"3\"\n";

/// The Apache-2.0 header a Rust source file starts with (ADR-0021), and the
/// blank line after it: `LICENSE`'s appendix as `//` comments, written out
/// here rather than taken from `xtask`, so that a change to either shows.
const RUST_HEADER: &str = "// Copyright 2026 Joseph Anthony Abbott III\n\
    //\n\
    // Licensed under the Apache License, Version 2.0 (the \"License\");\n\
    // you may not use this file except in compliance with the License.\n\
    // You may obtain a copy of the License at\n\
    //\n\
    //     http://www.apache.org/licenses/LICENSE-2.0\n\
    //\n\
    // Unless required by applicable law or agreed to in writing, software\n\
    // distributed under the License is distributed on an \"AS IS\" BASIS,\n\
    // WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.\n\
    // See the License for the specific language governing permissions and\n\
    // limitations under the License.\n\
    \n";

/// [`RUST_HEADER`] as one comment from `open` to `close`, each on a line of
/// its own, with the header's lines indented by two spaces (an empty one
/// left empty), then a blank line: as a stylesheet (`/* */`) or an HTML
/// file (`<!-- -->`) writes it.
fn enclosed_header(open: &str, close: &str) -> String {
    let mut text = format!("{open}\n");
    for line in RUST_HEADER.lines().take_while(|line| !line.is_empty()) {
        let line = line.strip_prefix("//").unwrap_or(line);
        match line.strip_prefix(' ') {
            Some(rest) => text.push_str(&format!("  {rest}\n")),
            None => text.push('\n'),
        }
    }
    text.push_str(&format!("{close}\n\n"));
    text
}

/// [`RUST_HEADER`] as a shell script or a workflow writes it, with `#` for
/// the `//` that starts each line (not the one in the URL).
fn hash_header() -> String {
    RUST_HEADER
        .lines()
        .map(|line| format!("{}\n", line.replacen("//", "#", 1)))
        .collect()
}

/// The body of the fixture's `ci.yml`, after its header ([`hash_header`]):
/// a workflow that keeps every rule of `workflows` (ADR-0022), with
/// `ci.yml`'s actions pinned as in the real one.
const CI_WORKFLOW: &str = r#"name: CI

on:
  push:
    branches: [main]
  pull_request:

permissions:
  contents: read

jobs:
  ci:
    runs-on: ubuntu-latest
    steps:
      - name: Check out
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          persist-credentials: false
      - name: Cache
        uses: actions/cache@55cc8345863c7cc4c66a329aec7e433d2d1c52a9 # v6.1.0
        with:
          path: target
          key: cargo
      - run: cargo xtask ci
"#;

/// The body of the fixture's `codeql.yml`, after its header: as
/// [`CI_WORKFLOW`], for CodeQL's row, whose analysis job may write
/// `security-events`.
const CODEQL_WORKFLOW: &str = r#"name: CodeQL

on:
  push:
    branches: [main]

permissions: {}

jobs:
  analyze:
    runs-on: ubuntu-latest
    permissions:
      contents: read
      security-events: write
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          persist-credentials: false
      - uses: github/codeql-action/init@9b3254085494b55a0f7c4c2b3d2d52cc712ce370 # v4.39.1
        with:
          languages: rust
      - uses: github/codeql-action/analyze@9b3254085494b55a0f7c4c2b3d2d52cc712ce370 # v4.39.1
"#;

/// GitHub's "CodeQL Advanced" starter workflow, as the user committed it on
/// 2026-11-01 (`git show ed8edf4:.github/workflows/codeql.yml`), byte for
/// byte. Its file name is one no check reads as a workflow. It is GitHub's,
/// under the MIT license in `data/NOTICE.md`.
const CODEQL_TEMPLATE: &str = include_str!("data/codeql-template.yml.txt");

/// What one run of the binary did.
struct Run {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl Run {
    /// Asserts the exit code, showing the output if it is not the one
    /// expected.
    fn expect_code(&self, code: i32) -> &Self {
        assert_eq!(
            self.code,
            Some(code),
            "stdout:\n{}\nstderr:\n{}",
            self.stdout,
            self.stderr
        );
        self
    }

    fn violations(&self) -> Vec<&str> {
        self.stdout
            .lines()
            .filter_map(|line| line.strip_prefix("violation: "))
            .collect()
    }

    /// The first two words of each line of `ci`'s summary table: the step
    /// and how it ended (`ok`, `FAILED`, `not`).
    fn summary(&self) -> Vec<(&str, &str)> {
        self.stdout
            .split("== summary\n")
            .nth(1)
            .unwrap_or("")
            .lines()
            .filter_map(|line| {
                let mut words = line.split_whitespace();
                Some((words.next()?, words.next()?))
            })
            .collect()
    }

    /// The last line of `ci`'s summary.
    fn verdict(&self) -> &str {
        self.stdout.lines().last().unwrap_or("")
    }
}

/// The cargo that runs these tests.
fn cargo() -> OsString {
    std::env::var_os("CARGO")
        .filter(|cargo| !cargo.is_empty())
        .unwrap_or_else(|| OsString::from("cargo"))
}

/// The build directory these tests run in, absolute: the parent of
/// `CARGO_TARGET_TMPDIR`, which cargo sets to its `tmp` directory.
fn build_dir() -> PathBuf {
    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR"));
    tmp.parent().unwrap_or(tmp).to_path_buf()
}

/// Runs the binary with `args`, with `target`, if given, as cargo's build
/// directory for anything it builds. Otherwise the build directory is the
/// one these tests run in ([`build_dir`]), which the checks know to skip.
/// It is passed absolute: a relative `CARGO_TARGET_DIR` inherited from the
/// cargo running these tests means the directory cargo started in, not
/// this test's current directory, which is the package's.
fn xtask(args: &[&str], target: Option<&Path>) -> Run {
    xtask_with(args, target, &[])
}

/// [`xtask`], with the variables in `env` set (`None` removes one).
fn xtask_with(args: &[&str], target: Option<&Path>, env: &[(&str, Option<&str>)]) -> Run {
    let mut command = Command::new(XTASK);
    command.args(args).env_remove("CARGO_NET_OFFLINE").env(
        "CARGO_TARGET_DIR",
        target.map_or_else(build_dir, Path::to_path_buf),
    );
    for (name, value) in env {
        match value {
            Some(value) => command.env(name, value),
            None => command.env_remove(name),
        };
    }
    let output = command.output().expect("the xtask binary runs");
    Run {
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// Runs the binary with `args` in `dir`, with `CARGO_TARGET_DIR` and
/// `CARGO_BUILD_TARGET_DIR` set as `env` says (`None` removes one).
fn xtask_in(dir: &Path, args: &[&str], env: &[(&str, Option<&str>)]) -> Run {
    let mut command = Command::new(XTASK);
    command
        .args(args)
        .current_dir(dir)
        .env_remove("CARGO_NET_OFFLINE")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR");
    for (name, value) in env {
        if let Some(value) = value {
            command.env(name, value);
        }
    }
    let output = command.output().expect("the xtask binary runs");
    Run {
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// A generated repository, removed when dropped.
struct Fixture {
    /// The directory that holds the repository and anything "outside" it.
    case: PathBuf,
    /// The repository root.
    root: PathBuf,
}

impl Fixture {
    /// An empty case directory for the test `name`.
    ///
    /// The case directory holds [`ENCLOSING`], a workspace with no members.
    /// Cargo looks for the workspace of a manifest that no workspace in the
    /// repository claims (a crate in an `exclude`d directory, a package
    /// outside the repository) in the directories above it, and stops at
    /// the first workspace root that does not exclude it. This one makes
    /// the answer the same wherever the case directory is: the manifest
    /// belongs to no workspace. Without it, cargo would walk on, to the real
    /// repository's `Cargo.toml` when the build directory is `<root>/target`
    /// (the same answer), or, outside any workspace, to nothing, making that
    /// manifest the root of a workspace of its own (another answer).
    fn empty(name: &str) -> Self {
        let case = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("xtask-fixture-{}-{name}", std::process::id()));
        // It may be left over from a crashed run with the same id.
        let _ = fs::remove_dir_all(&case);
        let root = case.join("repo");
        fs::create_dir_all(&root).expect("create the fixture directory");
        write(&case.join("Cargo.toml"), ENCLOSING);
        Self { case, root }
    }

    /// A small repository shaped like Tidepool's that passes every check
    /// but `ucd` (for which see [`Fixture::with_ucd`]): four crates that
    /// follow [`MATRIX`], each with its README ([`readme_of`]), copies of
    /// [`LICENSE`] and [`NOTICE`] and a manifest that inherits from the
    /// workspace, the two unsafe crate roots, two crate roots that forbid
    /// `unsafe` in doc tests (ADR-0036), the real allowlist, `LICENSE`
    /// and `NOTICE` at the root, a roadmap ([`ROADMAP`]) and its features
    /// ([`FEATURES`]), a lockfile, and the two workflows ([`CI_WORKFLOW`],
    /// [`CODEQL_WORKFLOW`]).
    fn passing(name: &str) -> Self {
        let fixture = Self::empty(name);
        fixture.write(
            "Cargo.toml",
            "[workspace]\nresolver = \"3\"\nmembers = [\"crates/core\", \"crates/sys\", \
             \"crates/platform\", \"crates/text\"]\n\n[workspace.package]\nedition = \"2024\"\n\
             version = \"0.1.0\"\nlicense = \"Apache-2.0\"\n\n[workspace.lints]\n",
        );
        fixture.package(
            "core",
            "",
            "//! Core.\n#![doc(test(attr(forbid(unsafe_code))))]\n",
        );
        fixture.package("sys", "", "//! Sys.\n#![allow(unsafe_code)]\n");
        fixture.package(
            "platform",
            "[dependencies]\ntidepool-core = { path = \"../core\" }\n\
             tidepool-sys = { path = \"../sys\" }\n",
            "//! Platform.\n#![allow(unsafe_code)]\n",
        );
        fixture.package(
            "text",
            "[dependencies]\ntidepool-core = { path = \"../core\" }\n",
            TEXT_LIB,
        );
        fixture.write("LICENSE", LICENSE);
        fixture.write("NOTICE", NOTICE);
        fixture.write("docs/01-architecture.md", MATRIX);
        fixture.write("docs/roadmap.md", ROADMAP);
        fixture.write("docs/features.jsonc", FEATURES);
        fixture.write("xtask/deps-allowlist.txt", ALLOWLIST);
        fixture.workflow("ci.yml", CI_WORKFLOW);
        fixture.workflow("codeql.yml", CODEQL_WORKFLOW);
        fixture.lock();
        fixture
    }

    /// The fixture with the real repository's [`UCD_FILES`] as well, so
    /// that `ucd` passes too. The tables are not part of the fixture's
    /// `tidepool-text`, so nothing compiles or formats them.
    fn with_ucd(self) -> Self {
        let real = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        for path in UCD_FILES {
            let target = self.root.join(path);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).expect("create the parent directory");
            }
            fs::copy(real.join(path), &target).expect("copy the repository's Unicode data");
        }
        self
    }

    /// Writes the package `tidepool-<dir>` in `crates/<dir>`, with its
    /// README and its copies of [`LICENSE`] and [`NOTICE`]. Its manifest
    /// inherits from the workspace and keeps the rules of `crates`; `tables`
    /// come last, so that text appended to the manifest goes on with them.
    fn package(&self, dir: &str, tables: &str, lib: &str) {
        let name = format!("tidepool-{dir}");
        self.write(
            &format!("crates/{dir}/Cargo.toml"),
            &member_manifest(&name, tables),
        );
        self.write(&format!("crates/{dir}/README.md"), &readme_of(&name));
        self.write(&format!("crates/{dir}/LICENSE"), LICENSE);
        self.write(&format!("crates/{dir}/NOTICE"), NOTICE);
        self.source(&format!("crates/{dir}/src/lib.rs"), lib);
    }

    /// Writes the Rust source `body` to `path`, relative to the repository
    /// root, after [`RUST_HEADER`], as every source file in the repository
    /// starts.
    fn source(&self, path: &str, body: &str) {
        self.write(path, &format!("{RUST_HEADER}{body}"));
    }

    /// Writes the workflow `body` to `.github/workflows/<name>`, after its
    /// header ([`hash_header`]), as every workflow in the repository starts.
    fn workflow(&self, name: &str, body: &str) {
        self.write(
            &format!(".github/workflows/{name}"),
            &format!("{}{body}", hash_header()),
        );
    }

    /// Writes `contents` to `path`, relative to the repository root.
    fn write(&self, path: &str, contents: &str) {
        self.portable(path, contents);
        write(&self.root.join(path), contents);
    }

    /// Appends `contents` to `path`, relative to the repository root.
    fn append(&self, path: &str, contents: &str) {
        self.portable(path, contents);
        let path = self.root.join(path);
        let mut text = fs::read_to_string(&path).expect("the file exists");
        text.push_str(contents);
        write(&path, &text);
    }

    /// Asserts that TOML for `path` holds no absolute path of this fixture
    /// and no backslash, so that it means the same on every OS (see the
    /// module documentation). Checked on every OS, so that a fixture that
    /// would fail only on Windows fails here too.
    fn portable(&self, path: &str, contents: &str) {
        if !path.ends_with(".toml") {
            return;
        }
        let case = self.case.to_string_lossy();
        let real = fs::canonicalize(&self.case).map(|real| real.to_string_lossy().into_owned());
        assert!(
            !contents.contains('\\')
                && !contents.contains(case.as_ref())
                && real.is_ok_and(|real| !contents.contains(&real)),
            "{path}: write paths in fixture TOML relative, with `/`:\n{contents}"
        );
    }

    /// Writes (or rewrites) the repository's `Cargo.lock`, offline.
    fn lock(&self) {
        self.lock_workspace("Cargo.toml");
    }

    /// Writes (or rewrites) the `Cargo.lock` of the workspace of
    /// `manifest`, offline, from the repository root, so that the
    /// fixture's own `.cargo/config.toml` applies as it does for `xtask`.
    fn lock_workspace(&self, manifest: &str) {
        let status = Command::new(cargo())
            .args(["generate-lockfile", "--offline", "--manifest-path"])
            .arg(self.root.join(manifest))
            .current_dir(&self.root)
            .env("CARGO_NET_OFFLINE", "true")
            .output()
            .expect("cargo runs");
        assert!(
            status.status.success(),
            "cargo generate-lockfile failed:\n{}",
            String::from_utf8_lossy(&status.stderr)
        );
    }

    /// Writes `contents` to `path`, relative to the case directory: outside
    /// the repository.
    fn write_outside(&self, path: &str, contents: &str) {
        self.portable(path, contents);
        write(&self.case.join(path), contents);
    }

    /// The canonical path of `path`, relative to the case directory.
    fn real(&self, path: &str) -> PathBuf {
        fs::canonicalize(self.case.join(path)).expect("the path exists")
    }

    /// Runs `xtask <command> --root <the fixture>`.
    fn run(&self, command: &str) -> Run {
        self.run_with(command, &[])
    }

    /// [`Fixture::run`], with the variables in `env` set (`None` removes
    /// one).
    fn run_with(&self, command: &str, env: &[(&str, Option<&str>)]) -> Run {
        let root = self.root.to_string_lossy().into_owned();
        xtask_with(
            &[command, "--root", &root],
            Some(&self.case.join("target")),
            env,
        )
    }
}

/// Excludes `vendor/` from the fixture's workspace, as a repository that
/// vendors code would: cargo then accepts a vendored crate named like a
/// member, or with a `[workspace]` of its own, under the workspace root.
fn exclude_vendor(fixture: &Fixture) {
    let path = fixture.root.join("Cargo.toml");
    let text = fs::read_to_string(&path).expect("the workspace manifest exists");
    write(
        &path,
        &text.replace(
            "resolver = \"3\"\n",
            "resolver = \"3\"\nexclude = [\"vendor\"]\n",
        ),
    );
}

/// Excludes `dir` from the fixture's workspace.
fn exclude(fixture: &Fixture, dir: &str) {
    let path = fixture.root.join("Cargo.toml");
    let text = fs::read_to_string(&path).expect("the workspace manifest exists");
    write(
        &path,
        &text.replace(
            "resolver = \"3\"\n",
            &format!("resolver = \"3\"\nexclude = [\"{dir}\"]\n"),
        ),
    );
}

/// The manifest of the member `name`, as [`Fixture::package`] writes it:
/// inheriting `edition`, `version`, `license` and lints from the workspace,
/// and not published, then `tables`.
fn member_manifest(name: &str, tables: &str) -> String {
    format!(
        "[package]\nname = \"{name}\"\nedition.workspace = true\nversion.workspace = true\n\
         license.workspace = true\npublish = false\n\n[lints]\nworkspace = true\n\n{tables}"
    )
}

/// A package manifest for `name`, with `tables` after the package table.
fn manifest(name: &str, version: &str, tables: &str) -> String {
    format!(
        "[package]\nname = \"{name}\"\nversion = \"{version}\"\nedition = \"2024\"\n\
         publish = false\n\n{tables}"
    )
}

/// Makes `link` a symbolic link to `target`, or says why it cannot.
fn symlink(target: &Path, link: &Path) -> Result<(), String> {
    if let Some(parent) = link.parent() {
        fs::create_dir_all(parent).expect("create the parent directory");
    }
    // `fs::soft_link` is deprecated in favour of one function per OS, but
    // those would need a platform `cfg`, which xtask must not have
    // (`AGENTS.md` rule 5). This one works wherever links do. On Windows it
    // always makes a file symbolic link, even to a directory, so directory
    // links and junctions are not covered there.
    #[allow(deprecated)]
    fs::soft_link(target, link).map_err(|error| error.to_string())
}

/// Reports a test that cannot run here. It is never counted as a pass.
fn skipped(test: &str, reason: &str) {
    let _ = writeln!(io::stderr(), "SKIPPED: {test}: {reason}");
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.case);
    }
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create the parent directory");
    }
    fs::write(path, contents).expect("write the file");
}

/// Asserts that `command` passes on `fixture`.
fn passes(fixture: &Fixture, command: &str) {
    let run = fixture.run(command);
    run.expect_code(0);
    assert!(
        run.stdout.ends_with(&format!("{command}: ok\n")),
        "{}",
        run.stdout
    );
}

// --- deps ---

#[test]
fn deps_passes_on_the_unbroken_fixture() {
    let fixture = Fixture::passing("deps-pass");
    let run = fixture.run("deps");
    run.expect_code(0);
    assert_eq!(
        run.stdout,
        "workspaces: 1 (Cargo.toml); lockfiles: 1\n\
         packages: 4 workspace members, 0 other path packages in the repository, 0 from outside \
         it\n\
         deps: ok\n"
    );
}

#[test]
fn deps_fails_on_a_path_package_outside_the_repository() {
    let fixture = Fixture::passing("deps-outside");
    write(
        &fixture.case.join("outside/left-pad/Cargo.toml"),
        "[package]\nname = \"left-pad\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n",
    );
    write(
        &fixture.case.join("outside/left-pad/src/lib.rs"),
        "//! Pads.\n",
    );
    fixture.append(
        "crates/text/Cargo.toml",
        "left-pad = { path = \"../../../outside/left-pad\" }\n",
    );
    fixture.lock();

    let run = fixture.run("deps");
    run.expect_code(1);
    let outside = fs::canonicalize(fixture.case.join("outside/left-pad/Cargo.toml"))
        .expect("the outside package exists");
    assert_eq!(
        run.violations(),
        [format!(
            "`left-pad 0.1.0` is a path package outside the repository ({}): vendored or \
             cross-repository code counts as a dependency; required by tidepool-text",
            outside.display()
        )]
    );
    assert!(
        run.stdout.ends_with("deps: FAILED (1 violation)\n"),
        "{}",
        run.stdout
    );
}

#[test]
fn deps_fails_closed_on_a_crates_io_dependency_it_cannot_resolve_offline() {
    // Offline, with the lockfile as it is, cargo cannot resolve this, so
    // the source check never sees it (that part is proved on canned
    // `cargo metadata` output in the unit tests); the check must still
    // fail, and say why.
    let fixture = Fixture::passing("deps-crates-io");
    fixture.append("crates/text/Cargo.toml", "left-pad = \"1\"\n");

    let run = fixture.run("deps");
    run.expect_code(1);
    let violations = run.violations();
    assert_eq!(violations.len(), 2, "{}", run.stdout);
    // What the member declares is checked all the same.
    assert_eq!(
        violations[0],
        "`tidepool-text` declares `left-pad ^1` (normal dependency) from \
         `registry+https://github.com/rust-lang/crates.io-index`, which is not in \
         xtask/deps-allowlist.txt"
    );
    assert!(
        violations[1].starts_with(
            "cannot resolve the workspace of Cargo.toml offline from its lockfile as it is"
        ),
        "{}",
        run.stdout
    );
    assert!(
        violations[1].contains("--offline --locked"),
        "{}",
        run.stdout
    );
}

#[test]
fn deps_fails_on_a_lockfile_source_line() {
    let fixture = Fixture::passing("deps-lockfile");
    let lines = fs::read_to_string(fixture.root.join("Cargo.lock"))
        .expect("the fixture has a lockfile")
        .lines()
        .count();
    fixture.append(
        "Cargo.lock",
        "\n[[package]]\nname = \"left-pad\"\nversion = \"1.0.3\"\n\
         source = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
    );

    let run = fixture.run("deps");
    run.expect_code(1);
    // Two violations: the lockfile now lists a package the workspace does
    // not use, so cargo will not resolve it `--locked` either.
    let violations = run.violations();
    assert_eq!(violations.len(), 2, "{}", run.stdout);
    assert!(
        violations[0].starts_with(
            "cannot resolve the workspace of Cargo.toml offline from its lockfile as it is"
        ),
        "{}",
        run.stdout
    );
    assert_eq!(
        violations[1],
        format!(
            "Cargo.lock:{}: `left-pad 1.0.3` comes from \
             `registry+https://github.com/rust-lang/crates.io-index`, which is not approved in \
             xtask/deps-allowlist.txt",
            lines + 5
        )
    );
}

#[test]
fn deps_fails_on_a_source_line_in_a_lockfile_no_workspace_uses() {
    // The same entry in a lockfile no manifest owns: one reason only.
    let fixture = Fixture::passing("deps-stray-lockfile");
    fixture.write(
        "docs/old/Cargo.lock",
        "version = 4\n\n[[package]]\nname = \"left-pad\"\nversion = \"1.0.3\"\n\
         source = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
    );
    let run = fixture.run("deps");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        ["docs/old/Cargo.lock:6: `left-pad 1.0.3` comes from \
          `registry+https://github.com/rust-lang/crates.io-index`, which is not approved in \
          xtask/deps-allowlist.txt"]
    );
}

/// A fixture whose `tidepool-text` depends on `left-pad = "=1.0.3"` from
/// crates.io, served offline by a directory source outside the repository,
/// so that cargo resolves a real registry package without the network. The
/// `.cargo/config.toml` that replaces crates.io with it is outside the
/// repository too, in the case directory above it, where cargo (run in the
/// repository) still reads it: inside, `deps` would reject it. A relative
/// path in a configuration file is relative to the directory that holds its
/// `.cargo`: here the case directory.
fn with_vendored_crates_io_package(name: &str) -> Fixture {
    let fixture = Fixture::passing(name);
    fixture.write_outside(
        "vendor/left-pad/Cargo.toml",
        &manifest("left-pad", "1.0.3", ""),
    );
    fixture.write_outside("vendor/left-pad/src/lib.rs", "//! Pads.\n");
    fixture.write_outside(
        "vendor/left-pad/.cargo-checksum.json",
        "{\"files\":{},\"package\":null}",
    );
    fixture.write_outside(
        ".cargo/config.toml",
        "[source.crates-io]\nreplace-with = \"vendored\"\n\n[source.vendored]\n\
         directory = \"vendor\"\n",
    );
    fixture.append("crates/text/Cargo.toml", "left-pad = \"=1.0.3\"\n");
    fixture.lock();
    fixture
}

#[test]
fn deps_fails_on_a_crates_io_package_and_passes_once_it_is_approved() {
    let fixture = with_vendored_crates_io_package("deps-registry");
    let crates_io = "registry+https://github.com/rust-lang/crates.io-index";
    let lockfile = fs::read_to_string(fixture.root.join("Cargo.lock")).expect("a lockfile");
    let source_line = lockfile
        .lines()
        .position(|line| line.starts_with("source = "))
        .expect("the lockfile has a source line")
        + 1;
    let run = fixture.run("deps");
    run.expect_code(1);
    // Cargo lists packages by name, so `left-pad` comes first.
    assert_eq!(
        run.violations(),
        [
            format!(
                "`left-pad 1.0.3` ({crates_io}) is not in xtask/deps-allowlist.txt; required by \
                 tidepool-text"
            ),
            format!(
                "`tidepool-text` declares `left-pad =1.0.3` (normal dependency) from \
                 `{crates_io}`, which is not in xtask/deps-allowlist.txt"
            ),
            format!(
                "Cargo.lock:{source_line}: `left-pad 1.0.3` comes from `{crates_io}`, which is \
                 not approved in xtask/deps-allowlist.txt"
            ),
        ]
    );

    // The exact pin approves it, end to end.
    fixture.append("xtask/deps-allowlist.txt", "left-pad 1.0.3 ADR-0099\n");
    passes(&fixture, "deps");
    // A range requirement is not what the policy approves.
    let text = fixture.root.join("crates/text/Cargo.toml");
    let manifest = fs::read_to_string(&text).expect("the manifest exists");
    write(&text, &manifest.replace("\"=1.0.3\"", "\"1.0\""));
    fixture.lock();
    let run = fixture.run("deps");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            "`tidepool-text` declares `left-pad ^1.0` (normal dependency) from crates.io; the \
          approved requirement is the exact `=1.0.3`"
        ]
    );
}

/// A fixture with a crate in `vendor/left-pad` that is no workspace's
/// member, and that `tidepool-text` uses as `left-pad = "1"` through a
/// `[patch.crates-io]` in `patch_file`. Its path is relative to the
/// repository root both in the root manifest and in `.cargo/config.toml`.
fn with_patched_crates_io_dependency(name: &str, patch_file: &str) -> Fixture {
    let fixture = Fixture::passing(name);
    fixture.write(
        "vendor/left-pad/Cargo.toml",
        &manifest("left-pad", "1.0.3", ""),
    );
    fixture.write("vendor/left-pad/src/lib.rs", "//! Pads.\n");
    let patch = "\n[patch.crates-io]\nleft-pad = { path = \"vendor/left-pad\" }\n";
    if patch_file == "Cargo.toml" {
        fixture.append("Cargo.toml", patch);
    } else {
        fixture.write(patch_file, patch);
    }
    fixture.append("crates/text/Cargo.toml", "left-pad = \"1\"\n");
    fixture.lock();
    fixture
}

#[test]
fn deps_fails_on_a_crates_io_dependency_patched_to_a_vendored_path() {
    // `scripts/check-deps.sh` catches this through the declared source;
    // so must `deps`, whether the patch is in the manifest or in cargo's
    // configuration.
    for patch_file in ["Cargo.toml", ".cargo/config.toml"] {
        let fixture = with_patched_crates_io_dependency("deps-patch", patch_file);
        let run = fixture.run("deps");
        run.expect_code(1);
        // A `[patch]` in the repository's cargo configuration is itself a
        // violation, whatever it patches.
        let config: &[&str] = if patch_file == "Cargo.toml" {
            &[]
        } else {
            &[
                ".cargo/config.toml:2: only an `[alias]` table is allowed, but found \
                 `[patch.…]`",
                ".cargo/config.toml:3: a key outside `[alias]`, but found `left-pad = …`",
            ]
        };
        assert_eq!(
            run.violations(),
            config.iter().chain(&[
                "`left-pad 1.0.3` is a path package in the repository (vendor/left-pad/Cargo.toml) \
                 that is not a member of this workspace: vendored or copied code, or another \
                 workspace's crate, counts as a dependency; required by tidepool-text",
                "`tidepool-text` declares `left-pad ^1` (normal dependency) from \
                 `registry+https://github.com/rust-lang/crates.io-index`, which is not in \
                 xtask/deps-allowlist.txt",
                "cannot resolve the workspace of vendor/left-pad/Cargo.toml offline from its \
                 lockfile as it is (an external dependency, a package that belongs to no \
                 workspace, or a Cargo.lock that needs updating): `cargo metadata \
                 --format-version 1 --offline --locked --manifest-path",
            ])
            .map(|expected| {
                run.violations()
                    .iter()
                    .find(|found| found.starts_with(expected))
                    .copied()
                    .unwrap_or_else(|| {
                        panic!("`{expected}` missing in {patch_file}:\n{}", run.stdout)
                    })
            })
            .collect::<Vec<_>>(),
            "{patch_file}"
        );
        assert_eq!(
            run.violations().len(),
            3 + config.len(),
            "{patch_file}:\n{}",
            run.stdout
        );
    }
}

#[test]
fn deps_and_layers_fail_on_a_vendored_crate_named_like_a_member() {
    let fixture = Fixture::passing("deps-namesake");
    exclude_vendor(&fixture);
    fixture.write(
        "vendor/tidepool-core/Cargo.toml",
        &manifest("tidepool-core", "0.2.0", ""),
    );
    fixture.write("vendor/tidepool-core/src/lib.rs", "//! Not ours.\n");
    fixture.write(
        "crates/text/Cargo.toml",
        &manifest(
            "tidepool-text",
            "0.1.0",
            "[dependencies]\ntidepool-core = { path = \"../../vendor/tidepool-core\" }\n",
        ),
    );
    fixture.lock();
    let vendored = "`tidepool-core 0.2.0` is a path package in the repository \
                    (vendor/tidepool-core/Cargo.toml) that is not a member of this workspace: \
                    vendored or copied code, or another workspace's crate, counts as a \
                    dependency; required by tidepool-text";
    let run = fixture.run("deps");
    run.expect_code(1);
    let violations = run.violations();
    assert_eq!(violations.len(), 2, "{}", run.stdout);
    assert_eq!(violations[0], vendored);
    // Its manifest belongs to no workspace (the case directory's encloses
    // it, and does not list it: `Fixture::empty`), so cargo cannot describe
    // it.
    assert!(
        violations[1].starts_with(
            "cannot resolve the workspace of vendor/tidepool-core/Cargo.toml offline from its \
             lockfile as it is (an external dependency, a package that belongs to no workspace, \
             or a Cargo.lock that needs updating): "
        ),
        "{}",
        run.stdout
    );
    assert!(
        run.stdout.ends_with("deps: FAILED (2 violations)\n"),
        "{}",
        run.stdout
    );
    // Named as resolved, relative to the workspace root, the same on
    // every OS and whatever links lead to the build directory.
    let run = fixture.run("layers");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            "tidepool-text -> tidepool-core not allowed (normal dependency): `tidepool-core` is a \
          path crate that is not a workspace member (vendor/tidepool-core)"
        ]
    );

    // In a checkout with no workspace above it, cargo makes such a crate
    // the root of a workspace of its own, as a `[workspace]` table does,
    // and it has no lockfile: that is a workspace the repository may not
    // have, and one cargo cannot resolve with `--locked`.
    fixture.write(
        "vendor/tidepool-core/Cargo.toml",
        &manifest("tidepool-core", "0.2.0", "[workspace]\n"),
    );
    let run = fixture.run("deps");
    run.expect_code(1);
    let violations = run.violations();
    assert_eq!(violations.len(), 3, "{}", run.stdout);
    assert_eq!(violations[0], vendored);
    assert_eq!(
        violations[1],
        "vendor/tidepool-core/Cargo.toml belongs to the workspace at vendor/tidepool-core, but \
         the repository has two workspaces only, the root one and agents/ \
         (docs/01-architecture.md): code in any other is vendored or copied code that no check \
         reads as part of Tidepool"
    );
    assert!(
        violations[2].starts_with(
            "cannot resolve the workspace of vendor/tidepool-core/Cargo.toml offline from its \
             lockfile as it is: its Cargo.lock is out of date, so the graph was not checked."
        ),
        "{}",
        run.stdout
    );
    // Not only out-of-date lockfiles: the plain form.
    assert!(
        run.stdout.ends_with("deps: FAILED (3 violations)\n"),
        "{}",
        run.stdout
    );
}

#[test]
fn deps_fails_on_a_vendored_crate_with_a_workspace_of_its_own() {
    // A `[workspace]` table makes it the member of a workspace in the
    // repository, just not of this one or the root one.
    let fixture = Fixture::passing("deps-own-workspace");
    exclude_vendor(&fixture);
    fixture.write(
        "vendor/left-pad/Cargo.toml",
        &manifest("left-pad", "1.0.3", "[workspace]\n"),
    );
    fixture.write("vendor/left-pad/src/lib.rs", "//! Pads.\n");
    fixture.append(
        "crates/text/Cargo.toml",
        "left-pad = { path = \"../../vendor/left-pad\" }\n",
    );
    fixture.lock();
    fixture.lock_workspace("vendor/left-pad/Cargo.toml");
    let run = fixture.run("deps");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            "`left-pad 1.0.3` is a path package in the repository (vendor/left-pad/Cargo.toml) \
          that is not a member of this workspace: vendored or copied code, or another \
          workspace's crate, counts as a dependency; required by tidepool-text",
            // And its workspace is one the repository may not have.
            "vendor/left-pad/Cargo.toml belongs to the workspace at vendor/left-pad, but the \
          repository has two workspaces only, the root one and agents/ \
          (docs/01-architecture.md): code in any other is vendored or copied code that no check \
          reads as part of Tidepool",
        ]
    );
}

#[test]
fn deps_and_layers_fail_on_a_member_patched_in_under_a_registry_requirement() {
    // `tidepool-core` asks for `tidepool-text = "0.1"` from crates.io, and
    // a `[patch]` serves the workspace member: the forbidden edge builds.
    let fixture = Fixture::passing("deps-patched-member");
    fixture.write(
        "crates/text/Cargo.toml",
        &manifest("tidepool-text", "0.1.0", ""),
    );
    fixture.append(
        "crates/core/Cargo.toml",
        "[dependencies]\ntidepool-text = \"0.1\"\n",
    );
    // The member's own path, as the workspace names it, so that the patch
    // and the member are one package whatever links lead here.
    fixture.append(
        "Cargo.toml",
        "\n[patch.crates-io]\ntidepool-text = { path = \"crates/text\" }\n",
    );
    fixture.lock();
    let run = fixture.run("deps");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            "`tidepool-core` declares `tidepool-text ^0.1` (normal dependency) from \
          `registry+https://github.com/rust-lang/crates.io-index`, which is not in \
          xtask/deps-allowlist.txt"
        ]
    );
    let run = fixture.run("layers");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            "tidepool-core -> tidepool-text not allowed (normal dependency): the matrix row for \
          `tidepool-core` (docs/01-architecture.md:7) allows no workspace crate"
        ]
    );
}

#[test]
fn deps_checks_every_workspace_and_every_lockfile() {
    // A second workspace, like `agents/`, that uses code from outside the
    // repository, and a crate of the root workspace: the reference agents
    // are std-only (`docs/subsystems/agents.md`).
    let fixture = Fixture::passing("deps-nested");
    fixture.write_outside(
        "outside/left-pad/Cargo.toml",
        &manifest("left-pad", "0.1.0", "[workspace]\n"),
    );
    fixture.write_outside("outside/left-pad/src/lib.rs", "//! Pads.\n");
    agents_workspace(
        &fixture,
        "[dependencies]\nleft-pad = { path = \"../../../outside/left-pad\" }\n\
         tidepool-core = { path = \"../../crates/core\" }\n",
    );
    fixture.lock_workspace("agents/Cargo.toml");
    let run = fixture.run("deps");
    run.expect_code(1);
    let mut violations = run.violations();
    violations.sort_unstable();
    assert_eq!(
        violations,
        [
            format!(
                "`left-pad 0.1.0` is a path package outside the repository ({}): vendored or \
                 cross-repository code counts as a dependency; required by tp-agent-mock",
                fixture.real("outside/left-pad/Cargo.toml").display()
            ),
            "`tidepool-core 0.1.0` is a path package in the repository \
             (crates/core/Cargo.toml) that is not a member of this workspace: vendored or copied \
             code, or another workspace's crate, counts as a dependency; required by \
             tp-agent-mock"
                .to_string(),
        ]
    );
    assert!(
        run.stdout.contains(
            "workspaces: 2 (Cargo.toml, agents/Cargo.toml); lockfiles: 2\n\
             packages: 5 workspace members, 1 other path packages in the repository, 1 from \
             outside it\n"
        ),
        "{}",
        run.stdout
    );

    // And a `source` line in the nested lockfile.
    let fixture = Fixture::passing("deps-nested-lockfile");
    agents_workspace(&fixture, "");
    fixture.lock_workspace("agents/Cargo.toml");
    passes(&fixture, "deps");
    let lines = fs::read_to_string(fixture.root.join("agents/Cargo.lock"))
        .expect("agents/ has a lockfile")
        .lines()
        .count();
    fixture.append(
        "agents/Cargo.lock",
        "\n[[package]]\nname = \"left-pad\"\nversion = \"1.0.3\"\n\
         source = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
    );
    let run = fixture.run("deps");
    run.expect_code(1);
    assert!(
        run.violations()
            .iter()
            .any(|violation| violation.starts_with(&format!(
                "agents/Cargo.lock:{}: `left-pad 1.0.3` comes from \
             `registry+https://github.com/rust-lang/crates.io-index`",
                lines + 5
            ))),
        "{}",
        run.stdout
    );
}

/// Gives `fixture` the second workspace, `agents/`, with the one member
/// `agents/tp-agent-mock`, whose manifest ends with `tables`. The caller
/// locks it, if it can be locked offline.
fn agents_workspace(fixture: &Fixture, tables: &str) {
    fixture.write(
        "agents/Cargo.toml",
        "[workspace]\nresolver = \"3\"\nmembers = [\"tp-agent-mock\"]\n",
    );
    fixture.write(
        "agents/tp-agent-mock/Cargo.toml",
        &manifest("tp-agent-mock", "0.1.0", tables),
    );
    fixture.write("agents/tp-agent-mock/src/lib.rs", "//! Mock.\n");
}

#[test]
fn deps_checks_workspaces_in_hidden_directories() {
    let fixture = Fixture::passing("deps-hidden");
    fixture.write(
        ".devtools/gen/Cargo.toml",
        &manifest(
            "gen",
            "0.1.0",
            "[workspace]\n\n[dependencies]\nleft-pad = \"1\"\n",
        ),
    );
    fixture.write(".devtools/gen/src/lib.rs", "//! Gen.\n");
    fixture.write(
        ".devtools/gen/Cargo.lock",
        "version = 4\n\n[[package]]\nname = \"left-pad\"\nversion = \"1.0.3\"\n\
         source = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
    );
    let run = fixture.run("deps");
    run.expect_code(1);
    let violations = run.violations();
    assert_eq!(violations.len(), 4, "{}", run.stdout);
    assert!(
        violations[0].starts_with(
            ".devtools/gen/Cargo.toml belongs to the workspace at .devtools/gen, but the \
             repository has two workspaces only"
        ),
        "{}",
        run.stdout
    );
    assert!(
        violations[1].starts_with("`gen` declares `left-pad ^1` (normal dependency) from"),
        "{}",
        run.stdout
    );
    assert!(
        violations[2].starts_with("cannot resolve the workspace of .devtools/gen/Cargo.toml"),
        "{}",
        run.stdout
    );
    assert!(
        violations[3].starts_with(".devtools/gen/Cargo.lock:6: `left-pad 1.0.3` comes from"),
        "{}",
        run.stdout
    );
}

#[test]
fn deps_reports_a_directory_behind_a_symbolic_link() {
    let fixture = Fixture::passing("deps-link");
    fixture.write_outside(
        "outside/tools/Cargo.toml",
        &manifest(
            "tools",
            "0.1.0",
            "[workspace]\n\n[dependencies]\nleft-pad = \"1\"\n",
        ),
    );
    if let Err(reason) = symlink(&fixture.real("outside/tools"), &fixture.root.join("tools")) {
        skipped("deps_reports_a_directory_behind_a_symbolic_link", &reason);
        return;
    }
    let run = fixture.run("deps");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            "tools is a symbolic link, which this check does not follow: what it leads to was not \
          checked"
        ]
    );
}

#[test]
fn deps_names_an_out_of_date_lockfile_and_its_advice_works() {
    // Right after a manifest edit that adds an edge between members, the
    // lockfile is out of date and `--locked` stops cargo: still a
    // violation, since the graph was not checked, but the advice it gives
    // (which compiles nothing) makes the next run pass.
    let fixture = Fixture::passing("deps-stale");
    fixture.append(
        "crates/sys/Cargo.toml",
        "[dev-dependencies]\ntidepool-core = { path = \"../core\" }\n",
    );
    let run = fixture.run("deps");
    run.expect_code(1);
    let violations = run.violations();
    assert_eq!(violations.len(), 1, "{}", run.stdout);
    assert!(
        violations[0].starts_with(
            "cannot resolve the workspace of Cargo.toml offline from its lockfile as it is: its \
             Cargo.lock is out of date, so the graph was not checked. Update the lockfile \
             offline, which compiles nothing (`cargo metadata --offline --format-version 1 \
             --manifest-path Cargo.toml`), then run `cargo xtask deps` again: "
        ),
        "{}",
        run.stdout
    );
    // The last line says so in fixed words, which the Claude Code hook
    // reads instead of matching the violation's text.
    assert!(
        run.stdout
            .ends_with("deps: FAILED (1 violation; only out-of-date lockfiles)\n"),
        "{}",
        run.stdout
    );
    let updated = Command::new(cargo())
        .args([
            "metadata",
            "--offline",
            "--format-version",
            "1",
            "--manifest-path",
            "Cargo.toml",
        ])
        .current_dir(&fixture.root)
        .env("CARGO_TARGET_DIR", fixture.case.join("target"))
        .output()
        .expect("cargo runs");
    assert!(
        updated.status.success(),
        "{}",
        String::from_utf8_lossy(&updated.stderr)
    );
    assert!(
        !fixture.case.join("target").exists(),
        "the advice compiled something"
    );
    passes(&fixture, "deps");
}

#[test]
fn deps_names_an_out_of_date_lockfile_whatever_colour_cargo_is_asked_for() {
    // A CI job may set `CARGO_TERM_COLOR=always`, and a cargo configuration
    // above the repository (which no check reads) may set `term.color`:
    // cargo then colours its `error:` lines even into a pipe, and an
    // out-of-date lockfile must still be told from any other failure.
    let fixture = Fixture::passing("deps-stale-colour");
    fixture.append(
        "crates/sys/Cargo.toml",
        "[dev-dependencies]\ntidepool-core = { path = \"../core\" }\n",
    );
    let stale = |run: &Run| {
        run.expect_code(1);
        assert_eq!(run.violations().len(), 1, "{}", run.stdout);
        assert!(
            run.stdout
                .ends_with("deps: FAILED (1 violation; only out-of-date lockfiles)\n"),
            "{}",
            run.stdout
        );
        assert!(!run.stdout.contains('\x1b'), "{}", run.stdout);
    };
    stale(&fixture.run_with("deps", &[("CARGO_TERM_COLOR", Some("always"))]));
    // Outside the repository, so the check itself does not read it.
    write(
        &fixture.case.join(".cargo/config.toml"),
        "[term]\ncolor = \"always\"\n",
    );
    stale(&fixture.run_with("deps", &[("CARGO_TERM_COLOR", None)]));
}

#[test]
fn deps_counts_out_of_date_lockfiles_in_every_workspace() {
    // A second reference agent joins `agents/`, and the root workspace gets
    // an edge between members, with neither lockfile updated.
    let fixture = Fixture::passing("deps-stale-two");
    agents_workspace(&fixture, "");
    fixture.lock_workspace("agents/Cargo.toml");
    passes(&fixture, "deps");
    fixture.write(
        "agents/Cargo.toml",
        "[workspace]\nresolver = \"3\"\nmembers = [\"tp-agent-mock\", \"tp-agent-local\"]\n",
    );
    fixture.write(
        "agents/tp-agent-local/Cargo.toml",
        &manifest("tp-agent-local", "0.1.0", ""),
    );
    fixture.write("agents/tp-agent-local/src/lib.rs", "//! Local.\n");
    fixture.append(
        "crates/sys/Cargo.toml",
        "[dev-dependencies]\ntidepool-core = { path = \"../core\" }\n",
    );
    let run = fixture.run("deps");
    run.expect_code(1);
    let violations = run.violations();
    assert_eq!(violations.len(), 2, "{}", run.stdout);
    for (violation, shown) in violations.iter().zip(["Cargo.toml", "agents/Cargo.toml"]) {
        assert!(
            violation.starts_with(&format!(
                "cannot resolve the workspace of {shown} offline from its lockfile as it is: its \
                 Cargo.lock is out of date, so the graph was not checked."
            )),
            "{}",
            run.stdout
        );
    }
    assert!(
        run.stdout
            .ends_with("deps: FAILED (2 violations; only out-of-date lockfiles)\n"),
        "{}",
        run.stdout
    );
}

#[test]
fn deps_never_calls_a_stale_lockfile_alongside_another_problem_the_only_one() {
    // The root workspace's lockfile is only out of date, and `agents/`
    // uses a path package outside the repository, in a directory whose name
    // holds the words of the stale-lockfile message: every violation line
    // then holds them, so only the last line can tell the two apart.
    let fixture = Fixture::passing("deps-stale-and-outside");
    let outside = "outside/Cargo.lock is out of date/left-pad";
    fixture.write_outside(
        &format!("{outside}/Cargo.toml"),
        &manifest("left-pad", "0.1.0", "[workspace]\n"),
    );
    fixture.write_outside(&format!("{outside}/src/lib.rs"), "//! Pads.\n");
    agents_workspace(
        &fixture,
        &format!("[dependencies]\nleft-pad = {{ path = \"../../../{outside}\" }}\n"),
    );
    fixture.lock_workspace("agents/Cargo.toml");
    fixture.append(
        "crates/sys/Cargo.toml",
        "[dev-dependencies]\ntidepool-core = { path = \"../core\" }\n",
    );
    let run = fixture.run("deps");
    run.expect_code(1);
    let violations = run.violations();
    assert_eq!(violations.len(), 2, "{}", run.stdout);
    assert!(
        violations[0].starts_with(
            "cannot resolve the workspace of Cargo.toml offline from its lockfile as it is: its \
             Cargo.lock is out of date, so the graph was not checked."
        ),
        "{}",
        run.stdout
    );
    assert_eq!(
        violations[1],
        format!(
            "`left-pad 0.1.0` is a path package outside the repository ({}): vendored or \
             cross-repository code counts as a dependency; required by tp-agent-mock",
            fixture.real(&format!("{outside}/Cargo.toml")).display()
        )
    );
    assert!(
        violations
            .iter()
            .all(|violation| violation.contains("Cargo.lock is out of date")),
        "{}",
        run.stdout
    );
    assert_eq!(
        run.stdout.lines().last(),
        Some("deps: FAILED (2 violations)"),
        "{}",
        run.stdout
    );
}

/// Makes `<repository>/<at>` a copy of a small workspace, registered as
/// the linked worktree `x` of the repository the way `git worktree add`
/// records one: `<at>/.git` names `.git/worktrees/x`, whose `gitdir` names
/// `<at>/.git` back. The copy breaks every check if it is checked here:
/// its sys crate root is not at `crates/sys/`, and it has no lockfile.
fn add_worktree(fixture: &Fixture, at: &str) {
    fixture.write(
        &format!("{at}/Cargo.toml"),
        "[workspace]\nresolver = \"3\"\nmembers = [\"crates/sys\", \"crates/extra\"]\n",
    );
    fixture.write(
        &format!("{at}/crates/sys/Cargo.toml"),
        &manifest("tidepool-sys", "0.1.0", ""),
    );
    fixture.write(
        &format!("{at}/crates/sys/src/lib.rs"),
        "//! Sys.\n#![allow(unsafe_code)]\n",
    );
    fixture.write(
        &format!("{at}/crates/extra/Cargo.toml"),
        &manifest("tidepool-extra", "0.1.0", ""),
    );
    fixture.write(&format!("{at}/crates/extra/src/lib.rs"), "//! Extra.\n");
    let up = "../".repeat(at.split('/').count());
    write(
        &fixture.root.join(at).join(".git"),
        &format!("gitdir: {up}.git/worktrees/x\n"),
    );
    let real = fs::canonicalize(fixture.root.join(at)).expect("the worktree exists");
    write(
        &fixture.root.join(".git/worktrees/x/gitdir"),
        &format!("{}\n", real.join(".git").display()),
    );
    write(
        &fixture.root.join(".git/worktrees/x/HEAD"),
        "ref: refs/heads/x\n",
    );
}

#[test]
fn deps_and_unsafe_audit_skip_a_linked_worktree_of_the_repository() {
    let fixture = Fixture::passing("worktree");
    write(&fixture.root.join(".git/HEAD"), "ref: refs/heads/main\n");
    add_worktree(&fixture, ".claude/worktrees/x");
    let note = "skipped linked worktrees of this repository (run xtask in each to check it): \
                .claude/worktrees/x\n";
    for command in ["deps", "unsafe-audit", "layers"] {
        let run = fixture.run(command);
        run.expect_code(0);
        assert!(
            run.stdout.ends_with(&format!("{command}: ok\n")),
            "{}",
            run.stdout
        );
        assert_eq!(
            run.stdout.contains(note),
            command != "layers",
            "{command}:\n{}",
            run.stdout
        );
    }

    // Without git's record of it, it is no worktree: checked, and failing.
    fs::remove_file(fixture.root.join(".git/worktrees/x/gitdir")).expect("the record exists");
    let run = fixture.run("deps");
    run.expect_code(1);
    assert!(
        run.violations().iter().any(|violation| violation
            .starts_with("cannot resolve the workspace of .claude/worktrees/x/Cargo.toml")),
        "{}",
        run.stdout
    );
    let run = fixture.run("unsafe-audit");
    run.expect_code(1);
    assert!(
        run.violations().contains(
            &".claude/worktrees/x/crates/sys/src/lib.rs:2: `allow(unsafe_code)` is only allowed \
              in the crate roots of tidepool-sys and tidepool-platform"
        ),
        "{}",
        run.stdout
    );
}

#[test]
fn code_in_a_skipped_worktree_still_fails_a_check_when_this_checkout_builds_it() {
    // Skipping a worktree does not let its code into this checkout's
    // build: cargo builds it only as a path package or a workspace member,
    // which the checks see through `cargo metadata` whatever the walk
    // skipped.
    let fixture = Fixture::passing("worktree-used");
    write(&fixture.root.join(".git/HEAD"), "ref: refs/heads/main\n");
    add_worktree(&fixture, "wt/x");
    fixture.append(
        "crates/text/Cargo.toml",
        "tidepool-extra = { path = \"../../wt/x/crates/extra\" }\n",
    );
    let note = "skipped linked worktrees of this repository (run xtask in each to check it): \
                wt/x\n";

    // A path dependency under the workspace root is a member: `layers`
    // finds no row for it, nor the edge, and the audit never read its
    // crate root.
    fixture.lock();
    let run = fixture.run("layers");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            "tidepool-text -> tidepool-extra not allowed (normal dependency): the matrix row for \
          `tidepool-text` (docs/01-architecture.md:10) allows `core`",
            "workspace member `tidepool-extra` has no row in the dependency matrix \
          (docs/01-architecture.md)",
        ]
    );
    let run = fixture.run("unsafe-audit");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            "wt/x/crates/extra/src/lib.rs: the lib target `tidepool_extra` of `tidepool-extra` \
          starts from a file the audit did not read; a crate root must be a `.rs` file in the \
          repository, outside build output and not behind a symbolic link"
        ]
    );
    assert!(run.stdout.contains(note), "{}", run.stdout);

    // Excluded from the workspace, it is a path package that is no member:
    // `deps` rejects it.
    exclude_vendor(&fixture);
    let manifest = fixture.root.join("Cargo.toml");
    let text = fs::read_to_string(&manifest).expect("the workspace manifest exists");
    write(
        &manifest,
        &text.replace("exclude = [\"vendor\"]", "exclude = [\"wt\"]"),
    );
    fixture.lock();
    let run = fixture.run("deps");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            "`tidepool-extra 0.1.0` is a path package in the repository \
          (wt/x/crates/extra/Cargo.toml) that is not a member of this workspace: vendored or \
          copied code, or another workspace's crate, counts as a dependency; required by \
          tidepool-text"
        ]
    );
    assert!(run.stdout.contains(note), "{}", run.stdout);
}

#[test]
fn deps_searches_a_build_directory_that_cargo_configuration_names() {
    // Probe p16: a committed `.cargo/config.toml` whose `[env]` names
    // `agents/` as the build directory, which cargo then hands to `cargo
    // xtask`. Simulated here by setting it for the binary, as cargo would.
    let fixture = Fixture::passing("deps-env-target");
    fixture.write(
        ".cargo/config.toml",
        "[alias]\nxtask = \"run --package xtask --\"\n\n[env]\n\
         CARGO_TARGET_DIR = { value = \"agents\", relative = true, force = true }\n",
    );
    agents_workspace(&fixture, "[dependencies]\nleft-pad = \"1\"\n");
    fixture.write(
        "agents/Cargo.lock",
        "version = 4\n\n[[package]]\nname = \"left-pad\"\nversion = \"1.0.3\"\n\
         source = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
    );
    let root = fixture.root.to_string_lossy().into_owned();
    let run = xtask(
        &["deps", "--root", &root],
        Some(&fixture.root.join("agents")),
    );
    run.expect_code(1);
    let violations = run.violations();
    for expected in [
        ".cargo/config.toml:4: only an `[alias]` table is allowed, but found `[env]`",
        ".cargo/config.toml:5: a key outside `[alias]`",
        "`tp-agent-mock` declares `left-pad ^1` (normal dependency) from",
        "cannot resolve the workspace of agents/Cargo.toml offline",
        "agents/Cargo.lock:6: `left-pad 1.0.3` comes from \
         `registry+https://github.com/rust-lang/crates.io-index`",
    ] {
        assert!(
            violations.iter().any(|found| found.starts_with(expected)),
            "`{expected}` missing:\n{}",
            run.stdout
        );
    }
    assert_eq!(violations.len(), 5, "{}", run.stdout);
    assert!(
        run.stdout.contains(
            "the build directory CARGO_TARGET_DIR (or CARGO_BUILD_TARGET_DIR) names, agents, is \
             inside the repository"
        ),
        "{}",
        run.stdout
    );
    // CI-like: no `force`, so the variable only takes effect where it is
    // not set already, and the other spelling of it.
    let run = xtask_in(
        &fixture.root,
        &["deps", "--root", &root],
        &[("CARGO_BUILD_TARGET_DIR", Some("agents"))],
    );
    run.expect_code(1);
    assert!(
        run.violations()
            .iter()
            .any(|found| found.starts_with("agents/Cargo.lock:6: ")),
        "{}",
        run.stdout
    );
}

#[test]
fn unsafe_audit_searches_a_build_directory_the_environment_names() {
    // Inside the repository, it is searched like anything else, and noted.
    let fixture = Fixture::passing("audit-env-target");
    fixture.write(
        "build/debug/out.rs",
        "pub fn f() {\n    unsafe { g() }\n}\n",
    );
    let root = fixture.root.to_string_lossy().into_owned();
    let run = xtask(
        &["unsafe-audit", "--root", &root],
        Some(&fixture.root.join("build")),
    );
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        ["build/debug/out.rs:2: `unsafe` is only allowed in tidepool-sys and tidepool-platform"]
    );
    assert!(
        run.stdout
            .contains("names, build, is inside the repository"),
        "{}",
        run.stdout
    );
    // The workspace root's `target` is still skipped, named or not.
    let fixture = Fixture::passing("audit-env-target-root");
    fixture.write(
        "target/debug/out.rs",
        "pub fn f() {\n    unsafe { g() }\n}\n",
    );
    let root = fixture.root.to_string_lossy().into_owned();
    let run = xtask(
        &["unsafe-audit", "--root", &root],
        Some(&fixture.root.join("target")),
    );
    run.expect_code(0);
    assert!(
        !run.stdout.contains("is inside the repository"),
        "{}",
        run.stdout
    );
}

#[test]
fn deps_and_unsafe_audit_search_a_target_directory_beside_a_member() {
    // Probe p3: cargo never builds into a member's `target`, so a workspace
    // there (whose files survive a clone if force-added) is checked.
    let fixture = Fixture::passing("deps-member-target");
    fixture.write(
        "crates/core/target/w/Cargo.toml",
        &manifest(
            "w",
            "0.1.0",
            "[workspace]\n\n[dependencies]\nleft-pad = \"1\"\n",
        ),
    );
    fixture.write("crates/core/target/w/src/lib.rs", "//! W.\n");
    fixture.write(
        "crates/core/target/w/Cargo.lock",
        "version = 4\n\n[[package]]\nname = \"left-pad\"\nversion = \"1.0.3\"\n\
         source = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
    );
    // And the workspace root's own `target` is still build output.
    fixture.write(
        "target/debug/x/Cargo.lock",
        "version = 4\n\n[[package]]\nname = \"left-pad\"\nversion = \"1.0.3\"\n\
         source = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
    );
    let run = fixture.run("deps");
    run.expect_code(1);
    let violations = run.violations();
    assert!(
        violations.contains(
            &"crates/core/target/w/Cargo.lock:6: `left-pad 1.0.3` comes from \
              `registry+https://github.com/rust-lang/crates.io-index`, which is not approved in \
              xtask/deps-allowlist.txt"
        ),
        "{}",
        run.stdout
    );
    assert!(
        violations
            .iter()
            .all(|violation| violation.contains("crates/core/target/w")
                || violation.starts_with("`w` declares")),
        "{}",
        run.stdout
    );
    // The `mod target;` way in: a member's `target/mod.rs` is audited.
    let fixture = Fixture::passing("audit-member-target");
    fixture.write(
        "crates/text/target/mod.rs",
        "#![allow(unsafe_code)]\n// SAFETY: no.\npub fn f() {\n    unsafe { g() }\n}\n",
    );
    let run = fixture.run("unsafe-audit");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            "crates/text/target/mod.rs:1: `allow(unsafe_code)` is only allowed in the crate roots \
             of tidepool-sys and tidepool-platform",
            "crates/text/target/mod.rs:4: `unsafe` is only allowed in tidepool-sys and \
             tidepool-platform",
        ]
    );
}

#[test]
fn deps_fails_on_copied_code_in_the_agents_workspace() {
    // Probe p1: a vendored crate that is a member of `agents/` because a
    // reference agent uses it by path inside that workspace's directory.
    let fixture = Fixture::passing("deps-agents-vendored");
    agents_workspace(
        &fixture,
        "[dependencies]\npad = { package = \"left-pad\", path = \"../vendor/left-pad\" }\n",
    );
    fixture.write(
        "agents/vendor/left-pad/Cargo.toml",
        &manifest("left-pad", "1.0.3", ""),
    );
    fixture.write("agents/vendor/left-pad/src/lib.rs", "//! Pads.\n");
    fixture.lock_workspace("agents/Cargo.toml");
    let run = fixture.run("deps");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            "`left-pad 1.0.3` (agents/vendor/left-pad/Cargo.toml) is a member of agents/, which \
             may hold only the reference agents tp-agent-mock, tp-agent-local, tp-agent-claude, \
             each in agents/<name>/ (docs/subsystems/agents.md): vendored or copied code cannot \
             join it"
        ]
    );

    // Probe p17: a crate elsewhere in the repository listed as a member,
    // with `package.workspace` pointing at `agents/`.
    let fixture = Fixture::passing("deps-agents-listed");
    agents_workspace(&fixture, "");
    fixture.lock_workspace("agents/Cargo.toml");
    passes(&fixture, "deps");
    fixture.write(
        "agents/Cargo.toml",
        "[workspace]\nresolver = \"3\"\nmembers = [\"tp-agent-mock\", \"../tools/left-pad\"]\n",
    );
    fixture.write(
        "tools/left-pad/Cargo.toml",
        "[package]\nname = \"left-pad\"\nversion = \"1.0.3\"\nedition = \"2024\"\n\
         workspace = \"../../agents\"\n",
    );
    fixture.write("tools/left-pad/src/lib.rs", "//! Pads.\n");
    exclude(&fixture, "tools");
    fixture.lock_workspace("agents/Cargo.toml");
    let run = fixture.run("deps");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            "`left-pad 1.0.3` (tools/left-pad/Cargo.toml) is a member of agents/, which may hold \
             only the reference agents tp-agent-mock, tp-agent-local, tp-agent-claude, each in \
             agents/<name>/ (docs/subsystems/agents.md): vendored or copied code cannot join it"
        ]
    );
}

#[test]
fn deps_fails_on_a_compiled_artifact_in_the_repository() {
    // Probe p8: a prebuilt rlib that a build script's
    // `cargo:rustc-link-search` (or `-L` in rustflags) could link in. No
    // manifest or lockfile names it.
    let fixture = Fixture::passing("deps-artifact");
    fixture.write(
        "third_party/prebuilt/libleft_pad.rlib",
        "!<arch>\n/               0           0     0     0       8         `\n",
    );
    // Renamed, it is still known by how it starts.
    fixture.write("third_party/blob.bin", "\u{7f}ELF\u{2}\u{1}\u{1}\0");
    fixture.write("docs/notes.txt", "Not code.\n");
    let run = fixture.run("deps");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            "third_party/blob.bin is a compiled artifact (an ELF object, library or executable by \
             its first bytes), which the repository must not hold: a build script's \
             `cargo:rustc-link-search` could link prebuilt code in without any manifest or \
             lockfile naming it",
            "third_party/prebuilt/libleft_pad.rlib is a compiled artifact (named `.rlib`), which \
             the repository must not hold: a build script's `cargo:rustc-link-search` could link \
             prebuilt code in without any manifest or lockfile naming it",
        ]
    );
}

#[test]
fn deps_fails_on_cargo_configuration_beyond_aliases() {
    // Aliases alone pass.
    let fixture = Fixture::passing("deps-config");
    fixture.write(
        ".cargo/config.toml",
        "[alias]\nxtask = \"run --profile xtask --package xtask --\"\n",
    );
    passes(&fixture, "deps");
    // Probe p8, variant A: `-L` in rustflags links a prebuilt rlib in; and
    // configuration in any directory, since cargo reads every
    // `.cargo/config.toml` above the directory it runs in.
    fixture.append(
        ".cargo/config.toml",
        "\n[build]\nrustflags = [\"-L\", \"third_party/prebuilt\"]\n",
    );
    fixture.write(
        "crates/text/.cargo/config.toml",
        "[source.crates-io]\nreplace-with = \"v\"\n",
    );
    fixture.write(
        "agents/.cargo/config",
        "[env]\nCARGO_TARGET_DIR = \"agents\"\n",
    );
    let run = fixture.run("deps");
    run.expect_code(1);
    let shown: Vec<&str> = run
        .violations()
        .iter()
        .map(|violation| violation.split(", but found").next().unwrap_or(violation))
        .collect();
    assert_eq!(
        shown,
        [
            ".cargo/config.toml:4: only an `[alias]` table is allowed",
            ".cargo/config.toml:5: a key outside `[alias]`",
            "agents/.cargo/config:1: only an `[alias]` table is allowed",
            "agents/.cargo/config:2: a key outside `[alias]`",
            "crates/text/.cargo/config.toml:1: only an `[alias]` table is allowed",
            "crates/text/.cargo/config.toml:2: a key outside `[alias]`",
        ],
        "{}",
        run.stdout
    );
}

#[test]
fn no_check_shows_the_secret_cargo_quotes_from_a_file_it_cannot_parse() {
    // A cargo configuration with a TOML syntax error on a line that holds
    // a (fake) token: cargo's error quotes that line, and every check that
    // shows cargo's error must leave it out (AGENTS.md rule 7), while still
    // failing and keeping the rest of what cargo said.
    let fixture = Fixture::passing("cargo-quoted-secret");
    fixture.write(
        ".cargo/config.toml",
        "[alias]\ntoken = \"FAKETOKENB_unterminated\n",
    );
    for command in ["deps", "layers", "crates", "unsafe-audit"] {
        let run = fixture.run(command);
        run.expect_code(1);
        for (stream, text) in [("stdout", &run.stdout), ("stderr", &run.stderr)] {
            assert!(
                !text.contains("FAKETOKENB"),
                "{command} {stream} shows the token:\n{text}"
            );
        }
        for kept in [
            "TOML parse error at line 2",
            "(cargo's quoted source lines are left out)",
        ] {
            assert!(
                run.stdout.contains(kept),
                "{command}: `{kept}` missing:\n{}",
                run.stdout
            );
        }
    }
}

#[test]
fn deps_fails_on_a_quoted_source_key_in_a_stray_lockfile() {
    // Probe p18: TOML lets the key be quoted, and cargo reads it.
    let fixture = Fixture::passing("deps-quoted-source");
    fixture.write(
        "tools/Cargo.lock",
        "version = 4\n\n[[package]]\nname = \"left-pad\"\nversion = \"1.0.3\"\n\
         \"source\" = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
    );
    let run = fixture.run("deps");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        ["tools/Cargo.lock:6: `left-pad 1.0.3` comes from \
          `registry+https://github.com/rust-lang/crates.io-index`, which is not approved in \
          xtask/deps-allowlist.txt"]
    );
}

#[test]
fn deps_reports_an_outside_path_dependency_even_when_the_lockfile_is_stale() {
    // Probe pA: the lockfile was not updated after the edit, so cargo will
    // not resolve the graph; what the member declares is still checked, and
    // the message does not call it only an out-of-date lockfile.
    let fixture = Fixture::passing("deps-stale-outside");
    fixture.write_outside(
        "outside/evil/Cargo.toml",
        &manifest("evil", "0.1.0", "[workspace]\n"),
    );
    fixture.write_outside("outside/evil/src/lib.rs", "//! Evil.\n");
    fixture.append(
        "crates/core/Cargo.toml",
        "[dependencies]\nevil = { path = \"../../../outside/evil\" }\n",
    );
    let lockfile = fs::read(fixture.root.join("Cargo.lock")).expect("a lockfile");
    let run = fixture.run("deps");
    run.expect_code(1);
    let violations = run.violations();
    assert_eq!(violations.len(), 2, "{}", run.stdout);
    assert_eq!(
        violations[0],
        format!(
            "`tidepool-core` declares `evil *` (normal dependency) from {}, outside the \
             repository: vendored or cross-repository code counts as a dependency",
            fixture.real("outside/evil").display()
        )
    );
    assert!(
        violations[1].starts_with(
            "cannot resolve the workspace of Cargo.toml offline from its lockfile as it is (an \
             external dependency"
        ),
        "{}",
        run.stdout
    );
    assert!(
        !run.stdout.contains("Cargo.lock is out of date"),
        "{}",
        run.stdout
    );
    assert!(
        run.stdout.ends_with("deps: FAILED (2 violations)\n"),
        "{}",
        run.stdout
    );
    assert_eq!(
        fs::read(fixture.root.join("Cargo.lock")).expect("a lockfile"),
        lockfile,
        "deps rewrote the lockfile"
    );
}

#[test]
#[should_panic(expected = "write paths in fixture TOML relative, with `/`")]
fn fixture_toml_with_an_absolute_path_is_refused_on_every_os() {
    // What once broke three tests on Windows only: `D:\a\...` in a TOML
    // basic string is invalid escapes there, but valid on Linux.
    let fixture = Fixture::empty("portable");
    let vendor = fixture.case.join("vendor");
    fixture.write(
        ".cargo/config.toml",
        &format!("[source.vendored]\ndirectory = \"{}\"\n", vendor.display()),
    );
}

#[test]
#[should_panic(expected = "write paths in fixture TOML relative, with `/`")]
fn fixture_toml_with_a_backslash_is_refused() {
    let fixture = Fixture::empty("portable-backslash");
    fixture.write(
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates\\\\core\"]\n",
    );
}

#[test]
#[should_panic(expected = "write paths in fixture TOML relative, with `/`")]
fn fixture_toml_appended_with_an_absolute_path_is_refused() {
    let fixture = Fixture::empty("portable-append");
    let case = fixture.case.display().to_string();
    fixture.append("Cargo.toml", &format!("# {case}/x\n"));
}

#[test]
#[should_panic(expected = "write paths in fixture TOML relative, with `/`")]
fn fixture_toml_outside_the_repository_with_an_absolute_path_is_refused() {
    let fixture = Fixture::empty("portable-outside");
    let case = fixture.case.display().to_string();
    fixture.write_outside("outside/Cargo.toml", &format!("# {case}/x\n"));
}

#[test]
fn fixture_toml_is_refused_with_the_case_path_as_given_or_resolved() {
    // A case directory reached through a link, as `CARGO_TARGET_TMPDIR`
    // is on macOS (`/var` is `/private/var`): either spelling is refused.
    let real = Fixture::empty("portable-link");
    let alias = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("xtask-alias-{}-portable", std::process::id()));
    let _ = fs::remove_file(&alias);
    if let Err(reason) = symlink(&real.case, &alias) {
        skipped(
            "fixture_toml_is_refused_with_the_case_path_as_given_or_resolved",
            &reason,
        );
        return;
    }
    let linked = Fixture {
        root: alias.join("repo"),
        case: alias,
    };
    // On Windows `symlink` makes a file link, which does not resolve to a
    // directory ("Access is denied"), so this case cannot run there.
    let resolved = match fs::canonicalize(&linked.case) {
        Ok(resolved) => resolved,
        Err(error) => {
            skipped(
                "fixture_toml_is_refused_with_the_case_path_as_given_or_resolved",
                &format!("the link to the case directory does not resolve: {error}"),
            );
            return;
        }
    };
    for spelling in [linked.case.clone(), resolved] {
        let contents = format!("# {}/x\n", spelling.display());
        let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            linked.write("Cargo.toml", &contents);
        }));
        assert!(refused.is_err(), "not refused: {contents}");
    }
}

#[test]
fn deps_fails_on_a_malformed_allowlist() {
    let fixture = Fixture::passing("deps-allowlist");
    fixture.append("xtask/deps-allowlist.txt", "left-pad 1.0.3\n");
    let run = fixture.run("deps");
    run.expect_code(1);
    assert_eq!(run.violations().len(), 1, "{}", run.stdout);
    assert!(
        run.violations()[0].ends_with(": expected `name version ADR-NNNN`, found `left-pad 1.0.3`"),
        "{}",
        run.stdout
    );
}

// --- layers ---

#[test]
fn layers_passes_on_the_unbroken_fixture() {
    let fixture = Fixture::passing("layers-pass");
    let run = fixture.run("layers");
    run.expect_code(0);
    assert_eq!(
        run.stdout,
        "workspace crates: 4, matrix rows: 4, workspace edges checked: 3\nlayers: ok\n"
    );
}

#[test]
fn layers_fails_on_a_forbidden_edge_and_names_it() {
    let fixture = Fixture::passing("layers-edge");
    fixture.append(
        "crates/core/Cargo.toml",
        "[dependencies]\ntidepool-text = { path = \"../text\" }\n",
    );
    // `tidepool-text` already depends on `tidepool-core`: cargo would
    // reject the cycle, so make `text` independent first. The fixture then
    // breaks the matrix in exactly one place.
    fixture.write(
        "crates/text/Cargo.toml",
        "[package]\nname = \"tidepool-text\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\
         publish = false\n",
    );
    fixture.lock();

    let run = fixture.run("layers");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            "tidepool-core -> tidepool-text not allowed (normal dependency): the matrix row for \
          `tidepool-core` (docs/01-architecture.md:7) allows no workspace crate"
        ]
    );
    assert!(
        run.stdout.ends_with("layers: FAILED (1 violation)\n"),
        "{}",
        run.stdout
    );
    // Nothing else is wrong with this fixture.
    passes(&fixture, "deps");
    passes(&fixture, "unsafe-audit");
}

#[test]
fn layers_counts_dev_dependencies_and_reads_the_fixture_s_own_matrix() {
    let fixture = Fixture::passing("layers-dev");
    fixture.append(
        "crates/sys/Cargo.toml",
        "[dev-dependencies]\ntidepool-core = { path = \"../core\" }\n",
    );
    fixture.lock();
    let run = fixture.run("layers");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            "tidepool-sys -> tidepool-core not allowed (dev-dependency): the matrix row for \
          `tidepool-sys` (docs/01-architecture.md:8) allows no workspace crate"
        ]
    );

    // The document is the source of truth: allow the edge there, and the
    // same workspace passes.
    fixture.write(
        "docs/01-architecture.md",
        &MATRIX.replace("| `tidepool-sys` | — |", "| `tidepool-sys` | `core` |"),
    );
    passes(&fixture, "layers");
}

#[test]
fn layers_fails_on_a_member_without_a_row() {
    let fixture = Fixture::passing("layers-row");
    fixture.write(
        "docs/01-architecture.md",
        &MATRIX.replace("| `tidepool-text` | `core` |\n", ""),
    );
    let run = fixture.run("layers");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            "workspace member `tidepool-text` has no row in the dependency matrix \
          (docs/01-architecture.md)"
        ]
    );
}

#[test]
fn layers_fails_on_a_matrix_a_reader_does_not_see() {
    // Probes pM and pN: the reader sees the strict matrix, written without
    // outer pipes, and the parser would read a permissive copy hidden in a
    // processing instruction, or after a code block whose closing fence is
    // indented four spaces (so the block goes on). The fixture uses the
    // edge only the hidden copy allows.
    let table_start = MATRIX.find("| Crate |").expect("the matrix has a header");
    let table_end = MATRIX.find("\n\n## Workspace layout").expect("and an end");
    let table = &MATRIX[table_start..table_end];
    let shown: String = table
        .lines()
        .map(|line| {
            let line = line.strip_prefix("| ").unwrap_or(line);
            format!("{}\n", line.strip_suffix(" |").unwrap_or(line))
        })
        .collect();
    let permissive = table.replace("| `tidepool-sys` | — |", "| `tidepool-sys` | `core` |");
    for (name, hidden, line) in [
        ("pM", format!("<?hidden\n{permissive}\n?>"), 12),
        (
            "pN",
            format!("```text\nnotes\n    ```\n{permissive}\n```"),
            15,
        ),
    ] {
        let fixture = Fixture::passing("layers-hidden");
        fixture.append(
            "crates/sys/Cargo.toml",
            "[dev-dependencies]\ntidepool-core = { path = \"../core\" }\n",
        );
        fixture.lock();
        fixture.write(
            "docs/01-architecture.md",
            &MATRIX.replace(table, &format!("{shown}\n{hidden}")),
        );
        let run = fixture.run("layers");
        run.expect_code(1);
        assert!(
            run.stdout
                .starts_with(&format!("layers: FAILED: docs/01-architecture.md:{line}: ")),
            "{name}:\n{}",
            run.stdout
        );
    }
}

// --- crates ---

#[test]
fn crates_passes_on_the_unbroken_fixture() {
    let fixture = Fixture::passing("crates-pass");
    let run = fixture.run("crates");
    run.expect_code(0);
    assert_eq!(
        run.stdout,
        "members checked: 4, task IDs in docs/roadmap.md: 2\ncrates: ok\n"
    );
}

#[test]
fn crates_fails_on_a_manifest_that_does_not_inherit_or_may_be_published() {
    // Manifests cargo accepts, each breaking rules of `crates` only.
    let fixture = Fixture::passing("crates-manifest");
    // A literal edition and version, no license and no `[lints]`.
    fixture.write(
        "crates/core/Cargo.toml",
        &manifest("tidepool-core", "0.1.0", ""),
    );
    // A literal license, `publish = true`, and lints of its own.
    fixture.write(
        "crates/sys/Cargo.toml",
        &member_manifest("tidepool-sys", "")
            .replace("license.workspace = true", "license = \"Apache-2.0\"")
            .replace("publish = false", "publish = true")
            .replace(
                "[lints]\nworkspace = true\n",
                "[lints.rust]\nunsafe_code = \"deny\"\n",
            ),
    );
    let run = fixture.run("crates");
    run.expect_code(1);
    let core = "crates/core/Cargo.toml";
    let sys = "crates/sys/Cargo.toml";
    assert_eq!(
        run.violations(),
        [
            format!(
                "{core} does not inherit `package.edition` from the workspace: write \
                 `edition.workspace = true` in `[package]`, and nothing else for `edition`"
            ),
            format!(
                "{core} does not inherit `package.version` from the workspace: write \
                 `version.workspace = true` in `[package]`, and nothing else for `version`"
            ),
            format!(
                "{core} has no `package.license`: write `license.workspace = true` in \
                 `[package]`, to inherit it from the workspace"
            ),
            format!(
                "{core} has no `[lints]` table: write `[lints]` with `workspace = true` and \
                 nothing else in it"
            ),
            format!(
                "{sys} does not inherit `package.license` from the workspace: write \
                 `license.workspace = true` in `[package]`, and nothing else for `license`"
            ),
            format!(
                "{sys} sets `package.publish` to something other than `false`: write `publish = \
                 false` in `[package]`, since no member is published"
            ),
            format!(
                "{sys} does not take its lints only from the workspace: write `[lints]` with \
                 `workspace = true` and nothing else in it"
            ),
        ],
        "{}",
        run.stdout
    );
    assert!(
        run.stdout.ends_with("crates: FAILED (7 violations)\n"),
        "{}",
        run.stdout
    );
}

#[test]
fn crates_fails_on_a_target_with_an_edition_of_its_own() {
    // Cargo builds this, with a warning, and compiles the library under
    // 2021 whatever the package inherits.
    let fixture = Fixture::passing("crates-target-edition");
    fixture.append("crates/core/Cargo.toml", "[lib]\nedition = \"2021\"\n");
    let run = fixture.run("crates");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            "crates/core/Cargo.toml: the `[lib]` target sets an `edition` of its own: remove it, \
             so that the target uses the package's edition, which it inherits from the workspace"
        ]
    );
    // The control: cargo does accept it.
    let checked = Command::new(cargo())
        .args([
            "metadata",
            "--offline",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(fixture.root.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", fixture.case.join("target"))
        .output()
        .expect("cargo runs");
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
}

#[test]
fn crates_fails_on_a_root_edition_other_than_2024() {
    let fixture = Fixture::passing("crates-edition");
    let text = fs::read_to_string(fixture.root.join("Cargo.toml")).expect("the root manifest");
    fixture.write(
        "Cargo.toml",
        &text.replace("edition = \"2024\"", "edition = \"2021\""),
    );
    let run = fixture.run("crates");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            "Cargo.toml sets `workspace.package.edition` to something other than \"2024\": write \
             `edition = \"2024\"` in `[workspace.package]`, the edition every member inherits"
        ]
    );
}

#[test]
fn crates_fails_on_a_member_without_a_readme() {
    let fixture = Fixture::passing("crates-no-readme");
    fs::remove_file(fixture.root.join("crates/sys/README.md")).expect("the README exists");
    let run = fixture.run("crates");
    run.expect_code(1);
    assert_eq!(run.violations(), ["crates/sys/README.md is missing"]);
    // `readme.md` is not `README.md`, where case matters.
    fixture.write("crates/sys/readme.md", &readme_of("tidepool-sys"));
    if fixture.root.join("crates/sys/README.md").exists() {
        skipped(
            "crates_fails_on_a_member_without_a_readme (readme.md)",
            "this file system ignores case, so `readme.md` opens as `README.md`",
        );
        return;
    }
    let run = fixture.run("crates");
    run.expect_code(1);
    assert_eq!(run.violations(), ["crates/sys/README.md is missing"]);
}

#[test]
fn crates_reports_a_linked_readme_without_reading_it() {
    let fixture = Fixture::passing("crates-linked");
    // The link leads to a README in the shape, so only the link fails.
    fixture.write_outside(
        "elsewhere.md",
        &format!(
            "{}\nThis line must never be printed.\n",
            readme_of("tidepool-text")
        ),
    );
    let link = fixture.root.join("crates/text/README.md");
    fs::remove_file(&link).expect("the README exists");
    if let Err(reason) = symlink(&fixture.real("elsewhere.md"), &link) {
        skipped("crates_reports_a_linked_readme_without_reading_it", &reason);
        return;
    }
    let run = fixture.run("crates");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            "crates/text/README.md cannot be read: crates/text/README.md is a symbolic link, \
             which xtask does not follow"
        ]
    );
    let output = format!("{}{}", run.stdout, run.stderr);
    assert!(!output.contains("must never be printed"), "{output}");
}

#[test]
fn crates_fails_on_a_readme_whose_first_line_is_not_the_package_name() {
    let fixture = Fixture::passing("crates-heading");
    fixture.write(
        "crates/platform/README.md",
        &readme_of("tidepool-platform").replacen(
            "# tidepool-platform",
            "# Platform, a heading from the file",
            1,
        ),
    );
    let run = fixture.run("crates");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            "crates/platform/README.md: its first line is not `# tidepool-platform`, the package \
             name as a heading"
        ]
    );
    // The line itself is never quoted.
    assert!(
        !run.stdout.contains("a heading from the file"),
        "{}",
        run.stdout
    );
}

#[test]
fn crates_fails_on_a_readme_without_a_section_outside_code() {
    let fixture = Fixture::passing("crates-section");
    // `## Documentation` only inside a fenced code block, and `## Statuses`
    // for `## Status`.
    fixture.write(
        "crates/core/README.md",
        &readme_of("tidepool-core")
            .replace("## Documentation\n", "```markdown\n## Documentation\n```\n")
            .replace("## Status\n", "## Statuses\n"),
    );
    let run = fixture.run("crates");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            "crates/core/README.md has no `## Status` line outside code blocks, HTML blocks and \
             comments",
            "crates/core/README.md has no `## Documentation` line outside code blocks, HTML \
             blocks and comments",
        ]
    );
    // A fence opener inside an HTML comment opens no code block, so the
    // sections after it count; a section only inside a comment does not.
    fixture.write(
        "crates/core/README.md",
        &readme_of("tidepool-core").replace("## Status\n", "<!--\n```\n-->\n\n## Status\n"),
    );
    passes(&fixture, "crates");
    fixture.write(
        "crates/core/README.md",
        &readme_of("tidepool-core").replace("## Status\n", "<!--\n## Status\n-->\n"),
    );
    let run = fixture.run("crates");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            "crates/core/README.md has no `## Status` line outside code blocks, HTML blocks and \
             comments"
        ]
    );
}

#[test]
fn crates_fails_on_a_section_twice_out_of_order_or_features_out_of_place() {
    let fixture = Fixture::passing("crates-order");
    // `## Rules` twice.
    fixture.append("crates/sys/README.md", "\n## Rules\n\nAgain.\n");
    // `## Documentation` before `## Usage`.
    fixture.write(
        "crates/platform/README.md",
        &readme_of("tidepool-platform")
            .replace("## Usage\n", "## Swap\n")
            .replace("## Documentation\n", "## Usage\n")
            .replace("## Swap\n", "## Documentation\n"),
    );
    // `## Features` after `## Documentation`.
    fixture.write(
        "crates/text/README.md",
        &readme_of("tidepool-text").replace("## Rules\n", "## Features\n\nNone.\n\n## Rules\n"),
    );
    let run = fixture.run("crates");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            "crates/platform/README.md: `## Usage` must come before `## Documentation`, but comes \
             after it",
            "crates/sys/README.md has more than one `## Rules` line outside code blocks, HTML \
             blocks and comments: a README has each section once",
            "crates/text/README.md: `## Features` may stand only between `## Usage` and `## \
             Documentation`",
        ]
    );
    assert!(
        run.stdout.ends_with("crates: FAILED (3 violations)\n"),
        "{}",
        run.stdout
    );
    // `## Features` in its place passes, and so do other headings anywhere.
    fixture.write(
        "crates/text/README.md",
        &readme_of("tidepool-text").replace(
            "## Documentation\n",
            "## Features\n\nNone.\n\n## Documentation\n",
        ),
    );
    fixture.write(
        "crates/platform/README.md",
        &readme_of("tidepool-platform").replace("## Rules\n", "## Design\n\nNone.\n\n## Rules\n"),
    );
    fixture.write("crates/sys/README.md", &readme_of("tidepool-sys"));
    passes(&fixture, "crates");
}

#[test]
fn crates_fails_on_a_missing_differing_or_linked_license_copy() {
    let fixture = Fixture::passing("crates-copies");
    fs::remove_file(fixture.root.join("crates/core/LICENSE")).expect("the copy exists");
    fixture.write(
        "crates/sys/NOTICE",
        &format!("{NOTICE}This line must never be printed.\n"),
    );
    let run = fixture.run("crates");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            "crates/core/LICENSE is missing",
            "crates/sys/NOTICE is not byte for byte the root's NOTICE: copy the root's NOTICE over \
             it",
        ]
    );
    let output = format!("{}{}", run.stdout, run.stderr);
    assert!(!output.contains("must never be printed"), "{output}");
    assert!(!output.contains("Joseph Anthony Abbott III"), "{output}");

    // A linked copy, to the root's own bytes, is named and not read.
    fixture.write("crates/core/LICENSE", LICENSE);
    fixture.write("crates/sys/NOTICE", NOTICE);
    passes(&fixture, "crates");
    fixture.write_outside("elsewhere/LICENSE", LICENSE);
    let link = fixture.root.join("crates/text/LICENSE");
    fs::remove_file(&link).expect("the copy exists");
    if let Err(reason) = symlink(&fixture.real("elsewhere/LICENSE"), &link) {
        skipped(
            "crates_fails_on_a_missing_differing_or_linked_license_copy (link)",
            &reason,
        );
        return;
    }
    let run = fixture.run("crates");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            "crates/text/LICENSE cannot be read: crates/text/LICENSE is a symbolic link, which \
             xtask does not follow"
        ]
    );
}

#[test]
fn crates_fails_on_a_workspace_without_a_notice_at_its_root() {
    let fixture = Fixture::passing("crates-root-notice");
    fs::remove_file(fixture.root.join("NOTICE")).expect("the root has a NOTICE");
    let run = fixture.run("crates");
    run.expect_code(1);
    // One violation: the members' copies have nothing to be compared with.
    assert_eq!(
        run.violations(),
        ["the workspace has no NOTICE at its root"]
    );
}

#[test]
fn crates_fails_on_a_task_id_the_roadmap_does_not_have() {
    let fixture = Fixture::passing("crates-task-id");
    // M0.3 twice, and M0.12, which is not M0.1 followed by a digit;
    // `xM0.4` names nothing, and M0.2 is in the roadmap.
    fixture.append(
        "crates/text/README.md",
        "\nSee M0.3, M0.12, M0.3 again and xM0.4; M0.2 is a task.\n",
    );
    let run = fixture.run("crates");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            "crates/text/README.md names the task M0.3, which docs/roadmap.md does not have",
            "crates/text/README.md names the task M0.12, which docs/roadmap.md does not have",
        ]
    );
    // Once the roadmap has them, it passes.
    fixture.append(
        "docs/roadmap.md",
        "- [ ] **M0.3 — fixture: a third task**\n- [ ] **M0.12 — fixture: and a twelfth**\n",
    );
    let run = fixture.run("crates");
    run.expect_code(0);
    assert!(
        run.stdout
            .starts_with("members checked: 4, task IDs in docs/roadmap.md: 4\n"),
        "{}",
        run.stdout
    );
}

#[test]
fn crates_cannot_run_without_a_roadmap_that_has_tasks() {
    let fixture = Fixture::passing("crates-roadmap");
    fs::remove_file(fixture.root.join("docs/roadmap.md")).expect("the roadmap exists");
    let run = fixture.run("crates");
    run.expect_code(1);
    // One line, the reason: nothing was checked.
    assert_eq!(run.stdout.lines().count(), 1, "{}", run.stdout);
    assert!(
        run.stdout.starts_with("crates: FAILED: cannot read `")
            && run.stdout.contains("docs/roadmap.md`: "),
        "{}",
        run.stdout
    );

    fixture.write(
        "docs/roadmap.md",
        "# Roadmap\n\nM0.1 is named, but no line is a task.\n",
    );
    let run = fixture.run("crates");
    run.expect_code(1);
    assert_eq!(
        run.stdout,
        "crates: FAILED: docs/roadmap.md: no line starts `- [ ] **M<n>.<m> — ` or `- [x] \
         **M<n>.<m> — `, so there is no task for a README to name; is the root right?\n"
    );
}

// --- unsafe-audit ---

#[test]
fn unsafe_audit_passes_on_the_unbroken_fixture() {
    let fixture = Fixture::passing("audit-pass");
    let run = fixture.run("unsafe-audit");
    run.expect_code(0);
    assert!(
        run.stdout
            .contains("Rust files scanned: 4\ncrate roots checked: 4\n"),
        "{}",
        run.stdout
    );
}

#[test]
fn unsafe_audit_fails_on_each_kind_of_violation() {
    let fixture = Fixture::passing("audit-fail");
    // `unsafe` outside sys and platform, even with a SAFETY comment.
    fixture.write(
        "crates/text/src/raw.rs",
        "pub fn first(p: *const u8) -> u8 {\n    // SAFETY: not allowed here anyway.\n    \
         unsafe { *p }\n}\n",
    );
    // A block in platform without `// SAFETY:`.
    fixture.write(
        "crates/platform/src/read.rs",
        "pub fn read(p: *const u8) -> u8 {\n    unsafe { *p }\n}\n",
    );
    // The lint relaxed outside the two crate roots.
    fixture.write(
        "crates/core/src/lib.rs",
        "//! Core.\n#![allow(unsafe_code)]\n#![doc(test(attr(forbid(unsafe_code))))]\n",
    );
    // A `pub unsafe fn` whose docs have no `# Safety` section.
    fixture.write(
        "crates/platform/src/ffi.rs",
        "/// Reads `p`.\npub unsafe fn peek(p: *const u8) -> u8 {\n    \
         // SAFETY: the caller promises `p` is valid.\n    unsafe { *p }\n}\n",
    );
    // An `extern` block outside sys.
    fixture.write(
        "crates/platform/src/decl.rs",
        "unsafe extern \"C\" {\n    pub fn getpid() -> i32;\n}\n",
    );

    let run = fixture.run("unsafe-audit");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            "crates/core/src/lib.rs:2: `allow(unsafe_code)` is only allowed in the crate roots \
             of tidepool-sys and tidepool-platform",
            "crates/platform/src/decl.rs:1: `extern` block outside tidepool-sys: raw OS \
             declarations live only there",
            "crates/platform/src/ffi.rs:2: `pub unsafe fn` has no `# Safety` section in its doc \
             comment",
            "crates/platform/src/read.rs:2: `unsafe` block has no `// SAFETY:` comment before it \
             in the same statement",
            "crates/text/src/raw.rs:3: `unsafe` is only allowed in tidepool-sys and \
             tidepool-platform",
        ]
    );
    assert!(
        run.stdout
            .ends_with("unsafe-audit: FAILED (5 violations)\n"),
        "{}",
        run.stdout
    );
}

#[test]
fn unsafe_audit_ignores_keywords_inside_comments_and_literals() {
    let fixture = Fixture::passing("audit-decoys");
    fixture.write(
        "crates/text/src/decoys.rs",
        "//! Never write `unsafe { }` or `#![allow(unsafe_code)]` here.\n\
         // unsafe { *p }\n\
         /* unsafe impl Send for X {}\n   /* nested: unsafe { } */ extern \"C\" { } */\n\
         /// Prose may say unsafe, and inline code `unsafe { f() }`.\n\
         pub const HELP: &str = \"unsafe { } and #![allow(unsafe_code)] are denied\";\n\
         pub const RAW: &str = r#\"unsafe extern \"C\" { fn f(); } \"#;\n\
         pub const BYTES: &[u8] = b\"unsafe { }\";\n\
         pub const QUOTE: char = '\"';\n\
         pub const AFTER: &str = \"unsafe { } after a quote character\";\n\
         pub fn is_unsafe(unsafe_code: bool) -> bool {\n    unsafe_code\n}\n\
         pub fn label<'unsafe_ok>(x: &'unsafe_ok str) -> &'unsafe_ok str {\n    x\n}\n",
    );
    passes(&fixture, "unsafe-audit");
}

#[test]
fn unsafe_audit_fails_on_code_brought_in_from_files_it_does_not_read() {
    // Each of these compiles into `tidepool-text` with `unsafe` in it, and
    // clippy accepts it, because the file relaxes the lint itself.
    let smuggled = "#![allow(unsafe_code)]\n\n/// Reads.\npub fn read(p: *const u8) -> u8 {\n    \
                    // SAFETY: no.\n    unsafe { *p }\n}\n";
    let fixture = Fixture::passing("audit-smuggle");
    fixture.write("crates/text/src/ffi.txt", smuggled);
    fixture.write("crates/text/.gen/m.rs", smuggled);
    fixture.write("crates/text/src/inc.txt", "pub fn f() {}\n");
    fixture.write(
        "crates/text/src/lib.rs",
        "//! Text.\n#![doc(test(attr(forbid(unsafe_code))))]\n#[path = \"ffi.txt\"]\nmod ffi;\n\
         #[path = \"../.gen/m.rs\"]\nmod m;\ninclude!(\"inc.txt\");\n",
    );
    // A crate root that is not a `.rs` file.
    fixture.write("crates/core/src/lib.txt", smuggled);
    fixture.append("crates/core/Cargo.toml", "[lib]\npath = \"src/lib.txt\"\n");
    let run = fixture.run("unsafe-audit");
    run.expect_code(1);
    let unreadable = |line: usize, what: &str| {
        format!(
            "crates/text/src/lib.rs:{line}: {what} does not name a relative `.rs` path inside \
             this file's directory (no `..`, no hidden or `target` directory, no escapes), so the \
             code it brings in may not be audited"
        )
    };
    assert_eq!(
        run.violations(),
        [
            "crates/text/.gen/m.rs:1: `allow(unsafe_code)` is only allowed in the crate roots of \
             tidepool-sys and tidepool-platform"
                .to_string(),
            "crates/text/.gen/m.rs:6: `unsafe` is only allowed in tidepool-sys and \
             tidepool-platform"
                .to_string(),
            unreadable(3, "`#[path = ...]`"),
            unreadable(5, "`#[path = ...]`"),
            unreadable(7, "`include!`"),
            "crates/core/src/lib.txt: the lib target `tidepool_core` of `tidepool-core` starts \
             from a file the audit did not read; a crate root must be a `.rs` file in the \
             repository, outside build output and not behind a symbolic link"
                .to_string(),
        ]
    );

    // A module file behind a symbolic link.
    let fixture = Fixture::passing("audit-link");
    fixture.write_outside("outside/s.rs", smuggled);
    if let Err(reason) = symlink(
        &fixture.real("outside/s.rs"),
        &fixture.root.join("crates/text/src/s.rs"),
    ) {
        skipped(
            "unsafe_audit_fails_on_code_brought_in_from_files_it_does_not_read (link)",
            &reason,
        );
        return;
    }
    fixture.append("crates/text/src/lib.rs", "mod s;\n");
    let run = fixture.run("unsafe-audit");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            "crates/text/src/s.rs: a symbolic link, which the audit does not follow: the code it \
          leads to was not audited"
        ]
    );
}

/// The violation for an `unsafe` of the form `what` on `line` of `path`,
/// in the exported macro `name`, which takes tokens from its caller.
fn exported_wrapper(path: &str, line: usize, what: &str, name: &str) -> String {
    format!(
        "{path}:{line}: `unsafe` {what} in the exported macro `{name}`, which takes tokens from \
         its caller (`$...`): they could run as `unsafe` in the caller's crate, where neither its \
         `unsafe_code` lint nor this audit sees them, and no `// SAFETY:` comment here can cover \
         them. Make it a function, or leave the `unsafe` to the caller"
    )
}

/// The line of `fixture`'s file `path` that holds `needle`, counted from 1.
fn line_of(fixture: &Fixture, path: &str, needle: &str) -> usize {
    fs::read_to_string(fixture.root.join(path))
        .expect("the file exists")
        .lines()
        .position(|line| line.contains(needle))
        .expect("the needle is in the file")
        + 1
}

#[test]
fn unsafe_audit_fails_on_an_exported_macro_that_wraps_caller_code_in_unsafe() {
    // Each of these, called from a crate that denies `unsafe_code`, runs
    // the caller's tokens as `unsafe` there with no `unsafe` token in that
    // crate: in a block, as the body of an `unsafe fn`, under
    // `#[unsafe(no_mangle)]`, or as declarations in an `unsafe extern` block
    // (the last one sys's, re-exported by platform).
    let fixture = Fixture::passing("audit-macro");
    fixture.append(
        "crates/platform/src/lib.rs",
        "\npub use tidepool_sys::declare_c;\n\n/// Runs `$e` as unsafe.\n#[macro_export]\n\
         macro_rules! in_unsafe {\n    ($e:expr) => {{\n        // SAFETY: delegated to the caller.\n        \
         unsafe { $e }\n    }};\n}\n\n/// Runs its tokens as the body of an `unsafe fn`, then calls \
         it.\n#[macro_export]\nmacro_rules! in_unsafe_fn {\n    ($($t:tt)*) => {{\n        \
         unsafe fn __body() {\n            $($t)*\n        }\n        // SAFETY: probe.\n        \
         unsafe { __body() }\n    }};\n}\n\n/// Gives the item its caller passes an unmangled \
         symbol name.\n#[macro_export]\nmacro_rules! unmangled {\n    ($i:item) => {\n        \
         #[unsafe(no_mangle)]\n        $i\n    };\n}\n",
    );
    fixture.append(
        "crates/sys/src/lib.rs",
        "\n/// Declares the C functions its caller lists.\n#[macro_export]\nmacro_rules! declare_c {\n    \
         ($($t:tt)*) => {\n        unsafe extern \"C\" {\n            $($t)*\n        }\n    };\n}\n",
    );
    let platform = "crates/platform/src/lib.rs";
    let sys = "crates/sys/src/lib.rs";
    let at = |path: &str, needle: &str| line_of(&fixture, path, needle);
    let run = fixture.run("unsafe-audit");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            exported_wrapper(
                platform,
                at(platform, "unsafe { $e }"),
                "block",
                "in_unsafe"
            ),
            exported_wrapper(
                platform,
                at(platform, "unsafe fn __body"),
                "fn",
                "in_unsafe_fn"
            ),
            exported_wrapper(
                platform,
                at(platform, "unsafe { __body() }"),
                "block",
                "in_unsafe_fn"
            ),
            exported_wrapper(
                platform,
                at(platform, "#[unsafe(no_mangle)]"),
                "attribute",
                "unmangled"
            ),
            exported_wrapper(
                sys,
                at(sys, "unsafe extern \"C\""),
                "`extern` block",
                "declare_c"
            ),
        ]
    );
}

#[test]
fn unsafe_audit_fails_on_unsafe_in_a_doc_test_that_another_crate_s_macro_wraps() {
    // `k1!(unsafe, *p)` puts the doc test's own `unsafe` before braces that
    // core's macro writes, whose span makes rustc skip the `unsafe_code`
    // lint, even at `forbid` (ADR-0037). The macro is banned at its source
    // (ADR-0038), and the doc reader, a second layer, sees the doc test.
    let fixture = Fixture::passing("audit-doc-test-macro");
    let core = "crates/core/src/lib.rs";
    fixture.append(
        core,
        "\n/// Puts its first token before a block of the rest.\n#[macro_export]\nmacro_rules! k1 {\n    \
         ($k:tt, $($t:tt)*) => {\n        $k { $($t)* }\n    };\n}\n\n/// Probe.\n///\n/// ```\n/// \
         let x = 1; let p = &raw const x; assert_eq!(tidepool_core::k1!(unsafe, *p), 1);\n/// ```\n\
         pub fn probe_k1() {}\n",
    );
    let run = fixture.run("unsafe-audit");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            format!(
                "{core}:{}: the exported macro `k1` puts `$k`, a `tt` fragment its caller can make \
                 the keyword `unsafe`, right before `{{`, which it writes itself: rustc does not \
                 apply the caller's `unsafe_code` lint to braces or an item from another crate's \
                 macro, in doc tests or in code. Use a fragment that cannot be `unsafe` (`path`, \
                 `ty`, `vis`), or leave the `unsafe` to the caller (ADR-0038)",
                line_of(&fixture, core, "$k { $($t)* }")
            ),
            format!(
                "{core}:{}: a code block in the documentation, which rustdoc may compile as a doc \
                 test, holds `unsafe`, which only tidepool-sys and tidepool-platform may use: every \
                 block counts, `ignore` and `text` too, since the `unsafe_code` lint can miss \
                 `unsafe` that another crate's macro expands. Inline code in prose is fine \
                 (ADR-0037)",
                line_of(&fixture, core, "k1!(unsafe")
            ),
        ]
    );
}

#[test]
fn unsafe_audit_fails_on_a_macro_that_defines_a_macro() {
    // A private generator in platform that defines an exported wrapper of
    // its caller's tokens through the `$` token trick: no rule can read the
    // macro it makes (ADR-0038).
    let fixture = Fixture::passing("audit-macro-generator");
    let platform = "crates/platform/src/lib.rs";
    fixture.append(
        platform,
        "\n#[rustfmt::skip]\nmacro_rules! define_wrap {\n    ($d:tt, $name:ident) => {\n        \
         /// Wraps caller tokens in an unsafe block.\n        #[macro_export]\n        \
         macro_rules! $name {\n            ($d($d t:tt)*) => {{\n                // SAFETY: probe.\n                \
         unsafe { $d($d t)* }\n            }};\n        }\n    };\n}\ndefine_wrap!($, wrap_gen);\n",
    );
    let run = fixture.run("unsafe-audit");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [format!(
            "{platform}:{}: the macro `define_wrap` defines a macro (its body holds \
             `macro_rules`): what it makes is never written out, so no rule here can read it \
             (ADR-0038)",
            line_of(&fixture, platform, "macro_rules! $name")
        )]
    );
}

#[test]
fn unsafe_audit_fails_on_a_proc_macro_crate() {
    // A proc macro can make `unsafe` from a string with a span for which
    // rustc skips the lint (ADR-0037). The crate root carries the doc-test
    // line, so the ban is its only violation.
    let fixture = Fixture::passing("audit-proc-macro");
    fixture.append("crates/text/Cargo.toml", "\n[lib]\nproc-macro = true\n");
    let run = fixture.run("unsafe-audit");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            "crates/text/src/lib.rs: the proc-macro target `tidepool_text` of `tidepool-text` is a \
             proc-macro crate, which no workspace under the repository may have, tidepool-sys and \
             tidepool-platform included: a proc macro can make the `unsafe` keyword from a string, \
             with a span for which rustc skips the `unsafe_code` lint, and this audit reads only \
             the string (ADR-0037)"
        ]
    );
}

#[test]
fn unsafe_audit_fails_on_a_worktree_inside_a_package() {
    // A registered worktree in `crates/text/src/evil`, which `pub mod evil;`
    // compiles into tidepool-text: the walk skips it, so the audit must not
    // pass.
    let fixture = Fixture::passing("audit-worktree-in-package");
    write(&fixture.root.join(".git/HEAD"), "ref: refs/heads/main\n");
    let evil = "crates/text/src/evil";
    fixture.write(
        &format!("{evil}/mod.rs"),
        "#![allow(unsafe_code)]\n// SAFETY: no.\npub fn f() {\n    unsafe { g() }\n}\n",
    );
    write(
        &fixture.root.join(evil).join(".git"),
        "gitdir: ../../../../.git/worktrees/x\n",
    );
    let real = fs::canonicalize(fixture.root.join(evil)).expect("the worktree exists");
    write(
        &fixture.root.join(".git/worktrees/x/gitdir"),
        &format!("{}\n", real.join(".git").display()),
    );
    fixture.append("crates/text/src/lib.rs", "\npub mod evil;\n");
    let run = fixture.run("unsafe-audit");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            "crates/text/src/evil: a linked worktree inside the package directory of \
             `tidepool-text`, which the audit skipped: a `mod` declaration of that crate could \
             compile code from it. Move the worktree out of every package (as in \
             `.claude/worktrees/`)"
        ]
    );
    assert!(
        run.stdout.contains(
            "skipped linked worktrees of this repository (run xtask in each to check it): \
             crates/text/src/evil\n"
        ),
        "{}",
        run.stdout
    );
}

#[test]
fn unsafe_audit_fails_on_a_crate_root_outside_its_package() {
    // `[lib] path = "../../lib.rs"` makes the repository root's `lib.rs`
    // the crate root of tidepool-text, and a plain `mod target;` there
    // compiles the root's `target/mod.rs`, which the walk skips as build
    // output. The root itself is a file the audit read.
    let fixture = Fixture::passing("audit-root-outside");
    let smuggled = "#![allow(unsafe_code)]\n\n/// Reads.\npub fn read(p: *const u8) -> u8 {\n    \
                    // SAFETY: no.\n    unsafe { *p }\n}\n";
    fixture.write(
        "lib.rs",
        "//! Text, from the root.\n#![doc(test(attr(forbid(unsafe_code))))]\n\npub mod target;\n",
    );
    fixture.write("target/mod.rs", smuggled);
    fixture.append(
        "crates/text/Cargo.toml",
        "\n[lib]\npath = \"../../lib.rs\"\n",
    );
    let run = fixture.run("unsafe-audit");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            "lib.rs: the lib target `tidepool_text` of `tidepool-text` starts from a file outside \
             its package directory crates/text: a crate root must lie in the directory of its \
             package's Cargo.toml, or a plain `mod` declaration in it could compile files the \
             audit skips (a workspace root's `target/`, a linked worktree)"
        ]
    );
    assert!(
        run.stdout.contains("Rust files scanned: 5\n"),
        "{}",
        run.stdout
    );

    // The control: cargo does compile the skipped file into tidepool-text.
    fixture.write(
        "target/mod.rs",
        "compile_error!(\"target/mod.rs is compiled\");\n",
    );
    let built = Command::new(cargo())
        .args(["check", "--offline", "--quiet", "--manifest-path"])
        .arg(fixture.root.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", fixture.case.join("target"))
        .output()
        .expect("cargo runs");
    assert!(!built.status.success(), "the control build passed");
    assert!(
        String::from_utf8_lossy(&built.stderr).contains("target/mod.rs is compiled"),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
}

/// The violation for the library target `name` of the package `package`,
/// whose crate root `src` does not forbid `unsafe` in its doc tests
/// (ADR-0036).
fn lets_doc_tests_use_unsafe(src: &str, name: &str, package: &str) -> String {
    format!(
        "{src}: the lib target `{name}` of `{package}` does not forbid `unsafe` in its doc tests, \
         and rustdoc does not apply the workspace's `unsafe_code` lint to them. Add this line to \
         its crate root after the `//!` doc comment, exactly as written (an inner attribute at \
         crate level, not under `cfg_attr`, with nothing else in it): \
         `#![doc(test(attr(forbid(unsafe_code))))]` (ADR-0036)"
    )
}

#[test]
fn unsafe_audit_fails_on_a_library_crate_root_that_does_not_forbid_unsafe_in_doc_tests() {
    // Each of these seems to forbid `unsafe` in the crate's doc tests, and
    // none is the one form the audit accepts: it counts them all as missing.
    let fixture = Fixture::passing("audit-doc-tests");
    let hello = "/// Says hello.\npub fn hello() -> &'static str {\n    \"hello\"\n}\n";
    let in_root = |attribute: &str| format!("//! Text.\n{attribute}\n\n{hello}");
    for (what, lib) in [
        ("missing", format!("//! Text.\n\n{hello}")),
        (
            "under `cfg_attr`",
            in_root("#![cfg_attr(all(), doc(test(attr(forbid(unsafe_code)))))]"),
        ),
        (
            "with another lint in the same `attr`",
            in_root("#![doc(test(attr(forbid(unsafe_code, missing_docs))))]"),
        ),
        (
            "`deny`, which a doc test could relax",
            in_root("#![doc(test(attr(deny(unsafe_code))))]"),
        ),
        (
            "an outer attribute, of one function",
            "//! Text.\n\n/// Says hello.\n#[doc(test(attr(forbid(unsafe_code))))]\n\
             pub fn hello() -> &'static str {\n    \"hello\"\n}\n"
                .to_string(),
        ),
        (
            "inside a nested `mod`",
            "//! Text.\n\n/// Greetings.\npub mod greet {\n    \
             #![doc(test(attr(forbid(unsafe_code))))]\n\n    /// Says hello.\n    \
             pub fn hello() -> &'static str {\n        \"hello\"\n    }\n}\n"
                .to_string(),
        ),
    ] {
        fixture.source("crates/text/src/lib.rs", &lib);
        let run = fixture.run("unsafe-audit");
        run.expect_code(1);
        assert_eq!(
            run.violations(),
            [lets_doc_tests_use_unsafe(
                "crates/text/src/lib.rs",
                "tidepool_text",
                "tidepool-text"
            )],
            "{what}"
        );
        assert!(
            run.stdout.ends_with("unsafe-audit: FAILED (1 violation)\n"),
            "{what}:\n{}",
            run.stdout
        );
    }

    // The exact form passes, after the `//!` doc comment and the comment
    // that says why, as the repository writes it.
    fixture.source(
        "crates/text/src/lib.rs",
        &format!("//! Text.\n\n// Why.\n#![doc(test(attr(forbid(unsafe_code))))]\n\n{hello}"),
    );
    passes(&fixture, "unsafe-audit");
}

#[test]
fn unsafe_audit_asks_it_only_of_libraries_other_than_sys_and_platform() {
    // The unbroken fixture's sys and platform do not carry the attribute.
    // A binary needs nothing either: a package that has only one, or one
    // beside a library.
    let fixture = Fixture::passing("audit-doc-tests-binaries");
    let manifest = fs::read_to_string(fixture.root.join("Cargo.toml")).expect("the manifest");
    fixture.write(
        "Cargo.toml",
        &manifest.replace("\"crates/text\"]", "\"crates/text\", \"crates/app\"]"),
    );
    fixture.write("crates/app/Cargo.toml", &member_manifest("tidepool", ""));
    fixture.source(
        "crates/app/src/main.rs",
        "//! The binary.\n\nfn main() {}\n",
    );
    fixture.source(
        "crates/text/src/main.rs",
        "//! A binary of text.\n\nfn main() {}\n",
    );
    let run = fixture.run("unsafe-audit");
    run.expect_code(0);
    assert!(
        run.stdout.contains(
            "crate roots checked: 6\nlibrary crate roots that must forbid `unsafe` in doc tests: \
             2\n"
        ),
        "{}",
        run.stdout
    );
    assert!(run.stdout.ends_with("unsafe-audit: ok\n"), "{}", run.stdout);
}

#[test]
fn unsafe_audit_asks_it_of_the_libraries_of_every_workspace_under_the_root() {
    // `agents/` is a workspace of its own (M9.2), outside the root one: its
    // libraries' crate roots must forbid `unsafe` in doc tests too.
    let fixture = Fixture::passing("audit-doc-tests-agents");
    exclude(&fixture, "agents");
    agents_workspace(&fixture, "");
    let run = fixture.run("unsafe-audit");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [lets_doc_tests_use_unsafe(
            "agents/tp-agent-mock/src/lib.rs",
            "tp_agent_mock",
            "tp-agent-mock"
        )]
    );

    fixture.write(
        "agents/tp-agent-mock/src/lib.rs",
        "//! Mock.\n#![doc(test(attr(forbid(unsafe_code))))]\n",
    );
    let run = fixture.run("unsafe-audit");
    run.expect_code(0);
    // The root workspace's four, and the mock agent's library.
    assert!(
        run.stdout.contains(
            "crate roots checked: 5\nlibrary crate roots that must forbid `unsafe` in doc tests: \
             3\n"
        ),
        "{}",
        run.stdout
    );
    assert!(run.stdout.ends_with("unsafe-audit: ok\n"), "{}", run.stdout);
}

/// The items after the crate doc and the attribute line, if any, of the
/// crate root of [`doc_tests_forbid_unsafe_where_the_crate_root_says_so`]'s
/// probe: a doc test for each way to use `unsafe` or allow it again, and one
/// that does neither.
const PROBE_ITEMS: &str = r#"
// The README's examples are compiled as doc tests too.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

/// An `unsafe` block.
///
/// ```
/// let x = 2;
/// let p: *const i32 = &x;
/// assert_eq!(unsafe { *p }, 2);
/// ```
pub fn unsafe_block() {}

/// The doc test allows the lint again.
///
/// ```
/// #![allow(unsafe_code)]
/// let x = 3;
/// let p: *const i32 = &x;
/// assert_eq!(unsafe { *p }, 3);
/// ```
pub fn allow_in_the_doc_test() {}

/// An item in the doc test allows the lint again.
///
/// ```
/// #[allow(unsafe_code)]
/// fn read(p: *const i32) -> i32 {
///     unsafe { *p }
/// }
/// assert_eq!(read(&4), 4);
/// ```
pub fn allow_on_an_item() {}

/// A module whose doc tests allow the lint again.
#[doc(test(attr(allow(unsafe_code))))]
pub mod allow_on_a_module {
    /// ```
    /// let x = 5;
    /// let p: *const i32 = &x;
    /// assert_eq!(unsafe { *p }, 5);
    /// ```
    pub fn read() {}
}

/// Compiled, never run.
///
/// ```no_run
/// let x = 6;
/// let p: *const i32 = &x;
/// assert_eq!(unsafe { *p }, 6);
/// ```
pub fn no_run() {}

/// A macro of the doc test's own, of `k1`'s shape: not another crate's, so
/// the lint sees the `unsafe` it puts before its braces (ADR-0038).
///
/// ```
/// macro_rules! k1 {
///     ($k:tt, $($t:tt)*) => {
///         $k { $($t)* }
///     };
/// }
/// let x = 7;
/// let p: *const i32 = &x;
/// assert_eq!(k1!(unsafe, *p), 7);
/// ```
pub fn local_macro() {}

/// Safe.
///
/// ```
/// assert_eq!(1 + 1, 2);
/// ```
pub fn safe() {}
"#;

/// The README of the probe: one example, with an `unsafe` block.
const PROBE_README: &str = "# probe\n\n```rust\nlet x = 1;\nlet p: *const i32 = &x;\n\
    assert_eq!(unsafe { *p }, 1);\n```\n";

/// The item each doc test that `cargo test --doc` printed `stdout` for
/// documents (`unsafe_block`, `ReadmeDoctests`, `allow_on_a_module::read`),
/// with how it ended (`ok`, `FAILED`), sorted by item.
fn doc_test_outcomes(stdout: &str) -> Vec<(&str, &str)> {
    let mut outcomes: Vec<(&str, &str)> = stdout
        .lines()
        .filter_map(|line| {
            let (name, outcome) = line.strip_prefix("test ")?.rsplit_once(" ... ")?;
            Some((doc_test_item(name)?, outcome))
        })
        .collect();
    outcomes.sort_unstable();
    outcomes
}

/// The item a doc test's name (`probe/src/lib.rs - unsafe_block (line 11)`)
/// says it documents.
fn doc_test_item(name: &str) -> Option<&str> {
    let (_file, rest) = name.split_once(" - ")?;
    Some(rest.split_once(" (line ")?.0)
}

/// What libtest printed for the failed doc test of `item`: its section of
/// `stdout`, after its `---- ... stdout ----` line.
fn doc_test_failure<'a>(stdout: &'a str, item: &str) -> &'a str {
    stdout
        .split("\n---- ")
        .skip(1)
        .find(|section| doc_test_item(section.lines().next().unwrap_or("")) == Some(item))
        .and_then(|section| section.split("\nfailures:").next())
        .unwrap_or_else(|| panic!("no failure output for `{item}`:\n{stdout}"))
}

#[test]
fn doc_tests_forbid_unsafe_where_the_crate_root_says_so() {
    // A workspace shaped like Tidepool's for this: edition 2024, its lints
    // denying `unsafe_code`, a library that inherits them and compiles its
    // README's examples as doc tests through `ReadmeDoctests`.
    let fixture = Fixture::empty("doc-tests");
    fixture.write(
        "Cargo.toml",
        "[workspace]\nresolver = \"3\"\nmembers = [\"probe\"]\n\n[workspace.package]\n\
         edition = \"2024\"\n\n[workspace.lints.rust]\nunsafe_code = \"deny\"\n",
    );
    fixture.write(
        "probe/Cargo.toml",
        "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition.workspace = true\n\
         publish = false\n\n[lints]\nworkspace = true\n",
    );
    fixture.write("probe/README.md", PROBE_README);
    let test_doc = || {
        let output = Command::new(cargo())
            .args([
                "test",
                "--doc",
                "--offline",
                "--color",
                "never",
                "--manifest-path",
            ])
            .arg(fixture.root.join("Cargo.toml"))
            .current_dir(&fixture.root)
            .env("CARGO_TARGET_DIR", fixture.case.join("target"))
            .env("CARGO_NET_OFFLINE", "true")
            .env("CARGO_TERM_COLOR", "never")
            .output()
            .expect("cargo runs");
        Run {
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    };
    let items = [
        "ReadmeDoctests",
        "allow_in_the_doc_test",
        "allow_on_a_module::read",
        "allow_on_an_item",
        "local_macro",
        "no_run",
        "safe",
        "unsafe_block",
    ];

    // The control, without the attribute: every one compiles, and runs.
    // That is the gap, as on Rust 1.99.0: rustdoc does not apply the
    // workspace's `deny`. If this fails, rustdoc does now, and ADR-0036 is
    // to be revisited.
    fixture.write(
        "probe/src/lib.rs",
        &format!("//! The probe.\n{PROBE_ITEMS}"),
    );
    let run = test_doc();
    run.expect_code(0);
    assert_eq!(
        doc_test_outcomes(&run.stdout),
        items.map(|item| (item, "ok")),
        "{}",
        run.stdout
    );

    // With it, each fails to compile for its reason, and the safe one
    // passes.
    fixture.write(
        "probe/src/lib.rs",
        &format!("//! The probe.\n#![doc(test(attr(forbid(unsafe_code))))]\n{PROBE_ITEMS}"),
    );
    let run = test_doc();
    run.expect_code(101);
    assert_eq!(
        doc_test_outcomes(&run.stdout),
        items.map(|item| (item, if item == "safe" { "ok" } else { "FAILED" })),
        "{}",
        run.stdout
    );
    for item in ["ReadmeDoctests", "unsafe_block", "no_run", "local_macro"] {
        let failure = doc_test_failure(&run.stdout, item);
        assert!(
            failure.contains("error: usage of an `unsafe` block"),
            "{item}:\n{failure}"
        );
    }
    for item in [
        "allow_in_the_doc_test",
        "allow_on_an_item",
        "allow_on_a_module::read",
    ] {
        let failure = doc_test_failure(&run.stdout, item);
        assert!(
            failure.contains("error[E0453]: allow(unsafe_code) incompatible with previous forbid"),
            "{item}:\n{failure}"
        );
    }
}

// --- headers ---

#[test]
fn headers_passes_on_the_unbroken_fixture_and_after_a_shebang() {
    let fixture = Fixture::passing("headers-pass");
    fixture.write(
        "scripts/tool.sh",
        &format!("#!/bin/sh\n{}echo tool\n", hash_header()),
    );
    fixture.write(
        ".github/workflows/ci.yml",
        &format!("{}name: CI\n", hash_header()),
    );
    // The demo page's: HTML after its doctype or without one, CSS and
    // JavaScript (ADR-0030).
    fixture.write(
        "site/index.html",
        &format!(
            "<!doctype html>\n{}<html></html>\n",
            enclosed_header("<!--", "-->")
        ),
    );
    fixture.write(
        "site/part.HTML",
        &format!("{}<p>part</p>\n", enclosed_header("<!--", "-->")),
    );
    fixture.write(
        "site/page.css",
        &format!("{}body {{ margin: 0; }}\n", enclosed_header("/*", "*/")),
    );
    fixture.write("site/page.js", &format!("{RUST_HEADER}'use strict';\n"));
    // Configuration and data carry none, nor does the page as plain text.
    fixture.write(".github/dependabot.yml", "version: 2\n");
    fixture.write("site/llms.txt", "# Tidepool\n");
    let run = fixture.run("headers");
    run.expect_code(0);
    assert!(
        run.stdout
            .ends_with("source files checked: 11\nheaders: ok\n"),
        "{}",
        run.stdout
    );
}

#[test]
fn headers_fails_on_each_kind_of_violation() {
    let fixture = Fixture::passing("headers-fail");
    // No header at all.
    fixture.write("crates/text/src/missing.rs", "//! Missing.\n");
    // A line of it altered.
    fixture.write(
        "crates/text/src/altered.rs",
        &format!("{RUST_HEADER}//! Altered.\n").replace("AS IS", "AS-IS"),
    );
    // Below other text.
    fixture.write(
        "crates/core/src/below.rs",
        &format!("//! Below.\n\n{RUST_HEADER}"),
    );
    // No blank line after it.
    fixture.write(
        "crates/sys/src/crowded.rs",
        &format!("{}\n//! Crowded.\n", RUST_HEADER.trim_end_matches('\n')),
    );
    // A script without it after its `#!` line, and a workflow without it.
    fixture.write("scripts/tool.sh", "#!/bin/sh\necho tool\n");
    fixture.write(".github/workflows/ci.yml", "name: CI\n");
    // The demo page's files without it: HTML after a doctype, CSS and
    // JavaScript, and HTML with a CSS file's comment.
    fixture.write("site/index.html", "<!doctype html>\n<html></html>\n");
    fixture.write("site/page.css", "body { margin: 0; }\n");
    fixture.write("site/page.js", "'use strict';\n");
    fixture.write(
        "site/other.html",
        &format!("{}<p></p>\n", enclosed_header("/*", "*/")),
    );

    let run = fixture.run("headers");
    run.expect_code(1);
    let first = "Copyright 2026 Joseph Anthony Abbott III";
    let appendix = "line 1 of the Apache-2.0 header in `LICENSE`'s appendix";
    assert_eq!(
        run.violations(),
        [
            format!(".github/workflows/ci.yml:1: expected `# {first}`, {appendix}"),
            format!("crates/core/src/below.rs:1: expected `// {first}`, {appendix}"),
            "crates/sys/src/crowded.rs:14: expected a blank line after the Apache-2.0 header"
                .to_string(),
            "crates/text/src/altered.rs:10: expected `// distributed under the License is \
             distributed on an \"AS IS\" BASIS,`, line 10 of the Apache-2.0 header in \
             `LICENSE`'s appendix"
                .to_string(),
            format!("crates/text/src/missing.rs:1: expected `// {first}`, {appendix}"),
            format!("scripts/tool.sh:2: expected `# {first}`, {appendix}"),
            "site/index.html:2: expected `<!--`, the comment that holds the Apache-2.0 header"
                .to_string(),
            "site/other.html:1: expected `<!--`, the comment that holds the Apache-2.0 header"
                .to_string(),
            "site/page.css:1: expected `/*`, the comment that holds the Apache-2.0 header"
                .to_string(),
            format!("site/page.js:1: expected `// {first}`, {appendix}"),
        ]
    );
    assert!(
        run.stdout.ends_with("headers: FAILED (10 violations)\n"),
        "{}",
        run.stdout
    );
}

#[test]
fn a_file_name_cannot_add_lines_to_the_output() {
    // A committed script without the header, named so that, printed as it
    // is, it would forge a GitHub Actions annotation and a passing verdict.
    let fixture = Fixture::passing("headers-forged-name");
    let name = "scripts/a\n::warning file=x::forged\nheaders: ok\n\u{1b}[2Kb.sh";
    let path = fixture.root.join(name);
    fs::create_dir_all(path.parent().expect("a parent")).expect("create scripts/");
    if let Err(error) = fs::write(&path, "#!/bin/sh\necho\n") {
        // Windows allows no control character in a file name.
        skipped(
            "a_file_name_cannot_add_lines_to_the_output",
            &format!("cannot name a file so here: {error}"),
        );
        return;
    }
    let run = fixture.run("headers");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            "scripts/a\\n::warning file=x::forged\\nheaders: ok\\n\\u{1b}[2Kb.sh:2: expected \
          `# Copyright 2026 Joseph Anthony Abbott III`, line 1 of the Apache-2.0 header in \
          `LICENSE`'s appendix"
        ]
    );
    assert_forges_nothing(&run, "headers");
    assert!(
        run.stdout.ends_with("headers: FAILED (1 violation)\n"),
        "{}",
        run.stdout
    );
}

/// Asserts that no line `run` printed starts with `::`, once GitHub Actions
/// has trimmed its leading whitespace, holds a control character, or reads
/// `<check>: ok`.
fn assert_forges_nothing(run: &Run, check: &str) {
    let verdict = format!("{check}: ok");
    for line in run.stdout.lines().chain(run.stderr.lines()) {
        assert!(
            !line.trim_start().starts_with("::")
                && line != verdict
                && !line.chars().any(char::is_control),
            "`{line}` in:\n{}\n{}",
            run.stdout,
            run.stderr
        );
    }
}

// --- workflows ---

#[test]
fn workflows_passes_on_the_unbroken_fixture() {
    let fixture = Fixture::passing("workflows-pass");
    let run = fixture.run("workflows");
    run.expect_code(0);
    assert_eq!(run.stdout, "workflows checked: 2\nworkflows: ok\n");
}

#[test]
fn workflows_reports_each_of_the_five_problems_of_github_s_codeql_template() {
    let fixture = Fixture::passing("workflows-template");
    fixture.write(".github/workflows/codeql.yml", CODEQL_TEMPLATE);
    let run = fixture.run("workflows");
    run.expect_code(1);
    // Three actions pinned by tag, a checkout that keeps its credentials,
    // and no top-level `permissions`; nothing else.
    assert_eq!(
        run.violations(),
        [
            ".github/workflows/codeql.yml:60: `actions/checkout@v7` is not pinned by a full \
             commit SHA",
            ".github/workflows/codeql.yml:60: `actions/checkout` does not set \
             `persist-credentials: false`",
            ".github/workflows/codeql.yml:70: `github/codeql-action/init@v4` is not pinned by a \
             full commit SHA",
            ".github/workflows/codeql.yml:99: `github/codeql-action/analyze@v4` is not pinned by \
             a full commit SHA",
            ".github/workflows/codeql.yml: no `permissions:` at the top, so the token gets the \
             repository's default permissions",
        ]
    );
    assert!(
        run.stdout.starts_with("workflows checked: 2\n")
            && run.stdout.ends_with("workflows: FAILED (5 violations)\n"),
        "{}",
        run.stdout
    );
}

#[test]
fn workflows_fails_on_each_kind_of_violation() {
    let fixture = Fixture::passing("workflows-fail");
    fixture.workflow(
        "ci.yml",
        "name: CI\n\
         on:\n  pull_request_target:\n\
         permissions: write-all\n\
         jobs:\n  ci:\n    runs-on: ubuntu-latest\n    permissions:\n      contents: write\n\
         \x20   steps:\n\
         \x20     - uses: actions/checkout@v7\n\
         \x20     - uses: actions/cache@55cc8345863c7cc4c66a329aec7e433d2d1c52a9\n\
         \x20     - uses: some/action@3d3c42e5aac5ba805825da76410c181273ba90b1 # v1.0.0\n",
    );
    // A workflow with no row in the table.
    fixture.workflow("deploy.yml", CI_WORKFLOW);
    let run = fixture.run("workflows");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            ".github/workflows/ci.yml:17: `pull_request_target` runs with the base repository's \
             token: use `pull_request`",
            ".github/workflows/ci.yml:18: the default permissions are `write-all`: make them `{}` \
             or `contents: read`",
            ".github/workflows/ci.yml:23: the job `ci` may not grant `contents: write`",
            ".github/workflows/ci.yml:25: `actions/checkout@v7` is not pinned by a full commit SHA",
            ".github/workflows/ci.yml:25: `actions/checkout` does not set \
             `persist-credentials: false`",
            ".github/workflows/ci.yml:26: `actions/cache` has no comment naming its tag, such as \
             `# v1.2.3`",
            ".github/workflows/ci.yml:27: `some/action` is not an action this workflow may use \
             (ADR-0022)",
            ".github/workflows/deploy.yml: no row in the table of what each workflow may use \
             (`WORKFLOWS` in xtask/src/workflows.rs, ADR-0022)",
        ]
    );
    assert!(
        run.stdout.starts_with("workflows checked: 3\n")
            && run.stdout.ends_with("workflows: FAILED (8 violations)\n"),
        "{}",
        run.stdout
    );
}

#[test]
fn workflows_fails_closed_without_a_workflow_to_check() {
    // A check of the wrong directory must not pass by finding nothing.
    let fixture = Fixture::passing("workflows-missing");
    let dir = fixture.root.join(".github/workflows");
    fs::remove_dir_all(&dir).expect("the fixture has workflows");
    let run = fixture.run("workflows");
    run.expect_code(1);
    assert_eq!(
        run.stdout,
        "workflows checked: 0\n\
         violation: .github/workflows was not found, so no workflow was checked: is this the \
         repository root?\n\
         workflows: FAILED (1 violation)\n"
    );
    // A directory with no workflow in it.
    fixture.write(".github/workflows/README.md", "No workflows.\n");
    let run = fixture.run("workflows");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            ".github/workflows holds no workflow (a `.yml` or `.yaml` file), so nothing was \
             checked"
        ]
    );
}

#[test]
fn workflows_reports_a_linked_workflow_without_reading_it() {
    let fixture = Fixture::passing("workflows-linked");
    // What the link leads to breaks every rule, and says so.
    let evil = format!(
        "{}# this line must never be printed\npermissions: write-all\njobs:\n  x:\n    \
         steps:\n      - uses: some/action@v1\n",
        hash_header()
    );
    fixture.write_outside("elsewhere/ci.yml", &evil);
    fixture.write_outside("elsewhere/codeql.yml", &evil);
    let link = fixture.root.join(".github/workflows/linked.yml");
    if let Err(reason) = symlink(&fixture.real("elsewhere/ci.yml"), &link) {
        skipped(
            "workflows_reports_a_linked_workflow_without_reading_it",
            &reason,
        );
        return;
    }
    let unread = |run: &Run| {
        let output = format!("{}{}", run.stdout, run.stderr);
        assert!(
            !output.contains("must never be printed")
                && !output.contains("some/action")
                && !output.contains("write-all"),
            "{output}"
        );
    };
    let run = fixture.run("workflows");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            ".github/workflows/linked.yml: a symbolic link, which `workflows` does not follow, \
             so it was not read"
        ]
    );
    assert!(
        run.stdout.starts_with("workflows checked: 2\n"),
        "{}",
        run.stdout
    );
    unread(&run);

    // The directory itself a link: nothing in it is listed or read.
    let dir = fixture.root.join(".github/workflows");
    fs::remove_dir_all(&dir).expect("the fixture has workflows");
    symlink(&fixture.real("elsewhere"), &dir).expect("a link to a directory");
    let run = fixture.run("workflows");
    run.expect_code(1);
    assert_eq!(
        run.stdout,
        "workflows checked: 0\n\
         violation: .github/workflows is a symbolic link, which `workflows` does not follow, so \
         no workflow was checked\n\
         workflows: FAILED (1 violation)\n"
    );
    unread(&run);
}

#[test]
fn workflows_fails_closed_on_yaml_it_cannot_read_line_by_line() {
    // Two workflows that GitHub reads as granting writes and running an
    // unapproved action, and that a check matching lines as written would
    // pass: a lone CR puts a job's `write-all` on the line before, as far as
    // `str::lines` sees, and a space before a colon hides a job's
    // `permissions` and a step's `uses`.
    let fixture = Fixture::passing("workflows-unreadable");
    fixture.workflow(
        "ci.yml",
        &CI_WORKFLOW.replace(
            "    runs-on: ubuntu-latest\n",
            "    runs-on: ubuntu-latest\r    permissions: write-all\n",
        ),
    );
    fixture.workflow(
        "codeql.yml",
        &(CODEQL_WORKFLOW
            .replace("    permissions:\n", "    permissions :\n")
            .replace(
                "      security-events: write\n",
                "      security-events: write\n      id-token: write\n",
            )
            + "      - uses : evil/action@main\n"),
    );
    let run = fixture.run("workflows");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            ".github/workflows/ci.yml:27: a line break other than LF or CR LF, which a YAML \
             reader reads as a new line and this check does not",
            ".github/workflows/codeql.yml:29: the job `analyze` may not grant `id-token: write`",
            ".github/workflows/codeql.yml:38: `evil/action` is not an action this workflow may \
             use (ADR-0022)",
            ".github/workflows/codeql.yml:38: `evil/action@main` is not pinned by a full commit \
             SHA",
        ]
    );
    assert!(
        run.stdout.starts_with("workflows checked: 2\n")
            && run.stdout.ends_with("workflows: FAILED (4 violations)\n"),
        "{}",
        run.stdout
    );
}

#[test]
fn a_workflow_s_name_or_text_cannot_add_lines_to_the_output() {
    // A workflow named to forge a GitHub Actions annotation and a passing
    // verdict, and an action reference that holds a C1 control (CSI), a
    // forged command and an escape sequence, which the violation quotes. (A
    // lone carriage return there is a line break, which fails without
    // quoting anything.)
    let fixture = Fixture::passing("workflows-forged");
    let name = ".github/workflows/a\n::warning file=x::forged\nworkflows: ok\n\u{1b}[2Kb.yml";
    if let Err(error) = fs::write(fixture.root.join(name), "name: x\n") {
        // Windows allows no control character in a file name.
        skipped(
            "a_workflow_s_name_or_text_cannot_add_lines_to_the_output",
            &format!("cannot name a file so here: {error}"),
        );
        return;
    }
    fixture.workflow(
        "ci.yml",
        &CI_WORKFLOW.replace(
            "actions/cache@55cc8345863c7cc4c66a329aec7e433d2d1c52a9 # v6.1.0",
            "actions/cache@v6\u{9b}::error::forged\u{1b}[2K",
        ),
    );
    let run = fixture.run("workflows");
    run.expect_code(1);
    assert_eq!(
        run.violations(),
        [
            ".github/workflows/a\\n::warning file=x::forged\\nworkflows: ok\\n\\u{1b}[2Kb.yml: no \
             row in the table of what each workflow may use (`WORKFLOWS` in \
             xtask/src/workflows.rs, ADR-0022)",
            ".github/workflows/ci.yml:34: `actions/cache@v6\\u{9b}::error::forged\\u{1b}[2K` is \
             not pinned by a full commit SHA",
        ]
    );
    assert_forges_nothing(&run, "workflows");
    assert!(
        run.stdout.ends_with("workflows: FAILED (2 violations)\n"),
        "{}",
        run.stdout
    );
}

// --- output and links (M0.10) ---

#[test]
fn a_directory_name_cannot_add_lines_through_cargo_s_messages() {
    // A package in a directory named to forge an annotation and a verdict.
    // `deps` and `unsafe-audit` ask cargo about it and quote its command
    // line and its error, which name the path.
    let fixture = Fixture::passing("forged-directory");
    let dir = "a\n::error file=README.md::forged by a directory name\nunsafe-audit: ok\n\
               deps: ok\nx";
    if let Err(error) = fs::create_dir_all(fixture.root.join(dir)) {
        // Windows allows no control character in a file name.
        skipped(
            "a_directory_name_cannot_add_lines_through_cargo_s_messages",
            &format!("cannot name a directory so here: {error}"),
        );
        return;
    }
    fixture.write(
        &format!("{dir}/Cargo.toml"),
        &manifest("stray", "0.1.0", ""),
    );
    fixture.source(&format!("{dir}/src/lib.rs"), "//! Stray.\n");
    for check in ["deps", "unsafe-audit"] {
        let run = fixture.run(check);
        run.expect_code(1);
        assert_forges_nothing(&run, check);
        assert!(
            run.stdout.contains("a\\n::error file=README.md::forged"),
            "{check}: the name is not shown escaped:\n{}",
            run.stdout
        );
    }
}

#[test]
fn deps_and_layers_refuse_a_linked_input_without_reading_it() {
    for (check, input) in [
        ("deps", "xtask/deps-allowlist.txt"),
        ("layers", "docs/01-architecture.md"),
    ] {
        let fixture = Fixture::passing("linked-input");
        fixture.write_outside("elsewhere.txt", "this line must never be printed\n");
        let link = fixture.root.join(input);
        fs::remove_file(&link).expect("the input exists");
        if let Err(reason) = symlink(&fixture.real("elsewhere.txt"), &link) {
            skipped(
                "deps_and_layers_refuse_a_linked_input_without_reading_it",
                &reason,
            );
            return;
        }
        let run = fixture.run(check);
        run.expect_code(1);
        let output = format!("{}{}", run.stdout, run.stderr);
        assert!(
            output.contains(&format!(
                "{input} is a symbolic link, which xtask does not follow"
            )),
            "{check}:\n{output}"
        );
        assert!(
            !output.contains("must never be printed"),
            "{check}:\n{output}"
        );
    }
}

// --- ci ---

#[test]
fn ci_keeps_going_after_a_formatting_failure_and_exits_1() {
    let fixture = Fixture::passing("ci-fmt").with_ucd();
    // Valid Rust that `cargo fmt --check` rejects, and nothing else does.
    fixture.source(
        "crates/text/src/lib.rs",
        "//! Text.\n#![doc(test(attr(forbid(unsafe_code))))]\n\n/// Says hello.\n\
         pub fn hello( )->&'static str{\"hello\"}\n",
    );

    let run = fixture.run("ci");
    run.expect_code(1);
    let headings: Vec<&str> = run
        .stdout
        .lines()
        .filter_map(|line| line.strip_prefix("== "))
        .map(|heading| heading.split(':').next().unwrap_or(heading))
        .collect();
    assert_eq!(
        headings,
        [
            "deps",
            "fmt",
            "clippy",
            "test",
            "layers",
            "crates",
            "features",
            "unsafe-audit",
            "headers",
            "workflows",
            "ucd",
            "summary"
        ],
        "{}",
        run.stdout
    );
    let summary = run
        .stdout
        .split("== summary\n")
        .nth(1)
        .expect("ci prints a summary");
    assert_eq!(
        run.summary(),
        [
            ("deps", "ok"),
            ("fmt", "FAILED"),
            ("clippy", "ok"),
            ("test", "ok"),
            ("layers", "ok"),
            ("crates", "ok"),
            ("features", "ok"),
            ("unsafe-audit", "ok"),
            ("headers", "ok"),
            ("workflows", "ok"),
            ("ucd", "ok"),
            ("tools", "not"),
            ("theme", "not"),
            ("ci:", "FAILED:"),
        ],
        "{}",
        run.stdout
    );
    assert!(
        summary.ends_with(
            "ci: FAILED: 1 of 11 steps failed: fmt; not yet implemented: tools, theme\n"
        ),
        "{summary}"
    );
    assert!(
        summary.contains("tools         not yet implemented (roadmap M5.1)\n"),
        "{summary}"
    );
}

#[test]
fn ci_fails_every_step_but_deps_each_for_its_own_reason() {
    // `deps` passes, so every other step runs, and each fails for the
    // reason planted for it: a step that ran nothing, or the wrong command,
    // would pass instead.
    let fixture = Fixture::passing("ci-all").with_ucd();
    // fmt: badly formatted, and nothing else is wrong with it.
    fixture.source(
        "crates/text/src/lib.rs",
        "//! Text.\n#![doc(test(attr(forbid(unsafe_code))))]\n\n/// Says hello.\n\
         pub fn hello( )->&'static str{\"hello\"}\n",
    );
    // clippy: `needless_return`, a warning that `-D warnings` denies. And
    // unsafe-audit: the lint relaxed outside the two crate roots.
    fixture.source(
        "crates/core/src/lib.rs",
        "//! Core.\n#![allow(unsafe_code)]\n#![doc(test(attr(forbid(unsafe_code))))]\n\n\
         /// One.\npub fn one() -> i32 {\n    return 1;\n}\n",
    );
    // test: a test that fails.
    fixture.source(
        "crates/sys/src/lib.rs",
        "//! Sys.\n#![allow(unsafe_code)]\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    \
         fn planted_failure() {\n        panic!(\"planted failure\");\n    }\n}\n",
    );
    // layers: a dev-dependency the matrix does not allow.
    fixture.append(
        "crates/sys/Cargo.toml",
        "[dev-dependencies]\ntidepool-core = { path = \"../core\" }\n",
    );
    fixture.lock();
    // crates: a member without its README.
    fs::remove_file(fixture.root.join("crates/platform/README.md")).expect("the README exists");
    // features: the ticked task in no feature.
    fixture.write(
        "docs/features.jsonc",
        &FEATURES.replace(
            "[\"M0.1\"], \"ticked\": [\"M0.1\"]",
            "[\"M0.2\"], \"ticked\": []",
        ),
    );
    // headers: a script without one.
    fixture.write("scripts/tool.sh", "#!/bin/sh\necho tool\n");
    // workflows: an action pinned by its tag.
    fixture.workflow(
        "ci.yml",
        &CI_WORKFLOW.replace(
            "actions/cache@55cc8345863c7cc4c66a329aec7e433d2d1c52a9 # v6.1.0",
            "actions/cache@v6",
        ),
    );
    // ucd: tables that are not what the Unicode data generates.
    fixture.write("crates/text/src/pos/grapheme/tables.rs", "// stale\n");

    let run = fixture.run("ci");
    run.expect_code(1);
    assert_eq!(
        run.summary(),
        [
            ("deps", "ok"),
            ("fmt", "FAILED"),
            ("clippy", "FAILED"),
            ("test", "FAILED"),
            ("layers", "FAILED"),
            ("crates", "FAILED"),
            ("features", "FAILED"),
            ("unsafe-audit", "FAILED"),
            ("headers", "FAILED"),
            ("workflows", "FAILED"),
            ("ucd", "FAILED"),
            ("tools", "not"),
            ("theme", "not"),
            ("ci:", "FAILED:"),
        ],
        "{}",
        run.stdout
    );
    assert_eq!(
        run.verdict(),
        "ci: FAILED: 10 of 11 steps failed: fmt, clippy, test, layers, crates, features, \
         unsafe-audit, headers, workflows, ucd; not yet implemented: tools, theme"
    );
    let output = format!("{}{}", run.stdout, run.stderr);
    for reason in [
        "Diff in",
        "clippy::needless_return",
        "planted failure",
        "violation: tidepool-sys -> tidepool-core not allowed (dev-dependency)",
        "violation: crates/platform/README.md is missing",
        "violation: docs/roadmap.md: M0.1 (fixture: the first task) is ticked, but no feature in \
         docs/features.jsonc needs it",
        "violation: crates/core/src/lib.rs:16: `allow(unsafe_code)` is only allowed",
        "violation: scripts/tool.sh:2: expected `# Copyright 2026 Joseph Anthony Abbott III`",
        "violation: .github/workflows/ci.yml:34: `actions/cache@v6` is not pinned by a full commit \
         SHA",
        "violation: crates/text/src/pos/grapheme/tables.rs is not what crates/text/ucd/17.0.0 \
         generates (first difference at line 1); run cargo xtask ucd",
    ] {
        assert!(output.contains(reason), "`{reason}` missing:\n{output}");
    }
}

#[test]
fn ci_compiles_nothing_after_deps_fails() {
    // A path dependency outside the repository whose build script leaves a
    // marker: deps rejects it, so clippy and test must not build it.
    let fixture = Fixture::passing("ci-gate").with_ucd();
    fixture.write_outside(
        "outside/evil/Cargo.toml",
        &manifest("evil", "0.1.0", "[workspace]\n"),
    );
    fixture.write_outside("outside/evil/src/lib.rs", "//! Evil.\n");
    fixture.write_outside(
        "outside/evil/build.rs",
        "//! Leaves a marker.\n\nfn main() {\n    let dir = std::env::var(\"CARGO_MANIFEST_DIR\").unwrap();\n    \
         std::fs::write(format!(\"{dir}/../MARKER\"), \"ran\").unwrap();\n}\n",
    );
    fixture.append(
        "crates/text/Cargo.toml",
        "evil = { path = \"../../../outside/evil\" }\n",
    );
    fixture.lock();
    let marker = fixture.case.join("outside/MARKER");

    let run = fixture.run("ci");
    run.expect_code(1);
    assert!(!marker.exists(), "ci ran the build script:\n{}", run.stdout);
    assert_eq!(
        run.summary(),
        [
            ("deps", "FAILED"),
            ("fmt", "ok"),
            ("clippy", "not"),
            ("test", "not"),
            ("layers", "FAILED"),
            ("crates", "ok"),
            ("features", "ok"),
            ("unsafe-audit", "ok"),
            ("headers", "ok"),
            ("workflows", "ok"),
            ("ucd", "ok"),
            ("tools", "not"),
            ("theme", "not"),
            ("ci:", "FAILED:"),
        ],
        "{}",
        run.stdout
    );
    assert_eq!(
        run.verdict(),
        "ci: FAILED: 2 of 11 steps failed: deps, layers; not run: clippy, test; not yet \
         implemented: tools, theme"
    );
    assert!(
        run.stdout.contains(
            "clippy: not run: `deps` failed, and this step would compile and run the code it \
             rejected\n"
        ),
        "{}",
        run.stdout
    );
    assert!(
        run.stdout
            .contains("violation: `evil 0.1.0` is a path package outside the repository"),
        "{}",
        run.stdout
    );

    // The control: building the fixture does run the build script, so its
    // absence above means something.
    let built = Command::new(cargo())
        .args(["check", "--offline", "--quiet", "--manifest-path"])
        .arg(fixture.root.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", fixture.case.join("target"))
        .output()
        .expect("cargo runs");
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    assert!(marker.exists(), "the control build did not run the script");
}

/// A `tidepool-text` library, formatted as rustfmt wants it, whose tests
/// print `SKIPPED:` as a real-tool test does without its tool: straight to
/// standard error, which the test harness does not capture, or to standard
/// output, which it does not capture either but writes its own lines to.
const SKIPPING_TESTS: &str = r#"//! Text.
#![doc(test(attr(forbid(unsafe_code))))]

/// Says hello.
pub fn hello() -> &'static str {
    "hello"
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    #[test]
    fn says_hello() {
        assert_eq!(super::hello(), "hello");
    }

    #[test]
    fn needs_git() {
        let _ = writeln!(
            std::io::stderr(),
            "SKIPPED: needs_git: TIDEPOOL_IT_GIT is not set"
        );
    }

    #[test]
    fn needs_rust_analyzer() {
        let mut err = std::io::stderr();
        let _ = writeln!(err, "note: this line is not SKIPPED: it only mentions it");
        let _ = writeln!(
            err,
            "SKIPPED: needs_rust_analyzer: TIDEPOOL_IT_RA is not set"
        );
    }

    #[test]
    fn needs_android() {
        // The harness may be part way through a line of its own here: the
        // leading newline starts a new one, in the same write.
        let _ = write!(
            std::io::stdout(),
            "\nSKIPPED: needs_android: TIDEPOOL_IT_ANDROID is not set\n"
        );
    }
}
"#;

#[test]
fn ci_lists_and_counts_skipped_tests_and_still_passes() {
    // Three tests print `SKIPPED:` (a real-tool test without its tool), two
    // on standard error and one on standard output, one runs, and a line
    // that only mentions the word does not count.
    let fixture = Fixture::passing("ci-skipped").with_ucd();
    fixture.source("crates/text/src/lib.rs", SKIPPING_TESTS);
    let run = fixture.run("ci");
    run.expect_code(0);
    assert_eq!(
        run.summary(),
        [
            ("deps", "ok"),
            ("fmt", "ok"),
            ("clippy", "ok"),
            ("test", "ok"),
            ("layers", "ok"),
            ("crates", "ok"),
            ("features", "ok"),
            ("unsafe-audit", "ok"),
            ("headers", "ok"),
            ("workflows", "ok"),
            ("ucd", "ok"),
            ("tools", "not"),
            ("theme", "not"),
            ("ci:", "ok"),
        ],
        "{}",
        run.stdout
    );
    // Each reason, the step's count, its summary row and the verdict.
    for expected in [
        "skipped: needs_git: TIDEPOOL_IT_GIT is not set\n",
        "skipped: needs_rust_analyzer: TIDEPOOL_IT_RA is not set\n",
        "skipped: needs_android: TIDEPOOL_IT_ANDROID is not set\n",
        "test: ok (3 SKIPPED)\n",
        "\ntest          ok (3 SKIPPED)  ",
    ] {
        assert!(
            run.stdout.contains(expected),
            "`{expected}` missing:\n{}",
            run.stdout
        );
    }
    assert_eq!(
        run.verdict(),
        "ci: ok (3 tests SKIPPED): 11 of 11 steps passed; not yet implemented: tools, theme"
    );
    // The test output still reaches the user as it came.
    assert!(
        run.stdout.contains("test result: ok. 4 passed"),
        "{}",
        run.stdout
    );
    assert!(
        run.stderr
            .contains("SKIPPED: needs_git: TIDEPOOL_IT_GIT is not set\n"),
        "{}",
        run.stderr
    );
    assert!(
        run.stderr
            .contains("note: this line is not SKIPPED: it only mentions it\n"),
        "{}",
        run.stderr
    );
}

#[test]
fn a_relative_build_directory_is_passed_on_absolute() {
    // Cargo resolves a relative `CARGO_TARGET_DIR` (or
    // `CARGO_BUILD_TARGET_DIR`) against the directory it started in, which
    // is where `xtask` starts; its cargo children run in the repository, so
    // they must get it absolute, or they build in `<repo>/reltarget`.
    for variable in ["CARGO_TARGET_DIR", "CARGO_BUILD_TARGET_DIR"] {
        let fixture = Fixture::passing("ci-reltarget").with_ucd();
        let root = fixture.root.to_string_lossy().into_owned();
        let run = xtask_in(
            &fixture.case,
            &["ci", "--root", &root],
            &[(variable, Some("reltarget"))],
        );
        run.expect_code(0);
        assert!(
            fixture.case.join("reltarget").is_dir(),
            "{variable}: nothing built in the directory it names:\n{}",
            run.stdout
        );
        assert!(
            !fixture.root.join("reltarget").exists(),
            "{variable}: built inside the repository instead"
        );
    }
}

#[test]
fn a_relative_root_is_made_absolute() {
    // The cargo children run in the root and are given paths under it, so
    // a relative `--root` other than `.` must be made absolute against the
    // directory `xtask` starts in before anything uses it.
    let fixture = Fixture::passing("relative-root");
    let target = fixture.case.join("target").to_string_lossy().into_owned();
    let case = fixture
        .case
        .file_name()
        .expect("the case directory has a name")
        .to_string_lossy()
        .into_owned();
    // The last one leaves the directory `xtask` starts in and comes back.
    for root in [
        "repo".to_string(),
        "./repo/".to_string(),
        format!("../{case}/repo"),
    ] {
        for command in ["deps", "layers", "crates", "unsafe-audit"] {
            let run = xtask_in(
                &fixture.case,
                &[command, "--root", &root],
                &[("CARGO_TARGET_DIR", Some(&target))],
            );
            run.expect_code(0);
            assert!(
                run.stdout.ends_with(&format!("{command}: ok\n")),
                "`{command} --root {root}`:\n{}",
                run.stdout
            );
        }
    }
}

// --- site ---

/// The variables `site` reads the commit's facts from.
const SITE_VARIABLES: [&str; 3] = [
    "TIDEPOOL_SITE_COMMIT",
    "TIDEPOOL_SITE_DATE",
    "TIDEPOOL_SITE_SUBJECT",
];

/// The commit a fixture's page names.
const SITE_COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

/// An example program, printing `text`, as rustfmt writes it.
fn example(text: &str) -> String {
    format!("//! An example.\n\nfn main() {{\n    println!(\"{text}\");\n}}\n")
}

impl Fixture {
    /// The fixture with what `site` reads too: a `tidepool-codec` with
    /// the example `formats` and the matrix row it needs, `tidepool-text`'s
    /// example `editing` and tests that print `SKIPPED:`
    /// ([`SKIPPING_TESTS`]), the real repository's `site/` and
    /// `rust-toolchain.toml`, and a `.git` whose HEAD is [`SITE_COMMIT`].
    fn with_site(self) -> Self {
        let real = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let manifest = fs::read_to_string(self.root.join("Cargo.toml")).expect("the manifest");
        self.write(
            "Cargo.toml",
            &manifest.replace("\"crates/text\"]", "\"crates/text\", \"crates/codec\"]"),
        );
        self.write(
            "docs/01-architecture.md",
            &MATRIX.replace(
                "| `tidepool-text` | `core` |\n",
                "| `tidepool-text` | `core` |\n| `tidepool-codec` | — |\n",
            ),
        );
        self.package(
            "codec",
            "",
            "//! Codec.\n#![doc(test(attr(forbid(unsafe_code))))]\n",
        );
        self.source(
            "crates/codec/examples/formats.rs",
            &example("formats <ok> & 🌊"),
        );
        self.source("crates/text/src/lib.rs", SKIPPING_TESTS);
        self.source(
            "crates/text/examples/editing.rs",
            &example("editing │ café"),
        );
        for file in [
            "rust-toolchain.toml",
            "site/index.html",
            "site/llms.txt",
            "site/tidepool.css",
            "site/tidepool.js",
        ] {
            let text = fs::read_to_string(real.join(file)).expect("the repository's file");
            write(&self.root.join(file), &text);
        }
        write(&self.root.join(".git/HEAD"), &format!("{SITE_COMMIT}\n"));
        self.lock();
        self
    }

    /// Runs `xtask site --out <out> --root <the fixture>` with `env`; the
    /// variables of [`SITE_VARIABLES`] that `env` does not set are removed.
    fn site(&self, out: &Path, env: &[(&str, &str)]) -> Run {
        let root = self.root.to_string_lossy().into_owned();
        let out = out.to_string_lossy().into_owned();
        let mut vars: Vec<(&str, Option<&str>)> =
            SITE_VARIABLES.iter().map(|name| (*name, None)).collect();
        vars.extend(env.iter().map(|(name, value)| (*name, Some(*value))));
        xtask_with(
            &["site", "--out", &out, "--root", &root],
            Some(&self.case.join("target")),
            &vars,
        )
    }
}

/// The files of the page in `dir`, and their bytes.
fn page_files(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<(String, Vec<u8>)> = fs::read_dir(dir)
        .expect("the page's directory")
        .map(|entry| {
            let entry = entry.expect("an entry");
            (
                entry.file_name().to_string_lossy().into_owned(),
                fs::read(entry.path()).expect("a file of the page"),
            )
        })
        .collect();
    files.sort();
    files
}

#[test]
fn site_builds_the_page_from_the_gate_the_examples_and_the_commit_s_facts() {
    let fixture = Fixture::passing("site").with_ucd().with_site();
    let out = fixture.case.join("page");
    let live = [
        ("TIDEPOOL_SITE_COMMIT", SITE_COMMIT),
        ("TIDEPOOL_SITE_DATE", "2026-11-08"),
        ("TIDEPOOL_SITE_SUBJECT", "site: the <page> & \"more\""),
    ];

    // A live build: the workflow's facts.
    let run = fixture.site(&out, &live);
    run.expect_code(0);
    // The gate printed as `ci` prints it, before the page was written.
    assert!(run.stdout.contains("\n== deps: "), "{}", run.stdout);
    assert!(
        run.stdout.contains(
            "ci: ok (3 tests SKIPPED): 11 of 11 steps passed; not yet implemented: tools, theme\n"
        ),
        "{}",
        run.stdout
    );
    assert!(run.stdout.contains("site: wrote "), "{}", run.stdout);
    let names: Vec<String> = page_files(&out).into_iter().map(|(name, _)| name).collect();
    assert_eq!(
        names,
        [
            ".tidepool-site",
            "index.html",
            "llms.txt",
            "tidepool.css",
            "tidepool.js"
        ]
    );
    let html = fs::read_to_string(out.join("index.html")).expect("the page");
    assert!(!html.contains("{{"), "a placeholder is left");
    for expected in [
        "Live build <span class=\"kicker__sep\">//</span> <a class=\"tp-mono\" \
         href=\"https://github.com/jabbott-iii/Tidepool/commit/\
         0123456789abcdef0123456789abcdef01234567\">0123456</a>",
        "<dd>2026-11-08</dd>",
        "Latest change: <span class=\"tp-mono\">site: the &lt;page&gt; &amp; \
         &quot;more&quot;</span>",
        // Four tests passed, three of which printed `SKIPPED:`: one pass.
        "<dd>1 passed</dd>",
        "<div><dt>Skipped</dt><dd>3</dd></div>",
        "<dd>11 of 11 passed</dd>",
        "<dd>5 in the workspace</dd>",
        "<dd>1 of 2 tasks</dd>",
        "--example formats\nformats &lt;ok&gt; &amp; 🌊</pre>",
        "--example editing\nediting │ café</pre>",
        "</span>\n== summary\ndeps ",
        "ci: ok (3 tests SKIPPED): 11 of 11 steps passed; not yet implemented: tools, \
         theme</pre>",
    ] {
        assert!(html.contains(expected), "missing {expected:?}");
    }
    // Nothing that cargo or a test printed reaches the page: not the skip
    // reasons `ci` lists, nor libtest's lines.
    for printed in ["needs_git", "TIDEPOOL_IT_GIT", "test result:", "Compiling"] {
        assert!(!html.contains(printed), "{printed} is on the page");
    }
    let text = fs::read_to_string(out.join("llms.txt")).expect("the page as text");
    assert!(
        text.contains("at commit 0123456 of 2026-11-08 (site: the <page> & \"more\")."),
        "{text}"
    );
    assert!(!text.contains("needs_git"), "{text}");
    for name in ["tidepool.css", "tidepool.js"] {
        assert_eq!(
            fs::read(out.join(name)).expect("a copy"),
            fs::read(fixture.root.join("site").join(name)).expect("the original"),
        );
    }

    // A local build into the same directory: HEAD from `.git`.
    fs::write(out.join("notes.txt"), "kept").expect("a file of the user's");
    let run = fixture.site(&out, &[]);
    run.expect_code(0);
    let html = fs::read_to_string(out.join("index.html")).expect("the page");
    assert!(html.contains("Local build <span"), "{html}");
    assert!(html.contains(">0123456</a>"));
    assert!(html.contains("<dd>Not given</dd>"));
    assert!(!html.contains("Latest change"));
    // Built from the working tree, which may differ from HEAD.
    assert!(
        html.contains(
            "A local build of the working tree at HEAD; it may hold changes HEAD does not."
        ),
        "{html}"
    );
    assert_eq!(
        fs::read_to_string(out.join("notes.txt")).expect("kept"),
        "kept"
    );
    let built = page_files(&out);

    // One or two of the variables, or one not valid: refused before
    // anything runs, naming the variable and not its value.
    let run = fixture.site(&out, &live[..1]);
    run.expect_code(1);
    assert_eq!(run.stdout, "");
    assert_eq!(
        run.stderr,
        "xtask: `TIDEPOOL_SITE_DATE` is not set, but TIDEPOOL_SITE_COMMIT is: set all three of \
         TIDEPOOL_SITE_COMMIT, TIDEPOOL_SITE_DATE and TIDEPOOL_SITE_SUBJECT, or none\n"
    );
    let bad = [
        live[0],
        live[1],
        ("TIDEPOOL_SITE_SUBJECT", "two\nlines ::error::x"),
    ];
    let run = fixture.site(&out, &bad);
    run.expect_code(1);
    assert_eq!(run.stdout, "");
    assert!(
        run.stderr
            .starts_with("xtask: `TIDEPOOL_SITE_SUBJECT` holds a control character"),
        "{}",
        run.stderr
    );
    assert!(!run.stderr.contains("::error::"), "{}", run.stderr);
    let bad = [live[0], ("TIDEPOOL_SITE_DATE", "2026-02-30"), live[2]];
    let run = fixture.site(&out, &bad);
    run.expect_code(1);
    assert!(
        run.stderr
            .starts_with("xtask: `TIDEPOOL_SITE_DATE` is not a date written YYYY-MM-DD"),
        "{}",
        run.stderr
    );
    assert_eq!(page_files(&out), built);

    // A directory `site` did not make is refused, and left alone.
    let other = fixture.case.join("other");
    write(&other.join("index.html"), "mine");
    let run = fixture.site(&other, &[]);
    run.expect_code(2);
    assert!(
        run.stderr
            .contains("is not empty, and `cargo xtask site` did not make it")
    );
    assert_eq!(
        fs::read_to_string(other.join("index.html")).expect("mine"),
        "mine"
    );
    // So is one `site` made that holds a directory where it writes a file:
    // before anything runs, and nothing in it is removed.
    let marked = fixture.case.join("marked");
    write(&marked.join(".tidepool-site"), "x");
    write(&marked.join("llms.txt"), "old");
    write(&marked.join("index.html").join("keep.txt"), "mine");
    let run = fixture.site(&marked, &[]);
    run.expect_code(2);
    assert_eq!(run.stdout, "");
    assert!(
        run.stderr.contains(
            "index.html` is a directory, where `cargo xtask site` writes a file: move it, or \
             give a new or empty directory\n"
        ),
        "{}",
        run.stderr
    );
    assert_eq!(
        fs::read_to_string(marked.join("index.html").join("keep.txt")).expect("mine"),
        "mine"
    );
    assert_eq!(
        fs::read_to_string(marked.join("llms.txt")).expect("old"),
        "old"
    );

    // The features failing: nothing runs, nothing is written.
    let features = fs::read_to_string(fixture.root.join("docs/features.jsonc")).expect("them");
    fixture.write(
        "docs/features.jsonc",
        &features.replace("\"ticked\": [\"M0.1\"]", "\"ticked\": []"),
    );
    let run = fixture.site(&out, &[]);
    run.expect_code(1);
    assert!(
        run.stdout.ends_with(
            "site: FAILED: docs/features.jsonc does not pass `features`, so no page was written\n"
        ),
        "{}",
        run.stdout
    );
    assert!(!run.stdout.contains("== deps"), "{}", run.stdout);
    fixture.write("docs/features.jsonc", &features);
    assert_eq!(page_files(&out), built);

    // A step of the gate failing: no example runs, nothing is written.
    fixture.source(
        "crates/codec/src/lib.rs",
        "//! Codec.\n#![doc(test(attr(forbid(unsafe_code))))]\npub fn badly( )->u8{1}\n",
    );
    let run = fixture.site(&out, &[]);
    run.expect_code(1);
    assert!(
        run.stdout
            .ends_with("site: FAILED: the gate failed, so no page was written\n"),
        "{}",
        run.stdout
    );
    assert!(!run.stdout.contains("running `cargo run"), "{}", run.stdout);
    fixture.source(
        "crates/codec/src/lib.rs",
        "//! Codec.\n#![doc(test(attr(forbid(unsafe_code))))]\n",
    );
    assert_eq!(page_files(&out), built);

    // An example that fails when it runs, after the gate passed: nothing
    // is written.
    fixture.source(
        "crates/text/examples/editing.rs",
        "//! An example.\n\nfn main() {\n    std::process::exit(3);\n}\n",
    );
    let run = fixture.site(&out, &[]);
    run.expect_code(1);
    assert!(
        run.stdout
            .contains("ci: ok (3 tests SKIPPED): 11 of 11 steps passed"),
        "{}",
        run.stdout
    );
    // The gate's cargo output comes first on standard error.
    assert!(
        run.stderr
            .contains("\nxtask: `cargo run --quiet -p tidepool-text --example editing` failed ("),
        "{}",
        run.stderr
    );
    assert_eq!(page_files(&out), built);
}

// --- the real repository and the command line ---

#[test]
fn the_real_repository_passes_deps_layers_unsafe_audit_and_headers() {
    // Not `ci`: its `cargo test` would run this test again, without end.
    // No build directory of its own is given: the checks only read
    // metadata, and must skip the one these tests run in when it is
    // `<root>/target` or outside the repository. Anywhere else in the
    // repository it is searched, and this test fails (docs/04-quality.md).
    for command in ["deps", "layers", "unsafe-audit", "headers"] {
        let run = xtask(&[command], None);
        run.expect_code(0);
        assert!(
            run.stdout.ends_with(&format!("{command}: ok\n")),
            "{}",
            run.stdout
        );
    }
}

#[test]
fn the_real_repository_s_crates_pass() {
    // Every member, its manifest and its README, and the roadmap's task
    // IDs, whose number changes as tasks are added.
    let run = xtask(&["crates"], None);
    run.expect_code(0);
    assert!(
        run.stdout
            .starts_with("members checked: 26, task IDs in docs/roadmap.md: "),
        "{}",
        run.stdout
    );
    assert!(run.stdout.ends_with("\ncrates: ok\n"), "{}", run.stdout);
    assert_eq!(run.stdout.lines().count(), 2, "{}", run.stdout);
}

#[test]
fn the_real_repository_s_workflows_pass() {
    // All three: `ci.yml`, `codeql.yml` and `pages.yml`, each with its row.
    let run = xtask(&["workflows"], None);
    run.expect_code(0);
    assert_eq!(run.stdout, "workflows checked: 3\nworkflows: ok\n");
}

#[test]
fn usage_errors_exit_2_and_later_commands_exit_1() {
    let run = xtask(&["frobnicate"], None);
    run.expect_code(2);
    assert!(
        run.stderr
            .starts_with("xtask: unknown command `frobnicate`\n\nUsage: cargo xtask"),
        "{}",
        run.stderr
    );
    assert_eq!(run.stdout, "");

    for (command, task) in [("tools", "roadmap M5.1"), ("theme", "roadmap M3.2")] {
        let run = xtask(&[command], None);
        run.expect_code(1);
        assert_eq!(
            run.stderr,
            format!("xtask: `{command}` is not yet implemented ({task})\n")
        );
        assert_eq!(run.stdout, "");
    }

    let run = xtask(&["--help"], None);
    run.expect_code(0);
    assert!(
        run.stdout
            .starts_with("Usage: cargo xtask <command> [--root DIR]\n")
    );
}
