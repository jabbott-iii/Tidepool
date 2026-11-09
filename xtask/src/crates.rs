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

//! `cargo xtask crates`: every member of the root workspace has the
//! manifest, the `README.md` and the copies of `LICENSE` and `NOTICE` that
//! ADR-0023 and ADR-0028 ask for.
//!
//! Written for Tidepool. The rule for the license copies is Nadir's, whose
//! own `crates` check fails a missing or differing copy (ADR-0028's
//! Context). The members are the root workspace's, as `cargo metadata
//! --offline --no-deps` lists them, and each is found in the directory of
//! its `Cargo.toml`. The check fails when:
//!
//! - a member's `Cargo.toml` does not inherit `edition`, `version` and
//!   `license` from the workspace: each must be a table whose only key is
//!   `workspace`, set to `true`, which is what both `edition.workspace =
//!   true` and `edition = { workspace = true }` are. A literal value, a
//!   missing key, `workspace = false` or another key beside it fails;
//! - it does not set `publish = false`: missing, `true`, a list of
//!   registries (`[]` included) and `publish.workspace = true` fail;
//! - its `[lints]` table is missing, or is anything but `workspace = true`;
//! - a target sets an `edition` of its own: any `edition` key in `[lib]`,
//!   or in a table of `[[bin]]`, `[[test]]`, `[[bench]]` or `[[example]]`.
//!   Cargo still accepts one, with a warning, and compiles that target under
//!   it, whatever the package inherits. One violation per target, named by
//!   its table and its `name` if it has one;
//! - the root `Cargo.toml` does not set `edition = "2024"` in
//!   `[workspace.package]`, the edition the members inherit;
//! - a member has no `README.md` beside its `Cargo.toml` (that name exactly),
//!   or it is a symbolic link, is reached through one, is not a regular
//!   file, is over 1 MiB or is not UTF-8;
//! - the README's first line, after one leading byte-order mark and one
//!   trailing CR, is not `# <package name>`;
//! - its sections are not [`SECTIONS`], each once and in that order. A
//!   section is a line `## Status`, `## Usage`, `## Documentation`,
//!   `## Rules`, `## Checks` or `## License` (exactly, after one trailing
//!   CR) that `layers`' reading of a document ([`matrix::classify`]) takes
//!   for text: outside fenced code blocks, HTML blocks and HTML comments as
//!   that reading finds them, line by line and at the top level, which is
//!   close to a renderer's but not the same (below). One violation for each
//!   section missing, each section there more than once, and each pair of
//!   sections that are neighbours in [`SECTIONS`] once those missing or
//!   there more than once are left out, and stand the other way round. So a
//!   section moved far away is one violation. A `## Features` line
//!   ([`FEATURES`]) may be there, once, after `## Usage` and before
//!   `## Documentation` (judged against those of the two there once): one
//!   violation if it is there more than once, one if it stands elsewhere.
//!   Other `## ` headings may stand anywhere;
//! - it names a task ID that `docs/roadmap.md` has no task for. A task ID is
//!   `M`, ASCII digits, `.` and ASCII digits, each run of digits as long as
//!   it goes, with no ASCII letter, digit or `_` just before the `M`: so
//!   `M1.23` is one ID, and `xM1.2` none. Each unknown ID is reported once
//!   per README, in numeric order;
//! - a member has no `LICENSE` or no `NOTICE` beside its `Cargo.toml`, or
//!   one of them is a symbolic link, is reached through one, is not a
//!   regular file, is over 1 MiB, is not UTF-8 or is not, byte for byte, the
//!   root's ([`LICENSE_FILES`]). One violation per copy. A member at the root
//!   is its own copy, and is not compared;
//! - the root's `LICENSE` or `NOTICE` is missing or cannot be read in the
//!   same way: one violation each, and the members' copies of that file are
//!   then not compared, only read;
//! - a README, a copy or the root's `LICENSE` or `NOTICE` cannot be read, or
//!   a manifest that cargo loaded cannot be read or parsed here (a link, over
//!   1 MiB, past this check's own [`LIMITS`] for [`tidepool_codec::toml`]):
//!   one violation naming it, and the check goes on with the other members.
//!   The root manifest is read once, also when the root is a member. A
//!   member whose manifest is not under the root fails too, unchecked.
//!
//! The tasks are the roadmap's lines that start `- [ ] **M<n>.<m> — ` or
//! `- [x] **M<n>.<m> — ` ([`task_line`], with which `features` reads them
//! too). When the roadmap cannot be read, or has no such line, the check
//! cannot run; nor can it when `cargo metadata` fails, as it does on a
//! manifest that is not TOML or holds a value cargo rejects, in the root or
//! any member. Every file is read as [`walk::read_unlinked`]
//! reads one, and is first checked to be a regular file, so that a FIFO is
//! never opened. A violation names a path, a package or target name, a
//! section or a task ID, never a line of a file: the first-line rule says
//! what the line should be, not what it is, and a copy that differs is
//! named, not shown.
//!
//! Not covered:
//!
//! - whether what a README calls built is built (ADR-0023's Consequences:
//!   the check verifies the shape and the task IDs, not the claims);
//! - the rest of ADR-0028's shape: the paragraphs and the layer line before
//!   `## Status`, what each section says, and whether a README's Rust
//!   examples are doc tests (`cargo test` runs them only where the crate's
//!   `src/lib.rs` includes the README);
//! - where that reading and a renderer differ, a section is hidden, and
//!   reported missing, or in one case seen though a browser hides it:
//!   - only a line that is itself a fence, at most three columns in, opens
//!     or closes one. A fence after `>` or a list marker on its line, or
//!     four or more columns in, is not seen. So when a list item opens its
//!     fence on the marker line (`- ```sh`), its closing fence opens one;
//!     and a fence that closes four or more columns in, or that its list
//!     item ends without closing, runs on. Each hides the sections after
//!     it;
//!   - every `<!--` outside a fence or an HTML block opens a comment that
//!     runs to the next `-->` after it, wherever the `<!--` is: in a code
//!     span, in indented code, after a backslash. `<!-->` and `<!--->` do
//!     not close. Each hides the sections up to the next `-->`;
//!   - every line whose first character after its indentation is `<`
//!     (other than a comment) opens an HTML block, as `layers` reads one,
//!     even where a renderer shows text, and a section in that block is not
//!     seen;
//!   - a `<!--` on a line of an HTML block is not followed, so a section
//!     after the block counts, though a browser shows it inside that
//!     comment;
//! - the members of `agents/`, a workspace of its own from roadmap M9.2;
//! - a file system that ignores case (macOS and Windows by default), where
//!   `readme.md` opens as `README.md`, and `license` as `LICENSE`;
//! - races, as for every check ("Files searched" in `docs/04-quality.md`).

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use tidepool_codec::Limits;
use tidepool_codec::toml::{self, Table, Value};

