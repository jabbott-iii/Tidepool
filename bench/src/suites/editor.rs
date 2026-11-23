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

//! The suites of `tidepool-editor`: the editor view's scrolling budget,
//! 60 fps through a 10,000-line Rust file at 1080p (`docs/00-overview.md`),
//! on which roadmap M4.1 (the editor view) is accepted, and what ADR-0063
//! (the view lays out only the lines it shows) asks the suite to measure.
//! A 60 fps frame is 16.7 ms.
//!
//! - `editor/scroll/10k_lines_1080p`: the view alone in a window of
//!   1920 × 1080 at scale 1, soft wrap off, over `source`'s 10,000
//!   lines. Each iteration is one notch of the wheel down, three rows, as
//!   the main loop handles it: [`EditorView::handle`], then
//!   [`Tree::frame`], [`Painter::paint`] of the frame's damage (the parts
//!   of the window that changed) into a 1920 × 1080 framebuffer, and
//!   [`EditorView::update`], framing and painting again while `update`
//!   says there is more to draw.
//! - `editor/scroll/10k_lines_1080p_wrap`: the same with soft wrap on,
//!   over `wrapping_source`'s 10,000 lines, about a fifth of which wrap.
//!   They are `source`'s, but for 1,930 (19.3%) made 250 to 600 bytes of
//!   prose: 322 doc comments, 467 line comments, and 1,141 `let`s of a
//!   string literal, 426 bytes on average (532 of 250 to 349 bytes, 544
//!   of 350 to 449, 605 of 450 to 549, and 249 of 550 to 600). A row at
//!   1080p holds 219 columns, so, counting each character as JetBrains
//!   Mono's 8.4-unit advance, 1,005 of them wrap to two rows and 925 to
//!   three. The file is 1,168,501 bytes; a line is 116.9 bytes on
//!   average, its median 54 and its 90th percentile 422.
//! - `editor/layout/long_line_10kb`: the notch that brings a line of
//!   exactly [`MAX_LINE_BYTES`] (10,000 bytes, so not cut) on screen, the
//!   first time it is laid out: the same steps as a scroll, from a new
//!   view each iteration, made outside the timing. The view measures each
//!   grapheme cluster (what a reader sees as one character) of a line the
//!   first time it lays it out, so this frame may take longer than one;
//!   the suite reports it and checks no budget.
//!
//! The text is measured with `gui`'s [`FontMetrics`] over the bundled
//! faces, loaded once for the suite, as the window measures it; the
//! painter loads the same faces, so its metrics give the same sizes. The
//! painter's glyph atlas is warm after the first iteration, as a window's
//! is after its first frame.
//!
//! # Going through the file
//!
//! A scroll suite keeps scrolling down through the file. A notch at the
//! end would not move, so after `notches_per_pass` notches, fewer than
//! reach the end, the next iteration starts from a new view at the top:
//! built, laid out and drawn once outside the timing, and swapped in at
//! the start of the iteration, a move of a few words; the old view is the
//! iteration's output, which the harness drops outside the timing.
//!
//! # Checks
//!
//! Before it is measured, each suite checks, untimed, that what it
//! measures happens: the first frame draws a text on each row on screen,
//! and with soft wrap on more rows than line numbers (lines wrap), with
//! it off a line on each row; a notch scrolls three rows, the fourth row
//! on screen becoming the first, and repaints the text area from its top
//! row to its bottom one; and for the long line, that it was off screen
//! before the notch, on screen after it, and laid out whole, as the
//! horizontal scrollbar's range shows. After it is measured, each suite
//! checks that no iteration failed and that every measured notch moved
//! the view.

use std::cell::{Cell, RefCell};

use tidepool_core::Pcg32;
use tidepool_core::geom::{DeviceSize, Point, Rect, Scale, Size};
use tidepool_core::input::{InputEvent, Modifiers, ScrollDelta};
use tidepool_editor::view::{EDITOR_TEXT, EditorView, MAX_LINE_BYTES, WHEEL_ROWS};
use tidepool_font::bundled;
use tidepool_gfx::framebuffer::Framebuffer;
use tidepool_gui::{FontMetrics, Painter};
use tidepool_text::Rope;
use tidepool_ui::Tree;
use tidepool_ui::draw::Command;
use tidepool_ui::layout::Axis;
use tidepool_ui::metrics::TextMetrics;
use tidepool_ui::theme::Token;
use tidepool_ui::widgets::ScrollBar;

