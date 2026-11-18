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

//! Buttons: `.btn` and its `--ghost`, `--teal`, `--amber` and `--slate`
//! kinds, 30 or 24 units high.

use tidepool_core::geom::{Point, Rect, Size};
use tidepool_core::input::{InputEvent, Key, Modifiers, MouseButton};

use crate::a11y::{Role, Semantics};
use crate::draw::{Corners, DrawList, PaintContext};
use crate::input::{ActionKind, EventContext, Handled, is_modifier};
use crate::metrics::TextMetrics;
use crate::theme::{Theme, Token};
use crate::tree::Widget;

use super::{
    BUTTON_HEIGHT, BUTTON_HEIGHT_SMALL, BUTTON_LABEL, BUTTON_PADDING, BUTTON_PADDING_SMALL,
    Control, Look, PRESSED_RIM, baseline, dimmed, focus_outline, inset,
};

/// What a button is for, and so how it looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ButtonKind {
    /// The default: a faint fill and a `line-2` border; hovered, a `teal`
    /// border and a `teal-hi` label.
    #[default]
    Ghost,
    /// Secondary: a `teal-wash` fill, a `teal-line` border and a `teal-hi`
    /// label.
    Teal,
    /// Primary: `amber`, with its top-right and bottom-left corners cut by
    /// the theme's `cut`, and an `on-amber` label.
    Amber,
    /// Quiet: a `slate` fill and no border.
    Slate,
}

/// How tall a button is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ButtonSize {
    /// [`BUTTON_HEIGHT`] high.
    #[default]
    Regular,
    /// [`BUTTON_HEIGHT_SMALL`] high.
    Small,
}

impl ButtonSize {
    const fn height(self) -> f32 {
        match self {
            Self::Regular => BUTTON_HEIGHT,
            Self::Small => BUTTON_HEIGHT_SMALL,
        }
    }

    const fn padding(self) -> f32 {
        match self {
            Self::Regular => BUTTON_PADDING,
            Self::Small => BUTTON_PADDING_SMALL,
        }
    }
}

/// A button with a label, which emits [`ActionKind::Activate`] when it is
/// clicked (pressed and released on it with the primary button), or, while
/// it has focus, when Enter goes down or Space comes up.
///
/// Its label is painted uppercase in [`BUTTON_LABEL`]; its accessible name
/// is the label as written. It takes focus.
///
/// ```
/// use tidepool_ui::a11y::Role;
/// use tidepool_ui::widgets::{Button, ButtonKind, ButtonSize, Control};
///
/// let save = Button::new("Save", ButtonKind::Teal).size(ButtonSize::Small);
/// assert_eq!(save.semantics().role, Role::Button);
/// assert_eq!(save.semantics().name, "Save");
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Button {
    label: String,
    shown: String,
    kind: ButtonKind,
    size: ButtonSize,
    activator: Activator,
}

impl Button {
    /// A regular button of `kind` labelled `label`.
    pub fn new(label: impl Into<String>, kind: ButtonKind) -> Self {
        let label = label.into();
        Self {
            shown: label.to_uppercase(),
            label,
            kind,
            size: ButtonSize::Regular,
            activator: Activator::default(),
        }
    }

    /// The same, of `size`.
    #[must_use]
    pub fn size(self, size: ButtonSize) -> Self {
        Self { size, ..self }
    }

    /// Its label, as written.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Labels it `label`.
    pub fn set_label(&mut self, label: impl Into<String>) {
        self.label = label.into();
        self.shown = self.label.to_uppercase();
    }

    /// Its kind.
    pub const fn kind(&self) -> ButtonKind {
        self.kind
    }

    /// Makes it `kind`.
    pub fn set_kind(&mut self, kind: ButtonKind) {
        self.kind = kind;
    }

    /// Its size.
    pub const fn button_size(&self) -> ButtonSize {
        self.size
    }

