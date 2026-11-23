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

//! The shell's own widgets (M3.4, ADR-0060): the surfaces its areas paint,
//! the title strip, the activity bar's buttons, and plain lines of text.
//! Every other part of the shell is one of `ui`'s controls.
//!
//! New in Tidepool.

use tidepool_core::color::Color;
use tidepool_core::geom::{Rect, Size};
use tidepool_core::input::{InputEvent, Key, Modifiers, MouseButton};
use tidepool_ui::a11y::{Role, Semantics};
use tidepool_ui::draw::{Corners, DrawList, PaintContext};
use tidepool_ui::input::{ActionKind, EventContext, Handled};
use tidepool_ui::layout::Style;
use tidepool_ui::metrics::{Family, TextMetrics, TextStyle, Weight};
use tidepool_ui::theme::Token;
use tidepool_ui::widgets::{Control, DISABLED_OPACITY, FOCUS_WIDTH};
use tidepool_ui::{Tree, Widget};

use super::commands::View;

/// The brand in the title strip: Saira 15 px, 600, tracked 0.28em,
/// uppercase, `text`.
pub const BRAND: TextStyle = TextStyle::new(Family::Ui, 15.0)
    .weight(Weight::SemiBold)
    .letter_spacing(0.28)
    .line_height(1.0);

/// The workspace's line beside the brand: Saira 10 px, 500, tracked
/// 0.42em, uppercase, `text-3`.
pub const BRAND_SUBLINE: TextStyle = TextStyle::new(Family::Ui, 10.0)
    .weight(Weight::Medium)
    .letter_spacing(0.42)
    .line_height(1.0);

/// The brand mark's side: a `teal` square with cut corners.
pub const BRAND_MARK: f32 = 12.0;

/// How much of each corner the brand mark cuts.
pub const BRAND_MARK_CUT: f32 = 3.0;

/// From the window's left edge to the brand mark, and from the mark to the
/// brand.
pub const BRAND_INSET: f32 = 16.0;

/// From the brand to the workspace's line.
pub const BRAND_GAP: f32 = 14.0;

/// The opacity of the `teal` glow along the title strip's bottom edge, at
/// its middle: the site header's `::after` gradient at 40%.
pub const GLOW_OPACITY: f32 = 0.4;

/// How many pieces draw the glow: each a constant alpha, from transparent
/// at the strip's ends to [`GLOW_OPACITY`] at its middle.
pub const GLOW_STEPS: usize = 32;

/// The side of an activity bar icon.
pub const ICON: f32 = 24.0;

/// The width of the `teal` rule down the left of the active view's button.
pub const ACTIVE_RULE: f32 = 2.0;

/// Which edges of a [`Surface`] have a hairline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Edges {
    /// The top edge.
    pub top: bool,
    /// The right edge.
    pub right: bool,
    /// The bottom edge.
    pub bottom: bool,
    /// The left edge.
    pub left: bool,
}

impl Edges {
    /// No edges.
    pub const NONE: Self = Self {
        top: false,
        right: false,
        bottom: false,
        left: false,
    };

    /// The top edge alone.
    pub const TOP: Self = Self {
        top: true,
        ..Self::NONE
    };

    /// The right edge alone.
    pub const RIGHT: Self = Self {
        right: true,
        ..Self::NONE
    };
}

/// A surface of the shell: its bounds filled with a token, and a 1-unit
/// `line` hairline inside the edges it is given, as jabbott.dev's panels
/// have (`.panel`: `panel` over the backdrop, a `line` border, no radius).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Surface {
    /// What fills it; `None` lets the backdrop show.
    pub fill: Option<Token>,
    /// Where its hairlines go.
    pub edges: Edges,
}

impl Widget for Surface {
    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let b = cx.bounds();
        let theme = cx.theme();
        if let Some(fill) = self.fill {
            list.rect(b, theme.color(fill));
        }
        hairlines(list, b, self.edges, theme.color(Token::Line));
    }
}