use crate::{BenchError, Bencher, Suite};

/// The editor suites, in the order a run takes them.
pub const SUITES: &[Suite] = &[
    Suite {
        name: "editor/scroll/10k_lines_1080p",
        run: |b| scroll(b, false),
    },
    Suite {
        name: "editor/scroll/10k_lines_1080p_wrap",
        run: |b| scroll(b, true),
    },
    Suite {
        name: "editor/layout/long_line_10kb",
        run: long_line,
    },
];

/// The seed of the generated file, as the text suites' ("tidepool").
const SEED: u64 = 0x7469_6465_706f_6f6c;

/// The lines of the generated file.
const LINES: usize = 10_000;

/// The window, in logical units: 1080p at scale 1.
const WINDOW: Size = Size::new(1920.0, 1080.0);

/// The window, in device pixels.
const DEVICE: DeviceSize = DeviceSize::new(1920, 1080);

/// The line of the long-line suite's file that is 10,000 bytes long: the
/// second line under the 51 rows a 1,070-unit text area shows, which a
/// notch of three rows brings on screen.
const LONG_AT: usize = 52;

/// The lines of the long-line suite's file, the long one among them.
const LONG_FILE_LINES: usize = 120;

/// What the generated code's names and comments are made of.
const WORDS: [&str; 24] = [
    "value", "input", "count", "index", "offset", "length", "scale", "total", "line", "width",
    "height", "buffer", "cursor", "range", "start", "end", "column", "row", "glyph", "span",
    "frame", "delta", "state", "limit",
];

/// The right-hand sides a generated `let` is made of, `{w}` a word and
/// `{n}` a number.
const TERMS: [&str; 8] = [
    "input[{n}]",
    "{w}_{n}",
    "scale.saturating_mul({n})",
    "{w}.wrapping_add({n})",
    "input.len() as u32",
    "({w} >> {n})",
    "{w}.min({n})",
    "u32::from({w} > {n})",
];

/// A word of [`WORDS`], drawn.
fn word(rng: &mut Pcg32) -> &'static str {
    rng.choose(&WORDS).copied().unwrap_or("value")
}

/// A number below 100, drawn.
fn number(rng: &mut Pcg32) -> u32 {
    rng.range(0..100_u32)
}

/// A whole number of bytes from `range`, drawn.
fn bytes(rng: &mut Pcg32, range: std::ops::Range<u32>) -> usize {
    rng.range(range) as usize
}

/// `start` and then words, the first right after it and each other after
/// a space, as long as the line stays within `target` bytes, at least one;
/// then `end`.
fn prose(rng: &mut Pcg32, start: &str, target: usize, end: &str) -> String {
    let mut line = start.to_string();
    loop {
        let next = word(rng);
        if line.len() > start.len() && line.len() + 1 + next.len() + end.len() > target {
            break;
        }
        if line.len() > start.len() {
            line.push(' ');
        }
        line.push_str(next);
    }
    line.push_str(end);
    line
}

/// A `let` at `indent` spaces, its right-hand side [`TERMS`] joined by
/// ` + `, as long as the line stays within `target` bytes, at least one.
fn statement(rng: &mut Pcg32, indent: usize, target: usize) -> String {
    let mut line = format!("{:indent$}let {}_{} = ", "", word(rng), number(rng));
    let start = line.len();
    loop {
        let term = rng
            .choose(&TERMS)
            .copied()
            .unwrap_or("{w}")
            .replace("{w}", word(rng))
            .replace("{n}", &number(rng).to_string());
        let joint = if line.len() > start { " + " } else { "" };
        if line.len() > start && line.len() + joint.len() + term.len() + 1 > target {
            break;
        }
        line.push_str(joint);
        line.push_str(&term);
    }
    line.push(';');
    line
}

