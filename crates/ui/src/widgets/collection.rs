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

//! Collections (ADR-0052): the list, the tree view and the table
//! (`.prose table`), which keep nodes only for the rows on screen and a
//! margin, over a model of [`Rows`] their owner provides.

use std::any::Any;
use std::fmt;
use std::ops::Range;

use tidepool_core::geom::{Point, Rect, Size};
use tidepool_core::input::{InputEvent, Key, MouseButton, ScrollDelta, TextLength};

use crate::a11y::{Role, Semantics};
use crate::draw::{DrawList, PaintContext};
use crate::input::{ActionKind, EventContext, Handled, SCROLL_LINE};
use crate::layout::{Insets, MAX_EXTENT, Style, length};
use crate::metrics::TextMetrics;
use crate::theme::Token;
use crate::tree::{NodeId, Tree, Widget};

use super::{
    CHEVRON_WIDTH, Control, FOCUS_WIDTH, MAX_COLUMNS, MAX_VIEW_ROWS, ROW_HEIGHT, ROW_MARGIN,
    ROW_PADDING, TREE_INDENT,
};

mod window;

/// What a collection shows: its owner's rows, which it asks for one at a
/// time.
///
/// A collection asks for [`len`](Self::len) each time it binds its rows,
/// and for [`row`](Self::row) for the rows it keeps a node for, those on
/// screen and [`ROW_MARGIN`](super::ROW_MARGIN) either side, as they come
/// into view or after its owner says the model changed; and a tree view
/// asks for its selected row and the one after it as Left or Right goes
/// down, which may be out of view (the WAI-ARIA tree pattern acts on the
/// selection wherever it is). It asks on the main thread, as the tree is
/// laid out or an event is handled, so both answer from what the owner
/// holds in memory and never wait on a file, a process or a server
/// (`AGENTS.md` rule 6): an owner whose rows come from elsewhere shows
/// what it has, and tells the collection when more arrives.
///
/// What a row on screen shows goes into the accessibility tree as its
/// name, as written: a list's or tree view's label, and each cell of a
/// table that has a column wider than 0, cut to
/// [`MAX_CELL_BYTES`](super::MAX_CELL_BYTES) as it is shown. So a row
/// holds no secret, and keeps what it does not show out of its cells.
///
/// Rows are placed by an `f64` offset in units, exact up to 2^53 units,
/// so past about 2^48 rows (2^53 ÷ 20) a row's place, and so which row a
/// key shows, is approximate.
///
/// `Vec<Row>` and `Vec<String>` are models. A model that makes its rows
/// as they are asked for holds a million rows in no more memory than it
/// needs to make one:
///
/// ```
/// use tidepool_ui::widgets::{Row, Rows};
///
/// /// Numbered lines, made as they are asked for.
/// struct Lines(usize);
///
/// impl Rows for Lines {
///     fn len(&self) -> usize {
///         self.0
///     }
///
///     fn row(&self, index: usize) -> Row {
///         Row::new(format!("line {}", index + 1))
///     }
/// }
///
/// let lines = Lines(1_000_000);
/// assert_eq!(lines.row(999_999).label(), "line 1000000");
/// assert!(!lines.is_empty());
/// ```
pub trait Rows: Any {
    /// How many rows there are.
    fn len(&self) -> usize;

    /// Whether there are none.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// What row `index` shows; asked only for an index below
    /// [`len`](Self::len).
    fn row(&self, index: usize) -> Row;
}

impl Rows for Vec<Row> {
    fn len(&self) -> usize {
        self.as_slice().len()
    }

    fn row(&self, index: usize) -> Row {
        self.get(index).cloned().unwrap_or_default()
    }
}

impl Rows for Vec<String> {
    fn len(&self) -> usize {
        self.as_slice().len()
    }

    fn row(&self, index: usize) -> Row {
        self.get(index)
            .map_or_else(Row::default, |label| Row::new(label.clone()))
    }
}

/// What one row shows: its text, a cell for each column of a table and
/// one for a list or tree view, as written; and in a tree view, its level,
/// whether it is expanded and its parent row.
///
/// What a row shows is the user's content, a file name, a path or a
/// message, so it is never case-transformed, and its `Debug` shows each
/// cell's length only (ADR-0043). A collection shows and names at most
/// [`MAX_CELL_BYTES`](super::MAX_CELL_BYTES) of each cell, cut at a
/// grapheme cluster boundary; what it shows of a row on screen is its
/// name in the accessibility tree (see [`Rows`]).
///
/// ```
/// use tidepool_ui::widgets::Row;
///
/// // `src`, the second row of an explorer, expanded, under the first.
/// let src = Row::new("src").depth(1).expanded(true).parent(0);
/// assert_eq!(src.label(), "src");
/// assert_eq!((src.level(), src.expansion(), src.parent_row()), (1, Some(true), Some(0)));
///
/// // A row of a table of problems.
/// let problem = Row::cells(["error", "mismatched types", "src/main.rs"]);
/// assert_eq!(problem.cell(1), "mismatched types");
/// assert_eq!(problem.cell(7), "");
/// ```
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Row {
    cells: Vec<String>,
    depth: u16,
    expanded: Option<bool>,
    parent: Option<usize>,
}

