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

//! The editor view, headless (M4.1, ADR-0063): the gutter and its colors,
//! tab stops, hidden characters' markers, selections and cursors, the
//! current line, scrolling with soft wrap off and on, the scrollbars, the
//! long-line cut, a 10,000-line file scrolled to its end, points and byte
//! offsets, bad selections, the accessibility tree, and idleness.

use tidepool_core::color::Color;
use tidepool_core::geom::{Point, Rect, Size};
use tidepool_core::input::{InputEvent, Key, Modifiers, MouseButton, ScrollDelta};
use tidepool_core::testing::check_sized;
use tidepool_editor::EditorError;
use tidepool_editor::view::{
    CURSOR_WIDTH, CUT_MARKER, EditorView, GUTTER_PADDING, MARKER, MAX_LINE_BYTES, ScrollPosition,
};
use tidepool_text::pos::grapheme;
use tidepool_text::{Cursor, Rope, Selection, TextError};
use tidepool_ui::draw::Command;
use tidepool_ui::layout::Axis;
use tidepool_ui::metrics::FixedMetrics;
use tidepool_ui::theme::Token;
use tidepool_ui::widgets::{FOCUS_WIDTH, SCROLLBAR_THICKNESS, ScrollBar};
use tidepool_ui::{Tree, UiError};

/// 7 units an advance and rows 21 high, so that every position is exact.
const METRICS: FixedMetrics = FixedMetrics {
    advance: 0.5,
    ascent: 0.75,
    descent: 0.25,
};

/// A character's advance in [`METRICS`] at 14 px.
const ADVANCE: f32 = 7.0;

/// A row's height in [`METRICS`]: 14 px at a line height of 1.5.
const ROW: f32 = 21.0;

/// A text area of 390 × 210 beside and above the scrollbars: ten rows.
const WINDOW: Size = Size::new(400.0, 220.0);

/// The gutter at three digits: the padding either side and 21 units of
/// digits.
const GUTTER: f32 = 2.0 * GUTTER_PADDING + 3.0 * ADVANCE;

/// The text's width beside the gutter.
const TEXT_WIDTH: f32 = 400.0 - SCROLLBAR_THICKNESS - GUTTER;

/// Where a wheel event over the text lands.
const OVER_TEXT: Point = Point::new(200.0, 100.0);

/// The flag of Wales: a black flag, five tag characters and a cancel tag.
const WALES: &str = "\u{1F3F4}\u{E0067}\u{E0062}\u{E0077}\u{E006C}\u{E0073}\u{E007F}";

fn view_of(text: &str) -> (Tree, EditorView) {
    let mut tree = Tree::new();
    let root = tree.root();
    let view = EditorView::insert(&mut tree, root, "main.rs", Rope::from(text)).expect("a view");
    settle(&mut tree, view);
    (tree, view)
}

/// Draws frames and updates the view, as the main loop does, until
/// neither has anything left to do.
fn settle(tree: &mut Tree, view: EditorView) {
    for _ in 0..8 {
        let drew = tree.frame(WINDOW, &METRICS).is_some();
        let changed = view.update(tree, &[], &METRICS).expect("update");
        if !drew && !changed {
            return;
        }
    }
    panic!("the view never settled");
}

/// Gives `event` to the view, then settles.
fn send(tree: &mut Tree, view: EditorView, event: &InputEvent) {
    view.handle(tree, event, &METRICS).expect("handled");
    settle(tree, view);
}

/// Every command the tree draws, painted afresh.
fn drawn(tree: &mut Tree) -> Vec<Command> {
    tree.invalidate_all();
    let frame = tree.frame(WINDOW, &METRICS).expect("a frame");
    frame.list.commands().to_vec()
}

fn color(tree: &Tree, token: Token) -> Color {
    tree.theme().color(token)
}

/// The texts drawn in `color`, with their origins.
fn texts(commands: &[Command], color: Color) -> Vec<(Point, String)> {
    commands
        .iter()
        .filter_map(|command| match command {
            Command::Text {
                origin,
                text,
                color: c,
                ..
            } if *c == color => Some((*origin, text.clone())),
            _ => None,
        })
        .collect()
}

/// The rects drawn in `color`.
fn rects(commands: &[Command], color: Color) -> Vec<Rect> {
    commands
        .iter()
        .filter_map(|command| match command {
            Command::Rect { rect, color: c } if *c == color => Some(*rect),
            _ => None,
        })
        .collect()
}

/// The baseline of row `row` from the view's top.
fn baseline(row: usize) -> f32 {
    row as f32 * ROW + (ROW - 14.0) / 2.0 + 10.5
}

/// Where column `column` of the text starts, unscrolled.
fn column(column: usize) -> f32 {
    GUTTER + column as f32 * ADVANCE
}

fn wheel(delta: ScrollDelta, modifiers: Modifiers) -> InputEvent {
    InputEvent::Wheel {
        delta,
        position: OVER_TEXT,
        modifiers,
    }
}

