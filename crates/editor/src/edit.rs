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

//! Editing: the [`Editor`], an editor view over a buffer's text and its
//! undo history, which runs the editing [`Command`]s and answers typed
//! text and the mouse (M4.2, ADR-0065). Specification:
//! `docs/subsystems/editor.md` → "Editing".
//!
//! New in Tidepool.
//!
//! # Commands
//!
//! Every editing action is a named [`Command`], with an `editor.` id and a
//! palette title, which [`Command::register`] adds to a
//! [`Registry`]; the shell registers them after its own, and the default
//! keymap binds them ([`keymap`](crate::keymap)). The editor's owner
//! resolves keys with its keymap engine and runs the command an id names
//! with [`Editor::run`]. They act on every cursor of the selection at once,
//! through [`Transaction::change_each`] for edits (ADR-0020):
//!
//! - **Motions** ([`Command::Move`]), each with a `…Select` twin that
//!   moves the head and keeps the anchor: a grapheme cluster (what a reader
//!   sees as one character) left or right, where a selection collapses to
//!   its start or end instead; a word left or right; a line up or down; a
//!   line's start or end; the text's start or end; and a page up or down,
//!   as many lines as rows fit in the view. See [`Motion`].
//! - **Edits:** typed text, which replaces each selection
//!   ([`Editor::type_text`], or an `InputEvent::Text` given to
//!   [`Editor::handle`]); deleting each selection or the cluster or word
//!   before or after each caret; a new line, with the line's indentation;
//!   indenting with [`INDENT_SPACES`] spaces, and outdenting.
//! - **Undo and redo** through the [`History`], one command one step.
//! - **The clipboard** ([`Request`]), the owner's to carry out.
//! - **Cursors:** select all, add a cursor above or below, and cancel.
//!
//! Every command that moves the primary cursor, or edits, scrolls the
//! primary cursor's head into view.
//!
//! # Why long lines are read in a window
//!
//! A grapheme cluster boundary can depend on all of a long run of
//! characters before it, so one query on a crafted 100 MB line can take a
//! second (M1.1's risk). The main thread never waits that long (ADR-0012):
//! on a line longer than [`MAX_LINE_BYTES`], a motion or a deletion reads
//! at most that many bytes either side of the cursor, and in a cluster or a
//! word longer than that it stops at the edge of what it read, on a char
//! boundary. So no keystroke reads more than about 20 KB of the text for
//! each cursor, and
//! on a line the view shows whole, motions are exact (ADR-0065).
//!
//! # Vertical motion
//!
//! Up and Down move each cursor one line, with soft wrap on too, to its
//! *goal column*: the column it had when a run of vertical motions began,
//! counting a tab to its next stop ([`TAB_STOP_SPACES`] apart) and any
//! other cluster as one. A cursor lands on the last cluster boundary whose
//! column is at most its goal. Up on the first line goes to its start, and
//! Down on the last to its end. Any other command forgets the goals.
//!
//! # Undo steps
//!
//! Each command commits one step. Typing coalesces: keystrokes that come
//! within [`TYPING_WINDOW`] of the last, with the cursors where it left
//! them, join its step, except that text starting with white space after a
//! character that is not starts a new one, so undo takes back a word, with
//! the space before it, at a time. Repeated deletions of a cluster before a
//! caret coalesce the same way, and so do those after one. Undo and redo
//! restore the selection their step names.
//!
//! # The clipboard
//!
//! The editor has no clipboard: the window's belongs to the frontend, and
//! a paste is answered later (ADR-0042). So [`Command::Copy`] and
//! [`Command::Cut`] return a [`Request::Copy`] with the text, and
//! [`Command::Paste`] a [`Request::Paste`], which the owner carries out,
//! giving the text that comes back to [`Editor::paste`]. M4.3 has the
//! binary do it with the window's clipboard.
//!
//! - With every cursor a caret, a copy or a cut takes each caret's whole
//!   line, its break included, or the editor's line break after a last line
//!   that has none. A paste of exactly that text inserts it above each
//!   caret's line.
//! - Otherwise a copy joins the cursors' selections with the editor's line
//!   break, leaving out empty ones.
//! - A paste whose lines are as many as the cursors, more than one, gives
//!   each cursor one line, in order; a final line break makes no extra line.
//!   Any other paste goes whole to every cursor.
//!
//! # The mouse
//!
//! A primary press on the text area places a caret at the nearest cluster
//! boundary ([`EditorView::point_to_byte`]); with Shift it extends the
//! primary cursor's selection to it, and with Alt it adds a caret there,
//! which becomes the primary. A drag extends the primary cursor's
//! selection, and a second press at the same place within [`DOUBLE_CLICK`]
//! selects the word there.
//!
//! # Focus and Tab
//!
//! While the text area has focus, [`Editor::handle`] keeps Tab and
//! Shift+Tab from the tree, whose default moves focus, and gives them back
//! unhandled, for the owner's keymap to indent and outdent with.
//!
//! # Example
//!
//! ```
//! use std::time::Instant;
//!
//! use tidepool_core::geom::Size;
//! use tidepool_editor::edit::{Command, Editor, Motion, Request};
//! use tidepool_text::Rope;
//! use tidepool_ui::Tree;
//! use tidepool_ui::metrics::FixedMetrics;
//!
//! let mut tree = Tree::new();
//! let root = tree.root();
//! let mut editor = Editor::insert(&mut tree, root, "notes.md", Rope::from("one two\n"))?;
//! let (size, metrics) = (Size::new(640.0, 480.0), FixedMetrics::DEFAULT);
//! assert!(tree.frame(size, &metrics).is_some());
//! let now = Instant::now();
//!
//! // Select the first word and type over it.
//! let select_word = Command::Move { motion: Motion::WordRight, select: true };
//! editor.run(&mut tree, select_word, now, &metrics)?;
//! editor.type_text(&mut tree, "1", now, &metrics)?;
//! assert_eq!(*editor.text(), "1 two\n");
//!
//! // Undo brings the word back, selected again.
//! editor.run(&mut tree, Command::Undo, now, &metrics)?;
//! assert_eq!(*editor.text(), "one two\n");
//!
//! // Copying asks the owner to put the selection on the clipboard.
//! let copy = editor.run(&mut tree, Command::Copy, now, &metrics)?;
//! assert_eq!(copy.request, Some(Request::Copy(Rope::from("one"))));
//! # Ok::<(), tidepool_editor::EditorError>(())
//! ```
//!
//! [`TAB_STOP_SPACES`]: crate::view::TAB_STOP_SPACES
//! [`MAX_LINE_BYTES`]: crate::view::MAX_LINE_BYTES

