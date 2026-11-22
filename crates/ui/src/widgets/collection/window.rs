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

//! A collection's window of rows (ADR-0052): binding its row nodes to the
//! rows of its model on screen, as the tree is laid out, and the widgets of
//! those nodes.
//!
//! A collection's node holds a table's header row, then a body, a column
//! that clips and does not scroll in the tree's own terms. The body holds
//! a gap, the rows kept, each as high as the part of it in the view (the
//! first and last may be cut), and another gap, all of fixed heights that
//! sum to how far the rows reach down the view: so the rows sit where the
//! collection's offset puts them, the tree never scrolls or shrinks them,
//! and the body's natural height is never more than the collection
//! measures.

use std::any::Any;
use std::fmt;

use tidepool_core::geom::{Point, Rect};
use tidepool_core::input::{InputEvent, TextLength};

use crate::a11y::{Role, Semantics};
use crate::draw::{DrawList, PaintContext};
use crate::input::{EventContext, Handled};
use crate::layout::{Length, Style};
use crate::theme::Token;
use crate::tree::{Node, NodeId, Tree, Widget};
use crate::widgets::button::Activator;
use crate::widgets::{
    CELL_PADDING, CHEVRON_WIDTH, Control, FOCUS_WIDTH, Look, MAX_CELL_BYTES, MAX_VIEW_ROWS,
    ROW_HEIGHT, ROW_PADDING, ROW_TEXT, TABLE_HEADER, TREE_INDENT, baseline, cut_to, dimmed,
};

use super::{Bound, Collection, CollectionKind, Column, Row};

/// The thickness of a table's rules and its header's underline.
const RULE: f32 = 1.0;

/// The width of a chevron's strokes.
const CHEVRON_STROKE: f32 = 1.5;

/// How far a chevron reaches from its centre.
const CHEVRON_REACH: f32 = 4.0;

/// The column of a column header, `node`.
pub(super) fn column_of(tree: &Tree, node: NodeId) -> Option<usize> {
    tree.widget::<HeaderCell>(node).map(|cell| cell.column)
}

/// `text` cut to at most [`MAX_CELL_BYTES`], at an extended grapheme
/// cluster boundary of the whole text ([`cut_to`]).
pub(super) fn capped(text: &str) -> &str {
    cut_to(text, MAX_CELL_BYTES)
}

/// Where each kept row sits in the body: the gap above the first, and for
/// each row how much of its top is cut off and how high the rest is.
struct Placed {
    above: f32,
    rows: Vec<(f32, f32)>,
    below: f32,
}

/// Where the rows `first` to `first + count` sit in a body whose rows are
/// scrolled to `offset` and reach `reach` units down it: row `i` from
/// `i * ROW_HEIGHT - offset`, cut to the reach.
fn place(first: usize, count: usize, offset: f64, reach: f64) -> Placed {
    let row = f64::from(ROW_HEIGHT);
    let mut rows = Vec::with_capacity(count);
    let mut above = 0.0;
    let mut bottom = 0.0;
    for k in 0..count {
        let y = (first + k) as f64 * row - offset;
        let top = y.max(0.0).min(reach);
        let end = (y + row).min(reach).max(top);
        if k == 0 {
            above = top;
        }
        // Each at most ROW_HEIGHT, and the gaps at most the reach, so the
        // casts to `f32` lose nothing that shows.
        rows.push(((top - y) as f32, (end - top) as f32));
        bottom = end;
    }
    let below = if count == 0 { reach } else { reach - bottom };
    let above = above as f32;
    // The tree sums its children's heights in `f32`, in order, for the
    // body's natural height: the gap below is cut until that sum is no
    // more than the reach, so that it is never more than the collection
    // measures, and a layout pass changes nothing the next one sees.
    let total = reach as f32;
    let before = rows.iter().fold(above, |sum, &(_, height)| sum + height);
    let mut below = (below.max(0.0) as f32).min((total - before).max(0.0));
    let mut steps = 0;
    while before + below > total && below > 0.0 && steps < 64 {
        below = below.next_down();
        steps += 1;
    }
    if before + below > total {
        below = 0.0;
    }
    Placed { above, rows, below }
}

