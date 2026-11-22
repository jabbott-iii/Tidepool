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

//! The modal dialog: a modal overlay ([`overlay`](crate::overlay)) whose
//! root is a bracketed panel with a title, a few lines of text, its
//! owner's content, and its owner's buttons (ADR-0052, ADR-0053).

use std::any::Any;
use std::fmt;

use tidepool_core::geom::{Point, Rect, Size};
use tidepool_core::input::{InputEvent, Key, Modifiers, TextLength};

use crate::a11y::{Role, Semantics};
use crate::draw::{Corners, DrawList, PaintContext};
use crate::error::UiError;
use crate::input::{ActionKind, EventContext, Handled};
use crate::layout::{Align, Insets, Length, Style};
use crate::metrics::{TextMetrics, TextStyle};
use crate::overlay::{Anchor, OverlayKind};
use crate::theme::Token;
use crate::tree::{MAX_NODES, Node, NodeId, Tree, Widget};

use super::{
    BRACKET_ARM, BRACKET_THICKNESS, MAX_CELL_BYTES, MAX_MODAL_LINES, MODAL_BUTTON_GAP, MODAL_GAP,
    MODAL_MARGIN, MODAL_PADDING, MODAL_TEXT, MODAL_WIDTH, SECTION_GAP, SECTION_RULE, SECTION_TITLE,
    baseline, cut_to, dimmed, focus_outline, line_height,
};

/// How much a laid-out length may fall short of what is asked of it and
/// still count as it, for the sums of `f32`s layout makes.
const SLACK: f32 = 0.01;

