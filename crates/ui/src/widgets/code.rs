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

//! The code block (`.highlighter-rouge`, `.code-bar`, `.code-copy`): code
//! in the Markdown preview and hover docs, with a bar holding its language
//! and COPY (ADR-0052).

use std::fmt;

use tidepool_core::geom::{Point, Rect, Size};
use tidepool_core::input::{InputEvent, TextLength};

use crate::a11y::{Role, Semantics};
use crate::draw::{DrawList, PaintContext};
use crate::error::UiError;
use crate::input::{EventContext, Handled};
use crate::layout::{Align, Insets, Length, Style};
use crate::metrics::TextMetrics;
use crate::theme::{Theme, Token};
use crate::tree::{NodeId, Tree, Widget};

use super::button::Activator;
use super::{
    CODE_BAR_HEIGHT, CODE_BAR_TEXT, CODE_PADDING, CODE_PADDING_LEFT, CODE_RULE, CODE_TEXT,
    COPY_HEIGHT, COPY_LABEL, COPY_PADDING, Control, Look, MAX_CODE_LINE_BYTES, MAX_CODE_LINES,
    MAX_CODE_SPANS, MAX_LANGUAGE_BYTES, baseline, cut_to, dimmed, focus_outline, line_height,
};

/// How many spaces a tab in code becomes.
const TAB_SPACES: &str = "    ";

/// The space right of COPY, inside the bar.
const COPY_RIGHT: f32 = 8.0;

/// What COPY says.
const COPY: &str = "COPY";

/// A run of code in one color: its text and the theme's token it is
/// painted in. Its `Debug` shows the text's length only (ADR-0043).
///
/// ```
/// use tidepool_ui::theme::Token;
/// use tidepool_ui::widgets::CodeSpan;
///
/// let keyword = CodeSpan::new("fn", Token::Teal);
/// assert_eq!((keyword.text(), keyword.token()), ("fn", Token::Teal));
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct CodeSpan {
    text: String,
    token: Token,
}

impl CodeSpan {
    /// `text` in `token`'s color.
    pub fn new(text: impl Into<String>, token: Token) -> Self {
        Self {
            text: text.into(),
            token,
        }
    }

    /// Its text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The token it is painted in.
    pub const fn token(&self) -> Token {
        self.token
    }
}

/// Shows the text as its length only.
impl fmt::Debug for CodeSpan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CodeSpan")
            .field("text", &TextLength(&self.text))
            .field("token", &self.token)
            .finish()
    }
}

