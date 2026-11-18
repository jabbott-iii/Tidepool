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

//! Fields: the text input and the search field (`.search input`), one line
//! of text edited by extended grapheme cluster (ADR-0049).

use std::cell::{Cell, RefCell};
use std::fmt;
use std::ops::Range;

use tidepool_core::geom::{Point, Rect, Size};
use tidepool_core::input::{InputEvent, Key, Modifiers, MouseButton, TextLength};
use tidepool_text::pos::grapheme;

use crate::a11y::{Role, Semantics};
use crate::draw::{DrawList, PaintContext};
use crate::input::{ActionKind, EventContext, Handled};
use crate::metrics::TextMetrics;
use crate::theme::{Theme, Token};
use crate::tree::Widget;

use super::{
    CARET_WIDTH, Control, FIELD_HEIGHT, FIELD_PADDING, FIELD_RING, FIELD_TEXT, FIELD_WIDTH, Look,
    SEARCH_PADDING_LEFT, SEARCH_PADDING_RIGHT, baseline, dimmed, grow, inset,
};

/// The most bytes a field holds; what would take it past that is cut, at
/// a char boundary.
pub const MAX_INPUT_BYTES: usize = 4096;

/// What a secret field paints for each grapheme cluster of its text.
const BULLET: &str = "\u{2022}";

/// Which field it is, which sets its shape and role.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FieldKind {
    /// A text input: a `slate` rect.
    #[default]
    Text,
    /// The search field: a `slate` pill, with a magnifier on its right.
    Search,
}

/// Where the last paint put the text's grapheme cluster boundaries, so
/// that a click can find the nearest: a handler gets no metrics.
#[derive(Clone, Debug, Default)]
struct Stops {
    /// The field's [`revision`](TextInput::revision) when they were
    /// painted: stops of another text are never used.
    revision: Option<u64>,
    /// The x of the text's origin, in window coordinates, less the scroll.
    origin: f32,
    /// Each boundary of the text, as a byte offset into it, with its x from
    /// the origin, in order.
    stops: Vec<(usize, f32)>,
}

/// One line of editable text: a text input, or the search field.
///
/// The text is a `String` of at most [`MAX_INPUT_BYTES`], edited at a
/// caret and a selection whose ends are always on extended grapheme
/// cluster boundaries (`tidepool_text::pos::grapheme`), so no edit splits a
/// cluster such as an emoji with a skin tone.
///
/// While it has focus:
///
/// - typed and pasted text (`Text` events) replaces the selection, with
///   control characters dropped and line breaks and tabs made spaces;
/// - Left and Right move a cluster, Home and End to the ends, each
///   extending the selection with Shift; Ctrl+A selects everything;
/// - Backspace and Delete delete the selection, or the cluster before or
///   after the caret;
/// - Enter emits [`ActionKind::Submit`], and Escape [`ActionKind::Dismiss`];
/// - a click places the caret at the nearest boundary, a Shift+click
///   extends the selection to it, and a drag selects. A handler has no
///   metrics, so the boundaries are where the last paint put them; a
///   click before the first paint, or after an edit and before the next,
///   puts the caret at the end.
///
/// Each edit emits [`ActionKind::Change`] and sets the node's value. Keys
/// with Ctrl, Alt or Super but Ctrl+A, and Up, Down, Page Up, Page Down,
/// Tab and the function keys, go on to the owner, which holds the
/// clipboard ([`selected_text`](Self::selected_text),
/// [`replace_selection`](Self::replace_selection)) and any list the field
/// steers. Keys that type text are kept from the owner's shortcuts; the
/// numpad's digits count among them, as `core`'s keys name places and not
/// what Num Lock makes of them, so with Num Lock off they move nothing.
///
/// It paints `slate`, its text in `text` and its placeholder in `text-2`
/// (no `text-3` on `slate`), in [`FIELD_TEXT`], as written. Hovered, it has
/// a `teal-line` border; with focus, a `teal` border and a [`FIELD_RING`]
/// `teal-ring` around it, its selection in `teal-glow` and a `teal` caret,
/// which does not blink. It takes focus.
///
/// A [secret](Self::secret) field paints a bullet for each cluster, never
/// gives its text as the node's value or from
/// [`selected_text`](Self::selected_text), and its `Debug`, as every
/// field's, shows the text's length only (ADR-0043).
///
/// ```
/// use tidepool_ui::a11y::Role;
/// use tidepool_ui::widgets::{Control, TextInput};
///
/// let mut find = TextInput::search("Find in files").placeholder("Search");
/// find.set_text("tide\u{1f30a}");
/// assert_eq!(find.semantics().role, Role::SearchInput);
/// assert_eq!(find.semantics().value.as_deref(), Some("tide\u{1f30a}"));
/// assert_eq!(find.selection(), 8..8);
/// ```
pub struct TextInput {
    kind: FieldKind,
    name: String,
    placeholder: String,
    text: String,
    caret: usize,
    anchor: usize,
    secret: bool,
    /// A primary press is selecting.
    dragging: bool,
    /// Counts the changes of the text, so that caret stops painted for
    /// another text are known.
    revision: u64,
    /// How far the text is scrolled left, so that the caret shows; kept
    /// from one paint to the next.
    scroll: Cell<f32>,
    stops: RefCell<Stops>,
}