    /// Paints the shape and the label's color for `look`.
    fn paint_shape(&self, theme: &Theme, rect: Rect, look: Look, list: &mut DrawList) -> Token {
        let c = |token| theme.color(token);
        match self.kind {
            ButtonKind::Ghost => {
                let fill = if look == Look::Pressed {
                    Token::TealWash
                } else {
                    Token::Ghost
                };
                let (border, label) = match look {
                    Look::Rest => (Token::Line2, Token::Text),
                    Look::Hover | Look::Pressed => (Token::Teal, Token::TealHi),
                };
                list.rect(rect, c(fill));
                list.border(rect, 1.0, c(border));
                label
            }
            ButtonKind::Teal => {
                let (fill, border) = match look {
                    Look::Rest => (Token::TealWash, Token::TealLine),
                    Look::Hover => (Token::TealFill, Token::Teal),
                    Look::Pressed => (Token::TealGlow, Token::Teal),
                };
                list.rect(rect, c(fill));
                list.border(rect, 1.0, c(border));
                Token::TealHi
            }
            ButtonKind::Slate => {
                let (fill, label) = match look {
                    Look::Rest => (Token::Slate, Token::Text),
                    Look::Hover | Look::Pressed => (Token::SlateHi, Token::TealHi),
                };
                list.rect(rect, c(fill));
                if look == Look::Pressed {
                    list.border(rect, 1.0, c(Token::TealLine));
                }
                label
            }
            ButtonKind::Amber => {
                let cut = theme.cut();
                let corners = Corners::TOP_RIGHT_AND_BOTTOM_LEFT;
                match look {
                    Look::Rest => list.cut_corner_rect(rect, cut, corners, c(Token::Amber)),
                    Look::Hover => list.cut_corner_rect(rect, cut, corners, c(Token::AmberHi)),
                    Look::Pressed => {
                        // The rest fill inside a rim of the hover fill: the
                        // inner cut keeps the diagonal rim as wide as the
                        // straight one, (2 − √2) × the rim shorter.
                        list.cut_corner_rect(rect, cut, corners, c(Token::AmberHi));
                        let inner_cut =
                            (cut - (2.0 - std::f32::consts::SQRT_2) * PRESSED_RIM).max(0.0);
                        list.cut_corner_rect(
                            inset(rect, PRESSED_RIM),
                            inner_cut,
                            corners,
                            c(Token::Amber),
                        );
                    }
                }
                Token::OnAmber
            }
        }
    }
}

impl Widget for Button {
    fn measure(&self, _available: Size, metrics: &dyn TextMetrics) -> Size {
        let width = metrics.width(&self.shown, &BUTTON_LABEL);
        Size::new(width + 2.0 * self.size.padding(), self.size.height())
    }

    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let rect = cx.bounds();
        let theme = cx.theme();
        let metrics = cx.metrics();
        dimmed(cx, list, |list| {
            let label = self.paint_shape(theme, rect, Look::of(cx.state()), list);
            let width = metrics.width(&self.shown, &BUTTON_LABEL);
            let origin = Point::new(
                rect.x + (rect.width - width) / 2.0,
                baseline(rect, metrics, &BUTTON_LABEL),
            );
            list.text(origin, self.shown.clone(), BUTTON_LABEL, theme.color(label));
        });
        if cx.state().focus_visible {
            focus_outline(list, rect, theme);
        }
    }

    fn event(&mut self, cx: &mut EventContext<'_>, event: &InputEvent) -> Handled {
        self.activator.activate(cx, event)
    }
}

impl Control for Button {
    fn semantics(&self) -> Semantics {
        Semantics::new(Role::Button, self.label.clone())
    }

    fn focusable(&self) -> bool {
        true
    }
}

/// What a clickable control (a button, a chip, a link) remembers between
/// events to know when it is activated, and which events it keeps from its
/// ancestors.
///
/// It is activated by a click, the release of a primary press it captured
/// while the pointer is over it (on the [hit](crate::Tree::hit_test) path,
/// so not where a clip or a node above it hides it); by Enter going down,
/// not repeating; or by Space coming up after it went down on the control,
/// as a browser's button is, so that the space Space types, which comes
/// between, is the control's whatever the owner does on the activation
/// (moving focus to a field, say). Either key counts with no modifier but
/// Shift. Space's activation is called off if, while it is held, keyboard
/// focus moves or the window loses it
/// ([`EventContext::focus_moves`]), so a release the platform reports for
/// a key held as the window lost focus activates nothing, or another key
/// but a modifier goes down, or the pointer presses on the control.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Activator {
    /// Space went down on the control, and has not come up: the count of
    /// focus moves then.
    space: Option<u64>,
}

impl Activator {
    /// Takes `event`, and returns whether it activated the control and
    /// whether the control keeps it from its ancestors: a primary press or
    /// release, Enter and Space going down (repeating or not), Space going
    /// up after it went down here, and the space it types.
    pub(crate) fn event(&mut self, cx: &EventContext<'_>, event: &InputEvent) -> (bool, Handled) {
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

    /// Takes `event` as [`event`](Self::event) does, emitting
    /// [`ActionKind::Activate`] when it activates the control.
    pub(crate) fn activate(&mut self, cx: &mut EventContext<'_>, event: &InputEvent) -> Handled {
        let (activated, handled) = self.event(cx, event);
        if activated {
            cx.emit(ActionKind::Activate);
        }
        handled
    }
}