/// A modal dialog, which asks its user something and must be answered
/// before the rest of the window can be used again
/// ([`overlay`](crate::overlay#kinds)): a builder of its title and text,
/// which [`open`](Self::open) opens as a modal overlay centered in the
/// window. Opening it is the only way to put one in a tree.
///
/// # What it holds
///
/// Its root, the dialog, holds three things, one above another:
///
/// - its title, in the look of a [`SectionTitle`](super::SectionTitle),
///   and at most [`MAX_MODAL_LINES`] lines of text in [`MODAL_TEXT`],
///   both set when it is made;
/// - its owner's content (a field, a list), in its body, which
///   [`body`](Self::body) makes the first time it is asked for;
/// - its owner's buttons, in a row at its bottom, lined up at the right
///   ([`actions`](Self::actions)).
///
/// Its title is painted uppercase, as chrome is; its text as written, a
/// line each. Each is one line, cut to the dialog's width.
///
/// # Its text, read and cleaned
///
/// It reads its owner's text only as far as it keeps it, so a huge string
/// costs no more than a short one:
///
/// - at most [`MAX_CELL_BYTES`] of its title, cut at an extended grapheme
///   cluster boundary;
/// - its text's lines, which end at a line feed, a carriage return and
///   line feed, or a lone carriage return (ADR-0019), at most
///   [`MAX_MODAL_LINES`] of them. A line longer than [`MAX_CELL_BYTES`] is
///   cut there, at a cluster boundary, and is the last it reads, as
///   finding where it ends would mean reading all of it.
///
/// It then cleans what it keeps, as a [`TextInput`](super::TextInput)
/// cleans typed text: tabs and line breaks (the line and paragraph
/// separators, U+2028 and U+2029, among them) become spaces, and other
/// control characters are left out. So are the bidirectional formatting
/// characters (U+202A to U+202E, U+2066 to U+2069, U+200E, U+200F and
/// U+061C), which could make the title read other than it is, such as
/// a file name that seems to end in `.jpg`, and the invisible format
/// characters, such as the zero width space (U+200B), the soft hyphen
/// (U+00AD) and the tag characters (U+E0000 to U+E007F), which would
/// make two texts that look alike differ. The zero width joiner and
/// non-joiner (U+200D, U+200C) stay: emoji and some scripts need them.
///
/// # Focus and keys
///
/// Focus lands on the dialog itself when it opens (ADR-0053), so a screen
/// reader announces its role and title, and Enter accepts nothing. It is
/// not one of the tab stops it keeps Tab among: Tab moves to its first
/// control, Shift+Tab to its last. A consent prompt (an install, ADR-0002)
/// or a destructive one (discard changes) leaves focus on the dialog, or
/// focuses its least destructive control, never the one that accepts; an
/// owner that focuses a control does so after inserting it. It shows the
/// focus outline, 3 units outside its bounds, while it has focus and focus
/// shows: after a key, not after a click (`:focus-visible`), as any
/// control does, since keys go to it.
///
/// Escape, going down with no modifier but Shift, while focus is in it,
/// emits [`ActionKind::Dismiss`] from its root (a repeat of a held Escape
/// is taken, and emits nothing, so one press closes one dialog): the owner
/// then closes it with [`Tree::close_overlay`], which gives focus back to
/// where it was when it opened. A [`TextInput`](super::TextInput) in it
/// handles Escape itself, emitting its own `Dismiss`. It handles no other
/// event: keys and presses nothing in it takes come back
/// [`blocked`](crate::input::Response::blocked).
///
/// # A window too small for it
///
/// It is [`MODAL_WIDTH`] wide, or the window less [`MODAL_MARGIN`] a side
/// if that is less, and as tall as what it holds, or the window less its
/// margins if that is less. Then its title and text give up room first,
/// whole lines from the last; its content and buttons keep theirs as long
/// as there is room for them. When there is not, so that they would be
/// squeezed, it disables its body and its row, which it owns: no control
/// it holds takes focus or activates while it cannot be seen whole, focus
/// in them goes to the dialog, and it enables them again once there is
/// room. It keeps this apart from their own
/// [`enabled`](Tree::enabled) flags, which stay its owner's: a row its
/// owner disabled, such as an Accept to be enabled once a license has been
/// read, stays disabled when the window grows back. Their
/// [state](crate::tree::NodeState::disabled) and the accessibility tree
/// say they are disabled either way. Nothing it holds paints outside its
/// bounds.
///
/// # Its look
///
/// The site's popup panel (`.deploy__panel`, a `.panel` with
/// `.panel--brackets`) at the IDE's scale, [`MODAL_PADDING`] in: a 1-unit
/// `line` border and [`BRACKET_ARM`]-unit `teal` bracket corners at its
/// top left and bottom right; its title `text` after a
/// [`SECTION_RULE`]-wide `teal` rule, and its lines `text-2`. The site has
/// no modal style, so the rest is derived (the spec's Components table,
/// ADR-0053). The window under it is dimmed with `panel`, the site's
/// translucent panel color, so that what is inert reads as set back. Its
/// fill is `panel`'s solid value, opaque, which the theme gives for a
/// surface with no blurred backdrop behind it: the site's popup is 97%
/// opaque over a blur, and no draw command blurs (ADR-0049), so a
/// translucent fill would let the window's text show through it. The
/// site's shadow is left out for the same reason. A dialog over another
/// dims the window again. Disabled, it keeps its dimming and fill, and
/// paints the rest at [`DISABLED_OPACITY`](super::DISABLED_OPACITY).
///
/// # In the accessibility tree
///
/// A dialog, named by its title as cleaned, its value its lines joined by
/// line feeds (none if it has none), focusable, and marked modal
/// ([`AccessNode::modal`](crate::a11y::AccessNode)).
///
/// ```
/// use tidepool_core::geom::Size;
/// use tidepool_core::input::{InputEvent, Key, Modifiers};
/// use tidepool_ui::input::{Action, ActionKind};
/// use tidepool_ui::metrics::FixedMetrics;
/// use tidepool_ui::widgets::{Button, ButtonKind, Control, Modal};
/// use tidepool_ui::Tree;
///
/// let mut tree = Tree::new();
/// let save = tree.insert(tree.root(), Button::new("Save", ButtonKind::Ghost).node())?;
/// tree.set_focus(save, true)?;
///
/// let dialog = Modal::new("Discard changes?")
///     .text("main.rs has changes that are not saved.")
///     .open(&mut tree)?;
/// let actions = Modal::actions(&tree, dialog).expect("its row of buttons");
/// tree.insert(actions, Button::new("Discard", ButtonKind::Amber).node())?;
/// tree.insert(actions, Button::new("Cancel", ButtonKind::Ghost).node())?;
/// // Focus is on the dialog, so Enter discards nothing; the button behind
/// // it is inert.
/// assert_eq!(tree.focused(), Some(dialog));
/// assert!(tree.set_focus(save, true).is_err());
/// tree.layout(Size::new(800.0, 600.0), &FixedMetrics::DEFAULT);
///
/// let escape = InputEvent::KeyDown { key: Key::Escape, modifiers: Modifiers::NONE, repeat: false };
/// let response = tree.handle(&escape);
/// assert_eq!(response.actions, [Action { node: dialog, kind: ActionKind::Dismiss }]);
/// // The owner closes it, and focus goes back to the button.
/// assert_eq!(tree.close_overlay(dialog)?, Some(save));
/// assert_eq!(tree.focused(), Some(save));
/// # Ok::<(), tidepool_ui::UiError>(())
/// ```
///
/// It is not a widget, so it cannot be put in a node by hand, where it
/// would dim the window without making anything inert:
///
/// ```compile_fail
/// use tidepool_ui::layout::Style;
/// use tidepool_ui::widgets::Modal;
/// use tidepool_ui::Node;
///
/// let node = Node::new(Style::default()).with_widget(Modal::new("Allow?"));
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct Modal {
    title: String,
    lines: Vec<String>,
}

