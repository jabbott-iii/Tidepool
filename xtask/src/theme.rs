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

//! `cargo xtask theme`: each theme in `themes/` meets WCAG AA for the pairs
//! of colors it uses for text (ADR-0004, ADR-0047).
//!
//! New in Tidepool; not ported.
//!
//! A theme file lists, in `[contrast]`, the text colors and the surfaces of
//! the table this check prints (`text` and `on`), and in each
//! `[[contrast.use]]` a pair it uses for text: a `text` color, the
//! surfaces it is used `on`, a `size`, `normal` ([`NORMAL`], 4.5:1) or
//! `large` ([`LARGE`], 3:1, also for UI glyphs), and an optional `over`,
//! the opaque color a translucent surface is composited over, `bg` when it
//! is not given. The check fails when a pair is below its minimum,
//! unrounded, and when the file breaks the rules of that table.
//!
//! It reads the file with `tidepool_codec::toml` and its colors with
//! `tidepool_core::color`, as `tidepool-ui` does; `xtask` may not use `ui`
//! (`docs/01-architecture.md` → "Dependency matrix"), whose own tests read
//! every token of the bundled theme. Of the colors it reads `value` and
//! `solid`; a `solid` value must be opaque, given only for a translucent
//! value, and within one level a channel of that value over `bg`: the
//! spec's solid values were composited with their alphas exactly (0.78
//! for `panel`), where `core` keeps alpha in 8 bits and rounds the
//! premultiplied channels, which moves four of the five by one level in a
//! channel (`panel` to `#0e1213`, `line` to `#1e1f20`).
//!
//! # Contrast
//!
//! WCAG 2.x: each channel `c` of 0 to 1 is linearized,
//! `c ≤ 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055)^2.4` (WCAG writes
//! 0.03928, which no 8-bit value falls between); the relative luminance is
//! `0.2126 R + 0.7152 G + 0.0722 B`; and the ratio of two colors is
//! `(L1 + 0.05) / (L2 + 0.05)`, the lighter over the darker. A
//! translucent color is first composited by `core`'s source-over, as `gui`
//! blends it: a text color over its surface, and a surface over `over`,
//! except that a surface over `bg` that gives a `solid` value is that
//! value, which the table's `(solid)` column names (`core`'s composite of
//! `panel` is `#0e1213`, the spec's solid value `#0f1113`).
//!
//! The table is Markdown, as `docs/subsystems/ui-and-theme.md` → Contrast
//! has it, which a test compares byte for byte: two decimals, a pair from
//! 3 to 4.5 marked `(large only)`, one below 3 `(fails)`, and a
//! translucent surface's column marked `(solid)`.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use tidepool_codec::Limits;
use tidepool_codec::toml::{self, Span, Table, Value};
use tidepool_core::color::Color;

use crate::check::Report;
use crate::crates;
use crate::error::Error;
use crate::walk;

/// The directory of the themes, relative to the workspace root.
pub(crate) const DIR: &str = "themes";

/// The least contrast for normal text.
pub(crate) const NORMAL: f64 = 4.5;

/// The least contrast for large text and UI glyphs.
pub(crate) const LARGE: f64 = 3.0;

/// The largest theme file read, in bytes.
const MAX_BYTES: u64 = 1024 * 1024;

/// What a theme file may hold for `tidepool_codec::toml` to read it, as
/// `tidepool-ui` reads one (its `theme::LIMITS`).
const LIMITS: Limits = Limits {
    max_input_bytes: MAX_BYTES as usize,
    max_depth: 8,
    max_string_bytes: 4096,
    max_number_bytes: 64,
    max_entries: 1024,
    ..Limits::DEFAULT
};

/// The keys `[contrast]` may have.
const CONTRAST_KEYS: [&str; 3] = ["text", "on", "use"];

/// The keys a `[[contrast.use]]` may have.
const USE_KEYS: [&str; 4] = ["text", "on", "size", "over"];

/// Runs the check on the workspace at `root`.
///
/// # Errors
///
/// [`Error::Read`] if a directory on the way to `themes/`, or `themes/`
/// itself, cannot be read.
pub(crate) fn run(root: &Path) -> Result<Report, Error> {
    let mut report = Report::default();
    if let Some(problem) = directory_problem(root)? {
        report.violations.push(problem);
        return Ok(report);
    }
    let found = list(&root.join(DIR))?;
    for link in &found.links {
        report.violations.push(format!(
            "{} is a symbolic link, which `theme` does not follow, so it was not checked",
            walk::relative(root, link)
        ));
    }
    for other in &found.others {
        report.violations.push(format!(
            "{} is named like a theme but is not a regular file or valid UTF-8, so it was not \
             read",
            walk::relative(root, other)
        ));
    }
    if found.themes.is_empty() && found.links.is_empty() && found.others.is_empty() {
        report.violations.push(format!(
            "{DIR}/ holds no theme (a `.toml` file), so none was checked"
        ));
    }
    for name in &found.themes {
        let rel = format!("{DIR}/{name}");
        let shown = walk::relative(root, &root.join(&rel));
        match crates::read_file(root, &rel, MAX_BYTES) {
            Ok(text) => check(&shown, &text, &mut report),
            Err(Error::Read { source, .. }) => report
                .violations
                .push(format!("{shown} cannot be read: {source}")),
            Err(other) => report
                .violations
                .push(format!("{shown} cannot be read: {other}")),
        }
    }
    Ok(report)
}

/// What is wrong with the way to `themes/`, if anything: a component that
/// is missing, a symbolic link or not a directory.
///
/// # Errors
///
/// [`Error::Read`] if a component cannot be looked at.
fn directory_problem(root: &Path) -> Result<Option<String>, Error> {
    let at = root.join(DIR);
    let problem = match fs::symlink_metadata(&at) {
        Ok(meta) if meta.file_type().is_symlink() => {
            "is a symbolic link, which `theme` does not follow, so no theme was checked"
        }
        Ok(meta) if meta.file_type().is_dir() => return Ok(None),
        Ok(_) => "is not a directory, so no theme was checked",
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(Some(format!(
                "{DIR}/ was not found, so no theme was checked: is this the repository root?"
            )));
        }
        Err(source) => return Err(Error::Read { path: at, source }),
    };
    Ok(Some(format!("{} {problem}", walk::relative(root, &at))))
}

/// What [`list`] found directly in `themes/`.
#[derive(Debug, Default)]
struct Found {
    /// The names of the regular files named like a theme, sorted.
    themes: Vec<String>,
    /// Every symbolic link, whatever its name, sorted.
    links: Vec<PathBuf>,
    /// The entries named like a theme that are not regular files, or
    /// whose names are not UTF-8, sorted.
    others: Vec<PathBuf>,
}

