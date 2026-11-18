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

//! Chips: `.chip` and `.chip.is-active`, fully rounded toggles for
//! filters and pills.

use tidepool_core::geom::{Point, Size};
use tidepool_core::input::InputEvent;

use crate::a11y::{Role, Semantics};
use crate::draw::{DrawList, PaintContext};
use crate::input::{ActionKind, EventContext, Handled};
use crate::metrics::TextMetrics;
use crate::theme::Token;
use crate::tree::Widget;

use super::button::Activator;
use super::{
    CHIP_HEIGHT, CHIP_LABEL, CHIP_PADDING, Control, Look, PRESSED_RIM, baseline, dimmed,
    focus_outline, inset,
};

/// A chip: a pill that is selected or not, as a search or severity filter
/// is. A click on it, or Enter going down or Space coming up while it has
/// focus, flips it and emits [`ActionKind::Change`].
///
/// Not selected, it is `slate` with a `teal` label, and hovered it gains a
/// `teal-line` border; selected, it is `teal` with an `on-teal` label,
/// `teal-hi` when hovered. Its label is painted uppercase in
/// [`CHIP_LABEL`]. It is a toggle button in the accessibility tree,
/// checked when selected, and takes focus.
///
/// ```
/// use tidepool_ui::a11y::Role;
/// use tidepool_ui::widgets::{Chip, Control};
///
/// let errors = Chip::new("Errors").selected(true);
/// assert_eq!(errors.semantics().role, Role::ToggleButton);
/// assert_eq!(errors.semantics().checked, Some(true));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chip {
    label: String,
    shown: String,
    selected: bool,
    activator: Activator,
}

impl Chip {
    /// A chip labelled `label`, not selected.
    pub fn new(label: impl Into<String>) -> Self {
        let label = label.into();
        Self {
            shown: label.to_uppercase(),
            label,
            selected: false,
            activator: Activator::default(),
        }
    }

    /// The same, selected or not.
    #[must_use]
    pub fn selected(self, selected: bool) -> Self {
        Self { selected, ..self }
    }

    /// Its label, as written.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Whether it is selected.
    pub const fn is_selected(&self) -> bool {
        self.selected
    }

    /// Selects it or not.
    pub fn set_selected(&mut self, selected: bool) {
        self.selected = selected;
    }
}

impl Widget for Chip {
    fn measure(&self, _available: Size, metrics: &dyn TextMetrics) -> Size {
        let width = metrics.width(&self.shown, &CHIP_LABEL);
        Size::new(width + 2.0 * CHIP_PADDING, CHIP_HEIGHT)
    }

    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let rect = cx.bounds();
        let theme = cx.theme();
        let metrics = cx.metrics();
        let c = |token| theme.color(token);
        dimmed(cx, list, |list| {
            let look = Look::of(cx.state());
            let label = if self.selected {
                match look {
                    Look::Rest => list.pill(rect, c(Token::Teal)),
                    Look::Hover => list.pill(rect, c(Token::TealHi)),
                    Look::Pressed => {
                        list.pill(rect, c(Token::TealHi));
                        list.pill(inset(rect, PRESSED_RIM), c(Token::Teal));
                    }
                }
                Token::OnTeal
            } else {
                let fill = if look == Look::Pressed {
                    Token::SlateHi
                } else {
                    Token::Slate
                };
                list.pill(rect, c(fill));
                if look != Look::Rest {
                    // A 1-unit border over the fill, as CSS paints one: the
                    // border's color over the whole pill, then the fill
                    // inside it.
                    list.pill(rect, c(Token::TealLine));
                    list.pill(inset(rect, 1.0), c(fill));
                }
                Token::Teal
            };
            let width = metrics.width(&self.shown, &CHIP_LABEL);
            let origin = Point::new(
                rect.x + (rect.width - width) / 2.0,
                baseline(rect, metrics, &CHIP_LABEL),
            );
            list.text(origin, self.shown.clone(), CHIP_LABEL, c(label));
        });
        if cx.state().focus_visible {
            focus_outline(list, rect, theme);
        }
    }

    fn event(&mut self, cx: &mut EventContext<'_>, event: &InputEvent) -> Handled {
        let (activated, handled) = self.activator.event(cx, event);
        if activated {
            self.selected = !self.selected;
            cx.emit(ActionKind::Change);
            cx.set_semantics(self.semantics());
            cx.request_paint();
        }
        handled
    }
}

impl Control for Chip {
    fn semantics(&self) -> Semantics {
        Semantics::new(Role::ToggleButton, self.label.clone()).checked(self.selected)
    }

    fn focusable(&self) -> bool {
        true
    }
}
