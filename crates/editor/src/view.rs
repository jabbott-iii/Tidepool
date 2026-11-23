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

//! The editor view: a [`Rope`]'s text with its gutter of line numbers,
//! cursors, selections, current line, soft wrap and scrolling, in a `ui`
//! [`Tree`] (M4.1, ADR-0063). Specification: `docs/subsystems/editor.md` →
//! "Editor view".
//!
//! New in Tidepool.
//!
//! [`EditorView::insert`] builds it under a node of the owner's, from the
//! top down:
//!
//! ```text
//! ┌──────────────────────────────────────────────┬──┐
//! │  1  fn main() {                              │▐ │ ← vertical scrollbar
//! │  2      println!("hi");▏                     │▐ │
//! │  3  }                                        │  │
//! │ gutter │ text                                │  │
//! ├──────────────────────────────────────────────┤  │
//! │ ▬▬▬▬▬▬                horizontal scrollbar   │  │
//! └──────────────────────────────────────────────┴──┘
//! ```
//!
//! - the view's **root** ([`EditorView::node`]), which fills its parent
//!   and paints `editor-bg` under the rest;
//! - the **text area** ([`EditorView::text_node`]), which takes focus and
//!   shows the text: a `TextInput` in the accessibility tree, with the name
//!   its owner gives, and no value, since a text can be any size;
//! - two of `ui`'s [`ScrollBar`]s ([`EditorView::scroll_bar`]),
//!   [`SCROLLBAR_THICKNESS`] thick, each named as the view is: the
//!   vertical one at the right, the horizontal one under the text area,
//!   hidden while soft wrap is on, when the text area takes its height.
//!
//! The owner sets the text and the selection ([`EditorView::set_text`],
//! [`EditorView::set_selection`]); a selection is checked against the
//! text, and one with a position past the end or inside a character is an
//! error, which changes nothing (ADR-0019). Editing comes with M4.2, and
//! the binary shows no view until M4.3 opens a file.
//!
//! # What it draws
//!
//! The text is set in [`EDITOR_TEXT`], JetBrains Mono 14 px with a line
//! height of 1.5, in rows a line high: 21 units with the bundled faces. In
//! the text area, back to front:
//!
//! 1. the **current line**: the `current-line` wash across each row of a
//!    line holding a caret (an empty cursor) and nothing selected, the
//!    gutter included, so that the wash is never under a selection;
//! 2. the **gutter**: each line's number, from 1, right-aligned in
//!    [`EDITOR_TEXT`] with [`GUTTER_PADDING`] either side, as wide as the
//!    last line's number and at least [`MIN_GUTTER_DIGITS`] digits; `text-3`,
//!    and `text-2` on a line holding a cursor's head. With soft wrap on,
//!    only a line's first row is numbered. The gutter does not scroll
//!    sideways;
//! 3. right of the gutter, cut to the text's area: each **selection**'s
//!    range in `teal-glow`, row by row, a selected line break as a space's
//!    width after the line's end;
//! 4. the **text** in `text`, one draw command for each run of shown text
//!    on a row (highlighting comes with M6.2);
//! 5. each **cursor** as a `teal` bar at its head, [`CURSOR_WIDTH`] wide
//!    and a row high, only while the text area has keyboard focus. It does
//!    not blink: motion is opt-in (`.claude/rules/ui-style.md`);
//! 6. the **focus outline** while its focus shows, which keyboard focus
//!    makes it do (`:focus-visible`: Tab shows it, a click does not): a
//!    `teal` border [`FOCUS_WIDTH`] wide over the text area's edges, inside
//!    its bounds as `ui`'s collections draw theirs, since the view fills
//!    its area and an outline outside it would be cut off.
//!
//! A position inside a grapheme cluster (what a reader sees as one
//! character, such as an `e` and the accent that combines with it) shows
//! at the start of the cluster, or of the marker it is in.
//!
//! # Text
//!
//! - A **tab** advances to the next stop, [`TAB_STOP_SPACES`] spaces' width
//!   apart, counted from the row's start; from a stop, it advances to the
//!   next.
//! - A **hidden character**, one that `ui`'s [`is_hidden`] says shows
//!   nothing or changes how the text around it reads, shows as a
//!   [`MARKER`], U+FFFD, in `error`, one for each: a control character
//!   (U+0000 to U+001F and U+007F to U+009F, NUL and NEL among them),
//!   an invisible or bidirectional formatting character such as U+200B or
//!   U+202E, the line and paragraph separators U+2028 and U+2029, and the
//!   interlinear annotation characters. The tab is shown as space, and the
//!   line breaks (LF, CR LF and a lone CR, which end lines, ADR-0019) as
//!   the end of their line. The flags of England, Scotland and Wales,
//!   which `ui`'s [`flag_at`] finds, stay whole. So code cannot hide or
//!   reorder what it says. The text itself is unchanged.
//! - A line longer than [`MAX_LINE_BYTES`] shows its start, up to the last
//!   grapheme cluster boundary at most that far in, then a
//!   [`CUT_MARKER`] in `text-3` where it is cut, so that no frame lays out
//!   more of a line than that. A position past the cut shows at the
//!   marker. Large-file mode is M4.3's.
//!
//! # Soft wrap
//!
//! Soft wrap is off at first, and [`EditorView::set_wrap`] turns it on or
//! off for one view (the setting comes with M4.7). Off, a line runs past
//! the right edge and the text scrolls sideways. On, a line wraps at the
//! text area's right edge, less [`CURSOR_WIDTH`] so that a caret at a row's
//! end shows: after the last space or tab of the row that fits, or, when
//! the row has none, at the grapheme cluster boundary before the first
//! cluster that does not fit. A row always holds at least one cluster. A
//! caret where a line wraps shows at the start of the next row.
//!
//! # Scrolling
//!
//! The view lays out only the lines it shows, and keeps their layouts
//! until the text, the wrap, the width lines wrap at or the metrics'
//! sizes change; it drops the layout of a line once a frame no longer
//! uses it. Its position ([`ScrollPosition`]) is the line at the top, the
//! row of that line, how far that row is scrolled up, and with soft wrap
//! off how far the text is scrolled left.
//!
//! - **The wheel** scrolls [`WHEEL_ROWS`] rows a notch, down for a
//!   positive `y`, and an exact distance as it is given. A horizontal
//!   delta, or a vertical one with Shift held, scrolls sideways, a notch
//!   [`WHEEL_ROWS`] rows' height; with soft wrap on, nothing scrolls
//!   sideways and such an event goes on to the view's ancestors. One event
//!   scrolls at most the text area's height (or width), so that it lays
//!   out no more than a screen of lines.
//! - **The vertical scrollbar** counts lines: its content is the number
//!   of lines, its view the rows that fit, and its offset the line at the
//!   top and the part of its rows scrolled past. So it needs no layout of
//!   lines off screen; with soft wrap on its thumb moves evenly by line
//!   however many rows each line wraps to, and can reach its end before
//!   the text does.
//! - **The horizontal scrollbar** counts units: its content is the widest
//!   line laid out since the layouts were last dropped, and room for a
//!   cursor after it, and its view the text's width.
//! - The view scrolls down until the last row is at its bottom, and no
//!   further.
//!
//! # Events and frames
//!
//! `ui` has no clock and a widget no metrics while it handles an event,
//! so the owner keeps the view in step:
//!
//! - [`EditorView::handle`] gives an event to the tree, then
//!   [`EditorView::update`]s the view with what came of it; an owner that
//!   routes events itself, as the shell does, calls
//!   [`EditorView::update`] with the response's actions.
//! - [`EditorView::update`] moves the text as the wheel and the scrollbars
//!   asked, and sets the scrollbars from where the text is. Call it after
//!   each event and after each frame, as a frame can resize the view or
//!   lay out a wider line; it returns whether there is a new frame to
//!   draw. An idle view changes nothing and draws nothing.
//! - For M4.2, [`EditorView::point_to_byte`] and
//!   [`EditorView::byte_to_point`] map between the window and the text, and
//!   [`EditorView::scroll_into_view`] scrolls a byte offset into view.
//!
//! Each takes the [`TextMetrics`] the frames are drawn with, so that the
//! view lays out what it maps or scrolls to as it draws it.
//!
//! # Example
//!
//! ```
//! use tidepool_core::geom::{Point, Size};
//! use tidepool_core::input::{InputEvent, Modifiers, ScrollDelta};
//! use tidepool_editor::view::EditorView;
//! use tidepool_text::{Rope, Selection};
//! use tidepool_ui::Tree;
//! use tidepool_ui::metrics::FixedMetrics;
//!
//! let text: String = (1..=100).map(|n| format!("let x{n} = {n};\n")).collect();
//! let mut tree = Tree::new();
//! let root = tree.root();
//! let view = EditorView::insert(&mut tree, root, "main.rs", Rope::from(text.as_str()))?;
//! view.set_selection(&mut tree, Selection::caret(4))?;
//! tree.set_focus(view.text_node(), false)?;
//!
//! let (size, metrics) = (Size::new(640.0, 480.0), FixedMetrics::DEFAULT);
//! assert!(tree.frame(size, &metrics).is_some());
//! view.update(&mut tree, &[], &metrics)?;
//!
//! // A notch of the wheel scrolls three rows.
//! let wheel = InputEvent::Wheel {
//!     delta: ScrollDelta::Lines { x: 0.0, y: 1.0 },
//!     position: Point::new(100.0, 100.0),
//!     modifiers: Modifiers::NONE,
//! };
//! view.handle(&mut tree, &wheel, &metrics)?;
//! assert_eq!(view.position(&tree)?.line, 3);
//! assert!(tree.frame(size, &metrics).is_some());
//! # Ok::<(), tidepool_editor::EditorError>(())
//! ```
//!
//! [`ScrollBar`]: tidepool_ui::widgets::ScrollBar
//! [`SCROLLBAR_THICKNESS`]: tidepool_ui::widgets::SCROLLBAR_THICKNESS
//! [`FOCUS_WIDTH`]: tidepool_ui::widgets::FOCUS_WIDTH
//! [`is_hidden`]: tidepool_ui::widgets::is_hidden
//! [`flag_at`]: tidepool_ui::widgets::flag_at