/// Lists the entries directly in `dir`. A directory there is not a theme,
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
        // Does not follow a symbolic link.
        let file_type = entry.file_type().map_err(read_error)?;
        let path = entry.path();
        let lossy = entry.file_name().to_string_lossy().into_owned();
        if file_type.is_symlink() {
            found.links.push(path);
        } else if file_type.is_dir() || !is_theme_name(&lossy) {
            continue;
        } else if let (true, Some(name)) = (file_type.is_file(), entry.file_name().to_str()) {
            found.themes.push(name.to_owned());
        } else {
            found.others.push(path);
        }
    }
    found.themes.sort();
    found.links.sort();
    found.others.sort();
    Ok(found)
}

/// Whether a file named `name` in [`DIR`] is a theme: its extension, after
/// a `.` that is not its first character, is `toml`.
fn is_theme_name(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(stem, extension)| !stem.is_empty() && extension == "toml")
}

/// A color of the theme, as this check reads it.
#[derive(Clone, Copy, Debug)]
struct Entry {
    /// Its `value`.
    value: Color,
    /// Its `solid`, if it gives one that reads ([`check_solids`] says
    /// what is wrong with one that does not, or that breaks a rule).
    solid: Option<Color>,
}

impl Entry {
    /// The color this entry makes as a surface over `below`, named
    /// `below_name`: its value when that is opaque; over `bg`, its `solid`
    /// when it gives one, as the table's `(solid)` column says; otherwise
    /// its value composited over `below`.
    fn over(&self, below_name: &str, below: Color) -> Color {
        match self.solid {
            Some(solid) if self.value.a != u8::MAX && below_name == "bg" => solid,
            _ => composite(self.value, below),
        }
    }
}

/// The colors of `[colors]` by name: `None` for one whose entry does not
/// read (not a table, no `value`, or a `value` that is not a color), which
/// has its violation already, so that what names it adds none.
type Palette = BTreeMap<String, Option<Entry>>;

/// The color `name` names in `palette`, if its entry reads.
fn color_of(palette: &Palette, name: &str) -> Option<Entry> {
    palette.get(name).copied().flatten()
}

/// One pair of `[[contrast.use]]`: a text color on a surface.
#[derive(Debug)]
struct Use {
    /// The text color's name.
    text: String,
    /// The surface's name.
    on: String,
    /// What the surface is composited over, if it is translucent.
    over: String,
    /// The least contrast it needs.
    min: f64,
    /// What it is used for: `normal text` or `large text`.
    size: &'static str,
    /// The line of the entry's `text`.
    line: usize,
}

/// Where a theme's problems are: the file as shown, and where its lines
/// start, so that naming the line of each of many violations takes time
/// in the log of the file's lines, not in its size.
struct Place<'a> {
    shown: &'a str,
    /// The offset of each line's first byte, from 0, in order.
    starts: Vec<usize>,
}

impl<'a> Place<'a> {
    /// The place of `text`, shown as `shown`. Lines end at each LF, as
    /// `codec`'s errors count them.
    fn new(shown: &'a str, text: &str) -> Self {
        let starts = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(at, _)| at + 1))
            .collect();
        Self { shown, starts }
    }

    /// `<file>:<line>` of `span`.
    fn at(&self, span: Span) -> String {
        format!("{}:{}", self.shown, self.line(span))
    }

    /// The line of `span`'s start, from 1.
    fn line(&self, span: Span) -> usize {
        self.starts.partition_point(|&start| start <= span.start)
    }
}

/// Checks the theme `text`, shown as `shown`, adding its table to the
/// notes and each problem to the violations.
fn check(shown: &str, text: &str, report: &mut Report) {
    let violations = &mut report.violations;
    let root = match toml::parse(text, LIMITS) {
        Ok(root) => root,
        Err(error) => {
            violations.push(format!(
                "{shown} cannot be parsed: {}",
                quoted(&error.to_string())
            ));
            return;
        }
    };
    let place = Place::new(shown, text);
    let at = |span: Span| place.at(span);
    let Some(colors) = table_at(&root, "colors", shown, &at, violations) else {
        return;
    };
    let palette = palette(colors, &at, violations);
    let bg = match palette.get("bg") {
        Some(Some(bg)) if bg.value.a == u8::MAX => bg.value,
        // Its entry does not read, and has its violation.
        Some(None) => return,
        Some(Some(_)) => {
            violations.push(format!(
                "{shown}: `colors.bg` is translucent, though translucent colors are composited \
                 over it"
            ));
            return;
        }
        None => {
            violations.push(format!(
                "{shown}: `colors.bg` is missing, though translucent colors are composited over it"
            ));
            return;
        }
    };
    check_solids(colors, &palette, bg, &at, violations);

    let Some(pairs) = table_at(&root, "contrast", shown, &at, violations) else {
        return;
    };
    for (key, _) in pairs.entries() {
        if !CONTRAST_KEYS.contains(&key.name.as_str()) {
            violations.push(format!(
                "{}: `contrast.{}` is not a key of `[contrast]`",
                at(key.span),
                key_part(&key.name)
            ));
        }
    }
    let rows = names(
        pairs,
        "text",
        "contrast.text",
        &palette,
        shown,
        &at,
        violations,
    );
    let columns = names(pairs, "on", "contrast.on", &palette, shown, &at, violations);
    let uses = uses(pairs, &palette, &place, violations);

    let name = root
        .get("name")
        .and_then(Value::as_str)
        .map_or(Cow::Borrowed("a theme with no name"), quoted);
    if let (Some(rows), Some(columns)) = (rows, columns) {
        report.notes.push(format!(
            "{shown} ({name}): contrast of each text color on each surface, a translucent one \
             composited over `bg`:"
        ));
        report.notes.extend(
            table(&rows, &columns, &palette, bg)
                .lines()
                .map(str::to_owned),
        );
    }
    let Some(uses) = uses else {
        return;
    };
    let mut below = 0;
    for pair in &uses {
        let (Some(text_color), Some(surface), Some(under)) = (
            color_of(&palette, &pair.text),
            color_of(&palette, &pair.on),
            color_of(&palette, &pair.over),
        ) else {
            continue;
        };
        if surface.value.a != u8::MAX && under.value.a != u8::MAX {
            below += 1;
            report.violations.push(format!(
                "{shown}:{}: `{}` is translucent, and composited over `{}`, which is \
                 translucent too",
                pair.line,
                quoted(&pair.on),
                quoted(&pair.over)
            ));
            continue;
        }
        let surface = surface.over(&pair.over, under.value);
        let ratio = contrast(composite(text_color.value, surface), surface);
        if ratio < pair.min {
            below += 1;
            let over = if pair.over == "bg" {
                String::new()
            } else {
                format!(" over `{}`", quoted(&pair.over))
            };
            report.violations.push(format!(
                "{shown}:{}: `{}` on `{}`{over} is {}:1, below the {}:1 that {} needs",
                pair.line,
                quoted(&pair.text),
                quoted(&pair.on),
                truncated(ratio),
                pair.min,
                pair.size
            ));
        }
    }
    report.notes.push(format!(
        "{shown}: {} pairs used for text, {} at or above the contrast their size needs",
        uses.len(),
        uses.len() - below
    ));
}

