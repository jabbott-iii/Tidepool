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

//! The editor view's text area: the widget that holds the text, the
//! selection and the scroll position, lays out the lines on screen, paints
//! them, and takes the wheel (M4.1, ADR-0063).
//!
//! New in Tidepool.
//!
//! What the [module documentation](super) promises is kept here. The
//! position, the layouts and what the wheel asked for live in cells, as
//! painting has only a shared reference: a wheel event records how far to
//! go, and the next [`Area::resolve`], from the view's `update` or from
//! painting, which has the metrics, moves there.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use tidepool_core::geom::{Point, Rect};
use tidepool_core::input::{InputEvent, Modifiers, ScrollDelta};
use tidepool_text::{Cursor, LineSpan, Rope, Selection, TextError};
use tidepool_ui::draw::{DrawList, PaintContext};
use tidepool_ui::input::{EventContext, Handled};
use tidepool_ui::layout::MAX_EXTENT;
use tidepool_ui::metrics::TextMetrics;
use tidepool_ui::theme::Token;
use tidepool_ui::widgets::FOCUS_WIDTH;
use tidepool_ui::{Theme, Widget};

use super::layout::{Kind, LineLayout, Measures, Piece};
use super::{
    CURSOR_WIDTH, CUT_MARKER, EDITOR_TEXT, GUTTER_PADDING, MARKER, MIN_GUTTER_DIGITS,
    ScrollPosition, WHEEL_ROWS,
};

/// The row at the top of the view: a line, a row of that line, and how far
/// that row is scrolled up past the view's top, from 0 up to a row's
/// height.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Top {
    pub(super) line: usize,
    pub(super) row: usize,
    pub(super) y: f32,
}

impl Top {
    /// Whether `self` is above `other`.
    fn before(self, other: Self) -> bool {
        (self.line, self.row) < (other.line, other.row)
            || ((self.line, self.row) == (other.line, other.row) && self.y < other.y)
    }
}

/// What the wheel and the scrollbars asked for since the last resolve.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Pending {
    /// Rows down, from wheel notches.
    rows_y: f32,
    /// Units down, from exact distances.
    units_y: f32,
    /// Rows' heights right, from wheel notches.
    rows_x: f32,
    /// Units right, from exact distances.
    units_x: f32,
    /// A line position the vertical scrollbar was dragged to.
    jump: Option<f32>,
    /// An offset the horizontal scrollbar was dragged to.
    left: Option<f32>,
}

/// When the kept layouts were laid out: the text's revision, the width
/// lines wrap at, and the metrics' measures.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Key {
    revision: u64,
    wrap: Option<u32>,
    measures: [u32; 7],
}

/// A kept layout, and whether it was used since the cache was last
/// trimmed.
#[derive(Debug)]
struct Entry {
    layout: Rc<LineLayout>,
    used: bool,
}

/// The layouts kept, by line.
#[derive(Debug, Default)]
struct Cache {
    key: Option<Key>,
    lines: HashMap<usize, Entry>,
}

/// Where things are in the text area, for its bounds and the metrics.
#[derive(Clone, Copy, Debug)]
pub(super) struct Geometry {
    /// The text area's bounds.
    pub(super) bounds: Rect,
    /// What the metrics give.
    pub(super) measures: Measures,
    /// The gutter's width.
    pub(super) gutter: f32,
    /// Where the text starts, right of the gutter.
    pub(super) text_x: f32,
    /// How wide the text is shown.
    pub(super) text_width: f32,
    /// The width lines wrap at, with soft wrap on.
    pub(super) wrap: Option<f32>,
}

/// A row on screen.
#[derive(Clone, Debug)]
pub(super) struct ScreenRow {
    pub(super) line: usize,
    pub(super) row: usize,
    /// The row's top, in window coordinates.
    pub(super) y: f32,
    pub(super) layout: Rc<LineLayout>,
}

/// What a scrollbar is to show: the content's length, the view's, and the
/// offset.
pub(super) type BarRange = (f32, f32, f32);