/// The generated Rust-like file of `lines` lines, joined by line feeds,
/// with none after the last, the same on every run (a seeded [`Pcg32`]).
///
/// It is a run of functions, each but the first after a blank line:
///
/// - one or two lines of doc comment, each 40 to 99 bytes;
/// - the function's signature, 70 to 75 bytes;
/// - 4 to 15 lines of body, each drawn: a quarter a line comment of 30
///   to 99 bytes, an eighth an `if` that opens a block one level deeper
///   (or, already in one, a `}` that closes it), and the rest a `let` of
///   25 to 99 bytes, indented 4 spaces, or 8 in the block;
/// - `    Some(scale)` and the closing brace.
///
/// The last function is cut where the file reaches its length. For
/// 10,000 lines that is 457,754 bytes in 655 functions: 655 blank lines,
/// 982 lines of doc comment, 1,540 line comments, 3,854 `let`s, 502
/// `if`s and 1,157 closing braces, 1,590 lines indented 8 spaces. A line
/// is 44.8 bytes on average, its median 44, its 90th percentile 81, and
/// the longest 99; 1,812 lines are under 10 bytes (blank, or a brace),
/// and 2,659 are 70 or more. The module's tests pin the count, size,
/// blank lines, and longest.
fn source(lines: usize) -> String {
    generate(lines, None)
}

/// The wrap suite's file of `lines` lines: [`source`]'s, with 5 in 16 of
/// its doc comments, line comments and `let`s, drawn from a stream of
/// their own, made 250 to 600 bytes long ([`lengthen`]), and every other
/// line as it is there. The module documentation gives the numbers.
fn wrapping_source(lines: usize) -> String {
    generate(lines, Some(Pcg32::new(SEED, 13)))
}

/// What a generated line is, for [`lengthen`]: a doc comment, or a line
/// comment or a `let` at its indent.
#[derive(Clone, Copy, Debug)]
enum Kind {
    Doc,
    Comment(usize),
    Let(usize),
}

/// Pushes `line`, of `kind`, onto `out`; or, when there is a `long`
/// stream, 5 times in 16 a long line of the same kind in its place, drawn
/// from that stream, so that the rest of the file is drawn as it would be
/// without it.
fn put(out: &mut Vec<String>, long: &mut Option<Pcg32>, line: String, kind: Kind) {
    if let Some(rng) = long.as_mut()
        && rng.range(0..16_u32) < 5
    {
        out.push(lengthen(rng, kind));
    } else {
        out.push(line);
    }
}

/// A line of `kind` of 250 to 600 bytes: a doc comment or a line comment
/// of prose, or a `let` of a string literal of prose.
fn lengthen(rng: &mut Pcg32, kind: Kind) -> String {
    let target = bytes(rng, 256..601);
    match kind {
        Kind::Doc => prose(rng, "/// ", target, "."),
        Kind::Comment(indent) => prose(rng, &format!("{:indent$}// ", ""), target, "."),
        Kind::Let(indent) => {
            let start = format!("{:indent$}let {}_{} = \"", "", word(rng), number(rng));
            prose(rng, &start, target, "\";")
        }
    }
}

/// [`source`]'s lines, and with `long`, [`wrapping_source`]'s.
fn generate(lines: usize, mut long: Option<Pcg32>) -> String {
    let mut rng = Pcg32::new(SEED, 11);
    let mut out: Vec<String> = Vec::with_capacity(lines + 32);
    let mut item = 0;
    while out.len() < lines {
        if !out.is_empty() {
            out.push(String::new());
        }
        for _ in 0..rng.range(1..3_u32) {
            let target = bytes(&mut rng, 40..100);
            let line = prose(&mut rng, "/// ", target, ".");
            put(&mut out, &mut long, line, Kind::Doc);
        }
        let name = word(&mut rng);
        out.push(format!(
            "pub fn {name}_{item}(input: &[u32], scale: u32, limit: usize) -> Option<u32> {{"
        ));
        let mut depth = 1;
        for _ in 0..rng.range(4..16_u32) {
            let indent = 4 * depth;
            match rng.range(0..8_u32) {
                0 | 1 => {
                    let target = bytes(&mut rng, 30..100);
                    let line = prose(&mut rng, &format!("{:indent$}// ", ""), target, ".");
                    put(&mut out, &mut long, line, Kind::Comment(indent));
                }
                2 if depth == 1 => {
                    let (w, n) = (word(&mut rng), number(&mut rng));
                    out.push(format!("    if {w}_{n} > limit as u32 {{"));
                    depth = 2;
                }
                2 => {
                    out.push("    }".to_string());
                    depth = 1;
                }
                _ => {
                    let target = bytes(&mut rng, 20..100);
                    let line = statement(&mut rng, indent, target);
                    put(&mut out, &mut long, line, Kind::Let(indent));
                }
            }
        }
        if depth == 2 {
            out.push("    }".to_string());
        }
        out.push("    Some(scale)".to_string());
        out.push("}".to_string());
        item += 1;
    }
    out.truncate(lines);
    out.join("\n")
}

