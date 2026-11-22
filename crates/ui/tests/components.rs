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

//! M3.7's other components, through the public API only (ADR-0052): the
//! tab strip's keys, presses, closes, overflow, caps and looks, presses
//! after the owner moved tabs, its target, and its reveal beside a
//! collection's, with a property test of random inserts, closes, keys and
//! presses, with and without frames between them, that keep one valid
//! selection, a valid target and one tab stop; the splitter's keys, drag,
//! the panes' minimums, nested splits, a resize after a clamp, value and
//! looks, drags with no finite position, cancelled or hidden, the damage
//! of a move, and a second divider in a split; the
//! card's role, value, layout and looks; and the code block's COPY, spans,
//! caps and looks. Each paints only theme tokens, its idle frames are
//! `None`, and what it damages is its own.

use tidepool_core::color::Color;
use tidepool_core::geom::{Point, Rect, Size};
use tidepool_core::input::{InputEvent, Key, Modifiers, MouseButton, ScrollDelta};
use tidepool_core::rng::Pcg32;
use tidepool_core::testing::check_sized;
use tidepool_text::pos::grapheme;
use tidepool_ui::a11y::Role;
use tidepool_ui::draw::{Command, DrawList, PaintContext};
use tidepool_ui::input::{ActionKind, Response};
use tidepool_ui::layout::{Axis, Length, Style};
use tidepool_ui::metrics::FixedMetrics;
use tidepool_ui::theme::{Theme, Token};
use tidepool_ui::widgets::{
    self, Button, ButtonKind, Card, CodeBlock, CodeSpan, Collection, Control, DISABLED_OPACITY,
    FOCUS_WIDTH, MAX_CELL_BYTES, MAX_CODE_LINE_BYTES, MAX_CODE_LINES, MAX_CODE_SPANS,
    MAX_LANGUAGE_BYTES, MAX_TAB_LABEL_BYTES, MAX_TABS, Row, SPLITTER_STEP, SPLITTER_THICKNESS,
    Splitter, TAB_RULE, Tab, TabKind, TabStrip,
};
use tidepool_ui::{Node, NodeId, Tree, Widget};

const METRICS: FixedMetrics = FixedMetrics::DEFAULT;
const WINDOW: Size = Size::new(400.0, 300.0);

fn key(key: Key) -> InputEvent {
    key_with(key, Modifiers::NONE)
}

fn key_with(key: Key, modifiers: Modifiers) -> InputEvent {
    InputEvent::KeyDown {
        key,
        modifiers,
        repeat: false,
    }
}

fn press(button: MouseButton, at: Point) -> InputEvent {
    InputEvent::MouseDown {
        button,
        position: at,
        modifiers: Modifiers::NONE,
    }
}

fn release(at: Point) -> InputEvent {
    InputEvent::MouseUp {
        button: MouseButton::Left,
        position: at,
        modifiers: Modifiers::NONE,
    }
}

fn hover(at: Point) -> InputEvent {
    InputEvent::MouseMove {
        position: at,
        modifiers: Modifiers::NONE,
    }
}

fn click(tree: &mut Tree, at: Point) -> Response {
    let response = tree.handle(&press(MouseButton::Left, at));
    tree.handle(&release(at));
    response
}

fn color(token: Token) -> Color {
    Theme::bundled().color(token)
}

fn centre(rect: Rect) -> Point {
    Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
}

/// The kinds of the actions `response` holds of `id`.
fn kinds(response: &Response, id: NodeId) -> Vec<ActionKind> {
    response
        .actions
        .iter()
        .filter(|action| action.node == id)
        .map(|action| action.kind)
        .collect()
}

/// The commands a whole frame lists now.
fn painted(tree: &mut Tree, size: Size) -> Vec<Command> {
    tree.invalidate_all();
    tree.frame(size, &METRICS)
        .expect("a frame")
        .list
        .commands()
        .to_vec()
}

/// The colors the commands fill or stroke, in order.
fn colors(commands: &[Command]) -> Vec<Color> {
    commands
        .iter()
        .filter_map(|command| match *command {
            Command::Rect { color, .. }
            | Command::CutCornerRect { color, .. }
            | Command::Pill { color, .. }
            | Command::Border { color, .. }
            | Command::Brackets { color, .. }
            | Command::Line { color, .. }
            | Command::Text { color, .. }
            | Command::Icon { color, .. } => Some(color),
            _ => None,
        })
        .collect()
}

/// The color of the text `text`, as painted.
fn text_color(commands: &[Command], text: &str) -> Option<Color> {
    commands.iter().find_map(|command| match command {
        Command::Text {
            text: shown, color, ..
        } if shown == text => Some(*color),
        _ => None,
    })
}

/// Whether `commands` hold a rect `rect` of `token`.
fn has_rect(commands: &[Command], rect: Rect, token: Token) -> bool {
    commands.iter().any(|command| {
        *command
            == Command::Rect {
                rect,
                color: color(token),
            }
    })
}

/// Whether `inner` lies inside `outer`, give or take a unit in the last
/// place.
fn inside(inner: Rect, outer: Rect) -> bool {
    let slack = 1e-3;
    outer.x - slack <= inner.x
        && inner.right() <= outer.right() + slack
        && outer.y - slack <= inner.y
        && inner.bottom() <= outer.bottom() + slack
}

// ---------- The tab strip ----------

/// A tree holding a strip of `tabs` under the root, laid out.
fn strip_of(kind: TabKind, tabs: Vec<Tab>) -> (Tree, NodeId) {
    let mut tree = Tree::new();
    let id = tree
        .insert(tree.root(), TabStrip::new("Editors", kind, tabs).node())
        .expect("a strip");
    assert!(tree.frame(WINDOW, &METRICS).is_some());
    (tree, id)
}

fn files() -> Vec<Tab> {
    ["main.rs", "lib.rs", "README.md"]
        .map(|name| Tab::new(name).closable())
        .to_vec()
}

fn selected_tab(tree: &Tree, id: NodeId) -> Option<usize> {
    tree.widget::<TabStrip>(id).and_then(TabStrip::selected)
}

fn target(tree: &Tree, id: NodeId) -> Option<usize> {
    tree.widget::<TabStrip>(id).and_then(TabStrip::target)
}

/// The strip's tab nodes, in order.
fn tab_nodes(tree: &Tree, id: NodeId) -> Vec<NodeId> {
    let track = tree.children(id).expect("a track")[0];
    tree.children(track).expect("tabs").to_vec()
}

/// The close control of tab node `tab`.
fn close_of(tree: &Tree, tab: NodeId) -> NodeId {
    tree.children(tab).expect("a close control")[0]
}

#[test]
fn a_tab_strip_moves_its_selection_with_each_key_wrapping_at_the_ends() {
    let (mut tree, strip) = strip_of(TabKind::Content, files());
    tree.set_focus(strip, true).expect("focus");
    let step = |tree: &mut Tree, pressed: Key| {
        let response = tree.handle(&key(pressed));
        (kinds(&response, strip), response.handled)
    };
    assert_eq!(selected_tab(&tree, strip), Some(0));
    assert_eq!(
        step(&mut tree, Key::Right),
        (vec![ActionKind::Select], true)
    );
    assert_eq!(selected_tab(&tree, strip), Some(1));
    assert_eq!(target(&tree, strip), Some(1));
    step(&mut tree, Key::Right);
    // Right on the last tab wraps to the first, and Left back to the last.
    assert_eq!(
        step(&mut tree, Key::Right),
        (vec![ActionKind::Select], true)
    );
    assert_eq!(selected_tab(&tree, strip), Some(0));
    step(&mut tree, Key::Left);
    assert_eq!(selected_tab(&tree, strip), Some(2));
    assert_eq!(step(&mut tree, Key::Home).0, [ActionKind::Select]);
    assert_eq!(selected_tab(&tree, strip), Some(0));
    // Home on the first moves nothing, but keeps the key.
    assert_eq!(step(&mut tree, Key::Home), (vec![], true));
    assert_eq!(step(&mut tree, Key::End).0, [ActionKind::Select]);
    assert_eq!(selected_tab(&tree, strip), Some(2));
    // Delete asks to close the selected tab.
    assert_eq!(
        step(&mut tree, Key::Delete),
        (vec![ActionKind::Dismiss], true)
    );
    assert_eq!(target(&tree, strip), Some(2));
    assert_eq!(selected_tab(&tree, strip), Some(2));
    // Other keys, and keys with a modifier, are the owner's.
    for owner in [Key::Up, Key::Down, Key::Enter, Key::Space, Key::Tab] {
        let response = tree.handle(&key(owner));
        assert!(kinds(&response, strip).is_empty(), "{owner:?}");
        if owner != Key::Tab {
            assert!(!response.handled, "{owner:?}");
        }
        tree.set_focus(strip, true).expect("focus");
    }
    let response = tree.handle(&key_with(Key::Right, Modifiers::CTRL));
    assert!(!response.handled && response.actions.is_empty());
    assert_eq!(selected_tab(&tree, strip), Some(2));
    // A tab that cannot be closed is not asked to close.
    let (mut tree, strip) = strip_of(TabKind::Chrome, vec![Tab::new("Problems")]);
    tree.set_focus(strip, true).expect("focus");
    assert!(!tree.handle(&key(Key::Delete)).handled);
    // With no tabs, the arrows select nothing.
    let (mut tree, strip) = strip_of(TabKind::Chrome, Vec::new());
    tree.set_focus(strip, true).expect("focus");
    for pressed in [Key::Left, Key::Right, Key::Home, Key::End] {
        let response = tree.handle(&key(pressed));
        assert!(response.handled && response.actions.is_empty());
    }
    assert_eq!(selected_tab(&tree, strip), None);
    tree.frame(WINDOW, &METRICS);
    assert_eq!(
        tree.accessibility().to_string(),
        "tab list \"Editors\" = \"no tabs\" focusable focused\n"
    );
}