use crate::check::Report;
use crate::error::Error;
use crate::matrix;
use crate::metadata::{self, Metadata, Resolve};
use crate::walk;

/// The roadmap, whose task IDs are the only ones a README may name.
pub(crate) const ROADMAP: &str = "docs/roadmap.md";

/// A member's manifest, and the root workspace's.
const MANIFEST: &str = "Cargo.toml";

/// A member's README, beside its manifest.
const README: &str = "README.md";

/// The largest roadmap read, in bytes.
pub(crate) const MAX_ROADMAP_BYTES: u64 = 4 * 1024 * 1024;

/// The largest manifest read, in bytes.
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

/// The largest README read, in bytes.
const MAX_README_BYTES: u64 = 1024 * 1024;

/// What a manifest may hold for `tidepool_codec::toml` to read it: a whole
/// file of [`MAX_MANIFEST_BYTES`], nested far deeper than any manifest
/// (`target.'cfg(…)'.dependencies.name.features` is 5 deep).
const LIMITS: Limits = Limits {
    max_input_bytes: MAX_MANIFEST_BYTES as usize,
    max_depth: 32,
    max_string_bytes: 64 * 1024,
    max_entries: 10_000,
    ..Limits::DEFAULT
};

/// The edition the root workspace sets and every member inherits.
const EDITION: &str = "2024";

/// The sections every README has, each a line of its own, once, in this
/// order (ADR-0028).
const SECTIONS: [&str; 6] = [
    "## Status",
    USAGE,
    DOCUMENTATION,
    "## Rules",
    "## Checks",
    "## License",
];

/// The section [`FEATURES`] comes after.
const USAGE: &str = "## Usage";

/// The section [`FEATURES`] comes before.
const DOCUMENTATION: &str = "## Documentation";

/// The section a README has only when its crate has Cargo features: once,
/// between [`USAGE`] and [`DOCUMENTATION`].
const FEATURES: &str = "## Features";

