//! The dependency matrix, read from `docs/01-architecture.md` at run time
//! so that the document stays the source of truth.
//!
//! New for Tidepool. (Nadir's xtask kept the matrix in code and compared it
//! with the document in a test.)
//!
//! The matrix is the first table after the `## Dependency matrix` heading,
//! up to the next heading. Its header row must be
//! `| Crate | Allowed workspace dependencies |`, then the delimiter row,
//! then one row per crate:
//!
//! - **Crate:** the package name in backticks, optionally followed by a
//!   one-word alias in parentheses: `` `tidepool` (app) ``.
//! - **Allowed:** one of
//!   - `—` (or `-`): no workspace crate;
//!   - `all crates above`: every crate in an earlier row;
//!   - `any workspace crate`: every workspace member;
//!   - a comma-separated list of names in backticks, each a package name,
//!     a package name without its `tidepool-` prefix, or an alias, then
//!     optionally the note `(used only as a dev-dependency)`, which marks
//!     the row's own crate as one that others may use only from
//!     `[dev-dependencies]`.
//!
//! Anything else is an error that names the line: a document change the
//! parser does not understand must not quietly loosen the check. Line
//! endings may be LF or CRLF.
//!
//! Only what a reader of the rendered document sees counts: lines inside
//! (or touched by) an HTML comment `<!-- -->`, lines of fenced code blocks
//! (opened and closed as CommonMark does, with at most three columns of
//! indentation) and lines of raw HTML blocks (`<script>`, `<?...?>` and the
//! rest) are ignored when looking for the heading, the section's end and
//! the table. So that a table nobody sees cannot stand in for the one they
//! do, the document may have only one header row of the matrix's shape
//! anywhere (comments, code and HTML included, with or without outer
//! pipes), and the section only one table. The document may hold no raw
//! HTML but comments, as a block or inline, outside code spans and fenced
//! code blocks (an indented code block is read as text): a browser can hide what follows an element (`<script>`, `<div
//! hidden>`), past where Markdown sees the element end. And the section
//! may have no `|` outside the table, so that a table written without
//! outer pipes, which renders, cannot sit beside the one the parser reads.

use std::collections::BTreeSet;

use crate::error::Error;

/// The document, relative to the repository root.
pub(crate) const DOC: &str = "docs/01-architecture.md";

/// The heading of the matrix's section.
const HEADING: &str = "## Dependency matrix";

/// The header row's cells.
const HEADER: [&str; 2] = ["Crate", "Allowed workspace dependencies"];

/// The note that marks a crate as used only as a dev-dependency.
const DEV_ONLY_NOTE: &str = "used only as a dev-dependency";

/// What one crate may depend on, among workspace crates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Allowed {
    /// These package names and no others.
    Only(BTreeSet<String>),
    /// Any member of the workspace.
    AnyWorkspaceCrate,
}

/// One row of the matrix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    /// The package name.
    pub(crate) name: String,
    /// The line of the document the row is on, counted from 1.
    pub(crate) line: usize,
    /// What the crate may depend on.
    pub(crate) allowed: Allowed,
    /// The allowed cell as written, for messages.
    pub(crate) written: String,
    /// Whether other crates may use this one only as a dev-dependency.
    pub(crate) dev_only: bool,
}

/// The parsed matrix, in document order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Matrix {
    /// The rows.
    pub(crate) rows: Vec<Row>,
}

impl Matrix {
    /// The row for `name`, if there is one.
    pub(crate) fn row(&self, name: &str) -> Option<&Row> {
        self.rows.iter().find(|row| row.name == name)
    }
}

impl Row {
    /// Whether this crate may depend on the workspace member `name`.
    pub(crate) fn allows(&self, name: &str) -> bool {
        match &self.allowed {
            Allowed::Only(names) => names.contains(name),
            Allowed::AnyWorkspaceCrate => true,
        }
    }
}

/// A row before its short names are resolved.
struct RawRow {
    name: String,
    alias: Option<String>,
    line: usize,
    cell: Cell,
    written: String,
    dev_only: bool,
}

/// An allowed cell before its short names are resolved.
enum Cell {
    None,
    AllAbove,
    Any,
    Names(Vec<String>),
}

