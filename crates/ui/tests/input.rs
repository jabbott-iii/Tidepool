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

//! M3.1's input routing and focus, through the public API only: bubbling,
//! the defaults for Tab and the wheel, focus showing, clicks and focus,
//! press capture, hover and press paths, disabled nodes, window focus, hit
//! testing, the order of actions, and focus moves scrolled into view at the
//! next layout, from the offsets any scroll since left, without a pass when
//! they scroll nothing.

use std::cell::Cell;

use tidepool_core::geom::{Point, Rect, Size};
use tidepool_core::input::{InputEvent, Key, Modifiers, MouseButton, ScrollDelta};
use tidepool_ui::a11y::{Role, Semantics};
use tidepool_ui::input::{Action, ActionKind, EventContext, Handled};
use tidepool_ui::layout::{Length, MAX_EXTENT, Style};
use tidepool_ui::metrics::{FixedMetrics, TextMetrics};
use tidepool_ui::{Node, NodeId, NodeState, Tree, Widget};

const METRICS: FixedMetrics = FixedMetrics::DEFAULT;
const WINDOW: Size = Size::new(400.0, 300.0);

/// What a probe saw: the event's kind and the node's state then.
#[derive(Clone, Debug, PartialEq)]
struct Seen {
    kind: &'static str,
    state: NodeState,
}

/// A widget that records what reaches it, emits actions, and handles
/// events if told to.
#[derive(Default)]
struct Probe {
    handles: bool,
    emits: Vec<ActionKind>,
    wants_focus: bool,
    seen: Vec<Seen>,
}

impl Probe {
    fn handling() -> Self {
        Self {
            handles: true,
            ..Self::default()
        }
    }

    fn emitting(kinds: &[ActionKind], handles: bool) -> Self {
        Self {
            handles,
            emits: kinds.to_vec(),
            ..Self::default()
        }
    }
}

impl Widget for Probe {
    fn event(&mut self, cx: &mut EventContext<'_>, event: &InputEvent) -> Handled {
        let kind = match event {
            InputEvent::KeyDown { .. } => "key down",
            InputEvent::KeyUp { .. } => "key up",
            InputEvent::Text(_) => "text",
            InputEvent::MouseMove { .. } => "move",
            InputEvent::MouseDown { .. } => "down",
            InputEvent::MouseUp { .. } => "up",
            InputEvent::Wheel { .. } => "wheel",
            InputEvent::FocusGained | InputEvent::FocusLost => "window focus",
        };
        self.seen.push(Seen {
            kind,
            state: cx.state(),
        });
        for &action in &self.emits {
            cx.emit(action);
        }
        if self.wants_focus {
            cx.request_focus();
        }
        if self.handles {
            Handled::Yes
        } else {
            Handled::No
        }
    }
}

fn key(key: Key, modifiers: Modifiers) -> InputEvent {
    InputEvent::KeyDown {
        key,
        modifiers,
        repeat: false,
    }
}

fn tab() -> InputEvent {
    key(Key::Tab, Modifiers::NONE)
}

fn mouse_move(x: f32, y: f32) -> InputEvent {
    InputEvent::MouseMove {
        position: Point::new(x, y),
        modifiers: Modifiers::NONE,
    }
}

fn down(button: MouseButton, x: f32, y: f32) -> InputEvent {
    InputEvent::MouseDown {
        button,
        position: Point::new(x, y),
        modifiers: Modifiers::NONE,
    }
}

fn up(button: MouseButton, x: f32, y: f32) -> InputEvent {
    InputEvent::MouseUp {
        button,
        position: Point::new(x, y),
        modifiers: Modifiers::NONE,
    }
}

fn wheel(x: f32, y: f32, delta: ScrollDelta) -> InputEvent {
    InputEvent::Wheel {
        delta,
        position: Point::new(x, y),
        modifiers: Modifiers::NONE,
    }
}

fn button(name: &str) -> Node {
    Node::new(Style::default().fixed(100.0, 30.0))
        .with_semantics(Semantics::new(Role::Button, name))
        .focusable()
}

fn seen(tree: &Tree, id: NodeId) -> Vec<&'static str> {
    tree.widget::<Probe>(id)
        .map(|probe| probe.seen.iter().map(|seen| seen.kind).collect())
        .unwrap_or_default()
}

fn states(tree: &Tree, id: NodeId) -> Vec<NodeState> {
    tree.widget::<Probe>(id)
        .map(|probe| probe.seen.iter().map(|seen| seen.state).collect())
        .unwrap_or_default()
}