/// The files every member keeps beside its manifest, each byte for byte the
/// root's (ADR-0028).
const LICENSE_FILES: [&str; 2] = ["LICENSE", "NOTICE"];

/// The largest `LICENSE` or `NOTICE` read, in bytes: Apache-2.0's is 11 KiB.
const MAX_LICENSE_BYTES: u64 = 1024 * 1024;

/// Runs the check on the repository at `root`.
///
/// # Errors
///
/// [`Error::Read`] if [`ROADMAP`] cannot be read, [`Error::Doc`] if it has
/// no task, and the errors of [`metadata::load`].
pub(crate) fn run(root: &Path) -> Result<Report, Error> {
    let roadmap = read_file(root, ROADMAP, MAX_ROADMAP_BYTES)?;
    let tasks = roadmap_tasks(&roadmap)?;
    let metadata = metadata::load(root, &root.join(MANIFEST), Resolve::No)?;
    Ok(check(root, &metadata, &tasks))
}

/// Checks the root manifest, the root's `LICENSE` and `NOTICE`, and the
/// manifest, README and copies of each member of `metadata`, the root
/// workspace of the repository at `root`, against the roadmap's `tasks`.
fn check(root: &Path, metadata: &Metadata, tasks: &BTreeSet<String>) -> Report {
    let mut violations = Vec::new();
    // Read once: a member at the root has this manifest too, and a problem
    // with it is reported once.
    let root_manifest = load(root, MANIFEST);
    match &root_manifest {
        Ok(document) => violations.extend(workspace_rules(&shown(root, MANIFEST), document)),
        Err(problem) => violations.push(problem.clone()),
    }
    // What each member's copies must be: `None` for a file the root does
    // not have, or that cannot be read, which is reported here, once.
    let mut originals: Vec<(&str, Option<String>)> = Vec::new();
    for name in LICENSE_FILES {
        match read_file(root, name, MAX_LICENSE_BYTES) {
            Ok(text) => originals.push((name, Some(text))),
            Err(error) => {
                violations.push(root_file_problem(root, name, &error));
                originals.push((name, None));
            }
        }
    }

    // Each member's directory relative to the root, with `/` between
    // components, and its name.
    let base = lexical(root);
    let mut members: Vec<(String, &str)> = Vec::new();
    for package in metadata.packages.iter().filter(|package| package.is_member) {
        match member_dir(&base, &package.manifest_path) {
            Some(dir) => members.push((dir, package.name.as_str())),
            None => violations.push(format!(
                "workspace member `{}` has its manifest at {}, which is not under the root, so it \
                 was not checked",
                package.name,
                walk::shown(&package.manifest_path)
            )),
        }
    }
    members.sort_unstable();

    for (dir, name) in &members {
        let manifest = beside(dir, MANIFEST);
        let shown_manifest = shown(root, &manifest);
        if dir.is_empty() {
            if let Ok(document) = &root_manifest {
                violations.extend(member_rules(&shown_manifest, document));
            }
        } else {
            match load(root, &manifest) {
                Ok(document) => violations.extend(member_rules(&shown_manifest, &document)),
                Err(problem) => violations.push(problem),
            }
        }
        let readme = beside(dir, README);
        match read_file(root, &readme, MAX_README_BYTES) {
            Ok(text) => {
                violations.extend(readme_violations(&shown(root, &readme), name, &text, tasks));
            }
            Err(error) => violations.push(unreadable(root, &readme, &error)),
        }
        // A member at the root is its own copy.
        if !dir.is_empty() {
            violations.extend(copy_violations(root, dir, &originals));
        }
    }

    let mut report = Report {
        notes: vec![format!(
            "members checked: {}, task IDs in {ROADMAP}: {}",
            members.len(),
            tasks.len()
        )],
        violations,
        ..Report::default()
    };
    if !report.violations.is_empty() {
        report.notes.push(
            "ADR-0023 and ADR-0028 (docs/decisions.md) give the shape of a member's manifest, \
             README and copies of LICENSE and NOTICE, and docs/04-quality.md (`crates`) the rules \
             this check applies."
                .to_string(),
        );
    }
    report
}

