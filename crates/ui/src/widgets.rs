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

//! The controls of the spec's Components table, in jabbott.dev's design
//! language: [`Button`]s (ghost, teal, amber and slate, each a tile of the
//! site's header, ADR-0058), [`Chip`]s, the [`TextInput`] and search
//! field, the [`ScrollBar`], the [focus outline](focus_outline), the
//! [`StatusDot`], the [`SectionTitle`], [`Eyebrow`] and [`Kicker`], and
//! [`Breadcrumbs`]; and
//! the collections, a [`Collection`]: the list, the tree view and the
//! table, which keep nodes only for the rows on screen over a model of
//! [`Rows`] their owner provides; and the [`TabStrip`], the [`Splitter`],
//! the [`Card`] and the [`CodeBlock`] (ADR-0052); and the overlays
//! ([`overlay`](crate::overlay), ADR-0052): the [`Modal`] dialog, the
//! [`Tooltip`], the [`Toast`] and the command [`Palette`].
//!
//! New in Tidepool; not ported. The looks follow jabbott.dev's stylesheet
//! (`assets/css/style.css`) at the IDE's denser scale, and ADR-0049 and
//! ADR-0052 where the site has none; the buttons follow its header tiles
//! (`.nav-tile`, ADR-0058).
//!
//! # Controls
//!
//! Each control is a plain struct that implements [`Widget`] and
//! [`Control`]: it says its [`Semantics`], its [`Style`] and whether it
//! takes focus, and [`Control::node`] makes the [`Node`] that holds it, so
//! that every control has its role and the name it is given in the
//! accessibility tree, focusable or not. The tree refuses a focusable node
//! without a name; one that takes no focus, a title or a status dot, has
//! the name its owner gives it, blank or not. Change a control in the tree
//! with [`update`], which sets its semantics again; a control's own
//! handler does so through
//! [`EventContext::set_semantics`](crate::input::EventContext::set_semantics).
//! [`Breadcrumbs`] builds a row of [`Crumb`]s, and [`focus_outline`] is what
//! the focusable controls paint when focus shows; neither is a control.
//!
//! ```
//! use tidepool_core::geom::{Point, Size};
//! use tidepool_core::input::{InputEvent, Modifiers, MouseButton};
//! use tidepool_ui::input::ActionKind;
//! use tidepool_ui::metrics::FixedMetrics;
//! use tidepool_ui::widgets::{Button, ButtonKind, Control};
//! use tidepool_ui::Tree;
//!
//! let mut tree = Tree::new();
//! let build = tree.insert(tree.root(), Button::new("Build", ButtonKind::Amber).node())?;
//! tree.layout(Size::new(400.0, 300.0), &FixedMetrics::DEFAULT);
//!
//! // A click: the press and its release on the button.
//! let at = Point::new(10.0, 10.0);
//! let press = InputEvent::MouseDown { button: MouseButton::Left, position: at, modifiers: Modifiers::NONE };
//! let release = InputEvent::MouseUp { button: MouseButton::Left, position: at, modifiers: Modifiers::NONE };
//! tree.handle(&press);
//! let response = tree.handle(&release);
//! assert_eq!(response.actions[0].node, build);
//! assert_eq!(response.actions[0].kind, ActionKind::Activate);
//! assert_eq!(tree.accessibility().to_string(), "button \"Build\" focusable focused\n");
//! # Ok::<(), tidepool_ui::UiError>(())
//! ```
//!
//! # Colors and sizes
//!
//! Every color is a token of the tree's theme, which
//! [`PaintContext::theme`] gives a control as it paints (ADR-0049); no
//! control holds a theme or a literal color. Sizes are the constants of
//! this module, from the spec's Typography and Components tables; a
//! button's are the site's header tile's, scaled from its 40 px to the
//! button's height (ADR-0058).
//! Labels are painted uppercase, as chrome is; what a user wrote (a
//! field's text, a breadcrumb's path segment) is painted as written, and
//! every accessible name is as written.
//!
//! # States
//!
//! A control paints from its node's [`NodeState`]: *hover* is the site's
//! `:hover`; *pressed* (the spec's "active") is the hover look with the
//! fill one step stronger, or a rim where the palette has no stronger
//! fill, and a button's is its hover look without the rise; *focus* shows
//! the [focus outline](focus_outline) when focus is visible, and a field
//! its ring whenever it has focus, and a button is also lit as when
//! hovered, without rising; *disabled* is the rest look in a layer at
//! [`DISABLED_OPACITY`], with no hover or pressed look. No control blinks
//! or animates: an idle tree damages nothing.
//!
//! [`NodeState`]: crate::tree::NodeState

