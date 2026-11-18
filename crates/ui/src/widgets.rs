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
//! language: [`Button`]s (ghost, teal, amber with cut corners, slate),
//! [`Chip`]s, the [`TextInput`] and search field, the [`ScrollBar`], the
//! [focus outline](focus_outline), the [`StatusDot`], the
//! [`SectionTitle`], [`Eyebrow`] and [`Kicker`], and [`Breadcrumbs`].
//!
//! New in Tidepool; not ported. The looks follow jabbott.dev's stylesheet
//! (`assets/css/style.css`) at the IDE's denser scale, and ADR-0049 where
//! the site has none.
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
//! this module, from the spec's Typography and Components tables, and the
//! amber button's cut is the theme's [`cut`](crate::theme::Theme::cut).
//! Labels are painted uppercase, as chrome is; what a user wrote (a
//! field's text, a breadcrumb's path segment) is painted as written, and
//! every accessible name is as written.
//!
//! # States
//!
//! A control paints from its node's [`NodeState`]: *hover* is the site's
//! `:hover`; *pressed* (the spec's "active") is the hover look with the
//! fill one step stronger, or a rim where the palette has no stronger
//! fill; *focus* shows the [focus outline](focus_outline) when focus is
//! visible, and a field its ring whenever it has focus; *disabled* is the
//! rest look in a layer at [`DISABLED_OPACITY`], with no hover or pressed
//! look. No control blinks or animates: an idle tree damages nothing.
//!
//! [`NodeState`]: crate::tree::NodeState

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
mod chip;
mod field;
mod labels;
mod scrollbar;

pub use breadcrumbs::{Breadcrumbs, Crumb};
pub use button::{Button, ButtonKind, ButtonSize};
pub use chip::Chip;
pub use field::{FieldKind, MAX_INPUT_BYTES, TextInput};
pub use labels::{Eyebrow, Kicker, SectionTitle, Status, StatusDot};
pub use scrollbar::ScrollBar;

/// The opacity of a disabled control's layer.
pub const DISABLED_OPACITY: f32 = 0.4;

/// The width of the focus outline (`:focus-visible`).
pub const FOCUS_WIDTH: f32 = 2.0;

/// How far outside a control's bounds the focus outline starts.
pub const FOCUS_OFFSET: f32 = 3.0;

/// The height of a regular button (`.btn`, at the IDE's scale).
pub const BUTTON_HEIGHT: f32 = 30.0;

/// The height of a small button (`.btn--sm`).
pub const BUTTON_HEIGHT_SMALL: f32 = 24.0;

/// The space either side of a regular button's label.
pub const BUTTON_PADDING: f32 = 14.0;

/// The space either side of a small button's label.
pub const BUTTON_PADDING_SMALL: f32 = 10.0;

/// The width of the rim a pressed control shows where the palette has no
/// stronger fill: an amber button, a selected chip (ADR-0049).
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

/// The label of a button: Saira 12 px, 600, tracked 0.10em, uppercase.
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
/// when their node's focus is visible; fields show their ring instead.
pub fn focus_outline(list: &mut DrawList, bounds: Rect, theme: &Theme) {
    list.border(
        grow(bounds, FOCUS_OFFSET + FOCUS_WIDTH),
        FOCUS_WIDTH,
        theme.color(Token::Teal),
    );
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