#[test]
fn events_bubble_until_a_widget_handles_them_and_actions_come_back_in_order() {
    let mut tree = Tree::new();
    let root = tree.root();
    tree.set_style(root, Style::stack()).expect("the root");
    let panel = tree
        .insert(
            root,
            Node::new(Style::stack().fill())
                .with_widget(Probe::emitting(&[ActionKind::Dismiss], false)),
        )
        .expect("insert");
    // The handler handles what its child does not, so the panel above it
    // never sees it.
    let handler = tree
        .insert(
            panel,
            Node::new(Style::stack().fill())
                .with_widget(Probe::emitting(&[ActionKind::Activate], true)),
        )
        .expect("insert");
    let leaf = tree
        .insert(
            handler,
            button("Run").with_widget(Probe::emitting(
                &[ActionKind::Change, ActionKind::Submit],
                false,
            )),
        )
        .expect("insert");
    tree.layout(WINDOW, &METRICS);
    tree.set_focus(leaf, true).expect("focusable");

    let response = tree.handle(&key(Key::Enter, Modifiers::NONE));
    assert!(response.handled);
    assert_eq!(
        response.actions,
        vec![
            Action {
                node: leaf,
                kind: ActionKind::Change
            },
            Action {
                node: leaf,
                kind: ActionKind::Submit
            },
            Action {
                node: handler,
                kind: ActionKind::Activate
            },
        ]
    );
    assert_eq!(seen(&tree, leaf), vec!["key down"]);
    assert_eq!(seen(&tree, handler), vec!["key down"]);

    // A click on the leaf bubbles the same way.
    let response = tree.handle(&down(MouseButton::Left, 10.0, 10.0));
    assert!(response.handled);
    assert_eq!(seen(&tree, leaf), vec!["key down", "down"]);
    assert_eq!(seen(&tree, handler), vec!["key down", "down"]);
    assert!(seen(&tree, panel).is_empty());

    // Without the handler, the event reaches the panel and comes back
    // unhandled, with every action in order.
    if let Some(probe) = tree.widget_mut::<Probe>(handler) {
        probe.handles = false;
    }
    let response = tree.handle(&key(Key::Escape, Modifiers::NONE));
    assert!(!response.handled);
    let kinds: Vec<ActionKind> = response.actions.iter().map(|action| action.kind).collect();
    assert_eq!(
        kinds,
        vec![
            ActionKind::Change,
            ActionKind::Submit,
            ActionKind::Activate,
            ActionKind::Dismiss
        ]
    );
    assert_eq!(seen(&tree, panel), vec!["key down"]);
}

#[test]
fn what_no_widget_handles_comes_back_unhandled() {
    let mut tree = Tree::new();
    let root = tree.root();
    let field = tree
        .insert(root, button("Field").with_widget(Probe::default()))
        .expect("insert");
    tree.layout(WINDOW, &METRICS);
    tree.set_focus(field, false).expect("focusable");
    for event in [
        key(Key::A, Modifiers::CTRL),
        InputEvent::KeyUp {
            key: Key::A,
            modifiers: Modifiers::NONE,
        },
        InputEvent::Text("a".to_string()),
        down(MouseButton::Right, 10.0, 10.0),
        wheel(10.0, 10.0, ScrollDelta::Lines { x: 0.0, y: 1.0 }),
        key(Key::Tab, Modifiers::CTRL),
        InputEvent::FocusGained,
    ] {
        let response = tree.handle(&event);
        assert!(!response.handled, "{event:?}");
        assert!(response.actions.is_empty(), "{event:?}");
    }
    assert_eq!(
        seen(&tree, field),
        vec!["key down", "key up", "text", "down", "wheel", "key down"]
    );
    // Ctrl+Tab moved nothing.
    assert_eq!(tree.focused(), Some(field));
}

#[test]
fn tab_and_shift_tab_traverse_skipping_disabled_and_hidden_and_wrap() {
    let mut tree = Tree::new();
    let root = tree.root();
    let a = tree.insert(root, button("A")).expect("insert");
    let disabled = tree.insert(root, button("B").disabled()).expect("insert");
    let hidden_group = tree
        .insert(root, Node::new(Style::default().hidden()))
        .expect("insert");
    let hidden = tree.insert(hidden_group, button("C")).expect("insert");
    let off = tree.insert(root, Node::default()).expect("insert");
    let inside_disabled = tree.insert(off, button("D")).expect("insert");
    tree.set_enabled(off, false).expect("in the tree");
    let d = tree.insert(root, button("E")).expect("insert");
    tree.layout(WINDOW, &METRICS);
    assert_eq!(tree.tab_order(), vec![a, d]);
    for skipped in [disabled, hidden, inside_disabled] {
        assert!(tree.set_focus(skipped, true).is_err(), "{skipped}");
    }

    let shift_tab = key(Key::Tab, Modifiers::SHIFT);
    assert!(tree.handle(&tab()).handled);
    assert_eq!(tree.focused(), Some(a));
    assert!(tree.handle(&tab()).handled);
    assert_eq!(tree.focused(), Some(d));
    assert!(tree.handle(&tab()).handled);
    assert_eq!(tree.focused(), Some(a), "wraps at the end");
    assert!(tree.handle(&shift_tab).handled);
    assert_eq!(tree.focused(), Some(d), "wraps at the start");
    tree.clear_focus();
    assert!(tree.handle(&shift_tab).handled);
    assert_eq!(tree.focused(), Some(d), "starts from the last");

    // With nothing to focus, Tab is not handled.
    let mut empty = Tree::new();
    assert!(!empty.handle(&tab()).handled);
    assert_eq!(empty.focus_next(), None);
    assert_eq!(empty.focused(), None);
}

#[test]
fn focus_shows_after_the_keyboard_and_not_after_a_click() {
    let mut tree = Tree::new();
    let root = tree.root();
    let a = tree.insert(root, button("A")).expect("insert");
    tree.layout(WINDOW, &METRICS);

    tree.handle(&down(MouseButton::Left, 10.0, 10.0));
    tree.handle(&up(MouseButton::Left, 10.0, 10.0));
    assert_eq!(tree.focused(), Some(a));
    assert!(!tree.focus_visible());
    assert!(!tree.state(a).expect("in the tree").focus_visible);

    // Shortcuts and modifiers alone do not show it.
    tree.handle(&key(Key::S, Modifiers::CTRL));
    tree.handle(&key(Key::LeftShift, Modifiers::SHIFT));
    assert!(!tree.focus_visible());

    tree.handle(&key(Key::Down, Modifiers::NONE));
    assert!(tree.focus_visible());
    let state = tree.state(a).expect("in the tree");
    assert!(state.focused && state.focus_visible && state.focus_within);

    tree.handle(&down(MouseButton::Left, 10.0, 10.0));
    assert!(!tree.focus_visible());
    assert!(tree.handle(&tab()).handled);
    assert!(tree.focus_visible());
}

