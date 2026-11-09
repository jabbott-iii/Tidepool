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

//! `cargo xtask workflows`: the GitHub Actions workflows keep the CI
//! supply-chain rules of ADR-0022.
//!
//! Ported from nadir xtask `workflows.rs` (Nadir's ADR-0032). Adapted for
//! Tidepool: its table ([`WORKFLOWS`]); the directory and each workflow
//! found without following or resolving a symbolic link, and read within a
//! size bound ([`walk::read_bounded`]), as M0.10 has every check do; paths
//! shown escaped ([`walk::relative`]); a missing directory, or one with no
//! workflow, fails, as `deps` fails on a tree with no `Cargo.toml`; a
//! local action (`./…`) fails like any other action its row does not name,
//! since the check does not read its steps; jobs found at whatever
//! indentation the file gives them; a permission's level read without its
//! quotes, and one that is not `read`, `write` or `none` failed;
//! `persist-credentials: false` counted only as an input in the checkout's
//! `with:`, in a step that starts at its dash, wherever that is; keys read
//! by name, as YAML ends them ([`split_key`]); and the YAML that a line
//! cannot show failed closed, as listed below.
//!
//! The check is specified in `docs/04-quality.md` → "`workflows`". The
//! workflows are the `.yml` and `.yaml` files directly in
//! `.github/workflows/`, where GitHub reads them, the extension in any case,
//! as `headers` finds them. In each:
//!
//! - every action is one its row of [`WORKFLOWS`] names, and a file with no
//!   row fails;
//! - every action is pinned by a full commit SHA, with its tag in a comment
//!   on the same line, as Dependabot keeps it;
//! - the token's default permissions, at the top of the file, are `{}` or
//!   `contents: read`, and a job grants `write` only on the permissions its
//!   row names for that job;
//! - nothing runs on `pull_request_target`, which gives a pull request's
//!   code the base repository's token;
//! - every `actions/checkout` sets `persist-credentials: false`, so the
//!   token is not left in `.git/config` for later steps.
//!
//! It reads the files line by line, not as YAML: the workflows are written
//! in block style, one key a line (`.claude/rules/ci-workflows.md`). What a
//! YAML reader would read differently from the lines fails, never passes:
//!
//! - a line break other than LF or CR LF, after which the file is not
//!   checked further ([`other_line_breaks`]);
//! - a quoted value or flow collection that does not end on its line, a
//!   mapping in flow style other than `{}`, and an alias or anchor as a
//!   value ([`scan`]);
//! - a tag or anchor before a key it reads, an explicit key (`? `), a
//!   double-quoted key with an escape, and a quoted top-level key or one
//!   with a `\`;
//! - in the top-level `on:` section, a `\`, and a ` #` inside a quoted
//!   value, either of which could hide `pull_request_target`;
//! - a YAML document marker or directive, but for one `---` at the start
//!   ([`document_marker`]).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::check::Report;
use crate::error::Error;
use crate::walk;

/// Where the workflows are, relative to the root, with `/` between
/// components.
const DIR: &str = ".github/workflows";

/// The largest workflow read, in bytes. A workflow written one key a line
/// is a few KiB; a larger file is reported unread.
const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// What one workflow may use.
struct Workflow {
    /// Its file name in [`DIR`].
    file: &'static str,
    /// The actions it may use, as `owner/repo` or `owner/repo/path`.
    actions: &'static [&'static str],
    /// The jobs that may grant `write`, each with the permissions it may.
    writes: &'static [(&'static str, &'static [&'static str])],
}

/// The action that checks the repository out, which every workflow uses.
const CHECKOUT: &str = "actions/checkout";
/// The action that caches cargo's files in `ci.yml` and `pages.yml`.
const CACHE: &str = "actions/cache";
/// GitHub's actions that upload and publish a Pages site, in `pages.yml`.
const UPLOAD_PAGES_ARTIFACT: &str = "actions/upload-pages-artifact";
const DEPLOY_PAGES: &str = "actions/deploy-pages";

/// Every workflow, and what it may use (ADR-0022, ADR-0030, and
/// `docs/04-quality.md` → "CI (GitHub Actions)"). A new workflow adds its
/// row here first; the release workflow's comes with roadmap M14.4.
const WORKFLOWS: &[Workflow] = &[
    Workflow {
        file: "ci.yml",
        actions: &[CHECKOUT, CACHE],
        writes: &[],
    },
    Workflow {
        file: "codeql.yml",
        actions: &[
            CHECKOUT,
            "github/codeql-action/init",
            "github/codeql-action/analyze",
        ],
        writes: &[("analyze", &["security-events"])],
    },
    Workflow {
        file: "pages.yml",
        actions: &[CHECKOUT, CACHE, UPLOAD_PAGES_ARTIFACT, DEPLOY_PAGES],
        writes: &[("deploy", &["pages", "id-token"])],
    },
];

/// Runs the check on the workflows of the repository at `root`.
///
/// # Errors
///
/// [`Error::Read`] if the directory cannot be listed, or a workflow cannot
/// be read.
pub(crate) fn run(root: &Path) -> Result<Report, Error> {
    check_dir(root, MAX_FILE_BYTES)
}

/// [`run`], with workflows larger than `limit` bytes reported unread.
fn check_dir(root: &Path, limit: u64) -> Result<Report, Error> {
    let mut report = Report::default();
    let mut checked = 0;
    match directory_problem(root)? {
        Some(problem) => report.violations.push(problem),
        None => {
            let found = list(&root.join(DIR))?;
            for link in &found.links {
                report.violations.push(format!(
                    "{}: a symbolic link, which `workflows` does not follow, so it was not read",
                    walk::relative(root, link)
                ));
            }
            for other in &found.others {
                report.violations.push(format!(
                    "{}: not a regular file, so it was not read",
                    walk::relative(root, other)
                ));
            }
            for path in &found.workflows {
                let shown = walk::relative(root, path);
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                match walk::read_bounded(path, limit) {
                    Ok(text) => {
                        checked += 1;
                        check(&shown, &name, &text, &mut report.violations);
                    }
                    Err(walk::ReadError::TooLarge { limit }) => report.violations.push(format!(
                        "{shown}: larger than {limit} bytes, so it was not checked"
                    )),
                    Err(walk::ReadError::NotUtf8) => report
                        .violations
                        .push(format!("{shown}: not UTF-8, so it was not checked")),
                    Err(walk::ReadError::Io(source)) => {
                        return Err(Error::Read {
                            path: path.clone(),
                            source,
                        });
                    }
                }
            }
            if found.workflows.is_empty() {
                report.violations.push(format!(
                    "{DIR} holds no workflow (a `.yml` or `.yaml` file), so nothing was checked"
                ));
            }
        }
    }
    report.notes.push(format!("workflows checked: {checked}"));
    Ok(report)
}

/// Why the workflows cannot be listed, if they cannot: a directory on the
/// way to [`DIR`], checked shallowest first, is missing, is a symbolic link
/// (not followed, nor resolved) or is not a directory. Without this, a
/// check of the wrong directory would pass by finding nothing.
///
/// # Errors
///
/// [`Error::Read`] if the OS cannot say what a directory on the way is.
fn directory_problem(root: &Path) -> Result<Option<String>, Error> {
    let mut at = root.to_path_buf();
    for component in DIR.split('/') {
        at.push(component);
        let problem = match fs::symlink_metadata(&at) {
            Ok(meta) if meta.file_type().is_symlink() => {
                "is a symbolic link, which `workflows` does not follow, so no workflow was checked"
            }
            Ok(meta) if meta.file_type().is_dir() => continue,
            Ok(_) => "is not a directory, so no workflow was checked",
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Some(format!(
                    "{DIR} was not found, so no workflow was checked: is this the repository \
                     root?"
                )));
            }
            Err(source) => return Err(Error::Read { path: at, source }),
        };
        return Ok(Some(format!("{} {problem}", walk::relative(root, &at))));
    }
    Ok(None)
}

/// What [`list`] found directly in the workflows' directory, each list
/// sorted by path.
#[derive(Debug, Default)]
struct Found {
    /// The regular files named like a workflow ([`is_workflow_name`]).
    workflows: Vec<PathBuf>,
    /// Every symbolic link, whatever its name: neither followed nor
    /// resolved, so what it leads to was not checked.
    links: Vec<PathBuf>,
    /// The entries named like a workflow that are neither a regular file, a
    /// directory nor a link (a FIFO, say), which are not opened.
    others: Vec<PathBuf>,
}

/// Lists the entries directly in `dir`. A directory there is not a
/// workflow (GitHub reads only the files directly in `.github/workflows/`),
/// and neither is a file with another extension.
///
/// # Errors
///
/// [`Error::Read`] if `dir` cannot be listed.
fn list(dir: &Path) -> Result<Found, Error> {
    let read_error = |source| Error::Read {
        path: dir.to_path_buf(),
        source,
    };
    let mut found = Found::default();
    for entry in fs::read_dir(dir).map_err(read_error)? {
        let entry = entry.map_err(read_error)?;
        // Does not follow a symbolic link: a link is neither a file nor a
        // directory here.
        let file_type = entry.file_type().map_err(read_error)?;
        let path = entry.path();
        if file_type.is_symlink() {
            found.links.push(path);
        } else if file_type.is_dir() || !is_workflow_name(&entry.file_name().to_string_lossy()) {
            continue;
        } else if file_type.is_file() {
            found.workflows.push(path);
        } else {
            found.others.push(path);
        }
    }
    found.workflows.sort();
    found.links.sort();
    found.others.sort();
    Ok(found)
}

/// Whether a file named `name` in [`DIR`] is a workflow: its extension,
/// after a `.` that is not its first character, is `yml` or `yaml` in any
/// case, as `headers` reads it.
fn is_workflow_name(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(stem, extension)| {
        !stem.is_empty()
            && (extension.eq_ignore_ascii_case("yml") || extension.eq_ignore_ascii_case("yaml"))
    })
}