impl Tree {
    /// Binds one collection's row nodes to the rows of its model on
    /// screen, if it is shown, from the last layout's placements; returns
    /// whether that added or removed nodes or moved them, so that layout
    /// must pass again. `after_pass` says that a layout pass has just
    /// placed the nodes, and `reveal` that the layout has settled, so that
    /// a selection to scroll into view is scrolled in the view that pass
    /// gave it, and the ask is done with; until then, its rows stay as
    /// they are.
    pub(crate) fn bind_collection(&mut self, id: NodeId, after_pass: bool, reveal: bool) -> bool {
        if !self.is_visible(id) {
            return false;
        }
        let Some(collection) = self.widget::<Collection>(id) else {
            return false;
        };
        let kind = collection.kind;
        let pending = collection.reveal;
        let widths: Vec<f32> = collection.columns.iter().map(Column::width).collect();
        let (body, built) = if collection.view.built {
            match self.part::<Body>(id) {
                Some(body) => (body, false),
                None => match self.build_parts(id) {
                    Some(body) => (body, true),
                    None => return false,
                },
            }
        } else {
            match self.build_parts(id) {
                Some(body) => (body, true),
                None => return false,
            }
        };
        let (Some(bounds), Some(placement)) =
            (self.bounds(id), self.get(body).map(|node| node.placement))
        else {
            return built;
        };
        let viewport = placement.viewport;
        // Before a pass, placements of a body with no height are stale: it
        // was hidden, or never laid out. Its rows stay as they are, as they
        // do when a selection is to be scrolled into view, until the layout
        // settles and the binding that reveals, asking for the rows it
        // shows only.
        if !built && ((!after_pass && viewport.height <= 0.0) || (pending && !reveal)) {
            return false;
        }
        let area = viewport
            .height
            .min(MAX_VIEW_ROWS as f32 * ROW_HEIGHT)
            .max(0.0);
        let band = on_screen(viewport, placement.clip);

        let Some(collection) = self.collection_mut(id) else {
            return built;
        };
        let (window, focus, offset) =
            collection.settle(area, viewport.height, band, after_pass, reveal);
        collection.view.body = Rect::new(
            viewport.x - bounds.x,
            viewport.y - bounds.y,
            viewport.width,
            viewport.height,
        );
        collection.view.area = area;
        collection.view.band = band;
        collection.view.shown = offset;
        collection.view.reach = window.reach;
        let bound = Bound {
            first: window.first,
            count: window.count,
            revision: collection.revision,
            selected: collection.selected,
        };
        let fresh = built || collection.view.bound != Some(bound);
        collection.view.bound = Some(bound);
        let selected = collection.selected;
        let semantics = collection.semantics();

        let added = self.fit_rows(body, kind, &widths, window.count);
        let rows = self.row_nodes(body);
        let at = (
            window.first,
            rows.len(),
            offset.to_bits(),
            window.reach.to_bits(),
        );
        let unmoved = !built
            && !added
            && self.widget::<Collection>(id).and_then(|c| c.view.placed) == Some(at);
        let moved = !unmoved && {
            let placed = place(window.first, rows.len(), offset, window.reach);
            self.place_rows(body, &rows, &placed)
        };
        if let Some(collection) = self.collection_mut(id) {
            collection.view.placed = Some(at);
        }
        // Rows are asked for again only when what is bound changed: a
        // scroll within the rows kept, or a tree too full to add the rest,
        // asks for none.
        if fresh || added {
            let contents: Vec<Row> = match self.widget::<Collection>(id) {
                Some(collection) => (0..rows.len())
                    .map(|k| collection.model.row(window.first + k))
                    .collect(),
                None => Vec::new(),
            };
            for (k, (&node, row)) in rows.iter().zip(contents).enumerate() {
                let index = window.first + k;
                self.bind_row(node, kind, &widths, index, &row, selected == Some(index));
            }
        }
        if let Some(focus) = focus
            && let Some(&node) = focus.checked_sub(window.first).and_then(|k| rows.get(k))
        {
            // The scrollers around bring it on screen at this layout: in
            // place of the owner's ask for a node within the collection, and
            // before the owner's ask for another, or another collection's.
            if self.reveal.is_some_and(|asked| self.is_within(asked, id)) {
                self.reveal = Some(node);
            } else if !self.reveal_rows.contains(&node) {
                self.reveal_rows.push(node);
            }
        }
        if self.semantics(id) != Some(&semantics) {
            // Refused only for a blank name, which the tree refused at the
            // insert already; the node keeps what it had.
            let _ = self.set_semantics(id, semantics);
        }
        built || added || moved
    }