#[test]
fn a_press_selects_a_tab_and_its_close_control_or_a_middle_press_closes_it() {
    let (mut tree, strip) = strip_of(TabKind::Content, files());
    let nodes = tab_nodes(&tree, strip);
    let second = tree.bounds(nodes[1]).expect("bounds");
    // A press on the second tab's label selects it.
    let at = Point::new(second.x + 14.0, centre(second).y);
    assert_eq!(kinds(&click(&mut tree, at), strip), [ActionKind::Select]);
    assert_eq!(selected_tab(&tree, strip), Some(1));
    assert_eq!(tree.focused(), Some(strip));
    assert!(!tree.focus_visible());
    // A press on the third's close control asks to close it, and selects
    // nothing.
    let close = tree.bounds(close_of(&tree, nodes[2])).expect("bounds");
    assert_eq!(
        kinds(&click(&mut tree, centre(close)), strip),
        [ActionKind::Dismiss]
    );
    assert_eq!(
        (target(&tree, strip), selected_tab(&tree, strip)),
        (Some(2), Some(1))
    );
    // A middle press on the first asks to close it.
    let first = tree.bounds(nodes[0]).expect("bounds");
    let response = tree.handle(&press(MouseButton::Middle, centre(first)));
    assert_eq!(kinds(&response, strip), [ActionKind::Dismiss]);
    assert_eq!(target(&tree, strip), Some(0));
    // The owner closes it.
    widgets::update(&mut tree, strip, |tabs: &mut TabStrip| {
        tabs.remove(0);
    })
    .expect("the strip");
    tree.frame(WINDOW, &METRICS);
    assert_eq!(selected_tab(&tree, strip), Some(0));
    assert_eq!(tab_nodes(&tree, strip).len(), 2);
    // A middle press on a tab that cannot be closed is the owner's.
    let (mut tree, strip) = strip_of(TabKind::Chrome, vec![Tab::new("Problems")]);
    let tab = tree.bounds(tab_nodes(&tree, strip)[0]).expect("bounds");
    let response = tree.handle(&press(MouseButton::Middle, centre(tab)));
    assert!(!response.handled && response.actions.is_empty());
}

/// The label of the strip's target, if it has one.
fn target_label(tree: &Tree, id: NodeId) -> Option<String> {
    let tabs = tree.widget::<TabStrip>(id)?;
    Some(tabs.tabs().get(tabs.target()?)?.label().to_string())
}

#[test]
fn a_press_after_the_owner_moved_tabs_and_before_a_frame_finds_no_tab() {
    // The owner closes README.md, and a press lands where it was before a
    // frame shows that: it selects nothing.
    let (mut tree, strip) = strip_of(TabKind::Content, files());
    let readme = on_label(&tree, tab_nodes(&tree, strip)[2]);
    widgets::update(&mut tree, strip, |tabs: &mut TabStrip| {
        tabs.remove(2);
    })
    .expect("the strip");
    assert!(kinds(&click(&mut tree, readme), strip).is_empty());
    let tabs = tree.widget::<TabStrip>(strip).expect("the strip");
    assert_eq!(
        (tabs.len(), tabs.selected(), tabs.target()),
        (2, Some(0), None)
    );
    tree.frame(WINDOW, &METRICS);
    assert_eq!(
        tree.semantics(strip)
            .and_then(|s| s.value.clone())
            .as_deref(),
        Some("tab 1 of 2")
    );
    // The owner opens a tab first, and a press lands on README.md's close
    // control where it was: it closes nothing.
    let (mut tree, strip) = strip_of(TabKind::Content, files());
    let close = centre(
        tree.bounds(close_of(&tree, tab_nodes(&tree, strip)[2]))
            .expect("bounds"),
    );
    widgets::update(&mut tree, strip, |tabs: &mut TabStrip| {
        tabs.insert(0, Tab::new("new.rs").closable());
    })
    .expect("the strip");
    assert!(kinds(&click(&mut tree, close), strip).is_empty());
    assert_eq!(target(&tree, strip), None);
    // Once a frame shows the tabs, a press finds them where they are.
    tree.frame(WINDOW, &METRICS);
    let readme = tab_nodes(&tree, strip)[3];
    let close = centre(tree.bounds(close_of(&tree, readme)).expect("bounds"));
    assert_eq!(
        kinds(&click(&mut tree, close), strip),
        [ActionKind::Dismiss]
    );
    assert_eq!(target_label(&tree, strip).as_deref(), Some("README.md"));
    // A new label is a move too.
    let second = on_label(&tree, tab_nodes(&tree, strip)[1]);
    widgets::update(&mut tree, strip, |tabs: &mut TabStrip| {
        tabs.set_label(0, "a_much_longer_name.rs");
    })
    .expect("the strip");
    assert!(kinds(&click(&mut tree, second), strip).is_empty());
    // The owner's select moves no tab: a press still finds them.
    tree.frame(WINDOW, &METRICS);
    widgets::update(&mut tree, strip, |tabs: &mut TabStrip| tabs.select(3)).expect("the strip");
    let second = on_label(&tree, tab_nodes(&tree, strip)[1]);
    assert_eq!(
        kinds(&click(&mut tree, second), strip),
        [ActionKind::Select]
    );
    assert_eq!(selected_tab(&tree, strip), Some(1));
}

#[test]
fn the_target_moves_with_its_tab_and_is_none_once_its_tab_is_removed() {
    let tabs = ["a", "b", "c", "d"].map(|name| Tab::new(name).closable());
    let (mut tree, strip) = strip_of(TabKind::Content, tabs.to_vec());
    tree.set_focus(strip, true).expect("focus");
    tree.handle(&key(Key::End));
    tree.handle(&key(Key::Left));
    assert_eq!(
        kinds(&tree.handle(&key(Key::Delete)), strip),
        [ActionKind::Dismiss]
    );
    assert_eq!(target_label(&tree, strip).as_deref(), Some("c"));
    // The owner closes and opens other tabs first: the target is still c.
    let change = |tree: &mut Tree, change: fn(&mut TabStrip)| {
        widgets::update(tree, strip, change).expect("the strip");
        target_label(tree, strip)
    };
    let removed = change(&mut tree, |tabs| {
        tabs.remove(0);
    });
    assert_eq!(
        (removed.as_deref(), target(&tree, strip)),
        (Some("c"), Some(1))
    );
    let opened = change(&mut tree, |tabs| {
        tabs.insert(0, Tab::new("z"));
    });
    assert_eq!(opened.as_deref(), Some("c"));
    let opened = change(&mut tree, |tabs| {
        tabs.insert(9, Tab::new("e"));
    });
    assert_eq!(opened.as_deref(), Some("c"));
    let removed = change(&mut tree, |tabs| {
        tabs.remove(3);
    });
    assert_eq!(
        (removed.as_deref(), target(&tree, strip)),
        (Some("c"), Some(2))
    );
    // Closing it leaves no target.
    let closed = change(&mut tree, |tabs| {
        tabs.remove(2);
    });
    assert_eq!(closed, None);
    assert_eq!(target(&tree, strip), None);
    // The last tab, asked to close, then a tab before it closed: the
    // target is still one of the tabs, and still it.
    let (mut tree, strip) = strip_of(TabKind::Content, files());
    tree.set_focus(strip, true).expect("focus");
    tree.handle(&key(Key::End));
    tree.handle(&key(Key::Delete));
    widgets::update(&mut tree, strip, |tabs: &mut TabStrip| {
        tabs.remove(0);
    })
    .expect("the strip");
    let tabs = tree.widget::<TabStrip>(strip).expect("the strip");
    assert!(tabs.target().is_some_and(|t| t < tabs.len()));
    assert_eq!(target_label(&tree, strip).as_deref(), Some("README.md"));
}

/// Whether the strip's selected tab lies inside its track.
fn selected_shown(tree: &Tree, strip: NodeId) -> bool {
    let track = tree.children(strip).expect("a track")[0];
    let Some(selected) = selected_tab(tree, strip) else {
        return false;
    };
    let tab = tree
        .bounds(tab_nodes(tree, strip)[selected])
        .expect("bounds");
    inside(tab, tree.bounds(track).expect("bounds"))
}

#[test]
fn the_selected_tab_is_revealed_again_when_tabs_before_it_change_or_the_track_narrows() {
    let tabs = (1..=20).map(|n| Tab::new(format!("file_{n}.rs")).closable());
    let mut strip = TabStrip::new("Editors", TabKind::Content, tabs);
    strip.select(19);
    let mut tree = Tree::new();
    let strip = tree.insert(tree.root(), strip.node()).expect("a strip");
    let wide = Size::new(800.0, 40.0);
    tree.frame(wide, &METRICS);
    assert!(selected_shown(&tree, strip));
    // The window narrows, and widens again.
    tree.frame(Size::new(400.0, 40.0), &METRICS);
    assert!(selected_shown(&tree, strip));
    tree.frame(wide, &METRICS);
    assert!(selected_shown(&tree, strip));
    // A tab opened before it, and one before it relabelled longer, push
    // it past the track's end.
    widgets::update(&mut tree, strip, |tabs: &mut TabStrip| {
        tabs.insert(0, Tab::new("a_very_long_name_inserted_first.rs"));
    })
    .expect("the strip");
    tree.frame(wide, &METRICS);
    assert_eq!(selected_tab(&tree, strip), Some(20));
    assert!(selected_shown(&tree, strip));
    widgets::update(&mut tree, strip, |tabs: &mut TabStrip| {
        tabs.set_label(1, "x".repeat(60));
    })
    .expect("the strip");
    tree.frame(wide, &METRICS);
    assert!(selected_shown(&tree, strip));
    assert_eq!(tree.frame(wide, &METRICS), None);
    // A sideways wheel scrolls away from it, and nothing brings it back
    // until it moves.
    let track = tree.children(strip).expect("a track")[0];
    let before = tree.scroll_offset(track).expect("an offset");
    tree.handle(&InputEvent::Wheel {
        delta: ScrollDelta::Logical { x: -300.0, y: 0.0 },
        position: centre(tree.bounds(track).expect("bounds")),
        modifiers: Modifiers::NONE,
    });
    assert!(tree.frame(wide, &METRICS).is_some());
    assert_ne!(tree.scroll_offset(track), Some(before));
    assert!(!selected_shown(&tree, strip));
    assert_eq!(tree.frame(wide, &METRICS), None);
    assert!(!selected_shown(&tree, strip));
    tree.frame(Size::new(600.0, 40.0), &METRICS);
    assert!(selected_shown(&tree, strip));
}

