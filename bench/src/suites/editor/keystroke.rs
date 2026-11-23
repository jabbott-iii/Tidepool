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

//! The keystroke suites of `tidepool-editor`, on which roadmap M4.2 (the
//! editing commands) is accepted: a keystroke at 10,000 carets on 100 MB,
//! and keystrokes on M1.1's crafted 100 MB lines, each within one 60 fps
//! frame, 16.7 ms (ADR-0065).
//!
//! - `editor/keystroke/10k_carets_100mb_1080p`: an `x` typed at 10,000
//!   carets of a 100 MB file, in an editor filling a 1920 × 1080 window at
//!   scale 1. The file is the scroll suites' 10,000-line [`source`] 219
//!   times, 100,248,344 bytes in 2,190,000 lines; the carets are at the
//!   ends of every 219th line, the first at line 0's, the primary, so the
//!   view stays at the top.
//! - `editor/crafted/<line>/<key>`: one keystroke on a crafted line of
//!   100 MB, followed by a short line, `short`, in the same window. The
//!   lines are `text`'s grapheme suites' (M1.1): `one_cluster`, a letter
//!   and 50,000,000 combining marks, with the caret in the middle;
//!   `combining_marks`, a consonant, 50,000,000 marks, a virama and a
//!   consonant, with the caret before the last consonant, where a
//!   boundary depends on every mark before it (GB9c); and
//!   `regional_indicators`, an `x` and 25,000,000 regional indicators,
//!   with the caret before the last. The keys are a cluster `right` and
//!   `left`, a word `word_right` and `word_left`, `delete_left`, `down` to
//!   the short line, and `type`, an `x`.
//!
//! Each iteration is the keystroke as the main loop handles it: the
//! command run, or the text given to [`Editor::handle`], then
//! [`Tree::frame`], [`Painter::paint`](tidepool_gui::Painter::paint) of
//! the frame's damage, and
//! [`EditorView::update`](tidepool_editor::view::EditorView::update),
//! framing and painting again while it says there is more to draw. It
//! starts from a new editor, focused, with its selection set and its first
//! frame drawn and painted, outside the timing; the old editor is the
//! iteration's output, which the harness drops outside the timing. The
//! 10,000-caret suite builds each editor over a rope of its own, built
//! outside the timing, so that the keystroke frees the 10,000 leaves its
//! edit replaced, as the editor does once nothing else holds the text
//! before; a crafted-line suite's editor is over an O(1) clone of one rope. Before it is measured, each suite
//! checks, untimed, that one keystroke does what it should.

use std::time::Instant;

use tidepool_core::geom::Scale;
use tidepool_core::input::InputEvent;
use tidepool_editor::edit::{Command, Editor, Motion};
use tidepool_editor::view::MAX_LINE_BYTES;
use tidepool_font::bundled;
use tidepool_gui::FontMetrics;
use tidepool_text::{Cursor, Rope, RopeBuilder, Selection};
use tidepool_ui::Tree;
use tidepool_ui::metrics::TextMetrics;

use super::{LINES, Setup, WINDOW, broke, check, failed, source};
use crate::{BenchError, Bencher};

/// How many times the keystroke file repeats [`source`]'s lines.
pub(super) const COPIES: usize = 219;

/// The carets of the 10,000-caret suite.
const CARETS: usize = 10_000;

/// U+0301 COMBINING ACUTE ACCENT, two bytes.
const MARK: &str = "\u{301}";

/// U+1F1E6 REGIONAL INDICATOR SYMBOL LETTER A, four bytes.
const INDICATOR: &str = "\u{1F1E6}";

/// The marks of a crafted line: 100 MB of them.
const MARKS: usize = 50_000_000;

/// The regional indicators of a crafted line: 100 MB of them.
const INDICATORS: usize = 25_000_000;

/// The line after a crafted line.
const SHORT: &str = "short";

/// A crafted 100 MB line, as `text`'s grapheme suites build it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Line {
    OneCluster,
    CombiningMarks,
    RegionalIndicators,
}

/// A keystroke a crafted-line suite measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Key {
    Right,
    Left,
    WordRight,
    WordLeft,
    DeleteLeft,
    Down,
    Type,
}

impl Key {
    /// The command it runs, or `None` for typing.
    const fn command(self) -> Option<Command> {
        let motion = match self {
            Self::Right => Motion::Right,
            Self::Left => Motion::Left,
            Self::WordRight => Motion::WordRight,
            Self::WordLeft => Motion::WordLeft,
            Self::Down => Motion::Down,
            Self::DeleteLeft => return Some(Command::DeleteLeft),
            Self::Type => return None,
        };
        Some(Command::Move {
            motion,
            select: false,
        })
    }
}

/// `head`, `count` times `unit`, then `tail`, built in chunks.
fn repeated(head: &str, unit: &str, count: usize, tail: &str) -> Rope {
    const CHUNK: usize = 16 * 1024;
    let chunk = unit.repeat(CHUNK);
    let mut builder = RopeBuilder::new();
    builder.push_str(head);
    let mut left = count;
    while left >= CHUNK {
        builder.push_str(&chunk);
        left -= CHUNK;
    }
    builder.push_str(&unit.repeat(left));
    builder.push_str(tail);
    builder.finish()
}

