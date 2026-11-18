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

//! The controls that only show something: the status dot (`.status-dot`),
//! the section title (`.section-title`), the eyebrow (`.eyebrow`) and the
//! kicker (`.kicker`). None takes focus or handles an event; each gives
//! itself its role and the name its owner gives it.

use tidepool_core::geom::{Point, Rect, Size};

use crate::a11y::{Role, Semantics};
use crate::draw::{DrawList, PaintContext};
use crate::metrics::{TextMetrics, TextStyle};
use crate::theme::Token;
use crate::tree::Widget;

use super::{
    Control, DOT, EYEBROW, HALO, KICKER, SECTION_GAP, SECTION_RULE, SECTION_TITLE, baseline,
    dimmed, line_height,
};

/// What a status dot says about a toolchain, a language server or an
/// agent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Status {
    /// Ready: `green`, with its halo.
    Ready,
    /// Busy, or outdated: `amber`, with its halo.
    Busy,
    /// Failed: `error`, with its halo.
    Failed,
    /// Off: `text-3`, with no halo.
    #[default]
    Off,
}

impl Status {
    /// The word for it, which is the dot's value in the accessibility
    /// tree: `"ready"`, `"busy"`, `"failed"`, `"off"`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Busy => "busy",
            Self::Failed => "failed",
            Self::Off => "off",
        }
    }

    /// The dot's color and its halo's.
    const fn tokens(self) -> (Token, Option<Token>) {
        match self {
            Self::Ready => (Token::Green, Some(Token::GreenHalo)),
            Self::Busy => (Token::Amber, Some(Token::AmberHalo)),
            Self::Failed => (Token::Error, Some(Token::ErrorHalo)),
            Self::Off => (Token::Text3, None),
        }
    }
}

/// A status dot: a [`DOT`]-wide circle in its status's color, with a
/// [`HALO`]-wide ring of its halo around it, centred in its bounds. It
/// does not pulse: the site's pulse is motion, which is opt-in, and its
/// soft glow is a blur, which no command draws (ADR-0049).
///
/// It is a status in the accessibility tree, named for what it is the
/// status of, its value the status's [name](Status::name).
///
/// ```
/// use tidepool_ui::widgets::{Control, Status, StatusDot};
///
/// let dot = StatusDot::new("rust-analyzer", Status::Ready);
/// assert_eq!(dot.semantics().value.as_deref(), Some("ready"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusDot {
    name: String,
    status: Status,
}

impl StatusDot {
    /// The status of what `name` names.
    pub fn new(name: impl Into<String>, status: Status) -> Self {
        Self {
            name: name.into(),
            status,
        }
    }

    /// Its status.
    pub const fn status(&self) -> Status {
        self.status
    }

    /// Sets its status.
    pub fn set_status(&mut self, status: Status) {
        self.status = status;
    }
}

impl Widget for StatusDot {
    fn measure(&self, _available: Size, _metrics: &dyn TextMetrics) -> Size {
        let side = DOT + 2.0 * HALO;
        Size::new(side, side)
    }

    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let bounds = cx.bounds();
        let theme = cx.theme();
        let centre = Point::new(
            bounds.x + bounds.width / 2.0,
            bounds.y + bounds.height / 2.0,
        );
        let circle = |diameter: f32| {
            Rect::new(
                centre.x - diameter / 2.0,
                centre.y - diameter / 2.0,
                diameter,
                diameter,
            )
        };
        let (dot, halo) = self.status.tokens();
        dimmed(cx, list, |list| {
            if let Some(halo) = halo {
                list.pill(circle(DOT + 2.0 * HALO), theme.color(halo));
            }
            list.pill(circle(DOT), theme.color(dot));
        });
    }
}

impl Control for StatusDot {
    fn semantics(&self) -> Semantics {
        Semantics::new(Role::Status, self.name.clone()).with_value(self.status.name())
    }

    fn focusable(&self) -> bool {
        false
    }
}

/// Measures and paints one line of uppercase chrome text, after `lead`
/// units, vertically centred.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Line {
    text: String,
    shown: String,
}

impl Line {
    fn new(text: String) -> Self {
        Self {
            shown: text.to_uppercase(),
            text,
        }
    }

