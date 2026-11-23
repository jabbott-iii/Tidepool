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

//! The editing commands, headless (M4.2, ADR-0065): motions by cluster,
//! word, line and page with goal columns; typing, deleting, new lines,
//! indenting and outdenting; multi-cursor editing; undo and redo, one
//! command one step and typing coalesced; the clipboard's requests and
//! pastes; the mouse; Tab kept from the tree; motions bounded on crafted
//! long lines; and a property test that random commands keep the
//! selection in the text and undo takes it back to where it started.

use std::time::{Duration, Instant};

use tidepool_core::geom::{Point, Size};
use tidepool_core::input::{InputEvent, Key, Modifiers, MouseButton};
use tidepool_core::rng::Pcg32;
use tidepool_core::testing::check_sized;
use tidepool_editor::command::is_valid_id;
use tidepool_editor::edit::{
    Command, DOUBLE_CLICK, Editor, INDENT_SPACES, Motion, Outcome, Request, TYPING_WINDOW,
};
use tidepool_editor::view::{GUTTER_PADDING, MAX_LINE_BYTES};
use tidepool_text::{Cursor, LineBreak, Rope, Selection};
use tidepool_ui::Tree;
use tidepool_ui::layout::Axis;
use tidepool_ui::metrics::FixedMetrics;

/// 7 units an advance and rows 21 high, so that every position is exact.
const METRICS: FixedMetrics = FixedMetrics {
    advance: 0.5,
    ascent: 0.75,
    descent: 0.25,
};

/// A character's advance in [`METRICS`] at 14 px.
const ADVANCE: f32 = 7.0;

/// A row's height in [`METRICS`].
const ROW: f32 = 21.0;

/// A text area of 390 × 210 beside and above the scrollbars: ten rows.
const WINDOW: Size = Size::new(400.0, 220.0);

/// The gutter at three digits.
const GUTTER: f32 = 2.0 * GUTTER_PADDING + 3.0 * ADVANCE;

/// An editor in a tree, at a time the test moves on.
struct Ed {
    tree: Tree,
    editor: Editor,
    now: Instant,
}

/// An editor over `text`, laid out, with keyboard focus.
fn ed(text: &str) -> Ed {
    let mut tree = Tree::new();
    let root = tree.root();
    let editor = Editor::insert(&mut tree, root, "main.rs", Rope::from(text)).expect("an editor");
    tree.set_focus(editor.view().text_node(), false)
        .expect("focusable");
    let mut ed = Ed {
        tree,
        editor,
        now: Instant::now(),
    };
    ed.settle();
    ed
}

impl Ed {
    /// Draws frames and updates the view, as the main loop does.
    fn settle(&mut self) {
        for _ in 0..8 {
            let drew = self.tree.frame(WINDOW, &METRICS).is_some();
            let view = self.editor.view();
            let changed = view.update(&mut self.tree, &[], &METRICS).expect("update");
            if !drew && !changed {
                return;
            }
        }
        panic!("the view never settled");
    }

    fn run(&mut self, command: Command) -> Outcome {
        let outcome = self
            .editor
            .run(&mut self.tree, command, self.now, &METRICS)
            .unwrap_or_else(|e| panic!("{command:?}: {e}"));
        self.settle();
        outcome
    }

    fn go(&mut self, motion: Motion) {
        self.run(Command::Move {
            motion,
            select: false,
        });
    }

    fn extend(&mut self, motion: Motion) {
        self.run(Command::Move {
            motion,
            select: true,
        });
    }

    fn typed(&mut self, typed: &str) {
        self.editor
            .type_text(&mut self.tree, typed, self.now, &METRICS)
            .expect("typed");
        self.settle();
    }

    fn paste(&mut self, pasted: &str) {
        self.editor
            .paste(&mut self.tree, pasted, self.now, &METRICS)
            .expect("pasted");
        self.settle();
    }

    fn event(&mut self, event: InputEvent) -> bool {
        let response = self
            .editor
            .handle(&mut self.tree, &event, self.now, &METRICS)
            .expect("handled");
        self.settle();
        response.handled
    }

    fn text(&self) -> String {
        self.editor.text().to_string()
    }

    /// Each cursor as (anchor, head), and which is primary.
    fn cursors(&self) -> (Vec<(usize, usize)>, usize) {
        let selection = self.editor.selection(&self.tree).expect("a selection");
        let cursors = selection
            .cursors()
            .iter()
            .map(|cursor| (cursor.anchor, cursor.head))
            .collect();
        (cursors, selection.primary_index())
    }

    /// The heads, for carets.
    fn heads(&self) -> Vec<usize> {
        self.cursors().0.into_iter().map(|(_, head)| head).collect()
    }