/// The violation for the root's `name` (`LICENSE` or `NOTICE`), which
/// [`read_file`] could not read: one for a missing file, else
/// [`unreadable`]'s.
fn root_file_problem(root: &Path, name: &str, error: &Error) -> String {
    match error {
        Error::Read { source, .. } if source.kind() == io::ErrorKind::NotFound => {
            format!("the workspace has no {name} at its root")
        }
        other => unreadable(root, name, other),
    }
}

/// The violations of the copies of `LICENSE` and `NOTICE` in the member
/// directory `dir`, against the root's `originals`: one for each copy that
/// cannot be read ([`unreadable`]), and one for each that is not byte for
/// byte the root's, where the root's could be read. Neither file is quoted.
fn copy_violations(root: &Path, dir: &str, originals: &[(&str, Option<String>)]) -> Vec<String> {
    let mut violations = Vec::new();
    for (name, original) in originals {
        let copy = beside(dir, name);
        match read_file(root, &copy, MAX_LICENSE_BYTES) {
            Ok(text) => {
                if original.as_ref().is_some_and(|original| *original != text) {
                    violations.push(format!(
                        "{} is not byte for byte the root's {name}: copy the root's {name} over it",
                        shown(root, &copy)
                    ));
                }
            }
            Err(error) => violations.push(unreadable(root, &copy, &error)),
        }
    }
    violations
}

/// `path` with `.` left out and each `..` taking away the component before
/// it, without asking the file system: as cargo writes the paths of the
/// members (`--root ..` names the repository as `xtask/..`, which cargo
/// reports as the repository itself). Links are not resolved.
fn lexical(path: &Path) -> PathBuf {
    let mut normal = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normal.pop();
            }
            other => normal.push(other),
        }
    }
    normal
}

/// The directory of the member whose manifest is at `manifest`, relative to
/// `base` (the root, [`lexical`]) with `/` between components, or `None` if
/// the manifest is not under it.
fn member_dir(base: &Path, manifest: &Path) -> Option<String> {
    let manifest = lexical(manifest);
    let inside = manifest.strip_prefix(base).ok()?;
    let dir = inside.parent()?;
    let parts: Vec<String> = dir
        .components()
        .map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Option<_>>()?;
    Some(parts.join("/"))
}