/// The long-line suite's file, [`LONG_FILE_LINES`] lines: those of
/// [`source`], with line [`LONG_AT`] (from 0) put among them, a `const`
/// holding a string of [`WORDS`], indented 4 spaces and exactly
/// [`MAX_LINE_BYTES`] long.
fn long_source() -> String {
    let mut rng = Pcg32::new(SEED, 12);
    let start = "    const TEXT: &str = \"";
    let end = "\";";
    let mut long = prose(&mut rng, start, MAX_LINE_BYTES + 64, "");
    // ASCII, so any length is a character boundary.
    long.truncate(MAX_LINE_BYTES - end.len());
    long.push_str(end);
    let text = source(LONG_FILE_LINES - 1);
    let mut lines: Vec<&str> = text.split('\n').collect();
    lines.insert(LONG_AT, &long);
    lines.join("\n")
}

/// An error naming what a suite's untimed check found wrong, unless it
/// holds.
fn check(holds: bool, what: &str) -> Result<(), BenchError> {
    if holds { Ok(()) } else { Err(failed(what)) }
}

/// The error of a suite's untimed check that found `what` wrong.
fn failed(what: &str) -> BenchError {
    BenchError::Check(format!("its check failed: {what}"))
}

/// The error of a suite whose setup or routine met `error` while it was
/// `doing` something.
fn broke(doing: &str, error: impl std::fmt::Display) -> BenchError {
    failed(&format!("{doing}: {error}"))
}

/// What one notch did.
#[derive(Clone, Copy, Debug)]
struct Step {
    /// Whether the view moved.
    moved: bool,
    /// The frames drawn: at least one for a notch that moved.
    frames: usize,
    /// The device pixels painted, over all its frames.
    painted: u64,
    /// The topmost and bottommost edges of the damage painted, in logical
    /// units, if any.
    span: Option<(f32, f32)>,
}

/// A view in a tree of its own, filling [`WINDOW`], laid out and drawn
/// once.
struct Scroller {
    tree: Tree,
    view: EditorView,
}

impl Scroller {
    /// A view of `rope`, its soft wrap `wrap`, at the top, with its first
    /// frame drawn (but not painted) and nothing left to draw.
    fn new(rope: &Rope, wrap: bool, metrics: &dyn TextMetrics) -> Result<Self, BenchError> {
        let mut tree = Tree::new();
        let root = tree.root();
        let view = EditorView::insert(&mut tree, root, "main.rs", rope.clone())
            .map_err(|e| broke("building the view", e))?;
        view.set_wrap(&mut tree, wrap)
            .map_err(|e| broke("setting the wrap", e))?;
        let mut scroller = Self { tree, view };
        for _ in 0..8 {
            let drew = scroller.tree.frame(WINDOW, metrics).is_some();
            let changed = scroller
                .view
                .update(&mut scroller.tree, &[], metrics)
                .map_err(|e| broke("updating the view", e))?;
            if !drew && !changed {
                return Ok(scroller);
            }
        }
        Err(failed("the new view settles"))
    }

    /// The text area's bounds.
    fn area(&self) -> Result<Rect, BenchError> {
        self.tree
            .bounds(self.view.text_node())
            .ok_or_else(|| failed("the text area is laid out"))
    }