    /// Sets the cursors, the first primary.
    fn set(&mut self, cursors: &[(usize, usize)]) {
        let all: Vec<Cursor> = cursors.iter().map(|&(a, h)| Cursor::new(a, h)).collect();
        let selection = Selection::new(all[0], all.iter().copied());
        self.editor
            .set_selection(&mut self.tree, selection)
            .expect("a valid selection");
        self.settle();
    }

    fn caret(&mut self, at: usize) {
        self.set(&[(at, at)]);
    }

    fn later(&mut self, by: Duration) {
        self.now += by;
    }

    /// The point in the window over column `column` of row `row`.
    fn point(column: usize, row: usize) -> Point {
        Point::new(
            GUTTER + column as f32 * ADVANCE + 1.0,
            row as f32 * ROW + ROW / 2.0,
        )
    }

    fn press(&mut self, at: Point, modifiers: Modifiers) {
        self.event(InputEvent::MouseDown {
            button: MouseButton::Left,
            position: at,
            modifiers,
        });
        self.event(InputEvent::MouseUp {
            button: MouseButton::Left,
            position: at,
            modifiers,
        });
    }
}

#[test]
fn the_commands_have_valid_distinct_ids_and_titles() {
    let mut ids = Vec::new();
    let mut titles = Vec::new();
    for command in Command::ALL {
        assert!(is_valid_id(command.id()), "{command:?}");
        assert!(command.id().starts_with("editor."), "{command:?}");
        assert_eq!(Command::from_id(command.id()), Some(command));
        ids.push(command.id());
        titles.push(command.title());
    }
    ids.sort_unstable();
    ids.dedup();
    titles.sort_unstable();
    titles.dedup();
    assert_eq!(ids.len(), 40);
    assert_eq!(titles.len(), 40);
    assert_eq!(Command::from_id("editor.nothing"), None);
    assert_eq!(Command::from_id("view.toggleSidePanel"), None);
}

#[test]
fn left_and_right_move_by_whole_clusters_and_collapse_a_selection() {
    // a, é as e and a combining accent, b, a CR LF, c.
    let mut ed = ed("ae\u{301}b\r\nc");
    let stops = [0, 1, 4, 5, 7, 8];
    for pair in stops.windows(2) {
        ed.go(Motion::Right);
        assert_eq!(ed.heads(), [pair[1]], "right of {}", pair[0]);
    }
    ed.go(Motion::Right);
    assert_eq!(ed.heads(), [8], "the end stays");
    for pair in stops.windows(2).rev() {
        ed.go(Motion::Left);
        assert_eq!(ed.heads(), [pair[0]], "left of {}", pair[1]);
    }
    ed.go(Motion::Left);
    assert_eq!(ed.heads(), [0], "the start stays");

    ed.extend(Motion::Right);
    ed.extend(Motion::Right);
    assert_eq!(ed.cursors().0, [(0, 4)]);
    ed.extend(Motion::Left);
    assert_eq!(ed.cursors().0, [(0, 1)]);
    ed.set(&[(1, 4)]);
    ed.go(Motion::Left);
    assert_eq!(ed.cursors().0, [(1, 1)], "to the selection's start");
    ed.set(&[(4, 1)]);
    ed.go(Motion::Right);
    assert_eq!(ed.cursors().0, [(4, 4)], "to the selection's end");
}

#[test]
fn word_motions_stop_at_runs_of_one_class_and_line_ends() {
    let mut ed = ed("  let x = foo.bar(1);\nnext");
    for to in [5, 7, 9, 13, 14, 17, 18, 19, 21, 22, 26, 26] {
        ed.go(Motion::WordRight);
        assert_eq!(ed.heads(), [to]);
    }
    for to in [22, 21, 19, 18, 17, 14, 13, 10, 8, 6, 2, 0, 0] {
        ed.go(Motion::WordLeft);
        assert_eq!(ed.heads(), [to]);
    }
    ed.extend(Motion::WordRight);
    assert_eq!(ed.cursors().0, [(0, 5)]);
}

#[test]
fn home_goes_to_the_indentation_then_the_start_and_end_to_the_end() {
    let mut ed = ed("    let x;\nnext");
    ed.caret(6);
    ed.go(Motion::Home);
    assert_eq!(ed.heads(), [4]);
    ed.go(Motion::Home);
    assert_eq!(ed.heads(), [0]);
    ed.go(Motion::Home);
    assert_eq!(ed.heads(), [4]);
    ed.go(Motion::End);
    assert_eq!(ed.heads(), [10], "before the break");
    ed.extend(Motion::Home);
    assert_eq!(ed.cursors().0, [(10, 4)]);
    ed.go(Motion::Bottom);
    assert_eq!(ed.heads(), [15]);
    ed.extend(Motion::Top);
    assert_eq!(ed.cursors().0, [(15, 0)]);
    ed.go(Motion::Bottom);
    ed.go(Motion::Top);
    assert_eq!(ed.heads(), [0]);
}