impl TextInput {
    /// An empty text input named `name`, as its label would name it.
    pub fn new(name: impl Into<String>) -> Self {
        Self::of(FieldKind::Text, name.into())
    }

    /// An empty search field named `name`.
    pub fn search(name: impl Into<String>) -> Self {
        Self::of(FieldKind::Search, name.into())
    }

    fn of(kind: FieldKind, name: String) -> Self {
        Self {
            kind,
            name,
            placeholder: String::new(),
            text: String::new(),
            caret: 0,
            anchor: 0,
            secret: false,
            dragging: false,
            revision: 0,
            scroll: Cell::new(0.0),
            stops: RefCell::new(Stops::default()),
        }
    }

    /// The same, showing `placeholder` while it is empty.
    #[must_use]
    pub fn placeholder(self, placeholder: impl Into<String>) -> Self {
        Self {
            placeholder: one_line(&placeholder.into(), MAX_INPUT_BYTES),
            ..self
        }
    }

    /// The same, holding a secret: a password or a key.
    #[must_use]
    pub fn secret(self) -> Self {
        Self {
            secret: true,
            ..self
        }
    }

    /// Which field it is.
    pub const fn kind(&self) -> FieldKind {
        self.kind
    }

    /// Whether it holds a secret.
    pub const fn is_secret(&self) -> bool {
        self.secret
    }

    /// Its text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Replaces its text with `text`, made one line and cut to
    /// [`MAX_INPUT_BYTES`] as typed text is, and puts the caret at its end.
    pub fn set_text(&mut self, text: &str) {
        self.text = one_line(text, MAX_INPUT_BYTES);
        self.caret = self.text.len();
        self.anchor = self.caret;
        self.revision = self.revision.wrapping_add(1);
    }

    /// The selection, as a range of bytes; empty, at the caret, when
    /// nothing is selected.
    pub fn selection(&self) -> Range<usize> {
        self.caret.min(self.anchor)..self.caret.max(self.anchor)
    }

    /// The caret, as a byte offset.
    pub const fn caret(&self) -> usize {
        self.caret
    }

