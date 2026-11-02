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

//! `cargo xtask deps`: no third-party code, unless the user approved it.
//!
//! Ported from nadir xtask `deps.rs`. Adapted for Tidepool: every Cargo
//! workspace in the repository is checked (`agents/` is a workspace of its
//! own), its graph is resolved offline from its lockfile as it is, a path
//! package must be a member of the workspace whose graph it is in (so
//! vendored code cannot hide as a path package, and the std-only reference
//! agents in `agents/` cannot use `crates/`: `docs/subsystems/agents.md`),
//! declared dependencies are checked as well as resolved ones, and every
//! `Cargo.lock` is read as text too.
//!
//! The policy is `docs/02-dependency-policy.md`; the check is specified in
//! `docs/04-quality.md`. It fails when:
//!
//! - a package in a workspace's resolved graph has a non-null `source` (a
//!   registry or git) and is not approved: its exact name and version are
//!   not in `xtask/deps-allowlist.txt`, or it does not come from crates.io;
//! - a workspace member declares a dependency with a non-null `source`
//!   that is not approved, or without the exact requirement `=version` the
//!   policy asks for. This also catches a crates.io dependency that a
//!   `[patch]`, `[replace]` or `[source]` replacement serves from a
//!   directory;
//! - a path package (`source` null) is outside the repository, or is in it
//!   but is not a member of the workspace whose graph it is in: vendored,
//!   copied or cross-repository code, or another workspace's crate (the
//!   root workspace's included), listed or not, even when it declares a
//!   `[workspace]` of its own;
//! - a `Cargo.lock` anywhere in the repository has a `source = ` entry for
//!   a package that is not approved (with the allowlist empty, any `source`
//!   line at all; a quoted `"source"` or `'source'` key counts as one), or
//!   a `source` key in a form cargo never writes (dotted, in an inline
//!   table, or quoted with an escape);
//! - cargo cannot resolve a workspace offline with its lockfile as it is
//!   (`--offline --locked`): the check fails closed, since an unresolvable
//!   graph proves nothing. What its members declare is still checked when
//!   cargo can list them (`--no-deps`): a registry, git or outside path
//!   dependency is reported as itself. A lockfile that is merely out of
//!   date (as it is right after a manifest edit), with nothing else found,
//!   gets its own message, saying how to update it without the network or
//!   compiling anything. When every violation of a run is such a lockfile,
//!   the last line ends `; only out-of-date lockfiles)`
//!   ([`check::ONLY_STALE_LOCKFILES`](crate::check::ONLY_STALE_LOCKFILES)),
//!   which the Claude Code hook reads instead of any message's text;
//! - a workspace other than the root one and `agents/` exists, or a member
//!   of `agents/` is not one of the reference agents in `agents/<name>/`
//!   (`docs/subsystems/agents.md`): vendored or copied code cannot hide as
//!   the member of a workspace that `layers`, which checks the root
//!   workspace only, never reads;
//! - a cargo configuration file in the repository (`.cargo/config` or
//!   `.cargo/config.toml`, in any directory, since cargo reads those above
//!   the directory it runs in) holds anything but `[alias]` with one-line
//!   string values: `[env]` could set `CARGO_TARGET_DIR` for `xtask` itself,
//!   and `[build]`, `[target]`, `[source]`, `[patch]` or `paths` could
//!   change what cargo builds or where its code comes from without any
//!   manifest saying so;
//! - the repository holds a compiled artifact: a file named like one
//!   (`.rlib`, `.rmeta`, `.a`, `.o`, `.obj`, `.so`, `.dylib`, `.dll`,
//!   `.lib`, `.exe`, `.wasm`) or starting like one (ELF, Mach-O, PE, an `ar`
//!   archive, rustc metadata, WebAssembly, LLVM bitcode), which a build
//!   script's `cargo:rustc-link-search` could link in without any manifest
//!   naming it. A file whose name says it may hold a secret (`.env`,
//!   `.pem`, `.p12`, a keystore...) is judged by its name only and never
//!   opened, since `xtask` never reads secrets. Any other file that cannot
//!   be read is a violation too, since it was not checked;
//! - a file named like a `Cargo.toml` or `Cargo.lock` (in any case: macOS
//!   and Windows ignore it), like a cargo configuration file (`config` or
//!   `config.toml`, in any directory) or like a compiled artifact is a
//!   symbolic link, or a directory is reached through one: the check does
//!   not follow links;
//! - the allowlist has a malformed line, or the repository has no
//!   `Cargo.toml` at all. A cargo configuration file or the allowlist may
//!   start with one byte-order mark, which cargo skips too.
//!
//! Only the repository's `.git`, workspace roots' build output and linked
//! worktrees of the repository are skipped ([`walk::scan`]): a workspace in
//! a hidden directory is checked too.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::cargo::without_userinfo;
use crate::check::Report;
use crate::error::Error;
use crate::metadata::{self, CRATES_IO, Dependency, Metadata, Package, Resolve};
use crate::walk;

/// The allowlist, relative to the repository root.
pub(crate) const ALLOWLIST: &str = "xtask/deps-allowlist.txt";

/// The largest allowlist, lockfile or cargo configuration file read, in
/// bytes.
const MAX_TEXT_BYTES: u64 = 16 * 1024 * 1024;

/// The directory of the second workspace, relative to the repository root
/// (`docs/01-architecture.md`).
const AGENTS: &str = "agents";

/// The members `agents/` may have, each in `agents/<name>/`: the reference
/// agents of `docs/subsystems/agents.md`.
const REFERENCE_AGENTS: [&str; 3] = ["tp-agent-mock", "tp-agent-local", "tp-agent-claude"];

/// The extensions, compared without case, of the files a compiler or
/// linker reads as compiled code.
const ARTIFACT_EXTENSIONS: [&str; 11] = [
    "rlib", "rmeta", "a", "o", "obj", "so", "dylib", "dll", "lib", "exe", "wasm",
];

/// How many bytes of each file are read to recognise a compiled artifact.
const MAGIC_BYTES: u64 = 1024;

/// One approved package: a line of the allowlist. It approves that name
/// and version from crates.io only.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Pin {
    /// The package name.
    pub(crate) name: String,
    /// The exact version.
    pub(crate) version: String,
}

/// Runs the check on the repository at `root`.
///
/// # Errors
///
/// [`Error::Read`] if the allowlist, a directory, a lockfile or a cargo
/// configuration file cannot be read. A workspace cargo cannot describe,
/// and any other file that cannot be read for the compiled-artifact check,
/// is a violation, not an error.
pub(crate) fn run(root: &Path) -> Result<Report, Error> {
    let allowlist = walk::read_required(&root.join(ALLOWLIST), MAX_TEXT_BYTES)?;
    let mut report = Report::default();
    let pins = parse_allowlist(&allowlist, &mut report.violations);
    let repository = fs::canonicalize(root).map_err(|source| Error::Read {
        path: root.to_path_buf(),
        source,
    })?;

    let tree = walk::scan_every_file(root, |name| {
        metadata::is_manifest_name(name)
            || metadata::is_lockfile_name(name)
            || is_config_name(name)
            || artifact_extension(name).is_some()
    })?;
    for link in &tree.links {
        report.violations.push(format!(
            "{} is a symbolic link, which this check does not follow: what it leads to was not \
             checked",
            walk::relative(root, link)
        ));
    }
    let mut manifests = Vec::new();
    let mut lockfiles = Vec::new();
    let mut configs = Vec::new();
    for path in &tree.files {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if metadata::is_manifest_name(&name) {
            manifests.push(path.clone());
        } else if metadata::is_lockfile_name(&name) {
            lockfiles.push(path.clone());
        } else if is_cargo_config(path) {
            configs.push(path.clone());
        }
        report
            .violations
            .extend(artifact_violation(root, path, &name));
    }
    if manifests.is_empty() {
        report
            .violations
            .push("no Cargo.toml under the repository root: nothing was checked".to_string());
    }
    for config in &configs {
        let text = walk::read_required(config, MAX_TEXT_BYTES)?;
        check_cargo_config(&walk::relative(root, config), &text, &mut report.violations);
    }

    let workspaces = metadata::workspaces(root, &manifests, Resolve::Yes);
    let repo = Repository {
        root: &repository,
        locate: real_path,
    };
    let mut counts = Counts::default();
    for workspace in &workspaces {
        let described = workspace
            .graph
            .as_ref()
            .ok()
            .or(workspace.declared.as_ref());
        if let Some(described) = described {
            check_workspace(&workspace.shown, described, &repo, &mut report.violations);
        }
        match (&workspace.graph, &workspace.declared) {
            (Ok(graph), _) => check_graph(graph, &pins, &repo, &mut report, &mut counts),
            (Err(error), declared) => {
                let before = report.violations.len();
                if let Some(declared) = declared {
                    check_declared(declared, &pins, &repo, &mut report.violations);
                }
                let found_more = report.violations.len() > before;
                match unresolved(&workspace.shown, error, found_more) {
                    (Unresolved::StaleLockfile, violation) => report.push_stale_lockfile(violation),
                    (Unresolved::Other, violation) => report.violations.push(violation),
                }
            }
        }
    }

    for lockfile in &lockfiles {
        let text = walk::read_required(lockfile, MAX_TEXT_BYTES)?;
        check_lockfile(
            &walk::relative(root, lockfile),
            &text,
            &pins,
            &mut report.violations,
        );
    }

    let shown: Vec<&str> = workspaces
        .iter()
        .map(|workspace| workspace.shown.as_str())
        .collect();
    report.notes.push(format!(
        "workspaces: {} ({}); lockfiles: {}",
        workspaces.len(),
        shown.join(", "),
        lockfiles.len()
    ));
    report.notes.push(format!(
        "packages: {} workspace members, {} other path packages in the repository, {} from \
         outside it",
        counts.members, counts.loose, counts.external
    ));
    report
        .notes
        .extend(walk::worktrees_note(root, &tree.worktrees));
    report.notes.extend(walk::build_dir_note(root, &tree));
    if !report.violations.is_empty() {
        report.notes.push(
            "A third-party crate needs the user's approval first: see \
             docs/02-dependency-policy.md."
                .to_string(),
        );
    }
    Ok(report)
}

/// Why cargo could not resolve a workspace offline with its lockfile as it
/// is, as far as the check can tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Unresolved {
    /// Its lockfile is only out of date, and what its members declare
    /// passed: the graph was not checked, but nothing was found wrong.
    StaleLockfile,
    /// Anything else, or a stale lockfile alongside other violations.
    Other,
}

/// The kind of violation, and its message, for a workspace that cargo
/// could not resolve offline with its lockfile as it is. `found_more` says
/// whether checking what its members declare found violations of its own:
/// then the lockfile is not the only problem, and neither the kind nor the
/// message says it is.
fn unresolved(shown: &str, error: &Error, found_more: bool) -> (Unresolved, String) {
    let stale = !found_more
        && matches!(
            error,
            Error::CommandFailed { stderr, .. } if is_stale_lockfile(stderr)
        );
    let kind = if stale {
        Unresolved::StaleLockfile
    } else {
        Unresolved::Other
    };
    (kind, unresolved_message(shown, kind, error))
}

/// Whether cargo's standard error says only that the lockfile is out of
/// date, as it is right after a manifest edit. Cargo has worded this two
/// ways: "the lock file X needs to be updated but --locked was passed to
/// prevent this" (older releases) and "cannot update the lock file X
/// because --locked was passed to prevent this" (the pinned 1.99).
fn is_stale_lockfile(stderr: &str) -> bool {
    stderr.lines().any(|line| {
        line.starts_with("error: ")
            && line.contains("lock file")
            && line.contains("--locked was passed to prevent this")
    })
}

/// The message of [`unresolved`], once its kind is known. A lockfile that
/// is merely out of date still fails, since the graph was not checked (the
/// update may bring in a third-party package, which the next run then
/// reports), but the message says how to update it without the network or
/// compiling anything.
fn unresolved_message(shown: &str, kind: Unresolved, error: &dyn std::fmt::Display) -> String {
    if kind == Unresolved::StaleLockfile {
        format!(
            "cannot resolve the workspace of {shown} offline from its lockfile as it is: its \
             Cargo.lock is out of date, so the graph was not checked. Update the lockfile \
             offline, which compiles nothing (`cargo metadata --offline --format-version 1 \
             --manifest-path {shown}`), then run `cargo xtask deps` again: {error}"
        )
    } else {
        format!(
            "cannot resolve the workspace of {shown} offline from its lockfile as it is (an \
             external dependency, a package that belongs to no workspace, or a Cargo.lock that \
             needs updating): {error}"
        )
    }
}