fn notch(x: f32, y: f32) -> InputEvent {
    wheel(ScrollDelta::Lines { x, y }, Modifiers::NONE)
}

fn mouse(down: bool, position: Point) -> InputEvent {
    if down {
        InputEvent::MouseDown {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::NONE,
        }
    } else {
        InputEvent::MouseUp {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::NONE,
        }
    }
}

fn hover(position: Point) -> InputEvent {
    InputEvent::MouseMove {
        position,
        modifiers: Modifiers::NONE,
    }
}

fn position(tree: &Tree, view: EditorView) -> ScrollPosition {
    view.position(tree).expect("a position")
}

fn bar(tree: &Tree, view: EditorView, axis: Axis) -> ScrollBar {
    tree.widget::<ScrollBar>(view.scroll_bar(axis))
        .expect("a scrollbar")
        .clone()
}

/// `count` numbered lines, joined by line feeds, with no break at the end.
fn numbered(count: usize) -> String {
    (1..=count)
        .map(|n| format!("line {n}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_gutter_numbers_lines_right_aligned_brighter_where_a_cursor_is() {
    let (mut tree, view) = view_of("alpha\nbeta\ngamma");
    view.set_selection(&mut tree, Selection::caret(8))
        .expect("a caret on line 2");
    let commands = drawn(&mut tree);
    let dim = texts(&commands, color(&tree, Token::Text3));
    let bright = texts(&commands, color(&tree, Token::Text2));
    // Right-aligned: the padding right of a one-digit number.
    let right = GUTTER - GUTTER_PADDING - ADVANCE;
    assert_eq!(
        dim,
        [
            (Point::new(right, baseline(0)), "1".to_string()),
            (Point::new(right, baseline(2)), "3".to_string())
        ]
    );
    assert_eq!(bright, [(Point::new(right, baseline(1)), "2".to_string())]);
    // A selection's head brightens its line too, not its anchor's line.
    view.set_selection(&mut tree, Selection::single(Cursor::new(2, 13)))
        .expect("a selection");
    let commands = drawn(&mut tree);
    let bright = texts(&commands, color(&tree, Token::Text2));
    assert_eq!(bright, [(Point::new(right, baseline(2)), "3".to_string())]);

    // A thousand lines take four digits: the gutter grows by one. The
    // caret at the start brightens line 1.
    let (mut tree, _view) = view_of(&numbered(1000));
    let commands = drawn(&mut tree);
    let bright = texts(&commands, color(&tree, Token::Text2));
    assert_eq!(
        bright,
        [(
            Point::new(GUTTER + ADVANCE - GUTTER_PADDING - ADVANCE, baseline(0)),
            "1".to_string()
        )]
    );
    let text = texts(&commands, color(&tree, Token::Text));
    assert_eq!(
        text[0],
        (
            Point::new(GUTTER + ADVANCE, baseline(0)),
            "line 1".to_string()
        )
    );
}

#[test]
fn a_tab_advances_to_the_next_stop_of_four_spaces() {
    let (mut tree, _view) = view_of("\tx\nab\tx\nabcd\tx\nabc\t\tx");
    let commands = drawn(&mut tree);
    let shown = texts(&commands, color(&tree, Token::Text));
    let at = |row: usize, text: &str| {
        shown
            .iter()
            .find(|(origin, shown)| shown == text && origin.y == baseline(row))
            .map(|(origin, _)| origin.x)
    };
    assert_eq!(at(0, "x"), Some(column(4)));
    assert_eq!(at(1, "ab"), Some(column(0)));
    assert_eq!(at(1, "x"), Some(column(4)));
    // From a stop, a tab goes on to the next.
    assert_eq!(at(2, "x"), Some(column(8)));
    assert_eq!(at(3, "x"), Some(column(8)));
    // A tab draws nothing of its own.
    assert!(shown.iter().all(|(_, text)| !text.contains('\t')));
}

#[test]
fn hidden_characters_show_as_markers_but_tabs_line_breaks_and_flags_do_not() {
    let text = format!("a\0b\u{85}c\u{202E}d\u{200B}e\u{2028}f\r\n\tg\rh\n{WALES} cymru\u{E0041}");
    let (mut tree, view) = view_of(&text);
    let commands = drawn(&mut tree);
    let markers = texts(&commands, color(&tree, Token::Error));
    let marker = MARKER.to_string();
    // NUL, NEL (a C1 control), U+202E, U+200B and U+2028 on the first
    // line, each one marker after its letter; and the stray tag character
    // after the flag's line.
    let first: Vec<Point> = markers
        .iter()
        .filter(|(origin, _)| origin.y == baseline(0))
        .map(|(origin, shown)| {
            assert_eq!(*shown, marker);
            *origin
        })
        .collect();
    assert_eq!(
        first,
        [1, 3, 5, 7, 9].map(|col| Point::new(column(col), baseline(0)))
    );
    assert_eq!(markers.len(), 6, "{markers:?}");
    // The letters are runs of their own between the markers.
    let shown = texts(&commands, color(&tree, Token::Text));
    let first_line: Vec<&str> = shown
        .iter()
        .filter(|(origin, _)| origin.y == baseline(0))
        .map(|(_, text)| text.as_str())
        .collect();
    assert_eq!(first_line, ["a", "b", "c", "d", "e", "f"]);
    // The tab, CR LF, the lone CR and LF show no marker: "g" sits at the
    // first stop, "h" starts a line of its own.
    assert!(shown.contains(&(Point::new(column(4), baseline(1)), "g".to_string())));
    assert!(shown.contains(&(Point::new(column(0), baseline(2)), "h".to_string())));
    // The flag is whole, in the run of text around it.
    let flag_line = format!("{WALES} cymru");
    assert!(shown.contains(&(Point::new(column(0), baseline(3)), flag_line)));
    // The buffer is unchanged.
    assert_eq!(view.text(&tree).expect("its text").to_string(), text);
}

#[test]
fn selections_paint_teal_glow_behind_their_text_with_many_cursors() {
    let (mut tree, view) = view_of("hello world\nsecond line\nthird");
    let selection = Selection::new(Cursor::new(6, 17), [Cursor::caret(26)]);
    view.set_selection(&mut tree, selection.clone())
        .expect("a selection");
    tree.set_focus(view.text_node(), false).expect("focus");
    let commands = drawn(&mut tree);
    let glow = rects(&commands, color(&tree, Token::TealGlow));
    assert_eq!(
        glow,
        [
            // "world", then its line break as a space.
            Rect::new(column(6), 0.0, 6.0 * ADVANCE, ROW),
            // "secon".
            Rect::new(column(0), ROW, 5.0 * ADVANCE, ROW),
        ]
    );
    // Behind the text, which stays `text`.
    let (glow, text) = (color(&tree, Token::TealGlow), color(&tree, Token::Text));
    let first_glow = commands
        .iter()
        .position(|command| matches!(command, Command::Rect { color, .. } if *color == glow))
        .expect("a selection");
    let first_text = commands
        .iter()
        .position(|command| matches!(command, Command::Text { color, .. } if *color == text))
        .expect("text");
    assert!(first_glow < first_text);
    // A bar at each head: the selection's, after "secon", and the caret's.
    let bars = rects(&commands, color(&tree, Token::Teal));
    assert_eq!(
        bars,
        [
            Rect::new(column(5), ROW, CURSOR_WIDTH, ROW),
            Rect::new(column(2), 2.0 * ROW, CURSOR_WIDTH, ROW),
        ]
    );
    assert_eq!(view.selection(&tree).expect("its selection"), &selection);

    // A selected empty line shows its break alone.
    let (mut tree, view) = view_of("a\n\nb");
    view.set_selection(&mut tree, Selection::single(Cursor::new(2, 3)))
        .expect("the empty line");
    let commands = drawn(&mut tree);
    assert_eq!(
        rects(&commands, color(&tree, Token::TealGlow)),
        [Rect::new(column(0), ROW, ADVANCE, ROW)]
    );
}

#[test]
fn cursors_show_only_while_the_view_has_focus() {
    let (mut tree, view) = view_of("one\ntwo");
    let teal = color(&tree, Token::Teal);
    assert!(rects(&drawn(&mut tree), teal).is_empty());
    tree.set_focus(view.text_node(), true).expect("focus");
    settle(&mut tree, view);
    assert_eq!(
        rects(&drawn(&mut tree), teal),
        [Rect::new(column(0), 0.0, CURSOR_WIDTH, ROW)]
    );
    tree.clear_focus();
    settle(&mut tree, view);
    assert!(rects(&drawn(&mut tree), teal).is_empty());
}

#[test]
fn keyboard_focus_shows_the_focus_outline_inside_the_text_area_and_a_click_does_not() {
    let (mut tree, view) = view_of("one\ntwo");
    let teal = color(&tree, Token::Teal);
    let outlines = |commands: &[Command]| -> Vec<(Rect, f32)> {
        commands
            .iter()
            .filter_map(|command| match command {
                Command::Border { rect, width, color } if *color == teal => Some((*rect, *width)),
                _ => None,
            })
            .collect()
    };
    // Inside the text area's bounds, clear of the scrollbars beside it.
    let area = tree.bounds(view.text_node()).expect("laid out");
    assert_eq!(
        area,
        Rect::new(
            0.0,
            0.0,
            400.0 - SCROLLBAR_THICKNESS,
            220.0 - SCROLLBAR_THICKNESS
        )
    );
    assert!(outlines(&drawn(&mut tree)).is_empty());
    // Keyboard focus, as the golden scenes give it.
    tree.set_focus(view.text_node(), true).expect("focus");
    settle(&mut tree, view);
    assert_eq!(outlines(&drawn(&mut tree)), [(area, FOCUS_WIDTH)]);
    // A click focuses it without showing focus: the cursor, no outline.
    tree.clear_focus();
    settle(&mut tree, view);
    let at = Point::new(column(1), 5.0);
    send(&mut tree, view, &mouse(true, at));
    send(&mut tree, view, &mouse(false, at));
    let state = tree.state(view.text_node()).expect("a state");
    assert!(state.focused && !state.focus_visible, "{state:?}");
    let commands = drawn(&mut tree);
    assert!(outlines(&commands).is_empty());
    assert_eq!(rects(&commands, teal).len(), 1, "the cursor shows");
    // Tab shows it.
    let tab = InputEvent::KeyDown {
        key: Key::Tab,
        modifiers: Modifiers::NONE,
        repeat: false,
    };
    send(&mut tree, view, &tab);
    assert_eq!(outlines(&drawn(&mut tree)), [(area, FOCUS_WIDTH)]);
}

#[test]
fn the_current_line_is_washed_only_with_a_caret_and_nothing_selected() {
    let (mut tree, view) = view_of("one two\nthree\nfour");
    let wash = color(&tree, Token::CurrentLine);
    let full = |row: usize| Rect::new(0.0, row as f32 * ROW, 400.0 - SCROLLBAR_THICKNESS, ROW);
    view.set_selection(&mut tree, Selection::caret(9))
        .expect("a caret on line 2");
    assert_eq!(rects(&drawn(&mut tree), wash), [full(1)]);
    // A caret on line 1 and a selection on line 3: only line 1.
    view.set_selection(
        &mut tree,
        Selection::new(Cursor::caret(1), [Cursor::new(15, 17)]),
    )
    .expect("two cursors");
    assert_eq!(rects(&drawn(&mut tree), wash), [full(0)]);
    // A caret and a selection on the same line: no wash under the glow.
    view.set_selection(
        &mut tree,
        Selection::new(Cursor::caret(0), [Cursor::new(4, 7)]),
    )
    .expect("two cursors on a line");
    assert!(rects(&drawn(&mut tree), wash).is_empty());
}

#[test]
fn with_wrap_off_shift_and_horizontal_deltas_scroll_sideways() {
    let long = "x".repeat(100);
    let (mut tree, view) = view_of(&format!("{long}\nshort"));
    // Three rows' height a notch, with Shift.
    send(
        &mut tree,
        view,
        &wheel(ScrollDelta::Lines { x: 0.0, y: 1.0 }, Modifiers::SHIFT),
    );
    assert_eq!(position(&tree, view).x, 3.0 * ROW);
    assert_eq!(position(&tree, view).line, 0);
    // A horizontal notch, and an exact distance.
    send(&mut tree, view, &notch(1.0, 0.0));
    assert_eq!(position(&tree, view).x, 6.0 * ROW);
    send(
        &mut tree,
        view,
        &wheel(ScrollDelta::Logical { x: 10.0, y: 0.0 }, Modifiers::NONE),
    );
    let left = 6.0 * ROW + 10.0;
    assert_eq!(position(&tree, view).x, left);
    let commands = drawn(&mut tree);
    // The text moves left; the gutter does not.
    let shown = texts(&commands, color(&tree, Token::Text));
    assert!(shown.iter().any(|(origin, _)| origin.x < GUTTER));
    let dim = texts(&commands, color(&tree, Token::Text3));
    assert!(
        dim.iter()
            .all(|(origin, _)| origin.x == GUTTER - GUTTER_PADDING - ADVANCE)
    );
    // No further than shows the widest line's end and a cursor after it.
    for _ in 0..4 {
        send(&mut tree, view, &notch(10.0, 0.0));
    }
    let most = 100.0 * ADVANCE + CURSOR_WIDTH - TEXT_WIDTH;
    assert_eq!(position(&tree, view).x, most);
    // And back to 0.
    for _ in 0..4 {
        send(&mut tree, view, &notch(-10.0, 0.0));
    }
    assert_eq!(position(&tree, view).x, 0.0);
}

#[test]
fn with_wrap_on_lines_wrap_after_a_space_or_inside_a_word_numbered_once() {
    // 49 columns fit: 343 units, the text's width less a cursor's.
    assert_eq!(((TEXT_WIDTH - CURSOR_WIDTH) / ADVANCE).floor(), 49.0);
    let words = format!("{} {}", "a".repeat(45), "b".repeat(10));
    let word = "c".repeat(120);
    let more = numbered(20);
    let (mut tree, view) = view_of(&format!("{words}\n{word}\n{more}"));
    view.set_wrap(&mut tree, true).expect("wrap");
    settle(&mut tree, view);
    assert!(view.wrap(&tree).expect("its wrap"));
    let commands = drawn(&mut tree);
    let shown = texts(&commands, color(&tree, Token::Text));
    let expected = [
        format!("{} ", "a".repeat(45)),
        "b".repeat(10),
        "c".repeat(49),
        "c".repeat(49),
        "c".repeat(22),
    ];
    let rows: Vec<(Point, String)> = expected
        .iter()
        .enumerate()
        .map(|(row, text)| (Point::new(column(0), baseline(row)), text.clone()))
        .collect();
    assert_eq!(shown.get(..5), Some(&rows[..]));
    // Only each line's first row is numbered.
    let numbers: Vec<(f32, String)> = [Token::Text2, Token::Text3]
        .into_iter()
        .flat_map(|token| texts(&commands, color(&tree, token)))
        .map(|(origin, text)| (origin.y, text))
        .collect();
    assert_eq!(
        numbers.get(..2),
        Some(
            &[
                (baseline(0), "1".to_string()),
                (baseline(2), "2".to_string())
            ][..]
        )
    );
    assert!(
        numbers
            .iter()
            .all(|(y, _)| ![1, 3, 4].map(baseline).contains(y))
    );
    // The horizontal scrollbar is hidden, and nothing scrolls sideways.
    let hidden = tree
        .style(view.scroll_bar(Axis::Horizontal))
        .map(|style| style.hidden);
    assert_eq!(hidden, Some(true));
    let response = view
        .handle(
            &mut tree,
            &wheel(ScrollDelta::Lines { x: 0.0, y: 1.0 }, Modifiers::SHIFT),
            &METRICS,
        )
        .expect("handled");
    assert!(!response.handled);
    assert_eq!(position(&tree, view).x, 0.0);
    // A caret where a row wraps shows at the next row's start.
    let point = view
        .byte_to_point(&tree, 46, &METRICS)
        .expect("a byte")
        .expect("on screen");
    assert_eq!(point, Point::new(column(0), ROW));
    // The wheel moves by rows, here into the second line's rows.
    send(&mut tree, view, &notch(0.0, 1.0));
    assert_eq!(
        (position(&tree, view).line, position(&tree, view).row),
        (1, 1)
    );
    // Off again, the top row is its line's first.
    view.set_wrap(&mut tree, false).expect("no wrap");
    settle(&mut tree, view);
    assert_eq!(
        (position(&tree, view).line, position(&tree, view).row),
        (1, 0)
    );
}

#[test]
fn the_scrollbars_count_lines_and_units_and_drag_the_view() {
    let mut text = numbered(100);
    text.replace_range(0..0, &format!("{}\n", "x".repeat(100)));
    let (mut tree, view) = view_of(&text);
    let vertical = bar(&tree, view, Axis::Vertical);
    assert_eq!(
        (vertical.content(), vertical.viewport(), vertical.offset()),
        (101.0, 10.0, 0.0)
    );
    let horizontal = bar(&tree, view, Axis::Horizontal);
    assert_eq!(
        (horizontal.content(), horizontal.viewport()),
        (100.0 * ADVANCE + CURSOR_WIDTH, TEXT_WIDTH)
    );
    // A notch moves the vertical one three lines.
    send(&mut tree, view, &notch(0.0, 1.0));
    assert_eq!(bar(&tree, view, Axis::Vertical).offset(), 3.0);
    send(&mut tree, view, &notch(0.0, -1.0));
    // Dragging its thumb half its travel: 24 long in a track of 220.
    let x = 400.0 - SCROLLBAR_THICKNESS / 2.0;
    let travel = 220.0 - 24.0;
    send(&mut tree, view, &mouse(true, Point::new(x, 10.0)));
    send(&mut tree, view, &hover(Point::new(x, 10.0 + travel / 2.0)));
    send(
        &mut tree,
        view,
        &mouse(false, Point::new(x, 10.0 + travel / 2.0)),
    );
    let line = position(&tree, view).line;
    assert_eq!(line, 45, "{:?}", position(&tree, view));
    assert_eq!(bar(&tree, view, Axis::Vertical).offset(), 45.5);
    // Dragging the horizontal thumb to the end shows the long line's end.
    view.scroll_into_view(&mut tree, 0, &METRICS)
        .expect("the start");
    settle(&mut tree, view);
    let y = 220.0 - SCROLLBAR_THICKNESS / 2.0;
    send(&mut tree, view, &mouse(true, Point::new(20.0, y)));
    send(&mut tree, view, &hover(Point::new(400.0, y)));
    send(&mut tree, view, &mouse(false, Point::new(400.0, y)));
    let most = 100.0 * ADVANCE + CURSOR_WIDTH - TEXT_WIDTH;
    assert_eq!(position(&tree, view).x, most);
    assert_eq!(bar(&tree, view, Axis::Horizontal).offset(), most);
}

#[test]
fn the_horizontal_range_is_the_widest_line_laid_out_so_far() {
    let mut lines: Vec<String> = (1..=60).map(|n| format!("line {n}")).collect();
    lines[49] = "y".repeat(80);
    let (mut tree, view) = view_of(&lines.join("\n"));
    let widest_on_screen = "line 10".len() as f32 * ADVANCE + CURSOR_WIDTH;
    assert_eq!(
        bar(&tree, view, Axis::Horizontal).content(),
        widest_on_screen
    );
    let fiftieth = view
        .text(&tree)
        .expect("its text")
        .line_to_byte(49)
        .expect("line 50");
    assert!(
        view.scroll_into_view(&mut tree, fiftieth, &METRICS)
            .expect("line 50")
    );
    settle(&mut tree, view);
    // At the bottom of the view.
    assert_eq!(position(&tree, view).line, 40);
    assert_eq!(
        bar(&tree, view, Axis::Horizontal).content(),
        80.0 * ADVANCE + CURSOR_WIDTH
    );
    // Back at the top, the range keeps what was laid out.
    assert!(
        view.scroll_into_view(&mut tree, 0, &METRICS)
            .expect("line 1")
    );
    settle(&mut tree, view);
    assert_eq!(position(&tree, view).line, 0);
    assert_eq!(
        bar(&tree, view, Axis::Horizontal).content(),
        80.0 * ADVANCE + CURSOR_WIDTH
    );
    // Already in view: nothing moves.
    assert!(
        !view
            .scroll_into_view(&mut tree, 3, &METRICS)
            .expect("line 1")
    );
}

#[test]
fn a_long_line_shows_its_start_and_a_marker_where_it_is_cut() {
    // Three bytes a cluster: the cut falls at a cluster boundary, 9,999.
    let accented = "e\u{301}".repeat(MAX_LINE_BYTES / 3 + 10);
    let (mut tree, view) = view_of(&accented);
    let cut = MAX_LINE_BYTES / 3 * 3;
    let far = Point::new(1e6, 5.0);
    assert_eq!(
        view.point_to_byte(&tree, far, &METRICS).expect("a byte"),
        cut
    );
    // Past the cut, a caret shows at the marker.
    assert!(
        view.scroll_into_view(&mut tree, accented.len(), &METRICS)
            .expect("the end")
    );
    settle(&mut tree, view);
    let commands = drawn(&mut tree);
    let marker_x = column(0) + (cut / 3 * 2) as f32 * ADVANCE - position(&tree, view).x;
    assert_eq!(
        texts(&commands, color(&tree, Token::Text3))
            .into_iter()
            .filter(|(_, text)| text == CUT_MARKER)
            .collect::<Vec<_>>(),
        [(Point::new(marker_x, baseline(0)), CUT_MARKER.to_string())]
    );
}

#[test]
fn a_100_mb_line_draws_a_bounded_frame() {
    let line = "x".repeat(100 << 20);
    let (mut tree, view) = view_of(&line);
    let bounded = |tree: &mut Tree| {
        let commands = drawn(tree);
        let text: usize = commands
            .iter()
            .map(|command| match command {
                Command::Text { text, .. } => text.chars().count(),
                _ => 0,
            })
            .sum();
        (commands.len(), text)
    };
    // Wrap off: a run of what shows across, the number, the wash.
    let (count, chars) = bounded(&mut tree);
    assert!(count <= 16, "{count} commands");
    assert!(
        chars <= 2 * (TEXT_WIDTH / ADVANCE) as usize,
        "{chars} characters"
    );
    // Scrolled to its end: what shows there, and the marker.
    view.scroll_into_view(&mut tree, line.len(), &METRICS)
        .expect("the end");
    settle(&mut tree, view);
    let (count, chars) = bounded(&mut tree);
    assert!(count <= 16, "{count} commands");
    assert!(
        chars <= 2 * (TEXT_WIDTH / ADVANCE) as usize,
        "{chars} characters"
    );
    // Wrap on, the horizontal scrollbar hides and the text area is 220
    // high: eleven rows of 49 columns at most and the number, at the top
    // and at the end, where the last of the 205 rows the kept start wraps
    // to is at the bottom.
    view.set_wrap(&mut tree, true).expect("wrap");
    settle(&mut tree, view);
    let most = 11 * 49 + 1;
    let (count, chars) = bounded(&mut tree);
    assert!(count <= 40, "{count} commands");
    assert!(chars <= most, "{chars} characters");
    view.scroll_into_view(&mut tree, line.len(), &METRICS)
        .expect("the end");
    settle(&mut tree, view);
    let (count, chars) = bounded(&mut tree);
    assert!(count <= 40, "{count} commands");
    assert!(chars <= most, "{chars} characters");
    let rows = (MAX_LINE_BYTES as f32 / 49.0).ceil();
    let scrolled = rows * ROW - 220.0;
    let top = (scrolled / ROW).floor();
    assert_eq!(
        (position(&tree, view).row, position(&tree, view).y),
        (top as usize, scrolled - top * ROW)
    );
    // The scrollbar counts one line, though it wraps to many rows.
    assert_eq!(bar(&tree, view, Axis::Vertical).content(), 1.0);
}

#[test]
fn a_10000_line_file_scrolls_to_its_end_a_notch_at_a_time_in_bounded_frames() {
    let lines: Vec<String> = (0..10_000)
        .map(|n| format!("    let value_{n} = compute({n}, \"text\");"))
        .collect();
    let (mut tree, view) = view_of(&lines.join("\n"));
    let mut most = 0;
    let mut frames = 0;
    loop {
        let before = position(&tree, view);
        view.handle(&mut tree, &notch(0.0, 1.0), &METRICS)
            .expect("handled");
        if let Some(frame) = tree.frame(WINDOW, &METRICS) {
            most = most.max(frame.list.len());
            frames += 1;
        }
        view.update(&mut tree, &[], &METRICS).expect("update");
        if let Some(frame) = tree.frame(WINDOW, &METRICS) {
            most = most.max(frame.list.len());
        }
        if position(&tree, view) == before {
            break;
        }
        assert!(frames < 4000, "never reached the end");
    }
    // Eleven rows at most show, each a number, a run and perhaps a wash
    // and a bar, with the ground, the clips and the scrollbars.
    assert!(most <= 11 * 4 + 8, "{most} commands in a frame");
    assert_eq!(frames, 3330);
    // The last line at the bottom.
    assert_eq!(
        position(&tree, view),
        ScrollPosition {
            line: 9990,
            row: 0,
            y: 0.0,
            x: 0.0
        }
    );
    let vertical = bar(&tree, view, Axis::Vertical);
    assert_eq!(vertical.offset(), vertical.max_offset());
    let last = view.text(&tree).expect("its text").len();
    let point = view
        .byte_to_point(&tree, last, &METRICS)
        .expect("the end")
        .expect("on screen");
    assert_eq!(point.y, 9.0 * ROW);
}

/// Text from pieces that try the clusters, markers and breaks.
fn arbitrary(rng: &mut tidepool_core::rng::Pcg32, size: u32) -> (String, bool) {
    const PIECES: [&str; 14] = [
        "a",
        "b",
        " ",
        "\t",
        "e\u{301}",
        "\u{1F600}",
        "\u{4E2D}",
        "\u{200B}",
        "\0",
        "a\u{E0041}",
        WALES,
        "\u{1F1FA}\u{1F1F8}",
        "\u{202E}",
        "word ",
    ];
    const BREAKS: [&str; 3] = ["\n", "\r\n", "\r"];
    let mut text = String::new();
    for _ in 0..size * 2 {
        let from: &[&str] = if rng.range(0..12_u32) == 0 {
            &BREAKS
        } else {
            &PIECES
        };
        text.push_str(rng.choose(from).copied().unwrap_or("a"));
    }
    (text, rng.range(0..2_u32) == 0)
}

#[test]
fn points_and_byte_offsets_round_trip_at_cluster_boundaries() {
    check_sized(
        "byte -> point -> byte at cluster boundaries",
        64,
        arbitrary,
        |(text, wrap)| {
            let (mut tree, view) = view_of(text);
            view.set_wrap(&mut tree, *wrap).map_err(|e| e.to_string())?;
            settle(&mut tree, view);
            let mut boundaries = vec![0];
            let mut at = 0;
            for cluster in grapheme::graphemes(text) {
                at += cluster.len();
                boundaries.push(at);
            }
            let mut shown = 0;
            for byte in boundaries {
                let Some(point) = view
                    .byte_to_point(&tree, byte, &METRICS)
                    .map_err(|e| e.to_string())?
                else {
                    continue;
                };
                shown += 1;
                let back = view
                    .point_to_byte(&tree, point, &METRICS)
                    .map_err(|e| e.to_string())?;
                // The boundary between a CR and its LF is none: CR LF is
                // one cluster, so `back` is exactly `byte`.
                if back != byte {
                    return Err(format!("{byte} shows at {point:?}, which maps to {back}"));
                }
            }
            if shown == 0 {
                return Err("nothing on screen".to_string());
            }
            Ok(())
        },
    );
}

#[test]
fn bad_positions_and_selections_are_errors_that_change_nothing() {
    let (mut tree, view) = view_of("caf\u{E9}");
    let len = 5;
    let past = view.set_selection(&mut tree, Selection::caret(len + 1));
    assert!(matches!(
        past,
        Err(EditorError::Text(TextError::ByteOutOfBounds {
            byte: 6,
            len: 5
        }))
    ));
    let inside = view.set_selection(&mut tree, Selection::single(Cursor::new(0, 4)));
    assert!(matches!(
        inside,
        Err(EditorError::Text(TextError::NotCharBoundary { byte: 4 }))
    ));
    assert_eq!(view.selection(&tree).expect("kept"), &Selection::caret(0));
    // A new text with a selection it does not have is refused whole.
    let refused = view.set_text(&mut tree, Rope::from("ab"), Selection::caret(3));
    assert!(matches!(refused, Err(EditorError::Text(_))));
    assert_eq!(view.text(&tree).expect("kept").to_string(), "caf\u{E9}");
    // Points of bad offsets, and scrolling to them, too.
    assert!(matches!(
        view.byte_to_point(&tree, 9, &METRICS),
        Err(EditorError::Text(TextError::ByteOutOfBounds { .. }))
    ));
    assert!(matches!(
        view.scroll_into_view(&mut tree, 4, &METRICS),
        Err(EditorError::Text(TextError::NotCharBoundary { byte: 4 }))
    ));
    // A view removed from its tree.
    tree.remove(view.node()).expect("removed");
    assert!(matches!(
        view.text(&tree),
        Err(EditorError::Ui(UiError::UnknownNode { .. }))
    ));
    assert!(matches!(
        view.update(&mut tree, &[], &METRICS),
        Err(EditorError::Ui(UiError::UnknownNode { .. }))
    ));
}

#[test]
fn a_new_text_replaces_the_old_and_its_layouts() {
    let (mut tree, view) = view_of(&numbered(50));
    // Ten notches are thirty rows, but an event scrolls at most the view's
    // ten.
    send(&mut tree, view, &notch(0.0, 10.0));
    assert_eq!(position(&tree, view).line, 10);
    view.set_text(&mut tree, Rope::from("short\ntext"), Selection::caret(6))
        .expect("a new text");
    settle(&mut tree, view);
    assert_eq!(position(&tree, view).line, 0);
    let shown = texts(&drawn(&mut tree), color(&tree, Token::Text));
    assert_eq!(
        shown,
        [
            (Point::new(column(0), baseline(0)), "short".to_string()),
            (Point::new(column(0), baseline(1)), "text".to_string())
        ]
    );
}

#[test]
fn the_view_is_a_named_text_input_in_the_accessibility_tree() {
    let (mut tree, view) = view_of(&numbered(40));
    tree.set_focus(view.text_node(), false).expect("focus");
    assert_eq!(
        tree.accessibility().to_string(),
        "text input \"main.rs\" focusable focused\n\
         scroll bar \"main.rs\" = \"0%\"\n\
         scroll bar \"main.rs\" = \"0%\"\n"
    );
    // A blank name is refused, and the tree is as it was.
    let before = tree.node_count();
    let root = tree.root();
    let blank = EditorView::insert(&mut tree, root, " ", Rope::new());
    assert!(matches!(
        blank,
        Err(EditorError::Ui(UiError::Unnamed { .. }))
    ));
    assert_eq!(tree.node_count(), before);
}

#[test]
fn an_idle_view_draws_nothing_and_a_scroll_repaints_it() {
    let (mut tree, view) = view_of(&numbered(100));
    assert!(tree.frame(WINDOW, &METRICS).is_none());
    assert!(!view.update(&mut tree, &[], &METRICS).expect("update"));
    assert!(tree.frame(WINDOW, &METRICS).is_none());
    view.handle(&mut tree, &notch(0.0, 1.0), &METRICS)
        .expect("handled");
    let frame = tree.frame(WINDOW, &METRICS).expect("a frame");
    let area = tree.bounds(view.text_node()).expect("laid out");
    assert!(!frame.damage.is_empty());
    let covered: f32 = frame
        .damage
        .iter()
        .map(|rect| {
            let width = (rect.right().min(area.right()) - rect.x.max(area.x)).max(0.0);
            let height = (rect.bottom().min(area.bottom()) - rect.y.max(area.y)).max(0.0);
            width * height
        })
        .sum();
    assert!(covered > 0.0, "{:?}", frame.damage);
    assert_eq!(position(&tree, view).line, 3);
    settle(&mut tree, view);
    assert!(tree.frame(WINDOW, &METRICS).is_none());
}

#[test]
fn points_map_to_the_nearest_boundary_on_the_nearest_row() {
    let (tree, view) = view_of("abc\nde");
    let at = |x: f32, y: f32| {
        view.point_to_byte(&tree, Point::new(x, y), &METRICS)
            .expect("a byte")
    };
    // Left and right halves of "b".
    assert_eq!(at(column(1) + 3.0, 5.0), 1);
    assert_eq!(at(column(1) + 4.0, 5.0), 2);
    // The gutter is the row's start; past the end, the line's end.
    assert_eq!(at(5.0, 5.0), 0);
    assert_eq!(at(300.0, 5.0), 3);
    // Below the last row, the last row.
    assert_eq!(at(column(1), 200.0), 5);
    assert_eq!(at(column(9), 200.0), 6);
    // Above the view, the first.
    assert_eq!(at(column(2), -50.0), 2);
}