mod motion;

use std::ops::Range;
use std::time::{Duration, Instant};

use tidepool_core::input::{InputEvent, Key, Modifiers, MouseButton};
use tidepool_text::{
    Assoc, ChangeSet, Cursor, Group, History, LineBreak, Place, Rope, Selection, TextError,
    Transaction,
};
use tidepool_ui::NodeId;
use tidepool_ui::Tree;
use tidepool_ui::input::Response;
use tidepool_ui::metrics::TextMetrics;

use crate::EditorError;
use crate::command::Registry;
use crate::view::{EDITOR_TEXT, EditorView, TAB_STOP_SPACES};

use motion::{
    at_column, column, indent_end, line_of, next_cluster, prev_cluster, word_at, word_left,
    word_right,
};

/// How long after a keystroke the next one may come and still join its
/// undo step.
pub const TYPING_WINDOW: Duration = Duration::from_secs(1);

/// How soon a second press at the same place counts as a double click.
pub const DOUBLE_CLICK: Duration = Duration::from_millis(500);

/// How many spaces Tab indents by, and Shift+Tab outdents by at most: a
/// tab stop's width, until M4.3 applies the settings (ADR-0067).
pub const INDENT_SPACES: usize = TAB_STOP_SPACES;

/// The [`Group::Coalesce`] kinds of typing, which take turns, so that a
/// word that starts after a space starts a new step and its letters join
/// it.
const TYPING: [u32; 2] = [1, 2];

/// The [`Group::Coalesce`] kind of deleting a cluster before a caret.
const DELETE_LEFT: u32 = 3;

/// The [`Group::Coalesce`] kind of deleting a cluster after a caret.
const DELETE_RIGHT: u32 = 4;

/// Where a [`Command::Move`] moves each cursor's head.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Motion {
    /// One grapheme cluster left; a selection collapses to its start.
    Left,
    /// One grapheme cluster right; a selection collapses to its end.
    Right,
    /// Back past white space, then to the start of a word or of a run of
    /// separators, on the line; from a line's start, before the break above.
    WordLeft,
    /// Past white space, then past a word or a run of separators, on the
    /// line; from a line's end, past its break.
    WordRight,
    /// One line up, to the goal column.
    Up,
    /// One line down, to the goal column.
    Down,
    /// The line's first character that is not a space or a tab, or the
    /// line's start from there.
    Home,
    /// The line's end, before its break.
    End,
    /// The text's start.
    Top,
    /// The text's end.
    Bottom,
    /// As many lines up as rows fit in the view, to the goal column.
    PageUp,
    /// As many lines down as rows fit in the view, to the goal column.
    PageDown,
}

/// An editing command (the [module documentation](self#commands)).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    /// Moves each cursor's head by `motion`; without `select`, each cursor
    /// becomes a caret there.
    Move {
        /// Where to.
        motion: Motion,
        /// Whether the anchor stays, selecting what the head passes.
        select: bool,
    },
    /// Deletes each selection, or the cluster before each caret.
    DeleteLeft,
    /// Deletes each selection, or the cluster after each caret.
    DeleteRight,
    /// Deletes each selection, or back to where [`Motion::WordLeft`] goes.
    DeleteWordLeft,
    /// Deletes each selection, or on to where [`Motion::WordRight`] goes.
    DeleteWordRight,
    /// Replaces each selection with the editor's line break and the line's
    /// indentation, up to the cursor.
    NewLine,
    /// With a selection over more than one line, indents each line it
    /// spans that is not empty by [`INDENT_SPACES`] spaces; otherwise
    /// replaces each selection with spaces to the next tab stop.
    Indent,
    /// Removes a tab, or up to [`INDENT_SPACES`] spaces, from the start of
    /// each line a cursor is on.
    Outdent,
    /// Undoes the last step.
    Undo,
    /// Redoes the step undone last.
    Redo,
    /// Asks the owner to put the selection on the clipboard
    /// ([`Request::Copy`]).
    Copy,
    /// As [`Command::Copy`], then deletes what it copied.
    Cut,
    /// Asks the owner for the clipboard's text ([`Request::Paste`]).
    Paste,
    /// Selects the whole text.
    SelectAll,
    /// Adds a caret a line above the topmost cursor, at its goal column.
    AddCursorAbove,
    /// Adds a caret a line below the bottommost cursor, at its goal column.
    AddCursorBelow,
    /// Leaves only the primary cursor, or with one cursor collapses its
    /// selection to its head.
    Cancel,
}

/// Each motion, in the order [`Command::ALL`] lists them.
const MOTIONS: [Motion; 12] = [
    Motion::Left,
    Motion::Right,
    Motion::WordLeft,
    Motion::WordRight,
    Motion::Up,
    Motion::Down,
    Motion::Home,
    Motion::End,
    Motion::Top,
    Motion::Bottom,
    Motion::PageUp,
    Motion::PageDown,
];

/// The commands that are not motions, in the order [`Command::ALL`] lists
/// them.
const OTHERS: [Command; 16] = [
    Command::DeleteLeft,
    Command::DeleteRight,
    Command::DeleteWordLeft,
    Command::DeleteWordRight,
    Command::NewLine,
    Command::Indent,
    Command::Outdent,
    Command::Undo,
    Command::Redo,
    Command::Copy,
    Command::Cut,
    Command::Paste,
    Command::SelectAll,
    Command::AddCursorAbove,
    Command::AddCursorBelow,
    Command::Cancel,
];

