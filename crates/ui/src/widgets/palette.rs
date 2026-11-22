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

//! The command palette: a modal overlay holding a search field over a list
//! of the items its owner ranks (ADR-0052, ADR-0054).

use std::any::Any;
use std::fmt;

use tidepool_core::geom::{Point, Size};
use tidepool_core::input::{InputEvent, Key, Modifiers, TextLength};

use crate::a11y::{Role, Semantics};
use crate::draw::{Corners, DrawList, PaintContext};
use crate::error::UiError;
use crate::input::{ActionKind, EventContext, Handled};
use crate::layout::{Align, Insets, Length, MAX_EXTENT, Style};
use crate::metrics::TextMetrics;
use crate::overlay::{Anchor, OverlayKind};
use crate::theme::Token;
use crate::tree::{MAX_NODES, Node, NodeId, Tree, Widget};

use super::modal::kept_lines;
use super::{
    BRACKET_ARM, BRACKET_THICKNESS, Collection, Control, FIELD_HEIGHT, FOCUS_WIDTH, MAX_VIEW_ROWS,
    MODAL_MARGIN, MODAL_TEXT, PALETTE_PADDING, PALETTE_ROWS, PALETTE_WIDTH, ROW_HEIGHT, Rows,
    TextInput, baseline, dimmed, line_height,
};

