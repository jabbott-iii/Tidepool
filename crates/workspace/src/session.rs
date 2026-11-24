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

//! Sessions: the editors a folder had open, how they were split, and each
//! one's selection and scroll, kept so that the folder opens as it was
//! left (M4.11, ADR-0068).
//!
//! New in Tidepool.
//!
//! # Where sessions live
//!
//! One file a folder, in [`DIR_NAME`], `sessions/` in the user data folder
//! (`platform::paths::user_dirs`, on Linux `$XDG_DATA_HOME/tidepool`),
//! never in the project: so `git status` shows nothing, and a repository
//! cannot ship a session. The file is named from the folder's canonical
//! path ([`file_name`]: its SHA-256, in hex). The file holds the path too,
//! and [`Session::load`] uses a session only when it names the folder it
//! was asked for ([`WorkspaceError::OtherFolder`]), so that two folders
//! never restore each other's.
//!
//! # The file
//!
//! TOML, which Tidepool writes ([`Session::to_text`]) and reads
//! ([`parse`]). The layout is a tree: a group of editors (tabs, one of them
//! shown), or a split of panes side by side (`"row"`) or stacked
//! (`"column"`), each pane holding a layout and its share of the space:
//!
//! ```toml
//! # The editors a folder had open (ADR-0068). Tidepool writes this file.
//! version = 1
//! folder = "/home/me/src/tidepool"
//! active_group = 1
//! layout = { split = "row", panes = [
//!     { weight = 1.0, layout = { active = 0, editors = [
//!         { path = "/home/me/src/tidepool/README.md", primary = 0,
//!           cursors = [[0, 0]], scroll = { line = 0, row = 0, y = 0.0, x = 0.0 } },
//!     ] } },
//!     { weight = 1.0, layout = { active = 0, editors = [] } },
//! ] }
//! ```
//!
//! (Tidepool writes the layout on one line; it is broken here to read.)
//!
//! Each editor names its file by absolute path, its cursors as pairs of
//! byte offsets, an anchor and a head, with the index of the primary one,
//! and where its view was scrolled: the line at the top, that line's row
//! with soft wrap on, and how far up and left it was scrolled, in units
//! (`editor::view::ScrollPosition`). Restoring checks the offsets against
//! the file as it is then ([`EditorState::selection_in`]).
//!
//! # Limits
//!
//! The file is read as untrusted input, since anything that can write the
//! user data folder can write it: at most [`MAX_FILE_BYTES`], splits at
//! most [`MAX_DEPTH`] deep of at most [`MAX_PANES`] panes, at most
//! [`MAX_GROUPS`] groups and [`MAX_EDITORS`] editors in all, at most
//! [`MAX_CURSORS`] cursors an editor, and paths of at most
//! [`MAX_PATH_BYTES`]. A file past any of them, or that is not a session,
//! is not used, and [`parse`] never panics; the fuzz target `session`
//! checks that (ADR-0027). A path that is not UTF-8 cannot be stored,
//! since TOML holds only text.
//!
//! Reading and writing the file blocks, so call [`Session::load`] and
//! [`Session::save`] on a worker.

use std::path::{Component, Path, PathBuf};

use tidepool_codec::Limits;
use tidepool_codec::toml::{Array, Document, Table, Value, ValueKind};
use tidepool_core::hash::{sha256, to_hex};
use tidepool_core::path;
use tidepool_text::{Cursor, Format, Rope, Selection};

use crate::WorkspaceError;
use crate::folder::Folder;
use crate::read::{self, Links};

/// The folder in the user data folder that holds the session files.
pub const DIR_NAME: &str = "sessions";

/// The session file's format version, which Tidepool writes and reads.
pub const VERSION: i64 = 1;

/// The largest session file read: 1 MiB.
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// How deep splits may nest: 8.
pub const MAX_DEPTH: usize = 8;

/// The most panes one split has: 16. It has at least 2.
pub const MAX_PANES: usize = 16;

/// The most groups of editors a session has: 64.
pub const MAX_GROUPS: usize = 64;

/// The most editors a session has, in all its groups: 1,024.
pub const MAX_EDITORS: usize = 1024;

/// The most cursors one editor keeps: 1,024. A selection with more is
/// stored with its primary cursor and the first others.
pub const MAX_CURSORS: usize = 1024;

/// The longest path stored, in bytes: 4,096, Linux's `PATH_MAX`.
pub const MAX_PATH_BYTES: usize = 4096;