impl Command {
    /// Every editing command, in the order [`register`](Self::register)
    /// registers them: each motion, then its `…Select` twin, then the rest.
    pub const ALL: [Command; 40] = {
        let mut all = [Command::Cancel; 40];
        let mut i = 0;
        while i < MOTIONS.len() {
            all[2 * i] = Command::Move {
                motion: MOTIONS[i],
                select: false,
            };
            all[2 * i + 1] = Command::Move {
                motion: MOTIONS[i],
                select: true,
            };
            i += 1;
        }
        let mut j = 0;
        while j < OTHERS.len() {
            all[2 * MOTIONS.len() + j] = OTHERS[j];
            j += 1;
        }
        all
    };

    /// Its id, such as `editor.cursorWordLeftSelect`.
    pub const fn id(self) -> &'static str {
        self.names().0
    }

    /// Its title in the palette, such as "Cursor: Select Word Left".
    pub const fn title(self) -> &'static str {
        self.names().1
    }

    /// The command `id` names, if it is an editing command.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|command| command.id() == id)
    }

    /// Registers every editing command in `registry`, in
    /// [`ALL`](Self::ALL)'s order.
    ///
    /// # Errors
    ///
    /// [`EditorError::TakenCommandId`] if `registry` has one of the ids
    /// already; the commands before it stay registered.
    pub fn register(registry: &mut Registry) -> Result<(), EditorError> {
        for command in Self::ALL {
            registry.register(command.id(), command.title())?;
        }
        Ok(())
    }

    /// Its id and title.
    const fn names(self) -> (&'static str, &'static str) {
        match self {
            Self::Move { motion, select } => match (motion, select) {
                (Motion::Left, false) => ("editor.cursorLeft", "Cursor: Move Left"),
                (Motion::Left, true) => ("editor.cursorLeftSelect", "Cursor: Select Left"),
                (Motion::Right, false) => ("editor.cursorRight", "Cursor: Move Right"),
                (Motion::Right, true) => ("editor.cursorRightSelect", "Cursor: Select Right"),
                (Motion::WordLeft, false) => ("editor.cursorWordLeft", "Cursor: Move Word Left"),
                (Motion::WordLeft, true) => {
                    ("editor.cursorWordLeftSelect", "Cursor: Select Word Left")
                }
                (Motion::WordRight, false) => ("editor.cursorWordRight", "Cursor: Move Word Right"),
                (Motion::WordRight, true) => {
                    ("editor.cursorWordRightSelect", "Cursor: Select Word Right")
                }
                (Motion::Up, false) => ("editor.cursorUp", "Cursor: Move Up"),
                (Motion::Up, true) => ("editor.cursorUpSelect", "Cursor: Select Up"),
                (Motion::Down, false) => ("editor.cursorDown", "Cursor: Move Down"),
                (Motion::Down, true) => ("editor.cursorDownSelect", "Cursor: Select Down"),
                (Motion::Home, false) => ("editor.cursorHome", "Cursor: Move to Line Start"),
                (Motion::Home, true) => ("editor.cursorHomeSelect", "Cursor: Select to Line Start"),
                (Motion::End, false) => ("editor.cursorEnd", "Cursor: Move to Line End"),
                (Motion::End, true) => ("editor.cursorEndSelect", "Cursor: Select to Line End"),
                (Motion::Top, false) => ("editor.cursorTop", "Cursor: Move to Start"),
                (Motion::Top, true) => ("editor.cursorTopSelect", "Cursor: Select to Start"),
                (Motion::Bottom, false) => ("editor.cursorBottom", "Cursor: Move to End"),
                (Motion::Bottom, true) => ("editor.cursorBottomSelect", "Cursor: Select to End"),
                (Motion::PageUp, false) => ("editor.cursorPageUp", "Cursor: Move Page Up"),
                (Motion::PageUp, true) => ("editor.cursorPageUpSelect", "Cursor: Select Page Up"),
                (Motion::PageDown, false) => ("editor.cursorPageDown", "Cursor: Move Page Down"),
                (Motion::PageDown, true) => {
                    ("editor.cursorPageDownSelect", "Cursor: Select Page Down")
                }
            },
            Self::DeleteLeft => ("editor.deleteLeft", "Edit: Delete Left"),
            Self::DeleteRight => ("editor.deleteRight", "Edit: Delete Right"),
            Self::DeleteWordLeft => ("editor.deleteWordLeft", "Edit: Delete Word Left"),
            Self::DeleteWordRight => ("editor.deleteWordRight", "Edit: Delete Word Right"),
            Self::NewLine => ("editor.newLine", "Edit: New Line"),
            Self::Indent => ("editor.indent", "Edit: Indent"),
            Self::Outdent => ("editor.outdent", "Edit: Outdent"),
            Self::Undo => ("editor.undo", "Edit: Undo"),
            Self::Redo => ("editor.redo", "Edit: Redo"),
            Self::Copy => ("editor.copy", "Edit: Copy"),
            Self::Cut => ("editor.cut", "Edit: Cut"),
            Self::Paste => ("editor.paste", "Edit: Paste"),
            Self::SelectAll => ("editor.selectAll", "Selection: Select All"),
            Self::AddCursorAbove => ("editor.addCursorAbove", "Selection: Add Cursor Above"),
            Self::AddCursorBelow => ("editor.addCursorBelow", "Selection: Add Cursor Below"),
            Self::Cancel => ("editor.cancel", "Selection: Back to One Cursor"),
        }
    }
}

/// What an editor asks its owner to do with the clipboard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// Put this text on the clipboard. Handed over as a rope, in O(1), so
    /// that turning it into the clipboard's text costs only the owner.
    /// `Debug` shows its size, never the text.
    Copy(Rope),
    /// Ask for the clipboard's text, and give it to [`Editor::paste`] when
    /// it comes.
    Paste,
}

/// What came of a command.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Outcome {
    /// Whether the text, the selection or the scroll position changed, so
    /// that the tree has a new frame to draw.
    pub changed: bool,
    /// What the owner is asked to do with the clipboard.
    pub request: Option<Request>,
}

impl Outcome {
    /// An outcome that changed something, or not.
    const fn changed(changed: bool) -> Self {
        Self {
            changed,
            request: None,
        }
    }
}

/// The goal columns of a selection that vertical motions left.
#[derive(Clone, Debug)]
struct Goals {
    /// The selection they are for.
    selection: Selection,
    /// One for each of its cursors, in order.
    columns: Vec<usize>,
}