    /// Selects everything.
    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.caret = self.text.len();
    }

    /// Selects `range` of bytes, its ends moved forward to the nearest
    /// cluster boundary and kept within the text; the caret goes to its
    /// end.
    pub fn select(&mut self, range: Range<usize>) {
        self.anchor = self.snap(range.start);
        self.caret = self.snap(range.end);
    }

    /// The selected text, to copy; `None` when nothing is selected, and
    /// for a secret.
    pub fn selected_text(&self) -> Option<&str> {
        let range = self.selection();
        if self.secret || range.is_empty() {
            return None;
        }
        self.text.get(range)
    }

    /// Replaces the selection with `text`, as typing or pasting does, and
    /// returns whether the text changed.
    pub fn replace_selection(&mut self, text: &str) -> bool {
        let range = self.selection();
        let room = MAX_INPUT_BYTES - (self.text.len() - range.len());
        let insert = one_line(text, room);
        if insert.is_empty() && range.is_empty() {
            return false;
        }
        self.text.replace_range(range.clone(), &insert);
        self.caret = self.snap(range.start + insert.len());
        self.anchor = self.caret;
        self.revision = self.revision.wrapping_add(1);
        true
    }

    /// The nearest cluster boundary at or after `byte`, within the text.
    fn snap(&self, byte: usize) -> usize {
        if byte >= self.text.len() {
            self.text.len()
        } else if grapheme::is_boundary(&self.text, byte) {
            byte
        } else {
            grapheme::next_boundary(&self.text, byte).unwrap_or(self.text.len())
        }
    }

    /// Moves the caret to `to`, extending the selection if `extend`.
    fn move_to(&mut self, to: usize, extend: bool) {
        self.caret = to;
        if !extend {
            self.anchor = to;
        }
    }

    /// Deletes the selection, or the range `fallback` gives when it is
    /// empty; returns whether the text changed.
    fn delete(&mut self, fallback: impl FnOnce(&Self) -> Option<Range<usize>>) -> bool {
        let mut range = self.selection();
        if range.is_empty() {
            match fallback(self) {
                Some(found) if !found.is_empty() => range = found,
                _ => return false,
            }
        }
        self.text.replace_range(range.clone(), "");
        self.caret = self.snap(range.start);
        self.anchor = self.caret;
        self.revision = self.revision.wrapping_add(1);
        true
    }

    /// The space the text is laid out in, inside the padding.
    fn content(&self, rect: Rect) -> Rect {
        let (left, right) = match self.kind {
            FieldKind::Text => (FIELD_PADDING, FIELD_PADDING),
            FieldKind::Search => (SEARCH_PADDING_LEFT, SEARCH_PADDING_RIGHT),
        };
        let width = (rect.width - left - right).max(0.0);
        Rect::new(rect.x + left, rect.y, width, rect.height)
    }

    /// What is painted for the text, and the boundaries of the text with
    /// their offsets into it: the text itself, or a bullet a cluster.
    fn shown(&self) -> (String, Vec<(usize, usize)>) {
        let mut map = vec![(0, 0)];
        if self.secret {
            let mut shown = String::new();
            let mut at = 0;
            for cluster in grapheme::graphemes(&self.text) {
                at += cluster.len();
                shown.push_str(BULLET);
                map.push((at, shown.len()));
            }
            (shown, map)
        } else {
            let mut at = 0;
            for cluster in grapheme::graphemes(&self.text) {
                at += cluster.len();
                map.push((at, at));
            }
            (self.text.clone(), map)
        }
    }

    /// The boundary nearest `x`, in window coordinates, among those the
    /// last paint placed; the end of the text if the text changed since,
    /// or nothing was painted yet. Always a boundary of the text.
    fn nearest(&self, x: f32) -> usize {
        let stops = self.stops.borrow();
        if stops.revision != Some(self.revision) {
            return self.text.len();
        }
        let at = x - stops.origin;
        let mut best = self.text.len();
        let mut distance = f32::INFINITY;
        for &(byte, stop) in &stops.stops {
            let d = (stop - at).abs();
            if d < distance {
                distance = d;
                best = byte;
            }
        }
        self.snap(best)
    }

    /// Paints the shape for `look`, and the ring and focused border.
    fn paint_shape(
        &self,
        theme: &Theme,
        rect: Rect,
        look: Look,
        focused: bool,
        list: &mut DrawList,
    ) {
        let c = |token| theme.color(token);
        let shape = |list: &mut DrawList, rect: Rect, token: Token| match self.kind {
            FieldKind::Text => list.rect(rect, c(token)),
            FieldKind::Search => list.pill(rect, c(token)),
        };
        if focused {
            shape(list, grow(rect, FIELD_RING), Token::TealRing);
        }
        shape(list, rect, Token::Slate);
        let border = if focused {
            Some(Token::Teal)
        } else if look == Look::Rest {
            None
        } else {
            Some(Token::TealLine)
        };
        if let Some(border) = border {
            // A 1-unit border over the fill, as CSS paints one.
            match self.kind {
                FieldKind::Text => list.border(rect, 1.0, c(border)),
                FieldKind::Search => {
                    list.pill(rect, c(border));
                    list.pill(inset(rect, 1.0), c(Token::Slate));
                }
            }
        }
        if self.kind == FieldKind::Search {
            magnifier(theme, rect, list);
        }
    }

    /// Paints the text, its selection and the caret, clipped to `content`,
    /// and keeps the caret stops.
    fn paint_text(&self, cx: &PaintContext<'_>, content: Rect, focused: bool, list: &mut DrawList) {
        let metrics = cx.metrics();
        let theme = cx.theme();
        let base = baseline(content, metrics, &FIELD_TEXT);
        let line = metrics.line(&FIELD_TEXT);
        let (shown, map) = self.shown();
        let xs = stops(&shown, &map, metrics);
        let total = xs.last().copied().unwrap_or(0.0);
        let index = |byte: usize| map.partition_point(|&(at, _)| at < byte).min(xs.len() - 1);
        let caret_x = xs[index(self.caret)];
        // Scroll as little as keeps the caret in view.
        let room = (content.width - CARET_WIDTH).max(0.0);
        let mut scroll = self.scroll.get();
        if caret_x - scroll > room {
            scroll = caret_x - room;
        }
        if caret_x < scroll {
            scroll = caret_x;
        }
        scroll = scroll.clamp(0.0, (total - room).max(0.0));
        self.scroll.set(scroll);
        let origin_x = content.x - scroll;
        *self.stops.borrow_mut() = Stops {
            revision: Some(self.revision),
            origin: origin_x,
            stops: map
                .iter()
                .map(|&(byte, _)| byte)
                .zip(xs.iter().copied())
                .collect(),
        };
        let top = base - line.ascent;
        let height = line.ascent + line.descent;
        list.push_clip(content);
        let selection = self.selection();
        if focused && !selection.is_empty() {
            let x0 = xs[index(selection.start)];
            let x1 = xs[index(selection.end)];
            list.rect(
                Rect::new(origin_x + x0, top, x1 - x0, height),
                theme.color(Token::TealGlow),
            );
        }
        if shown.is_empty() {
            if !self.placeholder.is_empty() {
                list.text(
                    Point::new(origin_x, base),
                    self.placeholder.clone(),
                    FIELD_TEXT,
                    theme.color(Token::Text2),
                );
            }
        } else {
            list.text(
                Point::new(origin_x, base),
                shown,
                FIELD_TEXT,
                theme.color(Token::Text),
            );
        }
        if focused {
            list.rect(
                Rect::new(origin_x + caret_x, top, CARET_WIDTH, height),
                theme.color(Token::Teal),
            );
        }
        list.pop_clip();
    }

    /// Handles a key going down; `None` for a key it leaves to the owner.
    fn key(
        &mut self,
        cx: &mut EventContext<'_>,
        key: Key,
        modifiers: Modifiers,
    ) -> Option<Handled> {
        let shift = modifiers.contains(Modifiers::SHIFT);
        let chord = modifiers.intersects(Modifiers::CTRL | Modifiers::ALT | Modifiers::SUPER);
        if chord {
            if key == Key::A && modifiers == Modifiers::CTRL {
                self.select_all();
                cx.request_paint();
                return Some(Handled::Yes);
            }
            return None;
        }
        let selection = self.selection();
        match key {
            Key::Left => {
                let to = if !shift && !selection.is_empty() {
                    selection.start
                } else {
                    grapheme::prev_boundary(&self.text, self.caret).unwrap_or(0)
                };
                self.move_to(to, shift);
            }
            Key::Right => {
                let to = if !shift && !selection.is_empty() {
                    selection.end
                } else {
                    grapheme::next_boundary(&self.text, self.caret).unwrap_or(self.text.len())
                };
                self.move_to(to, shift);
            }
            Key::Home => self.move_to(0, shift),
            Key::End => self.move_to(self.text.len(), shift),
            Key::Backspace => {
                let caret = self.caret;
                let changed = self.delete(|field| {
                    grapheme::prev_boundary(&field.text, caret).map(|from| from..caret)
                });
                if changed {
                    self.changed(cx);
                }
            }
            Key::Delete => {
                let caret = self.caret;
                let changed = self.delete(|field| {
                    grapheme::next_boundary(&field.text, caret).map(|to| caret..to)
                });
                if changed {
                    self.changed(cx);
                }
            }
            Key::Enter | Key::NumpadEnter => cx.emit(ActionKind::Submit),
            Key::Escape => cx.emit(ActionKind::Dismiss),
            _ if types_text(key) => {}
            _ => return None,
        }
        cx.request_paint();
        Some(Handled::Yes)
    }

    /// Tells the owner the text changed, and gives the node its new value.
    fn changed(&self, cx: &mut EventContext<'_>) {
        cx.emit(ActionKind::Change);
        cx.set_semantics(self.semantics());
    }
}