mod area;
mod layout;

use tidepool_core::geom::Point;
use tidepool_core::input::InputEvent;
use tidepool_text::{Rope, Selection};
use tidepool_ui::a11y::{Role, Semantics};
use tidepool_ui::draw::{DrawList, PaintContext};
use tidepool_ui::input::{Action, ActionKind, Response};
use tidepool_ui::layout::{Axis, Style};
use tidepool_ui::metrics::{Family, TextMetrics, TextStyle};
use tidepool_ui::theme::Token;
use tidepool_ui::widgets::{self, Control, ScrollBar};
use tidepool_ui::{Node, NodeId, Tree, UiError, Widget};

use crate::EditorError;

use area::{Area, BarRange};

/// The text, its line numbers and its markers: JetBrains Mono 14 px, 400,
/// line height 1.5, as written (the spec's "Code editor" typography).
pub const EDITOR_TEXT: TextStyle = TextStyle::new(Family::Mono, 14.0);

/// The space either side of the line numbers in the gutter: left of the
/// widest, and between them and the text.
pub const GUTTER_PADDING: f32 = 12.0;

/// The fewest digits the gutter is wide enough for, so that it keeps its
/// width while a file grows to 999 lines.
pub const MIN_GUTTER_DIGITS: usize = 3;

/// The width of a cursor's `teal` bar: twice a field's caret
/// ([`CARET_WIDTH`](tidepool_ui::widgets::CARET_WIDTH)), so that it reads
/// at the code's larger size.
pub const CURSOR_WIDTH: f32 = 2.0;