/// A code block (ADR-0052): code in JetBrains Mono ([`CODE_TEXT`]) on
/// `editor-bg`, in a 1-unit `line-2` border with a [`CODE_RULE`]-wide
/// `teal` left border, under a bar (`.code-bar`) of `code-bar` with a
/// `line` rule below it, holding its language, uppercase in
/// [`CODE_BAR_TEXT`] and `text-3`, and COPY.
///
/// Its code is its owner's lines of [`CodeSpan`]s, each painted in the
/// color of its token, as written. It adds no tokens of its own: the
/// editor's syntax colors (the spec's "Editor colors") come with the
/// editor's highlighting, and a block's owner colors its spans with the
/// theme's tokens that read on `editor-bg` (`text`, `text-2`, `text-3`,
/// `teal`, `amber`, `error`, `comment`).
///
/// It keeps and paints at most [`MAX_CODE_LINES`] lines, each of at most
/// [`MAX_CODE_SPANS`] spans and [`MAX_CODE_LINE_BYTES`] bytes, so that
/// what painting it costs is bounded however long the code is. Making it
/// reads no more of a span than it keeps, but for a run of control
/// characters, which it reads to its end as it keeps none of it, and
/// [`plain`](Self::plain) reads each line it keeps to its end, to find
/// the next. A tab is four spaces, and other control characters are left out;
/// a line too long is cut at a grapheme cluster boundary of the whole
/// line, and what is kept of it is always its start: the spans before
/// the cut, and what fits of the one it falls in.
/// [`is_cut`](Self::is_cut) says whether anything was cut. Lines wider
/// than the block are cut at its right edge, and lines below its bottom,
/// if its owner gives it less height than it measures, are not painted.
/// Its language is cleaned so too, and cut to [`MAX_LANGUAGE_BYTES`].
///
/// [`insert`](Self::insert) adds it with its COPY button, which takes
/// focus and emits [`ActionKind::Activate`](crate::input::ActionKind::Activate)
/// of its own node when it is clicked, or Enter goes down or Space comes
/// up on it, as a button does; the owner finds the button with
/// [`copy_button`](Self::copy_button), and copies the code, as the
/// clipboard is the frontend's. It copies its own source text, not
/// [`text`](Self::text): that is what the block keeps and shows, cleaned
/// and cut, and its line ends are line feeds. COPY is `teal` in a
/// `teal-glow` border on the bar (`.code-copy`), on `teal-wash` with a
/// `teal-hi` label when hovered and on `teal-fill` when pressed, with the
/// focus outline when focus shows. The block itself has no hover look.
///
/// The block takes no focus itself. It is a group in the accessibility
/// tree, named by its language (`"code"` if it has none), its value the
/// code it keeps. Disabled, it has its rest look at
/// [`DISABLED_OPACITY`](super::DISABLED_OPACITY).
///
/// ```
/// use tidepool_ui::theme::Token;
/// use tidepool_ui::widgets::{CodeBlock, CodeSpan};
/// use tidepool_ui::Tree;
///
/// let line = [
///     CodeSpan::new("fn", Token::Teal),
///     CodeSpan::new(" main() {}", Token::Text),
/// ];
/// let mut tree = Tree::new();
/// let root = tree.root();
/// let block = CodeBlock::new("rust", [line]).insert(&mut tree, root)?;
/// assert_eq!(
///     tree.accessibility().to_string(),
///     "group \"rust\" = \"fn main() {}\"\n  button \"Copy\" focusable\n"
/// );
/// assert!(CodeBlock::copy_button(&tree, block).is_some());
/// # Ok::<(), tidepool_ui::UiError>(())
/// ```
///
/// COPY's owner copies the source it made the block from:
///
/// ```
/// use tidepool_core::geom::Size;
/// use tidepool_core::input::{InputEvent, Modifiers, MouseButton};
/// use tidepool_ui::input::ActionKind;
/// use tidepool_ui::metrics::FixedMetrics;
/// use tidepool_ui::widgets::CodeBlock;
/// use tidepool_ui::Tree;
///
/// let source = "fn main() {\r\n\tprintln!(\"hi\");\r\n}\r\n";
/// let mut tree = Tree::new();
/// let root = tree.root();
/// let block = CodeBlock::plain("rust", source).insert(&mut tree, root)?;
/// tree.frame(Size::new(400.0, 200.0), &FixedMetrics::DEFAULT);
///
/// // What it shows is cleaned: its tab is four spaces, its lines end in LF.
/// let shown = tree.widget::<CodeBlock>(block).map(CodeBlock::text);
/// assert_eq!(shown.as_deref(), Some("fn main() {\n    println!(\"hi\");\n}"));
///
/// // A click on COPY: the owner puts its source on the clipboard.
/// let copy = CodeBlock::copy_button(&tree, block).expect("COPY");
/// let at = tree.bounds(copy).expect("bounds");
/// let position = tidepool_core::geom::Point::new(at.x + 2.0, at.y + 2.0);
/// let modifiers = Modifiers::NONE;
/// tree.handle(&InputEvent::MouseDown { button: MouseButton::Left, position, modifiers });
/// let response = tree.handle(&InputEvent::MouseUp { button: MouseButton::Left, position, modifiers });
/// let copied = response
///     .actions
///     .iter()
///     .find(|action| action.node == copy && action.kind == ActionKind::Activate)
///     .map(|_| source);
/// assert_eq!(copied, Some(source));
/// # Ok::<(), tidepool_ui::UiError>(())
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct CodeBlock {
    language: String,
    lines: Vec<Vec<CodeSpan>>,
    cut: bool,
}

impl CodeBlock {
    /// A block of code in `language` (`rust`, as a fence names it), of
    /// `lines` of spans, cut as the type's documentation says.
    pub fn new<L, S>(language: impl Into<String>, lines: L) -> Self
    where
        L: IntoIterator<Item = S>,
        S: IntoIterator<Item = CodeSpan>,
    {
        let mut block = Self::empty(&language.into());
        let mut buffer = Line::buffer();
        for spans in lines {
            if block.lines.len() == MAX_CODE_LINES {
                block.cut = true;
                break;
            }
            let mut line = Line::new(&mut buffer);
            for span in spans {
                if !line.take(&span.text, span.token) {
                    break;
                }
            }
            block.keep(line);
        }
        block
    }