/// The text of `line`, scaled by `scale` (1 for 100 MB; the tests use
/// less), and where its caret goes.
fn crafted(line: Line, scale: usize) -> (Rope, usize) {
    let after = format!("\n{SHORT}");
    match line {
        Line::OneCluster => {
            let marks = MARKS / scale;
            let rope = repeated("a", MARK, marks, &after);
            (rope, 1 + MARK.len() * (marks / 2))
        }
        Line::CombiningMarks => {
            let rope = repeated(
                "\u{915}",
                MARK,
                MARKS / scale,
                &format!("\u{94D}\u{915}{after}"),
            );
            let before_last = rope.len() - after.len() - "\u{915}".len();
            (rope, before_last)
        }
        Line::RegionalIndicators => {
            let rope = repeated("x", INDICATOR, INDICATORS / scale, &after);
            let before_last = rope.len() - after.len() - INDICATOR.len();
            (rope, before_last)
        }
    }
}

/// An editor in a tree of its own, filling [`WINDOW`], focused, with its
/// first frame drawn and painted.
struct Session {
    tree: Tree,
    editor: Editor,
}

impl Session {
    fn new(
        rope: &Rope,
        selection: &Selection,
        setup: &mut Setup,
        metrics: &dyn TextMetrics,
    ) -> Result<Self, String> {
        let mut tree = Tree::new();
        let root = tree.root();
        let mut editor = Editor::insert(&mut tree, root, "main.rs", rope.clone())
            .map_err(|e| format!("building the editor: {e}"))?;
        tree.set_focus(editor.view().text_node(), false)
            .map_err(|e| format!("focusing it: {e}"))?;
        editor
            .set_selection(&mut tree, selection.clone())
            .map_err(|e| format!("selecting: {e}"))?;
        let mut session = Self { tree, editor };
        session.draw(setup, metrics)?;
        let view = session.editor.view();
        let head = selection.primary().head;
        view.scroll_into_view(&mut session.tree, head, metrics)
            .map_err(|e| format!("scrolling: {e}"))?;
        session.draw(setup, metrics)?;
        Ok(session)
    }

    /// Frames, paints and updates until the view has nothing left to draw
    /// (at most four rounds); returns the frames drawn.
    fn draw(&mut self, setup: &mut Setup, metrics: &dyn TextMetrics) -> Result<usize, String> {
        let mut frames = 0;
        for _ in 0..4 {
            if let Some(frame) = self.tree.frame(WINDOW, metrics) {
                frames += 1;
                setup
                    .painter
                    .paint(&mut setup.target, &setup.base.view(), &frame, Scale::ONE)
                    .map_err(|e| e.to_string())?;
            }
            let view = self.editor.view();
            if !view
                .update(&mut self.tree, &[], metrics)
                .map_err(|e| e.to_string())?
            {
                break;
            }
        }
        Ok(frames)
    }

    /// One keystroke, as the main loop handles it: `key`'s command, or an
    /// `x` typed; returns the frames it drew.
    fn key(
        &mut self,
        key: Key,
        now: Instant,
        setup: &mut Setup,
        metrics: &dyn TextMetrics,
    ) -> Result<usize, String> {
        match key.command() {
            Some(command) => {
                self.editor
                    .run(&mut self.tree, command, now, metrics)
                    .map_err(|e| e.to_string())?;
            }
            None => {
                self.editor
                    .handle(
                        &mut self.tree,
                        &InputEvent::Text("x".to_string()),
                        now,
                        metrics,
                    )
                    .map_err(|e| e.to_string())?;
            }
        }
        self.draw(setup, metrics)
    }

    fn heads(&self) -> Vec<usize> {
        self.editor
            .selection(&self.tree)
            .map(|selection| selection.cursors().iter().map(|c| c.head).collect())
            .unwrap_or_default()
    }
}

/// Measures `key` on the text `text` makes, from `selection`, after
/// `checked` says, untimed, that one keystroke from there does what it
/// should.
fn measure(
    b: &mut Bencher,
    text: impl Fn() -> Rope,
    selection: &Selection,
    key: Key,
    checked: impl Fn(&Session) -> Result<(), BenchError>,
) -> Result<(), BenchError> {
    let faces = bundled::load()?;
    let metrics = FontMetrics::new(&faces, Scale::ONE);
    let mut setup = Setup::new()?;
    let now = Instant::now();
    let mut probe = Session::new(&text(), selection, &mut setup, &metrics)
        .map_err(|e| broke("setting up", e))?;
    // A keystroke that changes nothing on screen, such as a caret moving
    // inside a cluster the view shows whole, draws no frame.
    probe
        .key(key, now, &mut setup, &metrics)
        .map_err(|e| broke("a keystroke", e))?;
    checked(&probe)?;
    drop(probe);

    let mut failure: Option<String> = None;
    let setup = std::cell::RefCell::new(setup);
    b.iter_batched(
        || Session::new(&text(), selection, &mut setup.borrow_mut(), &metrics),
        |session| {
            let mut session = match session {
                Ok(session) => session,
                Err(error) => {
                    failure.get_or_insert(error);
                    return None;
                }
            };
            if let Err(error) = session.key(key, now, &mut setup.borrow_mut(), &metrics) {
                failure.get_or_insert(error);
            }
            Some(session)
        },
    );
    match failure {
        Some(error) => Err(broke("an iteration failed", error)),
        None => Ok(()),
    }
}