/// The keys this check reads by name ([`split_key`]). A tag or an anchor
/// before one of them is a form it cannot read.
const READ_KEYS: [&str; 6] = [
    "on",
    "jobs",
    "permissions",
    "uses",
    "with",
    "persist-credentials",
];

/// Checks the workflow named `name`, shown as `shown`, whose text is
/// `text`, adding a line to `violations` for each problem.
fn check(shown: &str, name: &str, text: &str, violations: &mut Vec<String>) {
    let Some(workflow) = WORKFLOWS.iter().find(|workflow| workflow.file == name) else {
        violations.push(format!(
            "{shown}: no row in the table of what each workflow may use (`WORKFLOWS` in \
             xtask/src/workflows.rs, ADR-0022)"
        ));
        return;
    };
    // A byte-order mark, which GitHub's YAML reader skips, as `headers`
    // does: it would otherwise hide a top-level `permissions:` on line 1.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    // The lines are split at LF, and the CR of a CR LF dropped. A YAML
    // reader also breaks a line at a lone CR, and a YAML 1.1 reader (GitHub's
    // YamlDotNet) at NEL, LS and PS: what follows one would be read there and
    // not here, so the file is not checked further.
    let breaks = other_line_breaks(text);
    if !breaks.is_empty() {
        for line in breaks {
            violations.push(format!(
                "{shown}:{line}: a line break other than LF or CR LF, which a YAML reader reads \
                 as a new line and this check does not"
            ));
        }
        return;
    }
    let lines: Vec<&str> = text.lines().collect();
    let mut at = |line: usize, problem: String| {
        violations.push(format!("{shown}:{}: {problem}", line + 1));
    };

    let mut defaults = false;
    // Whether the lines are under `jobs:`, the column the job names start
    // at (that of the first), and the job they are in.
    let (mut in_jobs, mut job_column, mut job) = (false, None, None);
    // Whether the lines are in the top-level `on:` section.
    let mut in_on = false;
    // The `permissions:` block the lines are in: the column of its key, and
    // whether it is the top-level one.
    let mut block: Option<(usize, bool)> = None;
    // Whether a line other than a comment came before, for `---`.
    let mut started = false;
    // When the line before was a key with no value on its line, or a bare
    // `-`, the column a value on this line is indented past.
    let mut pending: Option<usize> = None;
    for (index, line) in lines.iter().enumerate() {
        let code = without_comment(line);
        if code.trim().is_empty() {
            continue;
        }
        let indent = indentation(code);
        let dashed = is_item(code);
        let (column, text) = item(code);
        let key = split_key(text);
        if block.is_some_and(|(column, _)| indent <= column) {
            block = None;
        }
        if indent == 0 {
            if let Some(problem) = document_marker(code, started) {
                at(index, problem.to_string());
            }
            let top = key.map(|key| key.name);
            in_jobs = top == Some("jobs");
            in_on = top == Some("on");
            job_column = None;
            job = None;
            if let Some(key) = key
                && (key.written.starts_with(['"', '\'']) || key.written.contains('\\'))
            {
                at(
                    index,
                    format!(
                        "`{}`: a top-level key that is quoted or holds a `\\`, which this check \
                         cannot read",
                        key.written
                    ),
                );
            }
        } else if in_jobs && indent == *job_column.get_or_insert(indent) {
            job = key.map(|key| key.name);
        }
        started = true;

        if text
            .strip_prefix('?')
            .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '\t']))
        {
            at(
                index,
                "an explicit key (`? `), which this check cannot read".to_string(),
            );
        }
        if let Some(key) = key {
            if key.props && READ_KEYS.contains(&key.name) {
                at(
                    index,
                    format!(
                        "a tag or anchor before `{}`, which this check cannot read",
                        key.name
                    ),
                );
            }
            if indent > 0 && key.written.starts_with('"') && key.written.contains('\\') {
                at(
                    index,
                    format!(
                        "`{}`: a double-quoted key with an escape, which this check cannot read",
                        key.written
                    ),
                );
            }
        }

        // What YAML reads on the raw line, comment and quotes included: a
        // value on the line after its key, or after a bare `-`, starts a
        // value too.
        let (_, raw) = item(line);
        let value_line = pending.is_some_and(|column| indent > column) && !dashed && key.is_none();
        let scanned = scan(raw, value_line || (dashed && key.is_none()));
        if let Some(problem) = scanned.problem {
            at(index, problem.message().to_string());
        }
        pending = match key {
            Some(key) if strip_properties(key.value).1.is_empty() => Some(column),
            None if dashed && text.trim().is_empty() => Some(indent),
            _ => None,
        };

        if in_on {
            let content = scanned.comment.map_or(raw, |at| &raw[..at]);
            if content.contains('\\') {
                at(
                    index,
                    "a `\\` in the `on:` section, where an escape could spell an event this \
                     check cannot read"
                        .to_string(),
                );
            }
            if scanned.problem.is_none()
                && comment_start(raw).is_some_and(|naive| Some(naive) != scanned.comment)
            {
                at(
                    index,
                    "a ` #` inside a quoted value in the `on:` section, where this check would \
                     read it as a comment"
                        .to_string(),
                );
            }
        }

        if code.contains("pull_request_target") {
            at(
                index,
                "`pull_request_target` runs with the base repository's token: use `pull_request`"
                    .to_string(),
            );
        }

        if let Some(key) = key.filter(|key| key.name == "permissions") {
            let top = indent == 0;
            if top {
                defaults = true;
                for problem in default_permissions(&lines, index, key.value) {
                    at(index, problem);
                }
            }
            match key.value {
                "" => block = Some((indent, top)),
                "{}" => {}
                value if !top => at(
                    index,
                    format!(
                        "`permissions: {value}`: name each permission on a line of its own, as \
                         this check reads them"
                    ),
                ),
                _ => {}
            }
            continue;
        }

        // A permission. The top-level ones are checked above.
        if let Some((_, top)) = block {
            if !top && let Some(problem) = job_permission(workflow, job, key, text) {
                at(index, problem);
            }
            continue;
        }

        let Some(key) = key.filter(|key| key.name == "uses") else {
            continue;
        };
        let reference = unquoted(key.value);
        if reference.is_empty() {
            at(
                index,
                "`uses:` names no action on its own line, as this check reads it".to_string(),
            );
            continue;
        }
        let Some((action, pin)) = reference.split_once('@') else {
            at(
                index,
                format!("`{reference}` is not `owner/repo@<commit SHA>`"),
            );
            continue;
        };
        if !workflow.actions.contains(&action) {
            at(
                index,
                format!("`{action}` is not an action this workflow may use (ADR-0022)"),
            );
        }
        if !is_commit_sha(pin) {
            at(
                index,
                format!("`{action}@{pin}` is not pinned by a full commit SHA"),
            );
        } else if !tag_comment(line) {
            at(
                index,
                format!("`{action}` has no comment naming its tag, such as `# v1.2.3`"),
            );
        }
        if action == CHECKOUT && !keeps_no_credentials(&lines, index) {
            at(
                index,
                "`actions/checkout` does not set `persist-credentials: false`".to_string(),
            );
        }
    }
    if !defaults {
        violations.push(format!(
            "{shown}: no `permissions:` at the top, so the token gets the repository's default \
             permissions"
        ));
    }
}

/// The lines of `text`, counted from 1, that hold a line break other than
/// LF or CR LF: a CR not followed by LF (at the end of the text too), NEL
/// (U+0085), or the line or paragraph separator (U+2028, U+2029).
fn other_line_breaks(text: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let mut line = 1;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let other = match c {
            '\n' => {
                line += 1;
                continue;
            }
            '\r' => chars.peek() != Some(&'\n'),
            '\u{85}' | '\u{2028}' | '\u{2029}' => true,
            _ => false,
        };
        if other && found.last() != Some(&line) {
            found.push(line);
        }
    }
    found
}

/// What is wrong with `code`, a line at column 0, if it is a YAML document
/// marker or directive: the text after a second document, or after `...`,
/// would be read by another reader as a workflow of its own, or not at all.
/// One `---` before anything else starts the one document, and is allowed.
fn document_marker(code: &str, started: bool) -> Option<&'static str> {
    let marker = |mark: &str| {
        code.strip_prefix(mark)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '\t']))
    };
    if code.starts_with('%') || marker("...") || (marker("---") && (started || code != "---")) {
        return Some(
            "a YAML document marker or directive, after which this check does not read the \
             document as GitHub does",
        );
    }
    None
}

/// What is wrong with the top-level `permissions:` at `lines[index]`, whose
/// value on its own line is `value`: it must be `{}`, or a block that grants
/// `contents: read` and nothing else.
fn default_permissions(lines: &[&str], index: usize, value: &str) -> Vec<String> {
    match value {
        "{}" => return Vec::new(),
        "" => {}
        other => {
            return vec![format!(
                "the default permissions are `{other}`: make them `{{}}` or `contents: read`"
            )];
        }
    }
    let mut problems = Vec::new();
    let mut granted = 0;
    for line in &lines[index + 1..] {
        let code = without_comment(line);
        if code.trim().is_empty() {
            continue;
        }
        if indentation(code) == 0 {
            break;
        }
        granted += 1;
        let reads_contents = split_key(item(code).1)
            .is_some_and(|key| key.name == "contents" && unquoted(key.value) == "read");
        if !reads_contents {
            problems.push(format!(
                "the default permissions grant `{}`: grant it in the job that needs it",
                code.trim()
            ));
        }
    }
    if granted == 0 {
        problems.push(
            "the default permissions are empty, not `{}` or `contents: read`: write one of those"
                .to_string(),
        );
    }
    problems
}