    fn measure(&self, lead: f32, style: &TextStyle, metrics: &dyn TextMetrics) -> Size {
        Size::new(
            lead + metrics.width(&self.shown, style),
            line_height(metrics, style),
        )
    }

    fn paint(
        &self,
        cx: &PaintContext<'_>,
        lead: f32,
        style: TextStyle,
        token: Token,
        list: &mut DrawList,
    ) {
        let bounds = cx.bounds();
        let origin = Point::new(bounds.x + lead, baseline(bounds, cx.metrics(), &style));
        list.text(origin, self.shown.clone(), style, cx.theme().color(token));
    }
}

/// A section title, a panel's header: [`SECTION_TITLE`] in `text`,
/// uppercase, after a [`SECTION_RULE`]-wide `teal` rule down its left
/// edge. A heading in the accessibility tree, named as written.
///
/// ```
/// use tidepool_ui::a11y::Role;
/// use tidepool_ui::widgets::{Control, SectionTitle};
///
/// assert_eq!(SectionTitle::new("Problems").semantics().role, Role::Heading);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SectionTitle(Line);

impl SectionTitle {
    /// A section title reading `text`.
    pub fn new(text: impl Into<String>) -> Self {
        Self(Line::new(text.into()))
    }

    /// Its text, as written.
    pub fn text(&self) -> &str {
        &self.0.text
    }
}

impl Widget for SectionTitle {
    fn measure(&self, _available: Size, metrics: &dyn TextMetrics) -> Size {
        self.0
            .measure(SECTION_RULE + SECTION_GAP, &SECTION_TITLE, metrics)
    }

    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let bounds = cx.bounds();
        dimmed(cx, list, |list| {
            list.rect(
                Rect::new(bounds.x, bounds.y, SECTION_RULE, bounds.height),
                cx.theme().color(Token::Teal),
            );
            self.0.paint(
                cx,
                SECTION_RULE + SECTION_GAP,
                SECTION_TITLE,
                Token::Text,
                list,
            );
        });
    }
}

impl Control for SectionTitle {
    fn semantics(&self) -> Semantics {
        Semantics::new(Role::Heading, self.0.text.clone())
    }

    fn focusable(&self) -> bool {
        false
    }
}

/// An eyebrow, a side panel's group label (`EXPLORER`, `OPEN EDITORS`):
/// [`EYEBROW`] in `text-3`, uppercase. A heading in the accessibility
/// tree, named as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Eyebrow(Line);

impl Eyebrow {
    /// An eyebrow reading `text`.
    pub fn new(text: impl Into<String>) -> Self {
        Self(Line::new(text.into()))
    }

    /// Its text, as written.
    pub fn text(&self) -> &str {
        &self.0.text
    }
}

impl Widget for Eyebrow {
    fn measure(&self, _available: Size, metrics: &dyn TextMetrics) -> Size {
        self.0.measure(0.0, &EYEBROW, metrics)
    }

    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        dimmed(cx, list, |list| {
            self.0.paint(cx, 0.0, EYEBROW, Token::Text3, list);
        });
    }
}

impl Control for Eyebrow {
    fn semantics(&self) -> Semantics {
        Semantics::new(Role::Heading, self.0.text.clone())
    }

    fn focusable(&self) -> bool {
        false
    }
}

/// A kicker, the category over a title: [`KICKER`] in `teal`, uppercase. A
/// label in the accessibility tree, named as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kicker(Line);

impl Kicker {
    /// A kicker reading `text`.
    pub fn new(text: impl Into<String>) -> Self {
        Self(Line::new(text.into()))
    }

    /// Its text, as written.
    pub fn text(&self) -> &str {
        &self.0.text
    }
}

impl Widget for Kicker {
    fn measure(&self, _available: Size, metrics: &dyn TextMetrics) -> Size {
        self.0.measure(0.0, &KICKER, metrics)
    }

    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        dimmed(cx, list, |list| {
            self.0.paint(cx, 0.0, KICKER, Token::Teal, list);
        });
    }
}

impl Control for Kicker {
    fn semantics(&self) -> Semantics {
        Semantics::new(Role::Label, self.0.text.clone())
    }

    fn focusable(&self) -> bool {
        false
    }
}