/// The file `name` in the directory `dir`, both relative to the root.
fn beside(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

/// `rel`, relative to `root`, as a message shows it ([`walk::relative`]).
fn shown(root: &Path, rel: &str) -> String {
    walk::relative(root, &root.join(rel))
}

/// Reads the file at `rel`, relative to `root` with `/` between components,
/// as [`walk::read_unlinked`] does: no symbolic link on the way or at the
/// end, at most `limit` bytes, UTF-8. First it must be a regular file, so
/// that a FIFO or a device there is never opened.
///
/// # Errors
///
/// [`Error::Read`], naming the file: the OS error for one that is missing
/// (or a directory on the way), and otherwise what is wrong.
pub(crate) fn read_file(root: &Path, rel: &str, limit: u64) -> Result<String, Error> {
    let path = root.join(rel);
    let parts: Vec<&str> = rel.split('/').collect();
    let mut at = root.to_path_buf();
    for (index, part) in parts.iter().enumerate() {
        at.push(part);
        let meta = match fs::symlink_metadata(&at) {
            Ok(meta) => meta,
            Err(source) => return Err(Error::Read { path, source }),
        };
        // `read_unlinked` names a link, without opening it.
        if meta.file_type().is_symlink() {
            break;
        }
        if index + 1 == parts.len() && !meta.is_file() {
            return Err(Error::Read {
                path,
                source: io::Error::other("it is not a regular file"),
            });
        }
    }
    walk::read_unlinked(root, rel, limit)
}

/// The violation for the file `rel` that [`read_file`] could not read.
fn unreadable(root: &Path, rel: &str, error: &Error) -> String {
    let shown = shown(root, rel);
    match error {
        Error::Read { source, .. } if source.kind() == io::ErrorKind::NotFound => {
            format!("{shown} is missing")
        }
        Error::Read { source, .. } => format!("{shown} cannot be read: {source}"),
        other => format!("{shown} cannot be read: {other}"),
    }
}

/// Parses the manifest `text`, shown as `shown`, or gives the violation
/// that says why it cannot be parsed.
fn parse(shown: &str, text: &str) -> Result<Table, String> {
    toml::parse(text, LIMITS).map_err(|error| format!("{shown} cannot be parsed: {error}"))
}

/// Reads and parses the manifest at `rel`, or gives the one violation that
/// says why it cannot be ([`unreadable`], [`parse`]).
fn load(root: &Path, rel: &str) -> Result<Table, String> {
    let text =
        read_file(root, rel, MAX_MANIFEST_BYTES).map_err(|error| unreadable(root, rel, &error))?;
    parse(&shown(root, rel), &text)
}

/// Whether `value` is a table whose only key is `workspace`, set to `true`:
/// what `key.workspace = true` and `key = { workspace = true }` both are.
fn inherits(value: &Value) -> bool {
    value.as_table().is_some_and(|table| {
        table.len() == 1 && table.get("workspace").and_then(Value::as_bool) == Some(true)
    })
}

/// The violations of the root workspace's manifest `document`, shown as
/// `shown`: its `[workspace.package]` must set the edition 2024.
fn workspace_rules(shown: &str, document: &Table) -> Vec<String> {
    let edition = document
        .get("workspace")
        .and_then(|workspace| workspace.get("package"))
        .and_then(|package| package.get("edition"));
    let problem = match edition {
        None => "has no `edition` in `[workspace.package]`".to_string(),
        Some(edition) if edition.as_str() == Some(EDITION) => return Vec::new(),
        Some(_) => {
            format!("sets `workspace.package.edition` to something other than \"{EDITION}\"")
        }
    };
    vec![format!(
        "{shown} {problem}: write `edition = \"{EDITION}\"` in `[workspace.package]`, the edition \
         every member inherits"
    )]
}

/// The violations of a member's manifest `document`, shown as `shown`, one
/// for each rule it breaks.
fn member_rules(shown: &str, document: &Table) -> Vec<String> {
    let mut violations = Vec::new();
    match document.get("package").and_then(Value::as_table) {
        None => violations.push(format!(
            "{shown} has no `[package]` table, so it cannot inherit `edition`, `version` and \
             `license` or set `publish = false`"
        )),
        Some(package) => {
            for key in ["edition", "version", "license"] {
                match package.get(key) {
                    Some(value) if inherits(value) => {}
                    None => violations.push(format!(
                        "{shown} has no `package.{key}`: write `{key}.workspace = true` in \
                         `[package]`, to inherit it from the workspace"
                    )),
                    Some(_) => violations.push(format!(
                        "{shown} does not inherit `package.{key}` from the workspace: write \
                         `{key}.workspace = true` in `[package]`, and nothing else for `{key}`"
                    )),
                }
            }
            let problem = match package.get("publish") {
                None => Some("has no `package.publish`"),
                Some(publish) if publish.as_bool() == Some(false) => None,
                Some(_) => Some("sets `package.publish` to something other than `false`"),
            };
            if let Some(problem) = problem {
                violations.push(format!(
                    "{shown} {problem}: write `publish = false` in `[package]`, since no member is \
                     published"
                ));
            }
        }
    }
    let problem = match document.get("lints") {
        None => Some("has no `[lints]` table"),
        Some(lints) if inherits(lints) => None,
        Some(_) => Some("does not take its lints only from the workspace"),
    };
    if let Some(problem) = problem {
        violations.push(format!(
            "{shown} {problem}: write `[lints]` with `workspace = true` and nothing else in it"
        ));
    }
    violations.extend(target_editions(shown, document));
    violations
}

/// The tables of a manifest that each describe a target, and how a message
/// writes them: `[lib]` is one table, the others arrays of tables.
const TARGETS: [(&str, &str); 5] = [
    ("lib", "[lib]"),
    ("bin", "[[bin]]"),
    ("test", "[[test]]"),
    ("bench", "[[bench]]"),
    ("example", "[[example]]"),
];

/// One violation for each target of the manifest `document` that sets an
/// `edition` of its own, which cargo still accepts (with a warning) and
/// compiles that target under, whatever the package inherits. A target is
/// named by its table and its `name`, never by the edition it sets.
fn target_editions(shown: &str, document: &Table) -> Vec<String> {
    let mut violations = Vec::new();
    for (key, written) in TARGETS {
        let targets: Vec<&Table> = match document.get(key) {
            None => continue,
            Some(value) => match (value.as_table(), value.as_array()) {
                (Some(table), _) => vec![table],
                (None, Some(array)) => array.items().iter().filter_map(Value::as_table).collect(),
                (None, None) => continue,
            },
        };
        for target in targets {
            if !target.contains_key("edition") {
                continue;
            }
            let which = match target.get("name").and_then(Value::as_str) {
                Some(name) => format!("the `{written}` target `{name}`"),
                None if key == "lib" => format!("the `{written}` target"),
                None => format!("a `{written}` target with no `name`"),
            };
            violations.push(format!(
                "{shown}: {which} sets an `edition` of its own: remove it, so that the target \
                 uses the package's edition, which it inherits from the workspace"
            ));
        }
    }
    violations
}

/// The violations of the README `text` of the member `name`, shown as
/// `shown`, against the roadmap's `tasks`.
fn readme_violations(shown: &str, name: &str, text: &str, tasks: &BTreeSet<String>) -> Vec<String> {
    let mut violations = Vec::new();
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lines: Vec<&str> = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect();
    let heading = format!("# {name}");
    if lines.first() != Some(&heading.as_str()) {
        violations.push(format!(
            "{shown}: its first line is not `{heading}`, the package name as a heading"
        ));
    }
    violations.extend(section_violations(shown, &sections(&lines)));
    for id in unknown_ids(text, tasks) {
        violations.push(format!(
            "{shown} names the task {id}, which {ROADMAP} does not have"
        ));
    }
    violations
}

/// The lines among `lines` (each without its line ending) that are one of
/// [`SECTIONS`] or [`FEATURES`] and that a reader sees as text, as `layers`
/// reads its document ([`matrix::classify`]): not in a fenced code block, an
/// HTML block or an HTML comment. In the order they stand, repeats included.
fn sections(lines: &[&str]) -> Vec<&'static str> {
    matrix::classify(lines)
        .iter()
        .zip(lines)
        .filter(|((kind, _), _)| *kind == matrix::Kind::Shown)
        .filter_map(|(_, &line)| {
            SECTIONS
                .iter()
                .chain([&FEATURES])
                .find(|&&section| section == line)
                .copied()
        })
        .collect()
}