/// The text area's widget (the [module documentation](self)).
pub(super) struct Area {
    text: Rope,
    selection: Selection,
    wrap: bool,
    /// Counts the changes of the text and of the wrap, which the layouts
    /// depend on.
    revision: u64,
    top: Cell<Top>,
    /// How far the text is scrolled left.
    left: Cell<f32>,
    pending: Cell<Pending>,
    /// The widest line laid out since the layouts were last dropped.
    widest: Cell<f32>,
    cache: RefCell<Cache>,
}

impl Area {
    /// A text area over `text`, a caret at its start, at the top, with
    /// soft wrap off.
    pub(super) fn new(text: Rope) -> Self {
        Self {
            text,
            selection: Selection::caret(0),
            wrap: false,
            revision: 0,
            top: Cell::new(Top::default()),
            left: Cell::new(0.0),
            pending: Cell::new(Pending::default()),
            widest: Cell::new(0.0),
            cache: RefCell::new(Cache::default()),
        }
    }

    pub(super) const fn text(&self) -> &Rope {
        &self.text
    }

    pub(super) const fn selection(&self) -> &Selection {
        &self.selection
    }

    pub(super) const fn wrap(&self) -> bool {
        self.wrap
    }

    /// The text and the selection, which the caller checked against it.
    pub(super) fn set_text(&mut self, text: Rope, selection: Selection) {
        self.text = text;
        self.selection = selection;
        self.revision = self.revision.wrapping_add(1);
    }

    /// The selection, which the caller checked against the text.
    pub(super) fn set_selection(&mut self, selection: Selection) {
        self.selection = selection;
    }

    /// Turns soft wrap on or off: the top row becomes the first of its
    /// line, and with wrap on nothing scrolls sideways.
    pub(super) fn set_wrap(&mut self, wrap: bool) {
        if wrap == self.wrap {
            return;
        }
        self.wrap = wrap;
        self.revision = self.revision.wrapping_add(1);
        let top = self.top.get();
        self.top.set(Top {
            row: 0,
            y: if top.row == 0 { top.y } else { 0.0 },
            ..top
        });
        if wrap {
            self.left.set(0.0);
        }
    }

    /// Where the view is scrolled to.
    pub(super) fn position(&self) -> ScrollPosition {
        let top = self.top.get();
        ScrollPosition {
            line: top.line,
            row: top.row,
            y: top.y,
            x: self.left.get(),
        }
    }

    /// Asks the next resolve to move to line position `offset`, as the
    /// vertical scrollbar says.
    pub(super) fn jump(&self, offset: f32) {
        self.pending.set(Pending {
            jump: Some(offset),
            ..self.pending.get()
        });
    }

    /// Asks the next resolve to scroll the text `offset` left, as the
    /// horizontal scrollbar says.
    pub(super) fn slide(&self, offset: f32) {
        self.pending.set(Pending {
            left: Some(offset),
            ..self.pending.get()
        });
    }

    /// Where things are for `bounds` and `metrics`.
    pub(super) fn geometry(&self, bounds: Rect, metrics: &dyn TextMetrics) -> Geometry {
        let measures = Measures::new(metrics);
        let digits = digits(self.text.len_lines()).max(MIN_GUTTER_DIGITS);
        let gutter = 2.0 * GUTTER_PADDING + digits as f32 * measures.digit;
        let text_width = (bounds.width - gutter).max(0.0);
        Geometry {
            bounds,
            measures,
            gutter,
            text_x: bounds.x + gutter,
            text_width,
            wrap: self.wrap.then(|| (text_width - CURSOR_WIDTH).max(0.0)),
        }
    }

    /// Drops the kept layouts if the text, the wrap, the width lines wrap
    /// at or the metrics changed since they were laid out.
    fn check(&self, g: &Geometry) {
        let key = Key {
            revision: self.revision,
            wrap: g.wrap.map(f32::to_bits),
            measures: g.measures.bits(),
        };
        let mut cache = self.cache.borrow_mut();
        if cache.key != Some(key) {
            cache.key = Some(key);
            cache.lines.clear();
            self.widest.set(0.0);
        }
    }

    /// Drops the layouts not used since the last trim.
    fn trim(&self) {
        let mut cache = self.cache.borrow_mut();
        cache
            .lines
            .retain(|_, entry| std::mem::take(&mut entry.used));
    }