use tidepool_core::color::Color;
use tidepool_core::geom::Rect;

use crate::a11y::Semantics;
use crate::draw::{DrawList, PaintContext};
use crate::error::UiError;
use crate::layout::Style;
use crate::metrics::{Family, TextMetrics, TextStyle, Weight};
use crate::theme::{Theme, Token};
use crate::tree::{Node, NodeId, NodeState, Tree, Widget};

mod breadcrumbs;
mod button;
mod card;
mod chip;
mod code;
mod collection;
mod field;
mod labels;
mod modal;
mod palette;
mod scrollbar;
mod splitter;
mod tabs;
mod toast;
mod tooltip;

pub use breadcrumbs::{Breadcrumbs, Crumb};
pub use button::{Button, ButtonKind, ButtonSize};
pub use card::Card;
pub use chip::Chip;
pub use code::{CodeBlock, CodeSpan};
pub use collection::{Collection, CollectionKind, Column, Row, Rows};
pub use field::{FieldKind, MAX_INPUT_BYTES, TextInput};
pub use labels::{Eyebrow, Kicker, SectionTitle, Status, StatusDot};
pub use modal::Modal;
pub use palette::Palette;
pub use scrollbar::ScrollBar;
pub use splitter::Splitter;
pub use tabs::{Tab, TabKind, TabStrip};
pub use toast::{Toast, ToastKind};
pub use tooltip::{Tooltip, TooltipCause, TooltipTarget};

pub(crate) use toast::is_toast;
pub(crate) use tooltip::is_tooltip;

/// The opacity of a disabled control's layer.
pub const DISABLED_OPACITY: f32 = 0.4;

/// The width of the focus outline (`:focus-visible`).
pub const FOCUS_WIDTH: f32 = 2.0;

/// How far outside a control's bounds the focus outline starts.
pub const FOCUS_OFFSET: f32 = 3.0;

/// The height of a regular button: a header tile (`.nav-tile`, 40 px) at
/// the IDE's scale, three quarters of the site's (ADR-0058).
pub const BUTTON_HEIGHT: f32 = 30.0;

/// The height of a small button: the tile at 0.6 of the site's.
pub const BUTTON_HEIGHT_SMALL: f32 = 24.0;

/// The space either side of a regular button's label: the tile's 16 px
/// right of its label at 0.75. A tile with no icon pads both sides
/// evenly, as no button has one yet (ADR-0058).
pub const BUTTON_PADDING: f32 = 12.0;

/// The space either side of a small button's label: the tile's 16 px at
/// 0.6, 9.6, rounded to whole units.
pub const BUTTON_PADDING_SMALL: f32 = 10.0;

/// How far each arm of a regular button's bracket corners reaches when it
/// is lit (hovered, pressed or showing focus): the tile's 16 px `--arm`
/// at 0.75. At rest it reaches [`BUTTON_REACH_AT_REST`] of that.
pub const BUTTON_ARM: f32 = 12.0;

/// How far each arm of a small button's bracket corners reaches when it is
/// lit: the tile's 16 px at 0.6, 9.6, rounded to whole units.
pub const BUTTON_ARM_SMALL: f32 = 10.0;

/// How much of its arm a button's bracket corner draws at rest: the
/// tile's `--reach` of 0.5.
pub const BUTTON_REACH_AT_REST: f32 = 0.5;

/// How far a regular button rises while it is hovered: the tile's 2 px
/// `translateY(-2px)` at 0.75, 1.5, rounded half up to whole units, so
/// that its edges stay on device pixels at every whole scale. Unlike the
/// site's tile, it does not rise for keyboard focus (ADR-0058).
pub const BUTTON_LIFT: f32 = 2.0;

/// How far a small button rises: the tile's 2 px at 0.6, 1.2, rounded to
/// whole units.
pub const BUTTON_LIFT_SMALL: f32 = 1.0;

/// The width of the rim a pressed control shows where the palette has no
/// stronger fill: a selected chip (ADR-0049).
pub const PRESSED_RIM: f32 = 2.0;

/// The height of a chip.
pub const CHIP_HEIGHT: f32 = 24.0;

/// The space either side of a chip's label.
pub const CHIP_PADDING: f32 = 10.0;

/// The height of a field: a text input or the search field.
pub const FIELD_HEIGHT: f32 = 30.0;

/// The width a field measures, which its owner usually replaces.
pub const FIELD_WIDTH: f32 = 200.0;

/// The space either side of a text input's text.
pub const FIELD_PADDING: f32 = 10.0;