    /// The collection's widget, to change, without marking its node.
    fn collection_mut(&mut self, id: NodeId) -> Option<&mut Collection> {
        let widget: &mut dyn Widget = self.get_mut(id)?.widget.as_deref_mut()?;
        (widget as &mut dyn Any).downcast_mut()
    }

    /// The collection's child holding a `W`.
    fn part<W: Widget>(&self, id: NodeId) -> Option<NodeId> {
        self.children(id)?
            .iter()
            .copied()
            .find(|&child| self.widget::<W>(child).is_some())
    }

    /// Builds the collection's parts for its widget, in place of any it
    /// had: a table's header row, then a body holding its two gaps. Returns
    /// the body.
    fn build_parts(&mut self, id: NodeId) -> Option<NodeId> {
        let collection = self.widget::<Collection>(id)?;
        let kind = collection.kind;
        let columns = collection.columns.clone();
        for child in self.children(id)?.to_vec() {
            // A child of the collection, so not the root.
            let _ = self.remove(child);
        }
        if kind == CollectionKind::Table {
            let header = self.insert(id, header_row(&columns)).ok()?;
            for (index, column) in columns.iter().enumerate() {
                if self.insert(header, header_cell(index, column)).is_err() {
                    break;
                }
            }
        }
        let body = Node::new(Style::column().fill().clip()).with_widget(Body);
        let body = self.insert(id, body).ok()?;
        for _ in 0..2 {
            let gap = Node::new(gap_style(0.0)).with_widget(Gap);
            if self.insert(body, gap).is_err() {
                let _ = self.remove(body);
                return None;
            }
        }
        let collection = self.collection_mut(id)?;
        collection.view.built = true;
        collection.view.bound = None;
        collection.view.placed = None;
        Some(body)
    }

    /// The body's row nodes, in order.
    fn row_nodes(&self, body: NodeId) -> Vec<NodeId> {
        self.children(body).map_or_else(Vec::new, |children| {
            children
                .iter()
                .copied()
                .filter(|&child| self.widget::<RowView>(child).is_some())
                .collect()
        })
    }

    /// Gives the body `count` row nodes, adding them before its last gap
    /// or removing the last; returns whether it added any. If the tree is
    /// full, it keeps the rows it could add, and the next layout tries
    /// again without asking for the rows it has.
    fn fit_rows(
        &mut self,
        body: NodeId,
        kind: CollectionKind,
        widths: &[f32],
        count: usize,
    ) -> bool {
        let rows = self.row_nodes(body);
        let mut added = false;
        for _ in rows.len()..count {
            let Some(before) = self
                .children(body)
                .map(|children| children.len().saturating_sub(1))
            else {
                break;
            };
            let Ok(row) = self.insert_at(body, before, row_node(kind)) else {
                break;
            };
            added = true;
            if kind == CollectionKind::Table {
                for &width in widths {
                    if self.insert(row, cell_node(width)).is_err() {
                        break;
                    }
                }
            }
        }
        for &row in rows.iter().skip(count).rev() {
            // A row of the body, so not the root.
            let _ = self.remove(row);
        }
        added
    }

    /// Gives the body's gaps and rows the heights `placed` gives them, and
    /// the rows and their cells the cut of their tops; returns whether a
    /// height changed, which moves nodes.
    fn place_rows(&mut self, body: NodeId, rows: &[NodeId], placed: &Placed) -> bool {
        let children = self.children(body).map_or_else(Vec::new, <[_]>::to_vec);
        let mut moved = false;
        if let (Some(&above), Some(&below)) = (children.first(), children.last()) {
            moved |= self.set_height(above, placed.above);
            moved |= self.set_height(below, placed.below);
        }
        for (&row, &(cut, height)) in rows.iter().zip(&placed.rows) {
            moved |= self.set_height(row, height);
            let changed = self.get_mut(row).is_some_and(|data| {
                data.widget
                    .as_deref_mut()
                    .and_then(|widget| (widget as &mut dyn Any).downcast_mut::<RowView>())
                    .is_some_and(|view| std::mem::replace(&mut view.cut, cut) != cut)
            });
            if changed {
                self.mark(row);
            }
            for cell in self.children(row).map_or_else(Vec::new, <[_]>::to_vec) {
                let changed = self.get_mut(cell).is_some_and(|data| {
                    data.widget
                        .as_deref_mut()
                        .and_then(|widget| (widget as &mut dyn Any).downcast_mut::<CellView>())
                        .is_some_and(|view| std::mem::replace(&mut view.cut, cut) != cut)
                });
                if changed {
                    self.mark(cell);
                }
            }
        }
        moved
    }

