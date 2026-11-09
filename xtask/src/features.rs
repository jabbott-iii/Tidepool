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

//! `cargo xtask features`: `docs/features.jsonc`, Tidepool's features as
//! the demo page lists them, covers the roadmap (ADR-0030).
//!
//! Each feature names the roadmap tasks it needs. The page shows it as
//! shipped when every one of them is ticked, in progress when some are,
//! and planned when none are. Each also lists, as `ticked`, the tasks that
//! were ticked when its summary was written. The check fails when a ticked
//! task belongs to no feature, or when a feature's `ticked` is not what
//! the roadmap ticks: so the task that ships something also describes it,
//! in a new feature or by rewriting its feature's summary.
//!
//! Ported from nadir xtask `features.rs`. Adapted for Tidepool:
//!
//! - the roadmap's task lines are read as `crates` reads them
//!   ([`crates::task_line`]), so the two checks agree on what a task is:
//!   `[x]` ticks one, and `[X]`, which Nadir also takes, makes a line like
//!   a task's that is not one;
//! - a line with a CR inside it, where a renderer starts a new line and
//!   that reading does not, fails when a part of it looks like a task's;
//! - both files are read as `crates` reads a file ([`crates::read_file`]):
//!   a regular file, reached through no symbolic link, of at most
//!   [`MAX_BYTES`], in UTF-8;
//! - the features are read as JSONC ([`Dialect::Jsonc`]) within this
//!   check's own [`LIMITS`]. Tidepool's parser keeps every member of an
//!   object that repeats a key, where Nadir's refused the file, so a
//!   feature that gives a member twice fails here;
//! - a `name`, `area` or `summary` that holds a character `xtask` escapes
//!   in what it prints ([`walk::escape_controls`]: a control character, a
//!   line or paragraph separator, a bidirectional control) fails, so the
//!   page can show each as one line of plain text;
//! - a line of the roadmap is named by its number, not quoted; what a
//!   violation does quote of the files (a task ID or title, a member's
//!   name, the character the parser's error names) is escaped the same
//!   way, newlines and tabs too ([`quoted`]), so each violation is one line
//!   wherever it is printed. `check::printable` then prints it as it is.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::path::Path;

use tidepool_codec::Limits;
use tidepool_codec::json::{self, Dialect, Value};

use crate::check::Report;
use crate::crates;
use crate::error::Error;
use crate::walk;

/// The features, relative to the workspace root.
pub(crate) const FEATURES: &str = "docs/features.jsonc";
/// The roadmap, relative to the workspace root.
pub(crate) const ROADMAP: &str = crates::ROADMAP;

/// The largest file [`read`] reads, in bytes: as much as `crates` reads of
/// the roadmap.
const MAX_BYTES: u64 = crates::MAX_ROADMAP_BYTES;

/// What the features file may hold for `tidepool_codec::json` to read it: a
/// whole file of [`MAX_BYTES`], nested twice as deep as the file's four
/// levels (the object, `features`, a feature, its `tasks`).
const LIMITS: Limits = Limits {
    max_input_bytes: MAX_BYTES as usize,
    max_depth: 8,
    max_string_bytes: 64 * 1024,
    max_entries: 10_000,
    ..Limits::DEFAULT
};

/// The members a feature may have.
const MEMBERS: [&str; 5] = ["name", "area", "summary", "tasks", "ticked"];

/// A feature of Tidepool.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Feature {
    /// What it is called: a few words.
    pub(crate) name: String,
    /// The part of Tidepool it belongs to, such as `Text`.
    pub(crate) area: String,
    /// What it does, in a sentence or three.
    pub(crate) summary: String,
    /// The roadmap tasks it needs, such as `M1.3`.
    pub(crate) tasks: Vec<String>,
    /// Those of `tasks` that were ticked when `summary` was written.
    pub(crate) ticked: Vec<String>,
}

/// How far a feature has come.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Status {
    /// Every task it needs is ticked.
    Shipped,
    /// Some are.
    InProgress,
    /// None are.
    Planned,
}

impl Status {
    /// The status as the page names it.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Status::Shipped => "Shipped",
            Status::InProgress => "In progress",
            Status::Planned => "Planned",
        }
    }
}

/// A task of the roadmap.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Task {
    /// Its ID, such as `M1.3`.
    pub(crate) id: String,
    /// Its title, after the ID.
    pub(crate) title: String,
    /// Whether its box is ticked.
    pub(crate) done: bool,
}

/// Runs the check on the workspace at `root`.
///
/// # Errors
///
/// [`read`]'s, for either file.
pub(crate) fn run(root: &Path) -> Result<Report, Error> {
    let roadmap = read(root, ROADMAP)?;
    let features = read(root, FEATURES)?;
    Ok(check(&roadmap, &features))
}

/// The text of the file at `path`, relative to `root` with `/` between
/// components, read as [`crates::read_file`] reads one: a regular file,
/// reached through no symbolic link, of at most [`MAX_BYTES`], in UTF-8.
///
/// # Errors
///
/// [`Error::Read`], naming the file and what is wrong with it.
pub(crate) fn read(root: &Path, path: &str) -> Result<String, Error> {
    crates::read_file(root, path, MAX_BYTES)
}

