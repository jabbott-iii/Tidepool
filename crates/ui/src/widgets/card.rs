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

//! The card with its tab (`.card`, `.card__tab`): a toolchain's, a
//! device's or an agent's box, with a pill label on its right edge
//! (ADR-0052).

use std::fmt;

use tidepool_core::geom::{Point, Rect, Size};
use tidepool_core::input::TextLength;

use crate::a11y::{Role, Semantics};
use crate::draw::{DrawList, PaintContext};
use crate::layout::{Insets, Length, Style};
use crate::metrics::{TextMetrics, TextStyle};
use crate::theme::Token;
use crate::tree::Widget;

use super::{
    CARD_GAP, CARD_META, CARD_PADDING, CARD_TAB_HEIGHT, CARD_TAB_LABEL, CARD_TAB_PADDING_LEFT,
    CARD_TAB_PADDING_RIGHT, CARD_TAB_STRIPE, CARD_TAB_TOP, CARD_TEXT, CARD_TITLE, Control,
    MAX_CELL_BYTES, baseline, cut_to, dimmed, grow, line_height,
};

/// How far its lines of text reach down from its top: the padding, the
/// kicker's line, the title's and the text's, the gaps between them and
/// the gap below.
const HEADING: f32 =
    CARD_PADDING + 11.0 * 1.5 + CARD_GAP + 16.0 * 1.3 + CARD_GAP + 13.0 * 1.5 + CARD_GAP;

/// A card (`.card`, ADR-0052): a box for a toolchain, a device or an
/// agent, with three lines of text at its top: a kicker in
/// [`CARD_META`] (`TOOLCHAIN`), its title in [`CARD_TITLE`] and a line of
/// text in [`CARD_TEXT`]; and, on its right edge, a tab (`.card__tab`), a
/// pill label of its state (`READY`, `UPDATE`).
///
/// Its node is a column, and what the owner inserts into it, its buttons,
/// sits below its lines of text: its style's top padding leaves room for
/// all three, whether it has them or not, so that a change of its text
/// never moves what it holds. It takes no focus itself; its buttons do.
/// It is a group in the accessibility tree, named by its title, its value
/// the rest of what it shows, as written, in the order it reads: its
/// kicker, its line of text and its tab's label, those it has, each on a
/// line of its own; none if it has none.
///
/// Its kicker and its tab's label are chrome, painted uppercase; its
/// title and text are as written, each one line cut to the card, and at
/// most [`MAX_CELL_BYTES`] kept of each.
///
/// It is a `ghost` box with a 1-unit `line` border; hovered, as the
/// site's `.card:hover`, its border is `teal-line`, with a 1-unit
/// `teal-fill` ring around it and its title `teal-hi`. Its kicker is
/// `text-3`, its title `text` and its text `text-2`. Its tab is a
/// `card-tab` pill, rounded on the left, with an `on-card-tab` label and a
/// [`CARD_TAB_STRIPE`]-wide `teal` stripe on its right edge. Disabled, it
/// has its rest look at [`DISABLED_OPACITY`](super::DISABLED_OPACITY). The
/// site's media well, image and soft shadow are left out: a card here
/// holds text and controls.
///
/// ```
/// use tidepool_ui::a11y::Role;
/// use tidepool_ui::widgets::{Card, Control};
///
/// let card = Card::new("rust-analyzer").kicker("Toolchain").text("2026-11-17, up to date").tab("Ready");
/// let semantics = card.semantics();
/// assert_eq!((semantics.role, semantics.name.as_str()), (Role::Group, "rust-analyzer"));
/// assert_eq!(semantics.value.as_deref(), Some("Toolchain\n2026-11-17, up to date\nReady"));
/// assert!(!card.focusable());
/// assert_eq!(Card::new("adb").semantics().value, None);
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct Card {
    kicker: String,
    title: String,
    text: String,
    tab: String,
}

impl Card {
    /// A card titled `title`, with no kicker, text or tab.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            kicker: String::new(),
            title: kept(title.into()),
            text: String::new(),
            tab: String::new(),
        }
    }

    /// The same, with `kicker` over its title.
    #[must_use]
    pub fn kicker(self, kicker: impl Into<String>) -> Self {
        Self {
            kicker: kept(kicker.into()),
            ..self
        }
    }

    /// The same, with `text` under its title.
    #[must_use]
    pub fn text(self, text: impl Into<String>) -> Self {
        Self {
            text: kept(text.into()),
            ..self
        }
    }

    /// The same, with a tab reading `label`.
    #[must_use]
    pub fn tab(self, label: impl Into<String>) -> Self {
        Self {
            tab: kept(label.into()),
            ..self
        }
    }

    /// Its title, as written.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Its kicker, as written.
    pub fn kicker_text(&self) -> &str {
        &self.kicker
    }

    /// Its line of text, as written.
    pub fn line(&self) -> &str {
        &self.text
    }

    /// Its tab's label, as written; empty when it has no tab.
    pub fn tab_label(&self) -> &str {
        &self.tab
    }

    /// Gives it the tab reading `label`, or none if `label` is empty.
    pub fn set_tab(&mut self, label: impl Into<String>) {
        self.tab = kept(label.into());
    }

    /// Titles it `title`.
    pub fn set_title(&mut self, title: impl Into<String>) {
        self.title = kept(title.into());
    }

    /// Gives it the line of text `text`.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = kept(text.into());
    }

    /// Its tab's bounds in a card at `bounds`.
    fn tab_rect(&self, bounds: Rect, metrics: &dyn TextMetrics) -> Option<Rect> {
        if self.tab.is_empty() {
            return None;
        }
        let label = metrics.width(&self.tab.to_uppercase(), &CARD_TAB_LABEL);
        let width = CARD_TAB_PADDING_LEFT + label + CARD_TAB_PADDING_RIGHT;
        Some(Rect::new(
            bounds.x + bounds.width - width,
            bounds.y + CARD_TAB_TOP,
            width,
            CARD_TAB_HEIGHT,
        ))
    }
}