impl Modal {
    /// A dialog titled `title`, with no text, its title read and cleaned
    /// as the type's documentation says.
    pub fn new(title: impl AsRef<str>) -> Self {
        Self {
            title: clean(cut_to(title.as_ref(), MAX_CELL_BYTES)),
            lines: Vec::new(),
        }
    }

    /// The same, with `text` under its title, its lines read and cleaned
    /// as the type's documentation says.
    #[must_use]
    pub fn text(self, text: impl AsRef<str>) -> Self {
        Self {
            lines: kept_lines(text.as_ref(), MAX_MODAL_LINES),
            ..self
        }
    }

    /// Its title, as kept and cleaned.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Its lines of text, as kept and cleaned.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Opens it as a modal overlay centered in the window, at least
    /// [`MODAL_MARGIN`] from its edges, with focus on it, and returns its
    /// root, the dialog: the node its [`Dismiss`](ActionKind::Dismiss)
    /// comes from, which [`Tree::close_overlay`] closes.
    ///
    /// # Errors
    ///
    /// - [`UiError::Unnamed`] if its title is blank once cleaned: a dialog
    ///   takes focus, so it needs a name;
    /// - [`UiError::TooManyNodes`] if the tree has no room for the
    ///   dialog's three nodes;
    /// - the other errors of [`Tree::open_overlay`].
    ///
    /// Nothing changes then: each is found before the tree changes.
    pub fn open(self, tree: &mut Tree) -> Result<NodeId, UiError> {
        if tree.node_count().saturating_add(3) > MAX_NODES {
            return Err(UiError::TooManyNodes { max: MAX_NODES });
        }
        let mut semantics = Semantics::new(Role::Dialog, self.title.clone());
        if !self.lines.is_empty() {
            // At most `MAX_MODAL_LINES` lines of `MAX_CELL_BYTES`.
            semantics = semantics.with_value(self.lines.join("\n"));
        }
        let style = Style::column()
            .width(Length::Fixed(MODAL_WIDTH))
            .padding(Insets::all(MODAL_PADDING))
            .gap(MODAL_GAP)
            .clip();
        let root = Node::new(style)
            .with_semantics(semantics)
            .focusable()
            .with_widget(Dialog::default());
        let anchor = Anchor::window(Align::Center, Align::Center, MODAL_MARGIN);
        let dialog = tree.open_overlay(root, OverlayKind::Modal, anchor)?;
        let heading = Node::new(
            Style::default()
                .width(Length::Fill(1.0))
                .height(Length::Fill(1.0)),
        )
        .with_widget(Heading {
            shown: self.title.to_uppercase(),
            lines: self.lines,
        });
        let row = Node::new(
            Style::row()
                .width(Length::Fill(1.0))
                .gap(MODAL_BUTTON_GAP)
                .align(Align::End, Align::Center),
        )
        .with_widget(Actions);
        for node in [heading, row] {
            // Not reached: there is room for both, which are neither too
            // deep nor focusable.
            if let Err(error) = tree.insert(dialog, node) {
                let _ = tree.close_overlay(dialog);
                return Err(error);
            }
        }
        Ok(dialog)
    }