/// Parses the matrix out of the text of [`DOC`].
///
/// # Errors
///
/// [`Error::Doc`] naming the line if the section, the table or a row is not
/// in the shape the module documentation describes.
pub(crate) fn parse(text: &str) -> Result<Matrix, Error> {
    let lines: Vec<&str> = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect();
    let error = |line: usize, detail: String| Error::Doc {
        path: DOC.to_string(),
        line,
        detail,
    };
    let classified = classify(&lines);
    if let Some(index) = classified
        .iter()
        .position(|(kind, text)| *kind == Kind::Html || has_raw_html(text))
    {
        return Err(error(
            index + 1,
            "raw HTML, which a browser may hide along with what follows it: outside code, the \
             document that holds the matrix may have no HTML but comments"
                .to_string(),
        ));
    }
    let kinds: Vec<Kind> = classified.into_iter().map(|(kind, _)| kind).collect();
    let seen = |index: usize| kinds.get(index) == Some(&Kind::Shown);
    // `index` counts from 0, lines from 1.
    let table_line =
        |index: usize| seen(index) && lines.get(index).is_some_and(|l| l.starts_with('|'));

    let headers: Vec<usize> = (0..lines.len())
        .filter(|&index| lines.get(index).is_some_and(|line| is_header_row(line)))
        .collect();
    if let Some(&second) = headers.get(1) {
        return Err(error(
            second + 1,
            format!(
                "a second `| {} | {} |` header row (the first is on line {}): the document must \
                 have one dependency matrix",
                HEADER[0],
                HEADER[1],
                headers[0] + 1
            ),
        ));
    }

    let headings: Vec<usize> = (0..lines.len())
        .filter(|&index| seen(index) && lines.get(index).is_some_and(|l| l.trim_end() == HEADING))
        .collect();
    let heading = *headings
        .first()
        .ok_or_else(|| error(0, format!("no `{HEADING}` heading")))?;
    if let Some(&second) = headings.get(1) {
        return Err(error(second + 1, format!("a second `{HEADING}` heading")));
    }
    // The section ends at the next heading of the same or a higher level.
    let section_end = (heading + 1..lines.len())
        .find(|&index| {
            seen(index)
                && lines
                    .get(index)
                    .is_some_and(|line| line.starts_with("# ") || line.starts_with("## "))
        })
        .unwrap_or(lines.len());
    let table_start = (heading + 1..section_end)
        .find(|&index| table_line(index))
        // `heading` counts from 0, lines from 1: this is the heading's line.
        .ok_or_else(|| error(heading + 1, format!("no table under `{HEADING}`")))?;
    let table_end = (table_start..section_end)
        .find(|&index| !table_line(index))
        .unwrap_or(section_end);
    if let Some(other) = (table_end..section_end).find(|&index| table_line(index)) {
        return Err(error(
            other + 1,
            format!("a second table under `{HEADING}`: the matrix must be the only one"),
        ));
    }
    if let Some(index) = (heading + 1..section_end).find(|&index| {
        seen(index)
            && !(table_start..table_end).contains(&index)
            && lines.get(index).is_some_and(|line| line.contains('|'))
    }) {
        return Err(error(
            index + 1,
            format!(
                "a `|` outside the matrix table under `{HEADING}`: a table written without outer \
                 pipes would show there but not be read"
            ),
        ));
    }

    let table = lines.get(table_start..table_end).unwrap_or(&[]);
    let mut rows_text = table.iter().enumerate().map(|(offset, line)| {
        let number = table_start + offset + 1;
        (number, cells(line))
    });
    match rows_text.next() {
        Some((_, Some(header))) if header == HEADER => {}
        Some((number, _)) => {
            return Err(error(
                number,
                format!(
                    "the matrix header must be `| {} | {} |`",
                    HEADER[0], HEADER[1]
                ),
            ));
        }
        None => return Err(error(table_start + 1, "empty table".to_string())),
    }
    match rows_text.next() {
        Some((_, Some(delimiter))) if delimiter.iter().all(|cell| is_delimiter_cell(cell)) => {}
        _ => {
            return Err(error(
                table_start + 2,
                "expected the delimiter row `| :- | :- |` after the header".to_string(),
            ));
        }
    }

    let mut raw = Vec::new();
    for (number, row) in rows_text {
        let Some([crate_cell, allowed_cell]) = row else {
            return Err(error(
                number,
                "expected two cells: crate and allowed".to_string(),
            ));
        };
        let (name, alias) = crate_name(crate_cell).map_err(|detail| error(number, detail))?;
        if raw.iter().any(|row: &RawRow| row.name == name) {
            return Err(error(number, format!("a second row for `{name}`")));
        }
        let (cell, dev_only) = allowed(allowed_cell).map_err(|detail| error(number, detail))?;
        raw.push(RawRow {
            name,
            alias,
            line: number,
            cell,
            written: allowed_cell.to_string(),
            dev_only,
        });
    }
    if raw.is_empty() {
        return Err(error(table_start + 1, "the matrix has no rows".to_string()));
    }
    resolve(&raw).map_err(|(line, detail)| error(line, detail))
}

/// What a line of the document is to a reader of the rendered document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Text they see.
    Shown,
    /// A line of a fenced code block, its fences included.
    Fenced,
    /// A line that an HTML comment covers or touches.
    Comment,
    /// A line of a raw HTML block, which a renderer passes through (and a
    /// browser may hide) rather than shows as text.
    Html,
}

/// How a raw HTML block ends (CommonMark, "HTML blocks").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HtmlEnd {
    /// On the line that contains this text, in any case.
    Marker(&'static str),
    /// Before the next blank line.
    Blank,
}