#[test]
fn a_tab_strip_and_a_collection_reveal_their_selections_in_one_turn_in_either_order() {
    for collection_first in [true, false] {
        for scrolled in [false, true] {
            let case = format!("collection first {collection_first}, in a scroller {scrolled}");
            let mut tree = Tree::new();
            let root = tree.root();
            let add_list = |tree: &mut Tree| -> (NodeId, NodeId) {
                let rows: Vec<Row> = (0..100).map(|n| Row::new(format!("row {n}"))).collect();
                let node = Collection::list("Files", rows).node();
                if scrolled {
                    let scroller = Node::new(
                        Style::column()
                            .width(Length::Fill(1.0))
                            .height(Length::Fixed(100.0))
                            .scroll(false, true),
                    );
                    let scroller = tree.insert(root, scroller).expect("a scroller");
                    (tree.insert(scroller, node).expect("a list"), scroller)
                } else {
                    let node = Node {
                        style: Style {
                            height: Length::Fixed(100.0),
                            ..node.style
                        },
                        ..node
                    };
                    let list = tree.insert(root, node).expect("a list");
                    (list, list)
                }
            };
            let add_strip = |tree: &mut Tree| {
                let tabs = (1..=20).map(|n| Tab::new(format!("file_{n}.rs")).closable());
                tree.insert(
                    root,
                    TabStrip::new("Editors", TabKind::Content, tabs).node(),
                )
                .expect("a strip")
            };
            let ((list, area), strip) = if collection_first {
                let list = add_list(&mut tree);
                (list, add_strip(&mut tree))
            } else {
                let strip = add_strip(&mut tree);
                (add_list(&mut tree), strip)
            };
            tree.frame(WINDOW, &METRICS);
            // The owner opens file_20.rs: it selects its tab, and its row,
            // and asks for the list, outside the strip, to be in view.
            widgets::update(&mut tree, strip, |tabs: &mut TabStrip| tabs.select(19))
                .expect("the strip");
            widgets::update(&mut tree, list, |list: &mut Collection| {
                list.select(Some(80));
            })
            .expect("the list");
            tree.scroll_into_view(list).expect("the list");
            tree.frame(WINDOW, &METRICS);
            assert!(selected_shown(&tree, strip), "{case}");
            let snapshot = tree.accessibility();
            let row = snapshot.nodes().iter().find(|node| node.name == "row 80");
            let row = row.unwrap_or_else(|| panic!("{case}: row 80 is not kept"));
            let area = tree.bounds(area).expect("bounds");
            assert!(
                inside(row.bounds, area),
                "{case}: {:?} in {area:?}",
                row.bounds
            );
            assert_eq!(tree.frame(WINDOW, &METRICS), None, "{case}");
        }
    }
}

#[test]
fn a_tab_strip_is_one_tab_stop_whose_tabs_are_named_as_written() {
    let mut tabs = files();
    tabs.push(Tab::new("  "));
    let (mut tree, strip) = strip_of(TabKind::Content, tabs);
    let root = tree.root();
    tree.insert(root, Button::new("After", ButtonKind::Ghost).node())
        .expect("a button");
    tree.frame(WINDOW, &METRICS);
    let stops: Vec<NodeId> = tree
        .tab_order()
        .into_iter()
        .filter(|&id| id == strip || tree.parent(id) != Some(root))
        .collect();
    assert_eq!(stops, [strip], "one tab stop");
    assert_eq!(
        tree.accessibility().to_string(),
        "tab list \"Editors\" = \"tab 1 of 4\" focusable\n\
         \x20 tab \"main.rs\" selected\n\
         \x20   button \"Close main.rs\"\n\
         \x20 tab \"lib.rs\"\n\
         \x20   button \"Close lib.rs\"\n\
         \x20 tab \"README.md\"\n\
         \x20   button \"Close README.md\"\n\
         \x20 tab \"tab 4\"\n\
         button \"After\" focusable\n"
    );
    // Chrome is painted uppercase, and named as written.
    let (mut tree, _) = strip_of(TabKind::Chrome, vec![Tab::new("Problems")]);
    let commands = painted(&mut tree, WINDOW);
    assert!(text_color(&commands, "PROBLEMS").is_some());
    assert!(
        tree.accessibility()
            .to_string()
            .contains("tab \"Problems\" selected")
    );
    // Content is painted as written.
    let (mut tree, _) = strip_of(TabKind::Content, vec![Tab::new("Cargo.toml")]);
    assert!(text_color(&painted(&mut tree, WINDOW), "Cargo.toml").is_some());
}

#[test]
fn tabs_that_do_not_fit_scroll_the_selected_tab_into_view() {
    let tabs: Vec<Tab> = (1..=30)
        .map(|n| Tab::new(format!("file_{n}.rs")).closable())
        .collect();
    let mut tree = Tree::new();
    let strip = TabStrip::new("Editors", TabKind::Content, tabs).node();
    let strip = Node {
        style: Style::column()
            .width(Length::Fixed(300.0))
            .padding(tidepool_ui::layout::Insets::all(FOCUS_WIDTH)),
        ..strip
    };
    let strip = tree.insert(tree.root(), strip).expect("a strip");
    tree.frame(WINDOW, &METRICS);
    let track = tree.children(strip).expect("a track")[0];
    let shown = tree.bounds(track).expect("bounds");
    tree.set_focus(strip, true).expect("focus");
    tree.handle(&key(Key::End));
    tree.frame(WINDOW, &METRICS);
    let last = tree
        .bounds(*tab_nodes(&tree, strip).last().expect("a tab"))
        .expect("bounds");
    assert!(inside(last, shown), "{last:?} in {shown:?}");
    assert!(tree.scroll_offset(track).is_some_and(|at| at.x > 0.0));
    // The first is now out of view, and a press where it was selects the
    // tab shown there.
    let first = tree.bounds(tab_nodes(&tree, strip)[0]).expect("bounds");
    assert!(first.right() <= shown.x);
    let at = Point::new(shown.x + 5.0, centre(shown).y);
    click(&mut tree, at);
    let picked = selected_tab(&tree, strip).expect("a tab");
    let picked = tree
        .bounds(tab_nodes(&tree, strip)[picked])
        .expect("bounds");
    assert!(picked.contains(at), "{picked:?}");
    // Home brings the first back.
    tree.handle(&key(Key::Home));
    tree.frame(WINDOW, &METRICS);
    let first = tree.bounds(tab_nodes(&tree, strip)[0]).expect("bounds");
    assert!(inside(first, shown), "{first:?}");
    assert_eq!(tree.frame(WINDOW, &METRICS), None);
}

#[test]
fn a_tab_strip_caps_its_labels_and_its_tabs() {
    // Ten thousand clusters of three bytes: the cap falls inside one.
    let long = "e\u{301}".repeat(10_000);
    let (mut tree, strip) = strip_of(TabKind::Content, vec![Tab::new(long.clone())]);
    let name = tree.accessibility().nodes()[1].name.clone();
    assert!(name.len() <= MAX_TAB_LABEL_BYTES && !name.is_empty());
    assert!(grapheme::is_boundary(&long, name.len()) && long.starts_with(&name));
    let commands = painted(&mut tree, WINDOW);
    assert!(text_color(&commands, &name).is_some());
    assert_eq!(tab_nodes(&tree, strip).len(), 1);
    let mut full = TabStrip::new(
        "Editors",
        TabKind::Content,
        (0..MAX_TABS + 5).map(|n| Tab::new(format!("{n}"))),
    );
    assert_eq!(full.len(), MAX_TABS);
    assert!(!full.insert(0, Tab::new("more")));
    assert_eq!(full.len(), MAX_TABS);
    // The strip's own name, the owner's chrome, is capped as a label is.
    let named = TabStrip::new(long.clone(), TabKind::Chrome, Vec::new());
    assert!(named.name().len() <= MAX_TAB_LABEL_BYTES && long.starts_with(named.name()));
    assert!(grapheme::is_boundary(&long, named.name().len()));
}

/// The colors of the strokes `commands` draw inside `rect`.
fn strokes_in(commands: &[Command], rect: Rect) -> Vec<Color> {
    commands
        .iter()
        .filter_map(|command| match *command {
            Command::Line {
                from, to, color, ..
            } if rect.contains(from) && rect.contains(to) => Some(color),
            _ => None,
        })
        .collect()
}