/// The space left of the search field's text.
pub const SEARCH_PADDING_LEFT: f32 = 14.0;

/// The space right of the search field's text, which holds its icon.
pub const SEARCH_PADDING_RIGHT: f32 = 30.0;

/// The width of a focused field's `teal-ring`, outside its bounds.
pub const FIELD_RING: f32 = 3.0;

/// The width of a field's caret.
pub const CARET_WIDTH: f32 = 1.0;

/// The thickness of a scrollbar.
pub const SCROLLBAR_THICKNESS: f32 = 10.0;

/// The thickness of a scrollbar's thumb, centred in it.
pub const THUMB_THICKNESS: f32 = 6.0;

/// The shortest a scrollbar's thumb gets.
pub const MIN_THUMB: f32 = 24.0;

/// The diameter of a status dot.
pub const DOT: f32 = 8.0;

/// The width of a status dot's halo, around it.
pub const HALO: f32 = 3.0;

/// The width of a section title's `teal` rule.
pub const SECTION_RULE: f32 = 3.0;

/// The space between a section title's rule and its text.
pub const SECTION_GAP: f32 = 10.0;

/// The space either side of a breadcrumb's separator.
pub const CRUMB_GAP: f32 = 8.0;

/// The height of a row of a list, tree view or table, and of a table's
/// header: the UI body's line, 13 × 1.5, rounded to whole units as
/// [`SCROLL_LINE`](crate::input::SCROLL_LINE) is, so that a wheel's line
/// scrolls a row. Every row of a collection is this high, so the rows in
/// view are found by arithmetic (ADR-0052).
pub const ROW_HEIGHT: f32 = 20.0;

/// The space left of a list's or tree view's text.
pub const ROW_PADDING: f32 = 8.0;

/// How much further right each level of a tree view starts.
pub const TREE_INDENT: f32 = 16.0;

/// The width of the box a tree item's chevron is centred in, before its
/// text; a row with no chevron leaves it empty, so siblings' text lines up.
pub const CHEVRON_WIDTH: f32 = 16.0;

/// The space left of a table cell's text and a column header's label
/// (`.prose th, .prose td`'s 14 px, at the IDE's scale).
pub const CELL_PADDING: f32 = 10.0;

/// How many rows a collection keeps a node for beyond each end of the part
/// of its view on screen, within its view (ADR-0052).
pub const ROW_MARGIN: usize = 2;

/// The most rows a collection's view holds: its rows show in at most this
/// many rows' height, 10,240 units, taller than any window, and it
/// measures no taller (ADR-0052). So it keeps at most this many and two
/// row nodes, however it is laid out.
pub const MAX_VIEW_ROWS: usize = 512;

/// The most bytes of a cell's text, or a list's or tree view's row's, that
/// a collection shows and names: the rest is cut, at an extended grapheme
/// cluster boundary.
pub const MAX_CELL_BYTES: usize = 4096;

/// The height of a tab strip's tabs (`.btn`'s 30, as no site style has
/// tabs).
pub const TAB_HEIGHT: f32 = 30.0;

/// The space either side of a tab's label.
pub const TAB_PADDING: f32 = 12.0;

/// The side of a closable tab's close control.
pub const TAB_CLOSE: f32 = 16.0;

/// The space between a tab's label and its close control.
pub const TAB_CLOSE_GAP: f32 = 6.0;

/// The thickness of the `teal` rule under the selected tab.
pub const TAB_RULE: f32 = 2.0;

/// The most tabs a tab strip holds; more are refused.
pub const MAX_TABS: usize = 256;

/// The most bytes of a tab's label a tab strip shows and names: the rest
/// is cut, at an extended grapheme cluster boundary.
pub const MAX_TAB_LABEL_BYTES: usize = 256;

/// The thickness of a splitter's divider, which the pointer drags.
pub const SPLITTER_THICKNESS: f32 = 8.0;

/// How far an arrow key moves a splitter's divider, in units.
pub const SPLITTER_STEP: f32 = 10.0;

/// The space inside a card's edges (`.card__body`'s 15, at the IDE's
/// scale).
pub const CARD_PADDING: f32 = 12.0;

/// The space between a card's lines of text, and between them and what
/// the card holds below them.
pub const CARD_GAP: f32 = 4.0;

/// How far below a card's top its tab sits (`.card__tab`'s 10).
pub const CARD_TAB_TOP: f32 = 8.0;

/// The height of a card's tab: its label's line and 4 above and below.
pub const CARD_TAB_HEIGHT: f32 = 18.0;