impl Row {
    /// A row of one cell, `label`: a list's or tree view's row.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            cells: vec![label.into()],
            ..Self::default()
        }
    }

    /// A row of `cells`, one for each column of a table, in order.
    pub fn cells<S: Into<String>>(cells: impl IntoIterator<Item = S>) -> Self {
        Self {
            cells: cells.into_iter().map(Into::into).collect(),
            ..Self::default()
        }
    }

    /// The same, `depth` levels below the tree view's top.
    #[must_use]
    pub fn depth(self, depth: u16) -> Self {
        Self { depth, ..self }
    }

    /// The same, a tree view's row that has children, expanded or not; a
    /// row without this is a leaf.
    #[must_use]
    pub fn expanded(self, expanded: bool) -> Self {
        Self {
            expanded: Some(expanded),
            ..self
        }
    }

    /// The same, under the row at `parent`, which Left selects in a tree
    /// view.
    #[must_use]
    pub fn parent(self, parent: usize) -> Self {
        Self {
            parent: Some(parent),
            ..self
        }
    }

    /// Its first cell, a list's or tree view's text; empty if it has none.
    pub fn label(&self) -> &str {
        self.cell(0)
    }

    /// The cell of `column`, as written; empty if it has none.
    pub fn cell(&self, column: usize) -> &str {
        self.cells.get(column).map_or("", String::as_str)
    }

    /// Its cells, in order.
    pub fn cell_texts(&self) -> &[String] {
        &self.cells
    }

    /// How many levels below the tree view's top it is.
    pub const fn level(&self) -> u16 {
        self.depth
    }

    /// Whether it is expanded, if it has children.
    pub const fn expansion(&self) -> Option<bool> {
        self.expanded
    }

    /// The row it is under in a tree view, if it says.
    pub const fn parent_row(&self) -> Option<usize> {
        self.parent
    }
}

/// Shows each cell as its length only (ADR-0043).
impl fmt::Debug for Row {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let cells: Vec<TextLength<'_>> = self.cells.iter().map(|cell| TextLength(cell)).collect();
        f.debug_struct("Row")
            .field("cells", &cells)
            .field("depth", &self.depth)
            .field("expanded", &self.expanded)
            .field("parent", &self.parent)
            .finish()
    }
}

/// A column of a table: its title, painted uppercase in
/// [`TABLE_HEADER`](super::TABLE_HEADER) and named as written, and its
/// width, which the owner gives.
///
/// ```
/// use tidepool_ui::widgets::Column;
///
/// let message = Column::new("Message", 320.0);
/// assert_eq!((message.title(), message.width()), ("Message", 320.0));
/// // A width that is NaN or negative is 0.
/// assert_eq!(Column::new("Line", f32::NAN).width(), 0.0);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Column {
    title: String,
    width: f32,
}

impl Column {
    /// A column titled `title`, `width` units wide: NaN or negative is 0,
    /// and above [`MAX_EXTENT`] is [`MAX_EXTENT`].
    pub fn new(title: impl Into<String>, width: f32) -> Self {
        Self {
            title: title.into(),
            width: length(width),
        }
    }

    /// Its title, as written.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Its width.
    pub const fn width(&self) -> f32 {
        self.width
    }
}

/// Which collection it is, which sets its role, its rows' roles and how
/// its rows look.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CollectionKind {
    /// A list: rows of one line of text.
    #[default]
    List,
    /// A tree view, such as the file explorer: rows indented by their
    /// level, with a chevron on a row that has children.
    TreeView,
    /// A table: a header of column titles over rows of cells.
    Table,
}

/// Where a collection's rows are, from the last time it bound them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct View {
    /// Whether this widget's header and body were built: a widget put in
    /// another's place builds its own (ADR-0052).
    built: bool,
    /// The content box of the node its rows show in, from its bounds'
    /// origin.
    body: Rect,
    /// The height its rows show in: the body's, at most
    /// [`MAX_VIEW_ROWS`] rows'.
    area: f32,
    /// The part of the body on screen, from the body's top: its content
    /// box cut to its clip.
    band: (f64, f64),
    /// The offset it bound its rows at, which the rows on screen show.
    shown: f64,
    /// How far down the body the rows it bound reach.
    reach: f64,
    /// What it bound, so that it asks for rows again only when that
    /// changes.
    bound: Option<Bound>,
    /// Where it placed its rows: the first, how many nodes, and the bits
    /// of the offset and the reach, so that it places them again only when
    /// that changes.
    placed: Option<(usize, usize, u64, u64)>,
}

/// What a collection bound its row nodes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Bound {
    first: usize,
    count: usize,
    revision: u64,
    selected: Option<usize>,
}

/// The rows a collection keeps a node for: `count` from `first`, in rows
/// that reach `reach` units down its body.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Window {
    first: usize,
    count: usize,
    reach: f64,
}