    /// A block of `text` in `language`, all in `text`, a line for each of
    /// its lines, which end at a line feed, a carriage return and line
    /// feed, or a lone carriage return (ADR-0019).
    pub fn plain(language: impl Into<String>, text: &str) -> Self {
        let mut block = Self::empty(&language.into());
        let mut buffer = Line::buffer();
        for text in lines_of(text) {
            if block.lines.len() == MAX_CODE_LINES {
                block.cut = true;
                break;
            }
            let mut line = Line::new(&mut buffer);
            line.take(text, Token::Text);
            block.keep(line);
        }
        block
    }

    /// A block in `language`, cleaned and cut, with no lines yet.
    fn empty(language: &str) -> Self {
        let mut buffer = String::with_capacity(MAX_LANGUAGE_BYTES + TAB_SPACES.len());
        clean_into(&mut buffer, language, MAX_LANGUAGE_BYTES);
        Self {
            language: cut_to(&buffer, MAX_LANGUAGE_BYTES).to_string(),
            lines: Vec::new(),
            cut: false,
        }
    }

    /// Keeps `line`, the next.
    fn keep(&mut self, line: Line<'_>) {
        let (spans, cut) = line.finish();
        self.lines.push(spans);
        self.cut |= cut;
    }

    /// Its language, cleaned and cut to [`MAX_LANGUAGE_BYTES`].
    pub fn language(&self) -> &str {
        &self.language
    }

    /// The lines it keeps.
    pub fn lines(&self) -> &[Vec<CodeSpan>] {
        &self.lines
    }

    /// Whether it cut its owner's code to keep within its limits.
    pub const fn is_cut(&self) -> bool {
        self.cut
    }

    /// The code it keeps, its lines joined by line feeds.
    pub fn text(&self) -> String {
        let lines: Vec<String> = self
            .lines
            .iter()
            .map(|spans| spans.iter().map(CodeSpan::text).collect())
            .collect();
        lines.join("\n")
    }

    /// Adds it as the last child of `parent`, with its COPY button, and
    /// returns its node.
    ///
    /// # Errors
    ///
    /// The errors of [`Tree::insert`], after which the block is removed
    /// again, so the tree is as it was.
    pub fn insert(self, tree: &mut Tree, parent: NodeId) -> Result<NodeId, UiError> {
        let block = tree.insert(parent, self.node())?;
        if let Err(error) = tree.insert(block, CopyButton::default().node()) {
            // Just inserted, and not the root, so it can be removed.
            let _ = tree.remove(block);
            return Err(error);
        }
        Ok(block)
    }

    /// The COPY button of a block [`insert`](Self::insert) added, if
    /// `block` is one.
    pub fn copy_button(tree: &Tree, block: NodeId) -> Option<NodeId> {
        tree.children(block)?
            .iter()
            .copied()
            .find(|&child| tree.widget::<CopyButton>(child).is_some())
    }

    /// The width of its widest line.
    fn widest(&self, metrics: &dyn TextMetrics) -> f32 {
        self.lines
            .iter()
            .map(|spans| {
                spans
                    .iter()
                    .map(|span| metrics.width(&span.text, &CODE_TEXT))
                    .sum::<f32>()
            })
            .fold(0.0, f32::max)
    }
}

/// Pushes `text` onto `out` as a line of code holds it, a tab as four
/// spaces and leaving out other control characters, line breaks among
/// them, until `out` is longer than `room`; returns whether it is. So
/// `out` grows to at most `room` and a tab's four bytes, however long
/// `text` is, and cutting it to `room` cuts the whole text cleaned at the
/// same boundary, as it holds a character past `room` ([`cut_to`]). It
/// reads `text` up to the character that passes `room`: a run of the
/// control characters it leaves out, it reads to its end.
fn clean_into(out: &mut String, text: &str, room: usize) -> bool {
    for c in text.chars() {
        if out.len() > room {
            return true;
        }
        match c {
            '\t' => out.push_str(TAB_SPACES),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.len() > room
}

/// The lines of `text`, each without its line end: a line feed, a
/// carriage return and line feed, or a lone carriage return (ADR-0019). A
/// line end at its very end starts no line, as [`str::lines`] has it.
pub(super) fn lines_of(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = text;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let Some(at) = rest.find(['\n', '\r']) else {
            return Some(std::mem::take(&mut rest));
        };
        let (line, end) = rest.split_at(at);
        let skip = if end.starts_with("\r\n") { 2 } else { 1 };
        rest = end.get(skip..).unwrap_or("");
        Some(line)
    })
}