/// A strip of [`files`] after `act`, painted: its commands, its tabs'
/// bounds and their close controls'.
fn tab_scene(act: fn(&mut Tree, &[NodeId])) -> (Vec<Command>, Vec<Rect>, Vec<Rect>) {
    let (mut tree, strip) = strip_of(TabKind::Content, files());
    let nodes = tab_nodes(&tree, strip);
    act(&mut tree, &nodes);
    let commands = painted(&mut tree, WINDOW);
    let tabs = nodes
        .iter()
        .map(|&node| tree.bounds(node).expect("bounds"))
        .collect();
    let closes = nodes
        .iter()
        .map(|&node| tree.bounds(close_of(&tree, node)).expect("bounds"))
        .collect();
    (commands, tabs, closes)
}

/// A point on tab `tab`'s label, clear of its close control.
fn on_label(tree: &Tree, tab: NodeId) -> Point {
    let at = tree.bounds(tab).expect("bounds");
    Point::new(at.x + 14.0, centre(at).y)
}

#[test]
fn tabs_paint_each_state_in_the_theme_s_tokens() {
    use Token::*;
    // At rest: the selected tab's `teal` rule and `text` label, another's
    // `text-2` label and no fill, and `text-2` crosses.
    let (commands, tabs, closes) = tab_scene(|_, _| {});
    let rule = |tab: Rect| Rect::new(tab.x, tab.bottom() - TAB_RULE, tab.width, TAB_RULE);
    assert!(has_rect(&commands, rule(tabs[0]), Teal));
    assert!(!has_rect(&commands, rule(tabs[1]), Teal));
    assert_eq!(text_color(&commands, "main.rs"), Some(color(Text)));
    assert_eq!(text_color(&commands, "lib.rs"), Some(color(Text2)));
    for tab in &tabs {
        assert!(!has_rect(&commands, *tab, RowHover) && !has_rect(&commands, *tab, TealWash));
    }
    assert_eq!(
        strokes_in(&commands, closes[1]),
        [color(Text2), color(Text2)]
    );
    // Hovered: the wash and a `teal-hi` label.
    let (commands, tabs, _) = tab_scene(|tree, nodes| {
        let at = on_label(tree, nodes[1]);
        tree.handle(&hover(at));
    });
    assert!(has_rect(&commands, tabs[1], RowHover));
    assert_eq!(text_color(&commands, "lib.rs"), Some(color(TealHi)));
    // Pressed: `teal-wash`; the press selects it, so it has the rule too.
    let (commands, tabs, _) = tab_scene(|tree, nodes| {
        let at = on_label(tree, nodes[1]);
        tree.handle(&press(MouseButton::Left, at));
        tree.frame(WINDOW, &METRICS);
    });
    assert!(has_rect(&commands, tabs[1], TealWash));
    assert!(has_rect(&commands, rule(tabs[1]), Teal));
    assert_eq!(text_color(&commands, "lib.rs"), Some(color(TealHi)));
    // A hovered close control: a `teal-wash` circle and a `teal-hi` cross;
    // pressed, `teal-fill`.
    let (commands, _, closes) = tab_scene(|tree, nodes| {
        let at = tree.bounds(close_of(tree, nodes[1])).expect("bounds");
        tree.handle(&hover(centre(at)));
    });
    assert!(commands.contains(&Command::Pill {
        rect: closes[1],
        color: color(TealWash),
    }));
    assert_eq!(
        strokes_in(&commands, closes[1]),
        [color(TealHi), color(TealHi)]
    );
    let (commands, _, closes) = tab_scene(|tree, nodes| {
        let at = tree.bounds(close_of(tree, nodes[1])).expect("bounds");
        tree.handle(&press(MouseButton::Left, centre(at)));
    });
    assert!(commands.contains(&Command::Pill {
        rect: closes[1],
        color: color(TealFill),
    }));
    // The rule under the tabs.
    let (mut tree, strip) = strip_of(TabKind::Content, files());
    let commands = painted(&mut tree, WINDOW);
    let track = tree
        .bounds(tree.children(strip).expect("a track")[0])
        .expect("bounds");
    assert!(has_rect(
        &commands,
        Rect::new(track.x, track.bottom() - 1.0, track.width, 1.0),
        Line
    ));
    // Focus: the outline inside the strip.
    tree.set_focus(strip, true).expect("focus");
    let commands = painted(&mut tree, WINDOW);
    let bounds = tree.bounds(strip).expect("bounds");
    assert!(commands.contains(&Command::Border {
        rect: bounds,
        width: FOCUS_WIDTH,
        color: color(Teal),
    }));
    // Disabled, hovered: its rest look in a layer.
    tree.set_enabled(strip, false).expect("disabled");
    let at = on_label(&tree, tab_nodes(&tree, strip)[1]);
    tree.handle(&hover(at));
    let commands = painted(&mut tree, WINDOW);
    assert!(commands.contains(&Command::PushLayer {
        opacity: DISABLED_OPACITY
    }));
    assert_eq!(text_color(&commands, "lib.rs"), Some(color(Text2)));
    assert!(!colors(&commands).contains(&color(RowHover)));
}

#[test]
fn a_tab_strip_damages_only_the_tabs_it_changes_and_idles() {
    let (mut tree, strip) = strip_of(TabKind::Content, files());
    assert_eq!(tree.frame(WINDOW, &METRICS), None);
    // Focus shows from the keyboard, so it is shown first.
    tree.set_focus(strip, true).expect("focus");
    tree.frame(WINDOW, &METRICS);
    tree.handle(&key(Key::Right));
    let frame = tree.frame(WINDOW, &METRICS).expect("the selection moved");
    let nodes = tab_nodes(&tree, strip);
    let changed = [
        tree.bounds(nodes[0]).expect("bounds"),
        tree.bounds(nodes[1]).expect("bounds"),
    ];
    // It damages both tabs, the one that lost the selection and the one
    // that took it, across each and over the rule each lost or gained,
    // and nothing else.
    for rect in &frame.damage {
        assert!(changed.iter().any(|tab| inside(*rect, *tab)), "{rect:?}");
    }
    for tab in changed {
        let rule = Rect::new(tab.x, tab.bottom() - TAB_RULE, tab.width, TAB_RULE);
        assert!(
            frame.damage.iter().any(|rect| inside(rule, *rect)),
            "{rule:?} in {:?}",
            frame.damage
        );
    }
    assert_eq!(tree.frame(WINDOW, &METRICS), None);
}

/// One thing done to a tab strip in the property test.
#[derive(Clone, Debug)]
enum TabOp {
    Key(Key),
    /// A press, on a tab or its close control.
    Press {
        tab: usize,
        close: bool,
    },
    Insert {
        at: usize,
        closable: bool,
    },
    Remove(usize),
    /// The owner closes the tab a Dismiss named.
    CloseTarget,
}

fn tab_op(rng: &mut Pcg32) -> TabOp {
    match rng.range(0..9u32) {
        0..=2 => TabOp::Key(
            *rng.choose(&[
                Key::Left,
                Key::Right,
                Key::Home,
                Key::End,
                Key::Delete,
                Key::Up,
            ])
            .expect("keys"),
        ),
        3 | 4 => TabOp::Press {
            tab: rng.range(0..12u32) as usize,
            close: rng.range(0..2u32) == 0,
        },
        5 | 6 => TabOp::Insert {
            at: rng.range(0..12u32) as usize,
            closable: rng.range(0..2u32) == 0,
        },
        7 => TabOp::Remove(rng.range(0..12u32) as usize),
        _ => TabOp::CloseTarget,
    }
}