/// The keystroke file: [`source`]'s lines `copies` times, joined by line
/// feeds; [`COPIES`] times for the suite's 100 MB.
fn big_source(copies: usize) -> String {
    let lines = source(LINES);
    let mut text = String::with_capacity(copies * (lines.len() + 1));
    for copy in 0..copies {
        text.push_str(&lines);
        text.push_str(if copy + 1 < copies { "\n" } else { "" });
    }
    text
}

/// [`CARETS`] carets at the ends of every `step`th line of `rope`, the
/// first primary.
fn caret_ends(rope: &Rope, step: usize) -> Result<Selection, BenchError> {
    let mut carets = Vec::with_capacity(CARETS);
    for k in 0..CARETS {
        carets.push(Cursor::caret(rope.line_span(k * step)?.content_end));
    }
    let primary = carets.first().copied().unwrap_or(Cursor::caret(0));
    Ok(Selection::new(primary, carets))
}

/// The 10,000-caret suite (the [module documentation](self)), on the file
/// of `copies` copies; [`COPIES`] for the suite.
pub(super) fn ten_thousand_carets(b: &mut Bencher, copies: usize) -> Result<(), BenchError> {
    let text = big_source(copies);
    let rope = Rope::from(text.as_str());
    check(
        rope.len_lines() == LINES * copies && (copies != COPIES || rope.len() == 100_248_344),
        "the file is 10,000 lines a copy, 100,248,344 bytes in 219",
    )?;
    let selection = caret_ends(&rope, copies)?;
    check(
        selection.cursors().len() == CARETS,
        "10,000 distinct carets",
    )?;
    let heads: Vec<usize> = selection.cursors().iter().map(|c| c.head).collect();
    let len = rope.len();
    drop(rope);
    // A rope of its own each time, shared with nothing outside the editor,
    // so that the keystroke frees the leaves its edit replaced, as the
    // editor's does.
    let own = || Rope::from(text.as_str());
    measure(b, own, &selection, Key::Type, |session| {
        let moved: Vec<usize> = heads.iter().enumerate().map(|(k, h)| h + k + 1).collect();
        check(
            session.editor.text().len() == len + CARETS && session.heads() == moved,
            "an x is typed at every caret, each moving past its own",
        )
    })
}

/// A crafted-line suite (the [module documentation](self)), its text built
/// at `1 / scale` of its size.
pub(super) fn crafted_line(
    b: &mut Bencher,
    line: Line,
    key: Key,
    scale: usize,
) -> Result<(), BenchError> {
    let (rope, at) = crafted(line, scale);
    let selection = Selection::caret(at);
    let len = rope.len();
    let short_start = len - SHORT.len();
    // Each keystroke reads at most the window either side, rounded to a
    // char boundary.
    let reach = MAX_LINE_BYTES + 4;
    measure(
        b,
        || rope.clone(),
        &selection,
        key,
        |session| {
            let head = session.heads().first().copied().unwrap_or(at);
            let text = session.editor.text().len();
            let holds = match key {
                Key::Right | Key::WordRight => head > at && head - at <= reach,
                Key::Left | Key::WordLeft => head < at && at - head <= reach,
                Key::DeleteLeft => head < at && text < len && len - text <= reach,
                Key::Down => {
                    head == short_start
                        + match line {
                            // Past the cut, the column is the cut's, which is 0 for a
                            // line that is one cluster; the indicators' pairs fill it.
                            Line::OneCluster | Line::CombiningMarks => 0,
                            Line::RegionalIndicators => SHORT.len(),
                        }
                }
                Key::Type => text == len + 1 && head == at + 1,
            };
            if holds {
                Ok(())
            } else {
                Err(failed(&format!(
                    "{key:?} on {line:?} from {at} went to {head}, the text {len} bytes to {text}"
                )))
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_crafted_lines_are_text_s_and_their_carets_are_char_boundaries() {
        for line in [
            Line::OneCluster,
            Line::CombiningMarks,
            Line::RegionalIndicators,
        ] {
            let (rope, at) = crafted(line, 1_000);
            assert!(rope.is_char_boundary(at), "{line:?}");
            assert_eq!(rope.len_lines(), 2);
            assert_eq!(
                rope.line_span(1).expect("a short line").content().len(),
                SHORT.len()
            );
        }
        // At full size, each is 100 MB and a few bytes.
        assert_eq!(MARKS * MARK.len(), 100_000_000);
        assert_eq!(INDICATORS * INDICATOR.len(), 100_000_000);
    }
}