/// One line of code as a block keeps it, cleaned into a buffer span by
/// span, up to its limits.
struct Line<'a> {
    /// The line cleaned so far, at most [`MAX_CODE_LINE_BYTES`] and a
    /// tab's four bytes.
    buffer: &'a mut String,
    /// Where each span taken starts in the buffer, and its token.
    starts: Vec<(usize, Token)>,
    /// Where the spans it took end, once it refused one past
    /// [`MAX_CODE_SPANS`]: what it keeps ends there or before, at a
    /// cluster boundary that the character after it decides, which the
    /// spans it refuses are cleaned into the buffer to find.
    end: Option<usize>,
    /// How many spans it refused looking for that character.
    refused: usize,
    /// Whether it gave up looking, [`MAX_CODE_SPANS`] refused spans having
    /// cleaned to nothing: what it keeps then ends before the last
    /// cluster it took, which a character after them might have extended.
    unknown: bool,
    /// Whether the line was cut: what is left of it is not kept.
    cut: bool,
}

impl<'a> Line<'a> {
    /// A buffer for lines, which never grows.
    fn buffer() -> String {
        String::with_capacity(MAX_CODE_LINE_BYTES + TAB_SPACES.len())
    }

    /// A line to keep, in `buffer`, which it empties.
    fn new(buffer: &'a mut String) -> Self {
        buffer.clear();
        Self {
            buffer,
            starts: Vec::new(),
            end: None,
            refused: 0,
            unknown: false,
            cut: false,
        }
    }

    /// Takes the line's next span, `text` in `token`; returns whether it
    /// takes more, which it does not once the line is cut, unless it has
    /// refused a span and is still looking for the character after the
    /// spans it took.
    fn take(&mut self, text: &str, token: Token) -> bool {
        if let Some(end) = self.end {
            self.refused += 1;
            if clean_into(self.buffer, text, end) {
                return false;
            }
            self.unknown = self.refused == MAX_CODE_SPANS;
            return !self.unknown;
        }
        if self.cut {
            return false;
        }
        if self.starts.len() == MAX_CODE_SPANS {
            self.cut = true;
            self.end = Some(self.buffer.len());
            return self.take(text, token);
        }
        self.starts.push((self.buffer.len(), token));
        if clean_into(self.buffer, text, MAX_CODE_LINE_BYTES) {
            self.cut = true;
        }
        !self.cut
    }

    /// The spans it keeps, the line cut at the last grapheme cluster
    /// boundary of the whole line at most [`MAX_CODE_LINE_BYTES`] in, and
    /// no further than the spans it took, and whether it was cut.
    fn finish(self) -> (Vec<CodeSpan>, bool) {
        let room = match self.end {
            None => MAX_CODE_LINE_BYTES,
            Some(end) if self.unknown => end.saturating_sub(1),
            Some(end) => end,
        };
        let kept = cut_to(self.buffer, room.min(MAX_CODE_LINE_BYTES));
        let mut spans = Vec::new();
        for (k, &(start, token)) in self.starts.iter().enumerate() {
            let end = self
                .starts
                .get(k + 1)
                .map_or(kept.len(), |&(next, _)| next.min(kept.len()));
            if let Some(text) = kept.get(start..end).filter(|text| !text.is_empty()) {
                spans.push(CodeSpan {
                    text: text.to_string(),
                    token,
                });
            }
        }
        (spans, self.cut)
    }
}

/// Shows its code and language as their lengths only.
impl fmt::Debug for CodeBlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bytes: usize = self
            .lines
            .iter()
            .flatten()
            .map(|span| span.text.len())
            .sum();
        f.debug_struct("CodeBlock")
            .field("language", &TextLength(&self.language))
            .field("lines", &self.lines.len())
            .field("bytes", &bytes)
            .field("cut", &self.cut)
            .finish()
    }
}