    /// One notch of the wheel down over the text area, handled as the main
    /// loop handles it: the event, then frames painted into `target` over
    /// `base` and updates, until the view has nothing left to draw (at
    /// most four rounds).
    fn notch(
        &mut self,
        painter: &mut Painter,
        target: &mut Framebuffer,
        base: &Framebuffer,
        metrics: &dyn TextMetrics,
    ) -> Result<Step, String> {
        let area = self
            .tree
            .bounds(self.view.text_node())
            .ok_or("the text area is not laid out")?;
        let wheel = InputEvent::Wheel {
            delta: ScrollDelta::Lines { x: 0.0, y: 1.0 },
            position: Point::new(area.x + area.width / 2.0, area.y + area.height / 2.0),
            modifiers: Modifiers::NONE,
        };
        let before = self.view.position(&self.tree).map_err(|e| e.to_string())?;
        self.view
            .handle(&mut self.tree, &wheel, metrics)
            .map_err(|e| e.to_string())?;
        let mut step = Step {
            moved: false,
            frames: 0,
            painted: 0,
            span: None,
        };
        for _ in 0..4 {
            if let Some(frame) = self.tree.frame(WINDOW, metrics) {
                step.frames += 1;
                for rect in &frame.damage {
                    let (top, bottom) = step.span.unwrap_or((rect.y, rect.bottom()));
                    step.span = Some((top.min(rect.y), bottom.max(rect.bottom())));
                }
                let painted = painter
                    .paint(target, &base.view(), &frame, Scale::ONE)
                    .map_err(|e| e.to_string())?;
                step.painted += painted
                    .rects()
                    .iter()
                    .map(|rect| rect.width.max(0) as u64 * rect.height.max(0) as u64)
                    .sum::<u64>();
            }
            let more = self
                .view
                .update(&mut self.tree, &[], metrics)
                .map_err(|e| e.to_string())?;
            if !more {
                break;
            }
        }
        step.moved = self.view.position(&self.tree).map_err(|e| e.to_string())? != before;
        Ok(step)
    }
}

/// How many notches from the top a pass takes before the next starts from
/// the top again: fewer than reach the end, so that every one moves.
///
/// From the top, the end is where the last row is at the view's bottom.
/// With `rows` rows on screen and `lines` lines, that is at least `lines -
/// rows` rows down, as a line is at least one row; each notch is
/// [`WHEEL_ROWS`] rows. One more screen is kept as a margin.
fn notches_per_pass(lines: usize, rows: usize) -> usize {
    (lines.saturating_sub(2 * rows) as f32 / WHEEL_ROWS) as usize
}

/// What the suites share: the faces and metrics, the painter, the
/// framebuffer painted into, and the base under it, the theme's `bg`.
struct Setup {
    painter: Painter,
    target: Framebuffer,
    base: Framebuffer,
}

impl Setup {
    fn new() -> Result<Self, BenchError> {
        let painter = Painter::new().map_err(|e| broke("loading the painter", e))?;
        let target = Framebuffer::new(DEVICE).map_err(|e| broke("making the window", e))?;
        let mut base = Framebuffer::new(DEVICE).map_err(|e| broke("making the base", e))?;
        let tree = Tree::new();
        base.clear(tree.theme().color(Token::Bg).premultiply());
        Ok(Self {
            painter,
            target,
            base,
        })
    }
}

/// What a whole frame draws.
#[derive(Clone, Copy, Debug)]
struct Drawn {
    /// The rows with a text on them: the texts' distinct baselines.
    rows: usize,
    /// The line numbers among the texts (`text-2` and `text-3`, which the
    /// generated files draw nothing else in), one on each line's first
    /// row.
    numbers: usize,
}

/// What a whole frame of `scroller` draws, which it paints.
fn whole_frame(
    scroller: &mut Scroller,
    setup: &mut Setup,
    metrics: &dyn TextMetrics,
) -> Result<Drawn, BenchError> {
    scroller.tree.invalidate_all();
    let frame = scroller
        .tree
        .frame(WINDOW, metrics)
        .ok_or_else(|| failed("a whole frame draws"))?;
    let theme = scroller.tree.theme();
    let numbered = [theme.color(Token::Text2), theme.color(Token::Text3)];
    let mut baselines = Vec::new();
    let mut numbers = 0;
    for command in frame.list.commands() {
        if let Command::Text { origin, color, .. } = command {
            baselines.push(origin.y.to_bits());
            numbers += usize::from(numbered.contains(color));
        }
    }
    baselines.sort_unstable();
    baselines.dedup();
    setup
        .painter
        .paint(&mut setup.target, &setup.base.view(), &frame, Scale::ONE)
        .map_err(|e| broke("painting the whole frame", e))?;
    Ok(Drawn {
        rows: baselines.len(),
        numbers,
    })
}