#[test]
fn random_inserts_closes_keys_and_presses_keep_one_selection_and_one_tab_stop() {
    check_sized(
        "random tab strip use",
        200,
        |rng, size| {
            let first = rng.range(0..6u32);
            // Each op, and whether a frame follows it: the owner's changes
            // and the user's presses may come between two frames.
            let ops: Vec<(TabOp, bool)> = (0..size.max(1))
                .map(|_| (tab_op(rng), rng.range(0..3u32) != 0))
                .collect();
            (first, ops)
        },
        |(first, ops)| {
            let tabs = (0..*first).map(|n| Tab::new(format!("t{n}")).closable());
            let mut tree = Tree::new();
            let strip = tree
                .insert(
                    tree.root(),
                    TabStrip::new("Tabs", TabKind::Content, tabs).node(),
                )
                .map_err(|error| error.to_string())?;
            tree.frame(WINDOW, &METRICS);
            tree.set_focus(strip, true)
                .map_err(|error| error.to_string())?;
            let mut count = 0;
            for (step, (op, framed)) in ops.iter().enumerate() {
                let aimed = target_label(&tree, strip);
                match *op {
                    TabOp::Key(pressed) => {
                        tree.handle(&key(pressed));
                    }
                    TabOp::Press { tab, close } => {
                        if let Some(&node) = tab_nodes(&tree, strip).get(tab) {
                            let at = match tree.children(node).and_then(|c| c.first()) {
                                Some(&close_node) if close => tree.bounds(close_node),
                                _ => tree.bounds(node),
                            };
                            if let Some(at) = at {
                                click(&mut tree, centre(at));
                            }
                        }
                    }
                    TabOp::Insert { at, closable } => {
                        count += 1;
                        let tab = if closable {
                            Tab::new(format!("n{count}")).closable()
                        } else {
                            Tab::new(format!("n{count}"))
                        };
                        widgets::update(&mut tree, strip, |tabs: &mut TabStrip| {
                            tabs.insert(at, tab);
                        })
                        .map_err(|error| error.to_string())?;
                    }
                    TabOp::Remove(index) => {
                        widgets::update(&mut tree, strip, |tabs: &mut TabStrip| {
                            tabs.remove(index);
                        })
                        .map_err(|error| error.to_string())?;
                    }
                    TabOp::CloseTarget => {
                        widgets::update(&mut tree, strip, |tabs: &mut TabStrip| {
                            if let Some(index) = tabs.target() {
                                tabs.remove(index);
                            }
                        })
                        .map_err(|error| error.to_string())?;
                    }
                }
                // With or without a frame: the selection and the target are
                // always tabs, and the owner's changes keep the target on
                // its tab until it is removed.
                let at = |error: String| format!("after step {step}, {op:?}: {error}");
                let tabs = tree.widget::<TabStrip>(strip).ok_or("no strip")?;
                let (len, selected, aim) = (tabs.len(), tabs.selected(), tabs.target());
                if (len == 0) != selected.is_none() || selected.is_some_and(|s| s >= len) {
                    return Err(at(format!("selection {selected:?} of {len}")));
                }
                if aim.is_some_and(|t| t >= len) {
                    return Err(at(format!("target {aim:?} of {len}")));
                }
                let kept = target_label(&tree, strip);
                let owners = matches!(op, TabOp::Insert { .. } | TabOp::Remove(_));
                if owners && kept.is_some() && kept != aimed {
                    return Err(at(format!("the target moved to {kept:?} from {aimed:?}")));
                }
                if matches!(op, TabOp::CloseTarget) && aimed.is_some() && kept.is_some() {
                    return Err(at("the closed target is still named".into()));
                }
                if !framed {
                    continue;
                }
                tree.frame(WINDOW, &METRICS);
                let check = || -> Result<(), String> {
                    let tabs = tree.widget::<TabStrip>(strip).ok_or("no strip")?;
                    let (len, selected) = (tabs.len(), tabs.selected());
                    if (len == 0) != selected.is_none() || selected.is_some_and(|s| s >= len) {
                        return Err(format!("selection {selected:?} of {len}"));
                    }
                    if tab_nodes(&tree, strip).len() != len {
                        return Err(format!(
                            "{} tab nodes for {len}",
                            tab_nodes(&tree, strip).len()
                        ));
                    }
                    let snapshot = tree.accessibility();
                    let shown: Vec<_> = snapshot
                        .nodes()
                        .iter()
                        .filter(|n| n.role == Role::Tab)
                        .collect();
                    let marked = shown.iter().filter(|n| n.selected).count();
                    if marked != usize::from(len > 0) || shown.len() != len {
                        return Err(format!("{marked} selected of {} tabs", shown.len()));
                    }
                    if shown.iter().any(|n| n.name.trim().is_empty()) {
                        return Err("a tab with no name".into());
                    }
                    let stops: Vec<NodeId> = tree.tab_order();
                    if stops != [strip] {
                        return Err(format!("tab stops {stops:?}"));
                    }
                    Ok(())
                };
                check().map_err(at)?;
                if tree.frame(WINDOW, &METRICS).is_some() {
                    return Err(format!("after step {step}: an idle frame drew"));
                }
            }
            Ok(())
        },
    );
}

// ---------- The splitter ----------

/// A split on `axis` at `ratio` filling a window of [`WINDOW`], with panes
/// of minimum `min` each: the split and its divider.
fn split(axis: Axis, ratio: f32, min: f32) -> (Tree, NodeId, NodeId) {
    let mut tree = Tree::new();
    let pane = || {
        let min = match axis {
            Axis::Horizontal => Size::new(min, 0.0),
            Axis::Vertical => Size::new(0.0, min),
        };
        Node::new(Style::stack().fill().min(min))
    };
    let root = tree.root();
    let split = Splitter::new(axis, "Side panel", ratio)
        .insert(&mut tree, root, pane(), pane())
        .expect("a split");
    tree.frame(WINDOW, &METRICS);
    let divider = Splitter::divider(&tree, split).expect("a divider");
    (tree, split, divider)
}

fn ratio(tree: &Tree, divider: NodeId) -> f32 {
    tree.widget::<Splitter>(divider)
        .map_or(f32::NAN, Splitter::ratio)
}

fn first_pane(tree: &Tree, split: NodeId) -> Rect {
    tree.bounds(tree.children(split).expect("panes")[0])
        .expect("bounds")
}

#[test]
fn a_splitter_steps_with_the_keys_along_its_axis_and_keeps_the_panes_minimums() {
    for axis in [Axis::Horizontal, Axis::Vertical] {
        let (mut tree, split, divider) = split(axis, 0.5, 50.0);
        let extent = match axis {
            Axis::Horizontal => WINDOW.width,
            Axis::Vertical => WINDOW.height,
        };
        let (back, on, across) = match axis {
            Axis::Horizontal => (Key::Left, Key::Right, Key::Up),
            Axis::Vertical => (Key::Up, Key::Down, Key::Left),
        };
        let length = |tree: &Tree| match axis {
            Axis::Horizontal => first_pane(tree, split).width,
            Axis::Vertical => first_pane(tree, split).height,
        };
        tree.set_focus(divider, true).expect("focus");
        let response = tree.handle(&key(on));
        assert_eq!(kinds(&response, divider), [ActionKind::Change]);
        tree.frame(WINDOW, &METRICS);
        assert!(
            (length(&tree) - (extent / 2.0 + SPLITTER_STEP)).abs() < 0.01,
            "{axis:?}"
        );
        tree.handle(&key(back));
        tree.handle(&key(back));
        tree.frame(WINDOW, &METRICS);
        assert!((length(&tree) - (extent / 2.0 - SPLITTER_STEP)).abs() < 0.01);
        // The keys across its axis are the owner's.
        let response = tree.handle(&key(across));
        assert!(!response.handled && response.actions.is_empty());
        // Home and End take it to the panes' minimums.
        assert_eq!(
            kinds(&tree.handle(&key(Key::Home)), divider),
            [ActionKind::Change]
        );
        tree.frame(WINDOW, &METRICS);
        assert!((length(&tree) - 50.0).abs() < 0.01, "{}", length(&tree));
        assert_eq!(tree.handle(&key(Key::Home)).actions, []);
        tree.handle(&key(Key::End));
        tree.frame(WINDOW, &METRICS);
        let second = extent - length(&tree) - SPLITTER_THICKNESS;
        assert!((second - 50.0).abs() < 0.01, "{second}");
        // Stepping past the end moves nothing.
        assert_eq!(tree.handle(&key(on)).actions, []);
        let response = tree.handle(&key_with(back, Modifiers::SHIFT));
        assert!(!response.handled);
    }
}

#[test]
fn dragging_a_splitter_moves_it_with_the_pointer_and_its_value_follows() {
    let (mut tree, split, divider) = split(Axis::Horizontal, 0.25, 40.0);
    let bar = tree.bounds(divider).expect("bounds");
    assert_eq!(bar.x, 100.0);
    assert_eq!(
        tree.semantics(divider)
            .and_then(|s| s.value.clone())
            .as_deref(),
        Some("25%")
    );
    let grab = Point::new(bar.x + 3.0, 150.0);
    tree.handle(&press(MouseButton::Left, grab));
    assert_eq!(tree.focused(), Some(divider));
    let response = tree.handle(&hover(Point::new(203.0, 150.0)));
    assert_eq!(kinds(&response, divider), [ActionKind::Change]);
    tree.frame(WINDOW, &METRICS);
    assert!((first_pane(&tree, split).width - 200.0).abs() < 0.01);
    assert!((ratio(&tree, divider) - 0.5).abs() < 1e-4);
    assert_eq!(
        tree.semantics(divider)
            .and_then(|s| s.value.clone())
            .as_deref(),
        Some("50%")
    );
    // Past the second pane's minimum, it stops there.
    tree.handle(&hover(Point::new(399.0, 150.0)));
    tree.frame(WINDOW, &METRICS);
    let first = first_pane(&tree, split).width;
    assert!(
        (400.0 - first - SPLITTER_THICKNESS - 40.0).abs() < 0.01,
        "{first}"
    );
    tree.handle(&release(Point::new(399.0, 150.0)));
    // After the release, the pointer moves nothing, and the frame after
    // the one that shows it at rest is idle.
    assert!(
        tree.handle(&hover(Point::new(100.0, 150.0)))
            .actions
            .is_empty()
    );
    tree.frame(WINDOW, &METRICS);
    assert_eq!(tree.frame(WINDOW, &METRICS), None);
    // Disabled, it moves with neither.
    tree.set_enabled(divider, false).expect("disabled");
    let at = tree.bounds(divider).expect("bounds");
    tree.handle(&press(MouseButton::Left, centre(at)));
    assert!(
        tree.handle(&hover(Point::new(50.0, 150.0)))
            .actions
            .is_empty()
    );
}

#[test]
fn a_splitter_paints_each_state_in_the_theme_s_tokens() {
    let looks = |act: fn(&mut Tree, NodeId)| {
        let (mut tree, _, divider) = split(Axis::Horizontal, 0.5, 0.0);
        act(&mut tree, divider);
        let bounds = tree.bounds(divider).expect("bounds");
        (painted(&mut tree, WINDOW), bounds)
    };
    // At rest, a 1-unit `line` rule down its middle.
    let (commands, bounds) = looks(|_, _| {});
    assert!(has_rect(
        &commands,
        Rect::new(bounds.x + 3.5, bounds.y, 1.0, bounds.height),
        Token::Line
    ));
    // Hovered, a 2-unit `teal-glow` bar; pressed, `teal`.
    let (commands, bounds) = looks(|tree, divider| {
        let at = tree.bounds(divider).expect("bounds");
        tree.handle(&hover(centre(at)));
    });
    assert!(has_rect(
        &commands,
        Rect::new(bounds.x + 3.0, bounds.y, 2.0, bounds.height),
        Token::TealGlow
    ));
    let (commands, bounds) = looks(|tree, divider| {
        let at = tree.bounds(divider).expect("bounds");
        tree.handle(&press(MouseButton::Left, centre(at)));
    });
    assert!(has_rect(
        &commands,
        Rect::new(bounds.x + 3.0, bounds.y, 2.0, bounds.height),
        Token::Teal
    ));
    // Focus: the outline inside it.
    let (commands, bounds) = looks(|tree, divider| tree.set_focus(divider, true).expect("focus"));
    assert!(commands.contains(&Command::Border {
        rect: bounds,
        width: FOCUS_WIDTH,
        color: color(Token::Teal),
    }));
    // Disabled: the rest look in a layer.
    let (commands, _) = looks(|tree, divider| {
        tree.set_enabled(divider, false).expect("disabled");
        let at = tree.bounds(divider).expect("bounds");
        tree.handle(&hover(centre(at)));
    });
    assert!(commands.contains(&Command::PushLayer {
        opacity: DISABLED_OPACITY
    }));
    assert!(colors(&commands).contains(&color(Token::Line)));
    assert!(!colors(&commands).contains(&color(Token::TealGlow)));
}