/// The command palette: a search field over a list of the items its owner
/// ranks, in a modal overlay at the top of the window (ADR-0052, ADR-0054).
///
/// # What it holds
///
/// Opened with [`open`](Self::open), it holds, one above another:
///
/// - its field ([`Palette::field`]), a search field
///   ([`TextInput`](super::TextInput)) where focus goes as it opens, so the
///   user types at once (ADR-0054); each edit emits
///   [`ActionKind::Change`] from the field, and [`Palette::query`] is its
///   text;
/// - its list ([`Palette::list`]), a [`Collection`] of the items the owner
///   gives it with [`Palette::set_items`], ranked: `ui` matches nothing.
///   Each item is a [`Row`](super::Row) whose
///   [`matched`](super::Row::matched) ranges, the characters the query
///   matched, are drawn in `teal-hi`. It shows at most [`PALETTE_ROWS`]
///   rows at once and scrolls the rest, keeping nodes only for the rows on
///   screen, as any collection does (ADR-0052), and never takes focus. In
///   a window too short for the palette, the list gives up room first;
/// - a line saying there is nothing to show, its empty text, in place of
///   the list while the owner gives it no items.
///
/// # Keys and the pointer
///
/// Focus stays in the field; while the palette is open and no node has
/// focus, its field takes it again ([`overlay`](crate::overlay#giving-focus-back)).
/// Up and Down move the selection through the items, wrapping from the
/// last to the first and back, as the WAI-ARIA combobox pattern allows and
/// other editors' palettes do; Page Up and Page Down move it
/// [`PALETTE_ROWS`] rows, stopping at the ends. Each move emits
/// [`ActionKind::Select`] and scrolls the item into view; a key that
/// moves nothing, with no items, at an end or with one item, emits
/// nothing. Home and End stay the field's: they move its caret, which the
/// user needs while typing. Enter emits [`ActionKind::Activate`] if an
/// item is selected, and Escape [`ActionKind::Dismiss`]; the owner runs
/// [`Palette::selected`] and closes it with [`Tree::close_overlay`]. When
/// the owner gives it items, the item that was selected stays selected if
/// it is still among them; otherwise the first is, so Enter runs the
/// owner's first-ranked item (ADR-0054).
///
/// The pointer hovers items. A press on one selects it, and emits `Select`
/// if that moved the selection; its release activates it, emitting
/// `Activate`, if it is over the item the press went down on, that item is
/// still selected, and the owner gave the palette no items since the
/// press. So an item that a ranking arriving mid-click puts under the
/// pointer never runs. A press anywhere else, on the palette's padding or
/// outside it, reaches no control and comes back
/// [`blocked`](crate::input::Response::blocked), as a dialog's does: an
/// owner that closes the palette on a click outside it checks where the
/// press was. As with any modal, a press before the pointer has moved
/// more than [`PRESS_SLOP`](crate::input::PRESS_SLOP) device pixels since
/// the palette opened counts as on the palette (ADR-0055, ADR-0056).
///
/// Every `Select`, `Activate` and `Dismiss` comes from the palette's root,
/// by key or by pointer; only the field's [`ActionKind::Change`] comes
/// from the field.
///
/// # In the accessibility tree
///
/// A dialog named as the palette is, marked modal. Its field is a combo
/// box, named so too, its value the query, expanded, whose
/// [`active`](crate::a11y::AccessNode::active) option is the selected
/// item while focus stays in the field (`aria-activedescendant`). Its list
/// is a list named `Results`, its value the count and the selection, with
/// a list item for each row on screen, the selected one selected. The
/// empty line is a status, named by its text, so a screen reader says it
/// when it shows.
///
/// # Its look
///
/// As a modal dialog's ([`Modal`](super::Modal), ADR-0053, ADR-0054): the
/// window under it dimmed with `panel`, `panel`'s solid value, opaque,
/// with a 1-unit `line` border and the `teal` bracket corners; at the top
/// of the window, centered, [`MODAL_MARGIN`] from its top,
/// [`PALETTE_WIDTH`] wide, [`PALETTE_PADDING`] in, its field the search
/// field's look, its rows a list's, the empty line in [`MODAL_TEXT`]
/// (`text-2`). The width is one and a half times the dialog's: the site's
/// popup panel at the IDE's scale is the dialog's, and a palette's rows
/// hold names and paths wider than a dialog's message.
///
/// Its name, placeholder and empty text are each one line, cut to
/// [`MAX_CELL_BYTES`](super::MAX_CELL_BYTES) and cleaned as a
/// [`Modal`](super::Modal)'s title is.
///
/// ```
/// use tidepool_core::geom::Size;
/// use tidepool_core::input::{InputEvent, Key, Modifiers};
/// use tidepool_ui::input::{Action, ActionKind};
/// use tidepool_ui::metrics::FixedMetrics;
/// use tidepool_ui::widgets::{Palette, Row};
/// use tidepool_ui::Tree;
///
/// let mut tree = Tree::new();
/// let palette = Palette::new("Command palette").placeholder("Type a command").open(&mut tree)?;
/// let field = Palette::field(&tree, palette).expect("its field");
/// assert_eq!(tree.focused(), Some(field));
///
/// // The user types `o`; the owner ranks what matches and gives it.
/// let response = tree.handle(&InputEvent::Text("o".to_string()));
/// assert_eq!(response.actions, [Action { node: field, kind: ActionKind::Change }]);
/// assert_eq!(Palette::query(&tree, palette), Some("o"));
/// let items = vec![Row::new("Open File").matched([0..1]), Row::new("Go to Symbol").matched([1..2])];
/// Palette::set_items(&mut tree, palette, items)?;
/// tree.layout(Size::new(800.0, 600.0), &FixedMetrics::DEFAULT);
/// assert_eq!(Palette::selected(&tree, palette), Some(0));
///
/// // Down, then Enter: the owner runs the second.
/// let key = |key| InputEvent::KeyDown { key, modifiers: Modifiers::NONE, repeat: false };
/// tree.handle(&key(Key::Down));
/// let response = tree.handle(&key(Key::Enter));
/// assert_eq!(response.actions, [Action { node: palette, kind: ActionKind::Activate }]);
/// assert_eq!(Palette::selected(&tree, palette), Some(1));
/// tree.close_overlay(palette)?;
/// # Ok::<(), tidepool_ui::UiError>(())
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct Palette {
    name: String,
    placeholder: String,
    empty: String,
}

impl Palette {
    /// A palette named `name`, with no placeholder, and `No results` as its
    /// empty text.
    pub fn new(name: impl AsRef<str>) -> Self {
        Self {
            name: one_line(name.as_ref()),
            placeholder: String::new(),
            empty: "No results".to_string(),
        }
    }

    /// The same, with `placeholder` in its empty field.
    #[must_use]
    pub fn placeholder(self, placeholder: impl AsRef<str>) -> Self {
        Self {
            placeholder: one_line(placeholder.as_ref()),
            ..self
        }
    }

    /// The same, saying `text` while it has no items.
    #[must_use]
    pub fn empty_text(self, text: impl AsRef<str>) -> Self {
        Self {
            empty: one_line(text.as_ref()),
            ..self
        }
    }