/// An editor: an [`EditorView`] over a buffer's text and its [`History`],
/// which runs the editing commands (the [module documentation](self)).
///
/// The text changes only through it: every edit goes through its history
/// (ADR-0020), and the view shows each step.
#[derive(Debug)]
pub struct Editor {
    view: EditorView,
    text: Rope,
    history: History,
    line_break: LineBreak,
    goals: Option<Goals>,
    /// Which of [`TYPING`]'s kinds typing coalesces as now.
    typing: usize,
    /// The anchor of the drag a primary press on the text area started.
    drag: Option<usize>,
    /// The last primary press on the text area that may start a double
    /// click: when, and the offset it placed a caret at.
    last_press: Option<(Instant, usize)>,
    /// The text the last copy or cut of whole lines asked for.
    line_copy: Option<Rope>,
}

impl Editor {
    /// Builds an editor over `text` as the last child of `parent`: a new
    /// [`EditorView`] named `name`, a caret at the start, a new history,
    /// and the text's first line break as its line break, or LF.
    ///
    /// # Errors
    ///
    /// As for [`EditorView::insert`].
    pub fn insert(
        tree: &mut Tree,
        parent: NodeId,
        name: impl Into<String>,
        text: Rope,
    ) -> Result<Self, EditorError> {
        let view = EditorView::insert(tree, parent, name, text.clone())?;
        let line_break = if text.len_lines() > 1 {
            text.line_span(0)?.line_break.unwrap_or(LineBreak::Lf)
        } else {
            LineBreak::Lf
        };
        Ok(Self {
            view,
            history: History::new(&text),
            text,
            line_break,
            goals: None,
            typing: 0,
            drag: None,
            last_press: None,
            line_copy: None,
        })
    }

    /// Its view.
    pub const fn view(&self) -> EditorView {
        self.view
    }

    /// The text.
    pub const fn text(&self) -> &Rope {
        &self.text
    }

    /// The undo history, which holds every step.
    pub const fn history(&self) -> &History {
        &self.history
    }

    /// The line break [`Command::NewLine`] inserts.
    pub const fn line_break(&self) -> LineBreak {
        self.line_break
    }

    /// Sets the line break [`Command::NewLine`] inserts, as a file's line
    /// endings say (M4.3).
    pub fn set_line_break(&mut self, line_break: LineBreak) {
        self.line_break = line_break;
    }