/// The rows a text area of `area` shows, part rows included.
fn rows_shown(area: Rect, metrics: &dyn TextMetrics) -> usize {
    let row = metrics.line(&EDITOR_TEXT).height.max(1.0);
    (area.height / row).ceil() as usize
}

/// Checks, untimed, that a view of `rope` with soft wrap `wrap` draws a
/// text on each row on screen, and with wrap on more rows than lines,
/// with it off one line a row; and that a notch scrolls three rows and
/// repaints the text area. Returns the rows on screen.
fn check_scroll(
    rope: &Rope,
    wrap: bool,
    setup: &mut Setup,
    metrics: &dyn TextMetrics,
) -> Result<usize, BenchError> {
    let mut scroller = Scroller::new(rope, wrap, metrics)?;
    let area = scroller.area()?;
    let rows = rows_shown(area, metrics);
    let drawn = whole_frame(&mut scroller, setup, metrics)?;
    check(
        drawn.rows == rows,
        "the first frame draws a text on each row on screen",
    )?;
    if wrap {
        check(
            drawn.numbers < drawn.rows,
            "with soft wrap on, the first frame draws more rows than lines: lines wrap",
        )?;
    } else {
        check(
            drawn.numbers == drawn.rows,
            "with soft wrap off, each row on screen is a line",
        )?;
    }
    // In the gutter, a point maps to its row's start.
    let height = metrics.line(&EDITOR_TEXT).height;
    let row_start = |scroller: &Scroller, row: f32| {
        let point = Point::new(area.x + 1.0, area.y + (row + 0.5) * height);
        scroller
            .view
            .point_to_byte(&scroller.tree, point, metrics)
            .map_err(|e| broke("finding a row's start", e))
    };
    let fourth = row_start(&scroller, 3.0)?;
    let step = scroller
        .notch(&mut setup.painter, &mut setup.target, &setup.base, metrics)
        .map_err(|e| broke("scrolling", e))?;
    let position = scroller
        .view
        .position(&scroller.tree)
        .map_err(|e| broke("reading the position", e))?;
    let top = row_start(&scroller, 0.0)?;
    check(
        step.moved && position.y == 0.0 && fourth > 0 && top == fourth,
        "a notch scrolls three rows: the fourth row on screen is the first after it",
    )?;
    let (top, bottom) = step.span.unwrap_or((f32::MAX, f32::MIN));
    check(
        step.frames >= 1 && step.painted > 0 && top <= area.y && bottom >= area.y + area.height,
        "a notch repaints the text area from its top row to its bottom one",
    )?;
    Ok(rows)
}

/// A scroll suite: a notch at a time down a 10,000-line file, [`source`]'s
/// with soft wrap off and [`wrapping_source`]'s with it on (the [module
/// documentation](self)).
fn scroll(b: &mut Bencher, wrap: bool) -> Result<(), BenchError> {
    let text = if wrap {
        wrapping_source(LINES)
    } else {
        source(LINES)
    };
    check(
        text.split('\n').count() == LINES,
        "the file has 10,000 lines",
    )?;
    scroll_through(b, &text, wrap, 3_000)
}