/// The space left of a card tab's label (`.card__tab`'s 11).
pub const CARD_TAB_PADDING_LEFT: f32 = 9.0;

/// The space right of a card tab's label, which holds its `teal` stripe
/// (`.card__tab`'s 10).
pub const CARD_TAB_PADDING_RIGHT: f32 = 8.0;

/// The width of a card tab's `teal` stripe on its right edge
/// (`.card__tab`'s inset 4 px shadow).
pub const CARD_TAB_STRIPE: f32 = 4.0;

/// The height of a code block's bar, which holds its language and COPY
/// (`.code-bar`).
pub const CODE_BAR_HEIGHT: f32 = 28.0;

/// The space around a code block's code (`.prose pre`'s 16 and 18, at the
/// IDE's scale).
pub const CODE_PADDING: f32 = 10.0;

/// The space left of a code block's code and of its language.
pub const CODE_PADDING_LEFT: f32 = 12.0;

/// The width of a code block's `teal` left border.
pub const CODE_RULE: f32 = 2.0;

/// The height of a code block's COPY button.
pub const COPY_HEIGHT: f32 = 18.0;

/// The space either side of COPY's label (`.code-copy`'s 10).
pub const COPY_PADDING: f32 = 10.0;

/// The most lines a code block keeps and paints; more are cut.
pub const MAX_CODE_LINES: usize = 200;

/// The most bytes of a line a code block keeps and paints; the rest is
/// cut, at an extended grapheme cluster boundary.
pub const MAX_CODE_LINE_BYTES: usize = 400;

/// The most spans of a line a code block keeps; more are cut.
pub const MAX_CODE_SPANS: usize = 32;

/// The most bytes of a code block's language it shows and names, as the
/// theme's [`MAX_TEXT_BYTES`](crate::theme::MAX_TEXT_BYTES): the rest is
/// cut, at an extended grapheme cluster boundary.
pub const MAX_LANGUAGE_BYTES: usize = 64;

/// The most columns a table has; more are left out.
pub const MAX_COLUMNS: usize = 64;

/// How far each arm of a panel's bracket corner reaches along its edge
/// (`.panel--brackets`' 16 px): the focused panel's, the command
/// palette's and a modal dialog's. A button's are [`BUTTON_ARM`] and
/// [`BUTTON_ARM_SMALL`].
pub const BRACKET_ARM: f32 = 16.0;

/// How thick a bracket corner's arms are (`.panel--brackets`' 1 px
/// border, and a header tile's 1 px lines), drawn over the panel's or the
/// button's border.
pub const BRACKET_THICKNESS: f32 = 1.0;

/// The width of a modal dialog (the site's popup panel, `.deploy__panel`,
/// 390 px, at the IDE's scale); a window too narrow for it and
/// [`MODAL_MARGIN`] either side makes it narrower.
pub const MODAL_WIDTH: f32 = 320.0;

/// The space inside a modal dialog's edges (`.deploy__panel`'s 18 px, at
/// the IDE's scale).
pub const MODAL_PADDING: f32 = 14.0;

/// The space between a modal dialog's title and its text, below its text,
/// and between what it holds.
pub const MODAL_GAP: f32 = 10.0;

/// The least space between a modal dialog and the window's edges
/// (`.deploy__panel`'s `100vw - 32px`, 16 px a side, at the IDE's scale).
pub const MODAL_MARGIN: f32 = 12.0;

/// The space between a modal dialog's buttons.
pub const MODAL_BUTTON_GAP: f32 = 8.0;

/// The most lines of text a modal dialog keeps; more are left out.
pub const MAX_MODAL_LINES: usize = 4;

/// The space between a tooltip and the node it is anchored to.
pub const TOOLTIP_GAP: f32 = 4.0;

/// The space left and right of a tooltip's text.
pub const TOOLTIP_PADDING_X: f32 = 8.0;

/// The space above and below a tooltip's text.
pub const TOOLTIP_PADDING_Y: f32 = 4.0;

/// The widest a tooltip gets; its lines are cut to it.
pub const TOOLTIP_MAX_WIDTH: f32 = 320.0;

/// The most lines of text a tooltip keeps; more are left out.
pub const MAX_TOOLTIP_LINES: usize = 4;

/// The width of a toast: the modal dialog's, [`MODAL_WIDTH`].
pub const TOAST_WIDTH: f32 = MODAL_WIDTH;

/// The space inside a toast's edges.
pub const TOAST_PADDING: f32 = 10.0;

/// The width of the stripe down a toast's left edge in its kind's color.
pub const TOAST_STRIPE: f32 = 3.0;