#[test]
fn a_click_focuses_the_nearest_focusable_ancestor_or_keeps_focus() {
    let mut tree = Tree::new();
    let root = tree.root();
    tree.set_style(root, Style::column()).expect("the root");
    let card = tree
        .insert(
            root,
            Node::new(Style::stack().fixed(200.0, 100.0))
                .with_semantics(Semantics::new(Role::Group, "Toolchain"))
                .focusable(),
        )
        .expect("insert");
    let label = tree
        .insert(card, Node::new(Style::default().fixed(50.0, 20.0)))
        .expect("insert");
    let status = tree
        .insert(root, Node::new(Style::default().fixed(200.0, 20.0)))
        .expect("insert");
    tree.layout(WINDOW, &METRICS);
    assert_eq!(tree.hit_test(Point::new(10.0, 10.0)), Some(label));

    tree.handle(&down(MouseButton::Left, 10.0, 10.0));
    tree.handle(&up(MouseButton::Left, 10.0, 10.0));
    assert_eq!(tree.focused(), Some(card));

    assert_eq!(tree.hit_test(Point::new(10.0, 110.0)), Some(status));
    tree.handle(&down(MouseButton::Left, 10.0, 110.0));
    tree.handle(&up(MouseButton::Left, 10.0, 110.0));
    assert_eq!(tree.focused(), Some(card), "a click on a label keeps focus");

    // Other buttons never move focus.
    tree.clear_focus();
    tree.handle(&down(MouseButton::Right, 10.0, 10.0));
    assert_eq!(tree.focused(), None);
}

#[test]
fn a_press_captures_the_release_and_moves() {
    let mut tree = Tree::new();
    let root = tree.root();
    tree.set_style(root, Style::row()).expect("the root");
    let pressed = tree
        .insert(root, button("Drag").with_widget(Probe::default()))
        .expect("insert");
    let other = tree
        .insert(root, button("Other").with_widget(Probe::default()))
        .expect("insert");
    tree.layout(WINDOW, &METRICS);
    assert_eq!(tree.bounds(other), Some(Rect::new(100.0, 0.0, 100.0, 30.0)));

    tree.handle(&down(MouseButton::Left, 10.0, 10.0));
    assert!(tree.state(pressed).expect("in the tree").pressed);
    assert!(tree.state(root).expect("in the tree").pressed);
    assert!(!tree.state(other).expect("in the tree").pressed);
    // Moves and the release go to the pressed node, though the pointer
    // left it.
    tree.handle(&mouse_move(150.0, 10.0));
    tree.handle(&up(MouseButton::Left, 150.0, 10.0));
    assert_eq!(seen(&tree, pressed), vec!["down", "move", "up"]);
    assert!(seen(&tree, other).is_empty());
    let during = states(&tree, pressed);
    assert!(during.iter().all(|state| state.pressed), "{during:?}");
    assert!(!tree.state(pressed).expect("in the tree").pressed);
    assert!(!tree.state(root).expect("in the tree").pressed);

    // Without a capture, a move goes to the node under the pointer.
    tree.handle(&mouse_move(150.0, 10.0));
    assert_eq!(seen(&tree, other), vec!["move"]);
}

#[test]
fn hover_and_press_set_the_whole_path() {
    let mut tree = Tree::new();
    let root = tree.root();
    tree.set_style(root, Style::row()).expect("the root");
    let panel = tree
        .insert(root, Node::new(Style::stack().fixed(200.0, 200.0)))
        .expect("insert");
    let leaf = tree
        .insert(panel, Node::new(Style::default().fixed(50.0, 50.0)))
        .expect("insert");
    let sibling = tree
        .insert(root, Node::new(Style::default().fixed(50.0, 50.0)))
        .expect("insert");
    tree.layout(WINDOW, &METRICS);

    tree.handle(&mouse_move(10.0, 10.0));
    for id in [root, panel, leaf] {
        assert!(tree.state(id).expect("in the tree").hovered, "{id}");
    }
    assert!(!tree.state(sibling).expect("in the tree").hovered);
    // Moving within the panel but off the leaf unhovers the leaf only.
    tree.handle(&mouse_move(100.0, 100.0));
    assert!(!tree.state(leaf).expect("in the tree").hovered);
    assert!(tree.state(panel).expect("in the tree").hovered);

    tree.handle(&down(MouseButton::Left, 10.0, 10.0));
    for id in [root, panel, leaf] {
        assert!(tree.state(id).expect("in the tree").pressed, "{id}");
    }
    assert!(!tree.state(sibling).expect("in the tree").pressed);
    tree.handle(&up(MouseButton::Left, 10.0, 10.0));
    assert!(!tree.state(leaf).expect("in the tree").pressed);

    // Removing the hovered leaf takes it off the path.
    tree.handle(&mouse_move(10.0, 10.0));
    tree.remove(leaf).expect("remove");
    assert!(tree.state(panel).expect("in the tree").hovered);
}