impl Widget for CodeBlock {
    /// As wide as its widest line and as tall as its bar and its lines,
    /// with the space around them, less what its padding, which places
    /// COPY, holds.
    fn measure(&self, _available: Size, metrics: &dyn TextMetrics) -> Size {
        let width = CODE_RULE + CODE_PADDING_LEFT + self.widest(metrics) + CODE_PADDING;
        let lines = self.lines.len() as f32 * line_height(metrics, &CODE_TEXT);
        let height = CODE_BAR_HEIGHT + 2.0 * CODE_PADDING + lines;
        let (top, right) = copy_insets();
        Size::new((width - right).max(0.0), (height - top).max(0.0))
    }

    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let bounds = cx.bounds();
        let theme = cx.theme();
        let metrics = cx.metrics();
        dimmed(cx, list, |list| {
            let c = |token| theme.color(token);
            list.rect(bounds, c(Token::EditorBg));
            let bar = Rect::new(bounds.x, bounds.y, bounds.width, CODE_BAR_HEIGHT);
            list.rect(bar, c(Token::CodeBar));
            list.rect(
                Rect::new(bar.x, bar.y + bar.height - 1.0, bar.width, 1.0),
                c(Token::Line),
            );
            list.border(bounds, 1.0, c(Token::Line2));
            list.rect(
                Rect::new(bounds.x, bounds.y, CODE_RULE, bounds.height),
                c(Token::Teal),
            );
            if !self.language.is_empty() {
                // Clear of COPY, at the bar's right.
                let copy = metrics.width(COPY, &COPY_LABEL) + 2.0 * COPY_PADDING;
                let label = Rect::new(
                    bar.x + CODE_PADDING_LEFT,
                    bar.y,
                    (bar.width - CODE_PADDING_LEFT - 2.0 * COPY_RIGHT - copy).max(0.0),
                    bar.height,
                );
                list.push_clip(label);
                list.text(
                    Point::new(label.x, baseline(bar, metrics, &CODE_BAR_TEXT)),
                    self.language.to_uppercase(),
                    CODE_BAR_TEXT,
                    c(Token::Text3),
                );
                list.pop_clip();
            }
            let code = Rect::new(
                bounds.x + CODE_RULE,
                bar.y + bar.height,
                (bounds.width - CODE_RULE - 1.0).max(0.0),
                (bounds.height - bar.height - 1.0).max(0.0),
            );
            let high = line_height(metrics, &CODE_TEXT);
            list.push_clip(code);
            let mut y = code.y + CODE_PADDING;
            for spans in &self.lines {
                if y >= code.y + code.height {
                    break;
                }
                let row = Rect::new(code.x, y, code.width, high);
                let base = baseline(row, metrics, &CODE_TEXT);
                let mut x = bounds.x + CODE_PADDING_LEFT;
                for span in spans {
                    list.text(
                        Point::new(x, base),
                        span.text.clone(),
                        CODE_TEXT,
                        c(span.token),
                    );
                    x += metrics.width(&span.text, &CODE_TEXT);
                }
                y += high;
            }
            list.pop_clip();
        });
    }
}

impl Control for CodeBlock {
    fn semantics(&self) -> Semantics {
        let name = if self.language.trim().is_empty() {
            "code".to_string()
        } else {
            self.language.clone()
        };
        Semantics::new(Role::Group, name).with_value(self.text())
    }

    /// As wide as its parent, as tall as its code, with COPY at the right
    /// of its bar.
    fn style(&self) -> Style {
        let (top, right) = copy_insets();
        Style::stack()
            .width(Length::Fill(1.0))
            .padding(Insets::new(top, right, 0.0, 0.0))
            .align(Align::End, Align::Start)
    }

    fn focusable(&self) -> bool {
        false
    }
}

/// The block's padding above and right of COPY, which places it in the
/// bar.
fn copy_insets() -> (f32, f32) {
    ((CODE_BAR_HEIGHT - COPY_HEIGHT) / 2.0, COPY_RIGHT)
}

/// A code block's COPY button (`.code-copy`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct CopyButton {
    activator: Activator,
}

impl CopyButton {
    /// Paints its shape for `look`, and returns its label's token.
    fn shape(theme: &Theme, rect: Rect, look: Look, list: &mut DrawList) -> Token {
        let c = |token| theme.color(token);
        match look {
            Look::Rest => {}
            Look::Hover => list.rect(rect, c(Token::TealWash)),
            Look::Pressed => list.rect(rect, c(Token::TealFill)),
        }
        list.border(rect, 1.0, c(Token::TealGlow));
        if look == Look::Rest {
            Token::Teal
        } else {
            Token::TealHi
        }
    }
}

impl Widget for CopyButton {
    fn measure(&self, _available: Size, metrics: &dyn TextMetrics) -> Size {
        Size::new(
            metrics.width(COPY, &COPY_LABEL) + 2.0 * COPY_PADDING,
            COPY_HEIGHT,
        )
    }

    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let rect = cx.bounds();
        let theme = cx.theme();
        dimmed(cx, list, |list| {
            let label = Self::shape(theme, rect, Look::of(cx.state()), list);
            list.text(
                Point::new(
                    rect.x + COPY_PADDING,
                    baseline(rect, cx.metrics(), &COPY_LABEL),
                ),
                COPY,
                COPY_LABEL,
                theme.color(label),
            );
        });
        if cx.state().focus_visible {
            focus_outline(list, rect, theme);
        }
    }

    fn event(&mut self, cx: &mut EventContext<'_>, event: &InputEvent) -> Handled {
        self.activator.activate(cx, event)
    }
}