    /// Makes the node `height` high, keeping the rest of its style;
    /// returns whether that changed it.
    fn set_height(&mut self, node: NodeId, height: f32) -> bool {
        let Some(style) = self.style(node).copied() else {
            return false;
        };
        if style.height == Length::Fixed(height) {
            return false;
        }
        // The node is in the tree, so this cannot fail.
        let _ = self.set_style(node, style.height(Length::Fixed(height)));
        true
    }

    /// Binds the row node `node` to `row`, the model's row `index`,
    /// repainting only what changed. What it shows and names of each cell
    /// is [capped]; a table's row is named by the cells its columns wider
    /// than 0 show, and no others.
    fn bind_row(
        &mut self,
        node: NodeId,
        kind: CollectionKind,
        widths: &[f32],
        index: usize,
        row: &Row,
        selected: bool,
    ) {
        let semantics = row_semantics(kind, widths, index, row, selected);
        if kind == CollectionKind::Table {
            let cells: Vec<NodeId> = self.children(node).map_or_else(Vec::new, <[_]>::to_vec);
            for (column, cell) in cells.into_iter().enumerate() {
                let shown = widths.get(column).is_some_and(|&width| width > 0.0);
                let text = if shown { capped(row.cell(column)) } else { "" };
                let changed = self.get_mut(cell).is_some_and(|data| {
                    data.semantics = Semantics::new(Role::Cell, text);
                    let Some(view) = data
                        .widget
                        .as_deref_mut()
                        .and_then(|widget| (widget as &mut dyn Any).downcast_mut::<CellView>())
                    else {
                        return false;
                    };
                    let changed = view.text != text;
                    if changed {
                        view.text = text.to_string();
                    }
                    changed
                });
                if changed {
                    self.mark(cell);
                }
            }
        }
        let label = if kind == CollectionKind::Table {
            String::new()
        } else {
            capped(row.label()).to_string()
        };
        let changed = self.get_mut(node).is_some_and(|data| {
            data.semantics = semantics;
            let Some(view) = data
                .widget
                .as_deref_mut()
                .and_then(|widget| (widget as &mut dyn Any).downcast_mut::<RowView>())
            else {
                return false;
            };
            let next = (label, row.level(), row.expansion(), selected);
            let changed = (
                view.label.as_str(),
                view.depth,
                view.expansion,
                view.selected,
            ) != (next.0.as_str(), next.1, next.2, next.3);
            if changed {
                (view.label, view.depth, view.expansion, view.selected) = next;
            }
            changed
        });
        if changed {
            self.mark(node);
        }
    }
}

/// The part of the body on screen, from its top: its content box `viewport`
/// cut to its `clip`, nothing where they do not meet.
fn on_screen(viewport: Rect, clip: Rect) -> (f64, f64) {
    let left = viewport.x.max(clip.x);
    let right = viewport.right().min(clip.right());
    let top = viewport.y.max(clip.y);
    let bottom = viewport.bottom().min(clip.bottom());
    if right > left && bottom > top {
        (f64::from(top - viewport.y), f64::from(bottom - viewport.y))
    } else {
        (0.0, 0.0)
    }
}