#[test]
fn disabled_nodes_get_no_events() {
    let mut tree = Tree::new();
    let root = tree.root();
    let panel = tree
        .insert(
            root,
            Node::new(Style::stack().fixed(200.0, 200.0)).with_widget(Probe::default()),
        )
        .expect("insert");
    let off = tree
        .insert(
            panel,
            button("Off").disabled().with_widget(Probe::handling()),
        )
        .expect("insert");
    tree.layout(WINDOW, &METRICS);

    tree.handle(&mouse_move(10.0, 10.0));
    tree.handle(&down(MouseButton::Left, 10.0, 10.0));
    tree.handle(&up(MouseButton::Left, 10.0, 10.0));
    assert!(seen(&tree, off).is_empty());
    assert_eq!(seen(&tree, panel), vec!["move", "down", "up"]);
    // The press went to the nearest enabled node; focus did not move.
    assert_eq!(tree.focused(), None);
    let state = tree.state(off).expect("in the tree");
    assert!(state.disabled && state.hovered && !state.pressed);

    // Disabling the focused node's ancestor clears focus and a press.
    let on = tree.insert(panel, button("On")).expect("insert");
    tree.layout(WINDOW, &METRICS);
    tree.set_focus(on, true).expect("focusable");
    tree.handle(&down(MouseButton::Left, 150.0, 150.0));
    assert!(tree.state(panel).expect("in the tree").pressed);
    tree.set_enabled(panel, false).expect("in the tree");
    assert_eq!(tree.focused(), None);
    assert!(!tree.state(panel).expect("in the tree").pressed);
    assert!(tree.state(on).expect("in the tree").disabled);
}

#[test]
fn the_wheel_scrolls_the_nearest_scrolling_ancestor_and_chains() {
    let mut tree = Tree::new();
    let root = tree.root();
    let outer = tree
        .insert(
            root,
            Node::new(Style::column().fixed(100.0, 100.0).scroll(false, true)),
        )
        .expect("insert");
    let inner = tree
        .insert(
            outer,
            Node::new(
                Style::column()
                    .width(Length::Fill(1.0))
                    .height(Length::Fixed(50.0))
                    .scroll(false, true),
            ),
        )
        .expect("insert");
    let tall = Style::default()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(100.0));
    let content = tree.insert(inner, Node::new(tall)).expect("insert");
    tree.insert(outer, Node::new(tall.height(Length::Fixed(250.0))))
        .expect("insert");
    tree.layout(WINDOW, &METRICS);
    assert_eq!(tree.content_size(inner), Some(Size::new(100.0, 100.0)));
    assert_eq!(tree.content_size(outer), Some(Size::new(100.0, 300.0)));
    assert_eq!(tree.hit_test(Point::new(10.0, 10.0)), Some(content));

    // The inner node takes 50 of the 80 and passes on 30.
    let response = tree.handle(&wheel(10.0, 10.0, ScrollDelta::Logical { x: 0.0, y: 80.0 }));
    assert!(response.handled);
    assert_eq!(tree.scroll_offset(inner), Some(Point::new(0.0, 50.0)));
    assert_eq!(tree.scroll_offset(outer), Some(Point::new(0.0, 30.0)));
    tree.layout(WINDOW, &METRICS);
    assert_eq!(tree.bounds(inner), Some(Rect::new(0.0, -30.0, 100.0, 50.0)));
    assert_eq!(
        tree.bounds(content),
        Some(Rect::new(0.0, -80.0, 100.0, 100.0))
    );

    // A line is SCROLL_LINE units; nothing scrolls horizontally, and
    // nothing past the end.
    let response = tree.handle(&wheel(50.0, 50.0, ScrollDelta::Lines { x: 0.0, y: 1.0 }));
    assert!(response.handled);
    assert_eq!(tree.scroll_offset(outer), Some(Point::new(0.0, 50.0)));
    let sideways = tree.handle(&wheel(50.0, 50.0, ScrollDelta::Lines { x: 3.0, y: 0.0 }));
    assert!(!sideways.handled);
    tree.layout(WINDOW, &METRICS);
    tree.handle(&wheel(50.0, 50.0, ScrollDelta::Logical { x: 0.0, y: 1e9 }));
    tree.layout(WINDOW, &METRICS);
    assert_eq!(tree.scroll_offset(outer), Some(Point::new(0.0, 200.0)));
    let at_end = tree.handle(&wheel(50.0, 50.0, ScrollDelta::Logical { x: 0.0, y: 5.0 }));
    assert!(!at_end.handled);
    let wild = tree.handle(&wheel(
        50.0,
        50.0,
        ScrollDelta::Logical {
            x: f32::NAN,
            y: f32::NEG_INFINITY,
        },
    ));
    assert!(wild.handled);
    tree.layout(WINDOW, &METRICS);
    assert_eq!(tree.scroll_offset(outer), Some(Point::new(0.0, 0.0)));

    // A widget that handles the wheel stops the scrolling.
    tree.insert(
        content,
        Node::new(Style::default().fill()).with_widget(Probe::handling()),
    )
    .expect("insert");
    tree.layout(WINDOW, &METRICS);
    let before = tree.scroll_offset(inner);
    assert!(
        tree.handle(&wheel(
            10.0,
            10.0,
            ScrollDelta::Logical { x: 0.0, y: -40.0 }
        ))
        .handled
    );
    assert_eq!(tree.scroll_offset(inner), before);
}