/// For each line, what it is to a reader of the rendered document
/// ([`Kind`]), and its text outside HTML comments (none for code and HTML
/// blocks). Where this reading and a renderer could differ, it errs towards
/// hiding a line: the parser then sees less than the reader, never more,
/// and the one-header-row rule turns any difference into an error.
fn classify(lines: &[&str]) -> Vec<(Kind, String)> {
    let mut kinds = Vec::with_capacity(lines.len());
    let mut in_comment = false;
    // The fence character and how many of it opened the block.
    let mut fence: Option<(char, usize)> = None;
    let mut html: Option<HtmlEnd> = None;
    for line in lines {
        if let Some((c, length)) = fence {
            if closes_fence(line, c, length) {
                fence = None;
            }
            kinds.push((Kind::Fenced, String::new()));
            continue;
        }
        match html {
            Some(HtmlEnd::Blank) if line.trim().is_empty() => html = None,
            Some(HtmlEnd::Blank) => {
                kinds.push((Kind::Html, String::new()));
                continue;
            }
            Some(HtmlEnd::Marker(marker)) => {
                if contains_ignoring_case(line, marker) {
                    html = None;
                }
                kinds.push((Kind::Html, String::new()));
                continue;
            }
            None => {}
        }
        if !in_comment {
            if let Some(opened) = opens_fence(line) {
                fence = Some(opened);
                kinds.push((Kind::Fenced, String::new()));
                continue;
            }
            if let Some(end) = opens_html_block(line) {
                let ends_here = match end {
                    HtmlEnd::Marker(marker) => contains_ignoring_case(line, marker),
                    HtmlEnd::Blank => false,
                };
                html = (!ends_here).then_some(end);
                kinds.push((Kind::Html, String::new()));
                continue;
            }
        }
        let outside = outside_comments(line, in_comment);
        let (touched, still_open) = comment_state(line, in_comment);
        in_comment = still_open;
        kinds.push((if touched { Kind::Comment } else { Kind::Shown }, outside));
    }
    kinds
}

/// For each line, whether [`classify`] says a reader sees it as text.
#[cfg(test)]
fn visible(lines: &[&str]) -> Vec<bool> {
    classify(lines)
        .into_iter()
        .map(|(kind, _)| kind == Kind::Shown)
        .collect()
}

/// The parts of `line`, which starts inside an HTML comment if
/// `in_comment`, that no comment covers.
fn outside_comments(line: &str, mut in_comment: bool) -> String {
    let mut kept = String::new();
    let mut rest = line;
    loop {
        let marker = if in_comment { "-->" } else { "<!--" };
        let Some(at) = rest.find(marker) else {
            if !in_comment {
                kept.push_str(rest);
            }
            return kept;
        };
        if !in_comment {
            kept.push_str(rest.get(..at).unwrap_or(""));
        }
        in_comment = !in_comment;
        rest = rest.get(at + marker.len()..).unwrap_or("");
    }
}

/// Whether `text`, outside its code spans, has raw inline HTML: `<` (not
/// escaped with `\\`) followed by a tag name and a space, `>`, `/` or the
/// end, by `/` and a tag name, by `?`, or by `!`. An autolink
/// (`<https://...>`, `<a@b.c>`) is not HTML. A code span that does not
/// close on its line is read as text.
fn has_raw_html(text: &str) -> bool {
    let text = without_code_spans(text);
    text.match_indices('<').any(|(at, _)| {
        let escapes = text
            .get(..at)
            .unwrap_or("")
            .chars()
            .rev()
            .take_while(|&c| c == '\\')
            .count();
        if escapes % 2 == 1 {
            return false;
        }
        let after = text.get(at + 1..).unwrap_or("");
        let after = after.strip_prefix('/').unwrap_or(after);
        if after.starts_with(['?', '!']) {
            return true;
        }
        let name = after
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
            .count();
        let starts_with_letter = after.starts_with(|c: char| c.is_ascii_alphabetic());
        starts_with_letter
            && after.get(name..).is_some_and(|rest| {
                rest.chars()
                    .next()
                    .is_none_or(|c| c == '>' || c == '/' || c.is_whitespace())
            })
    })
}

/// `text` with every code span (a run of backticks, up to the next run of
/// as many) removed.
fn without_code_spans(text: &str) -> String {
    let mut kept = String::new();
    let mut rest = text;
    while let Some(at) = rest.find('`') {
        kept.push_str(rest.get(..at).unwrap_or(""));
        let from = rest.get(at..).unwrap_or("");
        let run = from.chars().take_while(|&c| c == '`').count();
        let after = from.get(run..).unwrap_or("");
        match closing_run(after, run) {
            Some(end) => rest = after.get(end..).unwrap_or(""),
            None => {
                kept.push_str(from.get(..run).unwrap_or(""));
                rest = after;
            }
        }
    }
    kept.push_str(rest);
    kept
}

/// Where the first run of exactly `run` backticks in `text` ends.
fn closing_run(text: &str, run: usize) -> Option<usize> {
    let mut at = 0;
    while let Some(found) = text.get(at..)?.find('`') {
        let start = at + found;
        let length = text.get(start..)?.chars().take_while(|&c| c == '`').count();
        if length == run {
            return Some(start + length);
        }
        at = start + length;
    }
    None
}

/// `line` without its indentation, and how many columns that is, a tab
/// reaching the next multiple of four.
fn indented(line: &str) -> (usize, &str) {
    let mut columns = 0;
    for (at, c) in line.char_indices() {
        match c {
            ' ' => columns += 1,
            '\t' => columns += 4 - columns % 4,
            _ => return (columns, line.get(at..).unwrap_or("")),
        }
    }
    (columns, "")
}

/// The fence character and length if `line` opens a fenced code block: at
/// most three columns of indentation, then three or more backticks or
/// tildes, and for backticks no backtick after them.
fn opens_fence(line: &str) -> Option<(char, usize)> {
    let (columns, rest) = indented(line);
    let c = rest.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let length = rest.chars().take_while(|&x| x == c).count();
    let info = rest.get(length..).unwrap_or("");
    (columns <= 3 && length >= 3 && !(c == '`' && info.contains('`'))).then_some((c, length))
}