    /// Its name, as kept and cleaned.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Opens it as a modal overlay at the top of the window, with no items
    /// and focus in its field, and returns its root: the node its
    /// [`Select`](ActionKind::Select), [`Activate`](ActionKind::Activate)
    /// and [`Dismiss`](ActionKind::Dismiss) come from, which
    /// [`Tree::close_overlay`] closes.
    ///
    /// # Errors
    ///
    /// - [`UiError::Unnamed`] if its name is blank once cleaned: its field
    ///   takes focus, so it needs a name;
    /// - [`UiError::TooManyNodes`] if the tree has no room for its nodes;
    /// - the other errors of [`Tree::open_overlay`].
    ///
    /// Nothing changes then: each is found before the tree changes.
    pub fn open(self, tree: &mut Tree) -> Result<NodeId, UiError> {
        if !Semantics::new(Role::ComboBox, self.name.as_str()).is_named() {
            return Err(UiError::Unnamed {
                role: Role::ComboBox,
            });
        }
        // The root, field, list, its body and two gaps, and the empty line.
        if tree.node_count().saturating_add(7) > MAX_NODES {
            return Err(UiError::TooManyNodes { max: MAX_NODES });
        }
        let style = Style::column()
            .width(Length::Fixed(PALETTE_WIDTH))
            .padding(Insets::all(PALETTE_PADDING))
            .gap(PALETTE_PADDING)
            .clip();
        let root = Node::new(style)
            .with_semantics(Semantics::new(Role::Dialog, self.name.as_str()))
            .with_widget(PaletteView::default());
        let anchor = Anchor::window(Align::Center, Align::Start, MODAL_MARGIN);
        let palette = tree.open_overlay(root, OverlayKind::Modal, anchor)?;
        let field = PaletteField {
            input: TextInput::search(self.name.clone()).placeholder(self.placeholder),
            name: self.name.clone(),
        };
        let field_node = Node::new(
            Style::default()
                .width(Length::Fill(1.0))
                .height(Length::Fixed(FIELD_HEIGHT)),
        )
        .with_semantics(field.semantics())
        .focusable()
        .with_widget(field);
        let list = Collection::list("Results", Vec::<super::Row>::new()).node();
        // Filling what the field leaves, up to `PALETTE_ROWS` rows: in a
        // window too short for both, the list, which scrolls, gives up
        // room, and the field keeps its height.
        let list_style = Style::column()
            .width(Length::Fill(1.0))
            .height(Length::Fill(1.0))
            .max(Size::new(
                MAX_EXTENT,
                PALETTE_ROWS as f32 * ROW_HEIGHT + 2.0 * FOCUS_WIDTH,
            ))
            .padding(Insets::all(FOCUS_WIDTH))
            .hidden();
        let empty = Node::new(Style::default().width(Length::Fill(1.0)))
            .with_semantics(Semantics::new(Role::Status, self.empty.as_str()))
            .with_widget(EmptyLine { text: self.empty });
        let mut made = Vec::new();
        for node in [
            field_node,
            Node {
                style: list_style,
                focusable: false,
                ..list
            },
            empty,
        ] {
            match tree.insert(palette, node) {
                Ok(id) => made.push(id),
                // Not reached: there is room, and none is too deep or
                // focusable without a name.
                Err(error) => {
                    let _ = tree.close_overlay(palette);
                    return Err(error);
                }
            }
        }
        let [field, list, empty] = made[..] else {
            let _ = tree.close_overlay(palette);
            return Err(UiError::UnknownNode { id: palette });
        };
        if let Some(view) = view_mut(tree, palette) {
            (view.field, view.list, view.empty) = (field, list, empty);
        }
        if let Some(node) = tree.get_mut(field) {
            node.active_in = Some(list);
        }
        if let Some(node) = tree.get_mut(list) {
            node.actions_to = Some(palette);
        }
        if let Some(collection) = tree.widget_mut::<Collection>(list) {
            collection.activate_on_click();
        }
        // Focus goes to the field (ADR-0054), showing as the last key or
        // click left it, and comes back to it while the palette is open.
        let visible = tree.focus_visible;
        tree.change_focus(Some(field), visible);
        tree.set_modal_default(palette, field);
        Ok(palette)
    }