#[test]
fn the_wheel_starts_from_the_clamped_offset() {
    let mut tree = Tree::new();
    let root = tree.root();
    let outer = tree
        .insert(
            root,
            Node::new(Style::column().fixed(100.0, 100.0).scroll(false, true)),
        )
        .expect("insert");
    let inner = tree
        .insert(
            outer,
            Node::new(
                Style::column()
                    .width(Length::Fill(1.0))
                    .height(Length::Fixed(50.0))
                    .scroll(false, true),
            ),
        )
        .expect("insert");
    let tall = Style::default()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(100.0));
    tree.insert(inner, Node::new(tall)).expect("insert");
    tree.insert(outer, Node::new(tall.height(Length::Fixed(250.0))))
        .expect("insert");
    tree.layout(WINDOW, &METRICS);

    // Kept at the end, as a log is, then scrolled up before a layout: the
    // inner list moves up from its end (50), and the outer panel stays.
    tree.set_scroll_offset(inner, Point::new(0.0, MAX_EXTENT))
        .expect("scroll");
    let response = tree.handle(&wheel(
        10.0,
        10.0,
        ScrollDelta::Logical { x: 0.0, y: -20.0 },
    ));
    assert!(response.handled);
    assert_eq!(tree.scroll_offset(inner), Some(Point::new(0.0, 30.0)));
    assert_eq!(tree.scroll_offset(outer), Some(Point::new(0.0, 0.0)));
    tree.layout(WINDOW, &METRICS);
    assert_eq!(tree.scroll_offset(inner), Some(Point::new(0.0, 30.0)));
    assert_eq!(tree.scroll_offset(outer), Some(Point::new(0.0, 0.0)));

    // Down from past the end: the inner list cannot move, and passes on
    // the wheel's 20, no more.
    tree.set_scroll_offset(inner, Point::new(0.0, MAX_EXTENT))
        .expect("scroll");
    let response = tree.handle(&wheel(10.0, 10.0, ScrollDelta::Logical { x: 0.0, y: 20.0 }));
    assert!(response.handled);
    assert_eq!(tree.scroll_offset(outer), Some(Point::new(0.0, 20.0)));
}

/// A list 100 by 100 of ten focusable items 50 tall, at the top of the
/// window, laid out.
fn list_of_ten() -> (Tree, NodeId, Vec<NodeId>) {
    let mut tree = Tree::new();
    let root = tree.root();
    let list = tree
        .insert(
            root,
            Node::new(Style::column().fixed(100.0, 100.0).scroll(false, true)),
        )
        .expect("insert");
    let items = (0..10)
        .map(|i| {
            let item = Style::default()
                .width(Length::Fill(1.0))
                .height(Length::Fixed(50.0));
            let item = Node::new(item)
                .with_semantics(Semantics::new(Role::ListItem, format!("Item {i}")))
                .focusable();
            tree.insert(list, item).expect("insert")
        })
        .collect();
    tree.layout(WINDOW, &METRICS);
    (tree, list, items)
}

#[test]
fn focus_moves_between_layouts_scroll_the_last_focus_into_view() {
    for by_tab in [false, true] {
        let (mut tree, list, items) = list_of_ten();
        tree.set_focus(items[1], true).expect("focusable");
        for _ in 0..3 {
            if by_tab {
                assert!(tree.handle(&tab()).handled);
            } else {
                assert!(tree.focus_next().is_some());
            }
        }
        assert_eq!(tree.focused(), Some(items[4]));
        tree.layout(WINDOW, &METRICS);
        // The least offset that shows item 4, at 200 to 250 in the list.
        assert_eq!(
            tree.scroll_offset(list),
            Some(Point::new(0.0, 150.0)),
            "by tab: {by_tab}"
        );
        let view = tree.bounds(list).expect("in the tree");
        let focused = tree.bounds(items[4]).expect("in the tree");
        assert_eq!(focused, Rect::new(0.0, 50.0, 100.0, 50.0));
        assert!(view.y <= focused.y && focused.bottom() <= view.bottom());
    }
}

#[test]
fn a_node_added_or_shown_since_the_last_layout_is_scrolled_into_view() {
    // A list at y 100, below a header, scrolled to 300, with focus on its
    // last item.
    let mut tree = Tree::new();
    let root = tree.root();
    tree.insert(root, Node::new(Style::default().fixed(100.0, 100.0)))
        .expect("insert");
    let list = tree
        .insert(
            root,
            Node::new(Style::column().fixed(100.0, 100.0).scroll(false, true)),
        )
        .expect("insert");
    let item = |name: &str| {
        let style = Style::default()
            .width(Length::Fill(1.0))
            .height(Length::Fixed(50.0));
        Node::new(style)
            .with_semantics(Semantics::new(Role::ListItem, name))
            .focusable()
    };
    let mut last = None;
    for i in 0..10 {
        last = Some(
            tree.insert(list, item(&format!("Item {i}")))
                .expect("insert"),
        );
    }
    tree.set_scroll_offset(list, Point::new(0.0, 300.0))
        .expect("scroll");
    tree.layout(WINDOW, &METRICS);
    tree.set_focus(last.expect("ten items"), true)
        .expect("focusable");

    // A new item at the end, focused before any layout.
    let added = tree.insert(list, item("Added")).expect("insert");
    assert_eq!(tree.focus_next(), Some(added));
    tree.layout(WINDOW, &METRICS);
    assert_eq!(tree.bounds(list), Some(Rect::new(0.0, 100.0, 100.0, 100.0)));
    assert_eq!(tree.scroll_offset(list), Some(Point::new(0.0, 450.0)));
    assert_eq!(tree.bounds(added), Some(Rect::new(0.0, 150.0, 100.0, 50.0)));

    // A hidden item shown, and reached with Tab, before any layout.
    let style = Style::default()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(50.0));
    let shown = tree
        .insert(
            list,
            Node::new(style.hidden())
                .with_semantics(Semantics::new(Role::ListItem, "Shown"))
                .focusable(),
        )
        .expect("insert");
    tree.set_scroll_offset(list, Point::ZERO).expect("scroll");
    tree.layout(WINDOW, &METRICS);
    tree.set_style(shown, style).expect("show");
    assert!(tree.handle(&tab()).handled);
    assert_eq!(tree.focused(), Some(shown));
    tree.layout(WINDOW, &METRICS);
    assert_eq!(tree.scroll_offset(list), Some(Point::new(0.0, 500.0)));
    assert_eq!(tree.bounds(shown), Some(Rect::new(0.0, 150.0, 100.0, 50.0)));
}