/// How many packages of each kind the resolved graphs hold.
#[derive(Debug, Default, PartialEq, Eq)]
struct Counts {
    /// Members of the workspace whose graph it is.
    members: usize,
    /// Other path packages in the repository: violations. These include
    /// the crates of another workspace in the repository, the root one too.
    loose: usize,
    /// Packages from a registry or git, or from a path outside the
    /// repository.
    external: usize,
}

/// What a graph is checked against.
struct Repository<'a> {
    /// The canonical repository root.
    root: &'a Path,
    /// Resolves a manifest's real path (`fs::canonicalize` outside tests);
    /// a manifest it cannot resolve fails the check.
    locate: fn(&Path) -> std::io::Result<PathBuf>,
}

/// `path` absolute, with symbolic links and `..` resolved.
fn real_path(path: &Path) -> std::io::Result<PathBuf> {
    fs::canonicalize(path)
}

/// Checks that the workspace `metadata` describes is one the repository may
/// have: the root one, or `agents/` with only the reference agents, each in
/// `agents/<name>/`. `shown` names the manifest it was found from.
fn check_workspace(
    shown: &str,
    metadata: &Metadata,
    repository: &Repository<'_>,
    violations: &mut Vec<String>,
) {
    let agents = repository.root.join(AGENTS);
    let root = (repository.locate)(&metadata.workspace_root);
    let shown_root = match &root {
        Ok(root) => walk::relative(repository.root, root),
        Err(_) => metadata.workspace_root.display().to_string(),
    };
    let is_agents = match root {
        Ok(root) if root == repository.root => return,
        Ok(root) => root == agents,
        Err(_) => false,
    };
    if !is_agents {
        violations.push(format!(
            "{shown} belongs to the workspace at {}, but the repository has two workspaces only, \
             the root one and {AGENTS}/ (docs/01-architecture.md): code in any other is vendored \
             or copied code that no check reads as part of Tidepool",
            if shown_root.is_empty() {
                "."
            } else {
                &shown_root
            }
        ));
        return;
    }
    for member in metadata.packages.iter().filter(|package| package.is_member) {
        let expected = agents.join(&member.name).join("Cargo.toml");
        let located = (repository.locate)(&member.manifest_path).ok();
        let known = REFERENCE_AGENTS.contains(&member.name.as_str());
        if !known || located.as_ref() != Some(&expected) {
            violations.push(format!(
                "`{} {}` ({}) is a member of {AGENTS}/, which may hold only the reference agents \
                 {}, each in {AGENTS}/<name>/ (docs/subsystems/agents.md): vendored or copied \
                 code cannot join it",
                member.name,
                member.version,
                located.as_deref().map_or_else(
                    || member.manifest_path.display().to_string(),
                    |manifest| walk::relative(repository.root, manifest)
                ),
                REFERENCE_AGENTS.join(", ")
            ));
        }
    }
}

/// Checks what the members of a workspace that cargo could not resolve
/// declare, from its `--no-deps` description: every registry or git
/// dependency must be approved ([`declared_problem`]), every path
/// dependency must lead to a member of the workspace, and every member must
/// lie in the repository. Its graph was still not checked; the caller says
/// so too.
fn check_declared(
    metadata: &Metadata,
    pins: &[Pin],
    repository: &Repository<'_>,
    violations: &mut Vec<String>,
) {
    let members = || metadata.packages.iter().filter(|package| package.is_member);
    let member_dirs: BTreeSet<PathBuf> = members()
        .filter_map(|package| {
            let dir = package.manifest_path.parent()?;
            (repository.locate)(dir).ok()
        })
        .collect();
    for package in members() {
        match (repository.locate)(&package.manifest_path) {
            Ok(manifest) if !manifest.starts_with(repository.root) => violations.push(format!(
                "`{} {}` is a workspace member outside the repository ({}): vendored or \
                 cross-repository code counts as a dependency",
                package.name,
                package.version,
                manifest.display()
            )),
            Ok(_) => {}
            Err(error) => violations.push(format!(
                "`{} {}` is a workspace member whose manifest {} cannot be resolved: {error}",
                package.name,
                package.version,
                package.manifest_path.display()
            )),
        }
        for dependency in &package.dependencies {
            let declares = format!(
                "`{}` declares `{} {}` ({})",
                package.name,
                dependency.name,
                dependency.req,
                dependency.kind.describe()
            );
            if let Some(problem) = declared_problem(dependency, pins) {
                violations.push(format!("{declares} {problem}"));
            }
            if dependency.source.is_some() {
                continue;
            }
            let problem = match dependency.path.as_deref().map(repository.locate) {
                None => "with no directory cargo reports, so it cannot be checked".to_string(),
                Some(Err(error)) => format!("from a directory that cannot be resolved: {error}"),
                Some(Ok(dir)) if !dir.starts_with(repository.root) => format!(
                    "from {}, outside the repository: vendored or cross-repository code counts \
                     as a dependency",
                    dir.display()
                ),
                Some(Ok(dir)) if !member_dirs.contains(&dir) => format!(
                    "from {}, a path package in the repository that is not a member of this \
                     workspace: vendored or copied code, or another workspace's crate, counts as \
                     a dependency",
                    walk::relative(repository.root, &dir)
                ),
                Some(Ok(_)) => continue,
            };
            violations.push(format!("{declares} {problem}"));
        }
    }
}

/// Whether a file named `name` may be a cargo configuration file, in any
/// case: `config` or `config.toml`.
fn is_config_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("config") || name.eq_ignore_ascii_case("config.toml")
}

/// Whether `path` is a cargo configuration file: a [`is_config_name`] file
/// in a directory named `.cargo`, in any case.
fn is_cargo_config(path: &Path) -> bool {
    let named = |path: Option<&Path>, check: fn(&str) -> bool| {
        path.and_then(Path::file_name)
            .is_some_and(|name| check(&name.to_string_lossy()))
    };
    named(Some(path), is_config_name)
        && named(path.parent(), |name| name.eq_ignore_ascii_case(".cargo"))
}

/// Checks a cargo configuration file in the repository. Cargo reads every
/// `.cargo/config.toml` above the directory it runs in, so whatever such a
/// file holds applies to anyone who runs cargo, and so `xtask`, there.
/// Only `[alias]` is allowed, with one-line basic-string or string-array
/// values that do not pass `--config` (which could set any other table);
/// every other line is a violation. `shown` names the file in messages.
///
/// The format is checked line by line, not parsed: anything this reading
/// does not understand is a violation rather than a guess, since a table
/// it missed could change what cargo builds or what the checks see. A value
/// that spans lines (a multi-line string, or an array or inline table whose
/// brackets do not close on its first line) is followed to its end
/// ([`Open`]): each line that continues it is a violation of its own, shown
/// as `…`, since it is part of a value, and never a table header.
fn check_cargo_config(shown: &str, text: &str, violations: &mut Vec<String>) {
    let mut in_alias = false;
    let mut open = Open::default();
    for (index, raw) in without_bom(text).lines().enumerate() {
        let line = raw.trim();
        let continues = open.is_open();
        if continues {
            open = open.after(line);
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let problem = if continues {
            Some("a line that continues a multi-line value")
        } else if line.starts_with('[') {
            let header = line
                .split_once('#')
                .map_or(line, |(header, _)| header)
                .trim_end();
            in_alias = header == "[alias]";
            open = open.after(line);
            (!in_alias).then_some("only an `[alias]` table is allowed")
        } else {
            open = open.after(line);
            if in_alias {
                alias_problem(line)
            } else {
                Some("a key outside `[alias]`")
            }
        };
        if let Some(problem) = problem {
            let found = if continues {
                "…".to_owned()
            } else {
                shown_config_line(line)
            };
            violations.push(format!(
                "{shown}:{}: {problem}, but found `{found}`: cargo configuration in the repository \
                 may hold only `[alias]` with one-line string values, since `[env]`, `[build]`, \
                 `[target]`, `[source]`, `[patch]`, `paths` and the rest could change what cargo \
                 builds, or what `xtask` sees, without any manifest saying so",
                index + 1
            ));
        }
    }
}

/// What a violation shows of a trimmed cargo configuration line that starts
/// a table or a key (not one that continues a value: the caller shows that
/// as `…`). A value could hold a secret, such as a registry token, and so
/// could a quoted key or a header's quoted segment (`[patch."https://
/// token@host/repo"]`); secrets never go into messages (AGENTS.md rule 7).
/// So:
///
/// - a table header shows its first segment only, and only if that is a
///   bare key: `[registry]`, `[patch.…]`, `[[bin]]`, `[[a.…]]`, `[…]` for a
///   quoted one, `[…` for one that does not close. Nothing after the header,
///   such as a comment, is shown;
/// - a key line shows its key with the value left out (`token = …`), but
///   only if the text before its first `=` is a bare or dotted bare key
///   ([`is_bare_key`]); anything else is `…`.
fn shown_config_line(line: &str) -> String {
    if line.starts_with('[') {
        let (open, close) = if line.starts_with("[[") {
            ("[[", "]]")
        } else {
            ("[", "]")
        };
        let rest = &line[open.len()..];
        let Some(end) = rest.find(['.', ']']).filter(|_| rest.contains(close)) else {
            return "[…".to_owned();
        };
        // As written, so that `[ alias ]` is shown as it is, not as the
        // `[alias]` it is reported for not being.
        let segment = &rest[..end];
        let more = if rest[end..].starts_with('.') {
            ".…"
        } else {
            ""
        };
        return if is_bare_key(segment.trim_start()) {
            format!("{open}{segment}{more}{close}")
        } else {
            format!("{open}…{close}")
        };
    }
    match line.split_once('=') {
        Some((key, _)) if is_bare_key(key) => format!("{} = …", key.trim_end()),
        _ => "…".to_owned(),
    }
}

/// Whether `text` is a bare or dotted bare TOML key, as it stands before a
/// `=` in a trimmed line: `^[A-Za-z0-9_-]+(\s*\.\s*[A-Za-z0-9_-]+)*\s*$`.
/// A quoted key, or text that is no key at all, is not one.
fn is_bare_key(text: &str) -> bool {
    !text.starts_with(char::is_whitespace)
        && text.split('.').all(|part| {
            let part = part.trim();
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
}

/// What a cargo configuration value has left open at the end of a line: the
/// lines after it continue that value until it closes. This follows TOML's
/// strings and brackets only as far as [`check_cargo_config`] needs, without
/// parsing the file: quotes, brackets and `#` inside a string are text, a
/// basic string's `\` escapes the next character, and a `#` outside one
/// starts a comment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Open {
    /// The quote of the multi-line string (`"""` or `'''`) the line ends
    /// inside, if any.
    string: Option<u8>,
    /// How many array brackets and inline-table braces are open outside
    /// strings. Cargo reads TOML 1.1, in which an inline table may span
    /// lines too.
    brackets: usize,
}

impl Open {
    /// Whether a value is open: the next line continues it.
    fn is_open(self) -> bool {
        self.string.is_some() || self.brackets > 0
    }

    /// What is open after `line`, read from where `self` left off.
    fn after(mut self, line: &str) -> Self {
        let bytes = line.as_bytes();
        let mut at = 0;
        while let Some(&byte) = bytes.get(at) {
            if let Some(quote) = self.string {
                match string_end(&bytes[at..], quote, true) {
                    Some(end) => {
                        at += end;
                        self.string = None;
                    }
                    None => return self,
                }
                continue;
            }
            at += 1;
            match byte {
                b'#' => return self,
                b'"' | b'\'' if bytes[at..].starts_with(&[byte, byte]) => {
                    self.string = Some(byte);
                    at += 2;
                }
                // A one-line string that does not close on its line cannot
                // go on to the next one: nothing is left open.
                b'"' | b'\'' => match string_end(&bytes[at..], byte, false) {
                    Some(end) => at += end,
                    None => return self,
                },
                b'[' | b'{' => self.brackets += 1,
                b']' | b'}' => self.brackets = self.brackets.saturating_sub(1),
                _ => {}
            }
        }
        self
    }
}

/// Where a string that `rest` is inside of ends: just past its closing
/// `quote` (three of them, and up to two more the string ends with, if
/// `multi_line`), or `None` if it does not end in `rest`. A `\` in a basic
/// (`"`) string escapes the byte after it.
fn string_end(rest: &[u8], quote: u8, multi_line: bool) -> Option<usize> {
    let closing = if multi_line { 3 } else { 1 };
    let mut at = 0;
    while let Some(&byte) = rest.get(at) {
        if byte == b'\\' && quote == b'"' {
            at += 2;
        } else if rest
            .get(at..at + closing)
            .is_some_and(|run| run.iter().all(|&b| b == quote))
        {
            let mut end = at + closing;
            while multi_line && end < at + closing + 2 && rest.get(end) == Some(&quote) {
                end += 1;
            }
            return Some(end);
        } else {
            at += 1;
        }
    }
    None
}

/// What is wrong with a line of the `[alias]` table, if anything: it must
/// be `name = "..."` or `name = ["...", ...]` on one line, with a bare name,
/// no escapes and no `--config`.
fn alias_problem(line: &str) -> Option<&'static str> {
    let Some((key, value)) = line.split_once('=') else {
        return Some("not a `name = value` line");
    };
    let key = key.trim();
    if key.is_empty()
        || !key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Some("an alias name that is not a bare name");
    }
    let value = value.trim();
    let strings = match value.strip_prefix('[') {
        Some(array) => {
            let Some((inside, rest)) = array.split_once(']') else {
                return Some("an array that does not end on its line");
            };
            if !is_comment_or_empty(rest) {
                return Some("text after the value");
            }
            let mut strings = Vec::new();
            for item in inside.split(',').map(str::trim) {
                if item.is_empty() {
                    continue;
                }
                match basic_string(item) {
                    Some((string, "")) => strings.push(string),
                    _ => return Some("an array item that is not a plain `\"...\"` string"),
                }
            }
            strings
        }
        None => match basic_string(value) {
            Some((string, rest)) if is_comment_or_empty(rest) => vec![string],
            _ => return Some("a value that is not a plain `\"...\"` string or array of them"),
        },
    };
    strings
        .iter()
        .any(|string| string.contains("--config"))
        .then_some("an alias that passes `--config`, which can set any table")
}