/// The comment Tidepool writes at the top of the file.
const HEADER: &str = "# The editors a folder had open (ADR-0068). Tidepool writes this file.\n";

/// How the editors of a folder were laid out.
#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    /// The folder's canonical path.
    pub folder: PathBuf,
    /// The groups and splits.
    pub layout: Layout,
    /// The group that had focus, by its place among the layout's groups in
    /// [`Session::groups`]' order.
    pub active_group: usize,
}

/// A layout: one group of editors, or a split of panes.
#[derive(Clone, Debug, PartialEq)]
pub enum Layout {
    /// A group of editors, shown as tabs.
    Group(Group),
    /// Panes side by side or stacked.
    Split(Split),
}

/// Panes side by side or stacked.
#[derive(Clone, Debug, PartialEq)]
pub struct Split {
    /// Which way the panes are laid out.
    pub axis: Axis,
    /// The panes, from the left or the top: from 2 to [`MAX_PANES`].
    pub panes: Vec<Pane>,
}

/// Which way a [`Split`]'s panes are laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Axis {
    /// Side by side, `"row"` in the file.
    Row,
    /// Stacked, `"column"` in the file.
    Column,
}

/// One pane of a [`Split`].
#[derive(Clone, Debug, PartialEq)]
pub struct Pane {
    /// Its share of the split's space, against the other panes': finite
    /// and above 0.
    pub weight: f32,
    /// What it holds.
    pub layout: Layout,
}

/// A group of editors, shown as tabs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Group {
    /// The editors, in tab order. A group may be empty.
    pub editors: Vec<EditorState>,
    /// The editor shown, by its place in `editors`; 0 for an empty group.
    pub active: usize,
}

/// One editor: its file, its selection and where it was scrolled.
#[derive(Clone, Debug, PartialEq)]
pub struct EditorState {
    /// The file's absolute path.
    pub path: PathBuf,
    /// The selection, in byte offsets of the text as it was.
    pub selection: Selection,
    /// Where the view was scrolled.
    pub scroll: Scroll,
}

/// Where an editor's view was scrolled, as `editor::view::ScrollPosition`
/// has it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Scroll {
    /// The line at the top, from 0.
    pub line: usize,
    /// The row of that line at the top, from 0: 0 with soft wrap off.
    pub row: usize,
    /// How far that row was scrolled up, in units: finite, at least 0.
    pub y: f32,
    /// How far the text was scrolled left, in units: finite, at least 0.
    pub x: f32,
}

impl Session {
    /// A session of `folder` with one empty group.
    pub fn new(folder: &Folder) -> Self {
        Self {
            folder: folder.root().to_path_buf(),
            layout: Layout::Group(Group::default()),
            active_group: 0,
        }
    }

    /// The groups, depth first, from the left or the top of each split:
    /// the order [`Session::active_group`] counts in.
    pub fn groups(&self) -> Vec<&Group> {
        let mut groups = Vec::new();
        let mut stack = vec![&self.layout];
        while let Some(layout) = stack.pop() {
            match layout {
                Layout::Group(group) => groups.push(group),
                Layout::Split(split) => {
                    stack.extend(split.panes.iter().rev().map(|pane| &pane.layout));
                }
            }
        }
        groups
    }

    /// Reads the session of `folder` from `data_dir`, the user data folder,
    /// or `None` when it has none.
    ///
    /// It blocks on the file system, so call it on a worker.
    ///
    /// # Errors
    ///
    /// A file that cannot be used, which the caller logs before it starts
    /// without one: [`WorkspaceError::NotRegularFile`],
    /// [`WorkspaceError::Read`], or one of [`parse`]'s errors, and
    /// [`WorkspaceError::OtherFolder`] when it holds another folder's
    /// session.
    pub fn load(data_dir: &Path, folder: &Folder) -> Result<Option<Session>, WorkspaceError> {
        let file = path_in(data_dir, folder.root());
        let Some(bytes) = read::bounded(&file, MAX_FILE_BYTES, Links::Follow)? else {
            return Ok(None);
        };
        let session = parse(&bytes, &file)?;
        if session.folder != folder.root() {
            return Err(WorkspaceError::OtherFolder {
                path: file,
                folder: folder.root().to_path_buf(),
            });
        }
        Ok(Some(session))
    }