#[test]
fn a_scroll_after_a_focus_move_does_not_drop_its_reveal() {
    // Tab, then the wheel, before a layout: the reveal moves from the
    // wheel's offset, the least that shows item 1, at 50 to 100.
    let (mut tree, list, items) = list_of_ten();
    tree.set_focus(items[0], true).expect("focusable");
    assert!(tree.handle(&tab()).handled);
    assert_eq!(tree.focused(), Some(items[1]));
    let down = wheel(10.0, 10.0, ScrollDelta::Logical { x: 0.0, y: 200.0 });
    assert!(tree.handle(&down).handled);
    assert_eq!(tree.scroll_offset(list), Some(Point::new(0.0, 200.0)));
    assert!(tree.frame(WINDOW, &METRICS).is_some());
    assert_eq!(tree.scroll_offset(list), Some(Point::new(0.0, 50.0)));
    assert_eq!(
        tree.bounds(items[1]),
        Some(Rect::new(0.0, 0.0, 100.0, 50.0))
    );

    // A focus move, then the owner's offset: the same, from that offset.
    let (mut tree, list, items) = list_of_ten();
    tree.set_focus(items[0], true).expect("focusable");
    assert_eq!(tree.focus_next(), Some(items[1]));
    tree.set_scroll_offset(list, Point::new(0.0, 250.0))
        .expect("scroll");
    tree.layout(WINDOW, &METRICS);
    assert_eq!(tree.scroll_offset(list), Some(Point::new(0.0, 50.0)));
    assert_eq!(
        tree.bounds(items[1]),
        Some(Rect::new(0.0, 0.0, 100.0, 50.0))
    );

    // Focus moves to item 2, out of view, then the owner sets the offset
    // it already has, as one that sets it every frame does: item 2 is
    // revealed.
    let (mut tree, list, items) = list_of_ten();
    tree.set_focus(items[0], true).expect("focusable");
    assert!(tree.handle(&tab()).handled);
    assert!(tree.handle(&tab()).handled);
    assert_eq!(tree.focused(), Some(items[2]));
    tree.set_scroll_offset(list, Point::ZERO).expect("scroll");
    tree.layout(WINDOW, &METRICS);
    assert_eq!(tree.scroll_offset(list), Some(Point::new(0.0, 50.0)));
    assert_eq!(
        tree.bounds(items[2]),
        Some(Rect::new(0.0, 50.0, 100.0, 50.0))
    );

    // Focus moves, the owner's offset, and one more move: the last
    // reveals, from that offset.
    let (mut tree, list, items) = list_of_ten();
    tree.set_focus(items[0], true).expect("focusable");
    assert!(tree.handle(&tab()).handled);
    assert!(tree.handle(&tab()).handled);
    tree.set_scroll_offset(list, Point::new(0.0, 300.0))
        .expect("scroll");
    assert!(tree.handle(&tab()).handled);
    assert_eq!(tree.focused(), Some(items[3]));
    tree.layout(WINDOW, &METRICS);
    // The least move from 300 that shows item 3, at 150 to 200.
    assert_eq!(tree.scroll_offset(list), Some(Point::new(0.0, 150.0)));
    assert_eq!(
        tree.bounds(items[3]),
        Some(Rect::new(0.0, 0.0, 100.0, 50.0))
    );
}