#[test]
fn up_and_down_keep_a_goal_column_over_short_lines_and_tabs() {
    // Lines at 0, 7, 10 and 13: "abcdef", "ab", a tab then "x", "abcdefgh".
    let mut ed = ed("abcdef\nab\n\tx\nabcdefgh");
    ed.caret(5);
    for to in [9, 12, 18] {
        ed.go(Motion::Down);
        assert_eq!(ed.heads(), [to], "down");
    }
    for to in [12, 9, 5, 0] {
        ed.go(Motion::Up);
        assert_eq!(ed.heads(), [to], "up");
    }
    // The goal stays 5 while the motions are vertical.
    ed.go(Motion::Down);
    assert_eq!(ed.heads(), [9]);
    ed.caret(0);
    for to in [7, 10, 13, 21] {
        ed.go(Motion::Down);
        assert_eq!(ed.heads(), [to], "down from column 0");
    }
    // A move across forgets the goal.
    ed.caret(18);
    ed.go(Motion::Left);
    ed.go(Motion::Up);
    assert_eq!(ed.heads(), [11], "column 4, after the tab");
    // A tab's middle is not a place: the cursor stays before it.
    ed.caret(15);
    ed.go(Motion::Up);
    assert_eq!(ed.heads(), [10]);
    ed.extend(Motion::Down);
    assert_eq!(ed.cursors().0, [(10, 15)], "back to column 2, selecting");
}

#[test]
fn page_up_and_down_move_the_rows_that_fit_and_scroll() {
    let text: String = (0..100).map(|n| format!("line {n}\n")).collect();
    let mut ed = ed(&text);
    let line = |n: usize| {
        text.lines()
            .take(n)
            .map(|line| line.len() + 1)
            .sum::<usize>()
    };
    ed.go(Motion::PageDown);
    assert_eq!(ed.heads(), [line(10)], "ten rows fit");
    ed.go(Motion::PageDown);
    assert_eq!(ed.heads(), [line(20)]);
    let view = ed.editor.view();
    assert!(
        view.position(&ed.tree).expect("a position").line > 10,
        "scrolled to it"
    );
    ed.extend(Motion::PageUp);
    assert_eq!(ed.cursors().0, [(line(20), line(10))]);
    ed.go(Motion::PageUp);
    ed.go(Motion::PageUp);
    assert_eq!(ed.heads(), [0]);
    assert_eq!(view.position(&ed.tree).expect("a position").line, 0);
}

#[test]
fn typing_replaces_every_selection_and_coalesces_a_word_at_a_time() {
    let mut ed = ed("");
    for c in ["h", "e", "l", "l", "o", " ", "w", "o"] {
        ed.typed(c);
    }
    assert_eq!(ed.text(), "hello wo");
    ed.run(Command::Undo);
    assert_eq!(ed.text(), "hello", "the word after the space");
    ed.run(Command::Undo);
    assert_eq!(ed.text(), "");
    ed.run(Command::Redo);
    ed.run(Command::Redo);
    assert_eq!(ed.text(), "hello wo");

    // A pause longer than the window starts a new step.
    ed.typed("r");
    ed.later(TYPING_WINDOW + Duration::from_millis(1));
    ed.typed("l");
    ed.run(Command::Undo);
    assert_eq!(ed.text(), "hello wor");
    // So does a move.
    ed.go(Motion::Left);
    ed.typed("x");
    ed.run(Command::Undo);
    assert_eq!(ed.text(), "hello wor");

    // Typing replaces a selection; control characters type nothing.
    ed.set(&[(0, 5)]);
    ed.typed("bye\u{7}");
    assert_eq!(ed.text(), "bye wor");
    assert_eq!(ed.heads(), [3]);
    ed.typed("\u{1b}\r\t");
    assert_eq!(ed.text(), "bye wor");
}