    /// Gives the palette `palette` the items `items`, ranked, in place of
    /// those it had, and selects one, scrolled into view: the item that
    /// was selected, if exactly one of the first [`MAX_VIEW_ROWS`] items
    /// has its cells (its text), or else the first, if any. Text is what
    /// the user sees of an item, so a refresh of the same ranking, or a
    /// new one that still holds the item, keeps what the user chose; two
    /// items alike leave nothing to tell them apart by, so neither is
    /// taken for the other. A press under way activates nothing on its
    /// release (see [Keys and the pointer](Self#keys-and-the-pointer)).
    ///
    /// # Errors
    ///
    /// [`UiError::UnknownNode`] if `palette` is not in the tree, and
    /// [`UiError::WrongWidget`] if it is not a palette.
    pub fn set_items(tree: &mut Tree, palette: NodeId, items: impl Rows) -> Result<(), UiError> {
        if !tree.contains(palette) {
            return Err(UiError::UnknownNode { id: palette });
        }
        let list = tree
            .widget::<PaletteView>(palette)
            .ok_or(UiError::WrongWidget { id: palette })?
            .list;
        let was = Self::selected(tree, palette).and_then(|selected| {
            tree.widget::<Collection>(list)
                .map(|collection| collection.model().row(selected))
        });
        let len = items.len();
        let kept = was.and_then(|was| {
            let mut alike = (0..len.min(MAX_VIEW_ROWS))
                .filter(|&index| items.row(index).cell_texts() == was.cell_texts());
            match (alike.next(), alike.next()) {
                (Some(index), None) => Some(index),
                _ => None,
            }
        });
        let collection = tree
            .widget_mut::<Collection>(list)
            .ok_or(UiError::WrongWidget { id: palette })?;
        collection.set_model(items);
        if let Some(view) = view_mut(tree, palette) {
            view.len = len;
            view.selected = kept.or((len > 0).then_some(0));
            view.applied = None;
        }
        tree.layout_dirty = true;
        Ok(())
    }

    /// The palette's field, if `palette` is one.
    pub fn field(tree: &Tree, palette: NodeId) -> Option<NodeId> {
        tree.widget::<PaletteView>(palette).map(|view| view.field)
    }

    /// The palette's list, if `palette` is one.
    pub fn list(tree: &Tree, palette: NodeId) -> Option<NodeId> {
        tree.widget::<PaletteView>(palette).map(|view| view.list)
    }

    /// The palette's query: what its field holds.
    pub fn query(tree: &Tree, palette: NodeId) -> Option<&str> {
        let field = Self::field(tree, palette)?;
        tree.widget::<PaletteField>(field)
            .map(|field| field.input.text())
    }

    /// The selected item, the one Enter activates: an index into the items
    /// the owner gave last; `None` if it has none, or `palette` is not a
    /// palette.
    pub fn selected(tree: &Tree, palette: NodeId) -> Option<usize> {
        let view = tree.widget::<PaletteView>(palette)?;
        let collection = tree.widget::<Collection>(view.list)?;
        let shown = collection.selected();
        // A press on a row selects it before the next layout tells the
        // palette.
        if shown != view.applied {
            return shown;
        }
        view.selected
            .filter(|&selected| selected < collection.len())
    }
}

/// Shows its texts as their lengths only (ADR-0043).
impl fmt::Debug for Palette {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Palette")
            .field("name", &TextLength(&self.name))
            .field("placeholder", &TextLength(&self.placeholder))
            .field("empty", &TextLength(&self.empty))
            .finish()
    }
}

/// `text` kept as one line: its first, cut to
/// [`MAX_CELL_BYTES`](super::MAX_CELL_BYTES), cleaned.
fn one_line(text: &str) -> String {
    kept_lines(text, 1).concat()
}

/// The palette's root widget, to change, without marking its node.
fn view_mut(tree: &mut Tree, palette: NodeId) -> Option<&mut PaletteView> {
    let widget: &mut dyn Widget = tree.get_mut(palette)?.widget.as_deref_mut()?;
    (widget as &mut dyn Any).downcast_mut()
}

/// The palette's root: its look, and the selection it keeps for its list.
#[derive(Debug)]
pub(super) struct PaletteView {
    field: NodeId,
    list: NodeId,
    empty: NodeId,
    /// How many items it has.
    len: usize,
    /// The selected item, as the keys left it.
    selected: Option<usize>,
    /// The selection its list was last given, or last told it of.
    applied: Option<usize>,
}

impl Default for PaletteView {
    fn default() -> Self {
        Self {
            field: NodeId::NONE,
            list: NodeId::NONE,
            empty: NodeId::NONE,
            len: 0,
            selected: None,
            applied: None,
        }
    }
}