/// The table at `name` in the file's root, or `None` with a violation.
fn table_at<'t>(
    root: &'t Table,
    name: &str,
    shown: &str,
    at: &dyn Fn(Span) -> String,
    violations: &mut Vec<String>,
) -> Option<&'t Table> {
    match root.get(name) {
        Some(value) => {
            let table = value.as_table();
            if table.is_none() {
                violations.push(format!("{}: `{name}` is not a table", at(value.span)));
            }
            table
        }
        None => {
            let why = if name == "contrast" {
                ": a theme in themes/ lists the pairs it uses for text (ADR-0047)"
            } else {
                ""
            };
            violations.push(format!("{shown}: there is no `[{name}]` table{why}"));
            None
        }
    }
}

/// The colors of `[colors]`, each `None` when its entry does not read,
/// with a violation for that.
fn palette(colors: &Table, at: &dyn Fn(Span) -> String, violations: &mut Vec<String>) -> Palette {
    let mut palette = Palette::new();
    for (key, value) in colors.entries() {
        let path = format!("colors.{}", key_part(&key.name));
        let entry = match value.as_table() {
            None => {
                violations.push(format!("{}: `{path}` is not a table", at(value.span)));
                None
            }
            Some(table) => match table.get("value") {
                None => {
                    violations.push(format!("{}: `{path}` has no `value`", at(value.span)));
                    None
                }
                Some(color) => {
                    color_at(color, &format!("{path}.value"), at, violations).map(|color| Entry {
                        value: color,
                        solid: table
                            .get("solid")
                            .and_then(Value::as_str)
                            .and_then(|solid| Color::parse(solid).ok())
                            .filter(|solid| solid.a == u8::MAX),
                    })
                }
            },
        };
        palette.insert(key.name.clone(), entry);
    }
    palette
}

/// The color `value` holds, or `None` with a violation.
fn color_at(
    value: &Value,
    path: &str,
    at: &dyn Fn(Span) -> String,
    violations: &mut Vec<String>,
) -> Option<Color> {
    let Some(text) = value.as_str() else {
        violations.push(format!("{}: `{path}` is not a string", at(value.span)));
        return None;
    };
    match Color::parse(text) {
        Ok(color) => Some(color),
        Err(error) => {
            violations.push(format!(
                "{}: `{path}` is not a color: {}",
                at(value.span),
                quoted(&error.to_string())
            ));
            None
        }
    }
}

/// Checks each color's `solid`: given only for a translucent value,
/// opaque, and within a level a channel of the value over `bg`.
fn check_solids(
    colors: &Table,
    palette: &Palette,
    bg: Color,
    at: &dyn Fn(Span) -> String,
    violations: &mut Vec<String>,
) {
    for (key, value) in colors.entries() {
        let (Some(entry), Some(solid)) = (
            color_of(palette, &key.name),
            value.as_table().and_then(|table| table.get("solid")),
        ) else {
            continue;
        };
        let path = format!("colors.{}.solid", key_part(&key.name));
        let Some(color) = color_at(solid, &path, at, violations) else {
            continue;
        };
        let line = at(solid.span);
        if entry.value.a == u8::MAX {
            violations.push(format!(
                "{line}: `{path}` is given for an opaque value; only a translucent one has a \
                 solid color"
            ));
        } else if color.a != u8::MAX {
            violations.push(format!("{line}: `{path}` should be opaque"));
        } else {
            let made = composite(entry.value, bg);
            let near = |a: u8, b: u8| a.abs_diff(b) <= 1;
            if !(near(color.r, made.r) && near(color.g, made.g) && near(color.b, made.b)) {
                violations.push(format!(
                    "{line}: `{path}` is {color}, but the value over `bg` is {made}: more than \
                     one level a channel apart"
                ));
            }
        }
    }
}

/// The names in the array at `key` of `table`, shown as `path`, each a
/// color of `palette`; `None` with violations when it is not that.
fn names(
    table: &Table,
    key: &str,
    path: &str,
    palette: &Palette,
    shown: &str,
    at: &dyn Fn(Span) -> String,
    violations: &mut Vec<String>,
) -> Option<Vec<String>> {
    let Some(value) = table.get(key) else {
        violations.push(format!("{shown}: `{path}` is missing"));
        return None;
    };
    let Some(array) = value.as_array() else {
        violations.push(format!("{}: `{path}` is not an array", at(value.span)));
        return None;
    };
    if array.is_empty() {
        violations.push(format!("{}: `{path}` names no color", at(value.span)));
        return None;
    }
    let mut out = Vec::new();
    let mut ok = true;
    for item in array.items() {
        match item.as_str() {
            Some(name) if color_of(palette, name).is_some() => out.push(name.to_owned()),
            // A color whose entry does not read has its violation.
            Some(name) if palette.contains_key(name) => ok = false,
            Some(name) => {
                ok = false;
                violations.push(format!(
                    "{}: `{path}` names `{}`, which is not a color of `[colors]`",
                    at(item.span),
                    quoted(name)
                ));
            }
            None => {
                ok = false;
                violations.push(format!(
                    "{}: `{path}` holds something other than a color's name",
                    at(item.span)
                ));
            }
        }
    }
    ok.then_some(out)
}