    /// The body of a dialog [`open`](Self::open) opened, between its text
    /// and its buttons: a column, with [`MODAL_GAP`] between what the owner
    /// inserts into it. It is made the first time it is asked for.
    ///
    /// # Errors
    ///
    /// [`UiError::UnknownNode`] if `dialog` is not in the tree,
    /// [`UiError::WrongWidget`] if it is not a dialog, and the errors of
    /// [`Tree::insert_at`] the first time.
    pub fn body(tree: &mut Tree, dialog: NodeId) -> Result<NodeId, UiError> {
        if !tree.contains(dialog) {
            return Err(UiError::UnknownNode { id: dialog });
        }
        if tree.widget::<Dialog>(dialog).is_none() {
            return Err(UiError::WrongWidget { id: dialog });
        }
        if let Some(body) = child::<Body>(tree, dialog) {
            return Ok(body);
        }
        let children = tree.children(dialog).unwrap_or_default();
        let at = children
            .iter()
            .position(|&node| tree.widget::<Actions>(node).is_some())
            .unwrap_or(children.len());
        let style = Style::column().width(Length::Fill(1.0)).gap(MODAL_GAP);
        let body = tree.insert_at(dialog, at, Node::new(style).with_widget(Body))?;
        // Made while the dialog is squeezed, it is squeezed at once.
        if tree
            .widget::<Dialog>(dialog)
            .is_some_and(|dialog| dialog.cut)
        {
            tree.squeeze(body, true);
        }
        Ok(body)
    }

    /// The row of buttons of a dialog [`open`](Self::open) opened, if
    /// `dialog` is one.
    pub fn actions(tree: &Tree, dialog: NodeId) -> Option<NodeId> {
        tree.widget::<Dialog>(dialog)?;
        child::<Actions>(tree, dialog)
    }
}

/// The child of `dialog` that holds a `W`, if any.
fn child<W: Widget>(tree: &Tree, dialog: NodeId) -> Option<NodeId> {
    tree.children(dialog)?
        .iter()
        .copied()
        .find(|&node| tree.widget::<W>(node).is_some())
}

/// Shows its title and lines as their lengths only (ADR-0043).
impl fmt::Debug for Modal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let lines: Vec<TextLength<'_>> = self.lines.iter().map(|line| TextLength(line)).collect();
        f.debug_struct("Modal")
            .field("title", &TextLength(&self.title))
            .field("lines", &lines)
            .finish()
    }
}

/// The lines of `text` a dialog keeps, at most `most` of them, as
/// [`Modal`]'s documentation says: it reads at most `most` times
/// [`MAX_CELL_BYTES`] and two bytes, and a cluster past the last. A
/// tooltip and a toast keep their text so too.
pub(super) fn kept_lines(text: &str, most: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut rest = text;
    while lines.len() < most && !rest.is_empty() {
        let bytes = rest.as_bytes();
        let window = bytes.get(..bytes.len().min(MAX_CELL_BYTES + 1));
        // Line ends are ASCII, so never inside a character.
        let end = window.and_then(|window| window.iter().position(|&b| b == b'\n' || b == b'\r'));
        let Some(at) = end else {
            // The last line, or one longer than the dialog keeps.
            lines.push(clean(cut_to(rest, MAX_CELL_BYTES)));
            break;
        };
        lines.push(clean(rest.get(..at).unwrap_or("")));
        let skip = if bytes.get(at..at + 2) == Some(b"\r\n") {
            2
        } else {
            1
        };
        rest = rest.get(at + skip..).unwrap_or("");
    }
    lines
}

/// `text` as a dialog shows it: tabs and line breaks, the line and
/// paragraph separators among them, made spaces; the other characters
/// [`is_hidden`] names left out.
pub(super) fn clean(text: &str) -> String {
    text.chars()
        .filter_map(|c| match c {
            '\t' | '\n' | '\r' | '\u{2028}' | '\u{2029}' => Some(' '),
            c if is_hidden(c) => None,
            c => Some(c),
        })
        .collect()
}

/// Returns `true` for a character that shows nothing, or changes how the
/// text around it reads, so that text from outside could look like other
/// text with it: a control character, an invisible one
/// ([`is_invisible`], the bidirectional formatting characters among
/// them), the line and paragraph separators (U+2028, U+2029), or the
/// interlinear annotation characters (U+FFF9 to U+FFFB), which hide text
/// in an annotation.
pub(super) const fn is_hidden(c: char) -> bool {
    c.is_control()
        || is_invisible(c)
        || matches!(c, '\u{2028}' | '\u{2029}' | '\u{FFF9}'..='\u{FFFB}')
}