/// `text` with at most [`MAX_CELL_BYTES`] kept.
fn kept(text: String) -> String {
    if text.len() <= MAX_CELL_BYTES {
        text
    } else {
        cut_to(&text, MAX_CELL_BYTES).to_string()
    }
}

/// Shows its texts as their lengths only (ADR-0043).
impl fmt::Debug for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Card")
            .field("kicker", &TextLength(&self.kicker))
            .field("title", &TextLength(&self.title))
            .field("text", &TextLength(&self.text))
            .field("tab", &TextLength(&self.tab))
            .finish()
    }
}

/// Paints one line of a card's text, cut to `clip`.
fn line(
    cx: &PaintContext<'_>,
    list: &mut DrawList,
    text: &str,
    style: TextStyle,
    token: Token,
    rect: Rect,
    clip: Rect,
) {
    if text.is_empty() {
        return;
    }
    list.push_clip(clip);
    list.text(
        Point::new(rect.x, baseline(rect, cx.metrics(), &style)),
        text,
        style,
        cx.theme().color(token),
    );
    list.pop_clip();
}

impl Widget for Card {
    /// As wide as its widest line and its tab, and no taller than its
    /// lines' room, which its padding holds.
    fn measure(&self, _available: Size, metrics: &dyn TextMetrics) -> Size {
        let tab = self
            .tab_rect(Rect::ZERO, metrics)
            .map_or(0.0, |rect| rect.width + CARD_GAP);
        let width = [
            metrics.width(&self.kicker.to_uppercase(), &CARD_META) + tab,
            metrics.width(&self.title, &CARD_TITLE),
            metrics.width(&self.text, &CARD_TEXT),
        ]
        .into_iter()
        .fold(0.0, f32::max);
        Size::new(width, 0.0)
    }

    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let bounds = cx.bounds();
        let theme = cx.theme();
        let metrics = cx.metrics();
        let state = cx.state();
        let hovered = state.hovered && !state.disabled;
        dimmed(cx, list, |list| {
            if hovered {
                list.border(grow(bounds, 1.0), 1.0, theme.color(Token::TealFill));
            }
            list.rect(bounds, theme.color(Token::Ghost));
            let border = if hovered {
                Token::TealLine
            } else {
                Token::Line
            };
            list.border(bounds, 1.0, theme.color(border));

            let x = bounds.x + CARD_PADDING;
            let width = (bounds.width - 2.0 * CARD_PADDING).max(0.0);
            let tab = self.tab_rect(bounds, metrics);
            let tab_width = tab.map_or(0.0, |rect| rect.width + CARD_GAP);
            let mut y = bounds.y + CARD_PADDING;
            let meta = Rect::new(x, y, width, line_height(metrics, &CARD_META));
            let clip = Rect::new(x, y, (width - tab_width).max(0.0), meta.height);
            line(
                cx,
                list,
                &self.kicker.to_uppercase(),
                CARD_META,
                Token::Text3,
                meta,
                clip,
            );
            y += meta.height + CARD_GAP;
            let title = Rect::new(x, y, width, line_height(metrics, &CARD_TITLE));
            let token = if hovered { Token::TealHi } else { Token::Text };
            line(cx, list, &self.title, CARD_TITLE, token, title, title);
            y += title.height + CARD_GAP;
            let text = Rect::new(x, y, width, line_height(metrics, &CARD_TEXT));
            line(cx, list, &self.text, CARD_TEXT, Token::Text2, text, text);

            if let Some(rect) = tab {
                // A pill rounded on the left, square on the right.
                list.pill(rect, theme.color(Token::CardTab));
                let half = rect.height / 2.0;
                list.rect(
                    Rect::new(rect.x + half, rect.y, rect.width - half, rect.height),
                    theme.color(Token::CardTab),
                );
                list.rect(
                    Rect::new(
                        rect.x + rect.width - CARD_TAB_STRIPE,
                        rect.y,
                        CARD_TAB_STRIPE,
                        rect.height,
                    ),
                    theme.color(Token::Teal),
                );
                list.text(
                    Point::new(
                        rect.x + CARD_TAB_PADDING_LEFT,
                        baseline(rect, metrics, &CARD_TAB_LABEL),
                    ),
                    self.tab.to_uppercase(),
                    CARD_TAB_LABEL,
                    theme.color(Token::OnCardTab),
                );
            }
        });
    }
}

impl Control for Card {
    fn semantics(&self) -> Semantics {
        let semantics = Semantics::new(Role::Group, self.title.clone());
        // Each at most `MAX_CELL_BYTES`, so the value is bounded too.
        let shown: Vec<&str> = [&self.kicker, &self.text, &self.tab]
            .into_iter()
            .map(String::as_str)
            .filter(|part| !part.is_empty())
            .collect();
        if shown.is_empty() {
            semantics
        } else {
            semantics.with_value(shown.join("\n"))
        }
    }

    /// A column as wide as its parent gives it, its top padding leaving
    /// room for its three lines of text, with [`CARD_GAP`] between what
    /// it holds.
    fn style(&self) -> Style {
        Style::column()
            .width(Length::Fill(1.0))
            .padding(Insets::new(
                HEADING,
                CARD_PADDING,
                CARD_PADDING,
                CARD_PADDING,
            ))
            .gap(CARD_GAP)
    }

    fn focusable(&self) -> bool {
        false
    }
}