    /// Writes the session to its file in `data_dir`, the user data folder,
    /// making `sessions/` if needed, through a temporary file and a rename
    /// ([`tidepool_text::file::save`]).
    ///
    /// It blocks on the file system, so call it on a worker.
    ///
    /// # Errors
    ///
    /// [`Session::to_text`]'s; [`WorkspaceError::CreateFolder`] or
    /// [`WorkspaceError::Save`] when the file cannot be written.
    pub fn save(&self, data_dir: &Path) -> Result<(), WorkspaceError> {
        let text = self.to_text()?;
        let file = path_in(data_dir, &self.folder);
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|source| WorkspaceError::CreateFolder {
                path: dir.to_path_buf(),
                source,
            })?;
        }
        tidepool_text::file::save(&file, &Rope::from(text.as_str()), Format::UTF8).map_err(
            |source| WorkspaceError::Save {
                path: file.clone(),
                source,
            },
        )
    }

    /// The session file's text, once it reads back as this same session,
    /// so that a file the next load would refuse is never written. A
    /// selection with more than [`MAX_CURSORS`] cursors is stored with its
    /// primary cursor and the first others.
    ///
    /// # Errors
    ///
    /// [`WorkspaceError::Path`] for a path that is not UTF-8, and
    /// [`WorkspaceError::BadSessionFile`] for a session [`parse`] would
    /// refuse: past a limit, a path that is not absolute and free of `.`
    /// and `..`, a weight or scroll that is not finite, a split of fewer
    /// than 2 panes, or an index past what it counts.
    pub fn to_text(&self) -> Result<String, WorkspaceError> {
        let stored = self.capped();
        let edit = |source| WorkspaceError::Edit {
            path: PathBuf::from(DIR_NAME),
            source,
        };
        let mut document = Document::parse(HEADER, Limits::DEFAULT).map_err(edit)?;
        let folder = path::to_utf8(&stored.folder)?.to_string();
        let active = integer(stored.active_group)?;
        document
            .set(&["version".into()], &ValueKind::Integer(VERSION))
            .map_err(edit)?;
        document
            .set(&["folder".into()], &ValueKind::String(folder))
            .map_err(edit)?;
        document
            .set(&["active_group".into()], &ValueKind::Integer(active))
            .map_err(edit)?;
        document
            .set(
                &["layout".into()],
                &ValueKind::Table(layout_table(&stored.layout)?),
            )
            .map_err(edit)?;
        let text = document.into_string();
        let unchanged = parse(text.as_bytes(), Path::new(DIR_NAME)).ok();
        if unchanged.as_ref() != Some(&stored) {
            return Err(bad(
                Path::new(DIR_NAME),
                "it is past a limit, or would not read back as the same session",
            ));
        }
        Ok(text)
    }

    /// This session with each selection cut to [`MAX_CURSORS`].
    fn capped(&self) -> Session {
        let mut session = self.clone();
        let mut stack = vec![&mut session.layout];
        while let Some(layout) = stack.pop() {
            match layout {
                Layout::Group(group) => {
                    for editor in &mut group.editors {
                        editor.selection = capped(&editor.selection);
                    }
                }
                Layout::Split(split) => stack.extend(split.panes.iter_mut().map(|p| &mut p.layout)),
            }
        }
        session
    }
}

impl EditorState {
    /// The selection, if it fits `text` (the file as it is now), or else a
    /// caret at the start: a file changed since the session was saved can
    /// be shorter, or have other characters at the offsets.
    pub fn selection_in(&self, text: &Rope) -> Selection {
        match self.selection.validate(text) {
            Ok(()) => self.selection.clone(),
            Err(_) => Selection::caret(0),
        }
    }
}

/// The session file's name for the folder whose canonical path is
/// `folder`: the SHA-256 of the path's bytes, in hex, and `.toml`.
pub fn file_name(folder: &Path) -> String {
    format!(
        "{}.toml",
        to_hex(&sha256(folder.as_os_str().as_encoded_bytes()))
    )
}

/// The session file of the folder whose canonical path is `folder`, in the
/// user data folder `data_dir`: `sessions/` and its [`file_name`].
pub fn path_in(data_dir: &Path, folder: &Path) -> PathBuf {
    data_dir.join(DIR_NAME).join(file_name(folder))
}