/// A list, a tree view or a table (ADR-0052): a collection of rows from
/// its owner's [`Rows`], with a selection of one row that it keeps.
///
/// # Rows
///
/// Its rows are all [`ROW_HEIGHT`] high, and show in its view: its body,
/// at most [`MAX_VIEW_ROWS`] rows high (10,240 units, taller than any
/// window); rows that overflow it by less than 1/64 of a unit, as the
/// tree's fitting of sizes in `f32` can leave them, do not scroll. It keeps a node only for each row of its view on screen, the
/// part of the body its clipping ancestors and the window show, and
/// [`ROW_MARGIN`] either side within the view, so at most
/// [`MAX_VIEW_ROWS`] and two, however it is laid out, inside a scroller
/// too; and it binds those nodes to other rows as it scrolls. Which rows
/// those are is arithmetic, so laying it out or painting it costs as much
/// for a million rows as for a page. It asks its model for a row only as
/// the row comes on screen or after the owner says the model changed.
///
/// It binds its rows as the tree is laid out, from the last layout's
/// placements and again after each pass, so its rows, their semantics and
/// their bounds follow a change at the next layout or frame. Its node's
/// children are its own: a body its rows show in, and a table's header
/// row before it; a widget put in its place builds its own. It fills its
/// parent by default; with
/// [`Length::Content`](crate::layout::Length::Content) it is as tall as
/// its rows and header, up to [`MAX_VIEW_ROWS`] rows.
///
/// It keeps its own scroll offset, in units (an `f64`, so that a million
/// rows scroll to the unit), and places its rows by it: the wheel over it
/// moves it, and what it cannot take goes on to the scrollers around it.
/// Its body does not scroll in the tree's own terms, so
/// [`Tree::set_scroll_offset`] does not apply to it; use
/// [`scroll_to`](Self::scroll_to).
///
/// # Its owner's model
///
/// The owner changes its model through [`model_mut`](Self::model_mut) or
/// [`set_model`](Self::set_model), with [`widgets::update`](super::update),
/// and says what changed with [`rows_inserted`](Self::rows_inserted) and
/// [`rows_removed`](Self::rows_removed), so that the selection stays on its
/// row; a change it does not describe keeps the selection's index, cut to
/// the rows there are. The selection is always a row of the model, or
/// none.
///
/// # Keys and the pointer
///
/// While it has focus, with no modifier: Up and Down move the selection a
/// row, Page Up and Page Down by the rows wholly on screen, Home and End
/// to the first and last row, stopping at the ends, and each scrolls it
/// into view at the next layout: the least move of its own offset brings
/// it into its view, and the scrollers around it bring it on screen, as
/// [`Tree::scroll_into_view`] does;
/// from no selection, any of them but End selects the first row. Enter
/// emits [`ActionKind::Activate`] for the selected row. In a tree view,
/// Right expands a collapsed row (it emits [`ActionKind::Expand`]) or
/// moves to an expanded row's first child, and Left collapses an expanded
/// row ([`ActionKind::Collapse`]) or moves to the row's parent, as the
/// WAI-ARIA tree pattern does; the owner expands and collapses its model.
/// A press on a row selects it, and on a tree item's chevron expands or
/// collapses it too. Each that moves the selection emits
/// [`ActionKind::Select`]. Every action is of the collection's node, about
/// its selected row. Other keys, and keys with a modifier, go on to the
/// owner.
///
/// A table's column header takes focus, and emits
/// [`ActionKind::Activate`] of its own node when it is clicked, or when
/// Enter goes down or Space comes up on it, as a button does; the owner
/// finds its column with [`Collection::column_of`] and sorts its model.
///
/// # Accessibility
///
/// It is a list, tree view or table, named as given, its value the row
/// count and the selected row (`"row 2 of 3"`, `"3 rows"`), and it takes
/// focus. The rows on screen are list items, tree items (expanded or
/// collapsed) or rows, named by what they show, as written: a table's row
/// by the cells its columns wider than 0 show, each a cell, after a header
/// row of column headers. Only the rows it keeps a node for, on screen and
/// in the margin, are in the accessibility tree; the value says where they
/// are. A row whose text is blank is named by its place, `"row 7"`. A
/// column header takes focus if it has a title and is wider than 0.
///
/// # Looks
///
/// Its rows paint over what is behind it. A row's text is [`ROW_TEXT`],
/// in `text`; a hovered row has the `row-hover` wash and the selected row
/// is `teal-glow`. A tree item is indented [`TREE_INDENT`] a level, with a
/// `text-2` chevron. A table's header row is `table-head`, with a 1-unit
/// `teal` underline and uppercase `teal` titles, `teal-hi` when hovered,
/// on `slate` when pressed; its rows are ruled in `line`. When focus
/// shows, its focus outline is drawn inside its bounds, in the
/// [`FOCUS_WIDTH`] of padding its default style leaves, since it usually
/// fills its panel or the window, which would cut an outline outside it
/// off; a column header's is inside its bounds too. Disabled, its rows
/// have their rest look at [`DISABLED_OPACITY`].
///
/// [`ROW_TEXT`]: super::ROW_TEXT
/// [`DISABLED_OPACITY`]: super::DISABLED_OPACITY
///
/// ```
/// use tidepool_core::geom::Size;
/// use tidepool_core::input::{InputEvent, Key, Modifiers};
/// use tidepool_ui::input::ActionKind;
/// use tidepool_ui::metrics::FixedMetrics;
/// use tidepool_ui::widgets::{self, Collection, Control};
/// use tidepool_ui::Tree;
///
/// let files: Vec<String> = ["Cargo.toml", "README.md", "src"].map(String::from).to_vec();
/// let mut tree = Tree::new();
/// let list = tree.insert(tree.root(), Collection::list("Explorer", files).node())?;
/// let window = Size::new(200.0, 100.0);
/// assert!(tree.frame(window, &FixedMetrics::DEFAULT).is_some());
///
/// // Down selects the first row, and again the next.
/// tree.set_focus(list, true)?;
/// let down = InputEvent::KeyDown { key: Key::Down, modifiers: Modifiers::NONE, repeat: false };
/// tree.handle(&down);
/// let response = tree.handle(&down);
/// assert_eq!(response.actions[0].kind, ActionKind::Select);
/// assert_eq!(tree.widget::<Collection>(list).and_then(Collection::selected), Some(1));
///
/// tree.frame(window, &FixedMetrics::DEFAULT);
/// assert_eq!(
///     tree.accessibility().to_string(),
///     "list \"Explorer\" = \"row 2 of 3\" focusable focused\n  list item \"Cargo.toml\"\n  \
///      list item \"README.md\" selected\n  list item \"src\"\n"
/// );
///
/// // The owner selects a row, which it may have just added.
/// widgets::update(&mut tree, list, |list: &mut Collection| list.select(Some(2)))?;
/// assert_eq!(tree.widget::<Collection>(list).and_then(Collection::selected), Some(2));
/// # Ok::<(), tidepool_ui::UiError>(())
/// ```
pub struct Collection {
    kind: CollectionKind,
    name: String,
    columns: Vec<Column>,
    model: Box<dyn Rows>,
    /// The model's length as the collection last knew it.
    len: usize,
    selected: Option<usize>,
    /// How far its rows are scrolled, in units.
    offset: f64,
    /// Whether the next binding scrolls the selection into view.
    reveal: bool,
    /// Counts the owner's changes to the model.
    revision: u64,
    view: View,
}