    /// The layout of `line`, kept or laid out now; `None` past the last.
    fn layout(
        &self,
        line: usize,
        g: &Geometry,
        metrics: &dyn TextMetrics,
    ) -> Option<Rc<LineLayout>> {
        if let Some(entry) = self.cache.borrow_mut().lines.get_mut(&line) {
            entry.used = true;
            return Some(Rc::clone(&entry.layout));
        }
        let span = self.text.line_span(line).ok()?;
        let layout = Rc::new(LineLayout::new(
            &self.text,
            span,
            g.wrap,
            &g.measures,
            metrics,
        ));
        self.widest.set(self.widest.get().max(layout.width));
        self.cache.borrow_mut().lines.insert(
            line,
            Entry {
                layout: Rc::clone(&layout),
                used: true,
            },
        );
        Some(layout)
    }

    /// How many rows `line` takes: one with soft wrap off, which needs no
    /// layout.
    fn rows(&self, line: usize, g: &Geometry, metrics: &dyn TextMetrics) -> usize {
        if g.wrap.is_none() {
            return 1;
        }
        self.layout(line, g, metrics)
            .map_or(1, |layout| layout.rows.len().max(1))
    }

    /// `top` made sane: on a line and row the text has, scrolled less than
    /// a row's height.
    fn sane(&self, top: Top, g: &Geometry, metrics: &dyn TextMetrics) -> Top {
        let line = top.line.min(self.text.len_lines() - 1);
        let row = top.row.min(self.rows(line, g, metrics) - 1);
        let y = if top.y.is_finite() {
            top.y.clamp(0.0, g.measures.row)
        } else {
            0.0
        };
        let y = if y >= g.measures.row { 0.0 } else { y };
        Top { line, row, y }
    }

    /// The position `top` with `total` units from the top of its line's
    /// first row, which must be inside the line.
    fn within(&self, line: usize, total: f32, rows: usize, g: &Geometry) -> Top {
        let height = g.measures.row;
        let total = total.max(0.0);
        let row = ((total / height).floor() as usize).min(rows.saturating_sub(1));
        let y = (total - row as f32 * height).clamp(0.0, height);
        let y = if y >= height { 0.0 } else { y };
        Top { line, row, y }
    }

    /// `top` moved `by` units up, stopping at the text's start.
    fn up(&self, top: Top, by: f32, g: &Geometry, metrics: &dyn TextMetrics) -> Top {
        let height = g.measures.row;
        let mut line = top.line;
        let mut total = top.row as f32 * height + top.y - by;
        while total < 0.0 {
            if line == 0 {
                return Top::default();
            }
            line -= 1;
            total += self.rows(line, g, metrics) as f32 * height;
        }
        self.within(line, total, self.rows(line, g, metrics), g)
    }

    /// `top` moved `by` units down, stopping with the last row at the top.
    fn down(&self, top: Top, by: f32, g: &Geometry, metrics: &dyn TextMetrics) -> Top {
        let height = g.measures.row;
        let mut line = top.line;
        let mut total = top.row as f32 * height + top.y + by;
        loop {
            let rows = self.rows(line, g, metrics);
            let size = rows as f32 * height;
            if total < size {
                return self.within(line, total, rows, g);
            }
            if line + 1 >= self.text.len_lines() {
                return Top {
                    line,
                    row: rows - 1,
                    y: 0.0,
                };
            }
            total -= size;
            line += 1;
        }
    }

    /// `top`, or as far up as keeps the view full: never further down than
    /// shows the last row at the view's bottom. It lays out only the lines
    /// from `top` down that fill the view, and those above it it moves to.
    fn clamp_bottom(&self, top: Top, g: &Geometry, metrics: &dyn TextMetrics) -> Top {
        let height = g.measures.row;
        let view = g.bounds.height;
        let mut below = -(top.row as f32 * height + top.y);
        let mut line = top.line;
        loop {
            below += self.rows(line, g, metrics) as f32 * height;
            if below >= view {
                return top;
            }
            if line + 1 >= self.text.len_lines() {
                break;
            }
            line += 1;
        }
        self.up(top, view - below, g, metrics)
    }