/// The space between toasts in their column, and between a toast's text
/// and its action.
pub const TOAST_GAP: f32 = 8.0;

/// The least space between the toasts and the window's edges, as a modal
/// dialog's [`MODAL_MARGIN`].
pub const TOAST_MARGIN: f32 = MODAL_MARGIN;

/// The side of a toast's close control.
pub const TOAST_CLOSE: f32 = 16.0;

/// The most toasts shown at once; showing one more closes the oldest the
/// user does not hold ([`Toast`]).
pub const MAX_TOASTS: usize = 4;

/// The most lines of text a toast keeps; more are left out.
pub const MAX_TOAST_LINES: usize = 3;

/// The width of the command palette: one and a half times the modal
/// dialog's [`MODAL_WIDTH`] (see [`Palette`]).
pub const PALETTE_WIDTH: f32 = 1.5 * MODAL_WIDTH;

/// The space inside the command palette's edges, and between its field
/// and its list.
pub const PALETTE_PADDING: f32 = 10.0;

/// The most rows the command palette's list shows at once; more scroll.
/// Page Up and Page Down move its selection this many rows.
pub const PALETTE_ROWS: usize = 10;

/// The most ranges of a row's text a collection paints as matched, the
/// first; more are left out.
pub const MAX_MATCHES: usize = 64;

/// The label of a button: Saira 12 px, 600, tracked 0.10em, uppercase.
/// The site sets a header tile's label, as its `.btn`'s, in 13 px at 40
/// high; the IDE's denser type scale sets both in 12 px (ADR-0049,
/// ADR-0058).
pub const BUTTON_LABEL: TextStyle = TextStyle::new(Family::Ui, 12.0)
    .weight(Weight::SemiBold)
    .letter_spacing(0.10);

/// The label of a chip: Saira 12 px, 600, tracked 0.08em, uppercase.
pub const CHIP_LABEL: TextStyle = TextStyle::new(Family::Ui, 12.0)
    .weight(Weight::SemiBold)
    .letter_spacing(0.08);

/// A field's text and placeholder: Saira 13 px, 500, as written.
pub const FIELD_TEXT: TextStyle = TextStyle::new(Family::Ui, 13.0).weight(Weight::Medium);

/// A section title: Saira 13 px, 500, tracked 0.04em, uppercase.
pub const SECTION_TITLE: TextStyle = TextStyle::new(Family::Ui, 13.0)
    .weight(Weight::Medium)
    .letter_spacing(0.04);

/// An eyebrow: Saira 11 px, 600, tracked 0.28em, uppercase, `text-3`.
pub const EYEBROW: TextStyle = TextStyle::new(Family::Ui, 11.0)
    .weight(Weight::SemiBold)
    .letter_spacing(0.28);

/// A kicker: Saira 11 px, 600, tracked 0.16em, uppercase, `teal`.
pub const KICKER: TextStyle = TextStyle::new(Family::Ui, 11.0)
    .weight(Weight::SemiBold)
    .letter_spacing(0.16);

/// A path, as in a breadcrumb: JetBrains Mono 12 px, as written.
pub const PATH: TextStyle = TextStyle::new(Family::Mono, 12.0);

/// The UI body, a row's text in a list, tree view or table: Saira 13 px,
/// line height 1.5, 400, as written.
pub const ROW_TEXT: TextStyle = TextStyle::new(Family::Ui, 13.0);

/// The characters of a row's text a search matched, as the command
/// palette shows them: [`ROW_TEXT`] in Saira Bold, 700, `teal-hi`.
pub const MATCH_TEXT: TextStyle = ROW_TEXT.weight(Weight::Bold);

/// A table's column header: Saira 11 px, 600, tracked 0.10em, uppercase,
/// `teal`.
pub const TABLE_HEADER: TextStyle = TextStyle::new(Family::Ui, 11.0)
    .weight(Weight::SemiBold)
    .letter_spacing(0.10);

/// A chrome tab's label (PROBLEMS, TERMINAL): the spec's "Button / chip /
/// tab label", Saira 12 px, 600, tracked 0.10em, uppercase. A content
/// tab's label (a file name) is [`ROW_TEXT`], as written.
pub const TAB_LABEL: TextStyle = BUTTON_LABEL;

/// A card's kicker (`.card__meta`): Saira 11 px, 500, tracked 0.06em,
/// uppercase, `text-3`.
pub const CARD_META: TextStyle = TextStyle::new(Family::Ui, 11.0)
    .weight(Weight::Medium)
    .letter_spacing(0.06);