/// Splits `"text" rest` into `text` and `rest`, for a basic string with no
/// escapes.
fn basic_string(text: &str) -> Option<(&str, &str)> {
    let (string, rest) = text.strip_prefix('"')?.split_once('"')?;
    (!string.contains('\\')).then_some((string, rest.trim()))
}

/// Whether `rest` is nothing, or a comment.
fn is_comment_or_empty(rest: &str) -> bool {
    let rest = rest.trim();
    rest.is_empty() || rest.starts_with('#')
}

/// The extension of `name` if a compiler or linker reads files named like
/// that as compiled code.
fn artifact_extension(name: &str) -> Option<&'static str> {
    let (_, extension) = name.rsplit_once('.')?;
    ARTIFACT_EXTENSIONS
        .iter()
        .find(|known| extension.eq_ignore_ascii_case(known))
        .copied()
}

/// Whether a file named `name` may hold a secret that `xtask` must never
/// read (`AGENTS.md` rule 7): keystores, `.p12`, `.p8`, `.pem`, `.key`,
/// `.env` files and provisioning profiles, and the names `.gitignore`
/// keeps out of the repository for that reason.
fn is_secret_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name == ".env"
        || name.starts_with(".env.")
        || name == "keystore.properties"
        || [
            ".jks",
            ".keystore",
            ".p12",
            ".p8",
            ".pem",
            ".key",
            ".mobileprovision",
            ".provisionprofile",
        ]
        .iter()
        .any(|extension| name.ends_with(extension))
}

/// The violation for the file at `path` (under `root`), named `name`, if
/// it is a compiled artifact ([`artifact`]), or if it cannot be read: then
/// it was not checked, which fails the check like any violation, but the
/// rest of the repository still is.
fn artifact_violation(root: &Path, path: &Path, name: &str) -> Option<String> {
    match artifact(path, name) {
        Ok(None) => None,
        Ok(Some(what)) => Some(format!(
            "{} is a compiled artifact ({what}), which the repository must not hold: a build \
             script's `cargo:rustc-link-search` could link prebuilt code in without any manifest \
             or lockfile naming it",
            walk::relative(root, path)
        )),
        Err(error) => Some(format!(
            "{} cannot be read ({error}), so it was not checked for compiled code",
            walk::relative(root, path)
        )),
    }
}

/// What kind of compiled artifact the file at `path`, named `name`, is, if
/// it is one: by its name, or by how it starts. A file that may hold a
/// secret ([`is_secret_name`]) is judged by its name only: it is never
/// opened.
///
/// # Errors
///
/// The error from opening or reading the file.
fn artifact(path: &Path, name: &str) -> std::io::Result<Option<String>> {
    if let Some(extension) = artifact_extension(name) {
        return Ok(Some(format!("named `.{extension}`")));
    }
    if is_secret_name(name) {
        return Ok(None);
    }
    let mut head = Vec::new();
    std::io::Read::read_to_end(
        &mut std::io::Read::take(fs::File::open(path)?, MAGIC_BYTES),
        &mut head,
    )?;
    Ok(magic(&head).map(|what| format!("{what} by its first bytes")))
}

/// The kind of compiled code that a file starting with `head` holds, if
/// its first bytes say so.
fn magic(head: &[u8]) -> Option<&'static str> {
    const MAGICS: [(&[u8], &str); 13] = [
        (b"\x7fELF", "an ELF object, library or executable"),
        (b"\xfe\xed\xfa\xce", "a Mach-O object"),
        (b"\xfe\xed\xfa\xcf", "a Mach-O object"),
        (b"\xce\xfa\xed\xfe", "a Mach-O object"),
        (b"\xcf\xfa\xed\xfe", "a Mach-O object"),
        // Java class files start with the same four bytes.
        (
            b"\xca\xfe\xba\xbe",
            "a universal Mach-O binary or a Java class",
        ),
        // The 64-bit universal header (`FAT_MAGIC_64`).
        (b"\xca\xfe\xba\xbf", "a 64-bit universal Mach-O binary"),
        (b"!<arch>\n", "an `ar` archive: an rlib or static library"),
        (b"!<thin>\n", "a thin `ar` archive"),
        (b"rust\0\0\0", "rustc metadata"),
        (b"\0asm", "WebAssembly"),
        (b"BC\xc0\xde", "LLVM bitcode"),
        // The bitcode wrapper header, `0x0B17C0DE` little-endian.
        (b"\xde\xc0\x17\x0b", "wrapped LLVM bitcode"),
    ];
    if let Some((_, what)) = MAGICS.iter().find(|(magic, _)| head.starts_with(magic)) {
        return Some(what);
    }
    // A PE file (Windows executable or DLL): `MZ`, and `PE\0\0` where the
    // offset at 0x3c points.
    let pe = head.starts_with(b"MZ")
        && head
            .get(0x3c..0x40)
            .and_then(|offset| <[u8; 4]>::try_from(offset).ok())
            .and_then(|offset| usize::try_from(u32::from_le_bytes(offset)).ok())
            .and_then(|offset| head.get(offset..offset.checked_add(4)?))
            == Some(b"PE\0\0");
    pe.then_some("a PE executable or DLL")
}

/// Checks one workspace's resolved graph.
///
/// Every path package is located, members included: a member can live
/// outside the workspace's directory (with `package.workspace`), and so
/// outside the repository.
fn check_graph(
    metadata: &Metadata,
    pins: &[Pin],
    repository: &Repository<'_>,
    report: &mut Report,
    counts: &mut Counts,
) {
    for package in &metadata.packages {
        let problem = match &package.source {
            Some(source) => {
                counts.external += 1;
                pin_problem(&package.name, &package.version, source, pins)
            }
            None => match (repository.locate)(&package.manifest_path) {
                Ok(manifest) if !manifest.starts_with(repository.root) => {
                    counts.external += 1;
                    Some(format!(
                        "is a path package outside the repository ({}): vendored or \
                         cross-repository code counts as a dependency",
                        manifest.display()
                    ))
                }
                Ok(_) if package.is_member => {
                    counts.members += 1;
                    None
                }
                Ok(manifest) => {
                    counts.loose += 1;
                    Some(format!(
                        "is a path package in the repository ({}) that is not a member of this \
                         workspace: vendored or copied code, or another workspace's crate, \
                         counts as a dependency",
                        walk::relative(repository.root, &manifest)
                    ))
                }
                Err(error) => {
                    counts.external += 1;
                    Some(format!(
                        "is a path package whose manifest {} cannot be resolved: {error}",
                        package.manifest_path.display()
                    ))
                }
            },
        };
        if let Some(problem) = problem {
            report.violations.push(format!(
                "`{} {}` {problem}{}",
                package.name,
                package.version,
                users(metadata, package)
            ));
        }
        if package.is_member {
            for dependency in &package.dependencies {
                if let Some(problem) = declared_problem(dependency, pins) {
                    report.violations.push(format!(
                        "`{}` declares `{} {}` ({}) {problem}",
                        package.name,
                        dependency.name,
                        dependency.req,
                        dependency.kind.describe()
                    ));
                }
            }
        }
    }
}

/// Says what is wrong with a resolved package that has a `source`, if
/// anything.
fn pin_problem(name: &str, version: &str, source: &str, pins: &[Pin]) -> Option<String> {
    let shown = without_userinfo(source);
    let mut approved = pins
        .iter()
        .filter(|pin| pin.name == name)
        .map(|pin| pin.version.as_str());
    let Some(first) = approved.clone().next() else {
        return Some(format!("({shown}) is not in {ALLOWLIST}"));
    };
    if source != CRATES_IO {
        return Some(format!(
            "({shown}) is approved only from crates.io ({CRATES_IO})"
        ));
    }
    if approved.any(|approved| approved == version) {
        return None;
    }
    Some(format!("({shown}) is not the approved version, {first}"))
}

/// Says what is wrong with a dependency a member declares, if anything: a
/// registry or git dependency must be approved, from crates.io, with the
/// exact requirement `=version` (`docs/02-dependency-policy.md`).
fn declared_problem(dependency: &Dependency, pins: &[Pin]) -> Option<String> {
    let source = dependency.source.as_deref()?;
    let shown = without_userinfo(source);
    let mut approved = pins.iter().filter(|pin| pin.name == dependency.name);
    let Some(first) = approved.next() else {
        return Some(format!("from `{shown}`, which is not in {ALLOWLIST}"));
    };
    if source != CRATES_IO {
        return Some(format!(
            "from `{shown}`; it is approved only from crates.io ({CRATES_IO})"
        ));
    }
    let exact = |pin: &Pin| dependency.req == format!("={}", pin.version);
    if exact(first) || approved.any(exact) {
        return None;
    }
    Some(format!(
        "from crates.io; the approved requirement is the exact `={}`",
        first.version
    ))
}

/// Names the workspace members that declare `package` directly, as a
/// suffix for a message. A package nobody declares directly came in through
/// another package. A path dependency is matched by its directory, so that
/// a vendored crate named like a member is not blamed on the member's
/// users; any other dependency by name.
fn users(metadata: &Metadata, package: &Package) -> String {
    let dir = package.manifest_path.parent().map(metadata::canonical);
    let declares = |dependency: &Dependency| {
        dependency.name == package.name
            && match &dependency.path {
                Some(path) => dir.as_ref() == Some(&metadata::canonical(path)),
                None => true,
            }
    };
    let users: BTreeSet<&str> = metadata
        .packages
        .iter()
        .filter(|member| member.is_member)
        .filter(|member| member.dependencies.iter().any(declares))
        .map(|member| member.name.as_str())
        .collect();
    if users.is_empty() {
        String::new()
    } else {
        format!(
            "; required by {}",
            users.into_iter().collect::<Vec<_>>().join(", ")
        )
    }
}