/// Checks the text of the features file against the roadmap's.
fn check(roadmap: &str, features: &str) -> Report {
    let mut report = Report::default();
    // A task the reading below misses would escape the check. The line is
    // named by its number, not quoted.
    for (number, _) in malformed(roadmap) {
        report.violations.push(format!(
            "{ROADMAP}:{number}: a line like a task's that is not one: write it \
             `- [ ] **M1.2 — title**`, ticked as `[x]`"
        ));
    }
    for (number, _) in split_by_cr(roadmap) {
        report.violations.push(format!(
            "{ROADMAP}:{number}: a line like a task's holds a CR that does not end it, where a \
             renderer starts a new line: end every line with LF or CR LF"
        ));
    }
    let tasks = &tasks(roadmap);
    let features = match parse(features) {
        Ok(features) => features,
        Err(problems) => {
            report.violations.extend(problems);
            return report;
        }
    };
    let mut names = BTreeSet::new();
    let mut covered = BTreeSet::new();
    for feature in &features {
        // `parse` let through only names that need no quoting.
        if !names.insert(feature.name.as_str()) {
            report.violations.push(format!(
                "{FEATURES}: the feature `{}` is listed twice",
                feature.name
            ));
        }
        let mut own = BTreeSet::new();
        for id in &feature.tasks {
            if !own.insert(id.as_str()) {
                report.violations.push(format!(
                    "{FEATURES}: `{}` names {} twice",
                    feature.name,
                    quoted(id)
                ));
            }
            if !tasks.iter().any(|task| task.id == *id) {
                report.violations.push(format!(
                    "{FEATURES}: `{}` needs {}, which is not a task in {ROADMAP}",
                    feature.name,
                    quoted(id)
                ));
            }
            covered.insert(id.as_str());
        }
        let mut listed = BTreeSet::new();
        for id in &feature.ticked {
            if !listed.insert(id.as_str()) {
                report.violations.push(format!(
                    "{FEATURES}: `{}` lists {} as ticked twice",
                    feature.name,
                    quoted(id)
                ));
            }
            if !feature.tasks.contains(id) {
                report.violations.push(format!(
                    "{FEATURES}: `{}` lists {} as ticked, but not among its tasks",
                    feature.name,
                    quoted(id)
                ));
            }
        }
        let ticked: BTreeSet<&str> = feature
            .tasks
            .iter()
            .filter(|id| tasks.iter().any(|task| task.id == **id && task.done))
            .map(String::as_str)
            .collect();
        let written: BTreeSet<&str> = feature.ticked.iter().map(String::as_str).collect();
        if ticked != written && written.is_subset(&own) {
            let list = |ids: &BTreeSet<&str>| match ids.len() {
                0 => "none".to_string(),
                _ => ids
                    .iter()
                    .map(|id| quoted(id))
                    .collect::<Vec<_>>()
                    .join(", "),
            };
            report.violations.push(format!(
                "{FEATURES}: `{}`'s summary was written with {} ticked, but the roadmap \
                 ticks {}: rewrite it for what is built now, then set `ticked`",
                feature.name,
                list(&written),
                list(&ticked)
            ));
        }
    }
    let mut seen = BTreeSet::new();
    for task in tasks {
        if !seen.insert(task.id.as_str()) {
            report.violations.push(format!(
                "{ROADMAP}: {} is the ID of two tasks: give the new one its own",
                task.id
            ));
        }
        if task.done && !covered.contains(task.id.as_str()) {
            report.violations.push(format!(
                "{ROADMAP}: {} ({}) is ticked, but no feature in {FEATURES} needs it",
                task.id,
                quoted(&task.title)
            ));
        }
    }
    let count = |status| {
        features
            .iter()
            .filter(|feature| self::status(feature, tasks) == status)
            .count()
    };
    let counts: Vec<String> = [Status::Shipped, Status::InProgress, Status::Planned]
        .into_iter()
        .map(|status| format!("{} {}", count(status), status.name().to_lowercase()))
        .collect();
    report.notes.push(format!(
        "features: {}; roadmap tasks: {} of {} ticked",
        counts.join(", "),
        tasks.iter().filter(|task| task.done).count(),
        tasks.len()
    ));
    if !report.violations.is_empty() {
        report.notes.push(format!(
            "Every ticked task belongs to a feature in {FEATURES}, which the demo page \
             lists, and each feature's summary says what its ticked tasks built: add the \
             task to a feature or describe a new one, and keep `ticked` in step (ADR-0030)."
        ));
    }
    report
}

/// `text` from one of the files, as a violation quotes it: on one line,
/// with each character [`walk::escape_controls`] escapes written as its
/// escape, newlines and tabs too.
fn quoted(text: &str) -> Cow<'_, str> {
    walk::escape_controls(text, |_| false)
}

/// Whether `text` holds no character that [`quoted`] would escape.
fn plain(text: &str) -> bool {
    matches!(quoted(text), Cow::Borrowed(_))
}