/// What is wrong with `text`, whose key is `key`, a line of the
/// `permissions:` block of the job `job` (`None` for a block in no job), if
/// anything: a `write` its row of `workflow` does not name, or a line this
/// check cannot read as a permission. Reading is `read` and `none`.
fn job_permission(
    workflow: &Workflow,
    job: Option<&str>,
    key: Option<Key<'_>>,
    text: &str,
) -> Option<String> {
    let Some(key) = key else {
        return Some(format!(
            "`{}` is not a permission written `name: level`, as this check reads them",
            text.trim()
        ));
    };
    let permission = key.name;
    match unquoted(key.value) {
        "read" | "none" => None,
        "write" => {
            let allowed = job.is_some_and(|job| {
                workflow
                    .writes
                    .iter()
                    .any(|(name, writes)| *name == job && writes.contains(&permission))
            });
            if allowed {
                return None;
            }
            Some(match job {
                Some(job) => format!("the job `{job}` may not grant `{permission}: write`"),
                None => format!("`{permission}: write` is granted outside a job"),
            })
        }
        other => Some(format!(
            "`{permission}: {other}`: a permission's level is `read`, `write` or `none`, as this \
             check reads them"
        )),
    }
}

/// A key on a line, and what follows it there, as [`split_key`] reads them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Key<'a> {
    /// Whether a tag (`!…`) or an anchor (`&…`) comes before it.
    props: bool,
    /// The key as written, after any tag or anchor, quotes included.
    written: &'a str,
    /// Its name: [`Key::written`] without one pair of matching quotes. A
    /// double-quoted key's escapes are not decoded.
    name: &'a str,
    /// What follows its `:` on the line, trimmed.
    value: &'a str,
}

/// The key of `text`, a line without its indentation, a list item's dash
/// and its comment, read as YAML ends a plain implicit key: at the first
/// `:` followed by a space, a tab or the end of the line. A key in quotes
/// ends at the `:` after its closing quote, which may follow it directly.
/// The key is trimmed, and its name loses one pair of matching quotes.
/// `None` if the line holds no key.
fn split_key(text: &str) -> Option<Key<'_>> {
    let (props, rest) = strip_properties(text);
    let bytes = rest.as_bytes();
    let after_quote = quoted_end(bytes).and_then(|end| {
        let gap = bytes[end + 1..]
            .iter()
            .take_while(|byte| matches!(byte, b' ' | b'\t'))
            .count();
        (bytes.get(end + 1 + gap) == Some(&b':')).then_some(end + 1 + gap)
    });
    let colon = after_quote.or_else(|| {
        (0..bytes.len()).find(|&i| {
            bytes[i] == b':'
                && bytes
                    .get(i + 1)
                    .is_none_or(|next| matches!(next, b' ' | b'\t'))
        })
    })?;
    let written = rest[..colon].trim_end_matches([' ', '\t']);
    Some(Key {
        props,
        written,
        name: unquoted(written),
        value: rest[colon + 1..].trim(),
    })
}

/// `text` without the tags (`!…`) and anchors (`&…`) it starts with, and
/// whether it had any.
fn strip_properties(text: &str) -> (bool, &str) {
    let mut rest = text;
    let mut props = false;
    while rest.starts_with(['!', '&']) {
        props = true;
        rest = rest
            .find([' ', '\t'])
            .map_or("", |space| rest[space..].trim_start());
    }
    (props, rest)
}

/// Where the quoted scalar that starts `text` ends: the index of its
/// closing quote. A `''` in single quotes, and a `\` and the byte after it in
/// double quotes, do not end it. `None` if `text` does not start with a
/// quote, or the scalar does not end in it.
fn quoted_end(text: &[u8]) -> Option<usize> {
    let quote = *text.first().filter(|byte| matches!(byte, b'"' | b'\''))?;
    let mut i = 1;
    while let Some(&byte) = text.get(i) {
        match byte {
            b'\'' if quote == b'\'' && text.get(i + 1) == Some(&b'\'') => i += 2,
            b'\\' if quote == b'"' => i += 2,
            byte if byte == quote => return Some(i),
            _ => i += 1,
        }
    }
    None
}

/// What [`scan`] found on a line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Scan {
    /// Where its comment starts, as YAML reads one: a `#` at the start of
    /// the text, or after a space or a tab, outside quotes.
    comment: Option<usize>,
    /// A form on the line that this check cannot read.
    problem: Option<Unreadable>,
}

/// A form of YAML that this check cannot read line by line, so it fails.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Unreadable {
    /// A quoted scalar that goes on past its line.
    Quote,
    /// A flow collection that goes on past its line.
    Flow,
    /// A mapping in flow style: `{…}` other than `{}`, or `key: value`
    /// inside `[…]`.
    FlowMapping,
    /// An alias (`*name`) or anchor (`&name`) as a value.
    Alias,
}

impl Unreadable {
    /// The violation's text.
    fn message(self) -> &'static str {
        match self {
            Self::Quote => {
                "a quoted value that does not end on its line, which this check cannot read"
            }
            Self::Flow => {
                "a flow collection (`[…]` or `{…}`) that does not end on its line, which this \
                 check cannot read"
            }
            Self::FlowMapping => {
                "a mapping written in flow style (`{…}` other than `{}`, or `key: value` in \
                 `[…]`), which this check does not read"
            }
            Self::Alias => {
                "an alias or anchor (`*name`, `&name`) as a value, which this check does not \
                 follow"
            }
        }
    }
}

/// Scans `text`, a line without its indentation and a list item's dash, as
/// YAML reads its nodes, for the forms this check cannot read
/// ([`Unreadable`]) and for where its comment starts.
///
/// `value` says whether the line's first node is a value (a list item's, or
/// that of the key on the line before) rather than a key. Only at the start
/// of a node is a quote, a bracket, an alias or an anchor read as one: in
/// the middle of a plain scalar, such as a line of a `run:` script
/// (`exec 2>&1`, `*[!0-9a-f]*`, `tr -d ' \n'`), it is text.
fn scan(text: &str, value: bool) -> Scan {
    let bytes = text.as_bytes();
    let blank = |i: usize| bytes.get(i).is_none_or(|byte| matches!(byte, b' ' | b'\t'));
    let mut depth = 0usize;
    // At the start of a node, and whether that node is a value.
    let (mut start, mut in_value) = (true, value);
    // After a space, a tab or the start, where a `#` starts a comment.
    let mut spaced = true;
    // Just after a quoted scalar or a flow collection, where a `:` may
    // follow directly.
    let mut closed = false;
    let mut i = 0;
    while let Some(&byte) = bytes.get(i) {
        if matches!(byte, b' ' | b'\t') {
            spaced = true;
            i += 1;
            continue;
        }
        if byte == b'#' && spaced {
            return Scan {
                comment: Some(i),
                problem: (depth > 0).then_some(Unreadable::Flow),
            };
        }
        spaced = false;
        if start {
            match byte {
                b'"' | b'\'' => {
                    let Some(end) = quoted_end(&bytes[i..]) else {
                        return Scan {
                            comment: None,
                            problem: Some(Unreadable::Quote),
                        };
                    };
                    i += end + 1;
                    (start, closed) = (false, true);
                    continue;
                }
                b'{' => {
                    // `{}` is the one flow mapping this check reads.
                    let gap = bytes[i + 1..]
                        .iter()
                        .take_while(|byte| matches!(byte, b' ' | b'\t'))
                        .count();
                    if bytes.get(i + 1 + gap) != Some(&b'}') {
                        return Scan {
                            comment: None,
                            problem: Some(Unreadable::FlowMapping),
                        };
                    }
                    i += gap + 2;
                    (start, closed) = (false, true);
                    continue;
                }
                b'[' => {
                    depth += 1;
                    (in_value, closed) = (true, false);
                    i += 1;
                    continue;
                }
                b'*' | b'&' if in_value || depth > 0 => {
                    return Scan {
                        comment: None,
                        problem: Some(Unreadable::Alias),
                    };
                }
                b'!' => {
                    // A tag: the node follows it.
                    while !blank(i) {
                        i += 1;
                    }
                    continue;
                }
                b'-' | b'?' if depth == 0 && blank(i + 1) => {
                    in_value = byte == b'-';
                    i += 1;
                    continue;
                }
                _ => start = false,
            }
        }
        match byte {
            b':' if blank(i + 1)
                || closed
                || (depth > 0 && matches!(bytes.get(i + 1), Some(b',' | b']' | b'}'))) =>
            {
                if depth > 0 {
                    return Scan {
                        comment: None,
                        problem: Some(Unreadable::FlowMapping),
                    };
                }
                (start, in_value) = (true, true);
            }
            b',' if depth > 0 => (start, in_value) = (true, true),
            b'[' if depth > 0 => depth += 1,
            b'{' if depth > 0 => {
                return Scan {
                    comment: None,
                    problem: Some(Unreadable::FlowMapping),
                };
            }
            b']' if depth > 0 => depth -= 1,
            _ => {}
        }
        closed = byte == b']';
        i += 1;
    }
    Scan {
        comment: None,
        problem: (depth > 0).then_some(Unreadable::Flow),
    }
}

/// `value` without one pair of matching quotes around it, if it has them.
fn unquoted(value: &str) -> &str {
    for quote in ['"', '\''] {
        if let Some(inner) = value
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
        {
            return inner;
        }
    }
    value
}