/// The pairs of `[[contrast.use]]`; `None` with violations when there are
/// none, or an entry breaks a rule.
fn uses(
    contrast: &Table,
    palette: &Palette,
    place: &Place<'_>,
    violations: &mut Vec<String>,
) -> Option<Vec<Use>> {
    let shown = place.shown;
    let at = &|span: Span| place.at(span);
    let Some(value) = contrast.get("use") else {
        violations.push(format!(
            "{shown}: there is no `[[contrast.use]]`, so no pair used for text was checked"
        ));
        return None;
    };
    let Some(array) = value.as_array().filter(|array| !array.is_empty()) else {
        violations.push(format!(
            "{}: `contrast.use` is not an array of tables, one for each text color's uses",
            at(value.span)
        ));
        return None;
    };
    let before = violations.len();
    let mut pairs = Vec::new();
    for item in array.items() {
        let Some(entry) = item.as_table() else {
            violations.push(format!(
                "{}: an entry of `contrast.use` is not a table",
                at(item.span)
            ));
            continue;
        };
        for (key, _) in entry.entries() {
            if !USE_KEYS.contains(&key.name.as_str()) {
                violations.push(format!(
                    "{}: `{}` is not a key of `[[contrast.use]]`",
                    at(key.span),
                    key_part(&key.name)
                ));
            }
        }
        let text = one_name(entry, "text", palette, item.span, place, violations);
        let over = match entry.get("over") {
            None => Some(("bg".to_owned(), 0)),
            Some(_) => one_name(entry, "over", palette, item.span, place, violations),
        };
        let on = names(
            entry,
            "on",
            "contrast.use.on",
            palette,
            &at(item.span),
            at,
            violations,
        );
        let min = match entry.get("size") {
            Some(size) => match size.as_str() {
                Some("normal") => Some((NORMAL, "normal text")),
                Some("large") => Some((LARGE, "large text")),
                _ => {
                    violations.push(format!(
                        "{}: `size` is `normal` (4.5:1) or `large` (3:1)",
                        at(size.span)
                    ));
                    None
                }
            },
            None => {
                violations.push(format!(
                    "{}: an entry of `contrast.use` has no `size`",
                    at(item.span)
                ));
                None
            }
        };
        if let (Some((text, line)), Some(on), Some((over, _)), Some((min, size))) =
            (text, on, over, min)
        {
            for surface in on {
                pairs.push(Use {
                    text: text.clone(),
                    on: surface,
                    over: over.clone(),
                    min,
                    size,
                    line,
                });
            }
        }
    }
    (violations.len() == before).then_some(pairs)
}

/// The name at `key` of a `[[contrast.use]]` entry, a color of `palette`,
/// with the line of its value.
fn one_name(
    entry: &Table,
    key: &str,
    palette: &Palette,
    entry_span: Span,
    place: &Place<'_>,
    violations: &mut Vec<String>,
) -> Option<(String, usize)> {
    let Some(value) = entry.get(key) else {
        violations.push(format!(
            "{}: an entry of `contrast.use` has no `{key}`",
            place.at(entry_span)
        ));
        return None;
    };
    match value.as_str() {
        Some(name) if color_of(palette, name).is_some() => {
            Some((name.to_owned(), place.line(value.span)))
        }
        // A color whose entry does not read has its violation.
        Some(name) if palette.contains_key(name) => None,
        Some(name) => {
            violations.push(format!(
                "{}: `{key}` names `{}`, which is not a color of `[colors]`",
                place.at(value.span),
                quoted(name)
            ));
            None
        }
        None => {
            violations.push(format!("{}: `{key}` is not a string", place.at(value.span)));
            None
        }
    }
}

/// The table of `rows` on `columns`, in Markdown, as the spec writes it.
fn table(rows: &[String], columns: &[String], palette: &Palette, bg: Color) -> String {
    // `check` draws the table only when every row and column reads.
    let entry = |name: &String| {
        color_of(palette, name).unwrap_or(Entry {
            value: Color::BLACK,
            solid: None,
        })
    };
    let color = |name: &String| entry(name).value;
    let mut out = String::from("| Text token |");
    for column in columns {
        let solid = if color(column).a == u8::MAX {
            ""
        } else {
            " (solid)"
        };
        out.push_str(&format!(" on `{}`{solid} |", quoted(column)));
    }
    out.push_str("\n|");
    out.push_str(&" :- |".repeat(columns.len() + 1));
    for row in rows {
        out.push_str(&format!("\n| `{}` |", quoted(row)));
        for column in columns {
            let surface = entry(column).over("bg", bg);
            let ratio = contrast(composite(color(row), surface), surface);
            out.push_str(&format!(" {} |", cell(ratio)));
        }
    }
    out.push('\n');
    out
}

/// A cell of the table: the ratio to two decimals, marked when it is
/// enough only for large text, or not even for that.
fn cell(ratio: f64) -> String {
    let shown = format!("{ratio:.2}");
    if ratio >= NORMAL {
        shown
    } else if ratio >= LARGE {
        format!("**{shown} (large only)**")
    } else {
        format!("**{shown} (fails)**")
    }
}

/// `ratio` cut, not rounded, to two decimals, so that one below a minimum
/// never shows as the minimum.
fn truncated(ratio: f64) -> String {
    format!("{:.2}", (ratio * 100.0).floor() / 100.0)
}

/// `top` composited over the opaque `below` by `core`'s source-over, as
/// `gui` blends it; `top` itself when it is opaque.
fn composite(top: Color, below: Color) -> Color {
    if top.a == u8::MAX {
        return top;
    }
    top.premultiply().over(below.premultiply()).unpremultiply()
}

/// The WCAG 2.x contrast ratio of two opaque colors, from 1 to 21.
pub(crate) fn contrast(a: Color, b: Color) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    let (lighter, darker) = if a >= b { (a, b) } else { (b, a) };
    (lighter + 0.05) / (darker + 0.05)
}