impl Collection {
    /// A list named `name` of `model`'s rows, with no row selected.
    pub fn list(name: impl Into<String>, model: impl Rows) -> Self {
        Self::of(
            CollectionKind::List,
            name.into(),
            Vec::new(),
            Box::new(model),
        )
    }

    /// A tree view named `name` of `model`'s rows, flattened: each row
    /// that is shown, in order, with its [level](Row::depth), whether it
    /// is [expanded](Row::expanded) and its [parent](Row::parent).
    pub fn tree_view(name: impl Into<String>, model: impl Rows) -> Self {
        Self::of(
            CollectionKind::TreeView,
            name.into(),
            Vec::new(),
            Box::new(model),
        )
    }

    /// A table named `name`, of `columns` (at most [`MAX_COLUMNS`]; more
    /// are left out) over `model`'s rows. Columns wider in all than the
    /// table are made narrower in proportion.
    pub fn table(
        name: impl Into<String>,
        columns: impl IntoIterator<Item = Column>,
        model: impl Rows,
    ) -> Self {
        let columns = columns.into_iter().take(MAX_COLUMNS).collect();
        Self::of(CollectionKind::Table, name.into(), columns, Box::new(model))
    }

    fn of(kind: CollectionKind, name: String, columns: Vec<Column>, model: Box<dyn Rows>) -> Self {
        Self {
            kind,
            name,
            columns,
            len: model.len(),
            model,
            selected: None,
            offset: 0.0,
            reveal: false,
            revision: 0,
            view: View::default(),
        }
    }

    /// Which collection it is.
    pub const fn kind(&self) -> CollectionKind {
        self.kind
    }

    /// Its name, as written.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Names it `name`.
    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    /// A table's columns; none for a list or tree view.
    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    /// How many rows its model has.
    pub fn len(&self) -> usize {
        self.model.len()
    }

    /// Whether its model has no rows.
    pub fn is_empty(&self) -> bool {
        self.model.is_empty()
    }

    /// Its model.
    pub fn model(&self) -> &dyn Rows {
        self.model.as_ref()
    }

    /// Its model, if it is an `M`.
    pub fn model_as<M: Rows>(&self) -> Option<&M> {
        (self.model.as_ref() as &dyn Any).downcast_ref()
    }

    /// Its model, if it is an `M`, to change; the collection asks for its
    /// rows on screen again at the next layout. Say what changed with
    /// [`rows_inserted`](Self::rows_inserted) or
    /// [`rows_removed`](Self::rows_removed), so that the selection stays
    /// on its row.
    pub fn model_mut<M: Rows>(&mut self) -> Option<&mut M> {
        self.revision = self.revision.wrapping_add(1);
        (self.model.as_mut() as &mut dyn Any).downcast_mut()
    }

    /// Shows `model` in place of its model, scrolled to its start with no
    /// row selected.
    pub fn set_model(&mut self, model: impl Rows) {
        self.model = Box::new(model);
        self.len = self.model.len();
        self.selected = None;
        self.offset = 0.0;
        self.reveal = false;
        self.revision = self.revision.wrapping_add(1);
    }

    /// Says that `count` rows were inserted into the model at `at`, cut to
    /// the rows it knew of: a selection at or after `at` moves down with
    /// its row.
    pub fn rows_inserted(&mut self, at: usize, count: usize) {
        let at = at.min(self.len);
        self.len = self.len.saturating_add(count);
        if let Some(selected) = self.selected
            && selected >= at
        {
            self.selected = Some(selected.saturating_add(count));
        }
        self.revision = self.revision.wrapping_add(1);
    }