/// The limits the file is parsed with: the file's size, nesting enough for
/// [`MAX_DEPTH`] splits, strings as long as [`MAX_PATH_BYTES`], numbers of
/// 64 bytes and [`MAX_EDITORS`] entries a table or array.
const LIMITS: Limits = {
    let mut limits = Limits::DEFAULT;
    limits.max_input_bytes = MAX_FILE_BYTES as usize;
    limits.max_depth = 64;
    limits.max_string_bytes = MAX_PATH_BYTES;
    limits.max_number_bytes = 64;
    limits.max_entries = MAX_EDITORS;
    limits
};

/// The session a session file's `bytes` hold, the file named `path` in
/// errors. It reads nothing, and never panics. Keys the format does not
/// name are ignored.
///
/// # Errors
///
/// [`WorkspaceError::TooLarge`] past [`MAX_FILE_BYTES`];
/// [`WorkspaceError::Toml`] for text that is not TOML, or past `codec`'s
/// limits; [`WorkspaceError::Version`] for a `version` other than
/// [`VERSION`], or none; [`WorkspaceError::BadSessionFile`] for anything
/// else that is not a session, past the [limits](self#limits) included.
pub fn parse(bytes: &[u8], path: &Path) -> Result<Session, WorkspaceError> {
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_FILE_BYTES {
        return Err(WorkspaceError::TooLarge {
            path: path.to_path_buf(),
            limit: MAX_FILE_BYTES,
        });
    }
    let root = tidepool_codec::toml::parse_bytes(bytes, LIMITS).map_err(|source| {
        WorkspaceError::Toml {
            path: path.to_path_buf(),
            source,
        }
    })?;
    if root.get("version").and_then(Value::as_integer) != Some(VERSION) {
        return Err(WorkspaceError::Version {
            path: path.to_path_buf(),
        });
    }
    let folder = root
        .get("folder")
        .and_then(Value::as_str)
        .ok_or_else(|| bad(path, "`folder` is missing, or is not a string"))?;
    let folder = plain_path(folder).ok_or_else(|| {
        bad(
            path,
            "`folder` is not an absolute path free of `.` and `..`",
        )
    })?;
    let layout = root
        .get("layout")
        .and_then(Value::as_table)
        .ok_or_else(|| bad(path, "`layout` is missing, or is not a table"))?;
    let mut count = Count::default();
    let layout = read_layout(layout, 0, &mut count, path)?;
    let active_group = root
        .get("active_group")
        .and_then(Value::as_integer)
        .and_then(|index| usize::try_from(index).ok())
        .filter(|&index| index < count.groups)
        .ok_or_else(|| bad(path, "`active_group` is not the index of a group"))?;
    Ok(Session {
        folder,
        layout,
        active_group,
    })
}

/// What a layout read so far holds, against the limits.
#[derive(Default)]
struct Count {
    groups: usize,
    editors: usize,
}

/// The layout a table holds, `depth` splits down.
fn read_layout(
    table: &Table,
    depth: usize,
    count: &mut Count,
    path: &Path,
) -> Result<Layout, WorkspaceError> {
    if table.get("editors").is_some() {
        return read_group(table, count, path).map(Layout::Group);
    }
    let axis = match table.get("split").and_then(Value::as_str) {
        Some("row") => Axis::Row,
        Some("column") => Axis::Column,
        _ => {
            return Err(bad(
                path,
                "a layout is neither a group with `editors` nor a `split` of \"row\" or \"column\"",
            ));
        }
    };
    if depth >= MAX_DEPTH {
        return Err(bad(path, "splits nest more than 8 deep"));
    }
    let items = table
        .get("panes")
        .and_then(Value::as_array)
        .ok_or_else(|| bad(path, "a split's `panes` is missing, or is not an array"))?;
    if !(2..=MAX_PANES).contains(&items.len()) {
        return Err(bad(path, "a split has fewer than 2 panes, or more than 16"));
    }
    let mut panes = Vec::with_capacity(items.len());
    for item in items.items() {
        let pane = item
            .as_table()
            .ok_or_else(|| bad(path, "a pane is not a table"))?;
        let weight = pane
            .get("weight")
            .and_then(number)
            .filter(|weight| weight.is_finite() && *weight > 0.0)
            .ok_or_else(|| bad(path, "a pane's `weight` is not a number above 0"))?;
        let layout = pane
            .get("layout")
            .and_then(Value::as_table)
            .ok_or_else(|| bad(path, "a pane's `layout` is missing, or is not a table"))?;
        panes.push(Pane {
            weight,
            layout: read_layout(layout, depth + 1, count, path)?,
        });
    }
    Ok(Layout::Split(Split { axis, panes }))
}