/// Whether `line` closes a fenced code block opened with `length` of `c`:
/// at most three columns of indentation, at least as many of `c`, then only
/// spaces and tabs. A line indented further is code, not a fence.
fn closes_fence(line: &str, c: char, length: usize) -> bool {
    let (columns, rest) = indented(line);
    let run = rest.chars().take_while(|&x| x == c).count();
    columns <= 3
        && run >= length
        && rest
            .get(run..)
            .is_some_and(|after| after.chars().all(|x| x == ' ' || x == '\t'))
}

/// How the raw HTML block that `line` opens ends, if it opens one: any line
/// whose first character (after any indentation) is `<`, except an HTML
/// comment, which [`comment_state`] follows. This takes every such line for
/// an HTML block, including some a renderer would show as text.
fn opens_html_block(line: &str) -> Option<HtmlEnd> {
    let rest = line.trim_start().strip_prefix('<')?;
    if rest.starts_with("!--") {
        return None;
    }
    let lower = rest.to_ascii_lowercase();
    for (tag, end) in [
        ("script", "</script>"),
        ("pre", "</pre>"),
        ("style", "</style>"),
        ("textarea", "</textarea>"),
    ] {
        if let Some(after) = lower.strip_prefix(tag)
            && after
                .chars()
                .next()
                .is_none_or(|c| c == '>' || c.is_whitespace())
        {
            return Some(HtmlEnd::Marker(end));
        }
    }
    Some(if lower.starts_with('?') {
        HtmlEnd::Marker("?>")
    } else if lower.starts_with("![cdata[") {
        HtmlEnd::Marker("]]>")
    } else if lower
        .strip_prefix('!')
        .is_some_and(|after| after.starts_with(|c: char| c.is_ascii_alphabetic()))
    {
        HtmlEnd::Marker(">")
    } else {
        HtmlEnd::Blank
    })
}

/// Whether `line` contains `marker` (ASCII, lower case) in any case.
fn contains_ignoring_case(line: &str, marker: &str) -> bool {
    line.to_ascii_lowercase().contains(marker)
}

/// Whether `line` is a header row of the matrix's shape, written with outer
/// pipes or, as GFM also renders it, without them.
fn is_header_row(line: &str) -> bool {
    let inner = line.trim();
    let inner = inner.strip_prefix('|').unwrap_or(inner);
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    let cells: Vec<&str> = inner.split('|').map(str::trim).collect();
    cells == HEADER
}

/// Whether `cell` is a cell of a GFM delimiter row: dashes, with an
/// optional colon at either end.
fn is_delimiter_cell(cell: &str) -> bool {
    let dashes = cell.strip_prefix(':').unwrap_or(cell);
    let dashes = dashes.strip_suffix(':').unwrap_or(dashes);
    !dashes.is_empty() && dashes.chars().all(|c| c == '-')
}

/// Follows HTML comments through `line`, which starts inside one if
/// `in_comment`. Returns whether any part of the line is in a comment, and
/// whether one is still open at its end.
fn comment_state(line: &str, mut in_comment: bool) -> (bool, bool) {
    let mut touched = in_comment;
    let mut rest = line;
    loop {
        let marker = if in_comment { "-->" } else { "<!--" };
        let Some(at) = rest.find(marker) else {
            return (touched, in_comment);
        };
        touched = true;
        in_comment = !in_comment;
        rest = rest.get(at + marker.len()..).unwrap_or("");
    }
}

/// The cells of a table row, trimmed: `| a | b |` gives `["a", "b"]`.
/// `None` if the row does not start and end with `|`.
fn cells(line: &str) -> Option<[&str; 2]> {
    let inner = line.trim().strip_prefix('|')?.strip_suffix('|')?;
    let mut cells = inner.split('|').map(str::trim);
    match (cells.next(), cells.next(), cells.next()) {
        (Some(first), Some(second), None) => Some([first, second]),
        _ => None,
    }
}

/// Reads a crate cell: `` `name` `` or `` `name` (alias) ``.
fn crate_name(cell: &str) -> Result<(String, Option<String>), String> {
    let (name, rest) = backticked(cell)
        .ok_or_else(|| format!("expected a crate name in backticks, found `{cell}`"))?;
    let rest = rest.trim();
    if rest.is_empty() {
        return Ok((name.to_string(), None));
    }
    match rest
        .strip_prefix('(')
        .and_then(|rest| rest.strip_suffix(')'))
    {
        Some(alias) if is_name(alias) => Ok((name.to_string(), Some(alias.to_string()))),
        _ => Err(format!(
            "expected `` `{name}` `` or `` `{name}` (alias) ``, found `{cell}`"
        )),
    }
}

/// Reads an allowed cell, and whether it carries the dev-only note.
fn allowed(cell: &str) -> Result<(Cell, bool), String> {
    match cell {
        "—" | "-" => return Ok((Cell::None, false)),
        "all crates above" => return Ok((Cell::AllAbove, false)),
        "any workspace crate" => return Ok((Cell::Any, false)),
        _ => {}
    }
    let (list, dev_only) = match cell
        .strip_suffix(')')
        .and_then(|rest| rest.rsplit_once('('))
    {
        Some((list, DEV_ONLY_NOTE)) => (list.trim_end(), true),
        Some((_, note)) => return Err(format!("unrecognized note `({note})` in `{cell}`")),
        None => (cell, false),
    };
    let mut names = Vec::new();
    for item in list.split(',') {
        let item = item.trim();
        match backticked(item) {
            Some((name, "")) if is_name(name) => names.push(name.to_string()),
            _ => {
                return Err(format!(
                    "expected `—`, `all crates above`, `any workspace crate` or a list of \
                     crate names in backticks, found `{cell}`"
                ));
            }
        }
    }
    Ok((Cell::Names(names), dev_only))
}