impl Control for CopyButton {
    fn semantics(&self) -> Semantics {
        Semantics::new(Role::Button, "Copy")
    }

    fn style(&self) -> Style {
        Style::stack()
    }

    fn focusable(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use tidepool_core::testing::check_sized;
    use tidepool_text::pos::grapheme;

    use super::*;

    /// The whole of `text` cleaned, as the block first cleaned each span.
    fn clean_whole(text: &str) -> String {
        let mut out = String::new();
        for c in text.chars() {
            match c {
                '\t' => out.push_str(TAB_SPACES),
                c if c.is_control() => {}
                c => out.push(c),
            }
        }
        out
    }

    /// What the block first kept of a line of `spans`: each span cleaned
    /// whole, then cut to the room left.
    fn first_kept(spans: &[(String, Token)]) -> (Vec<CodeSpan>, bool) {
        let (mut kept, mut bytes, mut cut) = (Vec::new(), 0, false);
        for (text, token) in spans {
            if kept.len() == MAX_CODE_SPANS || bytes == MAX_CODE_LINE_BYTES {
                cut = true;
                break;
            }
            let text = clean_whole(text);
            let room = MAX_CODE_LINE_BYTES - bytes;
            let text = if text.len() > room {
                cut = true;
                cut_to(&text, room).to_string()
            } else {
                text
            };
            bytes += text.len();
            if !text.is_empty() {
                kept.push(CodeSpan {
                    text,
                    token: *token,
                });
            }
        }
        (kept, cut)
    }

    const PIECES: [&str; 12] = [
        "x",
        "let",
        "\t",
        "\r",
        "\u{7}",
        "e\u{301}",
        "\u{301}",
        "\u{1F1FA}\u{1F1F8}",
        "\u{1F468}\u{200D}\u{1F469}",
        "日本",
        " ",
        ";",
    ];

    #[test]
    fn a_line_keeps_its_start_cut_at_a_boundary_of_the_whole_line() {
        check_sized(
            "code block lines",
            300,
            |rng, size| {
                let spans = rng.range(1..48u32);
                let long = rng.range(0..4u32) == 0;
                (0..spans)
                    .map(|_| {
                        let pieces = if long {
                            rng.range(0..size.max(1) * 8)
                        } else {
                            rng.range(0..size.max(1))
                        };
                        let text: String = (0..pieces)
                            .map(|_| *rng.choose(&PIECES).expect("pieces"))
                            .collect();
                        (
                            text,
                            *rng.choose(&[Token::Text, Token::Teal]).expect("tokens"),
                        )
                    })
                    .collect::<Vec<(String, Token)>>()
            },
            |spans| {
                let block = CodeBlock::new(
                    "x",
                    [spans
                        .iter()
                        .map(|(text, token)| CodeSpan::new(text.as_str(), *token))],
                );
                let kept = block.text();
                // The whole line, every span given; and what of it the
                // spans taken hold.
                let whole: String = spans.iter().map(|(text, _)| clean_whole(text)).collect();
                let taken: usize = spans
                    .iter()
                    .take(MAX_CODE_SPANS)
                    .map(|(text, _)| clean_whole(text).len())
                    .sum();
                // Within the limits, it keeps what it first kept. It takes
                // at most the limit of spans, empty ones too, where it
                // first went on through empty spans without end.
                let (first, first_cut) = first_kept(spans);
                let within = !first_cut && spans.len() <= MAX_CODE_SPANS;
                if within && (block.lines()[0] != first || block.is_cut()) {
                    return Err("a line within the limits changed".into());
                }
                if kept.len() > MAX_CODE_LINE_BYTES
                    || !whole.starts_with(&kept)
                    || !grapheme::is_boundary(&whole, kept.len())
                {
                    return Err(format!("kept {} bytes of {}", kept.len(), whole.len()));
                }
                // And it keeps all it can: the next boundary of the whole
                // line is past what the caps and the spans taken allow.
                let room = MAX_CODE_LINE_BYTES.min(taken);
                if grapheme::next_boundary(&whole, kept.len()).is_some_and(|next| next <= room) {
                    return Err(format!("kept {} bytes where {room} were room", kept.len()));
                }
                let all = kept == whole && spans.len() <= MAX_CODE_SPANS;
                if block.is_cut() == all {
                    return Err(format!(
                        "cut {} for {} of {}",
                        block.is_cut(),
                        kept.len(),
                        whole.len()
                    ));
                }
                // Each kept span is a run of its own span's text, in order.
                let mut at = 0;
                for span in &block.lines()[0] {
                    if span.text().is_empty()
                        || whole.get(at..at + span.text().len()) != Some(span.text())
                    {
                        return Err("a kept span is not the line's".into());
                    }
                    at += span.text().len();
                }
                Ok(())
            },
        );
    }

    /// A line of `count` spans of `x` and then `rest`, each in `text`.
    fn spans_then(count: usize, rest: &[&str]) -> Vec<CodeSpan> {
        let mut spans: Vec<CodeSpan> = (0..count)
            .map(|_| CodeSpan::new("x", Token::Text))
            .collect();
        spans.extend(rest.iter().map(|text| CodeSpan::new(*text, Token::Text)));
        spans
    }

    #[test]
    fn a_line_cut_by_the_span_limit_ends_at_a_cluster_boundary_of_the_whole_line() {
        let kept = |spans: Vec<CodeSpan>| {
            let block = CodeBlock::new("x", [spans]);
            assert!(block.is_cut());
            block.text()
        };
        let xs = |count: usize| "x".repeat(count);
        // The 33rd span is an accent on the 32nd's last letter: the letter
        // goes with it.
        let mut spans = spans_then(MAX_CODE_SPANS - 1, &["ye", "\u{301}"]);
        assert_eq!(kept(spans.clone()), format!("{}y", xs(MAX_CODE_SPANS - 1)));
        // So too past spans that clean to nothing, and the line is short.
        spans.insert(MAX_CODE_SPANS, CodeSpan::new("", Token::Text));
        spans.insert(MAX_CODE_SPANS, CodeSpan::new("\u{7}\r", Token::Text));
        assert_eq!(kept(spans), format!("{}y", xs(MAX_CODE_SPANS - 1)));
        // A flag across the 32nd and 33rd spans is left out whole.
        let flag = spans_then(MAX_CODE_SPANS - 1, &["\u{1F1FA}", "\u{1F1F8}"]);
        assert_eq!(kept(flag), xs(MAX_CODE_SPANS - 1));
        // A 33rd span that starts a cluster of its own leaves the 32 whole.
        assert_eq!(
            kept(spans_then(MAX_CODE_SPANS + 1, &[])),
            xs(MAX_CODE_SPANS)
        );
        // Spans without end that clean to nothing: it gives up looking,
        // and keeps only clusters a character after them could not extend.
        let empty = std::iter::repeat_with(|| CodeSpan::new("", Token::Text));
        let endless = spans_then(MAX_CODE_SPANS, &[]).into_iter().chain(empty);
        assert_eq!(
            CodeBlock::new("x", [endless]).text(),
            xs(MAX_CODE_SPANS - 1)
        );
        // The line ends after the spans it refuses: all 32 are kept.
        let ended = spans_then(MAX_CODE_SPANS, &["", "\u{7}"]);
        assert_eq!(kept(ended), xs(MAX_CODE_SPANS));
    }

    #[test]
    fn a_span_cut_ends_the_line() {
        // A wide span past the cap is cut to nothing, and the spans after
        // it, which would fit, are not kept: the line is its start.
        let block = CodeBlock::new(
            "rust",
            [[
                CodeSpan::new("x".repeat(398), Token::Text),
                CodeSpan::new("日本", Token::Amber),
                CodeSpan::new(";", Token::Text2),
                CodeSpan::new("y", Token::Text2),
            ]],
        );
        assert_eq!(block.text(), "x".repeat(398));
        assert_eq!(block.lines()[0].len(), 1);
        assert!(block.is_cut());
        // Cut inside a span, the spans before it are kept whole.
        let block = CodeBlock::new(
            "rust",
            [[
                CodeSpan::new("fn", Token::Teal),
                CodeSpan::new(" main", Token::Text),
                CodeSpan::new("a".repeat(500), Token::Text2),
                CodeSpan::new("z", Token::Text),
            ]],
        );
        let line = &block.lines()[0];
        assert_eq!(line.len(), 3);
        assert_eq!((line[0].text(), line[1].text()), ("fn", " main"));
        assert_eq!(line[2].text(), "a".repeat(MAX_CODE_LINE_BYTES - 7));
    }

    #[test]
    fn cleaning_a_huge_line_stops_past_its_room() {
        // Sixteen MiB of tabs: the buffer never grows past the room and a
        // tab, and the line keeps a hundred of them.
        let tabs = "\t".repeat(16 << 20);
        let mut buffer = Line::buffer();
        let capacity = buffer.capacity();
        assert!(capacity <= MAX_CODE_LINE_BYTES + TAB_SPACES.len());
        assert!(clean_into(&mut buffer, &tabs, MAX_CODE_LINE_BYTES));
        assert!(buffer.len() <= MAX_CODE_LINE_BYTES + TAB_SPACES.len());
        assert_eq!(buffer.capacity(), capacity);
        let block = CodeBlock::plain("text", &tabs);
        assert_eq!(block.text(), " ".repeat(MAX_CODE_LINE_BYTES));
        assert!(block.is_cut());
        let block = CodeBlock::new("text", [[CodeSpan::new(tabs, Token::Text)]]);
        assert_eq!(block.text(), " ".repeat(MAX_CODE_LINE_BYTES));
        // Within the room, it is the text cleaned.
        let mut buffer = Line::buffer();
        assert!(!clean_into(&mut buffer, "\tlet x\u{7} = 1;\r", 400));
        assert_eq!(buffer, "    let x = 1;");
    }

    #[test]
    fn plain_text_ends_lines_as_adr_0019_does() {
        fn lines(text: &str) -> Vec<&str> {
            lines_of(text).collect()
        }
        assert_eq!(lines("a\rb"), ["a", "b"]);
        assert_eq!(lines("a\r\nb\nc\r"), ["a", "b", "c"]);
        assert_eq!(lines("a\n\n\rb"), ["a", "", "", "b"]);
        assert_eq!(lines("\r\r"), ["", ""]);
        assert!(lines("").is_empty());
        assert_eq!(CodeBlock::plain("x", "a\rb").lines().len(), 2);
        assert_eq!(
            CodeBlock::plain("x", "one\rtwo\r\nthree").text(),
            "one\ntwo\nthree"
        );
    }

    #[test]
    fn a_language_is_cleaned_and_capped() {
        // Four MiB of clusters of three bytes: the cap falls inside one.
        let long = "e\u{301}".repeat((4 << 20) / 3);
        let block = CodeBlock::plain(long.as_str(), "x");
        let language = block.language();
        assert!(!language.is_empty() && language.len() <= MAX_LANGUAGE_BYTES);
        assert!(long.starts_with(language) && grapheme::is_boundary(&long, language.len()));
        assert!(block.semantics().name.len() <= MAX_LANGUAGE_BYTES);
        let shown = format!("{block:?}");
        assert!(shown.len() < 200, "{shown}");
        // Its `Debug` shows its length only.
        let shown = format!("{:?}", CodeBlock::plain("hunter2", "x"));
        assert!(
            !shown.contains("hunter2") && shown.contains("7 bytes"),
            "{shown}"
        );
        let block = CodeBlock::plain("ru\u{7}st\n", "x");
        assert_eq!(block.language(), "rust");
        assert_eq!(block.semantics().name, "rust");
    }

    #[test]
    fn code_is_cleaned_and_cut_to_its_limits() {
        let block = CodeBlock::new("rust", [[CodeSpan::new("\tlet x = 1;\r", Token::Text)]]);
        assert_eq!(block.text(), "    let x = 1;");
        assert!(!block.is_cut());
        // Too many lines, too long a line, too many spans.
        let many = CodeBlock::plain("text", &"x\n".repeat(MAX_CODE_LINES + 5));
        assert_eq!((many.lines().len(), many.is_cut()), (MAX_CODE_LINES, true));
        let long = CodeBlock::plain("text", &"é".repeat(MAX_CODE_LINE_BYTES));
        assert_eq!(long.text().len(), MAX_CODE_LINE_BYTES);
        assert!(long.is_cut());
        let spans = CodeBlock::new(
            "text",
            [(0..MAX_CODE_SPANS + 3).map(|_| CodeSpan::new("a", Token::Text))],
        );
        assert_eq!(
            (spans.lines()[0].len(), spans.is_cut()),
            (MAX_CODE_SPANS, true)
        );
        let shown = format!("{:?}", CodeBlock::plain("rust", "hunter2"));
        assert!(
            !shown.contains("hunter2") && shown.contains("bytes: 7"),
            "{shown}"
        );
    }
}