    /// The selection.
    ///
    /// # Errors
    ///
    /// [`EditorError::Ui`] if `tree` does not hold the editor's view.
    pub fn selection<'t>(&self, tree: &'t Tree) -> Result<&'t Selection, EditorError> {
        self.view.selection(tree)
    }

    /// Sets the selection, which forgets the goal columns.
    ///
    /// # Errors
    ///
    /// As for [`EditorView::set_selection`]; nothing changes then.
    pub fn set_selection(
        &mut self,
        tree: &mut Tree,
        selection: Selection,
    ) -> Result<(), EditorError> {
        self.view.set_selection(tree, selection)?;
        self.goals = None;
        Ok(())
    }

    /// Runs `command` at `now`, measuring with `metrics`, the metrics the
    /// frames are drawn with.
    ///
    /// # Errors
    ///
    /// [`EditorError::Ui`] if `tree` does not hold the editor's view; an
    /// [`EditorError::Text`] only from a bug, as the selection is checked
    /// against the text. Nothing changes then.
    pub fn run(
        &mut self,
        tree: &mut Tree,
        command: Command,
        now: Instant,
        metrics: &dyn TextMetrics,
    ) -> Result<Outcome, EditorError> {
        match command {
            Command::Move { motion, select } => self.move_by(tree, motion, select, metrics),
            Command::DeleteLeft => {
                self.delete(tree, now, metrics, Some(DELETE_LEFT), |text, at| {
                    Ok(prev_cluster(text, at)?..at)
                })
            }
            Command::DeleteRight => {
                self.delete(tree, now, metrics, Some(DELETE_RIGHT), |text, at| {
                    Ok(at..next_cluster(text, at)?)
                })
            }
            Command::DeleteWordLeft => self.delete(tree, now, metrics, None, |text, at| {
                Ok(word_left(text, at)?..at)
            }),
            Command::DeleteWordRight => self.delete(tree, now, metrics, None, |text, at| {
                Ok(at..word_right(text, at)?)
            }),
            Command::NewLine => self.new_line(tree, now, metrics),
            Command::Indent => self.indent(tree, now, metrics),
            Command::Outdent => self.outdent(tree, now, metrics),
            Command::Undo => {
                let step = self.history.undo(&mut self.text)?;
                self.show_step(tree, step.map(|step| step.selection), metrics)
            }
            Command::Redo => {
                let step = self.history.redo(&mut self.text)?;
                self.show_step(tree, step.map(|step| step.selection), metrics)
            }
            Command::Copy => Ok(Outcome {
                changed: false,
                request: Some(Request::Copy(self.copied(tree)?)),
            }),
            Command::Cut => self.cut(tree, now, metrics),
            Command::Paste => Ok(Outcome {
                changed: false,
                request: Some(Request::Paste),
            }),
            Command::SelectAll => {
                let all = Selection::single(Cursor::new(0, self.text.len()));
                self.select(tree, all, None, metrics)
            }
            Command::AddCursorAbove => self.add_cursor(tree, false, metrics),
            Command::AddCursorBelow => self.add_cursor(tree, true, metrics),
            Command::Cancel => {
                let selection = self.selection(tree)?;
                let primary = selection.primary();
                if selection.cursors().len() > 1 {
                    self.select(tree, Selection::single(primary), None, metrics)
                } else if !primary.is_caret() {
                    self.select(tree, Selection::caret(primary.head), None, metrics)
                } else {
                    Ok(Outcome::changed(false))
                }
            }
        }
    }

    /// Gives `event` to the view's tree, as [`EditorView::handle`] does,
    /// then answers what it leaves that is the editor's, at `now`: text
    /// typed while the text area has focus, and the mouse on the text area
    /// (the [module documentation](self#the-mouse)). Tab and Shift+Tab,
    /// while the text area has focus, go to no widget and come back
    /// unhandled, for the owner's keymap. Returns the tree's response, with
    /// `handled` set for what the editor took.
    ///
    /// # Errors
    ///
    /// As for [`run`](Self::run).
    pub fn handle(
        &mut self,
        tree: &mut Tree,
        event: &InputEvent,
        now: Instant,
        metrics: &dyn TextMetrics,
    ) -> Result<Response, EditorError> {
        let text_node = self.view.text_node();
        let focused = tree.focused() == Some(text_node);
        if focused
            && let InputEvent::KeyDown {
                key: Key::Tab,
                modifiers,
                ..
            } = *event
            && modifiers.difference(Modifiers::SHIFT).is_empty()
        {
            return Ok(Response::default());
        }
        let mut response = self.view.handle(tree, event, metrics)?;
        if response.blocked {
            return Ok(response);
        }
        match event {
            InputEvent::Text(typed) if !response.handled && tree.focused() == Some(text_node) => {
                self.type_text(tree, typed, now, metrics)?;
                response.handled = true;
            }
            InputEvent::MouseDown {
                button: MouseButton::Left,
                position,
                modifiers,
            } if tree.hit_test(*position) == Some(text_node) => {
                let at = self.view.point_to_byte(tree, *position, metrics)?;
                self.press(tree, at, *modifiers, now, metrics)?;
                response.handled = true;
            }
            InputEvent::MouseMove { position, .. } => {
                if let Some(anchor) = self.drag {
                    let at = self.view.point_to_byte(tree, *position, metrics)?;
                    let selection = self.selection(tree)?;
                    let primary = selection.primary_index();
                    let others = selection
                        .cursors()
                        .iter()
                        .enumerate()
                        .filter(|&(index, _)| index != primary)
                        .map(|(_, &cursor)| cursor)
                        .collect::<Vec<_>>();
                    let dragged = Selection::new(Cursor::new(anchor, at), others);
                    self.select(tree, dragged, None, metrics)?;
                    response.handled = true;
                }
            }
            InputEvent::MouseUp {
                button: MouseButton::Left,
                ..
            }
            | InputEvent::FocusLost => self.drag = None,
            _ => {}
        }
        Ok(response)
    }

    /// Types `typed` at every cursor, replacing each selection and leaving
    /// a caret after it, at `now`, coalesced as the [module
    /// documentation](self#undo-steps) says. Control characters are left
    /// out, as a frontend leaves them out of typed text; text with none
    /// left types nothing.
    ///
    /// # Errors
    ///
    /// As for [`run`](Self::run).
    pub fn type_text(
        &mut self,
        tree: &mut Tree,
        typed: &str,
        now: Instant,
        metrics: &dyn TextMetrics,
    ) -> Result<Outcome, EditorError> {
        let typed: String = typed.chars().filter(|c| !c.is_control()).collect();
        if typed.is_empty() {
            return Ok(Outcome::changed(false));
        }
        let selection = self.selection(tree)?.clone();
        let start = selection.primary().start();
        let after_word = start > 0
            && self
                .text
                .chars_in(self.text.floor_char_boundary(start - 1)..start)
                .ok()
                .and_then(|mut chars| chars.next())
                .is_some_and(|c| !c.is_whitespace());
        if after_word && typed.starts_with(char::is_whitespace) {
            self.typing = 1 - self.typing;
        }
        let group = Group::Coalesce {
            kind: TYPING[self.typing],
            window: TYPING_WINDOW,
        };
        let new = Rope::from(typed);
        let edit = Transaction::change_each(&self.text, &selection, |cursor| {
            Some((cursor.range(), new.clone(), Place::End))
        })?;
        self.commit(tree, edit, &selection, now, group, metrics)
    }

    /// Pastes `pasted`, the clipboard's text that a [`Request::Paste`] asked
    /// for, at `now`, as the [module documentation](self#the-clipboard)
    /// says. An empty text pastes nothing.
    ///
    /// # Errors
    ///
    /// As for [`run`](Self::run).
    pub fn paste(
        &mut self,
        tree: &mut Tree,
        pasted: &str,
        now: Instant,
        metrics: &dyn TextMetrics,
    ) -> Result<Outcome, EditorError> {
        if pasted.is_empty() {
            return Ok(Outcome::changed(false));
        }
        let selection = self.selection(tree)?.clone();
        let pasted = Rope::from(pasted);
        let all_carets = selection.cursors().iter().all(|cursor| cursor.is_caret());
        let edit = if all_carets && self.line_copy.as_ref() == Some(&pasted) {
            let mut starts = Vec::with_capacity(selection.cursors().len());
            for cursor in selection.cursors() {
                starts.push(line_of(&self.text, cursor.head)?.1.start);
            }
            let mut starts = starts.into_iter();
            Transaction::change_each(&self.text, &selection, |_| {
                let start = starts.next()?;
                Some((start..start, pasted.clone(), Place::Map))
            })?
        } else if let Some(pieces) = spread(&pasted, selection.cursors().len())? {
            let mut pieces = pieces.into_iter();
            Transaction::change_each(&self.text, &selection, |cursor| {
                Some((cursor.range(), pieces.next()?, Place::End))
            })?
        } else {
            Transaction::change_each(&self.text, &selection, |cursor| {
                Some((cursor.range(), pasted.clone(), Place::End))
            })?
        };
        self.commit(tree, edit, &selection, now, Group::New, metrics)
    }

    /// Commits `edit`, made from `before`, through the history, and shows
    /// the step.
    fn commit(
        &mut self,
        tree: &mut Tree,
        edit: Transaction,
        before: &Selection,
        now: Instant,
        group: Group,
        metrics: &dyn TextMetrics,
    ) -> Result<Outcome, EditorError> {
        let step = self
            .history
            .commit(&mut self.text, edit, before, now, group)?;
        if step.changes.is_empty() {
            return self.select(tree, step.selection, None, metrics);
        }
        self.show_step(tree, Some(step.selection), metrics)
    }

    /// Shows the text after a step of the history, with `selection`, which
    /// the history checked against it, scrolled to the primary cursor;
    /// nothing for no step.
    fn show_step(
        &mut self,
        tree: &mut Tree,
        selection: Option<Selection>,
        metrics: &dyn TextMetrics,
    ) -> Result<Outcome, EditorError> {
        let Some(selection) = selection else {
            return Ok(Outcome::changed(false));
        };
        // Offsets kept from before the step may lie past the text's end or
        // inside a char now: a drag's anchor, and the press a double click
        // would pair with. The step ends both.
        self.goals = None;
        self.drag = None;
        self.last_press = None;
        let head = selection.primary().head;
        self.view.show(tree, self.text.clone(), selection)?;
        self.view.scroll_into_view(tree, head, metrics)?;
        Ok(Outcome::changed(true))
    }

    /// Sets `selection`, with the goal columns `goals` for it, and scrolls
    /// its primary cursor into view.
    fn select(
        &mut self,
        tree: &mut Tree,
        selection: Selection,
        goals: Option<Vec<usize>>,
        metrics: &dyn TextMetrics,
    ) -> Result<Outcome, EditorError> {
        let head = selection.primary().head;
        let changed = *self.selection(tree)? != selection;
        if changed {
            self.view.set_selection(tree, selection.clone())?;
        }
        self.goals = goals.map(|columns| Goals { selection, columns });
        let scrolled = self.view.scroll_into_view(tree, head, metrics)?;
        Ok(Outcome::changed(changed || scrolled))
    }

    /// Moves every cursor's head by `motion`.
    fn move_by(
        &mut self,
        tree: &mut Tree,
        motion: Motion,
        select: bool,
        metrics: &dyn TextMetrics,
    ) -> Result<Outcome, EditorError> {
        let lines = match motion {
            Motion::Up => Some(-1),
            Motion::Down => Some(1),
            Motion::PageUp => Some(-signed(self.page_lines(tree, metrics))),
            Motion::PageDown => Some(signed(self.page_lines(tree, metrics))),
            _ => None,
        };
        if let Some(lines) = lines {
            return self.vertical(tree, lines, select, metrics);
        }
        let text = &self.text;
        let selection = self.selection(tree)?;
        let moved = rebuild(selection, |cursor| {
            if !select && !cursor.is_caret() {
                match motion {
                    Motion::Left => return Ok(Cursor::caret(cursor.start())),
                    Motion::Right => return Ok(Cursor::caret(cursor.end())),
                    _ => {}
                }
            }
            let at = cursor.head;
            let head = match motion {
                Motion::Left => prev_cluster(text, at)?,
                Motion::Right => next_cluster(text, at)?,
                Motion::WordLeft => word_left(text, at)?,
                Motion::WordRight => word_right(text, at)?,
                Motion::Home => {
                    let (_, span) = line_of(text, at)?;
                    let first = indent_end(text, &span);
                    if at == first { span.start } else { first }
                }
                Motion::End => line_of(text, at)?.1.content_end,
                Motion::Top => 0,
                Motion::Bottom => text.len(),
                Motion::Up | Motion::Down | Motion::PageUp | Motion::PageDown => at,
            };
            Ok(if select {
                Cursor::new(cursor.anchor, head)
            } else {
                Cursor::caret(head)
            })
        })?;
        self.select(tree, moved, None, metrics)
    }

    /// Moves every cursor's head `lines` lines down, or up for a negative
    /// count, to its goal column.
    fn vertical(
        &mut self,
        tree: &mut Tree,
        lines: isize,
        select: bool,
        metrics: &dyn TextMetrics,
    ) -> Result<Outcome, EditorError> {
        let selection = self.selection(tree)?.clone();
        let goals = self.goals_of(&selection)?;
        let text = &self.text;
        let last = text.len_lines() - 1;
        let mut landed = Vec::with_capacity(goals.len());
        let mut index = 0;
        let moved = rebuild(&selection, |cursor| {
            let goal = goals.get(index).copied().unwrap_or(0);
            index += 1;
            let (line, _) = line_of(text, cursor.head)?;
            let head = match line.checked_add_signed(lines) {
                None => 0,
                Some(target) if target > last => text.len(),
                Some(target) => at_column(text, &text.line_span(target)?, goal),
            };
            landed.push((head, goal));
            Ok(if select {
                Cursor::new(cursor.anchor, head)
            } else {
                Cursor::caret(head)
            })
        })?;
        let columns = goals_after(&moved, landed);
        self.select(tree, moved, Some(columns), metrics)
    }

    /// The goal column of each cursor of `selection`: those the last
    /// vertical motion left, if it left this selection, otherwise each
    /// head's column.
    fn goals_of(&self, selection: &Selection) -> Result<Vec<usize>, EditorError> {
        if let Some(goals) = &self.goals
            && goals.selection == *selection
        {
            return Ok(goals.columns.clone());
        }
        let mut columns = Vec::with_capacity(selection.cursors().len());
        for cursor in selection.cursors() {
            let (_, span) = line_of(&self.text, cursor.head)?;
            columns.push(column(&self.text, &span, cursor.head));
        }
        Ok(columns)
    }

    /// How many lines a page moves: the rows that fit in the text area, at
    /// least one.
    fn page_lines(&self, tree: &Tree, metrics: &dyn TextMetrics) -> usize {
        let height = tree
            .bounds(self.view.text_node())
            .map_or(0.0, |bounds| bounds.height);
        let row = metrics.line(&EDITOR_TEXT).height.max(1.0);
        // Saturating: a huge or NaN height counts as many or no rows.
        ((height / row).floor() as usize).max(1)
    }

    /// Adds a caret a line above the topmost cursor, or below the
    /// bottommost, at its goal column, as the primary.
    fn add_cursor(
        &mut self,
        tree: &mut Tree,
        below: bool,
        metrics: &dyn TextMetrics,
    ) -> Result<Outcome, EditorError> {
        let selection = self.selection(tree)?.clone();
        let mut goals = self.goals_of(&selection)?;
        let cursors = selection.cursors();
        let index = if below { cursors.len() - 1 } else { 0 };
        let (Some(&from), Some(&goal)) = (cursors.get(index), goals.get(index)) else {
            return Ok(Outcome::changed(false));
        };
        let (line, _) = line_of(&self.text, from.head)?;
        let target = if below {
            line.checked_add(1)
        } else {
            line.checked_sub(1)
        };
        let Some(target) = target.filter(|&target| target < self.text.len_lines()) else {
            return Ok(Outcome::changed(false));
        };
        let at = at_column(&self.text, &self.text.line_span(target)?, goal);
        let added = Selection::new(Cursor::caret(at), cursors.iter().copied());
        let mut landed: Vec<(usize, usize)> = cursors
            .iter()
            .map(|cursor| cursor.head)
            .zip(goals.drain(..))
            .collect();
        landed.push((at, goal));
        let columns = goals_after(&added, landed);
        self.select(tree, added, Some(columns), metrics)
    }

    /// Deletes each selection, or the range `range` gives for each caret's
    /// head, coalescing as `kind` when every cursor is a caret.
    fn delete(
        &mut self,
        tree: &mut Tree,
        now: Instant,
        metrics: &dyn TextMetrics,
        kind: Option<u32>,
        mut range: impl FnMut(&Rope, usize) -> Result<Range<usize>, TextError>,
    ) -> Result<Outcome, EditorError> {
        let selection = self.selection(tree)?.clone();
        let mut ranges = Vec::with_capacity(selection.cursors().len());
        for cursor in selection.cursors() {
            ranges.push(if cursor.is_caret() {
                range(&self.text, cursor.head)?
            } else {
                cursor.range()
            });
        }
        let all_carets = selection.cursors().iter().all(|cursor| cursor.is_caret());
        let group = match kind {
            Some(kind) if all_carets => Group::Coalesce {
                kind,
                window: TYPING_WINDOW,
            },
            _ => Group::New,
        };
        let mut ranges = ranges.into_iter();
        let edit = Transaction::change_each(&self.text, &selection, |_| {
            Some((ranges.next()?, "", Place::Start))
        })?;
        self.commit(tree, edit, &selection, now, group, metrics)
    }

    /// Replaces each selection with the line break and its line's
    /// indentation, up to the cursor.
    fn new_line(
        &mut self,
        tree: &mut Tree,
        now: Instant,
        metrics: &dyn TextMetrics,
    ) -> Result<Outcome, EditorError> {
        let selection = self.selection(tree)?.clone();
        let mut texts = Vec::with_capacity(selection.cursors().len());
        for cursor in selection.cursors() {
            let start = cursor.start();
            let (_, span) = line_of(&self.text, start)?;
            let indent = span.start..indent_end(&self.text, &span).min(start);
            let mut new = String::from(self.line_break.as_str());
            for chunk in self.text.chunks_in(indent)? {
                new.push_str(chunk);
            }
            texts.push(Rope::from(new));
        }
        let mut texts = texts.into_iter();
        let edit = Transaction::change_each(&self.text, &selection, |cursor| {
            Some((cursor.range(), texts.next()?, Place::End))
        })?;
        self.commit(tree, edit, &selection, now, Group::New, metrics)
    }

    /// Indents, as [`Command::Indent`] says.
    fn indent(
        &mut self,
        tree: &mut Tree,
        now: Instant,
        metrics: &dyn TextMetrics,
    ) -> Result<Outcome, EditorError> {
        let selection = self.selection(tree)?.clone();
        let mut lines = Vec::new();
        let mut spans_lines = false;
        for cursor in selection.cursors() {
            let touched = touched_lines(&self.text, *cursor)?;
            spans_lines |= touched.len() > 1;
            lines.extend(touched);
        }
        let edit = if spans_lines {
            lines.dedup();
            let spaces = Rope::from(" ".repeat(INDENT_SPACES));
            let mut edits = Vec::with_capacity(lines.len());
            for line in lines {
                let span = self.text.line_span(line)?;
                if span.content_end > span.start {
                    edits.push((span.start..span.start, spaces.clone()));
                }
            }
            let changes = ChangeSet::new(&self.text, edits)?;
            let after = selection.map(&changes, Assoc::After)?;
            Transaction::new(changes, after)
        } else {
            let mut texts = Vec::with_capacity(selection.cursors().len());
            for cursor in selection.cursors() {
                let start = cursor.start();
                let (_, span) = line_of(&self.text, start)?;
                let at = column(&self.text, &span, start);
                texts.push(Rope::from(
                    " ".repeat(TAB_STOP_SPACES - at % TAB_STOP_SPACES),
                ));
            }
            let mut texts = texts.into_iter();
            Transaction::change_each(&self.text, &selection, |cursor| {
                Some((cursor.range(), texts.next()?, Place::End))
            })?
        };
        self.commit(tree, edit, &selection, now, Group::New, metrics)
    }

    /// Outdents, as [`Command::Outdent`] says.
    fn outdent(
        &mut self,
        tree: &mut Tree,
        now: Instant,
        metrics: &dyn TextMetrics,
    ) -> Result<Outcome, EditorError> {
        let selection = self.selection(tree)?.clone();
        let mut lines = Vec::new();
        for cursor in selection.cursors() {
            lines.extend(touched_lines(&self.text, *cursor)?);
        }
        lines.dedup();
        let mut edits = Vec::new();
        for line in lines {
            let span = self.text.line_span(line)?;
            // A tab or spaces are one byte each: the bound only needs to
            // fall on a char boundary.
            let end = span
                .content_end
                .min(self.text.floor_char_boundary(span.start + INDENT_SPACES));
            let mut removed = span.start;
            for c in self.text.chars_in(span.start..end)? {
                match c {
                    '\t' if removed == span.start => {
                        removed += 1;
                        break;
                    }
                    ' ' => removed += 1,
                    _ => break,
                }
            }
            if removed > span.start {
                edits.push((span.start..removed, ""));
            }
        }
        let changes = ChangeSet::new(&self.text, edits)?;
        let after = selection.map(&changes, Assoc::After)?;
        self.commit(
            tree,
            Transaction::new(changes, after),
            &selection,
            now,
            Group::New,
            metrics,
        )
    }

    /// The text a copy takes (the [module
    /// documentation](self#the-clipboard)), noting it if it is whole lines.
    fn copied(&mut self, tree: &Tree) -> Result<Rope, EditorError> {
        let selection = self.selection(tree)?;
        let separator = self.line_break.as_str();
        let mut copied = Rope::new();
        if selection.cursors().iter().all(|cursor| cursor.is_caret()) {
            let mut lines = Vec::with_capacity(selection.cursors().len());
            for cursor in selection.cursors() {
                lines.push(line_of(&self.text, cursor.head)?.0);
            }
            lines.dedup();
            for line in lines {
                let span = self.text.line_span(line)?;
                copied.append(self.text.slice(span.full())?);
                if span.line_break.is_none() {
                    copied.append(Rope::from(separator));
                }
            }
            self.line_copy = Some(copied.clone());
            return Ok(copied);
        }
        let mut first = true;
        for cursor in selection
            .cursors()
            .iter()
            .filter(|cursor| !cursor.is_caret())
        {
            if !first {
                copied.append(Rope::from(separator));
            }
            first = false;
            copied.append(self.text.slice(cursor.range())?);
        }
        self.line_copy = None;
        Ok(copied)
    }

    /// Copies, then deletes what it copied: each selection, or with every
    /// cursor a caret each caret's whole line (a last line with no break
    /// takes the break before it).
    fn cut(
        &mut self,
        tree: &mut Tree,
        now: Instant,
        metrics: &dyn TextMetrics,
    ) -> Result<Outcome, EditorError> {
        let copied = self.copied(tree)?;
        let selection = self.selection(tree)?.clone();
        let edit = if self.line_copy.is_some() {
            let mut ranges: Vec<Range<usize>> = Vec::with_capacity(selection.cursors().len());
            for cursor in selection.cursors() {
                let (line, span) = line_of(&self.text, cursor.head)?;
                let range = match span.line_break {
                    Some(_) => span.full(),
                    None if line > 0 => self.text.line_span(line - 1)?.content_end..span.end(),
                    None => span.full(),
                };
                match ranges.last_mut() {
                    Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
                    _ => ranges.push(range),
                }
            }
            let changes = ChangeSet::new(&self.text, ranges.into_iter().map(|range| (range, "")))?;
            let after = selection.map(&changes, Assoc::After)?;
            Transaction::new(changes, after)
        } else {
            Transaction::change_each(&self.text, &selection, |cursor| {
                Some((cursor.range(), "", Place::Start))
            })?
        };
        let mut outcome = self.commit(tree, edit, &selection, now, Group::New, metrics)?;
        outcome.request = Some(Request::Copy(copied));
        Ok(outcome)
    }

    /// Answers a primary press on the text area at the offset `at`.
    fn press(
        &mut self,
        tree: &mut Tree,
        at: usize,
        modifiers: Modifiers,
        now: Instant,
        metrics: &dyn TextMetrics,
    ) -> Result<Outcome, EditorError> {
        let selection = self.selection(tree)?.clone();
        let double = self.last_press.is_some_and(|(time, byte)| {
            byte == at
                && now
                    .checked_duration_since(time)
                    .is_some_and(|gap| gap <= DOUBLE_CLICK)
        });
        // A third press starts again.
        self.last_press = if double { None } else { Some((now, at)) };
        let pressed = if double {
            let word = word_at(&self.text, at)?;
            let others = selection
                .cursors()
                .iter()
                .copied()
                .filter(|cursor| cursor.head != at);
            Selection::new(Cursor::new(word.start, word.end), others)
        } else if modifiers.contains(Modifiers::ALT) {
            Selection::new(Cursor::caret(at), selection.cursors().iter().copied())
        } else if modifiers.contains(Modifiers::SHIFT) {
            Selection::single(Cursor::new(selection.primary().anchor, at))
        } else {
            Selection::caret(at)
        };
        self.drag = Some(pressed.primary().anchor);
        self.select(tree, pressed, None, metrics)
    }
}