/// The group a table holds.
fn read_group(table: &Table, count: &mut Count, path: &Path) -> Result<Group, WorkspaceError> {
    count.groups += 1;
    if count.groups > MAX_GROUPS {
        return Err(bad(path, "the session has more than 64 groups"));
    }
    let items = table
        .get("editors")
        .and_then(Value::as_array)
        .ok_or_else(|| bad(path, "a group's `editors` is not an array"))?;
    count.editors = count.editors.saturating_add(items.len());
    if count.editors > MAX_EDITORS {
        return Err(bad(path, "the session has more than 1,024 editors"));
    }
    let mut editors = Vec::with_capacity(items.len());
    for item in items.items() {
        let editor = item
            .as_table()
            .ok_or_else(|| bad(path, "an editor is not a table"))?;
        editors.push(read_editor(editor, path)?);
    }
    let active = table
        .get("active")
        .and_then(Value::as_integer)
        .and_then(|index| usize::try_from(index).ok())
        .filter(|&index| index < editors.len().max(1))
        .ok_or_else(|| {
            bad(
                path,
                "a group's `active` is not the index of one of its editors",
            )
        })?;
    Ok(Group { editors, active })
}

/// The editor a table holds.
fn read_editor(table: &Table, path: &Path) -> Result<EditorState, WorkspaceError> {
    let file = table
        .get("path")
        .and_then(Value::as_str)
        .and_then(plain_path)
        .ok_or_else(|| {
            bad(
                path,
                "an editor's `path` is not an absolute path free of `.` and `..`",
            )
        })?;
    let items = table
        .get("cursors")
        .and_then(Value::as_array)
        .ok_or_else(|| bad(path, "an editor's `cursors` is missing, or is not an array"))?;
    if !(1..=MAX_CURSORS).contains(&items.len()) {
        return Err(bad(path, "an editor has no cursor, or more than 1,024"));
    }
    let mut cursors = Vec::with_capacity(items.len());
    for item in items.items() {
        let pair = item.as_array().filter(|pair| pair.len() == 2);
        let offset = |at| {
            pair.and_then(|pair| pair.get(at))
                .and_then(Value::as_integer)
                .and_then(|offset| usize::try_from(offset).ok())
        };
        let (Some(anchor), Some(head)) = (offset(0), offset(1)) else {
            return Err(bad(
                path,
                "a cursor is not a pair of byte offsets, an anchor and a head",
            ));
        };
        cursors.push(Cursor::new(anchor, head));
    }
    let primary = table
        .get("primary")
        .and_then(Value::as_integer)
        .and_then(|index| usize::try_from(index).ok())
        .filter(|&index| index < cursors.len())
        .ok_or_else(|| bad(path, "an editor's `primary` is not the index of a cursor"))?;
    let chosen = cursors.remove(primary);
    let selection = Selection::new(chosen, cursors);
    let scroll = table
        .get("scroll")
        .and_then(Value::as_table)
        .ok_or_else(|| bad(path, "an editor's `scroll` is missing, or is not a table"))?;
    let line = |name| {
        scroll
            .get(name)
            .and_then(Value::as_integer)
            .and_then(|value| usize::try_from(value).ok())
    };
    let units = |name| {
        scroll
            .get(name)
            .and_then(number)
            .filter(|value| value.is_finite() && *value >= 0.0)
    };
    let (Some(line), Some(row), Some(y), Some(x)) =
        (line("line"), line("row"), units("y"), units("x"))
    else {
        return Err(bad(
            path,
            "an editor's `scroll` does not hold a `line` and a `row` from 0, and finite `y` and \
             `x` from 0",
        ));
    };
    Ok(EditorState {
        path: file,
        selection,
        scroll: Scroll { line, row, y, x },
    })
}

/// A float or an integer, as an `f32`. One too large for an `f32` becomes
/// infinite, which the callers refuse.
fn number(value: &Value) -> Option<f32> {
    match value.kind {
        ValueKind::Float(float) => Some(float as f32),
        ValueKind::Integer(integer) => Some(integer as f32),
        _ => None,
    }
}

/// `text` as a path, if it is absolute, free of `.` and `..`, spelled as
/// its names join, and at most [`MAX_PATH_BYTES`] long.
fn plain_path(text: &str) -> Option<PathBuf> {
    let path = PathBuf::from(text);
    let plain = text.len() <= MAX_PATH_BYTES
        && path.is_absolute()
        && !path
            .components()
            .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
        && path.components().collect::<PathBuf>().as_os_str() == path.as_os_str();
    plain.then_some(path)
}