/// The value the divider shows in the accessibility tree.
fn shown(tree: &Tree, divider: NodeId) -> Option<String> {
    tree.semantics(divider).and_then(|s| s.value.clone())
}

#[test]
fn a_splitter_keeps_its_ratio_through_a_split_too_small_for_it() {
    // A quarter, with a second pane of at least 400: a window of 1000 has
    // room for it, one of 500 does not.
    let mut tree = Tree::new();
    let root = tree.root();
    let split = Splitter::new(Axis::Horizontal, "Side panel", 0.25)
        .insert(
            &mut tree,
            root,
            Node::new(Style::stack().fill()),
            Node::new(Style::stack().fill().min(Size::new(400.0, 0.0))),
        )
        .expect("a split");
    let divider = Splitter::divider(&tree, split).expect("a divider");
    let (wide, narrow) = (Size::new(1000.0, 300.0), Size::new(500.0, 300.0));
    tree.frame(wide, &METRICS);
    assert!((first_pane(&tree, split).width - 250.0).abs() < 0.01);
    tree.frame(narrow, &METRICS);
    let first = first_pane(&tree, split).width;
    assert!(
        (first - (500.0 - SPLITTER_THICKNESS - 400.0)).abs() < 0.01,
        "{first}"
    );
    assert_eq!(shown(&tree, divider).as_deref(), Some("18%"));
    // It keeps the quarter it was given, and has it back with the room.
    assert_eq!(ratio(&tree, divider), 0.25);
    tree.frame(wide, &METRICS);
    assert!((first_pane(&tree, split).width - 250.0).abs() < 0.01);
    assert_eq!(shown(&tree, divider).as_deref(), Some("25%"));
    assert_eq!(tree.frame(wide, &METRICS), None);
    // The keys step from where it shows: in the narrow window, Left moves
    // it 10 from 92, and that is the ratio it keeps then.
    tree.frame(narrow, &METRICS);
    tree.set_focus(divider, true).expect("focus");
    assert_eq!(
        kinds(&tree.handle(&key(Key::Left)), divider),
        [ActionKind::Change]
    );
    tree.frame(narrow, &METRICS);
    assert!((first_pane(&tree, split).width - 82.0).abs() < 0.01);
    assert!((ratio(&tree, divider) - 82.0 / 500.0).abs() < 1e-4);
}

/// Three splits, each in the second pane of the one before: A's first
/// pane at least 400 wide, B's second at least 300 and C's at least 150.
fn nested(tree: &mut Tree) -> [NodeId; 3] {
    let min = |width: f32| Node::new(Style::stack().fill().min(Size::new(width, 0.0)));
    let root = tree.root();
    let a = Splitter::new(Axis::Horizontal, "A", 0.2)
        .insert(tree, root, min(400.0), min(0.0))
        .expect("a split");
    let into = tree.children(a).expect("panes")[2];
    let b = Splitter::new(Axis::Horizontal, "B", 0.6)
        .insert(tree, into, min(0.0), min(300.0))
        .expect("a split");
    let into = tree.children(b).expect("panes")[2];
    let c = Splitter::new(Axis::Horizontal, "C", 0.5)
        .insert(tree, into, min(0.0), min(150.0))
        .expect("a split");
    [a, b, c]
}

#[test]
fn nested_splits_settle_in_one_frame_each_in_the_space_it_has() {
    let mut tree = Tree::new();
    let splits = nested(&mut tree);
    let dividers = splits.map(|split| Splitter::divider(&tree, split).expect("a divider"));
    let values = |tree: &Tree| dividers.map(|divider| shown(tree, divider).unwrap_or_default());
    let size = Size::new(1000.0, 300.0);
    tree.frame(size, &METRICS);
    // A is held at its first pane's 400; B at its second's 300 of 592; C
    // at its second's 150 of the 300 it has, (300 - 8 - 150) / 300, not at
    // a limit from a pass that gave it less.
    assert_eq!(values(&tree), ["40%", "48%", "47%"]);
    let c = tree.children(splits[2]).expect("panes").to_vec();
    assert!((tree.bounds(c[2]).expect("bounds").width - 150.0).abs() < 0.01);
    assert_eq!(tree.frame(size, &METRICS), None);
    // Each keeps the ratio it was given, which a window with room shows.
    assert_eq!(
        dividers.map(|divider| ratio(&tree, divider)),
        [0.2, 0.6, 0.5]
    );
    for width in [3000.0, 700.0, 1200.0, 500.0, 3000.0] {
        let size = Size::new(width, 300.0);
        tree.frame(size, &METRICS);
        assert_eq!(tree.frame(size, &METRICS), None, "{width}");
    }
    assert_eq!(values(&tree), ["20%", "60%", "50%"]);
}

#[test]
fn a_drag_to_a_position_that_is_not_finite_moves_nothing() {
    let (mut tree, split, divider) = split(Axis::Horizontal, 0.25, 0.0);
    let at = centre(tree.bounds(divider).expect("bounds"));
    tree.handle(&press(MouseButton::Left, at));
    assert!(tree.frame(WINDOW, &METRICS).is_some());
    for x in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let response = tree.handle(&hover(Point::new(x, at.y)));
        assert!(kinds(&response, divider).is_empty(), "{x}");
        let response = tree.handle(&hover(Point::new(at.x, x)));
        assert!(kinds(&response, divider).is_empty(), "{x}");
    }
    assert_eq!(ratio(&tree, divider), 0.25);
    assert_eq!(tree.frame(WINDOW, &METRICS), None);
    assert_eq!(
        tree.style(split).map(|style| style.layout),
        Some(tidepool_ui::layout::Layout::Split {
            axis: Axis::Horizontal,
            ratio: 0.25
        })
    );
    // A finite move after them drags it as ever.
    let response = tree.handle(&hover(Point::new(at.x + 50.0, at.y)));
    assert_eq!(kinds(&response, divider), [ActionKind::Change]);
    tree.handle(&release(Point::new(at.x + 50.0, at.y)));
    tree.frame(WINDOW, &METRICS);
    assert_eq!(tree.frame(WINDOW, &METRICS), None);
    assert_eq!(shown(&tree, divider).as_deref(), Some("38%"));
}

#[test]
fn a_split_with_no_length_moves_with_no_key() {
    // Before its first layout, and in a window with no width.
    for laid_out in [false, true] {
        let mut tree = Tree::new();
        let root = tree.root();
        let split = Splitter::new(Axis::Horizontal, "Side panel", 0.25)
            .insert(
                &mut tree,
                root,
                Node::new(Style::stack().fill()),
                Node::new(Style::stack().fill()),
            )
            .expect("a split");
        let divider = Splitter::divider(&tree, split).expect("a divider");
        if laid_out {
            tree.frame(Size::new(0.0, 300.0), &METRICS);
        }
        tree.set_focus(divider, true).expect("focus");
        for pressed in [Key::Right, Key::Left, Key::Home, Key::End] {
            let response = tree.handle(&key(pressed));
            assert!(response.handled, "{pressed:?}");
            assert!(kinds(&response, divider).is_empty(), "{pressed:?}");
        }
        assert_eq!(ratio(&tree, divider), 0.25);
    }
}

#[test]
fn a_splitter_s_name_is_capped_as_a_tab_label_is() {
    let long = "e\u{301}".repeat(10_000);
    let semantics = Splitter::new(Axis::Vertical, long.as_str(), 0.5).semantics();
    assert!(!semantics.name.is_empty() && semantics.name.len() <= MAX_TAB_LABEL_BYTES);
    assert!(
        long.starts_with(&semantics.name) && grapheme::is_boundary(&long, semantics.name.len())
    );
    let shown = format!("{:?}", Splitter::new(Axis::Vertical, long.as_str(), 0.5));
    // Its `Debug` escapes each mark, as `\u{301}`, but holds no more of
    // the name than that.
    assert!(
        shown.len() < 10 * MAX_TAB_LABEL_BYTES + 200,
        "{}",
        shown.len()
    );
    assert!(long.len() > 100 * MAX_TAB_LABEL_BYTES);
}

/// Whether `commands` paint the divider's `teal` bar, its pressed look.
fn dragged_look(commands: &[Command], divider: Rect) -> bool {
    has_rect(
        commands,
        Rect::new(divider.x + 3.0, divider.y, 2.0, divider.height),
        Token::Teal,
    )
}