/// Draws a 1-unit hairline inside each of `edges` of `b`.
fn hairlines(list: &mut DrawList, b: Rect, edges: Edges, color: Color) {
    let one = 1.0_f32.min(b.width).min(b.height);
    if edges.top {
        list.rect(Rect::new(b.x, b.y, b.width, one), color);
    }
    if edges.bottom {
        list.rect(Rect::new(b.x, b.y + b.height - one, b.width, one), color);
    }
    if edges.left {
        list.rect(Rect::new(b.x, b.y, one, b.height), color);
    }
    if edges.right {
        list.rect(Rect::new(b.x + b.width - one, b.y, one, b.height), color);
    }
}

/// The title strip (`.site-header`): `panel`, with the brand mark in
/// `teal`, `TIDEPOOL` in [`BRAND`] and the workspace's line in
/// [`BRAND_SUBLINE`], and along its bottom a `line` hairline with a `teal`
/// glow in its middle at [`GLOW_OPACITY`], fading to nothing at its ends:
/// the header's gradient, drawn as [`GLOW_STEPS`] pieces, since no draw
/// command paints a gradient.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TitleStrip {
    workspace: String,
}

impl TitleStrip {
    /// A strip naming `workspace`, shown uppercase, as chrome is.
    pub fn new(workspace: impl AsRef<str>) -> Self {
        Self {
            workspace: workspace.as_ref().to_uppercase(),
        }
    }

    /// What it shows for the workspace, uppercase.
    pub fn workspace(&self) -> &str {
        &self.workspace
    }
}

impl Widget for TitleStrip {
    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let b = cx.bounds();
        let theme = cx.theme();
        let metrics = cx.metrics();
        list.rect(b, theme.color(Token::Panel));
        list.push_clip(b);
        let middle = b.y + b.height / 2.0;
        let mark = Rect::new(
            b.x + BRAND_INSET,
            middle - BRAND_MARK / 2.0,
            BRAND_MARK,
            BRAND_MARK,
        );
        list.cut_corner_rect(mark, BRAND_MARK_CUT, Corners::ALL, theme.color(Token::Teal));
        let brand_x = mark.x + BRAND_MARK + BRAND_INSET / 2.0;
        list.text(
            tidepool_core::geom::Point::new(brand_x, baseline(b, metrics, &BRAND)),
            "TIDEPOOL",
            BRAND,
            theme.color(Token::Text),
        );
        let sub_x = brand_x + metrics.width("TIDEPOOL", &BRAND) + BRAND_GAP;
        list.text(
            tidepool_core::geom::Point::new(sub_x, baseline(b, metrics, &BRAND_SUBLINE)),
            self.workspace.clone(),
            BRAND_SUBLINE,
            theme.color(Token::Text3),
        );
        list.pop_clip();
        let rule = Rect::new(b.x, b.y + b.height - 1.0, b.width, 1.0_f32.min(b.height));
        list.rect(rule, theme.color(Token::Line));
        let teal = theme.color(Token::Teal);
        let piece = rule.width / GLOW_STEPS as f32;
        for step in 0..GLOW_STEPS {
            // Each piece's alpha is the gradient's at its middle.
            let t = (step as f32 + 0.5) / GLOW_STEPS as f32;
            let strength = GLOW_OPACITY * (1.0 - (2.0 * t - 1.0).abs());
            let alpha = (f32::from(teal.a) * strength).round() as u8;
            if alpha == 0 {
                continue;
            }
            let x = rule.x + piece * step as f32;
            // The last piece ends exactly at the strip's right edge.
            let width = if step + 1 == GLOW_STEPS {
                rule.x + rule.width - x
            } else {
                piece
            };
            list.rect(
                Rect::new(x, rule.y, width, rule.height),
                Color { a: alpha, ..teal },
            );
        }
    }
}