#[test]
fn deleting_takes_whole_clusters_and_breaks_and_coalesces() {
    let mut ed = ed("ab\r\nxe\u{301}y");
    ed.caret(8);
    ed.run(Command::DeleteLeft);
    assert_eq!(ed.text(), "ab\r\nxy", "the accent with its letter");
    ed.run(Command::DeleteLeft);
    ed.run(Command::DeleteLeft);
    assert_eq!(ed.text(), "aby", "the CR LF as one");
    ed.run(Command::Undo);
    assert_eq!(ed.text(), "ab\r\nxe\u{301}y", "one step");
    assert_eq!(ed.heads(), [8]);
    ed.caret(2);
    ed.run(Command::DeleteRight);
    assert_eq!(ed.text(), "abxe\u{301}y");
    ed.run(Command::DeleteRight);
    assert_eq!(ed.text(), "abe\u{301}y");
    ed.run(Command::DeleteRight);
    assert_eq!(ed.text(), "aby");
    ed.run(Command::Undo);
    assert_eq!(ed.text(), "ab\r\nxe\u{301}y");
    // A selection goes whole, as its own step, which the next deletion
    // does not join.
    ed.set(&[(5, 9)]);
    ed.run(Command::DeleteLeft);
    assert_eq!(ed.text(), "ab\r\nx");
    ed.run(Command::DeleteLeft);
    assert_eq!(ed.text(), "ab\r\n");
    ed.run(Command::Undo);
    assert_eq!(ed.text(), "ab\r\nx");
    ed.run(Command::Undo);
    assert_eq!(ed.text(), "ab\r\nxe\u{301}y");
}

#[test]
fn word_deletion_goes_where_the_word_motions_go() {
    let mut ed = ed("foo.bar baz");
    ed.caret(11);
    ed.run(Command::DeleteWordLeft);
    assert_eq!(ed.text(), "foo.bar ");
    ed.run(Command::DeleteWordLeft);
    assert_eq!(ed.text(), "foo.");
    ed.run(Command::Undo);
    assert_eq!(ed.text(), "foo.bar ", "each its own step");
    ed.caret(0);
    ed.run(Command::DeleteWordRight);
    assert_eq!(ed.text(), ".bar ");
}

#[test]
fn a_new_line_keeps_the_indentation_and_the_text_s_line_break() {
    let mut ed = ed("    if x {");
    ed.go(Motion::End);
    ed.run(Command::NewLine);
    assert_eq!(ed.text(), "    if x {\n    ");
    assert_eq!(ed.heads(), [15]);
    let mut crlf = self::ed("a\r\n  b");
    assert_eq!(crlf.editor.line_break(), LineBreak::CrLf);
    crlf.go(Motion::Bottom);
    crlf.run(Command::NewLine);
    assert_eq!(crlf.text(), "a\r\n  b\r\n  ");
    // Inside the indentation, only what is before the cursor.
    let mut inside = self::ed("    x");
    inside.caret(2);
    inside.run(Command::NewLine);
    assert_eq!(inside.text(), "  \n    x");
    inside.editor.set_line_break(LineBreak::Cr);
    inside.run(Command::NewLine);
    assert_eq!(inside.text(), "  \n  \r    x");
}

#[test]
fn tab_indents_to_the_next_stop_and_lines_of_a_selection() {
    let mut ed = ed("ab");
    ed.caret(1);
    ed.run(Command::Indent);
    assert_eq!(ed.text(), "a   b");
    ed.run(Command::Indent);
    assert_eq!(ed.text(), "a       b");
    assert_eq!(ed.heads(), [8]);

    let mut lines = self::ed("one\n\ntwo\nthree");
    lines.set(&[(0, 9)]);
    lines.run(Command::Indent);
    assert_eq!(
        lines.text(),
        "    one\n\n    two\nthree",
        "not the empty line, nor the line the selection ends at the start of"
    );
    assert_eq!(lines.cursors().0, [(4, 17)]);
    lines.run(Command::Undo);
    assert_eq!(lines.text(), "one\n\ntwo\nthree");
    // A selection on one line is replaced.
    lines.set(&[(1, 3)]);
    lines.run(Command::Indent);
    assert_eq!(lines.text(), "o   \n\ntwo\nthree");
}

#[test]
fn shift_tab_outdents_a_tab_or_up_to_four_spaces_from_each_line() {
    let mut ed = ed("    one\n\ttwo\n  three\nfour\n      five");
    ed.set(&[(0, 26)]);
    ed.run(Command::Outdent);
    assert_eq!(ed.text(), "one\ntwo\nthree\nfour\n      five");
    ed.caret(26);
    ed.run(Command::Outdent);
    assert_eq!(ed.text(), "one\ntwo\nthree\nfour\n  five");
    assert_eq!(ed.heads(), [22]);
    assert_eq!(INDENT_SPACES, 4);
}