/// The roadmap's tasks, in order: its lines `- [x] **M3.3 — title**`, or
/// with `[ ]` for a task not yet done, as [`crates::task_line`] reads them.
pub(crate) fn tasks(roadmap: &str) -> Vec<Task> {
    crates::roadmap_lines(roadmap)
        .filter_map(crates::task_line)
        .map(|task| Task {
            id: task.id.to_string(),
            title: task.title.to_string(),
            done: task.done,
        })
        .collect()
}

/// The lines of the roadmap, each with its number, counted from 1 at each
/// LF ([`crates::roadmap_lines`]).
fn numbered(roadmap: &str) -> impl Iterator<Item = (usize, &str)> {
    crates::roadmap_lines(roadmap)
        .enumerate()
        .map(|(index, line)| (index + 1, line))
}

/// The roadmap's lines that look like a task's but that [`tasks`] cannot
/// read, with their numbers: indented, with another bullet or dash than
/// `- ` and ` — `, more spaces, an ID that is not `M<number>.<number>`, or
/// another mark in the box than a space or `x`. A line with a CR inside it
/// is [`split_by_cr`]'s.
fn malformed(roadmap: &str) -> Vec<(usize, &str)> {
    numbered(roadmap)
        .filter(|(_, line)| {
            !line.contains('\r') && looks_like_task(line) && crates::task_line(line).is_none()
        })
        .collect()
}

/// The roadmap's lines that still hold a CR once their LF or CR LF ending
/// is taken off, and of which a part between CRs looks like a task's line,
/// with their numbers. A renderer starts a new line at such a CR, where
/// [`tasks`] does not: the task a reader sees may be one that [`tasks`]
/// misses.
fn split_by_cr(roadmap: &str) -> Vec<(usize, &str)> {
    numbered(roadmap)
        .filter(|(_, line)| line.contains('\r') && line.split('\r').any(looks_like_task))
        .collect()
}

/// Whether `line` looks like a task's: a list item, indented or quoted or
/// not, with a bullet (`-`, `*`, `+`) or a number (`1.`, `1)`), spaces or
/// tabs, then a checkbox of one character, and a task ID after it, such as
/// `M5.3` or `m5.3`, bold or not.
fn looks_like_task(line: &str) -> bool {
    let item = line.trim_start_matches(|c: char| c.is_whitespace() || c == '>');
    let after_bullet = match item.strip_prefix(['-', '*', '+']) {
        Some(rest) => rest,
        None => {
            let digits = item.len() - item.trim_start_matches(|c: char| c.is_ascii_digit()).len();
            match item[digits..].strip_prefix(['.', ')']) {
                Some(rest) if digits > 0 => rest,
                _ => return false,
            }
        }
    };
    let spaced = after_bullet.trim_start_matches([' ', '\t']);
    if spaced.len() == after_bullet.len() {
        return false;
    }
    let mut chars = spaced.chars();
    let checkbox = chars.next() == Some('[') && chars.next().is_some() && chars.next() == Some(']');
    checkbox && has_task_id(chars.as_str())
}

/// Whether `text` holds what starts a task ID: `M` or `m`, digits and `.`,
/// not inside a longer word. What follows the `.` may be a typo, such as
/// `M0.x`, which [`crates::task_line`] then refuses.
fn has_task_id(text: &str) -> bool {
    let bytes = text.as_bytes();
    (0..bytes.len()).any(|at| {
        let starts_word = at == 0 || !bytes[at - 1].is_ascii_alphanumeric();
        let digits = |from: usize| {
            bytes[from.min(bytes.len())..]
                .iter()
                .take_while(|byte| byte.is_ascii_digit())
                .count()
        };
        let milestone = digits(at + 1);
        starts_word
            && matches!(bytes[at], b'M' | b'm')
            && milestone > 0
            && bytes.get(at + 1 + milestone) == Some(&b'.')
    })
}

/// How far `feature` has come, by the ticks of the roadmap's `tasks`.
pub(crate) fn status(feature: &Feature, tasks: &[Task]) -> Status {
    let done = feature
        .tasks
        .iter()
        .filter(|id| tasks.iter().any(|task| task.id == **id && task.done))
        .count();
    if done == feature.tasks.len() {
        Status::Shipped
    } else if done > 0 {
        Status::InProgress
    } else {
        Status::Planned
    }
}

/// Reads the features file, JSONC: an object whose only member,
/// `features`, is an array of objects with the strings `name`, `area` and
/// `summary`, each one line of plain text ([`plain`]) and not blank,
/// `tasks`, an array of at least one task ID, and `ticked`, an array of
/// task IDs that may be left out; each member at most once. Returns every
/// problem found.
pub(crate) fn parse(text: &str) -> Result<Vec<Feature>, Vec<String>> {
    let root = json::parse(text, Dialect::Jsonc, LIMITS)
        .map_err(|error| vec![format!("{FEATURES}: {}", quoted(&error.to_string()))])?;
    let list = match root.as_object() {
        Some([(key, list)]) if key == "features" => list.as_array(),
        _ => None,
    }
    .ok_or_else(|| {
        vec![format!(
            "{FEATURES}: expected an object whose only member is the array `features`"
        )]
    })?;
    let mut features = Vec::new();
    let mut problems = Vec::new();
    for (index, entry) in list.iter().enumerate() {
        match feature(entry) {
            Ok(feature) => features.push(feature),
            Err(problem) => problems.push(format!("{FEATURES}: feature {}: {problem}", index + 1)),
        }
    }
    if problems.is_empty() {
        Ok(features)
    } else {
        Err(problems)
    }
}