#[test]
fn a_scroll_of_an_outer_panel_keeps_the_reveal_in_an_inner_list() {
    // A panel 100 by 100 that scrolls, holding a spacer 50 tall, a list 100
    // tall of ten focusable items 50 tall that scrolls, and a spacer 200
    // tall. Focus goes to the last item, and the outer panel scrolls by 30
    // (by the wheel over the first spacer, or by its owner) before a
    // layout.
    for by_wheel in [true, false] {
        let mut tree = Tree::new();
        let root = tree.root();
        let outer = tree
            .insert(
                root,
                Node::new(Style::column().fixed(100.0, 100.0).scroll(false, true)),
            )
            .expect("insert");
        let spacer = |height: f32| {
            Node::new(
                Style::default()
                    .width(Length::Fill(1.0))
                    .height(Length::Fixed(height)),
            )
        };
        tree.insert(outer, spacer(50.0)).expect("insert");
        let inner = tree
            .insert(
                outer,
                Node::new(
                    Style::column()
                        .width(Length::Fill(1.0))
                        .height(Length::Fixed(100.0))
                        .scroll(false, true),
                ),
            )
            .expect("insert");
        let items: Vec<NodeId> = (0..10)
            .map(|i| {
                let item = spacer(50.0)
                    .with_semantics(Semantics::new(Role::ListItem, format!("Item {i}")))
                    .focusable();
                tree.insert(inner, item).expect("insert")
            })
            .collect();
        tree.insert(outer, spacer(200.0)).expect("insert");
        tree.layout(WINDOW, &METRICS);

        assert_eq!(tree.focus_previous(), Some(items[9]));
        if by_wheel {
            let down = wheel(10.0, 10.0, ScrollDelta::Logical { x: 0.0, y: 30.0 });
            assert!(tree.handle(&down).handled);
        } else {
            tree.set_scroll_offset(outer, Point::new(0.0, 30.0))
                .expect("scroll");
        }
        assert_eq!(tree.scroll_offset(outer), Some(Point::new(0.0, 30.0)));
        tree.layout(WINDOW, &METRICS);
        // The inner list moves the least that shows item 9 (from 470 to
        // 520 against its 20 to 120), and then the outer panel the least
        // that shows the item where that put it (70 to 120).
        let context = format!("by wheel: {by_wheel}");
        assert_eq!(
            tree.scroll_offset(inner),
            Some(Point::new(0.0, 400.0)),
            "{context}"
        );
        assert_eq!(
            tree.scroll_offset(outer),
            Some(Point::new(0.0, 50.0)),
            "{context}"
        );
        assert_eq!(
            tree.bounds(inner),
            Some(Rect::new(0.0, 0.0, 100.0, 100.0)),
            "{context}"
        );
        assert_eq!(
            tree.bounds(items[9]),
            Some(Rect::new(0.0, 50.0, 100.0, 50.0)),
            "{context}"
        );
    }
}

/// Two lists 100 by 100 side by side, laid out: the first of ten focusable
/// items 50 tall, the second holding one child 300 tall.
fn two_lists() -> (Tree, NodeId, Vec<NodeId>, NodeId) {
    let mut tree = Tree::new();
    let root = tree.root();
    tree.set_style(root, Style::row()).expect("the root");
    let panel = Style::column().fixed(100.0, 100.0).scroll(false, true);
    let first = tree.insert(root, Node::new(panel)).expect("insert");
    let items = (0..10)
        .map(|i| {
            let item = Style::default()
                .width(Length::Fill(1.0))
                .height(Length::Fixed(50.0));
            let item = Node::new(item)
                .with_semantics(Semantics::new(Role::ListItem, format!("Item {i}")))
                .focusable();
            tree.insert(first, item).expect("insert")
        })
        .collect();
    let second = tree.insert(root, Node::new(panel)).expect("insert");
    let tall = Style::default()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(300.0));
    tree.insert(second, Node::new(tall)).expect("insert");
    tree.layout(WINDOW, &METRICS);
    (tree, first, items, second)
}

#[test]
fn a_scroll_elsewhere_or_one_that_moves_nothing_keeps_the_reveal() {
    // The wheel over the other list scrolls it, and the focus move still
    // reveals its item.
    let (mut tree, first, items, second) = two_lists();
    tree.set_focus(items[0], true).expect("focusable");
    for _ in 0..3 {
        assert!(tree.handle(&tab()).handled);
    }
    let down = wheel(150.0, 10.0, ScrollDelta::Logical { x: 0.0, y: 50.0 });
    assert!(tree.handle(&down).handled);
    tree.layout(WINDOW, &METRICS);
    assert_eq!(tree.scroll_offset(first), Some(Point::new(0.0, 100.0)));
    assert_eq!(tree.scroll_offset(second), Some(Point::new(0.0, 50.0)));

    // A wheel over the list that cannot move it, and an offset set on an
    // ancestor that does not scroll, drop nothing.
    let (mut tree, first, items, _) = two_lists();
    tree.set_focus(items[0], true).expect("focusable");
    for _ in 0..3 {
        assert!(tree.handle(&tab()).handled);
    }
    let up = wheel(10.0, 10.0, ScrollDelta::Logical { x: 0.0, y: -20.0 });
    assert!(!tree.handle(&up).handled);
    let root = tree.root();
    tree.set_scroll_offset(root, Point::new(0.0, 50.0))
        .expect("scroll");
    tree.layout(WINDOW, &METRICS);
    assert_eq!(tree.scroll_offset(first), Some(Point::new(0.0, 100.0)));
    assert_eq!(
        tree.bounds(items[3]),
        Some(Rect::new(0.0, 50.0, 100.0, 50.0))
    );
}

/// Counts the times layout measured it.
#[derive(Default)]
struct Counter(Cell<usize>);

impl Widget for Counter {
    fn measure(&self, _available: Size, _metrics: &dyn TextMetrics) -> Size {
        self.0.set(self.0.get() + 1);
        Size::new(10.0, 10.0)
    }
}