#[test]
fn every_cursor_edits_and_cursors_are_added_at_their_goal_column() {
    let mut ed = ed("abcdef\nab\nabcdef");
    ed.caret(5);
    ed.run(Command::AddCursorBelow);
    assert_eq!(
        ed.cursors(),
        (vec![(5, 5), (9, 9)], 1),
        "the new one primary"
    );
    ed.run(Command::AddCursorBelow);
    assert_eq!(ed.heads(), [5, 9, 15], "column 5 kept over the short line");
    ed.run(Command::AddCursorBelow);
    assert_eq!(ed.heads(), [5, 9, 15], "no line below");
    ed.typed("X");
    assert_eq!(ed.text(), "abcdeXf\nabX\nabcdeXf");
    ed.run(Command::DeleteLeft);
    assert_eq!(ed.text(), "abcdef\nab\nabcdef");
    // The edits forgot the goals: each cursor's column is its own again.
    ed.go(Motion::Up);
    assert_eq!(
        ed.heads(),
        [0, 2, 9],
        "every cursor moves, the first to the start"
    );
    ed.run(Command::Cancel);
    assert_eq!(ed.cursors(), (vec![(9, 9)], 0), "the primary stays");
    ed.run(Command::AddCursorAbove);
    assert_eq!(ed.heads(), [2, 9]);
    ed.run(Command::Cancel);
    ed.extend(Motion::Right);
    ed.run(Command::Cancel);
    assert_eq!(
        ed.cursors().0,
        [(3, 3)],
        "one cursor: the selection collapses to its head"
    );
    assert!(!ed.run(Command::Cancel).changed, "then nothing to cancel");
}

#[test]
fn select_all_selects_the_whole_text() {
    let mut ed = ed("one\ntwo");
    ed.run(Command::SelectAll);
    assert_eq!(ed.cursors().0, [(0, 7)]);
    ed.typed("x");
    assert_eq!(ed.text(), "x");
}

#[test]
fn undo_and_redo_restore_the_text_and_the_selection_of_their_step() {
    let mut ed = ed("abc");
    ed.set(&[(0, 3)]);
    ed.typed("x");
    assert_eq!((ed.text().as_str(), ed.heads()), ("x", vec![1]));
    assert!(ed.run(Command::Undo).changed);
    assert_eq!(ed.text(), "abc");
    assert_eq!(ed.cursors().0, [(0, 3)]);
    assert!(ed.run(Command::Redo).changed);
    assert_eq!((ed.text().as_str(), ed.heads()), ("x", vec![1]));
    assert!(!ed.run(Command::Redo).changed, "nothing to redo");
    ed.run(Command::Undo);
    assert!(!ed.run(Command::Undo).changed, "at the root");
    assert!(!ed.editor.history().is_modified());
}

#[test]
fn copy_and_cut_ask_for_the_selections_or_whole_lines() {
    let copy = |ed: &mut Ed| match ed.run(Command::Copy).request {
        Some(Request::Copy(text)) => text.to_string(),
        other => panic!("{other:?}"),
    };
    let mut ed = ed("one\ntwo");
    ed.set(&[(0, 3)]);
    assert_eq!(copy(&mut ed), "one");
    assert_eq!(ed.text(), "one\ntwo", "copying changes nothing");
    let cut = ed.run(Command::Cut);
    assert_eq!(cut.request, Some(Request::Copy(Rope::from("one"))));
    assert_eq!(ed.text(), "\ntwo");
    ed.run(Command::Undo);

    // Carets take their whole lines, a last line with the line break.
    ed.caret(5);
    assert_eq!(copy(&mut ed), "two\n");
    ed.caret(1);
    ed.paste("two\n");
    assert_eq!(
        ed.text(),
        "two\none\ntwo",
        "a line copy goes above the line"
    );
    assert_eq!(ed.heads(), [5], "the cursor stays on its line");
    ed.paste("tw");
    assert_eq!(
        ed.text(),
        "two\notwne\ntwo",
        "other text goes at the cursor"
    );

    let mut lines = self::ed("a\nb\nc");
    lines.caret(2);
    let cut = lines.run(Command::Cut);
    assert_eq!(cut.request, Some(Request::Copy(Rope::from("b\n"))));
    assert_eq!((lines.text().as_str(), lines.heads()), ("a\nc", vec![2]));
    lines.caret(3);
    lines.run(Command::Cut);
    assert_eq!(lines.text(), "a", "the last line takes the break before it");
    // Two carets on two lines, the last without a break.
    let mut two = self::ed("a\nb");
    two.set(&[(0, 0), (3, 3)]);
    assert_eq!(copy(&mut two), "a\nb\n");
    two.run(Command::Cut);
    assert_eq!(two.text(), "");
}