/// The baseline that centers a line of `style` in `b` vertically.
fn baseline(b: Rect, metrics: &dyn TextMetrics, style: &TextStyle) -> f32 {
    let line = metrics.line(style);
    b.y + (b.height - (line.ascent + line.descent)) / 2.0 + line.ascent
}

/// A button of the activity bar: a view's [`ICON`]-wide icon, centered,
/// that shows the view in the side panel.
///
/// At rest its icon is `text-2`; hovered, `text` on the `row-hover` wash;
/// pressed, on `teal-wash`. The active view's icon is `teal`, with a
/// [`ACTIVE_RULE`]-wide `teal` rule down its left. Keyboard focus draws the
/// focus outline [`FOCUS_WIDTH`] wide inside its bounds, as a collection's
/// is, since the bar runs along the window's edge, which would cut an
/// outline outside it off. Disabled, it is drawn at [`DISABLED_OPACITY`].
///
/// It is activated as `ui`'s buttons are: by a click, a primary press
/// released over it; by Enter going down, not repeating; or by Space
/// coming up after it went down on it, unless focus moved meanwhile.
/// Either key counts with no modifier but Shift. It is a tab in the
/// accessibility tree, named for its view, selected while its view is the
/// side panel's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActivityButton {
    view: View,
    active: bool,
    /// Space went down on it and has not come up: the count of focus moves
    /// then.
    space: Option<u64>,
}

impl ActivityButton {
    /// The button of `view`, active or not.
    pub const fn new(view: View, active: bool) -> Self {
        Self {
            view,
            active,
            space: None,
        }
    }

    /// Its view.
    pub const fn view(&self) -> View {
        self.view
    }

    /// Whether its view is the side panel's.
    pub const fn is_active(&self) -> bool {
        self.active
    }

    /// Makes its view the side panel's, or not.
    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    /// Takes `event` and returns whether it activated the button, and
    /// whether the button keeps it from its ancestors.
    fn activation(&mut self, cx: &EventContext<'_>, event: &InputEvent) -> (bool, Handled) {
        let plain = |modifiers: Modifiers| modifiers.difference(Modifiers::SHIFT).is_empty();
        match *event {
            InputEvent::MouseDown {
                button: MouseButton::Left,
                ..
            } => {
                self.space = None;
                (false, Handled::Yes)
            }
            InputEvent::MouseUp {
                button: MouseButton::Left,
                ..
            } => {
                let state = cx.state();
                (state.pressed && state.hovered, Handled::Yes)
            }
            InputEvent::KeyDown {
                key: Key::Enter | Key::NumpadEnter,
                modifiers,
                repeat,
            } if plain(modifiers) => {
                self.space = None;
                (!repeat, Handled::Yes)
            }
            InputEvent::KeyDown {
                key: Key::Space,
                modifiers,
                repeat,
            } if plain(modifiers) => {
                if !repeat {
                    self.space = Some(cx.focus_moves());
                }
                (false, Handled::Yes)
            }
            InputEvent::KeyUp {
                key: Key::Space, ..
            } => match self.space.take() {
                Some(moves) => (moves == cx.focus_moves(), Handled::Yes),
                None => (false, Handled::No),
            },
            InputEvent::Text(ref text) if text == " " => (false, Handled::Yes),
            InputEvent::KeyDown { key, .. } => {
                if !is_modifier(key) {
                    self.space = None;
                }
                (false, Handled::No)
            }
            _ => (false, Handled::No),
        }
    }
}

/// Whether `key` is Control, Shift, Alt or Super, left or right.
const fn is_modifier(key: Key) -> bool {
    matches!(
        key,
        Key::LeftControl
            | Key::LeftShift
            | Key::LeftAlt
            | Key::LeftSuper
            | Key::RightControl
            | Key::RightShift
            | Key::RightAlt
            | Key::RightSuper
    )
}