    /// Says that the model's `rows` were removed, cut to the rows it knew
    /// of: a selection after them moves up with its row, and a selection
    /// among them moves to the row now in its place, or the last row, or
    /// none if no row is left.
    pub fn rows_removed(&mut self, rows: Range<usize>) {
        let start = rows.start.min(self.len);
        let end = rows.end.clamp(start, self.len);
        let removed = end - start;
        self.len -= removed;
        if let Some(selected) = self.selected {
            self.selected = if selected >= end {
                Some(selected - removed)
            } else if selected >= start {
                self.len.checked_sub(1).map(|last| start.min(last))
            } else {
                Some(selected)
            };
        }
        self.revision = self.revision.wrapping_add(1);
    }

    /// The selected row, if any: always a row of the model, or `None`.
    pub fn selected(&self) -> Option<usize> {
        let len = self.model.len();
        self.selected
            .and_then(|selected| len.checked_sub(1).map(|last| selected.min(last)))
    }

    /// Selects `row`, cut to the last row, or nothing; a row selected is
    /// scrolled into view at the next layout, as a key's is. It emits
    /// nothing: actions come from the user.
    pub fn select(&mut self, row: Option<usize>) {
        self.selected = row;
        self.selected = self.selected();
        self.reveal = self.selected.is_some();
    }

    /// How far its rows are scrolled, in units.
    pub const fn offset(&self) -> f64 {
        self.offset
    }

    /// Scrolls its rows to `offset` units, NaN or negative being 0; the
    /// next layout keeps it within what the rows can scroll.
    pub fn scroll_to(&mut self, offset: f64) {
        self.offset = if offset.is_nan() {
            0.0
        } else {
            offset.max(0.0)
        };
        self.reveal = false;
    }

    /// The column of a table's column header, `node`, which emitted
    /// [`ActionKind::Activate`]; `None` if it is not a column header.
    ///
    /// ```
    /// use tidepool_core::geom::Size;
    /// use tidepool_ui::metrics::FixedMetrics;
    /// use tidepool_ui::widgets::{Collection, Column, Control, Row};
    /// use tidepool_ui::Tree;
    ///
    /// let columns = [Column::new("Severity", 80.0), Column::new("Message", 240.0)];
    /// let rows = vec![Row::cells(["error", "mismatched types"])];
    /// let mut tree = Tree::new();
    /// let table = tree.insert(tree.root(), Collection::table("Problems", columns, rows).node())?;
    /// tree.frame(Size::new(400.0, 200.0), &FixedMetrics::DEFAULT);
    ///
    /// // Tab reaches the table, then its first column's header.
    /// assert_eq!(tree.focus_next(), Some(table));
    /// let header = tree.focus_next().expect("a column header");
    /// assert_eq!(Collection::column_of(&tree, header), Some(0));
    /// assert_eq!(Collection::column_of(&tree, table), None);
    /// # Ok::<(), tidepool_ui::UiError>(())
    /// ```
    pub fn column_of(tree: &Tree, node: NodeId) -> Option<usize> {
        window::column_of(tree, node)
    }

    /// Its rows' height in all, in units.
    fn extent(&self) -> f64 {
        self.model.len() as f64 * f64::from(ROW_HEIGHT)
    }

    /// The largest offset in the view the last binding saw.
    fn max_offset(&self) -> f64 {
        room(self.extent(), f64::from(self.view.area))
    }

    /// The rows a page holds: those wholly on screen, at least one.
    fn page(&self) -> usize {
        let (top, bottom) = self.view.band;
        let rows = ((bottom - top) / f64::from(ROW_HEIGHT)).floor();
        // At most MAX_VIEW_ROWS, so the cast is exact.
        (rows as usize).max(1)
    }

    /// Takes the model's length, keeps the selection on a row and the
    /// offset within what the rows can scroll in a view `area` high, of
    /// which the part from `band.0` to `band.1` is on screen; returns the
    /// rows to keep a node for, and the selected row if it is to be
    /// scrolled into view through the scrollers around the collection.
    ///
    /// Only when `done`, a layout pass having just given the area, is the
    /// offset kept within what the rows can scroll, and only when `take`
    /// too, the layout having settled, the selection asked to be scrolled
    /// into view scrolled: the least move of the offset
    /// brings it into the area, and the rows kept are those of a part as
    /// high as the band's, moved the least to hold it, where the scrollers
    /// around will bring it on screen. Before a pass, the rows are placed
    /// at the offset cut to the last pass's area, which it keeps. The rows
    /// reach no further than `real`, the body's height, which the area may
    /// have rounded up by a fraction of a unit. Returns the offset the rows
    /// are placed at too.
    fn settle(
        &mut self,
        area: f32,
        real: f32,
        band: (f64, f64),
        done: bool,
        take: bool,
    ) -> (Window, Option<usize>, f64) {
        self.len = self.model.len();
        self.selected = self.selected();
        let row = f64::from(ROW_HEIGHT);
        let view = f64::from(area.max(0.0));
        let reveal = done && take && std::mem::take(&mut self.reveal);
        let focus = self.selected.filter(|_| reveal);
        let mut offset = self.offset;
        if let Some(selected) = focus {
            let top = selected as f64 * row;
            if top < offset || row > view {
                offset = top;
            } else if top + row > offset + view {
                offset = top + row - view;
            }
        }
        let max = room(self.len as f64 * row, view);
        let offset = offset.clamp(0.0, max);
        if done {
            self.offset = offset;
        }
        let reach = view
            .min(self.len as f64 * row - offset)
            .min(f64::from(real))
            .max(0.0);
        let (mut top, mut bottom) = (band.0.clamp(0.0, reach), band.1.clamp(0.0, reach));
        if let Some(selected) = focus {
            let y = selected as f64 * row - offset;
            let high = (bottom - top).max(row.min(reach));
            if y < top {
                (top, bottom) = (y, y + high);
            } else if y + row > bottom {
                (top, bottom) = (y + row - high, y + row);
            }
            (top, bottom) = (top.clamp(0.0, reach), bottom.clamp(0.0, reach));
        }
        let window = window(self.len, reach, (top, bottom), offset);
        (window, focus, offset)
    }