#[test]
fn several_cursors_copy_their_selections_and_a_paste_spreads_its_lines() {
    let mut ed = ed("ab cd ef");
    ed.set(&[(0, 2), (3, 5), (6, 6)]);
    let copied = ed.run(Command::Copy).request;
    assert_eq!(
        copied,
        Some(Request::Copy(Rope::from("ab\ncd"))),
        "carets copy nothing"
    );
    let mut spread = self::ed("1 2");
    spread.set(&[(1, 1), (3, 3)]);
    spread.paste("ab\ncd");
    assert_eq!(spread.text(), "1ab 2cd", "a line for each cursor");
    spread.paste("x\ny\n");
    assert_eq!(spread.text(), "1abx 2cdy", "a final break makes no line");
    spread.paste("z");
    assert_eq!(spread.text(), "1abxz 2cdyz", "one line for every cursor");
    spread.paste("1\n2\n3");
    assert_eq!(
        spread.text(),
        "1abxz1\n2\n3 2cdyz1\n2\n3",
        "too many lines: whole"
    );
    assert_eq!(spread.run(Command::Paste).request, Some(Request::Paste));
    let before = spread.text();
    assert!(
        !spread
            .editor
            .paste(&mut spread.tree, "", spread.now, &METRICS)
            .expect("ok")
            .changed
    );
    assert_eq!(spread.text(), before);
}

#[test]
fn a_click_places_the_caret_and_shift_alt_drag_and_double_click_select() {
    let mut ed = ed("hello world\nsecond line");
    ed.press(Ed::point(2, 0), Modifiers::NONE);
    assert_eq!(ed.cursors().0, [(2, 2)]);
    ed.press(Ed::point(8, 0), Modifiers::SHIFT);
    assert_eq!(ed.cursors().0, [(2, 8)]);
    ed.later(DOUBLE_CLICK * 2);
    ed.press(Ed::point(3, 1), Modifiers::ALT);
    assert_eq!(ed.cursors(), (vec![(2, 8), (15, 15)], 1));

    ed.later(DOUBLE_CLICK * 2);
    let down = InputEvent::MouseDown {
        button: MouseButton::Left,
        position: Ed::point(1, 0),
        modifiers: Modifiers::NONE,
    };
    assert!(ed.event(down));
    assert!(ed.event(InputEvent::MouseMove {
        position: Ed::point(5, 0),
        modifiers: Modifiers::NONE,
    }));
    assert_eq!(ed.cursors().0, [(1, 5)], "a drag selects");
    ed.event(InputEvent::MouseUp {
        button: MouseButton::Left,
        position: Ed::point(5, 0),
        modifiers: Modifiers::NONE,
    });
    assert!(!ed.event(InputEvent::MouseMove {
        position: Ed::point(9, 0),
        modifiers: Modifiers::NONE,
    }));
    assert_eq!(ed.cursors().0, [(1, 5)], "until the button goes up");

    ed.later(DOUBLE_CLICK * 2);
    ed.press(Ed::point(7, 0), Modifiers::NONE);
    ed.later(DOUBLE_CLICK / 2);
    ed.press(Ed::point(7, 0), Modifiers::NONE);
    assert_eq!(ed.cursors().0, [(6, 11)], "a double click selects the word");
    ed.press(Ed::point(7, 0), Modifiers::NONE);
    assert_eq!(ed.cursors().0, [(7, 7)], "a third press starts again");
    ed.later(DOUBLE_CLICK + Duration::from_millis(1));
    ed.press(Ed::point(7, 0), Modifiers::NONE);
    assert_eq!(ed.cursors().0, [(7, 7)], "too slow for a double click");

    // A press on the scrollbar is the scrollbar's.
    let bar = ed.editor.view().scroll_bar(Axis::Vertical);
    let bounds = ed.tree.bounds(bar).expect("laid out");
    ed.press(Point::new(bounds.x + 2.0, bounds.y + 2.0), Modifiers::NONE);
    assert_eq!(ed.cursors().0, [(7, 7)]);
}

#[test]
fn an_edit_during_a_drag_ends_the_drag() {
    // A press far into the text starts a drag; a cut while the button is
    // held makes its anchor lie past the end.
    let mut ed = ed("hello world\nsecond line\nthird");
    let down = InputEvent::MouseDown {
        button: MouseButton::Left,
        position: Ed::point(4, 2),
        modifiers: Modifiers::NONE,
    };
    assert!(ed.event(down));
    ed.run(Command::SelectAll);
    ed.run(Command::Cut);
    assert_eq!(ed.text(), "");
    let moved = ed
        .editor
        .handle(
            &mut ed.tree,
            &InputEvent::MouseMove {
                position: Ed::point(2, 0),
                modifiers: Modifiers::NONE,
            },
            ed.now,
            &METRICS,
        )
        .expect("a move after the edit is no error");
    assert!(!moved.handled, "the drag ended with the step");
    ed.settle();
    let selection = ed.editor.selection(&ed.tree).expect("a selection").clone();
    selection
        .validate(ed.editor.text())
        .expect("a valid selection");
    // The press before the step makes no double click with one after it.
    ed.typed("ab");
    ed.press(Ed::point(1, 0), Modifiers::NONE);
    ed.typed("c");
    ed.press(Ed::point(1, 0), Modifiers::NONE);
    assert_eq!(ed.cursors().0, [(1, 1)]);
}