#[test]
fn a_reveal_that_scrolls_nothing_lays_nothing_out_again() {
    let (mut tree, list, items) = list_of_ten();
    let root = tree.root();
    let counter = tree
        .insert(
            root,
            Node::new(Style::default()).with_widget(Counter::default()),
        )
        .expect("insert");
    tree.layout(WINDOW, &METRICS);
    let measured = |tree: &Tree| tree.widget::<Counter>(counter).map_or(0, |c| c.0.get());
    // What one pass measures.
    let start = measured(&tree);
    tree.invalidate_all();
    assert!(tree.frame(WINDOW, &METRICS).is_some());
    let pass = measured(&tree) - start;
    assert!(pass > 0);

    // Tab to an item in view: no pass. (The items paint nothing, so the
    // focus moves damage nothing.)
    tree.set_focus(items[0], true).expect("focusable");
    assert_eq!(tree.frame(WINDOW, &METRICS), None);
    let before = measured(&tree);
    assert!(tree.handle(&tab()).handled);
    assert_eq!(tree.frame(WINDOW, &METRICS), None);
    assert_eq!(measured(&tree), before);
    assert_eq!(tree.scroll_offset(list), Some(Point::ZERO));

    // Tab to one out of view: one pass, after a reveal from the last
    // one's bounds.
    assert!(tree.handle(&tab()).handled);
    assert_eq!(tree.focused(), Some(items[2]));
    tree.layout(WINDOW, &METRICS);
    assert_eq!(measured(&tree), before + pass);
    assert_eq!(tree.scroll_offset(list), Some(Point::new(0.0, 50.0)));
    assert_eq!(
        tree.bounds(items[2]),
        Some(Rect::new(0.0, 50.0, 100.0, 50.0))
    );
    tree.layout(WINDOW, &METRICS);
    assert_eq!(measured(&tree), before + pass);
}

#[test]
fn losing_window_focus_releases_a_capture() {
    let mut tree = Tree::new();
    let root = tree.root();
    let target = tree.insert(root, button("Hold")).expect("insert");
    tree.layout(WINDOW, &METRICS);
    assert!(tree.window_focused());
    tree.handle(&down(MouseButton::Left, 10.0, 10.0));
    assert!(tree.state(target).expect("in the tree").pressed);
    let response = tree.handle(&InputEvent::FocusLost);
    assert!(!response.handled);
    assert!(!tree.window_focused());
    assert!(!tree.state(target).expect("in the tree").pressed);
    tree.handle(&InputEvent::FocusGained);
    assert!(tree.window_focused());
}

#[test]
fn hit_testing_respects_paint_order_and_clips() {
    let mut tree = Tree::new();
    let root = tree.root();
    assert_eq!(tree.hit_test(Point::new(1.0, 1.0)), None, "before a layout");
    tree.set_style(root, Style::stack()).expect("the root");
    let below = tree
        .insert(root, Node::new(Style::default().fixed(100.0, 100.0)))
        .expect("insert");
    let above = tree
        .insert(root, Node::new(Style::default().fixed(50.0, 50.0)))
        .expect("insert");
    let hidden = tree
        .insert(root, Node::new(Style::default().fixed(80.0, 80.0).hidden()))
        .expect("insert");
    tree.layout(WINDOW, &METRICS);
    assert_eq!(tree.hit_test(Point::new(10.0, 10.0)), Some(above));
    assert_eq!(tree.hit_test(Point::new(70.0, 70.0)), Some(below));
    assert_eq!(tree.hit_test(Point::new(300.0, 200.0)), Some(root));
    assert_eq!(tree.hit_test(Point::new(-1.0, 10.0)), None);
    assert_eq!(tree.hit_test(Point::new(400.0, 10.0)), None);
    assert_eq!(tree.hit_test(Point::new(f32::NAN, 10.0)), None);
    assert_ne!(tree.hit_test(Point::new(60.0, 60.0)), Some(hidden));

    // A child scrolled out of its clipping parent is not hit outside it.
    let mut tree = Tree::new();
    let root = tree.root();
    let scroller = tree
        .insert(
            root,
            Node::new(Style::column().fixed(100.0, 100.0).scroll(false, true)),
        )
        .expect("insert");
    let rows: Vec<NodeId> = (0..3)
        .map(|_| {
            let row = Style::default()
                .width(Length::Fill(1.0))
                .height(Length::Fixed(80.0));
            tree.insert(scroller, Node::new(row)).expect("insert")
        })
        .collect();
    let after = tree
        .insert(root, Node::new(Style::default().fixed(100.0, 100.0)))
        .expect("insert");
    tree.layout(WINDOW, &METRICS);
    assert_eq!(
        tree.bounds(rows[1]),
        Some(Rect::new(0.0, 80.0, 100.0, 80.0))
    );
    assert_eq!(tree.hit_test(Point::new(10.0, 90.0)), Some(rows[1]));
    assert_eq!(tree.hit_test(Point::new(10.0, 120.0)), Some(after));
}

#[test]
fn a_handler_may_ask_for_focus() {
    let mut tree = Tree::new();
    let root = tree.root();
    let field = tree
        .insert(
            root,
            button("Field").with_widget(Probe {
                wants_focus: true,
                ..Probe::default()
            }),
        )
        .expect("insert");
    let plain = tree
        .insert(
            root,
            Node::new(Style::default().fixed(100.0, 30.0)).with_widget(Probe {
                wants_focus: true,
                ..Probe::default()
            }),
        )
        .expect("insert");
    tree.layout(WINDOW, &METRICS);
    // A right click reaches the field, which asks for focus: it gets it,
    // not showing.
    tree.handle(&down(MouseButton::Right, 10.0, 10.0));
    assert_eq!(tree.focused(), Some(field));
    assert!(!tree.focus_visible());
    // A node that cannot take focus does not get it.
    tree.handle(&down(MouseButton::Right, 10.0, 40.0));
    assert_eq!(tree.bounds(plain), Some(Rect::new(0.0, 30.0, 100.0, 30.0)));
    assert_eq!(tree.focused(), Some(field));
    // From a text event, focus shows.
    tree.clear_focus();
    tree.handle(&InputEvent::Text("x".to_string()));
    assert_eq!(tree.focused(), None, "the text went to the root");
    tree.set_focus(field, false).expect("focusable");
    tree.handle(&InputEvent::Text("x".to_string()));
    assert!(tree.focus_visible());
}