    /// Selects `row`, scrolling it into view at the next layout, and tells
    /// the owner if the selection moved.
    fn choose(&mut self, cx: &mut EventContext<'_>, row: usize) {
        let moved = self.selected() != Some(row);
        self.selected = Some(row);
        self.reveal = true;
        if moved {
            cx.emit(ActionKind::Select);
            cx.set_semantics(self.semantics());
        }
        cx.request_layout();
    }

    /// Handles a key going down while it has focus, with no modifier.
    fn key(&mut self, cx: &mut EventContext<'_>, key: Key, repeat: bool) -> Handled {
        let current = self.selected();
        let last = self.model.len().checked_sub(1);
        let page = self.page();
        let to = match key {
            Key::Up => current.map_or(0, |at| at.saturating_sub(1)),
            Key::Down => current.map_or(0, |at| at.saturating_add(1)),
            Key::PageUp => current.map_or(0, |at| at.saturating_sub(page)),
            Key::PageDown => current.map_or(0, |at| at.saturating_add(page)),
            Key::Home => 0,
            Key::End => last.unwrap_or(0),
            Key::Enter | Key::NumpadEnter => {
                if current.is_none() {
                    return Handled::No;
                }
                if !repeat {
                    cx.emit(ActionKind::Activate);
                }
                return Handled::Yes;
            }
            Key::Left | Key::Right if self.kind == CollectionKind::TreeView => {
                self.tree_key(cx, key == Key::Right);
                return Handled::Yes;
            }
            _ => return Handled::No,
        };
        if let Some(last) = last {
            self.choose(cx, to.min(last));
        }
        Handled::Yes
    }

    /// Right (`expand`) or Left in a tree view, as the WAI-ARIA tree
    /// pattern has them.
    fn tree_key(&mut self, cx: &mut EventContext<'_>, expand: bool) {
        let Some(at) = self.selected() else {
            return;
        };
        let len = self.model.len();
        let row = self.model.row(at);
        match (expand, row.expansion()) {
            (true, Some(false)) => cx.emit(ActionKind::Expand),
            (true, Some(true)) => {
                let next = at + 1;
                if next < len && self.model.row(next).level() > row.level() {
                    self.choose(cx, next);
                }
            }
            (false, Some(true)) => cx.emit(ActionKind::Collapse),
            (false, _) => {
                if let Some(parent) = row
                    .parent_row()
                    .filter(|&parent| parent < len && parent != at)
                {
                    self.choose(cx, parent);
                }
            }
            (true, None) => {}
        }
    }

    /// Handles a primary press: on a row, it selects it, and on a tree
    /// item's chevron expands or collapses it.
    fn press(&mut self, cx: &mut EventContext<'_>, x: f32, y: f32) -> Handled {
        let origin = cx.bounds();
        let body = Rect::new(
            origin.x + self.view.body.x,
            origin.y + self.view.body.y,
            self.view.body.width,
            self.view.body.height,
        );
        if !body.contains(Point::new(x, y)) {
            return Handled::No;
        }
        // Rows are drawn where the last binding put them: row `i` at
        // `i * ROW_HEIGHT - shown` down the body, as far as they reach.
        let down = f64::from(y - body.y);
        let at = (down + self.view.shown) / f64::from(ROW_HEIGHT);
        let len = self.model.len();
        // Not negative, as the point is in the body; a cast saturates.
        let index = at.floor() as usize;
        if down < self.view.reach && index < len {
            self.choose(cx, index);
            if self.kind == CollectionKind::TreeView {
                let row = self.model.row(index);
                let chevron = body.x + ROW_PADDING + f32::from(row.level()) * TREE_INDENT;
                if let Some(expanded) = row.expansion()
                    && (chevron..chevron + CHEVRON_WIDTH).contains(&x)
                {
                    cx.emit(if expanded {
                        ActionKind::Collapse
                    } else {
                        ActionKind::Expand
                    });
                }
            }
        }
        Handled::Yes
    }

    /// Scrolls by the wheel's vertical delta, if it can move; what it
    /// cannot take goes on to the scrollers around it.
    fn wheel(&mut self, cx: &mut EventContext<'_>, delta: ScrollDelta) -> Handled {
        let y = match delta {
            ScrollDelta::Lines { y, .. } => y * SCROLL_LINE,
            ScrollDelta::Logical { y, .. } => y,
        };
        let y = if y.is_nan() {
            0.0
        } else {
            f64::from(y.clamp(-MAX_EXTENT, MAX_EXTENT))
        };
        let max = self.max_offset();
        let from = self.offset.min(max);
        let to = (from + y).clamp(0.0, max);
        if to == from {
            return Handled::No;
        }
        self.offset = to;
        self.reveal = false;
        cx.request_layout();
        Handled::Yes
    }