/// `selection` with at most [`MAX_CURSORS`] cursors: its primary one and
/// the first others.
fn capped(selection: &Selection) -> Selection {
    if selection.cursors().len() <= MAX_CURSORS {
        return selection.clone();
    }
    let primary = selection.primary_index();
    let others = selection
        .cursors()
        .iter()
        .enumerate()
        .filter(|&(at, _)| at != primary)
        .map(|(_, &cursor)| cursor)
        .take(MAX_CURSORS - 1);
    Selection::new(selection.primary(), others)
}

/// The table a layout is written as.
fn layout_table(layout: &Layout) -> Result<Table, WorkspaceError> {
    let mut table = Table::new_inline();
    match layout {
        Layout::Group(group) => {
            table.insert("active", ValueKind::Integer(integer(group.active)?));
            let mut editors = Vec::with_capacity(group.editors.len());
            for editor in &group.editors {
                editors.push(Value::from(ValueKind::Table(editor_table(editor)?)));
            }
            table.insert("editors", ValueKind::Array(Array::new(editors)));
        }
        Layout::Split(split) => {
            let axis = match split.axis {
                Axis::Row => "row",
                Axis::Column => "column",
            };
            table.insert("split", ValueKind::String(axis.to_string()));
            let mut panes = Vec::with_capacity(split.panes.len());
            for pane in &split.panes {
                let mut entry = Table::new_inline();
                entry.insert("weight", ValueKind::Float(f64::from(pane.weight)));
                entry.insert("layout", ValueKind::Table(layout_table(&pane.layout)?));
                panes.push(Value::from(ValueKind::Table(entry)));
            }
            table.insert("panes", ValueKind::Array(Array::new(panes)));
        }
    }
    Ok(table)
}

/// The table an editor is written as.
fn editor_table(editor: &EditorState) -> Result<Table, WorkspaceError> {
    let mut table = Table::new_inline();
    let file = path::to_utf8(&editor.path)?.to_string();
    table.insert("path", ValueKind::String(file));
    let primary = editor.selection.primary_index();
    table.insert("primary", ValueKind::Integer(integer(primary)?));
    let mut cursors = Vec::with_capacity(editor.selection.cursors().len());
    for cursor in editor.selection.cursors() {
        let pair = vec![
            Value::from(ValueKind::Integer(integer(cursor.anchor)?)),
            Value::from(ValueKind::Integer(integer(cursor.head)?)),
        ];
        cursors.push(Value::from(ValueKind::Array(Array::new(pair))));
    }
    table.insert("cursors", ValueKind::Array(Array::new(cursors)));
    let mut scroll = Table::new_inline();
    scroll.insert("line", ValueKind::Integer(integer(editor.scroll.line)?));
    scroll.insert("row", ValueKind::Integer(integer(editor.scroll.row)?));
    scroll.insert("y", ValueKind::Float(f64::from(editor.scroll.y)));
    scroll.insert("x", ValueKind::Float(f64::from(editor.scroll.x)));
    table.insert("scroll", ValueKind::Table(scroll));
    Ok(table)
}

/// `value` as a TOML integer.
fn integer(value: usize) -> Result<i64, WorkspaceError> {
    i64::try_from(value).map_err(|_| bad(Path::new(DIR_NAME), "a number is past what TOML holds"))
}

fn bad(path: &Path, reason: &'static str) -> WorkspaceError {
    WorkspaceError::BadSessionFile {
        path: path.to_path_buf(),
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_the_paths_hash() {
        let name = file_name(Path::new("/home/me/src/tidepool"));
        assert_eq!(name.len(), 64 + ".toml".len());
        assert!(name.ends_with(".toml"));
        assert_ne!(name, file_name(Path::new("/home/me/src/tidepool2")));
        assert_eq!(
            path_in(Path::new("/data"), Path::new("/w")),
            Path::new("/data")
                .join("sessions")
                .join(file_name(Path::new("/w")))
        );
    }

    #[test]
    fn a_selection_past_the_limit_keeps_its_primary() {
        let others = (0..2000).map(|at| Cursor::caret(at * 2));
        let selection = Selection::new(Cursor::caret(3999), others);
        let cut = capped(&selection);
        assert_eq!(cut.cursors().len(), MAX_CURSORS);
        assert_eq!(cut.primary(), Cursor::caret(3999));
    }
}