/// One entry of the `features` array.
fn feature(entry: &Value) -> Result<Feature, String> {
    let members = entry
        .as_object()
        .ok_or("expected an object with `name`, `area`, `summary` and `tasks`")?;
    let mut given = BTreeSet::new();
    for (key, _) in members {
        if !MEMBERS.contains(&key.as_str()) {
            return Err(format!("unknown member `{}`", quoted(key)));
        }
        if !given.insert(key.as_str()) {
            return Err(format!("`{key}` is given twice"));
        }
    }
    let text = |key: &str| -> Result<String, String> {
        match entry.get(key).and_then(Value::as_str).map(str::trim) {
            Some(text) if !text.is_empty() && plain(text) => Ok(text.to_string()),
            Some(text) if !text.is_empty() => Err(format!(
                "`{key}` must be one line of plain text, without control characters, line or \
                 paragraph separators or bidirectional controls"
            )),
            _ => Err(format!("`{key}` must be a string that is not blank")),
        }
    };
    let name = text("name")?;
    let area = text("area")?;
    let summary = text("summary")?;
    let tasks: Vec<String> = entry
        .get("tasks")
        .and_then(Value::as_array)
        .and_then(|ids| ids.iter().map(|id| id.as_str().map(String::from)).collect())
        .filter(|ids: &Vec<String>| !ids.is_empty())
        .ok_or_else(|| format!("`{name}`: `tasks` must be an array of task IDs, not empty"))?;
    let ticked: Vec<String> = match entry.get("ticked") {
        None => Vec::new(),
        Some(ids) => ids
            .as_array()
            .and_then(|ids| ids.iter().map(|id| id.as_str().map(String::from)).collect())
            .ok_or_else(|| format!("`{name}`: `ticked` must be an array of task IDs"))?,
    };
    Ok(Feature {
        name,
        area,
        summary,
        tasks,
        ticked,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{self, Write as _};

    use super::*;
    use crate::walk::testing::{TempTree, symlink};

    const ROADMAP_TEXT: &str = "\
# Roadmap

## M0 — Foundation

- [x] **M0.1 — Workspace skeleton**
  - Do: things
- [ ] **M0.2 — core: math and color**
- [x] **M0.3 — core: handles, hashing, RNG**
- [x] Not a task: no ID

| Item | Trigger |
| :- | :- |
| Native Wayland | - [x] **M9.9 — in a table, not at the line's start** |
";

    fn roadmap() -> Vec<Task> {
        tasks(ROADMAP_TEXT)
    }

    /// A feature that needs `ids`, written with those that `ROADMAP_TEXT`
    /// ticks ticked.
    fn feature(name: &str, ids: &[&str]) -> String {
        let done: Vec<String> = roadmap()
            .into_iter()
            .filter(|task| task.done)
            .map(|task| task.id)
            .collect();
        let mut ticked: Vec<&str> = ids
            .iter()
            .copied()
            .filter(|id| done.iter().any(|done| done == id))
            .collect();
        ticked.dedup();
        feature_written_with(name, ids, &ticked)
    }

    /// A feature that needs `ids`, written with `ticked` ticked.
    fn feature_written_with(name: &str, ids: &[&str], ticked: &[&str]) -> String {
        let list = |ids: &[&str]| {
            ids.iter()
                .map(|id| format!("\"{id}\""))
                .collect::<Vec<_>>()
                .join(", ")
        };
        format!(
            "{{ \"name\": \"{name}\", \"area\": \"Core\", \"summary\": \"It works.\", \
             \"tasks\": [{}], \"ticked\": [{}] }}",
            list(ids),
            list(ticked)
        )
    }

    fn file(features: &[String]) -> String {
        format!(
            "// The features.\n{{\n  \"features\": [\n    {}\n  ]\n}}\n",
            features.join(",\n    ")
        )
    }

    #[test]
    fn the_roadmaps_tasks_are_its_checkbox_lines_with_an_id() {
        let text = format!(
            "{ROADMAP_TEXT}- [x] **M0.4 — with a CR LF ending**\r\n\
             - [ ] **M0.5 — a title** and text after it\n\
             - [ ] **M0.6 — no closing stars\n"
        );
        let tasks = tasks(&text);
        let ids: Vec<(&str, &str, bool)> = tasks
            .iter()
            .map(|task| (task.id.as_str(), task.title.as_str(), task.done))
            .collect();
        assert_eq!(
            ids,
            [
                ("M0.1", "Workspace skeleton", true),
                ("M0.2", "core: math and color", false),
                ("M0.3", "core: handles, hashing, RNG", true),
                ("M0.4", "with a CR LF ending", true),
                ("M0.5", "a title", false),
                ("M0.6", "no closing stars", false),
            ]
        );
        assert_eq!(malformed(&text), []);
        assert_eq!(split_by_cr(&text), []);
    }

    #[test]
    fn a_line_like_a_tasks_that_is_not_one_fails() {
        let bad = [
            "- [x] **M5.9 - an ASCII hyphen**",
            "- [x] **M5.9 – an en dash**",
            "- [ ] **M0.x — not a number**",
            "- [?] **M0.4 — an unknown mark**",
            // `crates` reads only `[x]` as ticked, and so does this check.
            "- [X] **M0.4 — a capital X**",
            "  - [x] **M5.3 — indented**",
            "- [x]  **M5.3 — two spaces**",
            "* [x] **M5.3 — another bullet**",
            "-  [x] **M5.3 — two spaces after the bullet**",
            "-\t[x] **M5.3 — a tab after the bullet**",
            "1. [x] **M5.3 — a numbered item**",
            "12) [ ] **M5.3 — a numbered item with a parenthesis**",
            "- [x] M12.1 — native Wayland, not bold",
            "- [x] __M12.1 — underscores__",
            "- [x] *M12.1 — one star*",
            "- [x] **m12.1 — a small m**",
            "- [x] **[M12.1 — a link](decisions.md)**",
            "- [x] **M1.2.3 — three parts**",
            "> - [x] **M12.1 — quoted**",
        ];
        let text = format!("{ROADMAP_TEXT}{}\n", bad.join("\n"));
        let first = ROADMAP_TEXT.lines().count() + 1;
        let expected: Vec<(usize, &str)> = (first..).zip(bad).collect();
        assert_eq!(malformed(&text), expected);
        let report = check(
            &text,
            &file(&[
                feature("Workspace", &["M0.1"]),
                feature("Math", &["M0.2", "M0.3"]),
            ]),
        );
        assert_eq!(
            report.violations.len(),
            bad.len(),
            "{:?}",
            report.violations
        );
        // Not task-like: no checkbox, no space after the bullet, no task ID,
        // no bullet, or a link where the checkbox would be.
        for line in [
            "- **M5.3 — no checkbox**",
            "-[x] **M5.3 — no space**",
            "- [x] **Milestone**",
            "- [x] **ADM5.3**",
            "- [x] **M5 — a milestone**",
            "[x] **M5.3 — no bullet**",
            "1.5 [x] **M5.3 — a decimal**",
            "  - [ADR-0026](decisions.md): **M4.8** moved",
        ] {
            assert!(!looks_like_task(line), "{line}");
        }
        assert_eq!(
            report.violations[0],
            format!(
                "docs/roadmap.md:{first}: a line like a task's that is not one: write it \
                 `- [ ] **M1.2 — title**`, ticked as `[x]`"
            )
        );
    }

    #[test]
    fn a_cr_inside_a_line_like_a_tasks_fails() {
        // A renderer shows M0.4 on a line of its own; the reading sees one
        // line, M0.3's, and would miss it.
        let hidden = "- [x] **M0.3 — core: handles, hashing, RNG**\r- [x] **M0.4 — hidden**";
        let text = format!(
            "{ROADMAP_TEXT}{hidden}\n\
             A CR\rin a line of prose is not a task's.\n"
        );
        let number = ROADMAP_TEXT.lines().count() + 1;
        assert_eq!(split_by_cr(&text), [(number, hidden)]);
        assert_eq!(malformed(&text), []);
        let report = check(
            &text,
            &file(&[
                feature("Workspace", &["M0.1"]),
                feature("Math", &["M0.2", "M0.3"]),
            ]),
        );
        assert_eq!(
            report.violations,
            [
                format!(
                    "docs/roadmap.md:{number}: a line like a task's holds a CR that does not \
                     end it, where a renderer starts a new line: end every line with LF or CR LF"
                ),
                "docs/roadmap.md: M0.3 is the ID of two tasks: give the new one its own"
                    .to_string(),
            ]
        );
    }

    #[test]
    fn a_feature_is_shipped_planned_or_in_between_by_its_tasks_ticks() {
        let tasks = roadmap();
        let status = |ids: &[&str]| {
            let feature = Feature {
                name: "x".to_string(),
                area: "Core".to_string(),
                summary: "It works.".to_string(),
                tasks: ids.iter().map(|id| (*id).to_string()).collect(),
                ticked: Vec::new(),
            };
            super::status(&feature, &tasks)
        };
        assert_eq!(status(&["M0.1", "M0.3"]), Status::Shipped);
        assert_eq!(status(&["M0.1", "M0.2"]), Status::InProgress);
        assert_eq!(status(&["M0.2"]), Status::Planned);
        assert_eq!(Status::Shipped.name(), "Shipped");
        assert_eq!(Status::InProgress.name(), "In progress");
        assert_eq!(Status::Planned.name(), "Planned");
    }

    #[test]
    fn features_that_cover_every_ticked_task_pass() {
        let text = file(&[
            feature("Workspace", &["M0.1"]),
            feature("Math", &["M0.2", "M0.3"]),
        ]);
        let report = check(ROADMAP_TEXT, &text);
        assert_eq!(report.violations, Vec::<String>::new());
        assert_eq!(
            report.notes,
            ["features: 1 shipped, 1 in progress, 0 planned; roadmap tasks: 2 of 3 ticked"]
        );
    }

    #[test]
    fn a_ticked_task_that_no_feature_needs_fails() {
        let report = check(ROADMAP_TEXT, &file(&[feature("Workspace", &["M0.1"])]));
        assert_eq!(
            report.violations,
            [
                "docs/roadmap.md: M0.3 (core: handles, hashing, RNG) is ticked, but no \
                 feature in docs/features.jsonc needs it"
            ]
        );
        assert!(report.notes[1].contains("add the task to a feature"));
    }

    #[test]
    fn unknown_and_repeated_tasks_and_repeated_names_fail() {
        let text = file(&[
            feature("Workspace", &["M0.1", "M0.1", "M7.7"]),
            feature("Workspace", &["M0.3"]),
        ]);
        assert_eq!(
            check(ROADMAP_TEXT, &text).violations,
            [
                "docs/features.jsonc: `Workspace` names M0.1 twice",
                "docs/features.jsonc: `Workspace` needs M7.7, which is not a task in \
                 docs/roadmap.md",
                "docs/features.jsonc: the feature `Workspace` is listed twice",
            ]
        );
    }

    #[test]
    fn a_file_of_the_wrong_shape_says_what_is_wrong_with_each_feature() {
        let wrong = [
            (
                "[]",
                "expected an object whose only member is the array `features`",
            ),
            (
                "{ \"features\": [], \"more\": 1 }",
                "expected an object whose only member is the array `features`",
            ),
            // Tidepool's parser keeps both members; the file is refused.
            (
                "{ \"features\": [], \"features\": [] }",
                "expected an object whose only member is the array `features`",
            ),
            (
                "{ \"features\": [1",
                "docs/features.jsonc: JSON error at line 1",
            ),
            // Nested past the check's own limits.
            (
                "{ \"features\": [[[[[[[[]]]]]]]] }",
                "docs/features.jsonc: JSON error at line 1",
            ),
        ];
        for (text, message) in wrong {
            let problems = parse(text).expect_err(text);
            assert_eq!(problems.len(), 1, "{text}: {problems:?}");
            assert!(problems[0].contains(message), "{text}: {problems:?}");
        }
        let text = r#"{ "features": [
            1,
            { "name": "A", "area": "Core", "summary": " ", "tasks": ["M0.1"] },
            { "name": "B", "area": "Core", "summary": "Yes.", "tasks": [] },
            { "name": "C", "area": "Core", "summary": "Yes.", "tasks": ["M0.1", 2] },
            { "name": "D", "area": "Core", "summary": "Yes.", "tasks": ["M0.1"], "x": 1 },
            { "area": "Core", "summary": "Yes.", "tasks": ["M0.1"] },
            { "name": "E", "area": "Core", "summary": "Yes.", "tasks": ["M0.1"] },
            { "name": "F", "area": "Core", "summary": "Yes.", "tasks": ["M0.1"], "tasks": [] },
            { "name": "G", "area": "Co\nre", "summary": "Yes.", "tasks": ["M0.1"] },
            { "name": "H", "area": "Core", "summary": "Yes.\u202e", "tasks": ["M0.1"] },
            { "name": "I", "area": "Core", "summary": 7, "tasks": ["M0.1"] },
            { "name": "J", "area": "Core", "summary": "Yes.", "tasks": ["M0.1"], "a\u001bb": 1 }
        ] }"#;
        assert_eq!(
            parse(text).expect_err("wrong features"),
            [
                "docs/features.jsonc: feature 1: expected an object with `name`, `area`, \
                 `summary` and `tasks`",
                "docs/features.jsonc: feature 2: `summary` must be a string that is not blank",
                "docs/features.jsonc: feature 3: `B`: `tasks` must be an array of task IDs, \
                 not empty",
                "docs/features.jsonc: feature 4: `C`: `tasks` must be an array of task IDs, \
                 not empty",
                "docs/features.jsonc: feature 5: unknown member `x`",
                "docs/features.jsonc: feature 6: `name` must be a string that is not blank",
                "docs/features.jsonc: feature 8: `tasks` is given twice",
                "docs/features.jsonc: feature 9: `area` must be one line of plain text, without \
                 control characters, line or paragraph separators or bidirectional controls",
                "docs/features.jsonc: feature 10: `summary` must be one line of plain text, \
                 without control characters, line or paragraph separators or bidirectional \
                 controls",
                "docs/features.jsonc: feature 11: `summary` must be a string that is not blank",
                "docs/features.jsonc: feature 12: unknown member `a\\u{1b}b`",
            ]
        );
        let report = check(ROADMAP_TEXT, "{");
        assert_eq!(report.violations.len(), 1);
        assert!(report.notes.is_empty());
    }

    #[test]
    fn the_names_and_summaries_are_trimmed() {
        let text = r#"{ "features": [
            { "name": " A ", "area": "Core\n", "summary": " Yes. ", "tasks": ["M0.1"] }
        ] }"#;
        let features = parse(text).expect("valid");
        assert_eq!(
            features,
            [Feature {
                name: "A".to_string(),
                area: "Core".to_string(),
                summary: "Yes.".to_string(),
                tasks: vec!["M0.1".to_string()],
                ticked: Vec::new(),
            }]
        );
    }

    #[test]
    fn a_summary_written_for_other_ticks_than_the_roadmaps_fails() {
        let check = |ticked: &[&str]| {
            let text = file(&[
                feature("Workspace", &["M0.1"]),
                feature_written_with("Math", &["M0.2", "M0.3"], ticked),
            ]);
            super::check(ROADMAP_TEXT, &text).violations
        };
        assert_eq!(check(&["M0.3"]), Vec::<String>::new());
        assert_eq!(
            check(&[]),
            [
                "docs/features.jsonc: `Math`'s summary was written with none ticked, but the \
                 roadmap ticks M0.3: rewrite it for what is built now, then set `ticked`"
            ]
        );
        assert_eq!(
            check(&["M0.2", "M0.3"]),
            [
                "docs/features.jsonc: `Math`'s summary was written with M0.2, M0.3 ticked, but \
                 the roadmap ticks M0.3: rewrite it for what is built now, then set `ticked`"
            ]
        );
        assert_eq!(
            check(&["M0.3", "M0.1"]),
            ["docs/features.jsonc: `Math` lists M0.1 as ticked, but not among its tasks"]
        );
        assert_eq!(
            check(&["M0.3", "M0.3"]),
            ["docs/features.jsonc: `Math` lists M0.3 as ticked twice"]
        );
        let text = r#"{ "features": [
            { "name": "A", "area": "Core", "summary": "Yes.", "tasks": ["M0.1"], "ticked": "M0.1" }
        ] }"#;
        assert_eq!(
            parse(text).expect_err("not an array"),
            ["docs/features.jsonc: feature 1: `A`: `ticked` must be an array of task IDs"]
        );
    }

    #[test]
    fn two_tasks_with_one_id_fail() {
        let text = format!("{ROADMAP_TEXT}- [ ] **M0.3 — a copy, not renumbered**\n");
        let report = super::check(
            &text,
            &file(&[
                feature("Workspace", &["M0.1"]),
                feature("Math", &["M0.2", "M0.3"]),
            ]),
        );
        assert_eq!(
            report.violations,
            ["docs/roadmap.md: M0.3 is the ID of two tasks: give the new one its own"]
        );
    }

    #[test]
    fn malformed_task_lines_are_reported_with_a_features_file_that_does_not_parse() {
        let text = format!("{ROADMAP_TEXT}- [x] **M5.9 - an ASCII hyphen**\n");
        let report = super::check(&text, "{");
        assert_eq!(report.violations.len(), 2, "{:?}", report.violations);
        let number = ROADMAP_TEXT.lines().count() + 1;
        assert!(
            report.violations[0].starts_with(&format!("docs/roadmap.md:{number}: a line like")),
            "{:?}",
            report.violations
        );
    }

    #[test]
    fn what_a_violation_quotes_of_the_files_is_escaped_on_one_line() {
        // A task ID and a title with control characters and a line
        // separator, as a hostile file might hold them.
        let roadmap = format!("{ROADMAP_TEXT}- [x] **M0.4 — a\u{1b}[2K\u{2028}title**\n");
        let features = file(&[
            feature("Workspace", &["M0.1"]),
            feature("Math", &["M0.2", "M0.3"]),
            feature_written_with("Forged", &["M0.9\\n::error::x"], &["M0.9\\n::error::x"]),
        ]);
        let report = check(&roadmap, &features);
        assert_eq!(
            report.violations,
            [
                "docs/features.jsonc: `Forged` needs M0.9\\n::error::x, which is not a task in \
                 docs/roadmap.md",
                "docs/features.jsonc: `Forged`'s summary was written with M0.9\\n::error::x \
                 ticked, but the roadmap ticks none: rewrite it for what is built now, then set \
                 `ticked`",
                "docs/roadmap.md: M0.4 (a\\u{1b}[2K\\u{2028}title) is ticked, but no feature in \
                 docs/features.jsonc needs it",
            ]
        );
        for violation in &report.violations {
            assert!(
                !violation.chars().any(|c| c.is_control() || c == '\u{2028}'),
                "{violation:?}"
            );
        }
    }

    #[test]
    fn the_files_are_read_only_as_regular_files_reached_through_no_link() {
        let tree = TempTree::new("features-read");
        let features = file(&[
            feature("Workspace", &["M0.1"]),
            feature("Math", &["M0.2", "M0.3"]),
        ]);
        // Without the roadmap, the check cannot run.
        let error = run(tree.path()).expect_err("there is no roadmap");
        assert!(
            matches!(&error, Error::Read { source, .. } if source.kind() == io::ErrorKind::NotFound),
            "{error:?}"
        );
        tree.write(ROADMAP, ROADMAP_TEXT);
        // Nor without the features, or with a directory in their place.
        let error = run(tree.path()).expect_err("there are no features");
        assert!(error.to_string().contains("features.jsonc"), "{error}");
        fs::create_dir_all(tree.path().join(FEATURES)).unwrap();
        let error = run(tree.path()).expect_err("the features are a directory");
        assert!(
            error.to_string().ends_with("it is not a regular file"),
            "{error}"
        );
        fs::remove_dir(tree.path().join(FEATURES)).unwrap();
        tree.write(FEATURES, &features);
        assert_eq!(run(tree.path()).unwrap().violations, Vec::<String>::new());
        // A linked file is refused, and what it leads to is not read.
        tree.write("elsewhere.jsonc", "this line must never be printed");
        fs::remove_file(tree.path().join(FEATURES)).unwrap();
        if let Err(reason) = symlink(&tree, FEATURES, &tree.path().join("elsewhere.jsonc")) {
            let _ = writeln!(
                io::stderr(),
                "SKIPPED: the_files_are_read_only_as_regular_files_reached_through_no_link: \
                 cannot make a link here: {reason}"
            );
            return;
        }
        let error = run(tree.path()).expect_err("the features are a link");
        let message = error.to_string();
        assert!(
            message.contains("docs/features.jsonc is a symbolic link, which xtask does not follow"),
            "{message}"
        );
        assert!(!message.contains("must never be printed"), "{message}");
    }

    #[test]
    fn the_workspaces_features_cover_its_roadmap() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("the workspace");
        let report = run(root).expect("the files read");
        assert_eq!(report.violations, Vec::<String>::new());
    }

    #[test]
    fn any_roadmap_gets_an_answer_whose_violations_are_one_line_each() {
        // Pieces of task lines and near misses, line endings, and
        // characters that no violation may hold unescaped. Whole starts of
        // task lines, after a line ending, make task lines common enough
        // for the agreement below to be tested.
        const PIECES: &[&str] = &[
            "\n- [x] **M1.2 \u{2014} ",
            "\n- [ ] **M0.12 \u{2014} ",
            "\r\n- [x] **M3.4 \u{2014} ",
            "\n- [X] **M1.2 \u{2014} ",
            "\n  - [x] **M1.2 \u{2014} ",
            "\n* [ ] **M1.2 \u{2014} ",
            "\n> - [x] **m5.6 \u{2014} ",
            "\n- [x] **M1.2.3 \u{2014} ",
            "\r- [x] **M7.7 \u{2014} ",
            "M1.2",
            "M0.x",
            "- [x] **",
            "- [ ] **",
            "- [X] **",
            "* [x] ",
            "1. [ ] ",
            "> ",
            "  ",
            "\t",
            "M",
            "m",
            "0",
            "12",
            ".",
            "x",
            " \u{2014} ",
            " - ",
            "**",
            "[",
            "]",
            "\n",
            "\r\n",
            "\r",
            "\u{1b}",
            "\u{2028}",
            "\u{202e}",
            "\u{c4}",
            "title",
        ];
        tidepool_core::testing::check_sized(
            "features_scanners",
            2000,
            |rng, size| {
                let mut text = String::new();
                for _ in 0..rng.range(0..size + 1) {
                    text.push_str(rng.choose(PIECES).copied().unwrap_or(""));
                }
                text
            },
            |roadmap| {
                let found = tasks(roadmap);
                let bad = malformed(roadmap);
                let split = split_by_cr(roadmap);
                // `crates`' reader and the look of a task line agree: every
                // task line looks like one, and none is also reported.
                for (number, line) in numbered(roadmap) {
                    if crates::task_line(line).is_some() {
                        if !looks_like_task(line) {
                            return Err(format!(
                                "line {number} is a task but does not look like one"
                            ));
                        }
                        if bad.iter().any(|(at, _)| *at == number) {
                            return Err(format!("line {number} is a task and reported malformed"));
                        }
                    }
                }
                // A feature that needs every task, written with the ticked
                // ones ticked, leaves only the roadmap's own violations.
                let mut ids: Vec<&str> = found.iter().map(|task| task.id.as_str()).collect();
                ids.sort_unstable();
                ids.dedup();
                let ticked: Vec<&str> = ids
                    .iter()
                    .copied()
                    .filter(|id| found.iter().any(|task| task.id == *id && task.done))
                    .collect();
                let features = if ids.is_empty() {
                    "{ \"features\": [] }".to_string()
                } else {
                    file(&[feature_written_with("All", &ids, &ticked)])
                };
                let report = check(roadmap, &features);
                let expected = bad.len() + split.len() + found.len() - ids.len();
                if report.violations.len() != expected {
                    return Err(format!(
                        "{} violations, expected {expected}: {:?}",
                        report.violations.len(),
                        report.violations
                    ));
                }
                if let Some(violation) = report
                    .violations
                    .iter()
                    .find(|violation| !violation.starts_with(ROADMAP) || !plain(violation))
                {
                    return Err(format!("a violation not one plain line: {violation:?}"));
                }
                Ok(())
            },
        );
    }
}