    /// The node's value: the selected row and the count, or the count.
    fn value(&self) -> String {
        let len = self.model.len();
        match self.selected() {
            Some(selected) => format!("row {} of {len}", selected + 1),
            None if len == 1 => "1 row".to_string(),
            None => format!("{len} rows"),
        }
    }
}

/// Shows what it is and where, never its rows.
impl fmt::Debug for Collection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Collection")
            .field("kind", &self.kind)
            .field("name", &self.name)
            .field("columns", &self.columns.len())
            .field("len", &self.model.len())
            .field("selected", &self.selected())
            .field("offset", &self.offset)
            .finish_non_exhaustive()
    }
}

impl Widget for Collection {
    /// A table's columns wide, and as tall as its header and its rows, up
    /// to [`MAX_VIEW_ROWS`] of them: what its owner's layout gives it with
    /// [`Length::Content`](crate::layout::Length::Content). What its rows'
    /// nodes take is never more, so it does not grow from one layout to
    /// the next.
    fn measure(&self, _available: Size, _metrics: &dyn TextMetrics) -> Size {
        let width: f32 = self.columns.iter().map(Column::width).sum();
        let header = if self.kind == CollectionKind::Table {
            ROW_HEIGHT
        } else {
            0.0
        };
        let rows = self.model.len().min(MAX_VIEW_ROWS);
        // At most 513 rows' height, which `f32` holds exactly.
        Size::new(width.min(MAX_EXTENT), header + rows as f32 * ROW_HEIGHT)
    }

    /// Its focus outline when focus shows: a `teal` border
    /// [`FOCUS_WIDTH`] wide, inside its bounds.
    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        if cx.state().focus_visible {
            list.border(cx.bounds(), FOCUS_WIDTH, cx.theme().color(Token::Teal));
        }
    }

    fn event(&mut self, cx: &mut EventContext<'_>, event: &InputEvent) -> Handled {
        match *event {
            InputEvent::KeyDown {
                key,
                modifiers,
                repeat,
            } if cx.state().focused && modifiers.is_empty() => self.key(cx, key, repeat),
            InputEvent::MouseDown {
                button: MouseButton::Left,
                position,
                ..
            } => self.press(cx, position.x, position.y),
            InputEvent::Wheel { delta, .. } => self.wheel(cx, delta),
            _ => Handled::No,
        }
    }
}

impl Control for Collection {
    fn semantics(&self) -> Semantics {
        let role = match self.kind {
            CollectionKind::List => Role::List,
            CollectionKind::TreeView => Role::TreeView,
            CollectionKind::Table => Role::Table,
        };
        Semantics::new(role, self.name.clone()).with_value(self.value())
    }

    /// Filling its parent, with [`FOCUS_WIDTH`] of padding on each side,
    /// where its focus outline is drawn.
    fn style(&self) -> Style {
        Style::column().fill().padding(Insets::all(FOCUS_WIDTH))
    }

    fn focusable(&self) -> bool {
        true
    }
}

/// The least range a collection scrolls: 1/64 of a unit.
const LEAST_ROOM: f64 = 1.0 / 64.0;

/// How far rows `extent` high scroll in a view `view` high: none if they
/// fit, or overflow by less than [`LEAST_ROOM`], which the tree's fitting
/// of sizes in `f32` can take from a view, so that a wheel the scrollers
/// around should have is not taken to scroll by that.
fn room(extent: f64, view: f64) -> f64 {
    let room = extent - view;
    if room < LEAST_ROOM { 0.0 } else { room }
}