/// The violations of a README, shown as `shown`, whose [`sections`] are
/// `found`, in this order: each of [`SECTIONS`] that is missing or there
/// more than once, in that order; [`FEATURES`] there more than once; each
/// pair of neighbours in [`SECTIONS`], once those not there exactly once
/// are left out, that stand the other way round; and [`FEATURES`], there
/// once, before [`USAGE`] or after [`DOCUMENTATION`], where that one is
/// there once.
fn section_violations(shown: &str, found: &[&str]) -> Vec<String> {
    let mut violations = Vec::new();
    let count = |section: &str| found.iter().filter(|&&line| line == section).count();
    // Where `section` stands, if it is there once.
    let once = |section: &str| {
        (count(section) == 1)
            .then(|| found.iter().position(|&line| line == section))
            .flatten()
    };
    for section in SECTIONS.iter().chain([&FEATURES]) {
        match count(section) {
            0 if *section != FEATURES => violations.push(format!(
                "{shown} has no `{section}` line outside code blocks, HTML blocks and comments"
            )),
            0 | 1 => {}
            _ => violations.push(format!(
                "{shown} has more than one `{section}` line outside code blocks, HTML blocks and \
                 comments: a README has each section once"
            )),
        }
    }
    let placed: Vec<(&str, usize)> = SECTIONS
        .iter()
        .filter_map(|&section| Some((section, once(section)?)))
        .collect();
    for pair in placed.windows(2) {
        if let [(first, at), (then, then_at)] = pair
            && at > then_at
        {
            violations.push(format!(
                "{shown}: `{first}` must come before `{then}`, but comes after it"
            ));
        }
    }
    if let Some(at) = once(FEATURES) {
        let early = once(USAGE).is_some_and(|usage| at < usage);
        let late = once(DOCUMENTATION).is_some_and(|documentation| at > documentation);
        if early || late {
            violations.push(format!(
                "{shown}: `{FEATURES}` may stand only between `{USAGE}` and `{DOCUMENTATION}`"
            ));
        }
    }
    violations
}

/// The task ID that `text` starts with: `M`, one or more ASCII digits, `.`
/// and one or more ASCII digits, each run as long as it goes.
fn leading_id(text: &str) -> Option<&str> {
    let digits = |from: usize| {
        text.as_bytes().get(from..).map_or(0, |rest| {
            rest.iter().take_while(|b| b.is_ascii_digit()).count()
        })
    };
    if !text.starts_with('M') {
        return None;
    }
    let major = digits(1);
    if major == 0 || text.as_bytes().get(1 + major) != Some(&b'.') {
        return None;
    }
    let minor = digits(2 + major);
    if minor == 0 {
        return None;
    }
    text.get(..2 + major + minor)
}