#[test]
fn typed_text_and_tab_reach_the_editor_only_with_focus() {
    let mut ed = ed("ab");
    let tab = |modifiers| InputEvent::KeyDown {
        key: Key::Tab,
        modifiers,
        repeat: false,
    };
    let text_node = ed.editor.view().text_node();
    assert!(!ed.event(tab(Modifiers::NONE)), "for the owner's keymap");
    assert!(!ed.event(tab(Modifiers::SHIFT)));
    assert_eq!(ed.tree.focused(), Some(text_node), "focus stays");
    assert!(ed.event(InputEvent::Text("x".to_string())));
    assert_eq!(ed.text(), "xab");
    ed.tree.clear_focus();
    assert!(!ed.event(InputEvent::Text("y".to_string())));
    assert_eq!(ed.text(), "xab", "no focus, no typing");
    ed.event(tab(Modifiers::NONE));
    assert_eq!(
        ed.tree.focused(),
        Some(text_node),
        "Tab from no focus moves it here"
    );
}

#[test]
fn every_command_runs_on_an_empty_text_and_at_the_ends() {
    for text in ["", "x", "\n", "a\r\nb"] {
        for at in [0, text.len()] {
            for command in Command::ALL {
                let mut ed = ed(text);
                ed.caret(at);
                ed.run(command);
                let selection = ed.editor.selection(&ed.tree).expect("a selection").clone();
                selection
                    .validate(ed.editor.text())
                    .unwrap_or_else(|e| panic!("{command:?} on {text:?} at {at}: {e}"));
            }
        }
    }
}

#[test]
fn a_thousand_carets_type_and_undo_together() {
    let text: String = (0..1_000).map(|n| format!("line {n}\n")).collect();
    let mut ed = ed(&text);
    let ends: Vec<(usize, usize)> = text.match_indices('\n').map(|(at, _)| (at, at)).collect();
    ed.set(&ends);
    ed.typed("x");
    ed.typed("y");
    let expected: String = (0..1_000).map(|n| format!("line {n}xy\n")).collect();
    assert_eq!(ed.text(), expected);
    ed.run(Command::Undo);
    assert_eq!(ed.text(), text, "one step");
    assert_eq!(ed.heads().len(), 1_000);
}

// ---------------------------------------------------------------------------
// Long lines.

/// U+0301 COMBINING ACUTE ACCENT, two bytes.
const MARK: &str = "\u{301}";

/// U+1F1E6 REGIONAL INDICATOR SYMBOL LETTER A, four bytes.
const INDICATOR: &str = "\u{1F1E6}";

#[test]
fn on_a_crafted_long_line_every_motion_reads_a_bounded_window() {
    // One cluster of 200 KB: a letter and its marks, then a short line.
    let marks = 100_000;
    let line = format!("a{}", MARK.repeat(marks));
    let mut ed = ed(&format!("{line}\nshort"));
    let middle = 1 + 2 * (marks / 2);
    ed.caret(middle);
    ed.go(Motion::Right);
    assert_eq!(ed.heads(), [middle + MAX_LINE_BYTES], "the window's edge");
    ed.caret(middle);
    ed.go(Motion::Left);
    assert_eq!(ed.heads(), [middle - MAX_LINE_BYTES]);
    ed.caret(middle);
    ed.go(Motion::WordRight);
    assert_eq!(ed.heads(), [middle + MAX_LINE_BYTES]);
    ed.go(Motion::WordLeft);
    assert_eq!(ed.heads(), [middle]);
    ed.go(Motion::Home);
    assert_eq!(ed.heads(), [0]);
    ed.go(Motion::End);
    assert_eq!(ed.heads(), [line.len()]);
    // Past the part the view shows, the column is where it cuts the line:
    // before the cluster, which is longer than the cut.
    ed.caret(middle);
    ed.go(Motion::Down);
    assert_eq!(ed.heads(), [line.len() + 1], "the short line's start");
    ed.caret(middle);
    ed.run(Command::DeleteLeft);
    assert_eq!(ed.editor.text().len(), line.len() + 6 - MAX_LINE_BYTES);
    ed.run(Command::Undo);
    ed.caret(middle);
    ed.typed("x");
    assert_eq!(ed.editor.text().len(), line.len() + 7);
}

#[test]
fn regional_indicators_pair_up_inside_the_window() {
    let count = 50_000;
    let mut ed = ed(&INDICATOR.repeat(count));
    ed.go(Motion::Right);
    assert_eq!(ed.heads(), [8], "a flag from the line's start");
    // At an even indicator, far into the line, the window starts at an even
    // one too, so the pairs are the line's.
    let at = 4 * (count / 2);
    ed.caret(at);
    ed.go(Motion::Right);
    assert_eq!(ed.heads(), [at + 8]);
    ed.go(Motion::Left);
    ed.go(Motion::Left);
    assert_eq!(ed.heads(), [at - 8]);
}