/// What a row of `kind` is told about itself: its role, its text or, if
/// that is blank, its place, whether it is selected, and a tree item's
/// expansion. A table's row is named by the cells of its columns wider
/// than 0, each [capped], and no others.
fn row_semantics(
    kind: CollectionKind,
    widths: &[f32],
    index: usize,
    row: &Row,
    selected: bool,
) -> Semantics {
    let (role, name) = match kind {
        CollectionKind::List => (Role::ListItem, capped(row.label()).to_string()),
        CollectionKind::TreeView => (Role::TreeItem, capped(row.label()).to_string()),
        CollectionKind::Table => {
            let cells: Vec<&str> = widths
                .iter()
                .enumerate()
                .filter(|&(_, &width)| width > 0.0)
                .map(|(column, _)| capped(row.cell(column)))
                .filter(|cell| !cell.trim().is_empty())
                .collect();
            (Role::Row, cells.join(", "))
        }
    };
    let name = if name.trim().is_empty() {
        format!("row {}", index.saturating_add(1))
    } else {
        name
    };
    let mut semantics = Semantics::new(role, name);
    semantics.selected = selected;
    if kind == CollectionKind::TreeView {
        semantics.expanded = row.expansion();
    }
    semantics
}

/// A gap of the body, `height` high.
fn gap_style(height: f32) -> Style {
    Style::default()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(height))
}

/// A row node of `kind`, bound to no row yet.
fn row_node(kind: CollectionKind) -> Node {
    let style = if kind == CollectionKind::Table {
        Style::row()
    } else {
        Style::stack()
    };
    let role = match kind {
        CollectionKind::List => Role::ListItem,
        CollectionKind::TreeView => Role::TreeItem,
        CollectionKind::Table => Role::Row,
    };
    Node::new(
        style
            .width(Length::Fill(1.0))
            .height(Length::Fixed(ROW_HEIGHT)),
    )
    .with_semantics(Semantics::new(role, ""))
    .with_widget(RowView {
        kind,
        label: String::new(),
        depth: 0,
        expansion: None,
        selected: false,
        cut: 0.0,
    })
}

/// A cell node of a table's row, `width` wide.
fn cell_node(width: f32) -> Node {
    Node::new(
        Style::stack()
            .width(Length::Fixed(width))
            .height(Length::Fill(1.0)),
    )
    .with_semantics(Semantics::new(Role::Cell, ""))
    .with_widget(CellView {
        text: String::new(),
        cut: 0.0,
    })
}

/// A table's header row, of `columns`.
fn header_row(columns: &[Column]) -> Node {
    let titles: Vec<&str> = columns
        .iter()
        .filter(|column| column.width() > 0.0)
        .map(Column::title)
        .filter(|title| !title.trim().is_empty())
        .collect();
    let name = if titles.is_empty() {
        "header".to_string()
    } else {
        titles.join(", ")
    };
    Node::new(
        Style::row()
            .width(Length::Fill(1.0))
            .height(Length::Fixed(ROW_HEIGHT)),
    )
    .with_semantics(Semantics::new(Role::Row, name))
    .with_widget(HeaderRow)
}

/// The header of column `index`, which takes focus if it has a title and
/// is wider than 0, so that every tab stop shows.
fn header_cell(index: usize, column: &Column) -> Node {
    let mut node = Node::new(
        Style::stack()
            .width(Length::Fixed(column.width()))
            .height(Length::Fill(1.0)),
    )
    .with_semantics(Semantics::new(Role::ColumnHeader, column.title()))
    .with_widget(HeaderCell {
        column: index,
        shown: column.title().to_uppercase(),
        activator: Activator::default(),
    });
    node.focusable = column.width() > 0.0 && !column.title().trim().is_empty();
    node
}

/// The node a collection's rows show in; it paints nothing.
struct Body;

impl Widget for Body {}

/// One of the body's two gaps, above and below its rows; it paints
/// nothing.
struct Gap;

impl Widget for Gap {}

/// A row of a list, tree view or table, bound to a row of the model.
///
/// It paints the `teal-glow` selection or the `row-hover` wash, then a
/// list's or tree view's text and a tree item's chevron, or a table row's
/// `line` rule under its cells, which paint their own text; all as a whole
/// row whose top `cut` units are above the view, cut to its bounds. Its
/// `Debug` shows its text as its length only (ADR-0043).
#[derive(PartialEq)]
struct RowView {
    kind: CollectionKind,
    label: String,
    depth: u16,
    expansion: Option<bool>,
    selected: bool,
    cut: f32,
}

/// Shows its text as its length only.
impl fmt::Debug for RowView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RowView")
            .field("kind", &self.kind)
            .field("label", &TextLength(&self.label))
            .field("depth", &self.depth)
            .field("expansion", &self.expansion)
            .field("selected", &self.selected)
            .field("cut", &self.cut)
            .finish()
    }
}