/// Whether `pin` is a full commit SHA: 40 lowercase hex digits.
fn is_commit_sha(pin: &str) -> bool {
    pin.len() == 40
        && pin
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// Whether the `uses:` line has a comment naming a tag, such as `# v7.0.1`.
fn tag_comment(line: &str) -> bool {
    comment_start(line).is_some_and(|at| {
        let mut chars = line[at + 1..].trim_start().chars();
        chars.next() == Some('v') && chars.next().is_some_and(|c| c.is_ascii_digit())
    })
}

/// The number of bytes of whitespace that `line` starts with.
fn indentation(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// A line's key, without the dash that starts a list item, and the column
/// the key starts at: 8 for `        uses: x`, `      - uses: x` and
/// `    -   uses: x`.
fn item(line: &str) -> (usize, &str) {
    let text = line.trim_start();
    let key = text
        .strip_prefix('-')
        .filter(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
        .map_or(text, str::trim_start);
    (line.len() - key.len(), key)
}

/// Whether a line starts an item of a list.
fn is_item(line: &str) -> bool {
    let text = line.trim_start();
    text.strip_prefix('-')
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
}

/// Whether the step whose `uses:` is at `lines[index]` gives
/// `persist-credentials: false` as an input in its `with:`.
///
/// The step starts at the nearest line at or above `uses:` that is indented
/// less than its keys, and is not blank or only a comment: that line holds
/// the step's dash, whatever follows it (`-`, `- # note`, `- uses:`). If it
/// is not a list item, the `uses:` is in no step, and nothing counts. The
/// step ends at the first line after that indented less than its keys,
/// which is the next item of its list or what follows the list. Its inputs
/// are the keys of its `with:` block, at the column of the first of them; a
/// line indented more is part of an input's value (a block scalar), not an
/// input.
///
/// It counts only when the step has one `with:`, holding one
/// `persist-credentials` input that reads `false` with no line below
/// continuing its value. YAML readers keep the last of two equal keys, and
/// a line indented more than a plain value goes on with it
/// (`false` then `  exit 1` reads `false exit 1`).
fn keeps_no_credentials(lines: &[&str], index: usize) -> bool {
    let (column, _) = item(without_comment(lines[index]));
    let Some(start) = (0..=index).rev().find(|&at| {
        let code = without_comment(lines[at]);
        !code.trim().is_empty() && indentation(code) < column
    }) else {
        return false;
    };
    if !is_item(without_comment(lines[start])) {
        return false;
    }
    // `Some` inside the step's `with:`, holding the column of its inputs
    // once the first is seen.
    let mut inputs: Option<Option<usize>> = None;
    let mut withs = 0;
    // The step's `persist-credentials` inputs, each `true` while it reads
    // `false` and nothing below has continued its value.
    let mut settings: Vec<bool> = Vec::new();
    // Whether the last input seen is a `persist-credentials`.
    let mut in_setting = false;
    for (at, line) in lines.iter().enumerate().skip(start) {
        let code = without_comment(line);
        if code.trim().is_empty() {
            continue;
        }
        let indent = indentation(code);
        if at > start && indent < column {
            break;
        }
        let (key_column, text) = item(code);
        let key = split_key(text);
        if key_column == column {
            // A key of the step.
            inputs = key
                .filter(|key| key.name == "with" && key.value.is_empty())
                .map(|_| None);
            withs += usize::from(inputs.is_some());
            in_setting = false;
            continue;
        }
        let Some(inputs) = &mut inputs else {
            continue;
        };
        let input_column = *inputs.get_or_insert(indent);
        if indent == input_column {
            in_setting = key
                .as_ref()
                .is_some_and(|key| key.name == "persist-credentials");
            if in_setting {
                settings.push(key.is_some_and(|key| key.value == "false"));
            }
        } else if indent > input_column
            && in_setting
            && let Some(last) = settings.last_mut()
        {
            // A line that goes on with the setting's value.
            *last = false;
        }
    }
    withs == 1 && settings == [true]
}

/// Where `line`'s comment starts as this check reads it: a `#` at its
/// start, or after a space or a tab, inside quotes or not.
fn comment_start(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    (0..bytes.len()).find(|&i| bytes[i] == b'#' && (i == 0 || matches!(bytes[i - 1], b' ' | b'\t')))
}

/// `line` without its comment ([`comment_start`]), and without the spaces
/// at its end. A `#` inside a quoted value after a space is taken as a
/// comment too: what is cut off holds an unclosed quote, which fails the
/// rule that reads it, and in the `on:` section such a `#` fails
/// ([`check`]).
fn without_comment(line: &str) -> &str {
    comment_start(line)
        .map_or(line, |at| &line[..at])
        .trim_end()
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;
    use crate::walk::testing::{TempTree, symlink};

    const SHA: &str = "3d3c42e5aac5ba805825da76410c181273ba90b1";

    /// GitHub's "CodeQL Advanced" starter workflow, as the user committed it
    /// on 2026-11-01 (`git show ed8edf4:.github/workflows/codeql.yml`, blob
    /// `c86343366d4fc8ddb4f22fcc2d2b117382f2568a`), byte for byte. Kept
    /// under a name no check reads as a workflow. It is GitHub's, under the
    /// MIT license in `tests/data/NOTICE.md`.
    const TEMPLATE: &str = include_str!("../tests/data/codeql-template.yml.txt");

    fn violations(name: &str, text: &str) -> Vec<String> {
        let mut violations = Vec::new();
        check(&format!("{DIR}/{name}"), name, text, &mut violations);
        violations
    }

    fn ci(steps: &str) -> String {
        format!(
            "name: CI\non:\n  push:\npermissions:\n  contents: read\njobs:\n  ci:\n    \
             runs-on: ubuntu-latest\n    steps:\n{steps}"
        )
    }

    fn checkout() -> String {
        format!(
            "      - name: Check out\n        uses: actions/checkout@{SHA} # v7.0.1\n        \
             with:\n          persist-credentials: false\n"
        )
    }

    /// Writes `text` as the workflow `name` of `tree`, after a passing
    /// `ci.yml`.
    fn with_workflow(tree: &TempTree, name: &str, text: &str) {
        tree.write(&format!("{DIR}/ci.yml"), ci(&checkout()));
        tree.write(&format!("{DIR}/{name}"), text);
    }

    #[test]
    fn the_repository_s_workflows_keep_the_rules() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let report = run(&root).expect("the workflows are readable");
        assert_eq!(report.violations, Vec::<String>::new());
        assert_eq!(
            report.notes,
            [format!("workflows checked: {}", WORKFLOWS.len())]
        );
        // Every row of the table is a workflow that exists, as a regular
        // file and not a link.
        for workflow in WORKFLOWS {
            let path = root.join(DIR).join(workflow.file);
            assert!(
                fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_file()),
                "{} has a row but no file",
                workflow.file
            );
        }
    }

    #[test]
    fn github_s_codeql_template_has_exactly_its_five_problems() {
        // Three actions pinned by tag, a checkout that keeps its
        // credentials, and no top-level `permissions`. Its job's
        // `packages: read` and `actions: read` are reads, which ADR-0022
        // does not fail.
        assert_eq!(
            violations("codeql.yml", TEMPLATE),
            [
                ".github/workflows/codeql.yml:60: `actions/checkout@v7` is not pinned by a full \
                 commit SHA",
                ".github/workflows/codeql.yml:60: `actions/checkout` does not set \
                 `persist-credentials: false`",
                ".github/workflows/codeql.yml:70: `github/codeql-action/init@v4` is not pinned \
                 by a full commit SHA",
                ".github/workflows/codeql.yml:99: `github/codeql-action/analyze@v4` is not \
                 pinned by a full commit SHA",
                ".github/workflows/codeql.yml: no `permissions:` at the top, so the token gets \
                 the repository's default permissions",
            ]
        );
    }

    #[test]
    fn a_pinned_allowed_action_with_its_tag_passes() {
        assert_eq!(violations("ci.yml", &ci(&checkout())), Vec::<String>::new());
        let cache = format!("      - uses: \"actions/cache@{SHA}\" # v6.1.0\n        with:\n");
        assert_eq!(
            violations("ci.yml", &ci(&(checkout() + &cache))),
            Vec::<String>::new()
        );
        // A byte-order mark before the first line is skipped.
        let marked = format!("\u{feff}{}", ci(&checkout()));
        assert_eq!(violations("ci.yml", &marked), Vec::<String>::new());
        // As a Windows checkout may have it.
        let crlf = ci(&checkout()).replace('\n', "\r\n");
        assert_eq!(violations("ci.yml", &crlf), Vec::<String>::new());
    }

    #[test]
    fn a_local_action_fails_like_any_action_its_row_does_not_name() {
        // Unlike Nadir's check: ADR-0022 names no local action, and this
        // check does not read the steps of one.
        let local = "      - uses: ./.github/actions/setup\n";
        assert_eq!(
            violations("ci.yml", &ci(&(checkout() + local))),
            [
                ".github/workflows/ci.yml:14: `./.github/actions/setup` is not \
                 `owner/repo@<commit SHA>`"
            ]
        );
        let pinned = format!("      - uses: ./.github/actions/setup@{SHA} # v1.0.0\n");
        assert_eq!(
            violations("ci.yml", &ci(&(checkout() + &pinned))),
            [
                ".github/workflows/ci.yml:14: `./.github/actions/setup` is not an action this \
                 workflow may use (ADR-0022)"
            ]
        );
    }

    #[test]
    fn a_tag_or_short_pin_fails() {
        let tag = "      - uses: actions/cache@v6\n";
        assert_eq!(
            violations("ci.yml", &ci(&(checkout() + tag))),
            [".github/workflows/ci.yml:14: `actions/cache@v6` is not pinned by a full commit SHA"]
        );
        let short = "      - uses: actions/cache@3d3c42e # v6.1.0\n";
        assert_eq!(violations("ci.yml", &ci(&(checkout() + short))).len(), 1);
        let upper = format!(
            "      - uses: actions/cache@{} # v6.1.0\n",
            SHA.to_uppercase()
        );
        assert_eq!(violations("ci.yml", &ci(&(checkout() + &upper))).len(), 1);
        let longer = format!("      - uses: actions/cache@{SHA}0 # v6.1.0\n");
        assert_eq!(violations("ci.yml", &ci(&(checkout() + &longer))).len(), 1);
        let bare = "      - uses: actions/cache\n";
        assert_eq!(
            violations("ci.yml", &ci(&(checkout() + bare))),
            [".github/workflows/ci.yml:14: `actions/cache` is not `owner/repo@<commit SHA>`"]
        );
        // A value on the next line is not seen as the action, so it fails.
        let below = "      - uses:\n          actions/cache@v6\n";
        assert_eq!(
            violations("ci.yml", &ci(&(checkout() + below))),
            [
                ".github/workflows/ci.yml:14: `uses:` names no action on its own line, as this \
                 check reads it"
            ]
        );
    }

    #[test]
    fn a_pin_without_its_tag_fails() {
        let untagged = format!("      - uses: actions/cache@{SHA}\n");
        let commented = format!("      - uses: actions/cache@{SHA} # the cache\n");
        for steps in [untagged, commented] {
            assert_eq!(
                violations("ci.yml", &ci(&(checkout() + &steps))),
                [
                    ".github/workflows/ci.yml:14: `actions/cache` has no comment naming its tag, \
                     such as `# v1.2.3`"
                ]
            );
        }
    }

    #[test]
    fn an_action_the_workflow_may_not_use_fails() {
        let other = format!("      - uses: some/action@{SHA} # v1.0.0\n");
        assert_eq!(
            violations("ci.yml", &ci(&(checkout() + &other))),
            [
                ".github/workflows/ci.yml:14: `some/action` is not an action this workflow may \
                 use (ADR-0022)"
            ]
        );
        // CodeQL's actions are its workflow's alone, and the cache is the
        // gate's alone.
        let init = format!("      - uses: github/codeql-action/init@{SHA} # v4.39.1\n");
        assert_eq!(violations("ci.yml", &ci(&(checkout() + &init))).len(), 1);
        let scan = ci(&(checkout() + &init))
            .replace("permissions:\n  contents: read\n", "permissions: {}\n");
        assert_eq!(violations("codeql.yml", &scan), Vec::<String>::new());
        let cache = format!("      - uses: actions/cache@{SHA} # v6.1.0\n");
        assert_eq!(
            violations("codeql.yml", &ci(&(checkout() + &cache))).len(),
            1
        );
        // A reusable workflow from elsewhere has no row either.
        let reusable =
            format!("  other:\n    uses: some/repo/.github/workflows/x.yml@{SHA} # v1\n");
        assert_eq!(
            violations("ci.yml", &(ci(&checkout()) + &reusable)),
            [
                ".github/workflows/ci.yml:15: `some/repo/.github/workflows/x.yml` is not an \
                 action this workflow may use (ADR-0022)"
            ]
        );
    }

    #[test]
    fn the_pages_workflow_may_use_github_s_two_pages_actions_and_no_other_may() {
        let upload = format!(
            "      - uses: actions/upload-pages-artifact@{SHA} # v5.0.0\n        with:\n          \
             path: target/site\n"
        );
        let publish = format!("      - uses: actions/deploy-pages@{SHA} # v5.0.1\n");
        let cache = format!("      - uses: actions/cache@{SHA} # v6.1.0\n");
        let steps = checkout() + &cache + &upload + &publish;
        assert_eq!(violations("pages.yml", &ci(&steps)), Vec::<String>::new());
        assert_eq!(
            violations("ci.yml", &ci(&steps)),
            [
                ".github/workflows/ci.yml:15: `actions/upload-pages-artifact` is not an action \
                 this workflow may use (ADR-0022)",
                ".github/workflows/ci.yml:18: `actions/deploy-pages` is not an action this \
                 workflow may use (ADR-0022)",
            ]
        );
        assert_eq!(violations("codeql.yml", &ci(&steps)).len(), 3);
        // Pinned like any other: by a full SHA with its tag.
        let tagged = checkout() + "      - uses: actions/deploy-pages@v5\n";
        assert_eq!(
            violations("pages.yml", &ci(&tagged)),
            [
                ".github/workflows/pages.yml:14: `actions/deploy-pages@v5` is not pinned by a full \
              commit SHA"
            ]
        );
        // Its checkout keeps no credentials, as every workflow's.
        let kept = format!("      - uses: actions/checkout@{SHA} # v7.0.1\n");
        assert_eq!(violations("pages.yml", &ci(&kept)).len(), 1);
    }

    #[test]
    fn a_workflow_with_no_row_fails() {
        for name in ["deploy.yml", "CI.yml", "ci.yaml"] {
            assert_eq!(
                violations(name, &ci(&checkout())),
                [format!(
                    ".github/workflows/{name}: no row in the table of what each workflow may use \
                     (`WORKFLOWS` in xtask/src/workflows.rs, ADR-0022)"
                )]
            );
        }
    }

    #[test]
    fn a_checkout_that_keeps_its_credentials_fails() {
        let kept =
            format!("      - name: Check out\n        uses: actions/checkout@{SHA} # v7.0.1\n");
        assert_eq!(
            violations("ci.yml", &ci(&kept)),
            [
                ".github/workflows/ci.yml:11: `actions/checkout` does not set \
                 `persist-credentials: false`"
            ]
        );
        // The setting in the next step does not count for this one.
        let next = format!(
            "{kept}      - name: Other\n        with:\n          persist-credentials: false\n"
        );
        assert_eq!(violations("ci.yml", &ci(&next)).len(), 1);
        // A `true` does not count, and nor does a comment.
        let true_ = format!("{kept}        with:\n          persist-credentials: true\n");
        assert_eq!(violations("ci.yml", &ci(&true_)).len(), 1);
        let comment = format!("{kept}        # persist-credentials: false\n");
        assert_eq!(violations("ci.yml", &ci(&comment)).len(), 1);
        // Only an input counts: not a variable, nor a line of an input's
        // value.
        let env = format!("{kept}        env:\n          persist-credentials: false\n");
        assert_eq!(violations("ci.yml", &ci(&env)).len(), 1);
        let scalar = format!(
            "{kept}        with:\n          path: |\n            persist-credentials: false\n"
        );
        assert_eq!(violations("ci.yml", &ci(&scalar)).len(), 1);
        // `with:` above `uses:` counts, and so does a later input.
        let above = format!(
            "      - with:\n          persist-credentials: false\n        uses: \
             actions/checkout@{SHA} # v7.0.1\n"
        );
        assert_eq!(violations("ci.yml", &ci(&above)), Vec::<String>::new());
        let later = format!(
            "{kept}        with:\n          fetch-depth: 1\n          persist-credentials: false\n"
        );
        assert_eq!(violations("ci.yml", &ci(&later)), Vec::<String>::new());
        // A second setting or a second `with:` fails whatever it says, as
        // YAML keeps the last; so does a line that goes on with the value.
        for twice in [
            "        with:\n          persist-credentials: false\n          persist-credentials: true\n",
            "        with:\n          persist-credentials: true\n          persist-credentials: false\n",
            "        with:\n          persist-credentials: false\n          persist-credentials: false\n",
            "        with:\n          persist-credentials: false\n        with:\n          fetch-depth: 1\n",
            "        with:\n          persist-credentials: false\n            exit 1\n",
        ] {
            let text = ci(&format!("{kept}{twice}"));
            assert_eq!(violations("ci.yml", &text).len(), 1, "{twice}");
        }
    }

    #[test]
    fn the_default_permissions_are_none_or_reading_the_contents() {
        let none = ci(&checkout()).replace("permissions:\n  contents: read\n", "permissions: {}\n");
        assert_eq!(violations("ci.yml", &none), Vec::<String>::new());
        let missing = ci(&checkout()).replace("permissions:\n  contents: read\n", "");
        assert_eq!(
            violations("ci.yml", &missing),
            [
                ".github/workflows/ci.yml: no `permissions:` at the top, so the token gets the \
                 repository's default permissions"
            ]
        );
        for value in ["read-all", "write-all", "{ contents: read }"] {
            let all = ci(&checkout()).replace(
                "permissions:\n  contents: read\n",
                &format!("permissions: {value}\n"),
            );
            let mut expected = vec![format!(
                ".github/workflows/ci.yml:4: the default permissions are `{value}`: make them \
                 `{{}}` or `contents: read`"
            )];
            // A flow mapping fails as one too.
            if value.starts_with('{') {
                expected.insert(
                    0,
                    format!(
                        ".github/workflows/ci.yml:4: {}",
                        Unreadable::FlowMapping.message()
                    ),
                );
            }
            assert_eq!(violations("ci.yml", &all), expected);
        }
        let more = ci(&checkout()).replace(
            "permissions:\n  contents: read\n",
            "permissions:\n  contents: read\n  actions: read\n",
        );
        assert_eq!(
            violations("ci.yml", &more),
            [
                ".github/workflows/ci.yml:4: the default permissions grant `actions: read`: grant \
                 it in the job that needs it"
            ]
        );
        let write = ci(&checkout()).replace("contents: read\n", "contents: write\n");
        assert_eq!(violations("ci.yml", &write).len(), 1);
        // An empty block grants neither of the two.
        let empty = ci(&checkout()).replace("permissions:\n  contents: read\n", "permissions:\n");
        assert_eq!(
            violations("ci.yml", &empty),
            [
                ".github/workflows/ci.yml:4: the default permissions are empty, not `{}` or \
                 `contents: read`: write one of those"
            ]
        );
    }

    #[test]
    fn a_job_writes_only_what_the_table_names_for_it() {
        let job = |name: &str, permission: &str| {
            format!(
                "  {name}:\n    runs-on: ubuntu-latest\n    permissions:\n      contents: \
                 read\n      {permission}: write\n    steps:\n      - run: true\n"
            )
        };
        let scan = ci(&checkout()) + &job("analyze", "security-events");
        assert_eq!(violations("codeql.yml", &scan), Vec::<String>::new());
        assert_eq!(
            violations("ci.yml", &scan),
            [
                ".github/workflows/ci.yml:18: the job `analyze` may not grant \
                 `security-events: write`"
            ]
        );
        // The job, not the workflow: CodeQL's other jobs may not write, nor
        // may its analysis job write anything else.
        for (name, permission) in [
            ("build", "security-events"),
            ("analyze", "contents"),
            ("analyze", "id-token"),
            ("analyze", "actions"),
        ] {
            assert_eq!(
                violations("codeql.yml", &(ci(&checkout()) + &job(name, permission))),
                [format!(
                    ".github/workflows/codeql.yml:18: the job `{name}` may not grant \
                     `{permission}: write`"
                )]
            );
        }
        // Any permission, even one GitHub adds later.
        let new = ci(&checkout()) + &job("ci", "code-quality");
        assert_eq!(violations("ci.yml", &new).len(), 1);
        // A quoted level is the same level.
        let quoted = (ci(&checkout()) + &job("ci", "contents")).replace(": write", ": \"write\"");
        assert_eq!(violations("ci.yml", &quoted).len(), 1);
        let single =
            (ci(&checkout()) + &job("analyze", "security-events")).replace(": write", ": 'write'");
        assert_eq!(violations("codeql.yml", &single), Vec::<String>::new());
        // A level it cannot read fails.
        let odd = (ci(&checkout()) + &job("ci", "contents")).replace(": write", ": Write");
        assert_eq!(
            violations("ci.yml", &odd),
            [
                ".github/workflows/ci.yml:18: `contents: Write`: a permission's level is `read`, \
                 `write` or `none`, as this check reads them"
            ]
        );
        // Written all at once, a job's permissions cannot be read.
        for value in ["write-all", "read-all", "{ security-events: write }"] {
            let inline = ci(&checkout())
                + &format!("  analyze:\n    permissions: {value}\n    steps:\n      - run: true\n");
            let expected = if value.starts_with('{') { 2 } else { 1 };
            assert_eq!(violations("codeql.yml", &inline).len(), expected, "{value}");
        }
        let none = ci(&checkout()) + "  ci:\n    permissions: {}\n";
        assert_eq!(violations("ci.yml", &none), Vec::<String>::new());
        // A key that is not in a `permissions:` block may say `write`.
        let input = ci(&checkout()) + "          mode: write\n";
        assert_eq!(violations("ci.yml", &input), Vec::<String>::new());
        // A block ends where the job's next key starts.
        let after = ci(&checkout())
            + "  ci:\n    permissions:\n      contents: read\n    env:\n      mode: write\n";
        assert_eq!(violations("ci.yml", &after), Vec::<String>::new());
        // Jobs are found at whatever indentation the file gives them.
        let wide = "permissions: {}\njobs:\n    analyze:\n        permissions:\n            \
                    contents: write\n";
        assert_eq!(
            violations("codeql.yml", wide),
            [".github/workflows/codeql.yml:5: the job `analyze` may not grant `contents: write`"]
        );
        // The demo page's deploy job may write `pages` and `id-token`, and
        // nothing else; its other jobs may write nothing; and no other
        // workflow's job named `deploy` may write either (ADR-0030).
        let deploy = |permission: &str| {
            format!(
                "  deploy:\n    runs-on: ubuntu-latest\n    permissions:\n      \
                 {permission}: write\n    steps:\n      - run: true\n"
            )
        };
        let publish = format!(
            "{}  deploy:\n    permissions:\n      pages: write\n      id-token: write\n",
            ci(&checkout())
        );
        assert_eq!(violations("pages.yml", &publish), Vec::<String>::new());
        assert_eq!(violations("ci.yml", &publish).len(), 2);
        assert_eq!(violations("codeql.yml", &publish).len(), 2);
        for (name, permission) in [
            ("deploy", "contents"),
            ("deploy", "actions"),
            ("deploy", "security-events"),
            ("build", "pages"),
            ("gate", "id-token"),
        ] {
            let text =
                ci(&checkout()) + &deploy(permission).replace("deploy:", &format!("{name}:"));
            assert_eq!(
                violations("pages.yml", &text),
                [format!(
                    ".github/workflows/pages.yml:17: the job `{name}` may not grant \
                     `{permission}: write`"
                )]
            );
        }
        // A read is not a write: the gate may read `actions`.
        let gate = ci(&checkout())
            + "  gate:\n    permissions:\n      contents: read\n      actions: read\n";
        assert_eq!(violations("pages.yml", &gate), Vec::<String>::new());
        // A block in no job may not write at all.
        let stray = "permissions: {}\non:\n  push:\n    permissions:\n      contents: write\n";
        assert_eq!(
            violations("ci.yml", stray),
            [".github/workflows/ci.yml:5: `contents: write` is granted outside a job"]
        );
    }

    #[test]
    fn list_items_are_read_however_they_are_spaced() {
        let wide = "      -   uses: some/action@v1\n";
        assert_eq!(violations("ci.yml", &ci(&(checkout() + wide))).len(), 2);
        assert_eq!(item("      -   uses: x"), (10, "uses: x"));
        assert_eq!(item("      - uses: x"), (8, "uses: x"));
        assert_eq!(item("        uses: x"), (8, "uses: x"));
        assert_eq!(item("        -x: y"), (8, "-x: y"));
        // A checkout written so keeps its setting.
        let checkout = format!(
            "      -   name: Check out\n          uses: actions/checkout@{SHA} # v7.0.1\n          \
             with:\n            persist-credentials: false\n      -   run: true\n"
        );
        assert_eq!(violations("ci.yml", &ci(&checkout)), Vec::<String>::new());
        let kept =
            format!("      -   uses: actions/checkout@{SHA} # v7.0.1\n      -   run: true\n");
        assert_eq!(violations("ci.yml", &ci(&kept)).len(), 1);
    }

    #[test]
    fn pull_request_target_fails() {
        let text = ci(&checkout()).replace("on:\n  push:\n", "on:\n  pull_request_target:\n");
        assert_eq!(
            violations("ci.yml", &text),
            [
                ".github/workflows/ci.yml:3: `pull_request_target` runs with the base repository's \
                 token: use `pull_request`"
            ]
        );
        // In a comment, it is only words.
        let comment = ci(&checkout()).replace("on:\n", "on:\n  # never pull_request_target\n");
        assert_eq!(violations("ci.yml", &comment), Vec::<String>::new());
    }

    #[test]
    fn comments_are_left_out() {
        assert_eq!(without_comment("  # uses: x@y"), "");
        assert_eq!(without_comment("  key: value # note"), "  key: value");
        assert_eq!(without_comment("  key: a#b"), "  key: a#b");
        assert!(tag_comment("uses: a/b@c # v1.2.3"));
        assert!(tag_comment("uses: a/b@c   #  v12"));
        assert!(!tag_comment("uses: a/b@c # version 1"));
        assert!(!tag_comment("uses: a/b@c"));
        assert_eq!(unquoted("\"write\""), "write");
        assert_eq!(unquoted("'write'"), "write");
        assert_eq!(unquoted("\"write'"), "\"write'");
        assert_eq!(unquoted("\""), "\"");
    }

    #[test]
    fn a_comment_may_follow_a_tab() {
        // As YAML reads one: a `#` after a space or a tab.
        assert_eq!(without_comment("  key: value\t# note"), "  key: value");
        assert!(tag_comment("uses: a/b@c\t# v1.2.3"));
        let tab = ci(&checkout()).replace("contents: read\n", "contents: read\t# note\n");
        assert_eq!(violations("ci.yml", &tab), Vec::<String>::new());
    }

    /// The violation for a line break other than LF or CR LF on `line`.
    fn line_break(line: usize) -> String {
        format!(
            ".github/workflows/ci.yml:{line}: a line break other than LF or CR LF, which a YAML \
             reader reads as a new line and this check does not"
        )
    }

    #[test]
    fn a_line_break_other_than_lf_or_cr_lf_fails_and_ends_the_check() {
        // A lone CR hides a job's permissions inside the line before.
        let job = ci(&checkout()).replace(
            "    runs-on: ubuntu-latest\n",
            "    runs-on: ubuntu-latest\r    permissions: write-all\n",
        );
        assert_eq!(violations("ci.yml", &job), [line_break(8)]);
        // A lone CR, NEL, LS or PS hides a step inside a comment.
        for other in ["\r", "\u{85}", "\u{2028}", "\u{2029}"] {
            let step =
                ci(&(checkout() + &format!("      # note{other}      - uses: evil/action@main\n")));
            assert_eq!(violations("ci.yml", &step), [line_break(14)], "{other:?}");
        }
        // At the end of the file too, and nothing else is reported: the file
        // is checked no further, though it has no top-level permissions.
        let end = ci(&checkout()).replace("permissions:\n  contents: read\n", "") + "\r";
        assert_eq!(violations("ci.yml", &end), [line_break(12)]);
        // One violation a line, however many there are on it.
        let twice = ci(&checkout()).replace("  push:\n", "  push:\r\r\n");
        assert_eq!(violations("ci.yml", &twice), [line_break(3)]);
        // CR LF is a line break the check reads.
        let crlf = ci(&checkout()).replace('\n', "\r\n");
        assert_eq!(violations("ci.yml", &crlf), Vec::<String>::new());
    }

    #[test]
    fn keys_are_read_by_name_however_they_are_spaced_or_quoted() {
        let at = |line: usize, text: &str| format!(".github/workflows/ci.yml:{line}: {text}");
        // A space or a tab before the colon is the same key.
        let inline = ci(&checkout()) + "  other:\n    permissions : write-all\n";
        assert_eq!(
            violations("ci.yml", &inline),
            [at(
                15,
                "`permissions: write-all`: name each permission on a line of its own, as this \
                 check reads them"
            )]
        );
        let block = ci(&checkout()) + "  other:\n    permissions :\n      contents: write\n";
        assert_eq!(
            violations("ci.yml", &block),
            [at(16, "the job `other` may not grant `contents: write`")]
        );
        for spaced in [
            "uses :",
            "uses\t:",
            "\"uses\":",
            "'uses' :",
            "\"uses\":evil",
        ] {
            let text = ci(&(checkout() + &format!("      - {spaced} evil/action@main\n")));
            let found = violations("ci.yml", &text);
            assert!(
                found.len() == 2
                    && found[0].contains("is not an action this workflow may use")
                    && found[1].contains("is not pinned by a full commit SHA"),
                "{spaced}: {found:?}"
            );
        }
        // So `with`, `persist-credentials`, `jobs` and the top-level
        // `permissions`, written so, are read, and pass.
        let fine = ci(&checkout())
            .replace("        with:\n", "        with :\n")
            .replace("persist-credentials: false", "persist-credentials : false")
            .replace("jobs:\n", "jobs :\n")
            .replace("permissions:\n", "permissions :\n");
        assert_eq!(violations("ci.yml", &fine), Vec::<String>::new());
        // A colon in a value, or in a script, is not a key's.
        let values = ci(&(checkout() + "      - name: \"a: b\"\n        run: echo a:b 'c: d'\n"));
        assert_eq!(violations("ci.yml", &values), Vec::<String>::new());
    }

    #[test]
    fn a_key_this_check_cannot_read_fails() {
        let at = |line: usize, text: &str| format!(".github/workflows/ci.yml:{line}: {text}");
        // An escape in a double-quoted key, which could spell `uses`.
        let escaped = ci(&(checkout() + "      - \"u\\x73es\": evil/action@main\n"));
        assert_eq!(
            violations("ci.yml", &escaped),
            [at(
                14,
                "`\"u\\x73es\"`: a double-quoted key with an escape, which this check cannot read"
            )]
        );
        // A tag or an anchor before one of the keys it reads. The key is
        // still read by its name.
        for props in ["!!str", "&a", "!!str &a", "!<tag:yaml.org,2002:str>"] {
            let text = ci(&(checkout() + &format!("      - {props} uses: evil/action@main\n")));
            assert_eq!(
                violations("ci.yml", &text),
                [
                    at(
                        14,
                        "a tag or anchor before `uses`, which this check cannot read"
                    ),
                    at(
                        14,
                        "`evil/action` is not an action this workflow may use (ADR-0022)"
                    ),
                    at(14, "`evil/action@main` is not pinned by a full commit SHA"),
                ],
                "{props}"
            );
        }
        let perms = ci(&checkout()) + "  other:\n    !!str permissions:\n      contents: write\n";
        assert!(violations("ci.yml", &perms).contains(&at(
            15,
            "a tag or anchor before `permissions`, which this check cannot read"
        )),);
        // But not before another key: a tag says what YAML already reads.
        let other = ci(&(checkout() + "      - !!str name: x\n        run: true\n"));
        assert_eq!(violations("ci.yml", &other), Vec::<String>::new());
        // An explicit key, which may hold any key on the lines after it.
        let explicit = ci(&(checkout() + "      - ? uses\n        : evil/action@main\n"));
        assert_eq!(
            violations("ci.yml", &explicit),
            [at(
                14,
                "an explicit key (`? `), which this check cannot read"
            )]
        );
    }

    #[test]
    fn a_step_that_starts_with_a_bare_dash_is_a_step_of_its_own() {
        // After a checkout that sets `persist-credentials: false`, a second
        // one whose dash is alone on its line, or before a comment.
        for dash in ["      -\n", "      - # second checkout\n", "      -   \n"] {
            let second = format!(
                "{dash}        uses: actions/checkout@{SHA} # v7.0.1\n        with:\n          \
                 path: other\n"
            );
            assert_eq!(
                violations("ci.yml", &ci(&(checkout() + &second))),
                [
                    ".github/workflows/ci.yml:15: `actions/checkout` does not set \
                     `persist-credentials: false`"
                ],
                "{dash:?}"
            );
        }
        // One that sets it itself passes, with a blank line or a comment
        // between its dash and its `uses:`.
        for gap in ["", "\n", "        # the step\n"] {
            let own = format!(
                "      -\n{gap}        uses: actions/checkout@{SHA} # v7.0.1\n        \
                 with:\n          persist-credentials: false\n"
            );
            assert_eq!(
                violations("ci.yml", &ci(&(checkout() + &own))),
                Vec::<String>::new(),
                "{gap:?}"
            );
        }
        // A `uses:` in no step is in no step's `with:`.
        let job = ci(&checkout())
            + &format!(
                "  other:\n    uses: actions/checkout@{SHA} # v7.0.1\n    with:\n      \
                 persist-credentials: false\n"
            );
        assert_eq!(
            violations("ci.yml", &job),
            [
                ".github/workflows/ci.yml:15: `actions/checkout` does not set \
                 `persist-credentials: false`"
            ]
        );
    }

    #[test]
    fn pull_request_target_fails_however_on_spells_it() {
        let on = |events: &str| ci(&checkout()).replace("on:\n  push:\n", events);
        let at = |line: usize, text: &str| format!(".github/workflows/ci.yml:{line}: {text}");
        let backslash = "a `\\` in the `on:` section, where an escape could spell an event this \
                         check cannot read";
        // An escape in a quoted event, a list item's or a key's.
        assert_eq!(
            violations("ci.yml", &on("on:\n  - \"pull_request\\x5ftarget\"\n")),
            [at(3, backslash)]
        );
        assert_eq!(
            violations("ci.yml", &on("on:\n  \"pull_request\\u005ftarget\":\n")),
            [
                at(
                    3,
                    "`\"pull_request\\u005ftarget\"`: a double-quoted key with an escape, which \
                     this check cannot read"
                ),
                at(3, backslash)
            ]
        );
        assert_eq!(
            violations("ci.yml", &on("on: [\"pull_request\\x5ftarget\"]\n")),
            [at(2, backslash)]
        );
        // One split over two lines by an escaped line break.
        assert_eq!(
            violations("ci.yml", &on("on:\n  - \"pull_request\\\n    _target\"\n")),
            [at(3, Unreadable::Quote.message()), at(3, backslash)]
        );
        // A top-level key that is quoted or holds a `\`, which could be `on`.
        for key in ["\"on\"", "'on'", "o\\x6e", "\"o\\x6e\""] {
            let text = on(&format!("{key}: [push, pull_request]\n"));
            assert_eq!(
                violations("ci.yml", &text),
                [at(
                    2,
                    &format!(
                        "`{key}`: a top-level key that is quoted or holds a `\\`, which this \
                         check cannot read"
                    )
                )],
                "{key}"
            );
        }
        // A ` #` inside quotes, which the check reads as a comment's start.
        assert_eq!(
            violations("ci.yml", &on("on: [\"x #\", pull_request_target]\n")),
            [at(
                2,
                "a ` #` inside a quoted value in the `on:` section, where this check would read \
                 it as a comment"
            )]
        );
        // A comment that names it or holds a `\` is only words, there or
        // elsewhere, and the real triggers pass.
        let comments = on("on:\n  # never pull_request_target\n  push: # a \\ b\n")
            .replace("name: CI\n", "name: CI # nor pull_request_target \\x5f\n");
        assert_eq!(violations("ci.yml", &comments), Vec::<String>::new());
        let real = on(
            "on:\n  push:\n    branches: [main]\n  pull_request:\n  schedule:\n    - cron: \
             \"35 9 * * 6\"\n  workflow_dispatch:\n",
        );
        assert_eq!(violations("ci.yml", &real), Vec::<String>::new());
    }

    #[test]
    fn a_quoted_value_or_flow_collection_that_goes_on_past_its_line_fails() {
        let at = |name: &str, line: usize, problem: Unreadable| {
            format!(".github/workflows/{name}:{line}: {}", problem.message())
        };
        // Its next lines would be read as keys: here, the top-level
        // permissions, single- or double-quoted.
        for quote in ['\'', '"'] {
            let fake = format!(
                "name: {quote}CI\npermissions: {{}}\n{quote}\non:\n  push:\njobs:\n  ci:\n    \
                 steps:\n{}",
                checkout()
            );
            assert_eq!(
                violations("ci.yml", &fake),
                [
                    at("ci.yml", 1, Unreadable::Quote),
                    at("ci.yml", 3, Unreadable::Quote)
                ],
                "{quote}"
            );
        }
        // An input whose next line would be read as `persist-credentials`.
        let input = format!(
            "      - uses: actions/checkout@{SHA} # v7.0.1\n        with:\n          path: \
             'src\n          persist-credentials: false\n          '\n"
        );
        assert_eq!(
            violations("ci.yml", &ci(&input)),
            [
                at("ci.yml", 12, Unreadable::Quote),
                at("ci.yml", 14, Unreadable::Quote)
            ]
        );
        // A job's name whose next line would be read as the job `analyze`.
        let job = "permissions: {}\njobs:\n  evil:\n    name: 'x\n  analyze: y'\n    \
                   permissions:\n      security-events: write\n";
        assert_eq!(
            violations("codeql.yml", job),
            [at("codeql.yml", 4, Unreadable::Quote)]
        );
        // What ends on its line passes: `''` and `\"` are escapes in their
        // quotes, and a ` #` inside quotes is text.
        let closed = ci(&checkout()).replace("name: CI\n", "name: 'it''s \"CI\" # one'\n")
            + "    env:\n      A: \"a \\\"b\\\" # c\"\n      B: 'x'\n";
        assert_eq!(violations("ci.yml", &closed), Vec::<String>::new());
        // A flow collection over two lines fails; on one line it passes.
        let flow =
            ci(&checkout()).replace("  push:\n", "  push:\n    branches: [main,\n      next]\n");
        assert_eq!(
            violations("ci.yml", &flow),
            [at("ci.yml", 4, Unreadable::Flow)]
        );
        let one = ci(&checkout()).replace("  push:\n", "  push:\n    branches: [main, \"x]\"]\n");
        assert_eq!(violations("ci.yml", &one), Vec::<String>::new());
        // A mapping in flow style is not read, so it fails, but `{}` passes.
        for value in [
            "{ uses: evil/action@main }",
            "[uses: evil/action@main]",
            "[\"uses\":evil/action@main]",
            "[x, {uses: evil/action@main}]",
        ] {
            let step = ci(&(checkout() + &format!("      - {value}\n")));
            assert_eq!(
                violations("ci.yml", &step),
                [at("ci.yml", 14, Unreadable::FlowMapping)],
                "{value}"
            );
        }
        let empty =
            ci(&checkout()).replace("permissions:\n  contents: read\n", "permissions: {}\n");
        assert_eq!(violations("ci.yml", &empty), Vec::<String>::new());
    }

    /// A `run:` script with the shapes of `ci.yml`'s: redirections, globs,
    /// quotes in the middle of a line, and workflow commands.
    const SCRIPT: &str = r#"      - name: Pause
        run: |
          unset token
          token="$(od -An -N16 -tx1 /dev/urandom | tr -d ' \n')"
          if [ "${#token}" -ne 32 ] || [[ "$token" == *[!0-9a-f]* ]]; then
            echo "no token" >&2
            exit 1
          fi
          exec 2>&1
          echo "::stop-commands::${token}"
          [ -f x ] && echo 'a: b' "c # d"
          cargo xtask ci
          echo "::${token}::"
"#;

    #[test]
    fn an_alias_or_anchor_as_a_value_fails_but_a_script_s_text_does_not() {
        let at = |line: usize| {
            format!(
                ".github/workflows/codeql.yml:{line}: {}",
                Unreadable::Alias.message()
            )
        };
        // A job copied by an alias would write where its row says it may
        // not.
        let copied = "permissions: {}\njobs:\n  analyze: &job\n    permissions:\n      \
                      security-events: write\n  other: *job\n";
        assert_eq!(violations("codeql.yml", copied), [at(3), at(6)]);
        // On the line after its key, as a list item, or merged.
        let next = "permissions: {}\njobs:\n  other:\n    *job\n";
        assert_eq!(violations("codeql.yml", next), [at(4)]);
        let item =
            "permissions: {}\njobs:\n  x:\n    steps:\n      - *step\n      -\n        *step\n";
        assert_eq!(violations("codeql.yml", item), [at(5), at(7)]);
        let merged = "permissions: {}\njobs:\n  other:\n    <<: *job\n    steps: [*s]\n";
        assert_eq!(violations("codeql.yml", merged), [at(4), at(5)]);
        // A script's `>&2`, `2>&1`, `*[!0-9a-f]*` and `' \n'` are text.
        assert_eq!(
            violations("ci.yml", &ci(&(checkout() + SCRIPT))),
            Vec::<String>::new()
        );
    }

    #[test]
    fn only_one_yaml_document_is_read() {
        let marker = |line: usize| {
            format!(
                ".github/workflows/ci.yml:{line}: a YAML document marker or directive, after \
                 which this check does not read the document as GitHub does"
            )
        };
        // One `---` before the document starts it.
        let leading = format!("# A comment.\n---\n{}", ci(&checkout()));
        assert_eq!(violations("ci.yml", &leading), Vec::<String>::new());
        // A first document that grants nothing, before the workflow.
        let two = format!(
            "permissions: {{}}\n---\n{}",
            ci(&checkout()).replace("permissions:\n  contents: read\n", "")
        );
        assert_eq!(violations("ci.yml", &two), [marker(2)]);
        let end = format!("{}...\n", ci(&checkout()));
        assert_eq!(violations("ci.yml", &end), [marker(14)]);
        let directive = format!("%YAML 1.2\n---\n{}", ci(&checkout()));
        assert_eq!(violations("ci.yml", &directive), [marker(1), marker(2)]);
        let inline = format!("--- !!map\n{}", ci(&checkout()));
        assert_eq!(violations("ci.yml", &inline), [marker(1)]);
    }

    #[test]
    fn workflows_are_the_yaml_files_headers_reads_as_workflows() {
        for (name, expected) in [
            ("ci.yml", true),
            ("release.yaml", true),
            ("NIGHTLY.YML", true),
            ("x.Yaml", true),
            (".yml", false),
            ("ci.yml.txt", false),
            ("notes.txt", false),
            ("README", false),
        ] {
            assert_eq!(is_workflow_name(name), expected, "{name}");
        }
    }

    #[test]
    fn the_directory_is_listed_without_following_a_link_and_read_within_a_bound() {
        let tree = TempTree::new("workflows-dir");
        with_workflow(&tree, "codeql.yml", &ci(&checkout()));
        // Not workflows: a directory, a folder below, another extension.
        tree.write(&format!("{DIR}/data/matrix.yml"), "uses: some/action@v1\n");
        tree.write(&format!("{DIR}/notes.txt"), "uses: some/action@v1\n");
        tree.write(&format!("{DIR}/sub.yml/x"), "");
        // Unread: too large, not UTF-8.
        tree.write(&format!("{DIR}/big.yml"), "#".repeat(4097));
        tree.write(&format!("{DIR}/latin1.yaml"), [0xE9, b'\n']);
        let report = check_dir(tree.path(), 4096).unwrap();
        assert_eq!(
            report.violations,
            [
                ".github/workflows/big.yml: larger than 4096 bytes, so it was not checked",
                ".github/workflows/latin1.yaml: not UTF-8, so it was not checked",
            ]
        );
        assert_eq!(report.notes, ["workflows checked: 2"]);
    }

    #[test]
    fn a_missing_or_empty_directory_fails() {
        let tree = TempTree::new("workflows-missing");
        let report = run(tree.path()).unwrap();
        assert_eq!(
            report.violations,
            [
                ".github/workflows was not found, so no workflow was checked: is this the \
                 repository root?"
            ]
        );
        assert_eq!(report.notes, ["workflows checked: 0"]);
        tree.write(".github/dependabot.yml", "version: 2\n");
        assert_eq!(run(tree.path()).unwrap().violations.len(), 1);
        tree.write(&format!("{DIR}/README.md"), "No workflows.\n");
        assert_eq!(
            run(tree.path()).unwrap().violations,
            [
                ".github/workflows holds no workflow (a `.yml` or `.yaml` file), so nothing was \
                 checked"
            ]
        );
        // A file where the directory should be.
        let file = TempTree::new("workflows-file");
        file.write(".github/workflows", "");
        assert_eq!(
            run(file.path()).unwrap().violations,
            [".github/workflows is not a directory, so no workflow was checked"]
        );
    }

    #[test]
    fn a_link_is_reported_and_nothing_it_leads_to_is_read() {
        let tree = TempTree::new("workflows-links");
        let outside = TempTree::new("workflows-links-outside");
        with_workflow(&tree, "codeql.yml", &ci(&checkout()));
        outside.write("evil.yml", "uses: some/action@v1\n");
        outside.write("dir/ci.yml", "uses: some/action@v1\n");
        if let Err(reason) = symlink(
            &tree,
            &format!("{DIR}/linked.yml"),
            &outside.path().join("evil.yml"),
        ) {
            let _ = writeln!(
                io::stderr(),
                "SKIPPED: a_link_is_reported_and_nothing_it_leads_to_is_read: cannot make a link \
                 here: {reason}"
            );
            return;
        }
        // Whatever its name and wherever it leads, a dangling one too.
        symlink(&tree, &format!("{DIR}/m"), &outside.path().join("dir")).expect("a second link");
        symlink(
            &tree,
            &format!("{DIR}/gone.yml"),
            &tree.path().join("missing"),
        )
        .expect("a third link");
        let report = run(tree.path()).unwrap();
        let link = |name: &str| {
            format!(
                ".github/workflows/{name}: a symbolic link, which `workflows` does not follow, so \
                 it was not read"
            )
        };
        assert_eq!(
            report.violations,
            [link("gone.yml"), link("linked.yml"), link("m")]
        );
        assert_eq!(report.notes, ["workflows checked: 2"]);

        // A linked directory on the way: nothing is listed.
        for linked in [".github", DIR] {
            let tree = TempTree::new("workflows-linked-dir");
            outside.write("repo/.github/workflows/ci.yml", ci(&checkout()));
            let target = if linked == DIR {
                outside.path().join("repo/.github/workflows")
            } else {
                outside.path().join("repo/.github")
            };
            symlink(&tree, linked, &target).expect("a link to a directory");
            let report = run(tree.path()).unwrap();
            assert_eq!(
                report.violations,
                [format!(
                    "{linked} is a symbolic link, which `workflows` does not follow, so no \
                     workflow was checked"
                )]
            );
            assert_eq!(report.notes, ["workflows checked: 0"]);
        }
    }

    #[test]
    fn a_workflow_named_with_control_characters_is_shown_escaped() {
        let tree = TempTree::new("workflows-control-names");
        with_workflow(&tree, "codeql.yml", &ci(&checkout()));
        let name = "a\n::warning::x\u{1b}[2K.yml";
        if let Err(error) = fs::write(tree.path().join(DIR).join(name), "name: x\n") {
            // Windows allows no control character in a file name.
            let _ = writeln!(
                io::stderr(),
                "SKIPPED: a_workflow_named_with_control_characters_is_shown_escaped: cannot name \
                 a file so here: {error}"
            );
            return;
        }
        assert_eq!(
            run(tree.path()).unwrap().violations,
            [
                ".github/workflows/a\\n::warning::x\\u{1b}[2K.yml: no row in the table of what \
                 each workflow may use (`WORKFLOWS` in xtask/src/workflows.rs, ADR-0022)"
            ]
        );
    }
}