// ---------------------------------------------------------------------------
// Random commands.

/// One step of a random session.
#[derive(Clone, Debug)]
enum Op {
    Run(Command),
    Type(String),
    Paste(String),
    Select(Vec<(u32, u32)>),
    Wait(u64),
}

/// Pieces random texts are made of: clusters of every kind the motions
/// meet, line breaks of each kind, and white space.
const PIECES: [&str; 14] = [
    "a", "word", " ", "\t", "\n", "\r\n", "\r", ".", "e\u{301}", "🇺🇸", "😀", "中", "  ", "x_y",
];

fn piece_text(rng: &mut Pcg32, count: u32) -> String {
    (0..count)
        .map(|_| *rng.choose(&PIECES).unwrap_or(&"a"))
        .collect()
}

fn op(rng: &mut Pcg32) -> Op {
    match rng.range(0..10u32) {
        0..=4 => Op::Run(*rng.choose(&Command::ALL).unwrap_or(&Command::Undo)),
        5 => {
            let count = rng.range(1..4u32);
            Op::Type(piece_text(rng, count))
        }
        6 => {
            let count = rng.range(0..6u32);
            Op::Paste(piece_text(rng, count))
        }
        7 => {
            let count = rng.range(1..4u32);
            Op::Select(
                (0..count)
                    .map(|_| (rng.next_u32(), rng.next_u32()))
                    .collect(),
            )
        }
        _ => Op::Wait(u64::from(rng.range(0..1_500u32))),
    }
}

/// The char boundary of `text` that `pick` lands nearest below.
fn boundary(text: &Rope, pick: u32) -> usize {
    text.floor_char_boundary(pick as usize % (text.len() + 1))
}

#[test]
fn random_commands_keep_the_selection_in_the_text_and_undo_goes_back() {
    check_sized(
        "random_commands_keep_the_selection_in_the_text_and_undo_goes_back",
        200,
        |rng, size| {
            let count = rng.range(0..12u32);
            let start = piece_text(rng, count);
            let ops: Vec<Op> = (0..size / 2).map(|_| op(rng)).collect();
            (start, ops)
        },
        |(start, ops)| {
            let mut ed = ed(start);
            for (step, op) in ops.iter().enumerate() {
                match op {
                    Op::Run(command) => {
                        ed.editor
                            .run(&mut ed.tree, *command, ed.now, &METRICS)
                            .map_err(|e| format!("step {step}, {command:?}: {e}"))?;
                    }
                    Op::Type(typed) => {
                        ed.editor
                            .type_text(&mut ed.tree, typed, ed.now, &METRICS)
                            .map_err(|e| format!("step {step}, typing: {e}"))?;
                    }
                    Op::Paste(pasted) => {
                        ed.editor
                            .paste(&mut ed.tree, pasted, ed.now, &METRICS)
                            .map_err(|e| format!("step {step}, pasting: {e}"))?;
                    }
                    Op::Select(picks) => {
                        let text = ed.editor.text().clone();
                        let cursors: Vec<Cursor> = picks
                            .iter()
                            .map(|&(a, h)| Cursor::new(boundary(&text, a), boundary(&text, h)))
                            .collect();
                        let selection = Selection::new(cursors[0], cursors.iter().copied());
                        ed.editor
                            .set_selection(&mut ed.tree, selection)
                            .map_err(|e| format!("step {step}, selecting: {e}"))?;
                    }
                    Op::Wait(ms) => ed.later(Duration::from_millis(*ms)),
                }
                if step % 4 == 3 {
                    ed.settle();
                }
                let text = ed.editor.text();
                let selection = ed.editor.selection(&ed.tree).map_err(|e| e.to_string())?;
                selection
                    .validate(text)
                    .map_err(|e| format!("step {step}, {op:?}: the selection: {e}"))?;
                let shown = ed.editor.view().text(&ed.tree).map_err(|e| e.to_string())?;
                if shown != text {
                    return Err(format!("step {step}, {op:?}: the view shows another text"));
                }
            }
            for _ in 0..=ops.len() {
                if !ed
                    .editor
                    .run(&mut ed.tree, Command::Undo, ed.now, &METRICS)
                    .map_err(|e| e.to_string())?
                    .changed
                {
                    break;
                }
            }
            if ed.text() != *start {
                return Err(format!("undo gave {:?}, not {start:?}", ed.text()));
            }
            Ok(())
        },
    );
}