/// A notch at a time down `text`, soft wrap `wrap`, a pass being more
/// than `least` notches (the [module documentation](self)).
fn scroll_through(b: &mut Bencher, text: &str, wrap: bool, least: usize) -> Result<(), BenchError> {
    let faces = bundled::load()?;
    let metrics = FontMetrics::new(&faces, Scale::ONE);
    let rope = Rope::from(text);
    let mut setup = Setup::new()?;
    let rows = check_scroll(&rope, wrap, &mut setup, &metrics)?;
    let per_pass = notches_per_pass(rope.len_lines(), rows);
    check(per_pass > least, &format!("a pass is over {least} notches"))?;

    let mut current = Scroller::new(&rope, wrap, &metrics)?;
    let mut since_top = 0;
    let failure: RefCell<Option<String>> = RefCell::new(None);
    let stuck = Cell::new(0_usize);
    let fail = |error: String| {
        let mut first = failure.borrow_mut();
        if first.is_none() {
            *first = Some(error);
        }
    };
    let Setup {
        painter,
        target,
        base,
    } = &mut setup;
    b.iter_batched(
        || {
            since_top += 1;
            if since_top > per_pass {
                since_top = 1;
                Some(Scroller::new(&rope, wrap, &metrics))
            } else {
                None
            }
        },
        |fresh| {
            let old = match fresh {
                Some(Ok(fresh)) => Some(std::mem::replace(&mut current, fresh)),
                Some(Err(error)) => {
                    fail(error.to_string());
                    None
                }
                None => None,
            };
            match current.notch(painter, target, base, &metrics) {
                Ok(step) if step.moved => {}
                Ok(_) => stuck.set(stuck.get() + 1),
                Err(error) => fail(error),
            }
            old
        },
    );
    if let Some(error) = failure.into_inner() {
        return Err(broke("an iteration failed", error));
    }
    check(stuck.get() == 0, "every measured notch scrolled the view")
}