    /// The line position of `top`: its line, and how far into the line's
    /// rows it is, as a fraction.
    fn line_position(&self, top: Top, g: &Geometry, metrics: &dyn TextMetrics) -> f32 {
        let height = g.measures.row;
        let rows = self.rows(top.line, g, metrics) as f32;
        let into = (top.row as f32 * height + top.y) / (rows * height);
        top.line as f32 + into.clamp(0.0, 1.0)
    }

    /// The position at line position `offset`.
    fn at_line_position(&self, offset: f32, g: &Geometry, metrics: &dyn TextMetrics) -> Top {
        let last = self.text.len_lines() - 1;
        let offset = if offset.is_finite() {
            offset.max(0.0)
        } else {
            0.0
        };
        let line = (offset.floor() as usize).min(last);
        let fraction = (offset - line as f32).clamp(0.0, 1.0);
        let rows = self.rows(line, g, metrics);
        let total = fraction * rows as f32 * g.measures.row;
        self.within(line, total, rows, g)
    }

    /// The rows on screen from `top` down, at least one: each row whose top
    /// is above the text area's bottom.
    pub(super) fn screen(
        &self,
        top: Top,
        g: &Geometry,
        metrics: &dyn TextMetrics,
    ) -> Vec<ScreenRow> {
        let mut rows = Vec::new();
        let bottom = g.bounds.y + g.bounds.height;
        let mut y = g.bounds.y - top.y;
        let mut line = top.line;
        let mut row = top.row;
        while let Some(layout) = self.layout(line, g, metrics) {
            while row < layout.rows.len() {
                rows.push(ScreenRow {
                    line,
                    row,
                    y,
                    layout: Rc::clone(&layout),
                });
                y += g.measures.row;
                row += 1;
                if y >= bottom {
                    return rows;
                }
            }
            line += 1;
            row = 0;
        }
        rows
    }

    /// The largest offset the text scrolls left: the widest line laid out
    /// so far and room for a cursor after it, less the width shown.
    fn max_left(&self, g: &Geometry) -> f32 {
        (self.widest.get() + CURSOR_WIDTH - g.text_width).max(0.0)
    }

    /// Moves to what the wheel and the scrollbars asked for, keeps the
    /// position inside the text, lays out the rows on screen and drops the
    /// other layouts. Returns whether the position moved, and the rows on
    /// screen. In bounds with no area, it keeps what was asked for until
    /// it has some.
    pub(super) fn resolve(
        &self,
        bounds: Rect,
        metrics: &dyn TextMetrics,
    ) -> (bool, Vec<ScreenRow>, Geometry) {
        let g = self.geometry(bounds, metrics);
        self.check(&g);
        let before = (self.top.get(), self.left.get());
        if !(bounds.width > 0.0 && bounds.height > 0.0) {
            let rows = self.screen(before.0, &g, metrics);
            return (false, rows, g);
        }
        let pending = self.pending.take();
        let height = g.measures.row;
        let mut top = self.sane(before.0, &g, metrics);
        if let Some(offset) = pending.jump {
            top = self.at_line_position(offset, &g, metrics);
        }
        // At most a view's height an event, so that one lays out no more
        // than a screen of lines.
        let page = bounds.height.max(WHEEL_ROWS * height);
        let dy = (pending.rows_y * height + pending.units_y).clamp(-page, page);
        if dy > 0.0 {
            top = self.down(top, dy, &g, metrics);
        } else if dy < 0.0 {
            top = self.up(top, -dy, &g, metrics);
        }
        top = self.clamp_bottom(top, &g, metrics);
        let rows = self.screen(top, &g, metrics);
        let left = if g.wrap.is_some() {
            0.0
        } else {
            let page = g.text_width.max(WHEEL_ROWS * height);
            let dx = (pending.rows_x * height + pending.units_x).clamp(-page, page);
            let left = pending.left.unwrap_or(before.1) + dx;
            if left.is_finite() {
                left.clamp(0.0, self.max_left(&g))
            } else {
                0.0
            }
        };
        self.top.set(top);
        self.left.set(left);
        self.trim();
        ((top, left) != before, rows, g)
    }