/// Shows the text and the placeholder as their lengths only (ADR-0043).
impl fmt::Debug for TextInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextInput")
            .field("kind", &self.kind)
            .field("name", &self.name)
            .field("placeholder", &TextLength(&self.placeholder))
            .field("text", &TextLength(&self.text))
            .field("caret", &self.caret)
            .field("anchor", &self.anchor)
            .field("secret", &self.secret)
            .finish_non_exhaustive()
    }
}

impl Widget for TextInput {
    fn measure(&self, _available: Size, _metrics: &dyn TextMetrics) -> Size {
        Size::new(FIELD_WIDTH, FIELD_HEIGHT)
    }

    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let rect = cx.bounds();
        let state = cx.state();
        let focused = state.focused && !state.disabled;
        dimmed(cx, list, |list| {
            self.paint_shape(cx.theme(), rect, Look::of(state), focused, list);
            self.paint_text(cx, self.content(rect), focused, list);
        });
    }

    fn event(&mut self, cx: &mut EventContext<'_>, event: &InputEvent) -> Handled {
        match *event {
            InputEvent::KeyDown { key, modifiers, .. } => {
                self.key(cx, key, modifiers).unwrap_or(Handled::No)
            }
            InputEvent::Text(ref text) => {
                if self.replace_selection(text) {
                    self.changed(cx);
                }
                cx.request_paint();
                Handled::Yes
            }
            InputEvent::MouseDown {
                button: MouseButton::Left,
                position,
                modifiers,
            } => {
                let to = self.nearest(position.x);
                self.move_to(to, modifiers.contains(Modifiers::SHIFT));
                self.dragging = true;
                cx.request_focus();
                cx.request_paint();
                Handled::Yes
            }
            InputEvent::MouseMove { position, .. } if self.dragging && cx.state().pressed => {
                let to = self.nearest(position.x);
                self.move_to(to, true);
                cx.request_paint();
                Handled::Yes
            }
            InputEvent::MouseUp {
                button: MouseButton::Left,
                ..
            } => {
                self.dragging = false;
                Handled::Yes
            }
            _ => Handled::No,
        }
    }
}