/// A card's title (`.card__title`): Saira 16 px, 500, line height 1.3, as
/// written.
pub const CARD_TITLE: TextStyle = TextStyle::new(Family::Ui, 16.0)
    .weight(Weight::Medium)
    .line_height(1.3);

/// A card's line of text (`.card__excerpt`): the UI body, `text-2`, as
/// written.
pub const CARD_TEXT: TextStyle = ROW_TEXT;

/// A card tab's label (`.card__tab`): Saira 10 px, 700, tracked 0.10em,
/// line height 1, uppercase.
pub const CARD_TAB_LABEL: TextStyle = TextStyle::new(Family::Ui, 10.0)
    .weight(Weight::Bold)
    .letter_spacing(0.10)
    .line_height(1.0);

/// A code block's code (`.prose pre`): JetBrains Mono 13 px, line height
/// 1.5, as written.
pub const CODE_TEXT: TextStyle = TextStyle::new(Family::Mono, 13.0);

/// A code block's language (`.code-bar`): JetBrains Mono 10 px, tracked
/// 0.12em, uppercase, `text-3`.
pub const CODE_BAR_TEXT: TextStyle = TextStyle::new(Family::Mono, 10.0).letter_spacing(0.12);

/// A modal dialog's text: the UI body, Saira 13 px, line height 1.5, 400,
/// `text-2`, as written.
pub const MODAL_TEXT: TextStyle = ROW_TEXT;

/// A tooltip's text: Saira 12 px, line height 1.5, 400, `text`, as
/// written: the size of the status bar's text, a step below the UI body.
pub const TOOLTIP_TEXT: TextStyle = TextStyle::new(Family::Ui, 12.0);

/// A toast's text: the UI body, `text`, as written.
pub const TOAST_TEXT: TextStyle = ROW_TEXT;

/// COPY's label (`.code-copy`): Saira 11 px, 600, tracked 0.10em, line
/// height 1, uppercase.
pub const COPY_LABEL: TextStyle = TextStyle::new(Family::Ui, 11.0)
    .weight(Weight::SemiBold)
    .letter_spacing(0.10)
    .line_height(1.0);

/// A control: a [`Widget`] that says what its node is, so that it gives
/// itself its role and name whether it takes focus or not.
pub trait Control: Widget + Sized {
    /// What assistive technology is told about it: its role, and the name
    /// it was given, which the tree refuses blank for a focusable node
    /// ([`UiError::Unnamed`]).
    fn semantics(&self) -> Semantics;

    /// How its node is laid out by default; the owner may change it.
    /// [`Style::default`] by default: its natural size, which it measures.
    fn style(&self) -> Style {
        Style::default()
    }

    /// Whether it takes keyboard focus.
    fn focusable(&self) -> bool;

    /// A node holding it, with its [`style`](Self::style),
    /// [`semantics`](Self::semantics) and focusability.
    fn node(self) -> Node {
        let mut node = Node::new(self.style()).with_semantics(self.semantics());
        node.focusable = self.focusable();
        node.with_widget(self)
    }
}

/// Changes the control of type `W` at `id` with `change`, then gives its
/// node the control's semantics and focusability again; returns what
/// `change` returned. The node is laid out and repainted at the next frame,
/// as [`Tree::widget_mut`] does.
///
/// # Errors
///
/// [`UiError::UnknownNode`] if the node is not in the tree, and
/// [`UiError::WrongWidget`] if it holds no `W`; nothing changes then.
/// [`UiError::Unnamed`] if the changed control is focusable and says no
/// role or no name: the change is made, and the node keeps its semantics
/// and focusability.
pub fn update<W: Control, R>(
    tree: &mut Tree,
    id: NodeId,
    change: impl FnOnce(&mut W) -> R,
) -> Result<R, UiError> {
    if !tree.contains(id) {
        return Err(UiError::UnknownNode { id });
    }
    let widget = tree
        .widget_mut::<W>(id)
        .ok_or(UiError::WrongWidget { id })?;
    let result = change(widget);
    let semantics = widget.semantics();
    let focusable = widget.focusable();
    if focusable {
        if !semantics.is_named() {
            return Err(UiError::Unnamed {
                role: semantics.role,
            });
        }
        tree.set_semantics(id, semantics)?;
        tree.set_focusable(id, true)?;
    } else {
        tree.set_focusable(id, false)?;
        tree.set_semantics(id, semantics)?;
    }
    Ok(result)
}