/// Every task ID `text` names, in order, repeats included: a
/// [`leading_id`] whose `M` has no ASCII letter, digit or `_` just before
/// it.
fn task_ids(text: &str) -> Vec<&str> {
    let mut ids = Vec::new();
    let mut from = 0;
    while let Some(offset) = text.get(from..).and_then(|rest| rest.find('M')) {
        let at = from + offset;
        from = at + 1;
        let joined = at
            .checked_sub(1)
            .and_then(|before| text.as_bytes().get(before))
            .is_some_and(|&b| b.is_ascii_alphanumeric() || b == b'_');
        if joined {
            continue;
        }
        if let Some(id) = text.get(at..).and_then(leading_id) {
            ids.push(id);
            from = at + id.len();
        }
    }
    ids
}

/// How task IDs are ordered in messages: by number, `M1.9` before `M1.10`.
fn numeric_order(id: &str) -> (usize, &str, usize, &str, &str) {
    let (major, minor) = id
        .get(1..)
        .and_then(|rest| rest.split_once('.'))
        .unwrap_or((id, ""));
    let major = major.trim_start_matches('0');
    let minor = minor.trim_start_matches('0');
    (major.len(), major, minor.len(), minor, id)
}

/// The task IDs `text` names that are not among `tasks`, each once, in
/// [`numeric_order`].
fn unknown_ids<'t>(text: &'t str, tasks: &BTreeSet<String>) -> Vec<&'t str> {
    let mut unknown: Vec<&str> = task_ids(text)
        .into_iter()
        .filter(|id| !tasks.contains(*id))
        .collect();
    unknown.sort_unstable_by(|a, b| numeric_order(a).cmp(&numeric_order(b)));
    unknown.dedup();
    unknown
}

/// The lines of the roadmap `text`, each without its LF or CR LF ending: as
/// [`roadmap_tasks`] and `features` read them. A lone CR does not end a
/// line here.
pub(crate) fn roadmap_lines(text: &str) -> impl Iterator<Item = &str> {
    text.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
}

/// A task of the roadmap, as [`task_line`] reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TaskLine<'a> {
    /// Its ID, such as `M1.3`.
    pub(crate) id: &'a str,
    /// Its title: what follows the ID's ` — `, up to the next `**`, or to
    /// the end of the line if no `**` follows.
    pub(crate) title: &'a str,
    /// Whether its box is ticked (`[x]`).
    pub(crate) done: bool,
}

/// The task on `line` of the roadmap ([`roadmap_lines`]), if it starts
/// `- [ ] **M<n>.<m> — ` or `- [x] **M<n>.<m> — `, where the ID is a
/// [`leading_id`]: the reading of a task line that `crates` and `features`
/// share.
pub(crate) fn task_line(line: &str) -> Option<TaskLine<'_>> {
    let (done, rest) = match line.strip_prefix("- [ ] **") {
        Some(rest) => (false, rest),
        None => (true, line.strip_prefix("- [x] **")?),
    };
    let id = leading_id(rest)?;
    let title = rest.get(id.len()..)?.strip_prefix(" \u{2014} ")?;
    let title = title.split_once("**").map_or(title, |(title, _)| title);
    Some(TaskLine { id, title, done })
}

/// The task IDs of the roadmap `text`: those of its lines that start
/// `- [ ] **M<n>.<m> — ` or `- [x] **M<n>.<m> — ` (with LF or CR LF
/// endings), as [`task_line`] reads them.
///
/// # Errors
///
/// [`Error::Doc`] if it has none: then no README's task IDs can be checked.
fn roadmap_tasks(text: &str) -> Result<BTreeSet<String>, Error> {
    let tasks: BTreeSet<String> = roadmap_lines(text)
        .filter_map(task_line)
        .map(|task| task.id.to_string())
        .collect();
    if tasks.is_empty() {
        return Err(Error::Doc {
            path: ROADMAP.to_string(),
            line: 0,
            detail: "no line starts `- [ ] **M<n>.<m> — ` or `- [x] **M<n>.<m> — `, so there is \
                     no task for a README to name; is the root right?"
                .to_string(),
        });
    }
    Ok(tasks)
}

#[cfg(test)]
mod tests;