/// Returns `true` for Unicode's default-ignorable code points
/// (`Default_Ignorable_Code_Point` in Unicode 17.0.0, which
/// `crates/text/ucd/` pins), which a renderer draws as nothing, but the
/// zero width non-joiner and joiner (U+200C, U+200D) and variation
/// selector 16 (U+FE0F), which emoji and some scripts need. Among them
/// are the bidirectional formatting characters (U+061C, U+200E, U+200F,
/// U+202A to U+202E, U+2066 to U+2069), the zero width space (U+200B),
/// the soft hyphen (U+00AD), the Hangul fillers, the other variation
/// selectors, and the tag characters (U+E0000 to U+E0FFF), with which
/// text could hide data or pass for other text. A unit test reads the
/// pinned data and checks this set against it.
const fn is_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{034F}'
            | '\u{061C}'
            | '\u{115F}'..='\u{1160}'
            | '\u{17B4}'..='\u{17B5}'
            | '\u{180B}'..='\u{180F}'
            | '\u{200B}'
            | '\u{200E}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{3164}'
            | '\u{FE00}'..='\u{FE0E}'
            | '\u{FEFF}'
            | '\u{FFA0}'
            | '\u{FFF0}'..='\u{FFF8}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0000}'..='\u{E0FFF}'
    )
}

/// The dialog, the root of a [`Modal`]'s overlay.
#[derive(Debug, Default)]
pub(super) struct Dialog {
    /// Whether its body and row are disabled for want of room, as the last
    /// layout found.
    cut: bool,
}

impl Widget for Dialog {
    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let bounds = cx.bounds();
        let theme = cx.theme();
        // The dimming of the rest of the window, which its clip is, and its
        // opaque fill, neither dimmed when it is disabled, so that nothing
        // shows through it.
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
        if cx.state().focus_visible {
            focus_outline(list, bounds, theme);
        }
    }

    fn event(&mut self, cx: &mut EventContext<'_>, event: &InputEvent) -> Handled {
        match *event {
            InputEvent::KeyDown {
                key: Key::Escape,
                modifiers,
                repeat,
            } if modifiers.difference(Modifiers::SHIFT).is_empty() => {
                if !repeat {
                    cx.emit(ActionKind::Dismiss);
                }
                Handled::Yes
            }
            _ => Handled::No,
        }
    }
}

/// A dialog's title and lines, which give up room first: it fills what
/// its content and buttons leave, and paints the title and the whole lines
/// that fit.
struct Heading {
    /// The title, uppercase.
    shown: String,
    lines: Vec<String>,
}

impl Heading {
    /// The heights of its title's line and of each line of text.
    fn heights(metrics: &dyn TextMetrics) -> (f32, f32) {
        (
            line_height(metrics, &SECTION_TITLE),
            line_height(metrics, &MODAL_TEXT),
        )
    }
}

impl Widget for Heading {
    /// As wide as its title, after its rule, and its widest line; as tall
    /// as its title and lines.
    fn measure(&self, _available: Size, metrics: &dyn TextMetrics) -> Size {
        let (title, line) = Self::heights(metrics);
        let width = self
            .lines
            .iter()
            .map(|text| metrics.width(text, &MODAL_TEXT))
            .fold(
                SECTION_RULE + SECTION_GAP + metrics.width(&self.shown, &SECTION_TITLE),
                f32::max,
            );
        let height = if self.lines.is_empty() {
            title
        } else {
            title + MODAL_GAP + self.lines.len() as f32 * line
        };
        Size::new(width, height)
    }

    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let bounds = cx.bounds();
        let theme = cx.theme();
        let (title, line) = Self::heights(cx.metrics());
        let bottom = bounds.y + bounds.height + SLACK;
        dimmed(cx, list, |list| {
            list.push_clip(bounds);
            let rule = Rect::new(bounds.x, bounds.y, SECTION_RULE, title);
            list.rect(rule, theme.color(Token::Teal));
            let lead = SECTION_RULE + SECTION_GAP;
            let rect = Rect::new(bounds.x + lead, bounds.y, bounds.width - lead, title);
            text_line(cx, list, &self.shown, SECTION_TITLE, Token::Text, rect);
            let mut y = bounds.y + title + MODAL_GAP;
            for text in &self.lines {
                if y + line > bottom {
                    break;
                }
                let rect = Rect::new(bounds.x, y, bounds.width, line);
                text_line(cx, list, text, MODAL_TEXT, Token::Text2, rect);
                y += line;
            }
            list.pop_clip();
        });
    }
}