/// Paints the focus outline: a `teal` border [`FOCUS_WIDTH`] wide,
/// [`FOCUS_OFFSET`] outside `bounds` (`:focus-visible`). Controls paint it
/// when their node's focus is visible; fields show their ring instead, and
/// a [`Button`] paints the same outline in its accent, around its tile,
/// which rises only while hovered (ADR-0058).
pub fn focus_outline(list: &mut DrawList, bounds: Rect, theme: &Theme) {
    outline(list, bounds, theme.color(Token::Teal));
}

/// Paints the focus outline's border, [`FOCUS_WIDTH`] wide and
/// [`FOCUS_OFFSET`] outside `bounds`, in `color`.
pub(crate) fn outline(list: &mut DrawList, bounds: Rect, color: Color) {
    list.border(grow(bounds, FOCUS_OFFSET + FOCUS_WIDTH), FOCUS_WIDTH, color);
}

/// Whether `widget` keeps other nodes in step with it, so that the tree
/// keeps its node among those each layout binds: a [`Collection`], a
/// [`TabStrip`] or a [`Splitter`] (ADR-0052), a [`Modal`]'s dialog,
/// which disables its controls when the window squeezes them, a
/// [`Palette`], which keeps its list's selection in step with its keys,
/// or a [`Toast`], which stands in the column of toasts.
pub(crate) fn is_bound(widget: &dyn Widget) -> bool {
    let any = widget as &dyn std::any::Any;
    any.is::<Collection>()
        || any.is::<TabStrip>()
        || any.is::<Splitter>()
        || any.is::<modal::Dialog>()
        || any.is::<palette::PaletteView>()
        || any.is::<toast::ToastView>()
}

impl Tree {
    /// Binds every node of [`is_bound`] widgets, from the last layout's
    /// placements; returns whether that added, removed or moved nodes, so
    /// that layout must pass again. `after_pass` says that a layout pass
    /// has just placed the nodes, and `reveal` that collections scroll a
    /// selection asked for into view now; until then they keep their rows.
    /// A dialog binds only after a pass, measuring with `metrics`.
    pub(crate) fn bind_widgets(
        &mut self,
        after_pass: bool,
        reveal: bool,
        metrics: &dyn TextMetrics,
    ) -> bool {
        if self.bound.is_empty() {
            return false;
        }
        let mut ids = std::mem::take(&mut self.bound);
        let mut again = false;
        let mut toasts = false;
        for &id in &ids {
            if self.widget::<Collection>(id).is_some() {
                again |= self.bind_collection(id, after_pass, reveal);
            } else if self.widget::<TabStrip>(id).is_some() {
                again |= self.bind_tabs(id, after_pass);
            } else if self.widget::<Splitter>(id).is_some() {
                again |= self.bind_splitter(id);
            } else if after_pass && self.widget::<modal::Dialog>(id).is_some() {
                again |= self.bind_dialog(id, metrics);
            } else if self.widget::<palette::PaletteView>(id).is_some() {
                again |= self.bind_palette(id);
            } else if self.widget::<toast::ToastView>(id).is_some() {
                toasts = true;
            }
        }
        // The toasts stand in one column, so they bind together.
        if toasts {
            again |= self.bind_toasts(after_pass, metrics);
        }
        ids.retain(|&id| self.contains(id));
        self.bound_gone = 0;
        // Binding inserts no bound widget, but keep any all the same.
        ids.append(&mut self.bound);
        self.bound = ids;
        again
    }
}

/// `text` cut to at most `max` bytes, at an extended grapheme cluster
/// boundary of the whole text, found from its start in what is kept and
/// the character after it, so in time linear in what is kept, however
/// long the text or the cluster the cut falls in.
pub(crate) fn cut_to(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    // A boundary depends only on the text before it and the character
    // after it (UAX #29), so those up to `max` are the whole text's in a
    // head that holds a character past `max`.
    let head = text
        .get(..text.ceil_char_boundary(max.saturating_add(1)))
        .unwrap_or(text);
    let mut end = 0;
    for cluster in tidepool_text::pos::grapheme::graphemes(head) {
        if end + cluster.len() > max {
            break;
        }
        end += cluster.len();
    }
    // `end` is a sum of whole clusters' lengths, so a char boundary.
    text.get(..end).unwrap_or("")
}

/// `rect` grown by `by` on every side.
pub(crate) fn grow(rect: Rect, by: f32) -> Rect {
    Rect::new(
        rect.x - by,
        rect.y - by,
        rect.width + 2.0 * by,
        rect.height + 2.0 * by,
    )
}