/// Reads a `Cargo.lock` as text and reports every `source = ` entry whose
/// package is not approved. `shown` names the file in messages.
///
/// The lockfile's own table headers delimit packages; a `source` key in
/// any other table (such as `[[patch.unused]]`) is reported whatever its
/// name.
fn check_lockfile(shown: &str, text: &str, pins: &[Pin], violations: &mut Vec<String>) {
    #[derive(Default)]
    struct Entry {
        in_package: bool,
        name: Option<String>,
        version: Option<String>,
        sources: Vec<(usize, String)>,
    }
    fn finish(shown: &str, entry: &Entry, pins: &[Pin], violations: &mut Vec<String>) {
        for (line, source) in &entry.sources {
            let (name, version) = (
                entry.name.as_deref().unwrap_or("?"),
                entry.version.as_deref().unwrap_or("?"),
            );
            let approved = entry.in_package
                && source == CRATES_IO
                && pins
                    .iter()
                    .any(|pin| pin.name == name && pin.version == version);
            if !approved {
                let source = without_userinfo(source);
                violations.push(format!(
                    "{shown}:{line}: `{name} {version}` comes from `{source}`, which is not \
                     approved in {ALLOWLIST}"
                ));
            }
        }
    }

    let mut entry = Entry::default();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.starts_with('[') {
            finish(shown, &entry, pins, violations);
            entry = Entry {
                in_package: line == "[[package]]",
                ..Entry::default()
            };
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"').to_string();
        let key = key.trim();
        // TOML lets a key be quoted, which cargo never does: read it as
        // the bare key, unless it has an escape, which could spell any key.
        let unquoted = ['"', '\'']
            .iter()
            .find_map(|&quote| key.strip_prefix(quote)?.strip_suffix(quote));
        if unquoted.is_some_and(|key| key.contains('\\')) {
            violations.push(format!(
                "{shown}:{}: the quoted key {key} has an escape, which cargo never writes and \
                 this check does not read",
                index + 1
            ));
            continue;
        }
        match unquoted.unwrap_or(key) {
            "name" => entry.name = Some(value),
            "version" => entry.version = Some(value),
            "source" => entry.sources.push((index + 1, value)),
            _ if mentions_source_key(line) => violations.push(format!(
                "{shown}:{}: a `source` key that is not a `source = ` line of its own (a dotted \
                 key or an inline table), which cargo never writes and this check does not \
                 read: `{}`",
                index + 1,
                without_userinfo(line)
            )),
            _ => {}
        }
    }
    finish(shown, &entry, pins, violations);
}

/// Whether `line` has a `source` key anywhere: the word, alone or quoted,
/// after the start of the line, a space, `{`, `,`, `.` or a quote, and
/// followed by `=`.
fn mentions_source_key(line: &str) -> bool {
    line.match_indices("source").any(|(at, word)| {
        let boundary = line
            .get(..at)
            .and_then(|before| before.chars().next_back())
            .is_none_or(|c| c.is_whitespace() || matches!(c, '{' | ',' | '.' | '"' | '\''));
        let after = line.get(at + word.len()..).unwrap_or("");
        boundary
            && after
                .trim_start_matches(['"', '\''])
                .trim_start()
                .starts_with('=')
    })
}

/// Reads the allowlist: one `name version ADR-NNNN` per line, with blank
/// lines and `#` comment lines ignored. A line of any other shape becomes a
/// violation.
pub(crate) fn parse_allowlist(text: &str, violations: &mut Vec<String>) -> Vec<Pin> {
    let mut pins = Vec::new();
    for (index, line) in without_bom(text).lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        match fields.as_slice() {
            [name, version, adr] if is_adr(adr) => pins.push(Pin {
                name: (*name).to_string(),
                version: (*version).to_string(),
            }),
            _ => violations.push(format!(
                "{ALLOWLIST}:{}: expected `name version ADR-NNNN`, found `{line}`",
                index + 1
            )),
        }
    }
    pins
}

/// `text` without the one UTF-8 byte-order mark it may start with, which
/// some editors write (on Windows above all) and cargo skips: `str::trim`
/// does not remove it, so it would otherwise be read as part of the first
/// line. Any other is kept, and fails where it is.
fn without_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