impl Control for TextInput {
    fn semantics(&self) -> Semantics {
        let role = match self.kind {
            FieldKind::Text => Role::TextInput,
            FieldKind::Search => Role::SearchInput,
        };
        let semantics = Semantics::new(role, self.name.clone());
        if self.secret {
            semantics
        } else {
            semantics.with_value(self.text.clone())
        }
    }

    fn focusable(&self) -> bool {
        true
    }
}

/// The x of each boundary of `shown`, from its origin, for the boundaries
/// `map` lists: the pen before each cluster, summed from each cluster's
/// advance in context (a pair measured less the cluster after it), so
/// that kerning and letter-spacing between clusters count, in time linear
/// in the clusters.
fn stops(shown: &str, map: &[(usize, usize)], metrics: &dyn TextMetrics) -> Vec<f32> {
    let mut xs = Vec::with_capacity(map.len());
    let mut x = 0.0_f32;
    xs.push(x);
    for (i, window) in map.windows(2).enumerate() {
        let (start, end) = (window[0].1, window[1].1);
        let advance = match map.get(i + 2) {
            Some(&(_, next)) => {
                let pair = shown.get(start..next).unwrap_or("");
                let after = shown.get(end..next).unwrap_or("");
                metrics.width(pair, &FIELD_TEXT) - metrics.width(after, &FIELD_TEXT)
            }
            None => metrics.width(shown.get(start..end).unwrap_or(""), &FIELD_TEXT),
        };
        x += advance.max(0.0);
        xs.push(x);
    }
    xs
}