/// The relative luminance of an opaque sRGB color, from 0 to 1.
fn luminance(color: Color) -> f64 {
    let linear = |channel: u8| {
        let c = f64::from(channel) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
}

/// A key's name as a part of a TOML dotted key, escaped as [`quoted`]
/// escapes it: quoted when it is not a bare key (ASCII letters, digits, `-`
/// and `_`), so that a quoted `"teal.value"` under `[colors]` shows as
/// `colors."teal.value"`, not as a key of the format.
fn key_part(name: &str) -> String {
    let bare = !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    if bare {
        return name.to_owned();
    }
    let mut part = String::from("\"");
    for c in name.chars() {
        if matches!(c, '"' | '\\') {
            part.push('\\');
        }
        part.push(c);
    }
    part.push('"');
    quoted(&part).into_owned()
}

/// `text` from the file, with every character `xtask` escapes written as
/// its escape, newlines and tabs too, so that a violation stays one line.
fn quoted(text: &str) -> Cow<'_, str> {
    walk::escape_controls(text, |_| false)
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;
    use crate::walk::testing::{TempTree, symlink};

    /// The repository's theme.
    const THEME: &str = include_str!("../../themes/tidepool-dark.toml");

    /// The line of byte `offset` of `text`, from 1, counted afresh.
    fn line_of(text: &str, offset: usize) -> usize {
        text[..offset].matches('\n').count() + 1
    }

    /// The violations and notes of the check on `text` as one theme.
    fn checked(text: &str) -> Report {
        let mut report = Report::default();
        check("themes/t.toml", text, &mut report);
        report
    }

    /// The repository's theme with `from` replaced by `to`, which must be
    /// there.
    fn edited(from: &str, to: &str) -> String {
        assert!(THEME.contains(from), "the theme has {from:?}");
        THEME.replacen(from, to, 1)
    }

    /// The line of the first `needle` in the repository's theme.
    fn line(needle: &str) -> usize {
        line_of(THEME, THEME.find(needle).expect("the theme has it"))
    }

    #[test]
    fn the_workspaces_theme_passes_and_prints_the_specs_table() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("the workspace");
        let report = run(root).expect("the theme reads");
        assert_eq!(report.violations, Vec::<String>::new());
        let spec = fs::read_to_string(root.join("docs/subsystems/ui-and-theme.md")).unwrap();
        let spec_table: Vec<&str> = spec
            .split("## Contrast")
            .nth(1)
            .unwrap()
            .lines()
            .skip_while(|line| !line.starts_with("| Text token |"))
            .take_while(|line| line.starts_with('|'))
            .collect();
        assert_eq!(spec_table.len(), 9, "{spec_table:?}");
        let printed: Vec<&str> = report
            .notes
            .iter()
            .map(String::as_str)
            .filter(|note| note.starts_with('|'))
            .collect();
        assert_eq!(printed, spec_table);
        assert_eq!(
            report.notes.first().map(String::as_str),
            Some(
                "themes/tidepool-dark.toml (tidepool-dark): contrast of each text color on each \
                 surface, a translucent one composited over `bg`:"
            )
        );
        assert_eq!(
            report.notes.last().map(String::as_str),
            Some(
                "themes/tidepool-dark.toml: 70 pairs used for text, 70 at or above the contrast \
                 their size needs"
            )
        );
    }

    #[test]
    fn a_token_change_that_breaks_aa_fails() {
        let text = edited(
            "text-3 = { value = \"#7d898e\", ansi = 102 }",
            "text-3 = { value = \"#5f6a6e\", ansi = 102 }",
        );
        let report = checked(&text);
        let at = line("text = \"text-3\"");
        let ghost = line("text = \"text-3\"\non = [\"ghost\"]");
        let bar = line("text = \"text-3\"\non = [\"code-bar\"]");
        let bar_ratio = "2.97";
        assert_eq!(
            report.violations,
            [
                format!(
                    "themes/t.toml:{at}: `text-3` on `bg` is 3.51:1, below the 4.5:1 that normal \
                     text needs"
                ),
                format!(
                    "themes/t.toml:{at}: `text-3` on `editor-bg` is 3.46:1, below the 4.5:1 that \
                     normal text needs"
                ),
                format!(
                    "themes/t.toml:{at}: `text-3` on `panel` is 3.39:1, below the 4.5:1 that \
                     normal text needs"
                ),
                // A status dot that is off, in the shell's status bar (M3.4).
                format!(
                    "themes/t.toml:{at}: `text-3` on `bar` is 3.02:1, below the 4.5:1 that \
                     normal text needs"
                ),
                // A card's kicker, and a code block's language (ADR-0052).
                format!(
                    "themes/t.toml:{ghost}: `text-3` on `ghost` is 3.34:1, below the 4.5:1 that \
                     normal text needs"
                ),
                format!(
                    "themes/t.toml:{bar}: `text-3` on `code-bar` over `editor-bg` is {bar_ratio}:1, \
                     below the 4.5:1 that normal text needs"
                ),
            ]
        );
        assert!(
            report.notes.contains(
                &"| `text-3` | **3.52 (large only)** | **3.46 (large only)** | **3.40 \
                            (large only)** | **2.27 (fails)** |"
                    .to_string()
            ),
            "{:?}",
            report.notes
        );
    }

    #[test]
    fn a_pair_is_compared_unrounded() {
        // `comment` of #607f8a is 4.498:1 on `editor-bg`: the table shows
        // 4.50, marked large only, and the check fails it, showing 4.49.
        let text = edited(
            "comment = { value = \"#6f7d82\" }",
            "comment = { value = \"#607f8a\" }",
        );
        let report = checked(&text);
        let at = line("text = \"comment\"");
        assert_eq!(
            report.violations,
            [format!(
                "themes/t.toml:{at}: `comment` on `editor-bg` is 4.49:1, below the 4.5:1 that \
                 normal text needs"
            )]
        );
        assert!(
            report.notes.contains(
                &"| `comment` | 4.57 | **4.50 (large only)** | **4.41 (large only)** | **2.94 \
                  (fails)** |"
                    .to_string()
            ),
            "{:?}",
            report.notes
        );
    }

    #[test]
    fn a_pair_used_for_large_text_needs_three_to_one() {
        // `text-3` on `slate` is 3.51:1: enough for large text only.
        let large = edited(
            "on = [\"bg\", \"editor-bg\", \"panel\", \"bar\"]\nsize = \"normal\"",
            "on = [\"bg\", \"editor-bg\", \"panel\", \"bar\"]\nsize = \"normal\"\n\n[[contrast.use]]\n\
             text = \"text-3\"\non = [\"slate\"]\nsize = \"large\"",
        );
        assert_eq!(checked(&large).violations, Vec::<String>::new());
        let normal = large.replacen(
            "on = [\"slate\"]\nsize = \"large\"",
            "on = [\"slate\"]\nsize = \"normal\"",
            1,
        );
        let at = line_of(
            &normal,
            normal.find("text = \"text-3\"\non = [\"slate\"]").unwrap(),
        );
        assert_eq!(
            checked(&normal).violations,
            [format!(
                "themes/t.toml:{at}: `text-3` on `slate` is 3.50:1, below the 4.5:1 that normal \
                 text needs"
            )]
        );
        // The comment color on `slate` fails even large text.
        let comment = large.replacen(
            "text = \"text-3\"\non = [\"slate\"]",
            "text = \"comment\"\non = [\"slate\"]",
            1,
        );
        let at = line_of(
            &comment,
            comment
                .find("text = \"comment\"\non = [\"slate\"]")
                .unwrap(),
        );
        assert_eq!(
            checked(&comment).violations,
            [format!(
                "themes/t.toml:{at}: `comment` on `slate` is 2.96:1, below the 3:1 that large \
                 text needs"
            )]
        );
    }

    #[test]
    fn the_selection_is_composited_over_editor_bg() {
        // `teal-glow` over `editor-bg` is #275152, where `text` is 7.81:1.
        let bg = Color::rgb(0x0b, 0x0c, 0x0d);
        let editor_bg = Color::rgb(0x0a, 0x0f, 0x11);
        let glow = Color::rgba(92, 203, 203, 89);
        let selection = composite(glow, editor_bg);
        assert_eq!(selection, Color::rgb(0x27, 0x51, 0x52));
        assert_eq!(
            truncated(contrast(Color::rgb(0xee, 0xf2, 0xf3), selection)),
            "7.81"
        );
        // At 0.56 (143 of 255), `text` on it is 4.45:1 over `editor-bg`,
        // below 4.5:1, and would be 4.56:1 over `bg`: only compositing
        // over `editor-bg` fails it.
        let text = edited(
            "teal-glow = { value = \"rgba(92,203,203,0.35)\" }",
            "teal-glow = { value = \"rgba(92,203,203,0.56)\" }",
        );
        let at = line("on = [\"teal-glow\"]") - 1;
        // The controls' pairs on `teal-glow` (ADR-0049) fail at 0.56 too: a
        // pressed teal button's label over `bg`, and a field's selection
        // over `slate`; and of the collections' (ADR-0052), a selected tree
        // item's chevron, a glyph, where a selected row's text holds at
        // 4.56:1 over `bg`.
        let pressed = line("text = \"teal-hi\"");
        let field = line("over = \"slate\"") - 2;
        let chevron = line("on = [\"row-hover\", \"teal-glow\"]\nsize = \"large\"") - 1;
        assert_eq!(
            checked(&text).violations,
            [
                format!(
                    "themes/t.toml:{at}: `text` on `teal-glow` over `editor-bg` is 4.45:1, below \
                     the 4.5:1 that normal text needs"
                ),
                format!(
                    "themes/t.toml:{pressed}: `teal-hi` on `teal-glow` is 3.55:1, below the 4.5:1 \
                     that normal text needs"
                ),
                format!(
                    "themes/t.toml:{field}: `text` on `teal-glow` over `slate` is 3.57:1, below \
                     the 4.5:1 that normal text needs"
                ),
                format!(
                    "themes/t.toml:{chevron}: `text-2` on `teal-glow` is 2.68:1, below the 3:1 \
                     that large text needs"
                ),
            ]
        );
        let glow = Color::rgba(92, 203, 203, 143);
        assert!(contrast(Color::rgb(0xee, 0xf2, 0xf3), composite(glow, bg)) > NORMAL);
        // A translucent surface that gives a solid value is composited over
        // anything but `bg`: `comment` on `panel` over `editor-bg` is
        // 4.42:1, where on `panel`'s solid it would be 4.44:1.
        let text = format!(
            "{THEME}\n[[contrast.use]]\ntext = \"comment\"\non = [\"panel\"]\nover = \
             \"editor-bg\"\nsize = \"normal\"\n"
        );
        let comment = line_of(&text, text.rfind("text = \"comment\"").unwrap());
        assert_eq!(
            checked(&text).violations,
            [format!(
                "themes/t.toml:{comment}: `comment` on `panel` over `editor-bg` is 4.42:1, below \
                 the 4.5:1 that normal text needs"
            )]
        );
        // Over a translucent color, a surface has no one color.
        let text = edited("over = \"editor-bg\"", "over = \"line\"");
        assert_eq!(
            checked(&text).violations,
            [format!(
                "themes/t.toml:{at}: `teal-glow` is translucent, and composited over `line`, \
                 which is translucent too"
            )]
        );
    }

    #[test]
    fn solid_values_are_checked() {
        let panel = line("solid = \"#0f1113\"");
        let cases = [
            (
                edited("solid = \"#0f1113\"", "solid = \"#111315\""),
                format!(
                    "themes/t.toml:{panel}: `colors.panel.solid` is #111315, but the value over \
                     `bg` is #0e1213: more than one level a channel apart"
                ),
            ),
            // Two levels from `core`'s #0e1213 in red: one more than allowed.
            (
                edited("solid = \"#0f1113\"", "solid = \"#101213\""),
                format!(
                    "themes/t.toml:{panel}: `colors.panel.solid` is #101213, but the value over \
                     `bg` is #0e1213: more than one level a channel apart"
                ),
            ),
            (
                edited("solid = \"#0f1113\"", "solid = \"#0f111380\""),
                format!("themes/t.toml:{panel}: `colors.panel.solid` should be opaque"),
            ),
            (
                edited(
                    "slate = { value = \"#29353a\", ansi = 236 }",
                    "slate = { value = \"#29353a\", solid = \"#29353a\" }",
                ),
                format!(
                    "themes/t.toml:{}: `colors.slate.solid` is given for an opaque value; only a \
                     translucent one has a solid color",
                    line("slate = {")
                ),
            ),
            (
                edited("solid = \"#0f1113\"", "solid = \"#0f11\""),
                format!(
                    "themes/t.toml:{panel}: `colors.panel.solid` is not a color: invalid color at \
                     byte 5: expected 6 or 8 hex digits"
                ),
            ),
        ];
        for (text, violation) in cases {
            assert_eq!(checked(&text).violations, [violation]);
        }
        // The spec's solid values of `panel` and `line` are each one level
        // from `core`'s composite in a channel or two.
        let bg = Color::rgb(0x0b, 0x0c, 0x0d);
        assert_eq!(
            composite(Color::rgba(16, 19, 21, 199), bg),
            Color::rgb(0x0e, 0x12, 0x13)
        );
        assert_eq!(
            composite(Color::rgba(255, 255, 255, 20), bg),
            Color::rgb(0x1e, 0x1f, 0x20)
        );
    }

    #[test]
    fn each_rule_of_the_table_fails() {
        let contrast = THEME
            .find("\n[contrast]\n")
            .expect("the theme has the table");
        let without = &THEME[..contrast];
        // `[contrast]` with its two lists and no `use`.
        let heading = "\n[contrast]\ntext = [\"text\"]\non = [\"bg\"]\n";
        let cases: Vec<(String, Vec<String>)> = vec![
            (
                without.to_string(),
                vec![
                    "themes/t.toml: there is no `[contrast]` table: a theme in themes/ lists the \
                     pairs it uses for text (ADR-0047)"
                        .into(),
                ],
            ),
            (
                edited("\"error\", \"comment\"]", "\"error\", \"gray\"]"),
                vec![format!(
                    "themes/t.toml:{}: `contrast.text` names `gray`, which is not a color of \
                     `[colors]`",
                    line("\"error\", \"comment\"]")
                )],
            ),
            (
                edited(
                    "on = [\"bg\", \"editor-bg\", \"panel\", \"slate\"]\n\n",
                    "on = []\n\n",
                ),
                vec![format!(
                    "themes/t.toml:{}: `contrast.on` names no color",
                    line("on = [\"bg\", \"editor-bg\", \"panel\", \"slate\"]\n\n")
                )],
            ),
            (
                edited("text = \"on-teal\"", "text = \"on-tael\""),
                vec![format!(
                    "themes/t.toml:{}: `text` names `on-tael`, which is not a color of \
                     `[colors]`",
                    line("text = \"on-teal\"")
                )],
            ),
            (
                edited("on = [\"teal\"]", "on = [\"teal\", 7]"),
                vec![format!(
                    "themes/t.toml:{}: `contrast.use.on` holds something other than a color's \
                     name",
                    line("on = [\"teal\"]")
                )],
            ),
            (
                edited("over = \"editor-bg\"", "over = \"editor\""),
                vec![format!(
                    "themes/t.toml:{}: `over` names `editor`, which is not a color of \
                     `[colors]`",
                    line("over = \"editor-bg\"")
                )],
            ),
            (
                edited(
                    "text = \"on-teal\"\non = [\"teal\"]\nsize = \"normal\"",
                    "text = \"on-teal\"\non = [\"teal\"]\nsize = \"small\"",
                ),
                vec![format!(
                    "themes/t.toml:{}: `size` is `normal` (4.5:1) or `large` (3:1)",
                    line("text = \"on-teal\"") + 2
                )],
            ),
            (
                edited(
                    "text = \"on-teal\"\non = [\"teal\"]\nsize = \"normal\"",
                    "text = \"on-teal\"\non = [\"teal\"]\nshade = 1\nsize = \"normal\"",
                ),
                vec![format!(
                    "themes/t.toml:{}: `shade` is not a key of `[[contrast.use]]`",
                    line("text = \"on-teal\"") + 2
                )],
            ),
            (
                edited("[contrast]\n", "[contrast]\nrows = 1\n"),
                vec![format!(
                    "themes/t.toml:{}: `contrast.rows` is not a key of `[contrast]`",
                    line("[contrast]\n") + 1
                )],
            ),
            // A theme with no `[colors]`, and with `colors` a number.
            (
                edited("\n[colors]\n", "\n[shades]\n"),
                vec!["themes/t.toml: there is no `[colors]` table".into()],
            ),
            (
                format!("colors = 1\n{}", edited("\n[colors]\n", "\n[shades]\n")),
                vec!["themes/t.toml:1: `colors` is not a table".into()],
            ),
            // `contrast` a number, before the table it would be.
            (
                format!("contrast = 1\n{without}"),
                vec!["themes/t.toml:1: `contrast` is not a table".into()],
            ),
            // `[contrast]` without its `use`, with an empty one, and with
            // one whose entry is not a table.
            (
                format!("{without}{heading}"),
                vec![
                    "themes/t.toml: there is no `[[contrast.use]]`, so no pair used for text was \
                     checked"
                        .into(),
                ],
            ),
            (
                format!("{without}{heading}use = []\n"),
                vec![format!(
                    "themes/t.toml:{}: `contrast.use` is not an array of tables, one for each text \
                     color's uses",
                    line_of(
                        &format!("{without}{heading}"),
                        without.len() + heading.len()
                    )
                )],
            ),
            (
                format!("{without}{heading}use = [1]\n"),
                vec![format!(
                    "themes/t.toml:{}: an entry of `contrast.use` is not a table",
                    line_of(
                        &format!("{without}{heading}"),
                        without.len() + heading.len()
                    )
                )],
            ),
            // `[contrast]`'s own lists: missing, and not an array.
            (
                edited(
                    "[contrast]\ntext = [\"text\", \"text-2\", \"text-3\", \"teal\", \"amber\", \
                     \"error\", \"comment\"]\n",
                    "[contrast]\n",
                ),
                vec!["themes/t.toml: `contrast.text` is missing".into()],
            ),
            (
                edited(
                    "on = [\"bg\", \"editor-bg\", \"panel\", \"slate\"]\n\n#",
                    "on = \"bg\"\n\n#",
                ),
                vec![format!(
                    "themes/t.toml:{}: `contrast.on` is not an array",
                    line("on = [\"bg\", \"editor-bg\", \"panel\", \"slate\"]\n\n#")
                )],
            ),
            // An entry of `use` without its `text`, its `on` or its `size`:
            // each is named at the entry's header.
            (
                edited("text = \"on-teal\"\n", ""),
                vec![format!(
                    "themes/t.toml:{}: an entry of `contrast.use` has no `text`",
                    line("text = \"on-teal\"") - 1
                )],
            ),
            (
                edited(
                    "text = \"on-teal\"\non = [\"teal\"]\n",
                    "text = \"on-teal\"\n",
                ),
                vec![format!(
                    "themes/t.toml:{}: `contrast.use.on` is missing",
                    line("text = \"on-teal\"") - 1
                )],
            ),
            (
                edited(
                    "text = \"on-teal\"\non = [\"teal\"]\nsize = \"normal\"\n",
                    "text = \"on-teal\"\non = [\"teal\"]\n",
                ),
                vec![format!(
                    "themes/t.toml:{}: an entry of `contrast.use` has no `size`",
                    line("text = \"on-teal\"") - 1
                )],
            ),
            // A key that is not a bare key is quoted in the path, so that
            // it is not taken for the format's `colors.teal.value`.
            (
                edited(
                    "teal = { value = \"#5ccbcb\", ansi = 80 }\n",
                    "teal = { value = \"#5ccbcb\", ansi = 80 }\n\"teal.value\" = 1\n",
                ),
                vec![format!(
                    "themes/t.toml:{}: `colors.\"teal.value\"` is not a table",
                    line("teal = { value = \"#5ccbcb\"") + 1
                )],
            ),
            (
                edited(
                    "bg = { value = \"#0b0c0d\", ansi = 232 }",
                    "bg = { value = \"#0b0c0d80\" }",
                ),
                vec![
                    "themes/t.toml: `colors.bg` is translucent, though translucent colors are \
                     composited over it"
                        .into(),
                ],
            ),
            (
                edited("bg = { value = \"#0b0c0d\", ansi = 232 }\n", ""),
                vec![
                    "themes/t.toml: `colors.bg` is missing, though translucent colors are \
                     composited over it"
                        .into(),
                ],
            ),
            (
                edited(
                    "green = { value = \"#3ddc97\", ansi = 42 }",
                    "green = \"#3ddc97\"",
                ),
                vec![format!(
                    "themes/t.toml:{}: `colors.green` is not a table",
                    line("green = {")
                )],
            ),
            (
                edited(
                    "green = { value = \"#3ddc97\", ansi = 42 }",
                    "green = { ansi = 42 }",
                ),
                vec![format!(
                    "themes/t.toml:{}: `colors.green` has no `value`",
                    line("green = {")
                )],
            ),
        ];
        for (text, expected) in cases {
            let violations = checked(&text).violations;
            assert_eq!(violations, expected);
        }
    }

    #[test]
    fn a_color_that_does_not_read_has_one_violation_and_no_others() {
        // Not `colors.bg` missing as well.
        let text = edited(
            "bg = { value = \"#0b0c0d\", ansi = 232 }",
            "bg = { value = \"#0b0c0g\", ansi = 232 }",
        );
        assert_eq!(
            checked(&text).violations,
            [format!(
                "themes/t.toml:{}: `colors.bg.value` is not a color: invalid color at byte 6: \
                 expected 6 or 8 hex digits",
                line("bg = { value = \"#0b0c0d\"")
            )]
        );
        // Nor `text-2` not a color of `[colors]` where `[contrast]` names
        // it; the table, which needs every row, is left out.
        let text = edited(
            "text-2 = { value = \"#b3bdc1\", ansi = 250 }",
            "text-2 = { value = \"#b3bdcz\", ansi = 250 }",
        );
        let report = checked(&text);
        assert_eq!(
            report.violations,
            [format!(
                "themes/t.toml:{}: `colors.text-2.value` is not a color: invalid color at byte 6: \
                 expected 6 or 8 hex digits",
                line("text-2 = {")
            )]
        );
        assert!(!report.notes.iter().any(|note| note.starts_with('|')));
    }

    #[test]
    fn many_violations_take_time_in_the_violations_not_the_file_squared() {
        // As many entries of `use` as the reader's limit of 1,024 leaves
        // room for, nearly 1,000, with an unknown key each, in a file of
        // about 60 KiB: each violation names its line from a table of
        // line starts, made once.
        // The entries' headers, not the file's comment that names them.
        let room = LIMITS.max_entries
            - THEME
                .lines()
                .filter(|line| *line == "[[contrast.use]]")
                .count();
        assert!(room >= 990, "{room}");
        let mut text = THEME.to_string();
        for _ in 0..room {
            text.push_str(
                "\n[[contrast.use]]\ntext = \"text\"\non = [\"bg\"]\nsize = \"normal\"\nx = 1\n",
            );
        }
        let report = checked(&text);
        assert_eq!(report.violations.len(), room);
        let last = text.rfind("x = 1").unwrap();
        assert_eq!(
            report.violations.last().map(String::as_str),
            Some(
                format!(
                    "themes/t.toml:{}: `x` is not a key of `[[contrast.use]]`",
                    line_of(&text, last)
                )
                .as_str()
            )
        );
    }

    #[test]
    fn what_a_violation_quotes_of_the_file_is_escaped() {
        let text = edited(
            "\"error\", \"comment\"]",
            "\"error\", \"a\\u001b[31m\\nb\"]",
        );
        let violations = checked(&text).violations;
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert!(
            violations[0].contains("names `a\\u{1b}[31m\\nb`, which"),
            "{violations:?}"
        );
    }

    #[test]
    fn the_directory_and_its_files_are_checked() {
        let tree = TempTree::new("theme-dir");
        let only = |tree: &TempTree| run(tree.path()).unwrap().violations;
        assert_eq!(
            only(&tree),
            ["themes/ was not found, so no theme was checked: is this the repository root?"]
        );
        tree.write("themes", "a file");
        assert_eq!(
            only(&tree),
            ["themes is not a directory, so no theme was checked"]
        );
        fs::remove_file(tree.path().join("themes")).unwrap();
        fs::create_dir(tree.path().join("themes")).unwrap();
        tree.write("themes/README.md", "not a theme");
        fs::create_dir(tree.path().join("themes/dir.toml")).unwrap();
        assert_eq!(
            only(&tree),
            ["themes/ holds no theme (a `.toml` file), so none was checked"]
        );
        tree.write("themes/a.toml", THEME);
        tree.write("themes/b.toml", "version = ");
        let report = run(tree.path()).unwrap();
        assert_eq!(report.violations.len(), 1, "{:?}", report.violations);
        assert!(
            report.violations[0].starts_with("themes/b.toml cannot be parsed: "),
            "{:?}",
            report.violations
        );
        assert!(
            report
                .notes
                .iter()
                .any(|note| note.starts_with("themes/a.toml (tidepool-dark)"))
        );
        fs::remove_file(tree.path().join("themes/b.toml")).unwrap();
        match symlink(&tree, "themes/c.toml", &tree.path().join("themes/a.toml")) {
            Ok(()) => assert_eq!(
                only(&tree),
                [
                    "themes/c.toml is a symbolic link, which `theme` does not follow, so it was not \
                  checked"
                ]
            ),
            Err(reason) => {
                let _ = writeln!(
                    io::stderr(),
                    "SKIPPED: the_directory_and_its_files_are_checked: cannot make a link here: \
                     {reason}"
                );
            }
        }
    }

    #[test]
    fn contrast_follows_wcag() {
        assert_eq!(contrast(Color::WHITE, Color::BLACK), 21.0);
        assert_eq!(contrast(Color::BLACK, Color::WHITE), 21.0);
        assert_eq!(contrast(Color::rgb(9, 9, 9), Color::rgb(9, 9, 9)), 1.0);
        // #777777 on white is the well-known near miss of 4.48:1.
        assert_eq!(
            cell(contrast(Color::rgb(0x77, 0x77, 0x77), Color::WHITE)),
            "**4.48 (large only)**"
        );
        assert_eq!(cell(4.5), "4.50");
        assert_eq!(cell(3.0), "**3.00 (large only)**");
        assert_eq!(cell(2.999), "**3.00 (fails)**");
        assert_eq!(truncated(4.4999), "4.49");
        assert!(is_theme_name("a.toml"));
        assert!(!is_theme_name(".toml"));
        assert!(!is_theme_name("a.TOML"));
        assert!(!is_theme_name("a.toml.bak"));
    }
}