/// The whole row of a node at `bounds` whose top `cut` units are cut off.
fn whole_row(bounds: Rect, cut: f32) -> Rect {
    Rect::new(bounds.x, bounds.y - cut, bounds.width, ROW_HEIGHT)
}

impl Widget for RowView {
    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let bounds = cx.bounds();
        let row = whole_row(bounds, self.cut);
        let theme = cx.theme();
        let look = Look::of(cx.state());
        dimmed(cx, list, |list| {
            list.push_clip(bounds);
            if self.selected {
                list.rect(row, theme.color(Token::TealGlow));
            } else if look != Look::Rest {
                list.rect(row, theme.color(Token::RowHover));
            }
            match self.kind {
                CollectionKind::Table => list.rect(
                    Rect::new(row.x, row.y + row.height - RULE, row.width, RULE),
                    theme.color(Token::Line),
                ),
                CollectionKind::List => self.paint_text(cx, row, row.x + ROW_PADDING, list),
                CollectionKind::TreeView => {
                    let indent = row.x + ROW_PADDING + f32::from(self.depth) * TREE_INDENT;
                    if let Some(expanded) = self.expansion {
                        chevron(cx, row, indent + CHEVRON_WIDTH / 2.0, expanded, list);
                    }
                    self.paint_text(cx, row, indent + CHEVRON_WIDTH, list);
                }
            }
            list.pop_clip();
        });
    }
}

impl RowView {
    /// Paints the row's text from `x`, in the whole `row`.
    fn paint_text(&self, cx: &PaintContext<'_>, row: Rect, x: f32, list: &mut DrawList) {
        if self.label.is_empty() {
            return;
        }
        list.text(
            Point::new(x, baseline(row, cx.metrics(), &ROW_TEXT)),
            self.label.clone(),
            ROW_TEXT,
            cx.theme().color(Token::Text),
        );
    }
}

/// A tree item's chevron, centred at `x` in its whole `row`, in `text-2`:
/// `›` collapsed, `⌄` expanded.
fn chevron(cx: &PaintContext<'_>, row: Rect, x: f32, expanded: bool, list: &mut DrawList) {
    let y = row.y + row.height / 2.0;
    let reach = CHEVRON_REACH;
    let half = reach / 2.0;
    let (a, b, c) = if expanded {
        (
            Point::new(x - reach, y - half),
            Point::new(x, y + half),
            Point::new(x + reach, y - half),
        )
    } else {
        (
            Point::new(x - half, y - reach),
            Point::new(x + half, y),
            Point::new(x - half, y + reach),
        )
    };
    let color = cx.theme().color(Token::Text2);
    list.line(a, b, CHEVRON_STROKE, color);
    list.line(b, c, CHEVRON_STROKE, color);
}

/// A table's cell: its text in `text`, as in a whole row whose top `cut`
/// units are cut off, cut to the cell. Its `Debug` shows its text as its
/// length only (ADR-0043).
struct CellView {
    text: String,
    cut: f32,
}

/// Shows its text as its length only.
impl fmt::Debug for CellView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CellView")
            .field("text", &TextLength(&self.text))
            .field("cut", &self.cut)
            .finish()
    }
}

impl Widget for CellView {
    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        if self.text.is_empty() {
            return;
        }
        let bounds = cx.bounds();
        let row = whole_row(bounds, self.cut);
        dimmed(cx, list, |list| {
            list.push_clip(bounds);
            list.text(
                Point::new(
                    bounds.x + CELL_PADDING,
                    baseline(row, cx.metrics(), &ROW_TEXT),
                ),
                self.text.clone(),
                ROW_TEXT,
                cx.theme().color(Token::Text),
            );
            list.pop_clip();
        });
    }
}

/// A table's header row: `table-head`, with a `teal` underline.
struct HeaderRow;

impl Widget for HeaderRow {
    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let bounds = cx.bounds();
        let theme = cx.theme();
        dimmed(cx, list, |list| {
            list.rect(bounds, theme.color(Token::TableHead));
            list.rect(
                Rect::new(
                    bounds.x,
                    bounds.y + bounds.height - RULE,
                    bounds.width,
                    RULE,
                ),
                theme.color(Token::Teal),
            );
        });
    }
}