impl PaletteView {
    /// Moves the selection as `key` does: `None` if it is not one of the
    /// keys that do, else whether the selection moved.
    fn step(&mut self, key: Key) -> Option<bool> {
        if !matches!(key, Key::Up | Key::Down | Key::PageUp | Key::PageDown) {
            return None;
        }
        let Some(last) = self.len.checked_sub(1) else {
            return Some(false);
        };
        let at = self.selected.map(|selected| selected.min(last));
        let to = match (key, at) {
            (Key::Down, Some(at)) if at == last => 0,
            (Key::Down, Some(at)) => at + 1,
            (Key::Up, Some(0) | None) => last,
            (Key::Up, Some(at)) => at - 1,
            (Key::PageDown, Some(at)) => at.saturating_add(PALETTE_ROWS).min(last),
            (Key::PageUp, Some(at)) => at.saturating_sub(PALETTE_ROWS),
            // From no selection, Down and the page keys select the first.
            _ => 0,
        };
        let moved = at != Some(to);
        self.selected = Some(to);
        Some(moved)
    }
}

impl Widget for PaletteView {
    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let bounds = cx.bounds();
        let theme = cx.theme();
        list.rect(cx.clip(), theme.color(Token::Panel));
        list.rect(bounds, theme.solid(Token::Panel));
        dimmed(cx, list, |list| {
            list.border(bounds, 1.0, theme.color(Token::Line));
            list.brackets(
                bounds,
                BRACKET_ARM,
                BRACKET_THICKNESS,
                Corners::TOP_LEFT_AND_BOTTOM_RIGHT,
                theme.color(Token::Teal),
            );
        });
    }

    fn event(&mut self, cx: &mut EventContext<'_>, event: &InputEvent) -> Handled {
        let plain = |modifiers: Modifiers| modifiers.difference(Modifiers::SHIFT).is_empty();
        match *event {
            InputEvent::KeyDown {
                key: Key::Escape,
                modifiers,
                repeat,
            } if plain(modifiers) => {
                if !repeat {
                    cx.emit(ActionKind::Dismiss);
                }
                Handled::Yes
            }
            InputEvent::KeyDown {
                key: Key::Enter | Key::NumpadEnter,
                modifiers,
                repeat,
            } if plain(modifiers) => {
                // With items, one is selected: the first, or the one the
                // keys or the pointer chose.
                if !repeat && self.len > 0 {
                    cx.emit(ActionKind::Activate);
                }
                Handled::Yes
            }
            InputEvent::KeyDown { key, modifiers, .. } if modifiers.is_empty() => {
                match self.step(key) {
                    Some(true) => {
                        cx.emit(ActionKind::Select);
                        cx.request_layout();
                        Handled::Yes
                    }
                    Some(false) => Handled::Yes,
                    None => Handled::No,
                }
            }
            // A press on the palette itself, its padding or the window
            // around it, reaches no control: it comes back blocked.
            _ => Handled::No,
        }
    }
}

/// The palette's field: a search field whose role is a combo box, which
/// leaves the keys that move through the list, Enter and Escape to the
/// palette.
struct PaletteField {
    input: TextInput,
    name: String,
}

impl PaletteField {
    /// A combo box named as the palette, its value the query, expanded.
    fn semantics(&self) -> Semantics {
        Semantics::new(Role::ComboBox, self.name.as_str())
            .with_value(self.input.text())
            .expanded(true)
    }
}

/// Shows the query as its length only (ADR-0043).
impl fmt::Debug for PaletteField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PaletteField")
            .field("input", &self.input)
            .field("name", &TextLength(&self.name))
            .finish()
    }
}

impl Widget for PaletteField {
    fn measure(&self, available: Size, metrics: &dyn TextMetrics) -> Size {
        self.input.measure(available, metrics)
    }

    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        self.input.paint(cx, list);
    }

    fn event(&mut self, cx: &mut EventContext<'_>, event: &InputEvent) -> Handled {
        let chord = Modifiers::CTRL | Modifiers::ALT | Modifiers::SUPER;
        if let InputEvent::KeyDown { key, modifiers, .. } = *event
            && !modifiers.intersects(chord)
            && matches!(
                key,
                Key::Up
                    | Key::Down
                    | Key::PageUp
                    | Key::PageDown
                    | Key::Enter
                    | Key::NumpadEnter
                    | Key::Escape
            )
        {
            return Handled::No;
        }
        let handled = self.input.event(cx, event);
        // Last, so that it stands over the search field's own.
        cx.set_semantics(self.semantics());
        handled
    }
}

/// The line a palette shows in place of its list while it has no items.
struct EmptyLine {
    text: String,
}