/// The long-line suite: the notch that brings a 10,000-byte line on
/// screen, laid out for the first time (the [module
/// documentation](self)).
fn long_line(b: &mut Bencher) -> Result<(), BenchError> {
    let faces = bundled::load()?;
    let metrics = FontMetrics::new(&faces, Scale::ONE);
    let rope = Rope::from(long_source().as_str());
    let span = rope.line_span(LONG_AT)?;
    check(
        rope.len_lines() == LONG_FILE_LINES && span.content().len() == MAX_LINE_BYTES,
        "line 53 of 120 is 10,000 bytes long",
    )?;
    let mut setup = Setup::new()?;

    // Untimed: off screen before the notch, on screen and whole after it.
    let mut scroller = Scroller::new(&rope, false, &metrics)?;
    let area = scroller.area()?;
    let rows = rows_shown(area, &metrics);
    check(
        rows <= LONG_AT && LONG_AT < rows + WHEEL_ROWS as usize,
        "the long line is just under the rows on screen, and a notch brings it on",
    )?;
    let content = |scroller: &Scroller| {
        scroller
            .tree
            .widget::<ScrollBar>(scroller.view.scroll_bar(Axis::Horizontal))
            .map(ScrollBar::content)
    };
    let narrow = content(&scroller).ok_or_else(|| failed("a horizontal scrollbar"))?;
    let step = scroller
        .notch(&mut setup.painter, &mut setup.target, &setup.base, &metrics)
        .map_err(|e| broke("scrolling", e))?;
    let shown = scroller
        .view
        .byte_to_point(&scroller.tree, span.start, &metrics)
        .map_err(|e| broke("finding the long line", e))?;
    let wide = content(&scroller).ok_or_else(|| failed("a horizontal scrollbar"))?;
    let whole = MAX_LINE_BYTES as f32 * metrics.width("e", &EDITOR_TEXT);
    check(
        step.moved && shown.is_some(),
        "the notch brings the long line on screen",
    )?;
    check(
        narrow < whole / 10.0 && wide >= whole * 0.99,
        "the long line is laid out whole: the horizontal range grows to its width",
    )?;

    let failure: RefCell<Option<String>> = RefCell::new(None);
    let fail = |error: String| {
        let mut first = failure.borrow_mut();
        if first.is_none() {
            *first = Some(error);
        }
    };
    let Setup {
        painter,
        target,
        base,
    } = &mut setup;
    b.iter_batched(
        || Scroller::new(&rope, false, &metrics),
        |fresh| {
            let mut fresh = match fresh {
                Ok(fresh) => fresh,
                Err(error) => {
                    fail(error.to_string());
                    return None;
                }
            };
            match fresh.notch(painter, target, base, &metrics) {
                Ok(step) if step.moved => {}
                Ok(_) => fail("a notch did not scroll the view".to_string()),
                Err(error) => fail(error),
            }
            Some(fresh)
        },
    );
    match failure.into_inner() {
        Some(error) => Err(broke("an iteration failed", error)),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::harness::{Settings, run_suite};

    /// `text`'s lines: how many, its bytes, the blank ones, and the
    /// longest's length.
    fn describe(text: &str) -> String {
        let lines: Vec<&str> = text.split('\n').collect();
        let blank = lines.iter().filter(|line| line.is_empty()).count();
        let longest = lines.iter().map(|line| line.len()).max().unwrap_or(0);
        format!(
            "{} lines, {} bytes, {blank} blank, longest {longest}",
            lines.len(),
            text.len()
        )
    }

    /// Settings that finish at once, for unoptimized test builds.
    const QUICK: Settings = Settings {
        warm_up: Duration::ZERO,
        sample_target: Duration::from_micros(10),
        budget: Duration::ZERO,
        min_samples: 2,
        max_samples: 2,
    };

    #[test]
    fn each_suite_checks_what_it_measures_and_measures_one_routine() {
        for suite in SUITES {
            let summary = run_suite(suite, &QUICK).expect("the suite runs");
            assert_eq!(summary.samples, 2, "{}", suite.name);
        }
    }

    #[test]
    fn the_file_is_the_same_10000_lines_of_up_to_99_bytes_every_run() {
        let text = source(LINES);
        assert_eq!(text, source(LINES), "the same on every run");
        assert!(!text.ends_with('\n'));
        // Pinned, so that a change to the generator, which would make the
        // baseline measure another file, fails here.
        assert_eq!(
            describe(&text),
            "10000 lines, 457754 bytes, 655 blank, longest 99"
        );
        // Indented code: most lines start with spaces.
        let indented = text
            .split('\n')
            .filter(|line| line.starts_with("    "))
            .count();
        assert!(indented > LINES / 2, "{indented} lines indented");
    }

    #[test]
    fn the_wrap_files_lines_are_the_others_but_a_fifth_made_250_to_600_bytes() {
        let text = wrapping_source(LINES);
        assert_eq!(text, wrapping_source(LINES), "the same on every run");
        let plain = source(LINES);
        let mut long = 0;
        for (line, other) in text.split('\n').zip(plain.split('\n')) {
            if line != other {
                long += 1;
                assert!((250..=600).contains(&line.len()), "{line}");
            }
        }
        // Pinned, as the other file is.
        assert_eq!(
            describe(&text),
            "10000 lines, 1168501 bytes, 655 blank, longest 600"
        );
        assert_eq!(long, 1_930, "about one line in five made long");
    }

    #[test]
    fn the_long_line_is_exactly_max_line_bytes_among_the_rest() {
        let text = long_source();
        let lines: Vec<&str> = text.split('\n').collect();
        assert_eq!(lines.len(), LONG_FILE_LINES);
        assert_eq!(lines[LONG_AT].len(), MAX_LINE_BYTES);
        assert!(lines[LONG_AT].ends_with("\";"));
        assert!(
            lines
                .iter()
                .enumerate()
                .all(|(at, line)| at == LONG_AT || line.len() < 100)
        );
    }

    #[test]
    fn a_pass_stops_short_of_the_end() {
        // 51 rows at 1080p with wrap off: (10,000 - 102) / 3.
        assert_eq!(notches_per_pass(LINES, 51), 3_299);
        assert_eq!(notches_per_pass(LINES, LINES), 0);
        assert_eq!(notches_per_pass(160, 51), 19);
    }

    #[test]
    fn a_scroll_starts_again_from_the_top_after_each_pass_and_every_notch_moves() {
        // 160 lines: 19 notches a pass, short of the 37 that reach the
        // end; 70 iterations, each a sample of one, make three passes and
        // start a fourth. Without the new view at the top, the twentieth
        // notch on would not move, and the suite would fail.
        let settings = Settings {
            min_samples: 69,
            max_samples: 69,
            ..QUICK
        };
        for wrap in [false, true] {
            let suite = Suite {
                name: "editor/scroll/test",
                run: if wrap {
                    |b| scroll_through(b, &wrapping_source(160), true, 10)
                } else {
                    |b| scroll_through(b, &source(160), false, 10)
                },
            };
            let summary = run_suite(&suite, &settings).expect("every notch moves");
            assert_eq!((summary.samples, summary.batch), (69, 1), "wrap {wrap}");
        }
    }
}