/// The rows of `len` to keep a node for, with the rows scrolled to
/// `offset` and reaching `reach` units down the body, of which the part
/// from `band.0` to `band.1` is on screen: those that meet that part, and
/// [`ROW_MARGIN`] either side, of the rows that meet the reach. None if
/// no part is on screen.
fn window(len: usize, reach: f64, band: (f64, f64), offset: f64) -> Window {
    let none = Window {
        first: 0,
        count: 0,
        reach,
    };
    let (top, bottom) = band;
    if len == 0 || reach <= 0.0 || bottom <= top {
        return none;
    }
    let row = f64::from(ROW_HEIGHT);
    // The reach is at most MAX_VIEW_ROWS rows, and the offset at most the
    // rows' extent, so the casts are exact or saturate.
    let low = (offset / row).floor() as usize;
    let high = (((offset + reach) / row).ceil() as usize).min(len);
    let seen = ((offset + top) / row).floor() as usize;
    let end = ((offset + bottom) / row).ceil() as usize;
    let first = seen.saturating_sub(ROW_MARGIN).max(low);
    let end = end.saturating_add(ROW_MARGIN).min(high);
    if end <= first {
        return none;
    }
    Window {
        first,
        count: end - first,
        reach,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_is_the_rows_on_screen_and_the_margin_within_the_view() {
        let rows = |w: Window| (w.first, w.count);
        // A view of 300 wholly on screen: the 15 rows that meet it, 16 when
        // scrolled between rows; the margin would be outside the view.
        assert_eq!(rows(window(1_000_000, 300.0, (0.0, 300.0), 0.0)), (0, 15));
        assert_eq!(
            rows(window(1_000_000, 300.0, (0.0, 300.0), 2010.0)),
            (100, 16)
        );
        // At the end: the last 15 rows.
        let end = 1_000_000.0 * 20.0 - 300.0;
        assert_eq!(
            rows(window(1_000_000, 300.0, (0.0, 300.0), end)),
            (1_000_000 - 15, 15)
        );
        // Part of the view on screen, as in a scroller: its rows and the
        // margin either side, inside the view.
        assert_eq!(rows(window(1_000_000, 300.0, (100.0, 160.0), 0.0)), (3, 7));
        assert_eq!(rows(window(1_000_000, 300.0, (0.0, 30.0), 0.0)), (0, 4));
        // Fewer rows than the view holds: all of them.
        assert_eq!(rows(window(3, 60.0, (0.0, 60.0), 0.0)), (0, 3));
        // Nothing on screen, no view or no rows: none.
        assert_eq!(window(10, 200.0, (50.0, 50.0), 0.0).count, 0);
        assert_eq!(window(10, 0.0, (0.0, 0.0), 0.0).count, 0);
        assert_eq!(window(0, 300.0, (0.0, 300.0), 0.0).count, 0);
    }

    #[test]
    fn settling_keeps_the_selection_on_a_row_and_reveals_it_after_a_pass() {
        let mut list = Collection::list("Rows", vec![Row::new("a"); 100]);
        let whole = (0.0, 100.0);
        list.select(Some(50));
        // Before a pass, the ask is kept and nothing moves.
        let (_, focus, _) = list.settle(100.0, 100.0, whole, false, true);
        assert_eq!((list.offset(), focus), (0.0, None));
        // After it, the least move: row 50 at the bottom of a view of 5.
        let (window, focus, _) = list.settle(100.0, 100.0, whole, true, true);
        assert_eq!(list.offset(), 51.0 * 20.0 - 100.0);
        assert_eq!(focus, Some(50));
        assert!(window.first <= 50 && 50 < window.first + window.count);
        // Asked once: the next settling reveals nothing.
        assert_eq!(list.settle(100.0, 100.0, whole, true, true).1, None);
        // Not told to take it, a settling after a pass keeps the ask.
        list.select(Some(30));
        let at = list.offset();
        assert_eq!(list.settle(100.0, 100.0, whole, true, false).1, None);
        assert_eq!(list.offset(), at);
        assert_eq!(list.settle(100.0, 100.0, whole, true, true).1, Some(30));
        // Moving up past the top brings the row to the top.
        list.select(Some(10));
        list.settle(100.0, 100.0, whole, true, true);
        assert_eq!(list.offset(), 200.0);
        // A view smaller than a row shows the row's top.
        list.select(Some(20));
        list.settle(5.0, 5.0, (0.0, 5.0), true, true);
        assert_eq!(list.offset(), 400.0);
        // Rows removed under the selection, without a word: cut to the last.
        if let Some(rows) = list.model_mut::<Vec<Row>>() {
            rows.truncate(4);
        }
        list.settle(100.0, 100.0, whole, true, true);
        assert_eq!(list.selected(), Some(3));
        assert_eq!(list.offset(), 0.0);
        list.set_model(Vec::<Row>::new());
        assert_eq!(list.settle(100.0, 100.0, whole, true, true).0.count, 0);
        assert_eq!(list.selected(), None);
    }

    #[test]
    fn a_selection_off_screen_keeps_the_rows_where_the_scrollers_will_show_it() {
        // A view of 200, of which 0 to 60 is on screen: row 7 (140 to 160)
        // is in the view but not on screen, so the rows kept are a part as
        // high as the screen's, ending at it, and the offset stays.
        let mut list = Collection::list("Rows", vec![Row::new("a"); 100]);
        list.select(Some(7));
        let (window, focus, _) = list.settle(200.0, 200.0, (0.0, 60.0), true, true);
        assert_eq!((list.offset(), focus), (0.0, Some(7)));
        assert_eq!((window.first, window.count), (3, 7));
        // Wholly off screen: the row and the margin either side.
        list.select(Some(4));
        let (window, _, _) = list.settle(200.0, 200.0, (0.0, 0.0), true, true);
        assert_eq!((window.first, window.count), (2, 5));
    }

    #[test]
    fn inserted_and_removed_rows_keep_the_selection_on_its_row() {
        let mut list = Collection::list("Rows", vec![Row::new("a"); 10]);
        list.select(Some(5));
        list.rows_inserted(2, 3);
        assert_eq!(list.selected, Some(8));
        list.rows_inserted(9, 1);
        assert_eq!(list.selected, Some(8));
        list.rows_removed(0..2);
        assert_eq!(list.selected, Some(6));
        // The selected row removed: the row now in its place.
        list.rows_removed(5..7);
        assert_eq!(list.selected, Some(5));
        // ...or the last row, or none.
        list.rows_removed(4..100);
        assert_eq!(list.selected, Some(3));
        list.rows_removed(0..4);
        assert_eq!(list.selected, None);
        assert_eq!(list.len, 0);
    }

    #[test]
    fn debug_shows_lengths_not_text() {
        let row = Row::cells(["hunter2", "x"]).depth(2);
        let shown = format!("{row:?}");
        assert!(!shown.contains("hunter2"), "{shown}");
        assert!(shown.contains("<7 bytes>"), "{shown}");
        let list = Collection::list("Secrets", vec![Row::new("hunter2")]);
        let shown = format!("{list:?}");
        assert!(!shown.contains("hunter2"), "{shown}");
        assert!(shown.contains("len: 1"), "{shown}");
    }
}