/// Paints one line of a dialog's text in `rect`, cut to it.
fn text_line(
    cx: &PaintContext<'_>,
    list: &mut DrawList,
    text: &str,
    style: TextStyle,
    token: Token,
    rect: Rect,
) {
    if text.is_empty() || rect.width <= 0.0 {
        return;
    }
    list.push_clip(rect);
    list.text(
        Point::new(rect.x, baseline(rect, cx.metrics(), &style)),
        text,
        style,
        cx.theme().color(token),
    );
    list.pop_clip();
}

/// A dialog's body, which marks it for [`Modal::body`].
struct Body;

impl Widget for Body {}

/// The row of a dialog's buttons, which marks it for [`Modal::actions`].
struct Actions;

impl Widget for Actions {}

impl Tree {
    /// After a layout pass, disables the body and row of the dialog at `id`
    /// if either got less height than it needs, and enables them again
    /// once both have it, apart from their own flags, as [`Modal`]'s
    /// documentation says. Changes no layout, so it never asks for another
    /// pass.
    pub(crate) fn bind_dialog(&mut self, id: NodeId, metrics: &dyn TextMetrics) -> bool {
        let parts: Vec<NodeId> = [child::<Body>(self, id), child::<Actions>(self, id)]
            .into_iter()
            .flatten()
            .collect();
        let mut cut = false;
        for &part in &parts {
            let Some(bounds) = self.bounds(part) else {
                continue;
            };
            if self.style(part).is_some_and(|style| style.hidden) {
                continue;
            }
            let needs = self.natural(part, Size::new(bounds.width, f32::INFINITY), metrics);
            cut |= bounds.height + SLACK < needs.height;
        }
        if let Some(dialog) = self
            .get_mut(id)
            .and_then(|node| node.widget.as_deref_mut())
            .and_then(|widget| (widget as &mut dyn Any).downcast_mut::<Dialog>())
        {
            dialog.cut = cut;
        }
        // Every part, each time: a body made since is squeezed too.
        for part in parts {
            self.squeeze(part, cut);
        }
        false
    }