    /// What the scrollbars show (the [module documentation](super#scrolling)):
    /// the vertical one in lines, the horizontal one in units.
    pub(super) fn bars(&self, g: &Geometry, metrics: &dyn TextMetrics) -> (BarRange, BarRange) {
        let lines = self.text.len_lines() as f32;
        let shown = (g.bounds.height / g.measures.row).min(lines);
        let top = self.top.get();
        let vertical = (lines, shown, self.line_position(top, g, metrics));
        let horizontal = (
            self.widest.get() + CURSOR_WIDTH,
            g.text_width,
            self.left.get(),
        );
        (vertical, horizontal)
    }

    /// The byte offset of the grapheme cluster boundary nearest `point`, in
    /// window coordinates, on the row on screen nearest it.
    pub(super) fn point_to_byte(
        &self,
        bounds: Rect,
        point: Point,
        metrics: &dyn TextMetrics,
    ) -> usize {
        let g = self.geometry(bounds, metrics);
        self.check(&g);
        let top = self.sane(self.top.get(), &g, metrics);
        let rows = self.screen(top, &g, metrics);
        let height = g.measures.row;
        let first = g.bounds.y - top.y;
        let index = if point.y.is_finite() {
            ((point.y - first) / height).floor().max(0.0) as usize
        } else {
            0
        };
        let Some(row) = rows.get(index.min(rows.len().saturating_sub(1))) else {
            return 0;
        };
        let layout = &row.layout;
        let x = if point.x.is_finite() {
            point.x - g.text_x + self.left.get()
        } else {
            0.0
        };
        layout.span.start + nearest(layout, row.row, x)
    }

    /// Where a caret at `byte` shows: the top-left of its place, in window
    /// coordinates, if its row is on screen.
    pub(super) fn byte_to_point(
        &self,
        bounds: Rect,
        byte: usize,
        metrics: &dyn TextMetrics,
    ) -> Result<Option<Point>, TextError> {
        let line = self.text.byte_to_line(byte)?;
        let g = self.geometry(bounds, metrics);
        self.check(&g);
        let top = self.sane(self.top.get(), &g, metrics);
        let rows = self.screen(top, &g, metrics);
        let Some(layout) = self.layout(line, &g, metrics) else {
            return Ok(None);
        };
        let at = byte - layout.span.start;
        let row = layout.row_of(at);
        Ok(rows
            .iter()
            .find(|shown| shown.line == line && shown.row == row)
            .map(|shown| {
                Point::new(
                    g.text_x - self.left.get() + layout.caret_x(row, at),
                    shown.y,
                )
            }))
    }

    /// Scrolls the least that shows a caret at `byte` whole: its row, and
    /// with soft wrap off its place across. Returns whether it moved.
    pub(super) fn reveal(
        &self,
        bounds: Rect,
        byte: usize,
        metrics: &dyn TextMetrics,
    ) -> Result<bool, TextError> {
        let line = self.text.byte_to_line(byte)?;
        let g = self.geometry(bounds, metrics);
        self.check(&g);
        let Some(layout) = self.layout(line, &g, metrics) else {
            return Ok(false);
        };
        let at = byte - layout.span.start;
        let row = layout.row_of(at);
        let height = g.measures.row;
        let view = g.bounds.height;
        let before = (self.top.get(), self.left.get());
        let mut top = self.sane(before.0, &g, metrics);
        let target = Top { line, row, y: 0.0 };
        if target.before(top) {
            top = target;
        } else if !self.bottom_shows(top, target, &g, metrics) {
            top = self.up(target, (view - height).max(0.0), &g, metrics);
        }
        top = self.clamp_bottom(top, &g, metrics);
        self.screen(top, &g, metrics);
        let mut left = before.1;
        if g.wrap.is_none() {
            let x = layout.caret_x(row, at);
            if x < left {
                left = x;
            }
            if x + CURSOR_WIDTH > left + g.text_width {
                left = x + CURSOR_WIDTH - g.text_width;
            }
            left = left.clamp(0.0, self.max_left(&g));
        }
        self.top.set(top);
        self.left.set(left);
        self.trim();
        Ok((top, left) != before)
    }