/// A column's header: its title uppercase in [`TABLE_HEADER`], `teal`,
/// `teal-hi` when hovered and on `slate` when pressed, with its focus
/// outline inside its bounds when focus shows. It activates as a button
/// does.
struct HeaderCell {
    column: usize,
    shown: String,
    activator: Activator,
}

impl Widget for HeaderCell {
    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let bounds = cx.bounds();
        let theme = cx.theme();
        let look = Look::of(cx.state());
        dimmed(cx, list, |list| {
            if look == Look::Pressed {
                // Above the header's underline, which stays.
                let fill = Rect::new(
                    bounds.x,
                    bounds.y,
                    bounds.width,
                    (bounds.height - RULE).max(0.0),
                );
                list.rect(fill, theme.color(Token::Slate));
            }
            let token = if look == Look::Rest {
                Token::Teal
            } else {
                Token::TealHi
            };
            list.push_clip(bounds);
            list.text(
                Point::new(
                    bounds.x + CELL_PADDING,
                    baseline(bounds, cx.metrics(), &TABLE_HEADER),
                ),
                self.shown.clone(),
                TABLE_HEADER,
                theme.color(token),
            );
            list.pop_clip();
        });
        if cx.state().focus_visible {
            list.border(bounds, FOCUS_WIDTH, theme.color(Token::Teal));
        }
    }

    fn event(&mut self, cx: &mut EventContext<'_>, event: &InputEvent) -> Handled {
        self.activator.activate(cx, event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tidepool_text::pos::grapheme;

    #[test]
    fn text_is_capped_at_a_cluster_boundary_of_the_whole_text() {
        assert_eq!(capped("short"), "short");
        let exact = "a".repeat(MAX_CELL_BYTES);
        assert_eq!(capped(&exact), exact);
        // Clusters of three bytes, an `e` and a combining accent: the cap
        // falls inside one, so the cut is before it.
        let marks = "e\u{301}".repeat(MAX_CELL_BYTES);
        let cut = capped(&marks);
        assert_eq!(cut.len(), MAX_CELL_BYTES / 3 * 3);
        assert!(grapheme::is_boundary(&marks, cut.len()));
        // An accent after the cap would join the char before it: that char
        // is cut too.
        let joined = format!("{}e\u{301}", "a".repeat(MAX_CELL_BYTES - 1));
        assert_eq!(capped(&joined).len(), MAX_CELL_BYTES - 1);
        // A single cluster longer than the cap shows nothing of it.
        let long = format!("e{}", "\u{301}".repeat(MAX_CELL_BYTES));
        assert_eq!(capped(&long), "");
    }

    #[test]
    fn rows_are_placed_from_the_offset_and_cut_to_the_reach() {
        // Scrolled by 5 units: the first row loses its top 5, the last is
        // cut to the reach, and the gaps fill the rest.
        let placed = place(0, 4, 5.0, 60.0);
        assert_eq!(placed.above, 0.0);
        assert_eq!(
            placed.rows,
            [(5.0, 15.0), (0.0, 20.0), (0.0, 20.0), (0.0, 5.0)]
        );
        assert_eq!(placed.below, 0.0);
        // Rows starting down the body, as in a scroller: a gap above them.
        let placed = place(5, 2, 0.0, 300.0);
        assert_eq!((placed.above, placed.below), (100.0, 160.0));
        assert_eq!(placed.rows, [(0.0, 20.0), (0.0, 20.0)]);
        // No rows: one gap as high as the reach.
        let placed = place(0, 0, 0.0, 80.0);
        assert_eq!(
            (placed.above, placed.rows.len(), placed.below),
            (0.0, 0, 80.0)
        );
    }

    #[test]
    fn a_rows_and_a_cells_debug_show_their_text_as_its_length() {
        let row = RowView {
            kind: CollectionKind::List,
            label: "hunter2".to_string(),
            depth: 0,
            expansion: None,
            selected: false,
            cut: 0.0,
        };
        let cell = CellView {
            text: "hunter2".to_string(),
            cut: 0.0,
        };
        for shown in [format!("{row:?}"), format!("{cell:?}")] {
            assert!(!shown.contains("hunter2"), "{shown}");
            assert!(shown.contains("<7 bytes>"), "{shown}");
        }
    }
}