#[test]
fn a_drag_cut_short_shows_the_divider_at_rest() {
    // The window loses focus mid-drag.
    let (mut tree, _, divider) = split(Axis::Horizontal, 0.25, 0.0);
    let bounds = tree.bounds(divider).expect("bounds");
    tree.handle(&press(MouseButton::Left, centre(bounds)));
    assert!(dragged_look(&painted(&mut tree, WINDOW), bounds));
    tree.handle(&InputEvent::FocusLost);
    tree.handle(&InputEvent::FocusGained);
    tree.handle(&hover(Point::new(300.0, 150.0)));
    assert!(!dragged_look(&painted(&mut tree, WINDOW), bounds));
    // The pointer moves nothing then, and over the divider shows hover.
    let response = tree.handle(&hover(Point::new(330.0, 150.0)));
    assert!(response.actions.is_empty());
    tree.handle(&hover(centre(bounds)));
    let commands = painted(&mut tree, WINDOW);
    assert!(!dragged_look(&commands, bounds));
    assert!(colors(&commands).contains(&color(Token::TealGlow)));
    assert_eq!(ratio(&tree, divider), 0.25);
    // The split is hidden mid-drag, and shown again.
    let (mut tree, split, divider) = split(Axis::Horizontal, 0.25, 0.0);
    tree.handle(&press(MouseButton::Left, centre(bounds)));
    let style = *tree.style(split).expect("a style");
    tree.set_style(split, style.hidden()).expect("hidden");
    tree.frame(WINDOW, &METRICS);
    tree.set_style(split, style).expect("shown");
    tree.handle(&hover(Point::new(10.0, 10.0)));
    assert!(!dragged_look(&painted(&mut tree, WINDOW), bounds));
    assert!(
        tree.handle(&hover(Point::new(300.0, 10.0)))
            .actions
            .is_empty()
    );
    assert_eq!(ratio(&tree, divider), 0.25);
    tree.frame(WINDOW, &METRICS);
    assert_eq!(tree.frame(WINDOW, &METRICS), None);
}

/// A box that fills its bounds with `text`'s color: what a pane holds.
struct Fill;

impl Widget for Fill {
    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        list.rect(cx.bounds(), cx.theme().color(Token::Text));
    }
}

/// Whether `a` and `b` overlap.
fn meets(a: Rect, b: Rect) -> bool {
    a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
}

#[test]
fn moving_a_splitter_damages_only_what_moves() {
    // A box in each pane, at each pane's start: the left one stays where
    // it is as the divider moves, and the right one moves with it.
    let mut tree = Tree::new();
    let root = tree.root();
    let split = Splitter::new(Axis::Horizontal, "Side panel", 0.5)
        .insert(
            &mut tree,
            root,
            Node::new(Style::stack().fill()),
            Node::new(Style::stack().fill()),
        )
        .expect("a split");
    let panes = tree.children(split).expect("panes").to_vec();
    let boxed = || Node::new(Style::stack().fixed(50.0, 50.0)).with_widget(Fill);
    let left = tree.insert(panes[0], boxed()).expect("a box");
    let right = tree.insert(panes[2], boxed()).expect("a box");
    let divider = panes[1];
    tree.frame(WINDOW, &METRICS);
    tree.set_focus(divider, true).expect("focus");
    tree.frame(WINDOW, &METRICS);
    let before = [left, right, divider].map(|id| tree.bounds(id).expect("bounds"));
    tree.handle(&key(Key::Right));
    let frame = tree.frame(WINDOW, &METRICS).expect("it moved");
    let after = [left, right, divider].map(|id| tree.bounds(id).expect("bounds"));
    assert_eq!(after[0], before[0]);
    assert!(
        frame.damage.iter().all(|rect| !meets(*rect, after[0])),
        "{:?}",
        frame.damage
    );
    // What moved is damaged where it was and where it is.
    for moved in [before[1], after[1], before[2], after[2]] {
        assert!(
            frame.damage.iter().any(|rect| inside(moved, *rect)),
            "{moved:?} in {:?}",
            frame.damage
        );
    }
    assert_eq!(tree.frame(WINDOW, &METRICS), None);
}

#[test]
fn only_the_divider_between_the_panes_binds_its_split() {
    // A second divider in the split, after the first: it gives the split
    // no ratio of its own, so the layout settles and idles.
    let mut tree = Tree::new();
    let root = tree.root();
    let split = tree
        .insert(root, Node::new(Style::split(Axis::Horizontal, 0.5).fill()))
        .expect("a split");
    tree.insert(split, Node::new(Style::stack().fill()))
        .expect("a pane");
    let one = tree
        .insert(split, Splitter::new(Axis::Horizontal, "One", 0.3).node())
        .expect("a divider");
    let two = tree
        .insert(split, Splitter::new(Axis::Horizontal, "Two", 0.7).node())
        .expect("a divider");
    tree.frame(WINDOW, &METRICS);
    assert_eq!(tree.frame(WINDOW, &METRICS), None);
    assert_eq!(tree.frame(WINDOW, &METRICS), None);
    assert_eq!(
        tree.style(split).map(|style| style.layout),
        Some(tidepool_ui::layout::Layout::Split {
            axis: Axis::Horizontal,
            ratio: 0.3
        })
    );
    assert_eq!(Splitter::divider(&tree, split), Some(one));
    // The one in the middle moves; the other moves nothing.
    tree.set_focus(one, true).expect("focus");
    assert_eq!(
        kinds(&tree.handle(&key(Key::Right)), one),
        [ActionKind::Change]
    );
    tree.set_focus(two, true).expect("focus");
    assert!(kinds(&tree.handle(&key(Key::Right)), two).is_empty());
    tree.frame(WINDOW, &METRICS);
    assert_eq!(tree.frame(WINDOW, &METRICS), None);
}

// ---------- The card ----------

fn card() -> Card {
    Card::new("rust-analyzer")
        .kicker("Toolchain")
        .text("2026-11-17, up to date")
        .tab("Ready")
}

/// A card under the root, 240 wide, holding a button: the card and the
/// button.
fn card_tree() -> (Tree, NodeId, NodeId) {
    let mut tree = Tree::new();
    let node = Node {
        style: Style {
            width: Length::Fixed(240.0),
            ..card().style()
        },
        ..card().node()
    };
    let id = tree.insert(tree.root(), node).expect("a card");
    let button = tree
        .insert(id, Button::new("Update", ButtonKind::Teal).node())
        .expect("a button");
    tree.frame(WINDOW, &METRICS);
    (tree, id, button)
}

#[test]
fn a_card_is_a_group_named_by_its_title_holding_its_buttons_below_its_text() {
    let (mut tree, id, button) = card_tree();
    // Its value is what else it shows, as written: its kicker, its text
    // and its tab.
    assert_eq!(
        tree.accessibility().to_string(),
        "group \"rust-analyzer\" = \"Toolchain\\n2026-11-17, up to date\\nReady\"\n  \
         button \"Update\" focusable\n"
    );
    // It is no tab stop; its button is.
    assert_eq!(tree.tab_order(), [button]);
    let bounds = tree.bounds(id).expect("bounds");
    let below = tree.bounds(button).expect("bounds");
    // Its padding, three lines and three gaps: 12 + 16.5 + 20.8 + 19.5 + 12.
    assert!((below.y - bounds.y - 80.8).abs() < 0.01, "{below:?}");
    assert!(inside(below, bounds));
    // Its kicker and tab are painted uppercase, its title and text as
    // written.
    let commands = painted(&mut tree, WINDOW);
    assert_eq!(
        text_color(&commands, "TOOLCHAIN"),
        Some(color(Token::Text3))
    );
    assert_eq!(
        text_color(&commands, "rust-analyzer"),
        Some(color(Token::Text))
    );
    assert_eq!(
        text_color(&commands, "2026-11-17, up to date"),
        Some(color(Token::Text2))
    );
    assert_eq!(
        text_color(&commands, "READY"),
        Some(color(Token::OnCardTab))
    );
    // Its texts are capped, and so what it exposes; with only a title it
    // has no value.
    let huge = "x".repeat(MAX_CELL_BYTES * 2);
    let long = Card::new(huge.as_str())
        .kicker(huge.as_str())
        .text(huge.as_str())
        .tab(huge.as_str());
    assert_eq!(long.title().len(), MAX_CELL_BYTES);
    let semantics = long.semantics();
    assert_eq!(semantics.name.len(), MAX_CELL_BYTES);
    assert_eq!(
        semantics.value.map(|value| value.len()),
        Some(3 * MAX_CELL_BYTES + 2)
    );
    assert_eq!(Card::new("adb").semantics().value, None);
    assert_eq!(
        Card::new("adb").tab("Update").semantics().value.as_deref(),
        Some("Update")
    );
}

#[test]
fn a_card_paints_its_box_and_tab_at_rest_and_hovered_in_the_theme_s_tokens() {
    use Token::*;
    let (mut tree, id, _) = card_tree();
    let bounds = tree.bounds(id).expect("bounds");
    let commands = painted(&mut tree, WINDOW);
    let card = colors(&commands);
    // Its box, its lines, then its tab: the pill, its square right, its
    // stripe and its label; then the button.
    assert_eq!(
        card[..9].to_vec(),
        [
            Ghost, Line, Text3, Text, Text2, CardTab, CardTab, Teal, OnCardTab
        ]
        .map(color)
        .to_vec()
    );
    assert!(has_rect(&commands, bounds, Ghost));
    // The tab sits on the right edge, 8 below the top.
    let stripe = commands.iter().find_map(|command| match *command {
        Command::Rect { rect, color: c } if c == color(Teal) && rect.width == 4.0 => Some(rect),
        _ => None,
    });
    let stripe = stripe.expect("the stripe");
    assert_eq!(
        (stripe.right(), stripe.y, stripe.height),
        (bounds.right(), bounds.y + 8.0, 18.0)
    );
    // Hovered: a `teal-fill` ring outside, a `teal-line` border, the title
    // `teal-hi`.
    tree.handle(&hover(centre(bounds)));
    let commands = painted(&mut tree, WINDOW);
    assert_eq!(
        colors(&commands)[..4].to_vec(),
        [TealFill, Ghost, TealLine, Text3].map(color).to_vec()
    );
    assert_eq!(text_color(&commands, "rust-analyzer"), Some(color(TealHi)));
    // Disabled: its rest look in a layer.
    tree.set_enabled(id, false).expect("disabled");
    let commands = painted(&mut tree, WINDOW);
    assert_eq!(
        commands.first(),
        Some(&Command::PushLayer {
            opacity: DISABLED_OPACITY
        })
    );
    assert_eq!(text_color(&commands, "rust-analyzer"), Some(color(Text)));
}