/// How many spaces' width apart tab stops are.
pub const TAB_STOP_SPACES: usize = 4;

/// How many rows a notch of the wheel scrolls.
pub const WHEEL_ROWS: f32 = 3.0;

/// The most bytes of a line the view lays out: the rest is cut, at a
/// grapheme cluster boundary, and shows as [`CUT_MARKER`]. The same bound
/// as Visual Studio Code's default `editor.stopRenderingLineAfter` (10,000
/// characters), counted in bytes so that what a frame lays out is bounded
/// whatever the characters are. In JetBrains Mono's 8.4-unit advance, that
/// is about 44 times the width of a view 1,920 units wide with soft wrap
/// off, or 44 of its rows with soft wrap on. Laid out, a line this long of
/// ASCII text takes about 250 KB: a 24-byte piece for each grapheme
/// cluster, and the text.
pub const MAX_LINE_BYTES: usize = 10_000;

/// What a hidden character shows as: U+FFFD, the replacement character, in
/// `error`.
pub const MARKER: char = '\u{FFFD}';

/// What shows where a line longer than [`MAX_LINE_BYTES`] is cut: U+2026,
/// the ellipsis, in `text-3`.
pub const CUT_MARKER: &str = "\u{2026}";

/// Where the view is scrolled to ([`EditorView::position`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollPosition {
    /// The line at the top of the view, from 0.
    pub line: usize,
    /// The row of that line at the top, from 0: always 0 with soft wrap off.
    pub row: usize,
    /// How far that row is scrolled up past the view's top, in units: from
    /// 0 up to a row's height.
    pub y: f32,
    /// How far the text is scrolled left, in units: always 0 with soft wrap
    /// on.
    pub x: f32,
}