/// `rect` shrunk by `by` on every side, never below an empty rect at its
/// centre.
pub(crate) fn inset(rect: Rect, by: f32) -> Rect {
    let by_x = by.min(rect.width / 2.0);
    let by_y = by.min(rect.height / 2.0);
    Rect::new(
        rect.x + by_x,
        rect.y + by_y,
        rect.width - 2.0 * by_x,
        rect.height - 2.0 * by_y,
    )
}

/// The look of an interactive control in `state`: disabled wins, and a
/// disabled control shows neither hover nor pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Look {
    Rest,
    Hover,
    Pressed,
}

impl Look {
    pub(crate) const fn of(state: NodeState) -> Self {
        if state.disabled {
            Self::Rest
        } else if state.pressed {
            Self::Pressed
        } else if state.hovered {
            Self::Hover
        } else {
            Self::Rest
        }
    }
}

/// Paints what `paint` paints in a layer at [`DISABLED_OPACITY`] if the
/// node is disabled, and as it is otherwise.
pub(crate) fn dimmed(
    cx: &PaintContext<'_>,
    list: &mut DrawList,
    paint: impl FnOnce(&mut DrawList),
) {
    let disabled = cx.state().disabled;
    if disabled {
        list.push_layer(DISABLED_OPACITY);
    }
    paint(list);
    if disabled {
        list.pop_layer();
    }
}

/// The baseline that centres a line of `style` vertically in `rect`.
pub(crate) fn baseline(rect: Rect, metrics: &dyn TextMetrics, style: &TextStyle) -> f32 {
    let line = metrics.line(style);
    rect.y + (rect.height - (line.ascent + line.descent)) / 2.0 + line.ascent
}

/// The height of a line of `style`.
pub(crate) fn line_height(metrics: &dyn TextMetrics, style: &TextStyle) -> f32 {
    metrics.line(style).height
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use tidepool_core::testing::check_sized;

    use super::*;

    /// The cut as it was first written: from the start of the whole text.
    fn cut_from_whole(text: &str, max: usize) -> &str {
        if text.len() <= max {
            return text;
        }
        let mut end = 0;
        for cluster in tidepool_text::pos::grapheme::graphemes(text) {
            if end + cluster.len() > max {
                break;
            }
            end += cluster.len();
        }
        text.get(..end).unwrap_or("")
    }

    /// Pieces whose clusters reach across each other: combining marks,
    /// regional indicators, ZWJ emoji and their modifiers, an Indic
    /// conjunct, line ends and wide characters.
    const PIECES: [&str; 16] = [
        "a",
        "e\u{301}",
        "\u{301}",
        "\u{300}\u{302}",
        "\u{1F1FA}",
        "\u{1F1F8}",
        "\u{1F468}",
        "\u{200D}",
        "\u{1F469}\u{200D}\u{1F467}",
        "\u{1F3FD}",
        "\u{FE0F}",
        "\u{915}\u{94D}",
        "\u{937}",
        "\r\n",
        "\r",
        "日本",
    ];

    #[test]
    fn cutting_within_the_head_agrees_with_cutting_the_whole_text() {
        check_sized(
            "cut_to on clusters",
            500,
            |rng, size| {
                let text: String = (0..size.max(1))
                    .map(|_| *rng.choose(&PIECES).expect("pieces"))
                    .collect();
                let max = rng.range(0..text.len() as u32 + 2) as usize;
                (text, max)
            },
            |(text, max)| {
                let (new, old) = (cut_to(text, *max), cut_from_whole(text, *max));
                if new == old {
                    Ok(())
                } else {
                    Err(format!(
                        "cut to {max}: {} bytes, not {}",
                        new.len(),
                        old.len()
                    ))
                }
            },
        );
    }

    #[test]
    fn a_cut_in_a_huge_cluster_reads_only_what_it_keeps() {
        // One cluster of 4 MiB: an `e` and two million combining marks.
        let mut text = String::from("e");
        text.push_str(&"\u{301}".repeat(2 << 20));
        let started = Instant::now();
        for _ in 0..100 {
            assert_eq!(cut_to(&text, MAX_TAB_LABEL_BYTES), "");
            assert_eq!(cut_to(&text, MAX_CELL_BYTES), "");
        }
        // Scanning the cluster each time took about 100 ms a cut; reading
        // only the head takes microseconds.
        let took = started.elapsed();
        assert!(took < Duration::from_millis(500), "{took:?}");
        // A cluster that ends inside the head is kept.
        let mut short = "e\u{301}".repeat(100);
        short.push_str(&text);
        assert_eq!(cut_to(&short, 301), "e\u{301}".repeat(100));
    }
}