/// Whether `text` is an ADR reference: `ADR-` and exactly four digits, as
/// in `ADR-0004`.
fn is_adr(text: &str) -> bool {
    text.strip_prefix("ADR-")
        .is_some_and(|number| number.len() == 4 && number.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::Kind;
    use crate::metadata::testing::{ROOT, graph, member, third_party};
    use crate::walk::testing::TempTree;

    /// Takes a manifest path as already resolved: the packages these tests
    /// make do not exist on disk, and their paths have no links or `..`.
    fn as_given(path: &Path) -> std::io::Result<PathBuf> {
        Ok(path.to_path_buf())
    }

    /// The repository at `/work/tidepool`, with paths taken as given.
    fn repository<'a>() -> Repository<'a> {
        Repository {
            root: Path::new(ROOT),
            locate: as_given,
        }
    }

    /// Runs [`check_graph`] on `metadata` alone with `allowlist`.
    fn check(metadata: &Metadata, allowlist: &str) -> Report {
        let mut report = Report::default();
        let pins = parse_allowlist(allowlist, &mut report.violations);
        let mut counts = Counts::default();
        check_graph(metadata, &pins, &repository(), &mut report, &mut counts);
        report
    }

    fn pin(name: &str, version: &str) -> Pin {
        Pin {
            name: name.to_string(),
            version: version.to_string(),
        }
    }

    #[test]
    fn a_workspace_of_only_members_passes() {
        let metadata = graph(vec![
            member("tidepool-core"),
            member("tidepool-codec").path_dep("tidepool-core", Kind::Dev),
        ]);
        let mut report = Report::default();
        let mut counts = Counts::default();
        check_graph(&metadata, &[], &repository(), &mut report, &mut counts);
        assert_eq!(report, Report::default());
        assert_eq!(
            counts,
            Counts {
                members: 2,
                ..Counts::default()
            }
        );
    }

    #[test]
    fn canned_metadata_with_a_crates_io_package_fails_naming_it() {
        // The JSON cargo prints for a graph with a crates.io package. The
        // fixture tests also resolve one offline, through a directory
        // source replacement.
        let text = r#"{
            "packages": [
                {"name": "tidepool-core", "version": "0.1.0",
                 "id": "path+file:///work/tidepool/crates/core#tidepool-core@0.1.0",
                 "source": null, "manifest_path": "/work/tidepool/crates/core/Cargo.toml",
                 "targets": [],
                 "dependencies": [
                    {"name": "left-pad", "kind": null, "req": "^1",
                     "source": "registry+https://github.com/rust-lang/crates.io-index"}
                 ]},
                {"name": "left-pad", "version": "1.0.3",
                 "id": "registry+https://github.com/rust-lang/crates.io-index#left-pad@1.0.3",
                 "source": "registry+https://github.com/rust-lang/crates.io-index",
                 "manifest_path": "/home/u/.cargo/registry/src/i/left-pad-1.0.3/Cargo.toml",
                 "targets": [], "dependencies": []}
            ],
            "workspace_members": ["path+file:///work/tidepool/crates/core#tidepool-core@0.1.0"],
            "workspace_root": "/work/tidepool",
            "version": 1
        }"#;
        let metadata = metadata::parse(text.as_bytes(), "cargo metadata").unwrap();
        let empty =
            fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("deps-allowlist.txt"))
                .expect("the real allowlist exists");
        let report = check(&metadata, &empty);
        assert_eq!(
            report.violations,
            [
                format!(
                    "`tidepool-core` declares `left-pad ^1` (normal dependency) from \
                     `{CRATES_IO}`, which is not in xtask/deps-allowlist.txt"
                ),
                format!(
                    "`left-pad 1.0.3` ({CRATES_IO}) is not in xtask/deps-allowlist.txt; \
                     required by tidepool-core"
                ),
            ]
        );
    }

    #[test]
    fn a_listed_package_passes_only_at_the_listed_version_with_an_exact_requirement() {
        let metadata = graph(vec![
            member("tidepool-core").registry_dep("left-pad", "=1.0.3"),
            third_party("left-pad", "1.0.3"),
        ]);
        let approved = check(&metadata, "left-pad 1.0.3 ADR-0099\n");
        assert_eq!(approved.violations, Vec::<String>::new());

        let other_version = check(&metadata, "left-pad 1.0.2 ADR-0099\n");
        assert_eq!(
            other_version.violations,
            [
                "`tidepool-core` declares `left-pad =1.0.3` (normal dependency) from crates.io; \
                 the approved requirement is the exact `=1.0.2`"
                    .to_string(),
                format!(
                    "`left-pad 1.0.3` ({CRATES_IO}) is not the approved version, 1.0.2; \
                     required by tidepool-core"
                ),
            ]
        );

        // A longer name or version must not match by prefix.
        let similar = check(
            &metadata,
            "left-pad 1.0.30 ADR-0099\nleft-padding 1.0.3 ADR-0099\n",
        );
        assert_eq!(similar.violations.len(), 2, "{:?}", similar.violations);

        // The policy asks for an exact requirement, not a range.
        let caret = graph(vec![
            member("tidepool-core").registry_dep("left-pad", "^1.0.3"),
            third_party("left-pad", "1.0.3"),
        ]);
        assert_eq!(
            check(&caret, "left-pad 1.0.3 ADR-0099\n").violations,
            [
                "`tidepool-core` declares `left-pad ^1.0.3` (normal dependency) from crates.io; \
                 the approved requirement is the exact `=1.0.3`"
            ]
        );
    }

    #[test]
    fn a_pin_approves_crates_io_only() {
        let fork = "git+https://example.com/left-pad#0123abc";
        let mut forked = third_party("left-pad", "1.0.3");
        forked.source = Some(fork.to_string());
        let mut core = member("tidepool-core").registry_dep("left-pad", "=1.0.3");
        core.dependencies[0].source = Some(fork.to_string());
        let report = check(&graph(vec![core, forked]), "left-pad 1.0.3 ADR-0099\n");
        assert_eq!(
            report.violations,
            [
                format!(
                    "`tidepool-core` declares `left-pad =1.0.3` (normal dependency) from \
                     `{fork}`; it is approved only from crates.io ({CRATES_IO})"
                ),
                format!(
                    "`left-pad 1.0.3` ({fork}) is approved only from crates.io ({CRATES_IO}); \
                     required by tidepool-core"
                ),
            ]
        );
    }

    #[test]
    fn a_transitive_or_git_package_fails() {
        let mut git = third_party("pad-core", "0.4.1");
        git.source = Some("git+https://example.com/pad-core#0123abc".to_string());
        let metadata = graph(vec![
            member("tidepool-core").registry_dep("left-pad", "=1.0.3"),
            // A registry package's own declarations are not checked: what
            // it really uses is in the resolved graph.
            third_party("left-pad", "1.0.3").registry_dep("pad-core", "^0.4"),
            git,
        ]);
        let report = check(&metadata, "left-pad 1.0.3 ADR-0099\n");
        assert_eq!(
            report.violations,
            [
                "`pad-core 0.4.1` (git+https://example.com/pad-core#0123abc) is not in \
              xtask/deps-allowlist.txt"
            ]
        );
    }

    #[test]
    fn a_registry_dependency_served_from_a_path_fails() {
        // `left-pad = "1"` with `[patch.crates-io] left-pad = { path =
        // "vendor/left-pad" }`: the resolved package is a path package in
        // the repository, but the member declares a crates.io dependency.
        let mut vendored = third_party("left-pad", "1.0.3");
        vendored.source = None;
        vendored.manifest_path = PathBuf::from(format!("{ROOT}/vendor/left-pad/Cargo.toml"));
        let metadata = graph(vec![
            member("tidepool-text").registry_dep("left-pad", "^1"),
            vendored,
        ]);
        assert_eq!(
            check(&metadata, "").violations,
            [
                format!(
                    "`tidepool-text` declares `left-pad ^1` (normal dependency) from \
                     `{CRATES_IO}`, which is not in xtask/deps-allowlist.txt"
                ),
                "`left-pad 1.0.3` is a path package in the repository \
                 (vendor/left-pad/Cargo.toml) that is not a member of this workspace: vendored \
                 or copied code, or another workspace's crate, counts as a dependency; required \
                 by tidepool-text"
                    .to_string(),
            ]
        );
        // The same for a crate that takes a workspace member's name, and
        // for a member patched in under a registry requirement.
        let mut namesake = member("tidepool-core");
        namesake.version = "0.2.0".to_string();
        namesake.is_member = false;
        namesake.manifest_path = PathBuf::from(format!("{ROOT}/vendor/core/Cargo.toml"));
        let metadata = graph(vec![
            member("tidepool-core").registry_dep("tidepool-text", "^0.1"),
            member("tidepool-text"),
            namesake,
        ]);
        assert_eq!(
            check(&metadata, "").violations,
            [
                format!(
                    "`tidepool-core` declares `tidepool-text ^0.1` (normal dependency) from \
                     `{CRATES_IO}`, which is not in xtask/deps-allowlist.txt"
                ),
                "`tidepool-core 0.2.0` is a path package in the repository \
                 (vendor/core/Cargo.toml) that is not a member of this workspace: vendored or \
                 copied code, or another workspace's crate, counts as a dependency"
                    .to_string(),
            ]
        );
    }

    #[test]
    fn no_workspace_may_use_another_one_s_crates() {
        // `agents/tool` (its own workspace) uses `crates/core`: the
        // reference agents are std-only (`docs/subsystems/agents.md`), and
        // the matrix in `docs/01-architecture.md` has no rows for them, so
        // nothing else would check that edge.
        let mut tool = member("tp-agent-mock");
        tool.manifest_path = PathBuf::from(format!("{ROOT}/agents/tool/Cargo.toml"));
        let tool = tool.path_dep("tidepool-core", Kind::Normal);
        let mut core = member("tidepool-core");
        core.is_member = false;
        let metadata = graph(vec![tool, core]);
        let mut report = Report::default();
        let mut counts = Counts::default();
        check_graph(&metadata, &[], &repository(), &mut report, &mut counts);
        assert_eq!(
            report.violations,
            ["`tidepool-core 0.1.0` is a path package in the repository \
                 (crates/core/Cargo.toml) that is not a member of this workspace: vendored or \
                 copied code, or another workspace's crate, counts as a dependency; required by \
                 tp-agent-mock"]
        );
        assert_eq!(
            counts,
            Counts {
                members: 1,
                loose: 1,
                ..Counts::default()
            }
        );
        // Nor a crate that is the member of a third workspace, such as
        // vendored code with a `[workspace]` of its own, and the root
        // workspace may not use `agents/` (`docs/01-architecture.md`:
        // agents are never linked into Tidepool).
        let mut vendored = member("left-pad");
        vendored.is_member = false;
        vendored.manifest_path = PathBuf::from(format!("{ROOT}/vendor/left-pad/Cargo.toml"));
        let mut agent = member("tp-agent-mock");
        agent.is_member = false;
        agent.manifest_path = PathBuf::from(format!("{ROOT}/agents/tool/Cargo.toml"));
        let metadata = graph(vec![
            member("tidepool-core")
                .path_dep("left-pad", Kind::Normal)
                .path_dep("tp-agent-mock", Kind::Dev),
            vendored,
            agent,
        ]);
        let report = check(&metadata, "");
        assert_eq!(report.violations.len(), 2, "{:?}", report.violations);
        assert!(
            report.violations[1].starts_with(
                "`tp-agent-mock 0.1.0` is a path package in the repository \
                 (agents/tool/Cargo.toml) that is not a member of this workspace"
            ),
            "{:?}",
            report.violations
        );
    }

    #[test]
    fn a_path_package_outside_the_repository_fails_even_when_listed() {
        let tree = TempTree::new("deps-outside");
        tree.write("repo/crates/core/Cargo.toml", "");
        tree.write("outside/left-pad/Cargo.toml", "");
        let repository = fs::canonicalize(tree.path().join("repo")).unwrap();

        let mut core = member("tidepool-core").path_dep("left-pad", Kind::Normal);
        core.manifest_path = repository.join("crates/core/Cargo.toml");
        core.dependencies[0].path = Some(tree.path().join("repo/../outside/left-pad"));
        let mut vendored = third_party("left-pad", "1.0.3");
        vendored.source = None;
        vendored.manifest_path = tree.path().join("repo/../outside/left-pad/Cargo.toml");

        let mut report = Report::default();
        let mut counts = Counts::default();
        let metadata = graph(vec![core, vendored]);
        check_graph(
            &metadata,
            &[pin("left-pad", "1.0.3")],
            &Repository {
                root: &repository,
                locate: real_path,
            },
            &mut report,
            &mut counts,
        );
        let outside = fs::canonicalize(tree.path().join("outside/left-pad/Cargo.toml")).unwrap();
        assert_eq!(
            report.violations,
            [format!(
                "`left-pad 1.0.3` is a path package outside the repository ({}): vendored or \
                 cross-repository code counts as a dependency; required by tidepool-core",
                outside.display()
            )]
        );
        assert_eq!(
            counts,
            Counts {
                members: 1,
                external: 1,
                ..Counts::default()
            }
        );
    }

    #[test]
    fn a_workspace_member_outside_the_repository_fails() {
        // `members = ["../elsewhere"]` with `package.workspace` set there.
        let mut far = member("tidepool-far");
        far.manifest_path = PathBuf::from("/work/elsewhere/far/Cargo.toml");
        let metadata = graph(vec![member("tidepool-core"), far]);
        let mut report = Report::default();
        check_graph(
            &metadata,
            &[],
            &repository(),
            &mut report,
            &mut Counts::default(),
        );
        assert_eq!(
            report.violations,
            [
                "`tidepool-far 0.1.0` is a path package outside the repository \
              (/work/elsewhere/far/Cargo.toml): vendored or cross-repository code counts as a \
              dependency"
            ]
        );
        // A sibling directory whose name merely starts like the root's is
        // outside too: paths are compared by component.
        let mut sibling = member("tidepool-sibling");
        sibling.manifest_path = PathBuf::from("/work/tidepool-old/Cargo.toml");
        sibling.is_member = false;
        assert_eq!(check(&graph(vec![sibling]), "").violations.len(), 1);
    }

    #[test]
    fn a_path_package_that_cannot_be_found_fails() {
        let mut ghost = third_party("ghost", "0.1.0");
        ghost.source = None;
        ghost.manifest_path = PathBuf::from("/nonexistent/tidepool-xtask/ghost/Cargo.toml");
        let mut report = Report::default();
        let mut counts = Counts::default();
        let metadata = graph(vec![ghost]);
        check_graph(
            &metadata,
            &[],
            &Repository {
                locate: real_path,
                ..repository()
            },
            &mut report,
            &mut counts,
        );
        assert_eq!(report.violations.len(), 1);
        assert!(
            report.violations[0].starts_with(
                "`ghost 0.1.0` is a path package whose manifest \
                 /nonexistent/tidepool-xtask/ghost/Cargo.toml cannot be resolved"
            ),
            "{:?}",
            report.violations
        );
    }

    #[test]
    fn every_member_that_asks_for_the_package_is_named() {
        let metadata = graph(vec![
            member("tidepool-text").registry_dep("left-pad", "=1.0.3"),
            member("tidepool-core").registry_dep("left-pad", "=1.0.3"),
            third_party("left-pad", "1.0.3"),
        ]);
        let report = check(&metadata, "");
        assert_eq!(report.violations.len(), 3);
        assert!(
            report.violations[2].ends_with("; required by tidepool-core, tidepool-text"),
            "{}",
            report.violations[2]
        );
    }

    const LOCK: &str = "# This file is automatically @generated by Cargo.\r\n\
        version = 4\r\n\
        \r\n\
        [[package]]\r\n\
        name = \"tidepool-core\"\r\n\
        version = \"0.1.0\"\r\n\
        \r\n\
        [[package]]\r\n\
        name = \"left-pad\"\r\n\
        version = \"1.0.3\"\r\n\
        source = \"registry+https://github.com/rust-lang/crates.io-index\"\r\n\
        checksum = \"0000\"\r\n\
        \r\n\
        [[package]]\r\n\
        name = \"pad-core\"\r\n\
        version = \"0.4.1\"\r\n\
        source = \"git+https://example.com/pad-core#abc\"\r\n\
        \r\n\
        [[patch.unused]]\r\n\
        name = \"unused\"\r\n\
        version = \"2.0.0\"\r\n\
        source = \"git+https://example.com/unused#abc\"\r\n";

    #[test]
    fn a_lockfile_source_line_fails_unless_approved() {
        let mut violations = Vec::new();
        check_lockfile("Cargo.lock", LOCK, &[], &mut violations);
        assert_eq!(
            violations,
            [
                format!(
                    "Cargo.lock:11: `left-pad 1.0.3` comes from `{CRATES_IO}`, which is not \
                     approved in xtask/deps-allowlist.txt"
                ),
                "Cargo.lock:17: `pad-core 0.4.1` comes from `git+https://example.com/pad-core#abc`, \
                 which is not approved in xtask/deps-allowlist.txt"
                    .to_string(),
                "Cargo.lock:22: `unused 2.0.0` comes from `git+https://example.com/unused#abc`, \
                 which is not approved in xtask/deps-allowlist.txt"
                    .to_string(),
            ]
        );

        // A pin approves a crates.io package entry, never a git one with
        // the same name and version, nor another table's source.
        let pins = [
            pin("left-pad", "1.0.3"),
            pin("pad-core", "0.4.1"),
            pin("unused", "2.0.0"),
        ];
        let mut violations = Vec::new();
        check_lockfile("Cargo.lock", LOCK, &pins, &mut violations);
        assert_eq!(violations.len(), 2, "{violations:?}");
        assert!(violations[0].starts_with("Cargo.lock:17: `pad-core 0.4.1`"));
        assert!(violations[1].starts_with("Cargo.lock:22: `unused 2.0.0`"));
    }

    #[test]
    fn a_source_key_in_any_other_form_fails() {
        // TOML lets a key be quoted (cargo still reads it), dotted, or in an
        // inline table: none of them may carry a source past the check.
        for (text, expected) in [
            (
                "[[package]]\nname = \"left-pad\"\nversion = \"1.0.3\"\n\
                 \"source\" = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
                format!(
                    "x/Cargo.lock:4: `left-pad 1.0.3` comes from `{CRATES_IO}`, which is not \
                     approved in xtask/deps-allowlist.txt"
                ),
            ),
            (
                "[[package]]\nname = \"left-pad\"\nversion = \"1.0.3\"\n'source' = 'git+x'\n",
                "x/Cargo.lock:4: `left-pad 1.0.3` comes from `'git+x'`, which is not approved in \
                 xtask/deps-allowlist.txt"
                    .to_string(),
            ),
            (
                "[[package]]\nname = \"left-pad\"\n\"\\u0073ource\" = \"git+x\"\n",
                "x/Cargo.lock:3: the quoted key \"\\u0073ource\" has an escape, which cargo never \
                 writes and this check does not read"
                    .to_string(),
            ),
            (
                "package = [{ name = \"left-pad\", version = \"1.0.3\", source = \"git+x\" }]\n",
                "x/Cargo.lock:1: a `source` key that is not a `source = ` line of its own (a dotted \
                 key or an inline table), which cargo never writes and this check does not read: \
                 `package = [{ name = \"left-pad\", version = \"1.0.3\", source = \"git+x\" }]`"
                    .to_string(),
            ),
            (
                "package.source = \"git+x\"\n",
                "x/Cargo.lock:1: a `source` key that is not a `source = ` line of its own (a dotted \
                 key or an inline table), which cargo never writes and this check does not read: \
                 `package.source = \"git+x\"`"
                    .to_string(),
            ),
            // Unspaced, and quoted in a dotted key.
            (
                "package = [{name = \"a\",source = \"git+x\"}]\n",
                "x/Cargo.lock:1: a `source` key that is not a `source = ` line of its own (a dotted \
                 key or an inline table), which cargo never writes and this check does not read: \
                 `package = [{name = \"a\",source = \"git+x\"}]`"
                    .to_string(),
            ),
            (
                "package = [{source=\"git+x\"}]\n",
                "x/Cargo.lock:1: a `source` key that is not a `source = ` line of its own (a dotted \
                 key or an inline table), which cargo never writes and this check does not read: \
                 `package = [{source=\"git+x\"}]`"
                    .to_string(),
            ),
            (
                "package.\"source\" = \"git+x\"\n",
                "x/Cargo.lock:1: a `source` key that is not a `source = ` line of its own (a dotted \
                 key or an inline table), which cargo never writes and this check does not read: \
                 `package.\"source\" = \"git+x\"`"
                    .to_string(),
            ),
        ] {
            let mut violations = Vec::new();
            check_lockfile("x/Cargo.lock", text, &[], &mut violations);
            assert_eq!(violations, [expected], "{text}");
        }
        // The words in a value, such as a git URL, are not a key.
        let mut violations = Vec::new();
        check_lockfile(
            "x/Cargo.lock",
            "[[package]]\nname = \"a\"\nversion = \"0.1.0\"\ndependencies = [\n \"b 0.1.0 \
             (git+https://example.com/opensource?branch=main#0)\",\n]\n",
            &[],
            &mut violations,
        );
        assert_eq!(violations, Vec::<String>::new());
    }

    #[test]
    fn the_real_lockfile_has_no_source_lines() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../Cargo.lock");
        let text = fs::read_to_string(path).expect("the workspace has a lockfile");
        let mut violations = Vec::new();
        check_lockfile("Cargo.lock", &text, &[], &mut violations);
        assert_eq!(violations, Vec::<String>::new());
    }

    #[test]
    fn the_real_allowlist_is_empty_and_well_formed() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("deps-allowlist.txt");
        let text = fs::read_to_string(path).expect("the allowlist exists");
        let mut violations = Vec::new();
        assert_eq!(parse_allowlist(&text, &mut violations), Vec::<Pin>::new());
        assert_eq!(violations, Vec::<String>::new());
    }

    #[test]
    fn allowlist_ignores_comments_and_blank_lines() {
        let mut violations = Vec::new();
        let pins = parse_allowlist(
            "# Approved packages.\n\n  # indented comment\nleft-pad 1.0.3 ADR-0012\n\
             \tpad-core   0.4.1\tADR-0007 \r\n",
            &mut violations,
        );
        assert_eq!(violations, Vec::<String>::new());
        assert_eq!(pins, [pin("left-pad", "1.0.3"), pin("pad-core", "0.4.1")]);
    }

    #[test]
    fn malformed_allowlist_lines_are_violations_with_their_line_number() {
        let text = "# header\nleft-pad 1.0.3\nleft-pad 1.0.3 ADR-0012 extra\n\
                    left-pad 1.0.3 adr-0012\nleft-pad 1.0.3 ADR-\nleft-pad 1.0.3 ADR-12a\n\
                    left-pad 1.0.3 ADR-7\nleft-pad 1.0.3 ADR-00012\nok 1.0.0 ADR-0001\n";
        let mut violations = Vec::new();
        let pins = parse_allowlist(text, &mut violations);
        let expected: Vec<String> = [
            (2, "left-pad 1.0.3"),
            (3, "left-pad 1.0.3 ADR-0012 extra"),
            (4, "left-pad 1.0.3 adr-0012"),
            (5, "left-pad 1.0.3 ADR-"),
            (6, "left-pad 1.0.3 ADR-12a"),
            (7, "left-pad 1.0.3 ADR-7"),
            (8, "left-pad 1.0.3 ADR-00012"),
        ]
        .iter()
        .map(|(line, text)| {
            format!(
                "xtask/deps-allowlist.txt:{line}: expected `name version ADR-NNNN`, found `{text}`"
            )
        })
        .collect();
        assert_eq!(violations, expected);
        assert_eq!(pins, [pin("ok", "1.0.0")]);
    }

    #[test]
    fn an_out_of_date_lockfile_says_how_to_update_it_and_still_fails() {
        // Cargo's words when `--locked` stops it rewriting the lockfile,
        // older and newer.
        assert!(is_stale_lockfile(
            "error: the lock file /w/Cargo.lock needs to be updated but --locked was passed to \
             prevent this\nIf you want to try to generate the lock file without accessing the \
             network, remove the --locked flag and use --offline instead.\n"
        ));
        assert!(is_stale_lockfile(
            "error: cannot update the lock file /w/Cargo.lock because --locked was passed to \
             prevent this\nhelp: to generate the lock file without accessing the network, \
             remove the --locked flag and use --offline instead.\n"
        ));
        assert!(!is_stale_lockfile(
            "error: no matching package named `left-pad` found\n"
        ));
        // Only an error line of cargo's own counts, not a path or a
        // package's output that happens to hold the words.
        assert!(!is_stale_lockfile(
            "warning: lock file note: --locked was passed to prevent this\n"
        ));
        // The error line need not be the first one.
        assert!(is_stale_lockfile(
            "warning: unused manifest key: package.foo\nerror: cannot update the lock file \
             /w/Cargo.lock because --locked was passed to prevent this\n"
        ));
        // Another lock file error is not this one, nor is `--locked`
        // stopping something other than the lock file.
        assert!(!is_stale_lockfile(
            "error: failed to parse lock file at: /w/Cargo.lock\n"
        ));
        assert!(!is_stale_lockfile(
            "error: the manifest needs to be updated but --locked was passed to prevent this\n"
        ));
        let stale = unresolved_message(
            "agents/Cargo.toml",
            Unresolved::StaleLockfile,
            &"`cargo metadata` failed",
        );
        assert_eq!(
            stale,
            "cannot resolve the workspace of agents/Cargo.toml offline from its lockfile as it \
             is: its Cargo.lock is out of date, so the graph was not checked. Update the lockfile \
             offline, which compiles nothing (`cargo metadata --offline --format-version 1 \
             --manifest-path agents/Cargo.toml`), then run `cargo xtask deps` again: `cargo \
             metadata` failed"
        );
        let other = unresolved_message("Cargo.toml", Unresolved::Other, &"`cargo metadata` failed");
        assert!(
            other.starts_with(
                "cannot resolve the workspace of Cargo.toml offline from its lockfile as it is \
                 (an external dependency"
            ),
            "{other}"
        );
        // Only cargo's failure can say so: any other error is the general
        // message.
        let spawn = Error::Spawn {
            command: "cargo metadata".to_string(),
            source: std::io::Error::other("needs to be updated but --locked was passed"),
        };
        let (kind, spawned) = unresolved("Cargo.toml", &spawn, false);
        assert_eq!(kind, Unresolved::Other);
        assert!(spawned.contains("(an external dependency"), "{spawned}");
        // When what the members declare is wrong too, the lockfile is not
        // the only problem, and neither the kind nor the message says it is.
        let failed = Error::CommandFailed {
            command: "cargo metadata".to_string(),
            status: std::process::ExitStatus::default(),
            stderr: "error: cannot update the lock file /w/Cargo.lock because --locked was \
                     passed to prevent this\n"
                .to_string(),
        };
        let (kind, only) = unresolved("Cargo.toml", &failed, false);
        assert_eq!(kind, Unresolved::StaleLockfile);
        assert!(only.contains("Cargo.lock is out of date"), "{only}");
        let (kind, both) = unresolved("Cargo.toml", &failed, true);
        assert_eq!(kind, Unresolved::Other);
        assert!(both.contains("(an external dependency"), "{both}");
        assert!(!both.contains("Cargo.lock is out of date"), "{both}");
        // The kind comes from cargo's error, not from the words of the
        // message: a path holding them changes nothing.
        let (kind, _) = unresolved(
            "Cargo.lock is out of date/Cargo.toml",
            &Error::UnexpectedOutput {
                command: "cargo metadata".to_string(),
                detail: "Cargo.lock is out of date".to_string(),
            },
            false,
        );
        assert_eq!(kind, Unresolved::Other);
    }

    #[test]
    fn what_an_unresolved_workspace_declares_is_still_checked() {
        // `cargo metadata --no-deps` of a workspace whose lockfile is out of
        // date: a member declares a path dependency outside the repository,
        // one on a crate that is no member, and one from crates.io.
        let mut core = member("tidepool-core").registry_dep("left-pad", "^1");
        core.dependencies.push(Dependency {
            name: "evil".to_string(),
            source: None,
            req: "*".to_string(),
            path: Some(PathBuf::from("/elsewhere/evil")),
            kind: Kind::Normal,
        });
        core.dependencies.push(Dependency {
            name: "pad".to_string(),
            source: None,
            req: "*".to_string(),
            path: Some(PathBuf::from(format!("{ROOT}/vendor/pad"))),
            kind: Kind::Build,
        });
        let text = member("tidepool-text").path_dep("tidepool-core", Kind::Normal);
        let mut far = member("tidepool-far");
        far.manifest_path = PathBuf::from("/elsewhere/far/Cargo.toml");
        let mut violations = Vec::new();
        check_declared(
            &graph(vec![core, text, far]),
            &[],
            &repository(),
            &mut violations,
        );
        assert_eq!(
            violations,
            [
                format!(
                    "`tidepool-core` declares `left-pad ^1` (normal dependency) from \
                     `{CRATES_IO}`, which is not in xtask/deps-allowlist.txt"
                ),
                "`tidepool-core` declares `evil *` (normal dependency) from /elsewhere/evil, \
                 outside the repository: vendored or cross-repository code counts as a dependency"
                    .to_string(),
                "`tidepool-core` declares `pad *` (build-dependency) from vendor/pad, a path \
                 package in the repository that is not a member of this workspace: vendored or \
                 copied code, or another workspace's crate, counts as a dependency"
                    .to_string(),
                "`tidepool-far 0.1.0` is a workspace member outside the repository \
                 (/elsewhere/far/Cargo.toml): vendored or cross-repository code counts as a \
                 dependency"
                    .to_string(),
            ]
        );
        // A path dependency on a member is fine.
        let mut violations = Vec::new();
        let ok = graph(vec![
            member("tidepool-core"),
            member("tidepool-text").path_dep("tidepool-core", Kind::Dev),
        ]);
        check_declared(&ok, &[], &repository(), &mut violations);
        assert_eq!(violations, Vec::<String>::new());
    }

    #[test]
    fn only_the_root_workspace_and_agents_with_reference_agents_may_exist() {
        let workspace = |root: &str, members: Vec<Package>| Metadata {
            workspace_root: PathBuf::from(root),
            packages: members,
        };
        let agent = |name: &str, dir: &str| {
            let mut package = member(name);
            package.manifest_path = PathBuf::from(format!("{ROOT}/agents/{dir}/Cargo.toml"));
            package
        };
        let check = |metadata: &Metadata| {
            let mut violations = Vec::new();
            check_workspace("x/Cargo.toml", metadata, &repository(), &mut violations);
            violations
        };
        // The root workspace, whatever its members (`layers` checks them).
        assert_eq!(
            check(&workspace(ROOT, vec![member("tidepool-core")])),
            Vec::<String>::new()
        );
        // `agents/` with the reference agents in their directories.
        let agents = format!("{ROOT}/agents");
        assert_eq!(
            check(&workspace(
                &agents,
                vec![
                    agent("tp-agent-mock", "tp-agent-mock"),
                    agent("tp-agent-claude", "tp-agent-claude")
                ]
            )),
            Vec::<String>::new()
        );
        // Copied code as a member of `agents/`, under its own name (in a
        // directory of that name, as a reference agent has, or another), under
        // a reference agent's name in another directory, or outside it.
        let mut outside = member("tp-agent-local");
        outside.manifest_path = PathBuf::from(format!("{ROOT}/tools/left-pad/Cargo.toml"));
        let found = check(&workspace(
            &agents,
            vec![
                agent("tp-agent-mock", "tp-agent-mock"),
                agent("left-pad", "vendor/left-pad"),
                agent("tp-agent-local", "vendor/left-pad"),
                outside,
                agent("left-pad", "left-pad"),
            ],
        ));
        assert_eq!(found.len(), 4, "{found:?}");
        assert_eq!(
            found[0],
            "`left-pad 0.1.0` (agents/vendor/left-pad/Cargo.toml) is a member of agents/, which \
             may hold only the reference agents tp-agent-mock, tp-agent-local, tp-agent-claude, \
             each in agents/<name>/ (docs/subsystems/agents.md): vendored or copied code cannot \
             join it"
        );
        assert!(found[1].starts_with("`tp-agent-local 0.1.0` (agents/vendor/left-pad/Cargo.toml)"));
        assert!(found[2].starts_with("`tp-agent-local 0.1.0` (tools/left-pad/Cargo.toml)"));
        assert!(found[3].starts_with("`left-pad 0.1.0` (agents/left-pad/Cargo.toml) is a member"));
        // Any other workspace, inside the repository or not.
        for (root, shown) in [
            (format!("{ROOT}/vendor/left-pad"), "vendor/left-pad"),
            (format!("{ROOT}/agents/tool"), "agents/tool"),
            ("/elsewhere/ws".to_string(), "/elsewhere/ws"),
        ] {
            assert_eq!(
                check(&workspace(&root, vec![member("left-pad")])),
                [format!(
                    "x/Cargo.toml belongs to the workspace at {shown}, but the repository has two \
                     workspaces only, the root one and agents/ (docs/01-architecture.md): code in \
                     any other is vendored or copied code that no check reads as part of Tidepool"
                )]
            );
        }
    }

    #[test]
    fn the_reference_agents_are_the_ones_the_agents_document_lists() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/subsystems/agents.md");
        let text = fs::read_to_string(path).expect("the agents document exists");
        let listed: Vec<&str> = text
            .lines()
            .filter_map(|line| line.strip_prefix("| `tp-agent-"))
            .filter_map(|rest| rest.split_once('`'))
            .map(|(name, _)| name)
            .collect();
        let expected: Vec<String> = REFERENCE_AGENTS
            .iter()
            .map(|name| name.trim_start_matches("tp-agent-").to_string())
            .collect();
        assert_eq!(listed, expected);
    }

    /// The violations [`check_cargo_config`] finds in `text`.
    fn config_violations(text: &str) -> Vec<String> {
        let mut violations = Vec::new();
        check_cargo_config(".cargo/config.toml", text, &mut violations);
        violations
    }

    #[test]
    fn a_cargo_configuration_may_hold_only_aliases() {
        // The real one passes.
        let real =
            fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.cargo/config.toml"))
                .expect("the repository has a cargo configuration");
        assert_eq!(config_violations(&real), Vec::<String>::new());
        assert_eq!(
            config_violations(
                "# Aliases.\n\n[alias] # ours\nxtask = \"run --package xtask --\"\n\
                 tp    = [\"run\", \"--package\", \"tidepool\"] # array form\n"
            ),
            Vec::<String>::new()
        );
        // Everything else that could change the build or what xtask sees.
        for (text, line) in [
            (
                "[alias]\nx = \"y\"\n\n[env]\nCARGO_TARGET_DIR = { value = \"agents\", relative = \
                 true, force = true }\n",
                4,
            ),
            (
                "[build]\nrustflags = [\"-L\", \"third_party/prebuilt\"]\n",
                1,
            ),
            ("[target.x86_64-unknown-linux-gnu]\nlinker = \"evil\"\n", 1),
            (
                "[source.crates-io]\nreplace-with = \"v\"\n[source.v]\ndirectory = \"x\"\n",
                1,
            ),
            (
                "[patch.crates-io]\nleft-pad = { path = \"vendor/left-pad\" }\n",
                1,
            ),
            ("paths = [\"vendor/left-pad\"]\n", 1),
            ("env.CARGO_TARGET_DIR = \"agents\"\n", 1),
            ("[alias]\nenv.CARGO_TARGET_DIR = \"agents\"\n", 2),
            ("[ alias ]\nx = \"y\"\n", 1),
            ("[alias]\n\"quoted\" = \"y\"\n", 2),
            ("[alias]\nx = \"\"\"\nrun\n\"\"\"\n", 2),
            ("[alias]\nx = 'run'\n", 2),
            ("[alias]\nx = \"run\\u0020x\"\n", 2),
            ("[alias]\nx = [\n  \"run\",\n]\n", 2),
            ("[alias]\nx = \"run\" extra\n", 2),
            (
                "[alias]\nxtask = \"run --config env.CARGO_TARGET_DIR='agents' --package xtask\"\n",
                2,
            ),
            (
                "[alias]\nx = [\"run\", \"--config=build.rustflags=[]\"]\n",
                2,
            ),
            ("[[alias]]\nx = \"y\"\n", 1),
            // The header is exactly `[alias]`, and nothing follows an array.
            ("[aliases]\nx = \"y\"\n", 1),
            ("[alias.x]\ny = \"z\"\n", 1),
            ("[alias]\nx = [\"run\"] extra\n", 2),
        ] {
            let found = config_violations(text);
            assert!(
                found
                    .first()
                    .is_some_and(|first| first.starts_with(&format!(".cargo/config.toml:{line}: "))),
                "{text}:\n{found:?}"
            );
        }
        assert_eq!(
            config_violations("[env]\nX = \"1\"\n"),
            [
                ".cargo/config.toml:1: only an `[alias]` table is allowed, but found `[env]`: cargo \
                 configuration in the repository may hold only `[alias]` with one-line string \
                 values, since `[env]`, `[build]`, `[target]`, `[source]`, `[patch]`, `paths` and \
                 the rest could change what cargo builds, or what `xtask` sees, without any \
                 manifest saying so",
                ".cargo/config.toml:2: a key outside `[alias]`, but found `X = …`: cargo \
                 configuration in the repository may hold only `[alias]` with one-line string \
                 values, since `[env]`, `[build]`, `[target]`, `[source]`, `[patch]`, `paths` and \
                 the rest could change what cargo builds, or what `xtask` sees, without any \
                 manifest saying so",
            ]
        );
    }

    #[test]
    fn credentials_in_a_source_url_are_never_shown() {
        for (text, shown) in [
            (
                "git+https://FAKETOKEN@example.invalid/x",
                "git+https://…@example.invalid/x",
            ),
            (
                "registry+https://u:FAKEPW@h/index",
                "registry+https://…@h/index",
            ),
            ("sparse+https://h/a@b", "sparse+https://h/a@b"),
            (
                "a git+ssh://git@h/x and https://k@h",
                "a git+ssh://…@h/x and https://…@h",
            ),
            (
                "`source = \"git+https://t@h\"`",
                "`source = \"git+https://…@h\"`",
            ),
            ("no url", "no url"),
        ] {
            assert_eq!(without_userinfo(text), shown, "{text}");
        }
        let pins = [];
        let message = pin_problem("x", "1.0.0", "git+https://FAKETOKEN@h/x", &pins);
        assert!(message.is_some_and(|m| !m.contains("FAKETOKEN")));
        let mut violations = Vec::new();
        check_lockfile(
            "Cargo.lock",
            "[[package]]\nname = \"zz\"\nversion = \"1.0.0\"\nsource = \"registry+https://FAKETOKEN@h/i\"\n\
             package = [{ source = \"git+https://FAKETOKEN@h/x\" }]\n",
            &pins,
            &mut violations,
        );
        assert_eq!(violations.len(), 2, "{violations:?}");
        assert!(
            violations.iter().all(|v| !v.contains("FAKETOKEN")),
            "{violations:?}"
        );
    }

    #[test]
    fn a_value_opened_after_a_table_header_is_followed_too() {
        for text in [
            "[env] A = \"\"\"\nFAKESEC25 = 1\n[FAKESEC25b]\n\"\"\"\n",
            "[env]A = [\nFAKESEC26 = 1,\n]\n",
            "[[bin]] x = '''\nFAKESEC27 = 1\n'''\n",
        ] {
            let found = config_violations(text);
            assert!(!found.is_empty(), "{text}");
            assert!(
                found.iter().all(|v| !v.contains("FAKESEC")),
                "{text}:\n{found:?}"
            );
        }
    }

    #[test]
    fn a_configuration_value_is_never_shown() {
        // A registry token is the obvious secret a cargo configuration
        // can hold; it must not reach the output (AGENTS.md rule 7).
        let found = config_violations(
            "[registry]\ntoken = \"s3cr3t\"\n[alias]\nx = \"run --config=s3cr3t\"\n\
             [registries.x] token = \"s3cr3t\"\n[[s3cr3t\n\"s3cr3t\",\n",
        );
        assert_eq!(found.len(), 6, "{found:?}");
        assert!(
            found.iter().all(|violation| !violation.contains("s3cr3t")),
            "{found:?}"
        );
        // A line that continues a multi-line string or array has no key,
        // whatever it looks like; a quoted key, or a header's quoted segment,
        // can hold a secret too.
        let found = config_violations(
            "[registry]\ntoken = \"\"\"\nFAKETOKENA0123==\n\"\"\"\n\
             [env]\nCMD = [\n  \"run\",\n  \"SECRET=x\",\n]\n\
             [patch.\"https://ghp_SECRET@github.com/org/repo\"]\n\
             [registries.\"tok\"]\n\"tok=en\" = 1\n",
        );
        let shown: Vec<(&str, &str)> = found
            .iter()
            .map(|violation| {
                let (line, rest) = violation
                    .strip_prefix(".cargo/config.toml:")
                    .and_then(|rest| rest.split_once(": "))
                    .expect("a located violation");
                let shown = rest
                    .split_once("but found `")
                    .and_then(|(_, found)| found.split_once('`'))
                    .map(|(found, _)| found)
                    .expect("the violation shows what it found");
                (line, shown)
            })
            .collect();
        assert_eq!(
            shown,
            [
                ("1", "[registry]"),
                ("2", "token = …"),
                ("3", "…"),
                ("4", "…"),
                ("5", "[env]"),
                ("6", "CMD = …"),
                ("7", "…"),
                ("8", "…"),
                ("9", "…"),
                ("10", "[patch.…]"),
                ("11", "[registries.…]"),
                ("12", "…"),
            ],
            "{found:?}"
        );
        assert!(
            found[2].starts_with(
                ".cargo/config.toml:3: a line that continues a multi-line value, but found `…`: \
                 cargo configuration in the repository may hold only `[alias]` with one-line \
                 string values"
            ),
            "{found:?}"
        );
        for secret in [
            "FAKETOKENA0123",
            "SECRET",
            "ghp_",
            "\"tok",
            "tok=en",
            "https",
        ] {
            assert!(
                found.iter().all(|violation| !violation.contains(secret)),
                "{secret}: {found:?}"
            );
        }
        for (line, shown) in [
            ("[registry] # a comment", "[registry]"),
            ("[registry] # a [comment]", "[registry]"),
            ("[ alias ]", "[ alias ]"),
            ("[patch.crates-io]", "[patch.…]"),
            (
                "[patch.\"https://ghp_SECRET@github.com/org/repo\"]",
                "[patch.…]",
            ),
            ("[registries.\"tok\"]", "[registries.…]"),
            ("[\"x\"]", "[…]"),
            ("[\"x.y\"]", "[…]"),
            ("[]", "[…]"),
            ("[[bin]]", "[[bin]]"),
            ("[[a.b]] x", "[[a.…]]"),
            ("[[\"a\".b]]", "[[…]]"),
            ("[unclosed", "[…"),
            ("[[unclosed]", "[…"),
            ("[patch.\"x\"", "[…"),
            ("token = \"x\"", "token = …"),
            ("a.b-c = 1", "a.b-c = …"),
            ("a . b_c=1", "a . b_c = …"),
            ("\"tok=en\" = 1", "…"),
            ("a.\"b\" = 1", "…"),
            ("a..b = 1", "…"),
            ("a b = 1", "…"),
            ("= 1", "…"),
            ("\"x\",", "…"),
        ] {
            assert_eq!(shown_config_line(line), shown, "{line}");
        }
    }

    #[test]
    fn a_multi_line_value_is_followed_to_its_end() {
        // What each line leaves open, from nothing open.
        let basic = Open {
            string: Some(b'"'),
            brackets: 0,
        };
        let literal = Open {
            string: Some(b'\''),
            brackets: 0,
        };
        let array = |brackets| Open {
            string: None,
            brackets,
        };
        for (line, open) in [
            ("x = \"y\"", Open::default()),
            ("x = \"\"", Open::default()),
            ("x = \"\"\"\"\"\"", Open::default()),
            ("x = \"\"\"y\"\"\"\"\" # z", Open::default()),
            ("x = \"\"\"", basic),
            ("x = \"\"\"y\\\"\"\"", basic),
            ("x = '''", literal),
            ("x = '''y\\'''", Open::default()),
            ("x = '\"\"\"'", Open::default()),
            ("x = \"'''\"", Open::default()),
            ("x = [", array(1)),
            ("x = [\"]\", '['", array(1)),
            ("x = [[1, 2], [", array(2)),
            ("x = { a = [1,", array(2)),
            ("x = { a = 1 }", Open::default()),
            ("x = 1 # [ \"\"\"", Open::default()),
            ("x = \"# [\" # \"", Open::default()),
            // A one-line string cannot go on to the next line.
            ("x = \"unterminated [", Open::default()),
            ("x = ]", Open::default()),
        ] {
            assert_eq!(Open::default().after(line), open, "{line}");
        }
        // Where an open value ends.
        for (open, line, after) in [
            (basic, "still \\\"\"\" inside", basic),
            (basic, "ends\"\"\"", Open::default()),
            (basic, "ends \"\"\"\"\" x = [", array(1)),
            (basic, "a line-ending backslash \\", basic),
            (literal, "no escape \\'''", Open::default()),
            (array(1), "\"a\", # ]", array(1)),
            (array(2), "]]", Open::default()),
            (
                array(1),
                "\"\"\"",
                Open {
                    string: Some(b'"'),
                    brackets: 1,
                },
            ),
        ] {
            assert_eq!(open.after(line), after, "{line}");
        }

        // Once the value ends, the next line is read as usual: a header
        // switches tables again, and `[alias]` passes.
        let found = config_violations(
            "[env]\nX = [\n  \"a\",\n  # a comment\n\n  \"b\",\n]\n[alias]\nx = \"y\"\n",
        );
        let lines: Vec<&str> = found
            .iter()
            .filter_map(|violation| violation.strip_prefix(".cargo/config.toml:"))
            .filter_map(|rest| rest.split_once(':').map(|(line, _)| line))
            .collect();
        assert_eq!(lines, ["1", "2", "3", "6", "7"], "{found:?}");
        // A line that continues a value is not a table header, even when it
        // starts like one, and inside `[alias]` it fails too.
        let found = config_violations("[alias]\nx = [\n[env]\n]\ny = \"z\"\n");
        assert_eq!(found.len(), 3, "{found:?}");
        assert!(found[0].starts_with(
            ".cargo/config.toml:2: an array that does not end on its line, but found `x = …`"
        ));
        for (violation, line) in found[1..].iter().zip([3, 4]) {
            assert!(
                violation.starts_with(&format!(
                    ".cargo/config.toml:{line}: a line that continues a multi-line value, but \
                     found `…`"
                )),
                "{found:?}"
            );
        }
        let found = config_violations("[alias]\nx = \"\"\"\ny = \"run\"\n\"\"\"\nz = \"w\"\n");
        assert_eq!(found.len(), 3, "{found:?}");
        assert!(found[1].starts_with(".cargo/config.toml:3: a line that continues"));
        assert!(found[2].starts_with(".cargo/config.toml:4: a line that continues"));
    }

    #[test]
    fn a_byte_order_mark_is_read_as_cargo_reads_it() {
        // Cargo skips one leading byte-order mark, which editors on
        // Windows add; so do the configuration and allowlist readers.
        assert_eq!(
            config_violations("\u{feff}[alias]\nhello = \"version\"\n"),
            Vec::<String>::new()
        );
        let mut violations = Vec::new();
        let pins = parse_allowlist(
            "\u{feff}# Approved.\nleft-pad 1.0.3 ADR-0004\n",
            &mut violations,
        );
        assert_eq!(violations, Vec::<String>::new());
        assert_eq!(pins, [pin("left-pad", "1.0.3")]);
        let mut violations = Vec::new();
        let pins = parse_allowlist("\u{feff}left-pad 1.0.3 ADR-0004\n", &mut violations);
        assert_eq!(violations, Vec::<String>::new());
        assert_eq!(pins, [pin("left-pad", "1.0.3")]);
        // Only one, and only at the start: any other is read as text.
        for (text, line) in [
            ("\u{feff}\u{feff}[alias]\nx = \"y\"\n", 1),
            ("[alias]\n\u{feff}x = \"y\"\n", 2),
        ] {
            let found = config_violations(text);
            assert!(
                found
                    .first()
                    .is_some_and(|first| first.starts_with(&format!(".cargo/config.toml:{line}: "))),
                "{text:?}:\n{found:?}"
            );
        }
        let mut violations = Vec::new();
        let pins = parse_allowlist(
            "# Approved.\n\u{feff}left-pad 1.0.3 ADR-0004\n",
            &mut violations,
        );
        assert!(!pins.contains(&pin("left-pad", "1.0.3")), "{pins:?}");
    }

    #[test]
    fn cargo_configuration_files_are_found_by_name_in_any_case() {
        for path in [
            ".cargo/config.toml",
            ".cargo/config",
            "agents/.cargo/config.toml",
            ".Cargo/Config.TOML",
        ] {
            assert!(is_cargo_config(Path::new(path)), "{path}");
        }
        for path in [
            "config.toml",
            "cargo/config.toml",
            ".cargo/config.json",
            "docs/config",
        ] {
            assert!(!is_cargo_config(Path::new(path)), "{path}");
        }
    }

    #[test]
    fn compiled_artifacts_are_known_by_name_or_first_bytes() {
        for name in [
            "libleft_pad.rlib",
            "libleft_pad.rmeta",
            "libx.a",
            "x.o",
            "x.obj",
            "libx.so",
            "libx.dylib",
            "x.dll",
            "x.lib",
            "x.exe",
            "x.wasm",
            "LIBX.RLIB",
            // The extension is what follows the last dot.
            "libleft_pad-1.0.3.rlib",
            "x.v2.o",
        ] {
            assert!(artifact_extension(name).is_some(), "{name}");
        }
        for name in ["lib.rs", "x.toml", "notes", "x.so.txt", "a", "README.md"] {
            assert_eq!(artifact_extension(name), None, "{name}");
        }
        let mut pe = vec![0_u8; 0x90];
        pe[..2].copy_from_slice(b"MZ");
        pe[0x3c] = 0x80;
        pe[0x80..0x84].copy_from_slice(b"PE\0\0");
        for (head, expected) in [
            (
                &b"\x7fELF\x02\x01\x01"[..],
                "an ELF object, library or executable",
            ),
            // 32- and 64-bit, big- and little-endian.
            (b"\xfe\xed\xfa\xce\x00", "a Mach-O object"),
            (b"\xfe\xed\xfa\xcf\x01", "a Mach-O object"),
            (b"\xce\xfa\xed\xfe\x07", "a Mach-O object"),
            (b"\xcf\xfa\xed\xfe\x07", "a Mach-O object"),
            (
                b"\xca\xfe\xba\xbe\x00\x00\x00\x02",
                "a universal Mach-O binary or a Java class",
            ),
            (
                b"\xca\xfe\xba\xbf\x00\x00\x00\x02",
                "a 64-bit universal Mach-O binary",
            ),
            (
                b"!<arch>\n/               0",
                "an `ar` archive: an rlib or static library",
            ),
            (b"!<thin>\n/               0", "a thin `ar` archive"),
            (b"rust\0\0\0\x09", "rustc metadata"),
            (b"\0asm\x01\0\0\0", "WebAssembly"),
            (b"BC\xc0\xde\x35\x14", "LLVM bitcode"),
            (b"\xde\xc0\x17\x0b\x00\x00\x00\x00", "wrapped LLVM bitcode"),
            (&pe, "a PE executable or DLL"),
        ] {
            assert_eq!(magic(head), Some(expected), "{head:?}");
        }
        // Text, and near misses.
        pe[0x80] = b'X';
        for head in [
            &b"//! Text.\n"[..],
            b"MZ is a text",
            &pe,
            b"!<arch",
            b"",
            b"\x7fEL",
            b"!<thin>",
            b"BC\xc0",
            b"\xca\xfe\xba",
            b"\xca\xfe\xba\xb0",
            b"\xde\xc0\x17",
            b"\x0b\x17\xc0\xde",
        ] {
            assert_eq!(magic(head), None, "{head:?}");
        }
    }

    #[test]
    fn a_pe_file_is_known_by_the_signature_its_header_points_to() {
        // Real executables keep the `PE\0\0` signature at 0x80 to 0x118:
        // past the first bytes, but inside what is read.
        let tree = TempTree::new("deps-pe");
        let mut pe = vec![0_u8; 0x200];
        pe[..2].copy_from_slice(b"MZ");
        pe[0x3c] = 0x00;
        pe[0x3d] = 0x01;
        pe[0x100..0x104].copy_from_slice(b"PE\0\0");
        tree.write("tool.bin", &pe);
        assert_eq!(
            artifact(&tree.path().join("tool.bin"), "tool.bin").unwrap(),
            Some("a PE executable or DLL by its first bytes".to_string())
        );
        // The signature at the last bytes that are read still counts.
        let mut far = vec![0_u8; 0x800];
        far[..2].copy_from_slice(b"MZ");
        far[0x3c] = 0xfc;
        far[0x3d] = 0x03;
        far[0x3fc..0x400].copy_from_slice(b"PE\0\0");
        tree.write("far.bin", &far);
        assert_eq!(
            artifact(&tree.path().join("far.bin"), "far.bin").unwrap(),
            Some("a PE executable or DLL by its first bytes".to_string())
        );
    }

    #[test]
    fn a_file_that_cannot_be_read_is_a_violation_and_the_check_goes_on() {
        // A directory stands in for a file that cannot be read: opening one
        // fails on Windows, and reading one fails elsewhere.
        let tree = TempTree::new("deps-unreadable");
        tree.write("docs/private/notes.txt", "notes\n");
        tree.write("bin/tool", "\x7fELF");
        let root = tree.path();
        let unreadable = artifact_violation(root, &root.join("docs/private"), "private");
        assert!(
            unreadable.as_deref().is_some_and(|violation| violation
                .starts_with("docs/private cannot be read (")
                && violation.ends_with("), so it was not checked for compiled code")),
            "{unreadable:?}"
        );
        assert!(
            artifact_violation(root, &root.join("gone.txt"), "gone.txt")
                .is_some_and(|violation| violation.starts_with("gone.txt cannot be read ("))
        );
        assert_eq!(
            artifact_violation(root, &root.join("docs/private/notes.txt"), "notes.txt"),
            None
        );
        assert!(
            artifact_violation(root, &root.join("bin/tool"), "tool").is_some_and(|violation| {
                violation.starts_with("bin/tool is a compiled artifact (an ELF object")
            })
        );
    }

    #[test]
    fn files_that_may_hold_secrets_are_never_opened() {
        // `AGENTS.md` rule 7: `xtask` never reads secrets. Such a file is
        // judged by its name alone: a missing one is no error, and the
        // content of one that exists is never looked at.
        let tree = TempTree::new("deps-secret-names");
        for name in [
            ".env",
            ".env.local",
            "release.jks",
            "upload.keystore",
            "cert.P12",
            "AuthKey.p8",
            "server.pem",
            "id.key",
            "app.mobileprovision",
            "x.provisionprofile",
            "X.PROVISIONPROFILE",
            "keystore.properties",
        ] {
            assert!(is_secret_name(name), "{name}");
            tree.write(name, "\x7fELF");
            assert_eq!(
                artifact(&tree.path().join(name), name).unwrap(),
                None,
                "{name}"
            );
            let gone = tree.path().join("gone").join(name);
            assert_eq!(artifact(&gone, name).unwrap(), None, "{name}");
        }
        for name in ["env.rs", "keys.md", "key.rs", "Cargo.toml", "environment"] {
            assert!(!is_secret_name(name), "{name}");
        }
        // Any other file is opened, and one that cannot be is an error.
        tree.write("blob.bin", "\x7fELF");
        assert!(
            artifact(&tree.path().join("blob.bin"), "blob.bin")
                .unwrap()
                .is_some()
        );
        assert!(artifact(&tree.path().join("missing.bin"), "missing.bin").is_err());
    }

    #[test]
    fn manifests_and_lockfiles_are_found_in_any_case() {
        // On macOS and Windows cargo opens `tools/cargo.toml` and
        // `tools/cargo.lock` as that workspace's own files.
        let tree = TempTree::new("deps-case");
        tree.write("xtask/deps-allowlist.txt", "# Nothing approved.\n");
        tree.write(
            "tools/cargo.toml",
            "[package]\nname = \"tools\"\nversion = \"0.1.0\"\n\n[workspace]\n\n\
             [dependencies]\nleft-pad = \"1\"\n",
        );
        tree.write(
            "tools/cargo.lock",
            "version = 4\n\n[[package]]\nname = \"left-pad\"\nversion = \"1.0.3\"\n\
             source = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
        );
        let report = run(tree.path()).expect("the tree can be read");
        assert_eq!(report.violations.len(), 2, "{report:?}");
        assert!(
            report.violations[0].starts_with("cannot resolve the workspace of tools/cargo.toml"),
            "{report:?}"
        );
        assert!(
            report.violations[1].starts_with(
                "tools/cargo.lock:6: `left-pad 1.0.3` comes from \
                 `registry+https://github.com/rust-lang/crates.io-index`"
            ),
            "{report:?}"
        );
    }

    #[test]
    fn a_tree_without_a_manifest_fails() {
        let tree = TempTree::new("deps-empty");
        tree.write("xtask/deps-allowlist.txt", "# Nothing approved.\n");
        let report = run(tree.path()).expect("the tree can be read");
        assert_eq!(
            report.violations,
            ["no Cargo.toml under the repository root: nothing was checked"]
        );
    }

    #[test]
    fn a_missing_allowlist_is_an_error() {
        let tree = TempTree::new("deps-no-allowlist");
        match run(tree.path()) {
            Err(Error::Read { path, .. }) => assert!(path.ends_with(ALLOWLIST)),
            other => panic!("expected a read error, got {other:?}"),
        }
    }

    #[test]
    fn the_real_repository_passes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let report = run(&root).expect("the check runs on the real repository");
        assert_eq!(report.violations, Vec::<String>::new(), "{report:?}");
        // The root workspace is found first; `agents/` adds another later.
        assert!(
            report.notes[0].starts_with("workspaces: ") && report.notes[0].contains(" (Cargo.toml"),
            "{:?}",
            report.notes
        );
    }
}