    /// Whether the bottom of `target`'s row is inside the view from `top`,
    /// `target` being at or below `top`: it walks no more rows than fill
    /// the view.
    fn bottom_shows(&self, top: Top, target: Top, g: &Geometry, metrics: &dyn TextMetrics) -> bool {
        let height = g.measures.row;
        let view = g.bounds.height;
        let mut bottom = height - top.y;
        let (mut line, mut row) = (top.line, top.row);
        while bottom <= view {
            if (line, row) == (target.line, target.row) {
                return true;
            }
            row += 1;
            if row >= self.rows(line, g, metrics) {
                line += 1;
                row = 0;
                if line >= self.text.len_lines() {
                    return false;
                }
            }
            bottom += height;
        }
        false
    }

    /// The cursors that may touch `span`: those whose closed span meets
    /// it, found by binary search, as the selection keeps its cursors in
    /// order and apart.
    fn cursors_on(&self, span: &LineSpan) -> &[Cursor] {
        let cursors = self.selection.cursors();
        let low = cursors.partition_point(|cursor| cursor.end() < span.start);
        let high = cursors.partition_point(|cursor| cursor.start() <= span.end());
        cursors.get(low..high.max(low)).unwrap_or(&[])
    }

    /// Paints the rows on screen (the [module documentation](super#what-it-draws)).
    fn draw(&self, cx: &PaintContext<'_>, g: &Geometry, rows: &[ScreenRow], list: &mut DrawList) {
        let theme = cx.theme();
        let metrics = cx.metrics();
        let b = g.bounds;
        let height = g.measures.row;
        let last_line = self.text.len_lines() - 1;
        let marks: Vec<Marks> = rows
            .iter()
            .map(|row| self.marks(&row.layout.span, row.line == last_line))
            .collect();
        list.push_clip(b);
        for (row, marks) in rows.iter().zip(&marks) {
            if marks.wash {
                list.rect(
                    Rect::new(b.x, row.y, b.width, height),
                    theme.color(Token::CurrentLine),
                );
            }
        }
        for (row, marks) in rows.iter().zip(&marks) {
            if row.row != 0 {
                continue;
            }
            let number = (row.line + 1).to_string();
            let width = metrics.width(&number, &EDITOR_TEXT);
            let token = if marks.head {
                Token::Text2
            } else {
                Token::Text3
            };
            list.text(
                Point::new(
                    b.x + g.gutter - GUTTER_PADDING - width,
                    g.measures.baseline(row.y),
                ),
                number,
                EDITOR_TEXT,
                theme.color(token),
            );
        }
        list.push_clip(Rect::new(g.text_x, b.y, g.text_width, b.height));
        let origin = g.text_x - self.left.get();
        for row in rows {
            self.draw_selection(row, origin, g, theme, list);
        }
        for row in rows {
            draw_text(row, origin, self.left.get(), g, theme, list);
        }
        if cx.state().focused {
            for row in rows {
                self.draw_cursors(row, origin, g, theme, list);
            }
        }
        list.pop_clip();
        // Inside the bounds, as `ui`'s collections draw it: the view fills
        // its area, which would cut an outline outside them off.
        if cx.state().focus_visible {
            list.border(b, FOCUS_WIDTH, theme.color(Token::Teal));
        }
        list.pop_clip();
    }

    /// What the cursors mark on the line `span`.
    fn marks(&self, span: &LineSpan, last: bool) -> Marks {
        let on = |head: usize| head >= span.start && (head < span.end() || last);
        let (mut head, mut caret, mut selected) = (false, false, false);
        for cursor in self.cursors_on(span) {
            if on(cursor.head) {
                head = true;
                caret |= cursor.is_caret();
            }
            if !cursor.is_caret() && cursor.start() < span.end() && cursor.end() > span.start {
                selected = true;
            }
        }
        Marks {
            head,
            wash: caret && !selected,
        }
    }