    /// Disables the node for want of room, or enables it again, apart from
    /// its own [`enabled`](Tree::enabled) flag, which its owner keeps: a
    /// part its owner disabled stays disabled when the window grows back.
    fn squeeze(&mut self, id: NodeId, squeezed: bool) {
        let Some(node) = self.get_mut(id) else {
            return;
        };
        if node.squeezed == squeezed {
            return;
        }
        node.squeezed = squeezed;
        for inside in self.subtree(id) {
            self.mark(inside);
        }
        if squeezed {
            self.leave_disabled(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    fn it_reads_only_the_lines_it_keeps() {
        let modal = Modal::new("t").text("one\r\ntwo\rthree\nfour\nfive");
        assert_eq!(modal.lines(), ["one", "two", "three", "four"]);
        // A line ending at the text's end starts no other.
        assert_eq!(Modal::new("t").text("a\n").lines(), ["a"]);
        assert_eq!(Modal::new("t").text("").lines().len(), 0);
        // A line of exactly `MAX_CELL_BYTES` is whole, and the next is read.
        let full = "x".repeat(MAX_CELL_BYTES);
        let modal = Modal::new("t").text(format!("{full}\nnext"));
        assert_eq!(modal.lines(), [full.as_str(), "next"]);
        // A longer one is cut, at a cluster boundary, and is the last.
        let long = "é".repeat(MAX_CELL_BYTES);
        let modal = Modal::new(long.as_str()).text(format!("{long}\nnext"));
        assert_eq!(modal.title().len(), MAX_CELL_BYTES);
        assert_eq!(modal.lines().len(), 1);
        assert_eq!(modal.lines()[0].len(), MAX_CELL_BYTES);
        let accents = format!("{}e\u{301}\u{301}", "x".repeat(MAX_CELL_BYTES - 2));
        assert_eq!(
            Modal::new("t").text(&accents).lines()[0].len(),
            MAX_CELL_BYTES - 2
        );
    }

    #[test]
    fn a_huge_text_costs_what_a_short_one_does() {
        // 32 MiB in one line, as a title and as text, a hundred times: read
        // whole, each took tens of milliseconds; read as far as it is kept,
        // microseconds.
        let huge = "a".repeat(32 << 20);
        let started = Instant::now();
        for _ in 0..100 {
            let modal = Modal::new(huge.as_str()).text(huge.as_str());
            assert_eq!(modal.title().len(), MAX_CELL_BYTES);
            assert_eq!(modal.lines()[0].len(), MAX_CELL_BYTES);
        }
        let took = started.elapsed();
        assert!(took < Duration::from_millis(500), "{took:?}");
    }

    #[test]
    fn it_cleans_control_and_direction_characters() {
        let modal = Modal::new("Allow \u{202E}exe.gpj\u{202C}?\u{0}\u{1b}\tnow\nok")
            .text("run \u{1b}[31mthis\u{0}\tnow\u{85}next\u{2066}\u{200F}\u{061C}\u{2028}end");
        assert_eq!(modal.title(), "Allow exe.gpj? now ok");
        assert_eq!(modal.lines(), ["run [31mthis nownext end"]);
        for c in [
            '\u{202A}', '\u{202E}', '\u{2066}', '\u{2069}', '\u{200E}', '\u{200F}', '\u{061C}',
        ] {
            assert!(is_hidden(c), "{c:?}");
        }
        assert!(!is_hidden('\u{200D}'), "a joiner is not one");
    }

    #[test]
    fn the_invisible_characters_are_unicodes_default_ignorables_but_three() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../text/ucd/17.0.0/DerivedCoreProperties.txt"
        );
        let data = std::fs::read_to_string(path).expect("the pinned Unicode data");
        let mut ignorable = vec![false; 0x11_0000];
        for line in data.lines() {
            let line = line.split('#').next().unwrap_or("");
            let mut fields = line.split(';').map(str::trim);
            let (Some(range), Some("Default_Ignorable_Code_Point")) =
                (fields.next(), fields.next())
            else {
                continue;
            };
            let (from, to) = range.split_once("..").unwrap_or((range, range));
            let from = u32::from_str_radix(from, 16).expect("a code point");
            let to = u32::from_str_radix(to, 16).expect("a code point");
            for code in from..=to {
                ignorable[code as usize] = true;
            }
        }
        let kept = ['\u{200C}', '\u{200D}', '\u{FE0F}'];
        let mut count = 0;
        for code in 0..0x11_0000u32 {
            let Some(c) = char::from_u32(code) else {
                continue;
            };
            let expected = ignorable[code as usize] && !kept.contains(&c);
            assert_eq!(is_invisible(c), expected, "U+{code:04X}");
            count += usize::from(expected);
        }
        // Unicode 17.0.0 has 4,174, none of them surrogates.
        assert_eq!(count, 4_174 - kept.len());
    }

    #[test]
    fn it_leaves_out_the_invisible_format_characters_but_the_joiners() {
        let hidden = [
            '\u{200B}',
            '\u{2060}',
            '\u{2064}',
            '\u{206A}',
            '\u{206F}',
            '\u{FEFF}',
            '\u{00AD}',
            '\u{180E}',
            '\u{034F}',
            '\u{FFF9}',
            '\u{FFFB}',
            '\u{E0000}',
            '\u{E0001}',
            '\u{E0049}',
            '\u{E007F}',
        ];
        let text: String = hidden.iter().fold("rust".to_string(), |mut text, &c| {
            text.push(c);
            text
        }) + "-analyzer";
        let modal = Modal::new(&text).text(&text);
        assert_eq!(modal.title(), "rust-analyzer");
        assert_eq!(modal.lines(), ["rust-analyzer"]);
        // The joiners stay, as emoji and some scripts need them.
        let family = "\u{1F469}\u{200D}\u{1F467} \u{0928}\u{094D}\u{200C}\u{0915}";
        assert_eq!(Modal::new(family).title(), family);
        for c in hidden {
            assert!(is_hidden(c), "{c:?}");
        }
        for c in ['\u{200C}', '\u{200D}', '\u{FE0F}', 'a', ' '] {
            assert!(!is_hidden(c), "{c:?}");
        }
        // Fillers and the other variation selectors, with which a name
        // could carry what it does not show.
        for c in [
            '\u{115F}',
            '\u{3164}',
            '\u{FFA0}',
            '\u{FE00}',
            '\u{FE0E}',
            '\u{E0100}',
        ] {
            assert!(is_hidden(c), "{c:?}");
        }
        assert_eq!(Modal::new("a\u{3164}\u{FE00}\u{E0100}b").title(), "ab");
    }
}