/// The editor view in a tree: the ids of its nodes (the
/// [module documentation](self)). Its state lives in the tree, so the
/// handle is `Copy`, and every method takes the tree it was built in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EditorView {
    root: NodeId,
    text: NodeId,
    vertical: NodeId,
    horizontal: NodeId,
}

impl EditorView {
    /// Builds a view of `text` as the last child of `parent`, named `name`
    /// in the accessibility tree (a file's name, as written), with a caret
    /// at the start, scrolled to the top, and soft wrap off.
    ///
    /// # Errors
    ///
    /// The tree's errors: [`UiError::Unnamed`] for a blank name,
    /// [`UiError::UnknownNode`] for an unknown parent, and those of
    /// [`Tree::insert`]. The tree is then as it was.
    pub fn insert(
        tree: &mut Tree,
        parent: NodeId,
        name: impl Into<String>,
        text: Rope,
    ) -> Result<Self, EditorError> {
        let name = name.into();
        let root = tree.insert(parent, Node::new(Style::row().fill()).with_widget(Ground))?;
        match Self::build(tree, root, name, text) {
            Ok(view) => Ok(view),
            Err(error) => {
                // Just inserted, and not the root, so it can be removed.
                let _ = tree.remove(root);
                Err(error.into())
            }
        }
    }

    /// Builds the view's nodes under `root`.
    fn build(tree: &mut Tree, root: NodeId, name: String, text: Rope) -> Result<Self, UiError> {
        let column = tree.insert(root, Node::new(Style::column().fill()))?;
        let text = tree.insert(
            column,
            Node::new(Style::default().fill())
                .with_semantics(Semantics::new(Role::TextInput, name.clone()))
                .focusable()
                .with_widget(Area::new(text)),
        )?;
        let horizontal = tree.insert(
            column,
            ScrollBar::new(Axis::Horizontal, name.clone()).node(),
        )?;
        let vertical = tree.insert(root, ScrollBar::new(Axis::Vertical, name).node())?;
        Ok(Self {
            root,
            text,
            vertical,
            horizontal,
        })
    }