// ---------- The code block ----------

fn snippet() -> CodeBlock {
    CodeBlock::new(
        "rust",
        [
            vec![
                CodeSpan::new("fn", Token::Teal),
                CodeSpan::new(" main() {", Token::Text),
            ],
            vec![CodeSpan::new("    // hello", Token::Comment)],
            vec![CodeSpan::new("}", Token::Text)],
        ],
    )
}

/// A code block under the root: the block and its COPY.
fn block_tree(block: CodeBlock) -> (Tree, NodeId, NodeId) {
    let mut tree = Tree::new();
    let root = tree.root();
    let id = block.insert(&mut tree, root).expect("a block");
    let copy = CodeBlock::copy_button(&tree, id).expect("COPY");
    tree.frame(WINDOW, &METRICS);
    (tree, id, copy)
}

#[test]
fn a_code_block_s_copy_activates_and_its_code_is_its_value() {
    let (mut tree, id, copy) = block_tree(snippet());
    assert_eq!(
        tree.accessibility().to_string(),
        "group \"rust\" = \"fn main() {\\n    // hello\\n}\"\n  button \"Copy\" focusable\n"
    );
    assert_eq!(tree.tab_order(), [copy]);
    let at = centre(tree.bounds(copy).expect("bounds"));
    // It activates as the press is released on it.
    tree.handle(&press(MouseButton::Left, at));
    assert_eq!(
        kinds(&tree.handle(&release(at)), copy),
        [ActionKind::Activate]
    );
    tree.set_focus(copy, true).expect("focus");
    assert_eq!(
        kinds(&tree.handle(&key(Key::Enter)), copy),
        [ActionKind::Activate]
    );
    // COPY sits at the right of the bar.
    let bounds = tree.bounds(id).expect("bounds");
    let button = tree.bounds(copy).expect("bounds");
    assert_eq!(button.right(), bounds.right() - 8.0);
    assert_eq!(button.y, bounds.y + 5.0);
    // The frame that shows its focus, then an idle one.
    assert!(tree.frame(WINDOW, &METRICS).is_some());
    assert_eq!(tree.frame(WINDOW, &METRICS), None);
    // A block with no language is named `code`.
    let (tree, _, _) = block_tree(CodeBlock::plain("", "x"));
    assert!(
        tree.accessibility()
            .to_string()
            .starts_with("group \"code\"")
    );
}

#[test]
fn a_code_block_paints_its_spans_in_their_tokens_and_its_copy_in_each_state() {
    use Token::*;
    let (mut tree, id, copy) = block_tree(snippet());
    let bounds = tree.bounds(id).expect("bounds");
    let commands = painted(&mut tree, WINDOW);
    assert!(has_rect(&commands, bounds, EditorBg));
    assert!(has_rect(
        &commands,
        Rect::new(bounds.x, bounds.y, bounds.width, widgets::CODE_BAR_HEIGHT),
        CodeBar
    ));
    assert!(has_rect(
        &commands,
        Rect::new(bounds.x, bounds.y, widgets::CODE_RULE, bounds.height),
        Teal
    ));
    assert!(commands.contains(&Command::Border {
        rect: bounds,
        width: 1.0,
        color: color(Line2),
    }));
    assert_eq!(text_color(&commands, "RUST"), Some(color(Text3)));
    assert_eq!(text_color(&commands, "fn"), Some(color(Teal)));
    assert_eq!(text_color(&commands, " main() {"), Some(color(Text)));
    assert_eq!(text_color(&commands, "    // hello"), Some(color(Comment)));
    // A span starts where the one before it ends, in the mono face: two
    // chars of 0.6 em at 13 units.
    let origin = |text: &str| {
        commands.iter().find_map(|command| match command {
            Command::Text {
                origin, text: t, ..
            } if t == text => Some(*origin),
            _ => None,
        })
    };
    let (first, second) = (
        origin("fn").expect("fn"),
        origin(" main() {").expect("main"),
    );
    assert!((second.x - first.x - 2.0 * 0.6 * 13.0).abs() < 1e-3);
    assert_eq!(text_color(&commands, "COPY"), Some(color(Teal)));
    // COPY hovered, pressed and focused.
    let at = centre(tree.bounds(copy).expect("bounds"));
    let button = tree.bounds(copy).expect("bounds");
    tree.handle(&hover(at));
    let commands = painted(&mut tree, WINDOW);
    assert!(has_rect(&commands, button, TealWash));
    assert_eq!(text_color(&commands, "COPY"), Some(color(TealHi)));
    tree.handle(&press(MouseButton::Left, at));
    let commands = painted(&mut tree, WINDOW);
    assert!(has_rect(&commands, button, TealFill));
    tree.handle(&release(at));
    tree.set_focus(copy, true).expect("focus");
    let commands = painted(&mut tree, WINDOW);
    assert!(commands.iter().any(|command| matches!(command,
        Command::Border { width, color: c, .. } if *width == widgets::FOCUS_WIDTH && *c == color(Teal))));
    // Disabled: the rest look in a layer.
    tree.set_enabled(id, false).expect("disabled");
    let commands = painted(&mut tree, WINDOW);
    assert_eq!(
        commands.first(),
        Some(&Command::PushLayer {
            opacity: DISABLED_OPACITY
        })
    );
    assert_eq!(text_color(&commands, "COPY"), Some(color(Teal)));
}

#[test]
fn a_code_block_keeps_and_paints_a_bounded_part_of_long_code() {
    // Three hundred lines, each 10 KiB long: it keeps and paints at most
    // the caps.
    let line = "x".repeat(10 * 1024);
    let text = vec![line.as_str(); 300].join("\n");
    let (mut tree, _, _) = block_tree(CodeBlock::plain("text", &text));
    let commands = painted(&mut tree, Size::new(400.0, 300.0));
    let texts: Vec<&String> = commands
        .iter()
        .filter_map(|command| match command {
            Command::Text { text, .. } => Some(text),
            _ => None,
        })
        .filter(|text| text.starts_with('x'))
        .collect();
    assert!(texts.len() <= MAX_CODE_LINES, "{}", texts.len());
    assert!(texts.iter().all(|text| text.len() <= MAX_CODE_LINE_BYTES));
    let block = CodeBlock::plain("text", &text);
    assert!(block.is_cut() && block.lines().len() == MAX_CODE_LINES);
    // Lines of many spans, of wide characters, tabs and marks: each line
    // keeps at most the caps, and always its start, cut at a cluster
    // boundary of the whole line.
    let pieces = ["let ", "日本", "\t", "e\u{301}", "x", ";"];
    let lines: Vec<Vec<CodeSpan>> = (0..MAX_CODE_LINES + 20)
        .map(|n| {
            (0..MAX_CODE_SPANS + 8)
                .map(|k| {
                    let piece = pieces[(n + k) % pieces.len()];
                    CodeSpan::new(piece.repeat(1 + (n * k) % 9), Token::Text)
                })
                .collect()
        })
        .collect();
    let block = CodeBlock::new("rust", lines.clone());
    assert!(block.is_cut());
    assert_eq!(block.lines().len(), MAX_CODE_LINES);
    for (kept, given) in block.lines().iter().zip(&lines) {
        let kept: String = kept.iter().map(CodeSpan::text).collect();
        let cleaned = |span: &CodeSpan| span.text().replace('\t', "    ");
        // The whole line, every span given, and what the spans taken hold.
        let whole: String = given.iter().map(cleaned).collect();
        let taken: usize = given
            .iter()
            .take(MAX_CODE_SPANS)
            .map(|span| cleaned(span).len())
            .sum();
        assert!(kept.len() <= MAX_CODE_LINE_BYTES);
        assert!(
            whole.starts_with(&kept),
            "{} of {}",
            kept.len(),
            whole.len()
        );
        assert!(grapheme::is_boundary(&whole, kept.len()));
        // It is cut where the next cluster would not fit in the cap or the
        // spans taken.
        let room = MAX_CODE_LINE_BYTES.min(taken);
        let next = grapheme::next_boundary(&whole, kept.len());
        assert!(
            next.is_none_or(|next| next > room),
            "{} of {room}",
            kept.len()
        );
    }
    // Its language is capped, in its name and what it shows.
    let (tree, block, _) = block_tree(CodeBlock::plain("x".repeat(4 << 20), "x"));
    let name = tree.semantics(block).map(|s| s.name.len());
    assert_eq!(name, Some(MAX_LANGUAGE_BYTES));
    // Given less height than it measures, it paints the lines that fit.
    let mut tree = Tree::new();
    let root = tree.root();
    let id = CodeBlock::plain("text", &"line\n".repeat(50))
        .insert(&mut tree, root)
        .expect("a block");
    tree.set_style(
        id,
        Style {
            height: Length::Fixed(100.0),
            ..*tree.style(id).expect("a style")
        },
    )
    .expect("a style");
    let commands = painted(&mut tree, WINDOW);
    let lines = commands
        .iter()
        .filter(|command| matches!(command, Command::Text { text, .. } if text == "line"))
        .count();
    // 100 less the bar and the padding above: under four lines of 19.5.
    assert_eq!(lines, 4);
}