    /// Paints the selected part of `row` in `teal-glow`, a selected line
    /// break as a space after the line's end, merging what touches.
    fn draw_selection(
        &self,
        row: &ScreenRow,
        origin: f32,
        g: &Geometry,
        theme: &Theme,
        list: &mut DrawList,
    ) {
        let layout = &row.layout;
        let span = layout.span;
        let last_row = row.row + 1 == layout.rows.len();
        let Some(spanned) = layout.rows.get(row.row) else {
            return;
        };
        let row_start = span.start + spanned.start;
        let row_end = if last_row {
            span.content_end
        } else {
            span.start + layout.row_end(row.row)
        };
        let mut run: Option<(f32, f32)> = None;
        let flush = |run: Option<(f32, f32)>, list: &mut DrawList| {
            if let Some((x0, x1)) = run
                && x1 > x0
            {
                list.rect(
                    Rect::new(origin + x0, row.y, x1 - x0, g.measures.row),
                    theme.color(Token::TealGlow),
                );
            }
        };
        for cursor in self.cursors_on(&span) {
            if cursor.is_caret() {
                continue;
            }
            let start = cursor.start().max(row_start);
            let end = cursor.end().min(row_end);
            let mut part = (start < end).then(|| {
                (
                    layout.caret_x(row.row, start - span.start),
                    layout.end_x(row.row, end - span.start),
                )
            });
            let breaks = last_row
                && span.line_break.is_some()
                && cursor.start() <= span.content_end
                && cursor.end() > span.content_end;
            if breaks {
                let (x0, x1) = part.unwrap_or((spanned.width, spanned.width));
                part = Some((x0, x1 + g.measures.space));
            }
            let Some((x0, x1)) = part else {
                continue;
            };
            run = match run {
                Some((a, z)) if x0 <= z => Some((a, z.max(x1))),
                other => {
                    flush(other, list);
                    Some((x0, x1))
                }
            };
        }
        flush(run, list);
    }

    /// Paints a `teal` bar at each cursor's head on `row`, once at each
    /// place.
    fn draw_cursors(
        &self,
        row: &ScreenRow,
        origin: f32,
        g: &Geometry,
        theme: &Theme,
        list: &mut DrawList,
    ) {
        let layout = &row.layout;
        let span = layout.span;
        let last = row.line + 1 == self.text.len_lines();
        let mut drawn: Option<f32> = None;
        for cursor in self.cursors_on(&span) {
            let head = cursor.head;
            if head < span.start || (head >= span.end() && !last) {
                continue;
            }
            let at = head - span.start;
            if layout.row_of(at) != row.row {
                continue;
            }
            let x = layout.caret_x(row.row, at);
            if drawn == Some(x) {
                continue;
            }
            drawn = Some(x);
            list.rect(
                Rect::new(origin + x, row.y, CURSOR_WIDTH, g.measures.row),
                theme.color(Token::Teal),
            );
        }
    }
}

/// What the cursors mark on a line.
struct Marks {
    /// A cursor's head is on it: its number is `text-2`.
    head: bool,
    /// A caret is on it, and nothing on it is selected: it has the
    /// current line's wash.
    wash: bool,
}

/// Paints the pieces of `row` that show across, `left` being how far the
/// text is scrolled: one text for each run of shown text, a marker for each
/// hidden character, and the cut marker.
fn draw_text(
    row: &ScreenRow,
    origin: f32,
    left: f32,
    g: &Geometry,
    theme: &Theme,
    list: &mut DrawList,
) {
    let layout = &row.layout;
    let Some(spanned) = layout.rows.get(row.row) else {
        return;
    };
    let pieces = layout.pieces.get(spanned.first..spanned.end).unwrap_or(&[]);
    // Pieces are in order across the row, so those that show are a range.
    let from = pieces.partition_point(|piece| piece.x + piece.advance <= left);
    let to = pieces
        .partition_point(|piece| piece.x < left + g.text_width)
        .max(from);
    let shown = pieces.get(from..to).unwrap_or(&[]);
    let base = g.measures.baseline(row.y);
    let flush = |run: Option<&Piece>, end: usize, list: &mut DrawList| {
        if let Some(first) = run {
            list.text(
                Point::new(origin + first.x, base),
                layout.text.get(first.at..end).unwrap_or(""),
                EDITOR_TEXT,
                theme.color(Token::Text),
            );
        }
    };
    let mut run = None;
    for piece in shown {
        let (mark, token) = match piece.kind {
            Kind::Text => {
                run = run.or(Some(piece));
                continue;
            }
            Kind::Tab => {
                flush(run.take(), piece.at, list);
                continue;
            }
            Kind::Marker => (MARKER.to_string(), Token::Error),
            Kind::Cut => (CUT_MARKER.to_string(), Token::Text3),
        };
        flush(run.take(), piece.at, list);
        list.text(
            Point::new(origin + piece.x, base),
            mark,
            EDITOR_TEXT,
            theme.color(token),
        );
    }
    if !shown.is_empty() {
        flush(run, layout.piece_end(spanned.first + to - 1), list);
    }
}