    /// The view's root, which the owner places and removes.
    pub const fn node(&self) -> NodeId {
        self.root
    }

    /// The text area's node, which takes focus.
    pub const fn text_node(&self) -> NodeId {
        self.text
    }

    /// The scrollbar on `axis`.
    pub const fn scroll_bar(&self, axis: Axis) -> NodeId {
        match axis {
            Axis::Vertical => self.vertical,
            Axis::Horizontal => self.horizontal,
        }
    }

    /// The text area's widget.
    fn area<'t>(&self, tree: &'t Tree) -> Result<&'t Area, EditorError> {
        tree.widget::<Area>(self.text)
            .ok_or_else(|| self.missing(tree, self.text))
    }

    /// The text area's widget, to change: it is repainted at the next
    /// frame.
    fn area_mut<'t>(&self, tree: &'t mut Tree) -> Result<&'t mut Area, EditorError> {
        if tree.widget::<Area>(self.text).is_none() {
            return Err(self.missing(tree, self.text));
        }
        tree.widget_mut::<Area>(self.text)
            .ok_or(EditorError::Ui(UiError::WrongWidget { id: self.text }))
    }

    /// The error for `id`, which holds none of the view's widgets.
    fn missing(&self, tree: &Tree, id: NodeId) -> EditorError {
        EditorError::Ui(if tree.contains(id) {
            UiError::WrongWidget { id }
        } else {
            UiError::UnknownNode { id }
        })
    }

    /// The text it shows.
    ///
    /// # Errors
    ///
    /// [`UiError::UnknownNode`] or [`UiError::WrongWidget`] if `tree` does
    /// not hold the view.
    pub fn text<'t>(&self, tree: &'t Tree) -> Result<&'t Rope, EditorError> {
        self.area(tree).map(Area::text)
    }

    /// Its selection.
    ///
    /// # Errors
    ///
    /// As for [`text`](Self::text).
    pub fn selection<'t>(&self, tree: &'t Tree) -> Result<&'t Selection, EditorError> {
        self.area(tree).map(Area::selection)
    }

    /// Whether soft wrap is on.
    ///
    /// # Errors
    ///
    /// As for [`text`](Self::text).
    pub fn wrap(&self, tree: &Tree) -> Result<bool, EditorError> {
        self.area(tree).map(Area::wrap)
    }

    /// Where it is scrolled to, as the last [`update`](Self::update) or
    /// frame left it.
    ///
    /// # Errors
    ///
    /// As for [`text`](Self::text).
    pub fn position(&self, tree: &Tree) -> Result<ScrollPosition, EditorError> {
        self.area(tree).map(Area::position)
    }

    /// Shows `text` with `selection`, dropping the layouts kept of the old
    /// text; the position is kept, moved up if the text is shorter.
    ///
    /// # Errors
    ///
    /// [`EditorError::Text`] if a position of `selection` is past the end
    /// of `text` or inside a character; as for [`text`](Self::text)
    /// otherwise. Nothing changes then.
    pub fn set_text(
        &self,
        tree: &mut Tree,
        text: Rope,
        selection: Selection,
    ) -> Result<(), EditorError> {
        selection.validate(&text)?;
        self.area_mut(tree)?.set_text(text, selection);
        Ok(())
    }

    /// Shows `text` with `selection`, as [`set_text`](Self::set_text) does,
    /// for a selection already checked against `text`, as a `History`
    /// step's is (M4.2), so that a step with many cursors is not checked
    /// twice.
    ///
    /// # Errors
    ///
    /// As for [`text`](Self::text).
    pub(crate) fn show(
        &self,
        tree: &mut Tree,
        text: Rope,
        selection: Selection,
    ) -> Result<(), EditorError> {
        self.area_mut(tree)?.set_text(text, selection);
        Ok(())
    }

    /// Sets the selection.
    ///
    /// # Errors
    ///
    /// [`EditorError::Text`] if a position of `selection` is past the end
    /// of the text or inside a character; as for [`text`](Self::text)
    /// otherwise. Nothing changes then.
    pub fn set_selection(&self, tree: &mut Tree, selection: Selection) -> Result<(), EditorError> {
        selection.validate(self.text(tree)?)?;
        self.area_mut(tree)?.set_selection(selection);
        Ok(())
    }

    /// Turns soft wrap on or off: the top row becomes its line's first,
    /// and with wrap on the text no longer scrolls sideways and the
    /// horizontal scrollbar is hidden.
    ///
    /// # Errors
    ///
    /// As for [`text`](Self::text).
    pub fn set_wrap(&self, tree: &mut Tree, wrap: bool) -> Result<(), EditorError> {
        if self.wrap(tree)? == wrap {
            return Ok(());
        }
        self.area_mut(tree)?.set_wrap(wrap);
        let id = self.horizontal;
        let style = *tree.style(id).ok_or(UiError::UnknownNode { id })?;
        tree.set_style(
            id,
            Style {
                hidden: wrap,
                ..style
            },
        )?;
        Ok(())
    }

    /// Gives `event` to `tree`, then [`update`](Self::update)s the view
    /// with the actions that came of it; returns the tree's response, for
    /// the owner to handle what it left.
    ///
    /// # Errors
    ///
    /// As for [`update`](Self::update).
    pub fn handle(
        &self,
        tree: &mut Tree,
        event: &InputEvent,
        metrics: &dyn TextMetrics,
    ) -> Result<Response, EditorError> {
        let response = tree.handle(event);
        self.update(tree, &response.actions, metrics)?;
        Ok(response)
    }

    /// Keeps the view in step (the [module
    /// documentation](self#events-and-frames)): moves the text as its
    /// scrollbars' actions among `actions` and the wheel asked, within the
    /// text, and sets the scrollbars from where the text is, in the bounds
    /// of the last layout, measured with `metrics`. Returns whether it
    /// changed anything, after which the tree has a new frame to draw.
    /// Before the view is first laid out, it waits.
    ///
    /// # Errors
    ///
    /// As for [`text`](Self::text), for any of the view's nodes.
    pub fn update(
        &self,
        tree: &mut Tree,
        actions: &[Action],
        metrics: &dyn TextMetrics,
    ) -> Result<bool, EditorError> {
        for action in actions
            .iter()
            .filter(|action| action.kind == ActionKind::Change)
        {
            if action.node == self.vertical {
                let offset = self.bar(tree, self.vertical)?.offset();
                self.area(tree)?.jump(offset);
            } else if action.node == self.horizontal {
                let offset = self.bar(tree, self.horizontal)?.offset();
                self.area(tree)?.slide(offset);
            }
        }
        let bounds = tree
            .bounds(self.text)
            .ok_or(UiError::UnknownNode { id: self.text })?;
        let area = self.area(tree)?;
        if !(bounds.width > 0.0 && bounds.height > 0.0) {
            return Ok(false);
        }
        let (moved, _, g) = area.resolve(bounds, metrics);
        let (vertical, horizontal) = area.bars(&g, metrics);
        let wrap = area.wrap();
        if moved {
            // Repainted at the next frame.
            self.area_mut(tree)?;
        }
        let mut changed = moved;
        changed |= self.set_bar(tree, self.vertical, vertical)?;
        if !wrap {
            changed |= self.set_bar(tree, self.horizontal, horizontal)?;
        }
        Ok(changed)
    }

    /// The scrollbar at `id`.
    fn bar<'t>(&self, tree: &'t Tree, id: NodeId) -> Result<&'t ScrollBar, EditorError> {
        tree.widget::<ScrollBar>(id)
            .ok_or_else(|| self.missing(tree, id))
    }

    /// Gives the scrollbar at `id` `range`, if that changes it; returns
    /// whether it did.
    fn set_bar(&self, tree: &mut Tree, id: NodeId, range: BarRange) -> Result<bool, EditorError> {
        let (content, viewport, offset) = range;
        let current = self.bar(tree, id)?;
        let mut wanted = current.clone();
        wanted.set_range(content, viewport);
        wanted.set_offset(offset);
        if wanted == *current {
            return Ok(false);
        }
        widgets::update::<ScrollBar, _>(tree, id, |bar| {
            bar.set_range(content, viewport);
            bar.set_offset(offset);
        })?;
        Ok(true)
    }

    /// The byte offset of the grapheme cluster boundary nearest `point`, in
    /// window coordinates: on the row on screen nearest it, which for a
    /// point past the last row is the last, and of that row's boundaries,
    /// the nearest across. Left of the text, as in the gutter, that is the
    /// row's start. A row's end is its last boundary, which on a row that
    /// wraps is the next row's start, and on a line cut at
    /// [`MAX_LINE_BYTES`], the cut. Of two boundaries at the same place, the
    /// first.
    ///
    /// # Errors
    ///
    /// As for [`text`](Self::text).
    pub fn point_to_byte(
        &self,
        tree: &Tree,
        point: Point,
        metrics: &dyn TextMetrics,
    ) -> Result<usize, EditorError> {
        let bounds = tree
            .bounds(self.text)
            .ok_or(UiError::UnknownNode { id: self.text })?;
        Ok(self.area(tree)?.point_to_byte(bounds, point, metrics))
    }

    /// Where a caret at `byte` shows, in window coordinates: the top-left
    /// of its place, a row high, if its row is on screen (it may be left
    /// or right of the text's area, with soft wrap off); `None` if it is
    /// not. A position inside a grapheme cluster shows at the cluster's
    /// start, past a cut at the cut marker, and where a line wraps at the
    /// next row's start, so that for each boundary on screen,
    /// [`point_to_byte`](Self::point_to_byte) of the point gives it back.
    ///
    /// # Errors
    ///
    /// [`EditorError::Text`] if `byte` is past the end of the text or
    /// inside a character; as for [`text`](Self::text) otherwise.
    pub fn byte_to_point(
        &self,
        tree: &Tree,
        byte: usize,
        metrics: &dyn TextMetrics,
    ) -> Result<Option<Point>, EditorError> {
        let bounds = tree
            .bounds(self.text)
            .ok_or(UiError::UnknownNode { id: self.text })?;
        Ok(self.area(tree)?.byte_to_point(bounds, byte, metrics)?)
    }

    /// Scrolls the least that shows a caret at `byte` whole, in the bounds
    /// of the last layout: its row, at the top if it is above the view and
    /// at the bottom if it is below, and with soft wrap off its place
    /// across, then keeps the scrollbars in step. Returns whether it
    /// scrolled.
    ///
    /// # Errors
    ///
    /// As for [`byte_to_point`](Self::byte_to_point).
    pub fn scroll_into_view(
        &self,
        tree: &mut Tree,
        byte: usize,
        metrics: &dyn TextMetrics,
    ) -> Result<bool, EditorError> {
        let bounds = tree
            .bounds(self.text)
            .ok_or(UiError::UnknownNode { id: self.text })?;
        let moved = self.area(tree)?.reveal(bounds, byte, metrics)?;
        if moved {
            self.area_mut(tree)?;
        }
        self.update(tree, &[], metrics)?;
        Ok(moved)
    }
}

/// The view's root: `editor-bg` under the text area and the scrollbars,
/// which paint no track at rest.
struct Ground;

impl Widget for Ground {
    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        list.rect(cx.bounds(), cx.theme().color(Token::EditorBg));
    }
}