impl Widget for ActivityButton {
    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let b = cx.bounds();
        let theme = cx.theme();
        let state = cx.state();
        let disabled = state.disabled;
        if disabled {
            list.push_layer(DISABLED_OPACITY);
        }
        if !disabled && state.pressed && state.hovered {
            list.rect(b, theme.color(Token::TealWash));
        } else if !disabled && state.hovered {
            list.rect(b, theme.color(Token::RowHover));
        }
        let icon = Rect::new(
            b.x + (b.width - ICON) / 2.0,
            b.y + (b.height - ICON) / 2.0,
            ICON,
            ICON,
        );
        let color = if self.active {
            Token::Teal
        } else if state.hovered && !disabled {
            Token::Text
        } else {
            Token::Text2
        };
        list.icon(icon, self.view.icon(), theme.color(color));
        if self.active {
            list.rect(
                Rect::new(b.x, b.y, ACTIVE_RULE.min(b.width), b.height),
                theme.color(Token::Teal),
            );
        }
        if state.focus_visible && !disabled {
            let inset = FOCUS_WIDTH;
            if b.width > 2.0 * inset && b.height > 2.0 * inset {
                list.border(
                    Rect::new(
                        b.x + inset,
                        b.y + inset,
                        b.width - 2.0 * inset,
                        b.height - 2.0 * inset,
                    ),
                    FOCUS_WIDTH,
                    theme.color(Token::Teal),
                );
            }
        }
        if disabled {
            list.pop_layer();
        }
    }

    fn event(&mut self, cx: &mut EventContext<'_>, event: &InputEvent) -> Handled {
        let (activated, handled) = self.activation(cx, event);
        if activated {
            cx.emit(ActionKind::Activate);
        }
        handled
    }
}

impl Control for ActivityButton {
    fn semantics(&self) -> Semantics {
        let semantics = Semantics::new(Role::Tab, self.view.name());
        if self.active {
            semantics.selected()
        } else {
            semantics
        }
    }

    fn style(&self) -> Style {
        Style::stack().fixed(super::ACTIVITY_WIDTH, super::ACTIVITY_WIDTH)
    }

    fn focusable(&self) -> bool {
        true
    }
}

/// One line of text in a style and a token, as written: the side panel's
/// and the bottom panel's empty lines, the welcome, and the status bar's
/// items. Its natural size is the text's; it paints nothing outside its
/// bounds. It has no role of its own: the area it is in names what it
/// says.
#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    text: String,
    style: TextStyle,
    token: Token,
}

impl Label {
    /// `text` in `style` and `token`.
    pub fn new(text: impl Into<String>, style: TextStyle, token: Token) -> Self {
        Self {
            text: text.into(),
            style,
            token,
        }
    }

    /// What it says.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Makes it say `text`.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
    }
}

impl Widget for Label {
    fn measure(&self, _available: Size, metrics: &dyn TextMetrics) -> Size {
        Size::new(
            metrics.width(&self.text, &self.style),
            metrics.line(&self.style).height,
        )
    }

    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let b = cx.bounds();
        if b.width <= 0.0 || b.height <= 0.0 {
            return;
        }
        let y = baseline(b, cx.metrics(), &self.style);
        list.push_clip(b);
        list.text(
            tidepool_core::geom::Point::new(b.x, y),
            self.text.clone(),
            self.style,
            cx.theme().color(self.token),
        );
        list.pop_clip();
    }
}

/// Changes the [`Label`] at `id` to say `text`; does nothing if `id` holds
/// none, or it says `text` already, so that an unchanged label is not
/// repainted ([`Tree::widget_mut`] marks its node).
pub fn set_label(tree: &mut Tree, id: tidepool_ui::NodeId, text: &str) {
    if tree
        .widget::<Label>(id)
        .is_some_and(|label| label.text() != text)
        && let Some(label) = tree.widget_mut::<Label>(id)
    {
        label.set_text(text);
    }
}