/// The byte of `layout`'s line, from its start, of the caret stop on `row`
/// nearest `x` across the row: a grapheme cluster's start, or the row's
/// end, which on a row that wraps is the next row's start, and past the
/// kept text of a cut line, where it is cut. Of two stops at the same
/// place, the first.
fn nearest(layout: &LineLayout, row: usize, x: f32) -> usize {
    let Some(spanned) = layout.rows.get(row) else {
        return 0;
    };
    let pieces = layout.pieces.get(spanned.first..spanned.end).unwrap_or(&[]);
    let mut best: Option<(usize, f32)> = None;
    for piece in pieces.iter().filter(|piece| piece.stop) {
        let distance = (x - piece.x).abs();
        if best.is_none_or(|(_, nearest)| distance < nearest) {
            best = Some((piece.at, distance));
        }
    }
    match best {
        Some((at, distance)) if distance <= (x - spanned.content).abs() => at,
        _ => layout.row_end(row),
    }
}

impl Widget for Area {
    /// Paints what the [module documentation](super#what-it-draws) says,
    /// after moving to what the wheel asked for.
    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let (_, rows, g) = self.resolve(cx.bounds(), cx.metrics());
        self.draw(cx, &g, &rows, list);
    }

    /// Takes the wheel (the [module documentation](super#scrolling)).
    fn event(&mut self, cx: &mut EventContext<'_>, event: &InputEvent) -> Handled {
        let InputEvent::Wheel {
            delta, modifiers, ..
        } = *event
        else {
            return Handled::No;
        };
        let (x, y, notches) = match delta {
            ScrollDelta::Lines { x, y } => (sane(x), sane(y), true),
            ScrollDelta::Logical { x, y } => (sane(x), sane(y), false),
        };
        let (x, y) = if modifiers.contains(Modifiers::SHIFT) {
            (x + y, 0.0)
        } else {
            (x, y)
        };
        let x = if self.wrap { 0.0 } else { x };
        if x == 0.0 && y == 0.0 {
            return Handled::No;
        }
        let mut pending = self.pending.get();
        if notches {
            pending.rows_x = sane(pending.rows_x + x * WHEEL_ROWS);
            pending.rows_y = sane(pending.rows_y + y * WHEEL_ROWS);
        } else {
            pending.units_x = sane(pending.units_x + x);
            pending.units_y = sane(pending.units_y + y);
        }
        self.pending.set(pending);
        cx.request_paint();
        Handled::Yes
    }
}

/// `value` with NaN as 0, clamped to ±[`MAX_EXTENT`].
fn sane(value: f32) -> f32 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(-MAX_EXTENT, MAX_EXTENT)
    }
}

/// How many decimal digits `n` has.
fn digits(n: usize) -> usize {
    n.checked_ilog10().map_or(1, |log| log as usize + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_count_decimal_places() {
        assert_eq!(digits(0), 1);
        assert_eq!(digits(9), 1);
        assert_eq!(digits(10), 2);
        assert_eq!(digits(10_000), 5);
        assert_eq!(digits(usize::MAX), 20);
    }

    #[test]
    fn positions_order_by_line_row_and_offset() {
        let at = |line, row, y| Top { line, row, y };
        assert!(at(0, 0, 0.0).before(at(0, 0, 1.0)));
        assert!(at(0, 1, 0.0).before(at(1, 0, 0.0)));
        assert!(!at(2, 0, 0.0).before(at(1, 5, 9.0)));
        assert!(!at(1, 0, 3.0).before(at(1, 0, 3.0)));
    }
}