/// `text` made one line, as a field holds it: line breaks (CR LF, LF, CR)
/// and tabs become spaces, other control characters are dropped, and it
/// is cut at a char boundary to at most `max` bytes.
fn one_line(text: &str, max: usize) -> String {
    let mut out = String::with_capacity(text.len().min(max));
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let c = match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                ' '
            }
            '\n' | '\t' => ' ',
            c if c.is_control() => continue,
            c => c,
        };
        if out.len() + c.len_utf8() > max {
            break;
        }
        out.push(c);
    }
    out
}

/// Whether `key`, with no modifier but Shift, types text: letters, digits,
/// Space, punctuation and the numpad's digits and operators. A field keeps
/// them from the owner's shortcuts; their text comes as `Text` events.
fn types_text(key: Key) -> bool {
    let usage = key as u8;
    let letters_digits_space =
        (Key::A as u8..=Key::Digit0 as u8).contains(&usage) || key == Key::Space;
    let punctuation =
        (Key::Minus as u8..=Key::Slash as u8).contains(&usage) || key == Key::IntlBackslash;
    let numpad = (Key::NumpadDivide as u8..=Key::NumpadDecimal as u8).contains(&usage)
        && key != Key::NumpadEnter
        || key == Key::NumpadEqual;
    letters_digits_space || punctuation || numpad
}

/// The search field's magnifier, in `text-2`, centred in its right
/// padding: a ring of a 9-unit circle 1.5 wide, and a handle down to the
/// right.
fn magnifier(theme: &Theme, rect: Rect, list: &mut DrawList) {
    let centre_x = rect.x + rect.width - SEARCH_PADDING_RIGHT / 2.0 - 1.5;
    let centre_y = rect.y + rect.height / 2.0 - 1.5;
    let ring = Rect::new(centre_x - 4.5, centre_y - 4.5, 9.0, 9.0);
    list.pill(ring, theme.color(Token::Text2));
    list.pill(inset(ring, 1.5), theme.color(Token::Slate));
    // The handle leaves the ring at 45°: 4.5 × cos 45° ≈ 3.18 from the
    // centre on each axis.
    list.line(
        Point::new(centre_x + 3.18, centre_y + 3.18),
        Point::new(centre_x + 6.5, centre_y + 6.5),
        1.5,
        theme.color(Token::Text2),
    );
}