impl Widget for EmptyLine {
    fn measure(&self, _available: Size, metrics: &dyn TextMetrics) -> Size {
        Size::new(0.0, line_height(metrics, &MODAL_TEXT))
    }

    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let bounds = cx.bounds();
        dimmed(cx, list, |list| {
            list.push_clip(bounds);
            list.text(
                Point::new(
                    bounds.x + PALETTE_PADDING,
                    baseline(bounds, cx.metrics(), &MODAL_TEXT),
                ),
                self.text.clone(),
                MODAL_TEXT,
                cx.theme().color(Token::Text2),
            );
            list.pop_clip();
        });
    }
}

impl Tree {
    /// Keeps a palette's list in step with it: takes a selection a press
    /// made on the list, gives the list the selection the keys made,
    /// scrolled into view, and shows the list or the empty line as it has
    /// items or not. Returns whether that changed layout.
    pub(crate) fn bind_palette(&mut self, id: NodeId) -> bool {
        let Some(view) = self.widget::<PaletteView>(id) else {
            return false;
        };
        let (list, empty) = (view.list, view.empty);
        let Some(collection) = self.widget::<Collection>(list) else {
            // Its list was removed: it has no items to select or activate.
            if let Some(view) = view_mut(self, id) {
                (view.len, view.selected, view.applied) = (0, None, None);
            }
            return false;
        };
        let (shown, len) = (collection.selected(), collection.len());
        let mut give = None;
        if let Some(view) = view_mut(self, id) {
            view.len = len;
            if shown != view.applied {
                view.selected = shown;
                view.applied = shown;
            }
            view.selected = view.selected.filter(|&selected| selected < len);
            if view.selected != view.applied {
                view.applied = view.selected;
                give = Some(view.selected);
            }
        }
        if let Some(selected) = give
            && let Some(collection) = self.widget_mut::<Collection>(list)
        {
            collection.select(selected);
        }
        let mut changed = false;
        for (node, hidden) in [(list, len == 0), (empty, len > 0)] {
            if let Some(style) = self.style(node).copied()
                && style.hidden != hidden
            {
                let _ = self.set_style(node, Style { hidden, ..style });
                changed = true;
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_move_the_selection_wrapping_by_one_and_stopping_by_a_page() {
        let mut view = PaletteView {
            len: 25,
            ..PaletteView::default()
        };
        assert_eq!(view.step(Key::Down), Some(true));
        assert_eq!(view.selected, Some(0));
        assert_eq!(view.step(Key::PageUp), Some(false), "at the start");
        assert_eq!(view.step(Key::Up), Some(true));
        assert_eq!(view.selected, Some(24), "Up from the first wraps");
        assert_eq!(view.step(Key::PageDown), Some(false), "at the end");
        assert_eq!(view.step(Key::Down), Some(true));
        assert_eq!(view.selected, Some(0), "Down from the last wraps");
        assert_eq!(view.step(Key::PageDown), Some(true));
        assert_eq!(view.selected, Some(10));
        view.step(Key::PageDown);
        view.step(Key::PageDown);
        assert_eq!(view.selected, Some(24), "a page stops at the end");
        view.step(Key::PageUp);
        assert_eq!(view.selected, Some(14));
        view.step(Key::PageUp);
        view.step(Key::PageUp);
        assert_eq!(view.selected, Some(0), "and at the start");
        assert_eq!(view.step(Key::Home), None, "Home is the field's");
        let mut empty = PaletteView::default();
        assert_eq!(empty.step(Key::Down), Some(false), "taken, moving nothing");
        assert_eq!(empty.selected, None);
        let mut one = PaletteView {
            len: 1,
            selected: Some(0),
            ..PaletteView::default()
        };
        for key in [Key::Up, Key::Down, Key::PageUp, Key::PageDown] {
            assert_eq!(one.step(key), Some(false), "{key:?} with one item");
        }
    }

    #[test]
    fn its_texts_are_one_line_cut_and_cleaned() {
        let long = "x".repeat(2 * super::super::MAX_CELL_BYTES);
        let palette = Palette::new("Go \u{202E}to\u{0}\nsecond")
            .placeholder(&long)
            .empty_text("none\tfound");
        assert_eq!(palette.name(), "Go to");
        assert_eq!(palette.placeholder.len(), super::super::MAX_CELL_BYTES);
        assert_eq!(palette.empty, "none found");
        let shown = format!("{palette:?}");
        assert!(shown.contains("<5 bytes>"), "{shown}");
    }
}