/// `selection` with each cursor replaced by what `f` makes of it, merged,
/// the primary's staying primary.
fn rebuild(
    selection: &Selection,
    mut f: impl FnMut(Cursor) -> Result<Cursor, TextError>,
) -> Result<Selection, TextError> {
    let mut moved = Vec::with_capacity(selection.cursors().len());
    for &cursor in selection.cursors() {
        moved.push(f(cursor)?);
    }
    let primary = moved
        .get(selection.primary_index())
        .copied()
        .unwrap_or(Cursor::caret(0));
    Ok(Selection::new(primary, moved))
}

/// The goal column of each cursor of `selection`, from the heads the
/// motion `landed` at and their goals; a cursor whose head is not among
/// them, which merging made, takes the goal of the first one that is.
fn goals_after(selection: &Selection, mut landed: Vec<(usize, usize)>) -> Vec<usize> {
    landed.sort_by_key(|&(head, _)| head);
    selection
        .cursors()
        .iter()
        .map(|cursor| {
            let index = landed.partition_point(|&(head, _)| head < cursor.head);
            landed
                .get(index)
                .or(landed.last())
                .map_or(0, |&(_, goal)| goal)
        })
        .collect()
}

/// The lines `cursor` is on: from its start's to its end's, but not the
/// line a selection ends at the start of.
fn touched_lines(text: &Rope, cursor: Cursor) -> Result<Range<usize>, TextError> {
    let (first, _) = line_of(text, cursor.start())?;
    let (last, span) = line_of(text, cursor.end())?;
    let last = if last > first && cursor.end() == span.start {
        last - 1
    } else {
        last
    };
    Ok(first..last + 1)
}

/// The lines of `pasted`, one for each of `cursors` cursors, if it has as
/// many as that and they are more than one; a final break makes no extra
/// line.
fn spread(pasted: &Rope, cursors: usize) -> Result<Option<Vec<Rope>>, TextError> {
    let ends_in_break = pasted
        .len()
        .checked_sub(1)
        .and_then(|last| {
            pasted
                .chars_in(pasted.floor_char_boundary(last)..pasted.len())
                .ok()
        })
        .and_then(|mut chars| chars.next())
        .is_some_and(|c| c == '\n' || c == '\r');
    let lines = pasted.len_lines() - usize::from(ends_in_break);
    if cursors < 2 || lines != cursors {
        return Ok(None);
    }
    let mut pieces = Vec::with_capacity(lines);
    for line in 0..lines {
        pieces.push(pasted.slice(pasted.line_span(line)?.content())?);
    }
    Ok(Some(pieces))
}

/// `count` as a signed count, saturating.
fn signed(count: usize) -> isize {
    isize::try_from(count).unwrap_or(isize::MAX)
}