/// Splits `` `name` rest `` into `name` and `rest`.
fn backticked(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix('`')?;
    let (name, rest) = rest.split_once('`')?;
    Some((name, rest))
}

/// Whether `text` can be a package name or alias.
fn is_name(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Resolves every short name to a package name.
fn resolve(raw: &[RawRow]) -> Result<Matrix, (usize, String)> {
    let lookup = |short: &str| -> Option<&str> {
        let prefixed = format!("tidepool-{short}");
        raw.iter()
            .find(|row| row.name == short)
            .or_else(|| raw.iter().find(|row| row.name == prefixed))
            .or_else(|| raw.iter().find(|row| row.alias.as_deref() == Some(short)))
            .map(|row| row.name.as_str())
    };
    let mut rows = Vec::with_capacity(raw.len());
    for (index, row) in raw.iter().enumerate() {
        let allowed = match &row.cell {
            Cell::None => Allowed::Only(BTreeSet::new()),
            Cell::AllAbove => Allowed::Only(
                raw.iter()
                    .take(index)
                    .map(|above| above.name.clone())
                    .collect(),
            ),
            Cell::Any => Allowed::AnyWorkspaceCrate,
            Cell::Names(names) => {
                let mut resolved = BTreeSet::new();
                for short in names {
                    let name = lookup(short).ok_or_else(|| {
                        (
                            row.line,
                            format!(
                                "`{short}` in the row for `{}` names no crate in the matrix",
                                row.name
                            ),
                        )
                    })?;
                    if name == row.name {
                        return Err((row.line, format!("`{}` lists itself", row.name)));
                    }
                    resolved.insert(name.to_string());
                }
                Allowed::Only(resolved)
            }
        };
        rows.push(Row {
            name: row.name.clone(),
            line: row.line,
            allowed,
            written: row.written.clone(),
            dev_only: row.dev_only,
        });
    }
    Ok(Matrix { rows })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    const SMALL: &str = "# Doc\n\n## Layers\n\n| Layer | Crates |\n| :- | :- |\n| 0 | `core` |\n\n\
        ## Dependency matrix\n\nSome words.\n\n\
        | Crate | Allowed workspace dependencies |\n\
        | :- | :- |\n\
        | `tidepool-core` | — |\n\
        | `tidepool-codec` | `core` |\n\
        | `tidepool` (app) | all crates above |\n\
        | `xtask` | `core`, `codec` |\n\
        | `tidepool-bench` | any workspace crate |\n\
        | `tidepool-testkit` | `core`, `codec` (used only as a dev-dependency) |\n\
        \nAfter the table.\n\n## Workspace layout\n\n| a | b |\n| - | - |\n| `x` | `y` |\n";

    fn names(items: &[&str]) -> Allowed {
        Allowed::Only(items.iter().map(|name| name.to_string()).collect())
    }

    fn detail(text: &str) -> (usize, String) {
        match parse(text) {
            Err(Error::Doc { path, line, detail }) => {
                assert_eq!(path, DOC);
                (line, detail)
            }
            other => panic!("expected a document error, got {other:?}"),
        }
    }

    #[test]
    fn reads_every_form_of_row() {
        let matrix = parse(SMALL).expect("the small matrix parses");
        let summary: Vec<(&str, usize, &Allowed, bool)> = matrix
            .rows
            .iter()
            .map(|row| (row.name.as_str(), row.line, &row.allowed, row.dev_only))
            .collect();
        assert_eq!(
            summary,
            [
                ("tidepool-core", 15, &names(&[]), false),
                ("tidepool-codec", 16, &names(&["tidepool-core"]), false),
                (
                    "tidepool",
                    17,
                    &names(&["tidepool-core", "tidepool-codec"]),
                    false
                ),
                (
                    "xtask",
                    18,
                    &names(&["tidepool-core", "tidepool-codec"]),
                    false
                ),
                ("tidepool-bench", 19, &Allowed::AnyWorkspaceCrate, false),
                (
                    "tidepool-testkit",
                    20,
                    &names(&["tidepool-core", "tidepool-codec"]),
                    true
                ),
            ]
        );
        assert_eq!(matrix.rows[3].written, "`core`, `codec`");
        let xtask = matrix.row("xtask").expect("xtask has a row");
        assert!(xtask.allows("tidepool-codec"));
        assert!(!xtask.allows("tidepool"));
        assert!(matrix.row("tidepool-bench").unwrap().allows("anything"));
        assert!(matrix.row("missing").is_none());
    }

    #[test]
    fn crlf_line_endings_give_the_same_matrix() {
        let crlf = SMALL.replace('\n', "\r\n");
        assert_eq!(parse(&crlf).unwrap(), parse(SMALL).unwrap());
    }

    #[test]
    fn aliases_resolve_and_unknown_names_are_errors() {
        let text = SMALL.replace(
            "| `tidepool-bench` | any workspace crate |",
            "| `tidepool-bench` | `app`, `xtask` |",
        );
        let matrix = parse(&text).unwrap();
        assert_eq!(
            matrix.row("tidepool-bench").unwrap().allowed,
            names(&["tidepool", "xtask"])
        );
        let text = SMALL.replace(
            "| `tidepool-codec` | `core` |",
            "| `tidepool-codec` | `cor` |",
        );
        assert_eq!(
            detail(&text),
            (
                16,
                "`cor` in the row for `tidepool-codec` names no crate in the matrix".to_string()
            )
        );
        let text = SMALL.replace(
            "| `tidepool-codec` | `core` |",
            "| `tidepool-codec` | `codec` |",
        );
        assert_eq!(
            detail(&text),
            (16, "`tidepool-codec` lists itself".to_string())
        );
    }

    #[test]
    fn a_change_the_parser_does_not_understand_is_an_error() {
        for (from, to, line) in [
            (
                "| `tidepool-core` | — |",
                "| `tidepool-core` | nothing |",
                15,
            ),
            ("| `tidepool-core` | — |", "| tidepool-core | — |", 15),
            (
                "| `tidepool-core` | — |",
                "| `tidepool-core` (x y) | — |",
                15,
            ),
            (
                "| `tidepool-core` | — |",
                "| `tidepool-core` | — | extra |",
                15,
            ),
            ("| `tidepool-core` | — |", "| `tidepool-core` |", 15),
            (
                "| `tidepool-codec` | `core` |",
                "| `tidepool-codec` | `core`; `x` |",
                16,
            ),
            (
                "| `tidepool-codec` | `core` |",
                "| `tidepool-codec` | `core`, |",
                16,
            ),
            (
                "| `tidepool-codec` | `core` |",
                "| `tidepool-codec` | `core` (only in tests) |",
                16,
            ),
            (
                "| `tidepool-codec` | `core` |",
                "| `tidepool-core` | `core` |",
                16,
            ),
            (
                "| :- | :- |\n| `tidepool-core`",
                "| x | y |\n| `tidepool-core`",
                14,
            ),
        ] {
            assert!(SMALL.contains(from), "{from}");
            let (found, message) = detail(&SMALL.replacen(from, to, 1));
            assert_eq!(found, line, "{to}: {message}");
        }
        assert_eq!(
            detail(&SMALL.replace("## Dependency matrix", "## Matrix")),
            (0, "no `## Dependency matrix` heading".to_string())
        );
        assert_eq!(
            detail(&SMALL.replace(
                "| Crate | Allowed workspace dependencies |",
                "| Crate | Allowed |"
            )),
            (
                13,
                "the matrix header must be `| Crate | Allowed workspace dependencies |`"
                    .to_string()
            )
        );
        let no_table = "## Dependency matrix\n\nwords\n\n## Next\n\n| a | b |\n";
        assert_eq!(
            detail(no_table),
            (1, "no table under `## Dependency matrix`".to_string())
        );
        let no_rows =
            "## Dependency matrix\n\n| Crate | Allowed workspace dependencies |\n| :- | :- |\n";
        assert_eq!(detail(no_rows), (3, "the matrix has no rows".to_string()));
    }

    #[test]
    fn a_table_nobody_sees_cannot_replace_the_matrix() {
        let permissive = "| Crate | Allowed workspace dependencies |\n| :- | :- |\n\
                          | `tidepool-core` | any workspace crate |\n";
        // In an HTML comment above the real table.
        let hidden = SMALL.replace(
            "Some words.\n",
            &format!("Some words.\n\n<!--\n{permissive}-->\n"),
        );
        assert_eq!(
            detail(&hidden),
            (
                19,
                "a second `| Crate | Allowed workspace dependencies |` header row (the first is on \
                 line 14): the document must have one dependency matrix"
                    .to_string()
            )
        );
        // In a fenced code block, or anywhere else in the document.
        for wrapped in [
            format!("```\n{permissive}```\n"),
            format!("~~~~markdown\n{permissive}~~~~\n"),
        ] {
            let text = SMALL.replace("Some words.\n", &format!("Some words.\n\n{wrapped}"));
            assert_eq!(detail(&text).0, 19, "{wrapped}");
            let text = format!("{SMALL}\n## Appendix\n\n{wrapped}");
            assert!(
                detail(&text).1.starts_with("a second `| Crate |"),
                "{wrapped}"
            );
        }
    }

    #[test]
    fn comments_and_code_blocks_are_not_read_as_the_document() {
        // A table without the matrix's header, hidden above the real one.
        let hidden_table = SMALL.replace(
            "Some words.\n",
            "Some words.\n<!-- | a | b |\n| :- | :- | -->\n",
        );
        let rows = |text: &str| -> Vec<(String, Allowed)> {
            parse(text)
                .unwrap()
                .rows
                .into_iter()
                .map(|row| (row.name, row.allowed))
                .collect()
        };
        assert_eq!(rows(&hidden_table), rows(SMALL));
        // A heading inside a code block does not end the section.
        let fenced_heading = SMALL.replace(
            "Some words.\n",
            "Some words.\n\n```text\n## Not a heading\n```\n",
        );
        assert_eq!(rows(&fenced_heading), rows(SMALL));
        // A heading in a comment is not the heading.
        let commented = SMALL.replace("## Dependency matrix", "<!-- ## Dependency matrix -->");
        assert_eq!(
            detail(&commented),
            (0, "no `## Dependency matrix` heading".to_string())
        );
        // A comment that touches a row ends the table there, and what
        // follows is a second table: an error, never a silent change.
        let split = SMALL.replace(
            "| `tidepool-codec` | `core` |",
            "| `tidepool-codec` | `core` | <!-- -->",
        );
        assert_eq!(
            detail(&split),
            (
                17,
                "a second table under `## Dependency matrix`: the matrix must be the only one"
                    .to_string()
            )
        );
    }

    #[test]
    fn a_second_heading_or_table_is_an_error() {
        let twice = format!("{SMALL}\n## Dependency matrix\n\nAgain.\n");
        assert_eq!(
            detail(&twice),
            (30, "a second `## Dependency matrix` heading".to_string())
        );
        let second_table =
            SMALL.replace("\nAfter the table.\n", "\nAfter.\n\n| a | b |\n| - | - |\n");
        assert_eq!(
            detail(&second_table).1,
            "a second table under `## Dependency matrix`: the matrix must be the only one"
        );
    }

    #[test]
    fn visibility_follows_comments_and_fences() {
        let lines = [
            "a",
            "<!-- one -->",
            "b <!-- open",
            "inside",
            "close --> c",
            "d",
            "```rust",
            "<!--",
            "```",
            "e",
            "````",
            "```",
            "````",
            "f",
        ];
        let shown: Vec<&str> = lines
            .iter()
            .zip(visible(&lines))
            .filter_map(|(line, seen)| seen.then_some(*line))
            .collect();
        assert_eq!(shown, ["a", "d", "e", "f"]);
    }

    /// The matrix table of [`SMALL`].
    fn small_table() -> &'static str {
        let start = SMALL.find("| Crate |").unwrap();
        let end = SMALL.find("\nAfter the table.").unwrap();
        &SMALL[start..end]
    }

    /// `table` written without outer pipes, as GFM also renders it.
    fn without_outer_pipes(table: &str) -> String {
        table
            .lines()
            .map(|line| {
                let line = line.strip_prefix("| ").unwrap_or(line);
                format!("{}\n", line.strip_suffix(" |").unwrap_or(line))
            })
            .collect()
    }

    #[test]
    fn a_hidden_table_behind_a_pipeless_one_is_an_error() {
        // Probe pM: the reader sees the strict matrix, written without outer
        // pipes; the parser would read a permissive copy in a processing
        // instruction, which renders as nothing.
        let permissive = small_table().replace(
            "| `tidepool-core` | — |",
            "| `tidepool-core` | any workspace crate |",
        );
        let shown = without_outer_pipes(small_table());
        let pm = SMALL.replace(small_table(), &format!("{shown}\n<?hidden\n{permissive}?>"));
        let (line, message) = detail(&pm);
        assert_eq!(line, 22, "{message}");
        assert!(
            message.starts_with("raw HTML, which a browser may hide"),
            "{message}"
        );
        // The same in a comment: two header rows, whether or not each has
        // outer pipes.
        let commented = SMALL.replace(small_table(), &format!("{shown}\n<!--\n{permissive}-->"));
        assert_eq!(
            detail(&commented),
            (
                23,
                "a second `| Crate | Allowed workspace dependencies |` header row (the first is on \
                 line 13): the document must have one dependency matrix"
                    .to_string()
            )
        );
        // Probe pN: after a code block whose closing fence is indented four
        // spaces, which CommonMark reads as code, so the block goes on.
        let pn = SMALL.replace(
            small_table(),
            &format!("{shown}\n```text\nnotes\n    ```\n{permissive}```"),
        );
        assert_eq!(detail(&pn).0, 25, "{pn}");
        assert!(detail(&pn).1.starts_with("a second `| Crate |"));
        // And on its own, a pipeless table beside the real one.
        let beside = SMALL.replace(
            "\nAfter the table.\n",
            "\nAfter.\n\n`tidepool-sys` | any workspace crate\n",
        );
        assert_eq!(
            detail(&beside),
            (
                24,
                "a `|` outside the matrix table under `## Dependency matrix`: a table written \
                 without outer pipes would show there but not be read"
                    .to_string()
            )
        );
    }

    /// The lines of `lines` that a reader sees as text.
    fn shown(lines: &[&str]) -> Vec<String> {
        lines
            .iter()
            .zip(visible(lines))
            .filter(|(_, seen)| *seen)
            .map(|(line, _)| (*line).to_string())
            .collect()
    }

    #[test]
    fn fences_open_and_close_as_commonmark_says() {
        // A closing fence indented four columns is code: the block goes on.
        assert_eq!(
            shown(&["```text", "    ```", "| hidden |", "```", "after"]),
            ["after"]
        );
        assert_eq!(
            shown(&["~~~", "\t~~~", "| hidden |", "~~~", "after"]),
            ["after"]
        );
        // Up to three spaces is a fence, and so is a longer closing run.
        assert_eq!(shown(&["   ```", "a", "   ````", "b"]), ["b"]);
        // A closing fence has nothing after it.
        assert_eq!(shown(&["```", "a", "``` x", "b", "```", "c"]), ["c"]);
        // Not openers: indented four columns (code), or backticks in a
        // backtick fence's info string (inline code).
        assert_eq!(shown(&["    ```", "x"]), ["    ```", "x"]);
        assert_eq!(shown(&["\t```", "x"]), ["\t```", "x"]);
        assert_eq!(shown(&["``` `x`", "y"]), ["``` `x`", "y"]);
    }

    #[test]
    fn html_blocks_are_hidden_and_any_raw_html_is_an_error() {
        assert_eq!(
            shown(&["<div hidden>", "| x |", "", "after"]),
            ["", "after"]
        );
        assert_eq!(shown(&["<SCRIPT>", "a", "", "b", "</script>", "c"]), ["c"]);
        assert_eq!(shown(&["<?x", "a", "?>", "b"]), ["b"]);
        assert_eq!(shown(&["<![CDATA[", "a", "]]>", "b"]), ["b"]);
        assert_eq!(shown(&["<!DOCTYPE", "a>", "b"]), ["b"]);
        assert_eq!(shown(&["<pre>x</pre>", "b"]), ["b"]);
        // Anywhere in the document, as a block or inline, raw HTML fails.
        for (from, to, line) in [
            ("Some words.", "<div hidden>", 11),
            ("Some words.", "Some <script> words", 11),
            ("Some words.", "Some words. <!-- c --> <span hidden>", 11),
            ("After the table.", "After </div> the table.", 22),
            ("# Doc", "# Doc <?x ?>", 1),
            (
                "| `tidepool-core` | — |",
                "| `tidepool-core` | — <b>x</b> |",
                15,
            ),
            // An even run of backslashes escapes nothing; `<!`, `<![CDATA[`
            // and a self-closing tag are HTML too; and a backtick that never
            // closes on its line is only a backtick.
            ("Some words.", "Some \\\\<b> words", 11),
            ("Some words.", "Some <!x> words", 11),
            ("Some words.", "Some <![CDATA[ x ]]> words", 11),
            ("Some words.", "Some <br/> words", 11),
            ("Some words.", "Some `a <span hidden> words", 11),
        ] {
            let text = SMALL.replacen(from, to, 1);
            let (found, message) = detail(&text);
            assert_eq!(found, line, "{to}: {message}");
            assert!(message.starts_with("raw HTML"), "{to}: {message}");
        }
        // Not HTML: code spans, autolinks, escapes, comparisons.
        for to in [
            "Some `<root>/target` words, and ``a <b> c``.",
            "See <https://example.com> or <a@example.com>.",
            "Some \\<b> words, and 1 < 2.",
            // A code span closes only at a run of as many backticks.
            "Some `a`` <span hidden> ` words.",
        ] {
            let text = SMALL.replacen("Some words.", to, 1);
            assert!(parse(&text).is_ok(), "{to}");
        }
    }

    #[test]
    fn a_heading_of_either_level_ends_the_section() {
        // The table after it is in another section, not a second table in
        // this one.
        for heading in ["## Workspace layout", "# Workspace layout"] {
            let text = SMALL.replacen("## Workspace layout", heading, 1);
            assert!(parse(&text).is_ok(), "{heading}");
        }
        // Without one, it would be.
        let text = SMALL.replacen("## Workspace layout", "Workspace layout", 1);
        assert_eq!(detail(&text).0, 26, "{text}");
    }

    #[test]
    fn the_delimiter_row_must_be_one_gfm_renders() {
        let header = "| Crate | Allowed workspace dependencies |\n";
        let with = |delimiter: &str| {
            SMALL.replacen(
                &format!("{header}| :- | :- |"),
                &format!("{header}{delimiter}"),
                1,
            )
        };
        assert_eq!(detail(&with("| : | : |")).0, 14);
        assert_eq!(detail(&with("| :-: | ::- |")).0, 14);
        for delimiter in ["| --- | :-: |", "| -: | :- |"] {
            assert!(parse(&with(delimiter)).is_ok(), "{delimiter}");
        }
    }

    #[test]
    fn the_real_document_parses_as_documented() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(DOC);
        let text = std::fs::read_to_string(path).expect("the architecture document exists");
        let matrix = parse(&text).expect("the real matrix parses");
        assert_eq!(matrix.rows.len(), 26);
        let row = |name: &str| {
            matrix
                .row(name)
                .unwrap_or_else(|| panic!("no row for {name}"))
        };
        assert_eq!(row("tidepool-core").allowed, names(&[]));
        assert_eq!(row("tidepool-sys").allowed, names(&[]));
        assert_eq!(
            row("xtask").allowed,
            names(&["tidepool-core", "tidepool-codec"])
        );
        assert_eq!(row("tidepool-bench").allowed, Allowed::AnyWorkspaceCrate);
        assert!(row("tidepool-testkit").dev_only);
        assert_eq!(
            matrix.rows.iter().filter(|row| row.dev_only).count(),
            1,
            "only testkit is dev-only"
        );
        // `tidepool` (app) may use every crate in the rows above it: the 22
        // library crates, not xtask, bench or testkit.
        let Allowed::Only(app) = &row("tidepool").allowed else {
            panic!("the app row is a list");
        };
        assert_eq!(app.len(), 22);
        assert!(app.contains("tidepool-tui") && app.contains("tidepool-sys"));
        assert!(!app.contains("tidepool-testkit") && !app.contains("xtask"));
    }
}
