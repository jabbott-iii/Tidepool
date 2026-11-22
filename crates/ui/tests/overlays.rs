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

//! The overlay layer (M3.3, ADR-0052), through the public API only: a
//! modal overlay makes the rest inert (no hover, presses, events, focus,
//! tab stops or accessibility nodes), keeps Tab within itself and gives
//! focus back when it closes, with its fallbacks when that node is gone,
//! disabled, hidden or inert, and with modals stacked and closed in either
//! order; a hidden modal makes nothing inert; overlays stack in order for
//! hit testing, paint and the accessibility tree; anchors place them in
//! the window or beside a node, flipped and kept inside the window, and an
//! anchor removed leaves them where they were; bad ids are errors; opening,
//! moving and closing damage what they paint and painted; a collection in
//! an overlay keeps its rows; and the modal dialog's role, name, value,
//! keys, layout, caps and looks. A property test runs random sequences of
//! opens, closes, removals, keys, presses, focus changes, hiding and
//! frames, after each of which no inert node is hovered, pressed, focused,
//! tabbed to or exposed, focus went back as the rules say, and every shown
//! overlay lies inside the window.

use tidepool_core::color::Color;
use tidepool_core::geom::{Point, Rect, Size};
use tidepool_core::input::{InputEvent, Key, Modifiers, MouseButton, ScrollDelta};
use tidepool_core::rng::Pcg32;
use tidepool_core::testing::check_sized;
use tidepool_ui::a11y::{Role, Semantics};
use tidepool_ui::draw::{Command, Corners, DrawList, PaintContext};
use tidepool_ui::input::{Action, ActionKind, Handled};
use tidepool_ui::layout::{Align, Style};
use tidepool_ui::metrics::FixedMetrics;
use tidepool_ui::overlay::{Anchor, MAX_OVERLAYS, OverlayKind, Side};
use tidepool_ui::theme::{Theme, Token};
use tidepool_ui::widgets::{
    BRACKET_ARM, BRACKET_THICKNESS, Button, ButtonKind, Collection, Control, DISABLED_OPACITY,
    MAX_CELL_BYTES, MAX_MODAL_LINES, MODAL_MARGIN, MODAL_PADDING, MODAL_WIDTH, Modal, Palette, Row,
    TextInput, Toast, Tooltip,
};
use tidepool_ui::{Node, NodeId, Tree, UiError, Widget};

const METRICS: FixedMetrics = FixedMetrics::DEFAULT;
const WINDOW: Size = Size::new(400.0, 300.0);

/// Counts the events it gets, and the key and text events among them,
/// handling none.
#[derive(Default)]
struct Probe {
    events: usize,
    keys: usize,
}

impl Widget for Probe {
    fn event(
        &mut self,
        _cx: &mut tidepool_ui::input::EventContext<'_>,
        event: &InputEvent,
    ) -> Handled {
        self.events += 1;
        if matches!(
            event,
            InputEvent::KeyDown { .. } | InputEvent::KeyUp { .. } | InputEvent::Text(_)
        ) {
            self.keys += 1;
        }
        Handled::No
    }
}

/// Fills its bounds with a token's color.
struct Fill(Token);

impl Widget for Fill {
    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        list.rect(cx.bounds(), cx.theme().color(self.0));
    }
}

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

fn press(at: Point) -> InputEvent {
    InputEvent::MouseDown {
        button: MouseButton::Left,
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

/// Moves the pointer to `at` from 20 units to its left, past the slop a
/// press waits for after the active modal changed (ADR-0056).
fn arrive(tree: &mut Tree, at: Point) {
    tree.handle(&hover(Point::new(at.x - 20.0, at.y)));
    tree.handle(&hover(at));
}

fn wheel(at: Point, lines: f32) -> InputEvent {
    InputEvent::Wheel {
        delta: ScrollDelta::Lines { x: 0.0, y: lines },
        position: at,
        modifiers: Modifiers::NONE,
    }
}

fn color(token: Token) -> Color {
    Theme::bundled().color(token)
}

fn center(rect: Rect) -> Point {
    Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
}

fn bounds(tree: &Tree, id: NodeId) -> Rect {
    tree.bounds(id).expect("in the tree")
}

/// A focusable button 100 by 30 holding a [`Probe`].
fn probe_button(name: &str) -> Node {
    Node::new(Style::default().fixed(100.0, 30.0))
        .with_semantics(Semantics::new(Role::Button, name))
        .focusable()
        .with_widget(Probe::default())
}

fn events(tree: &Tree, id: NodeId) -> usize {
    tree.widget::<Probe>(id).map_or(0, |probe| probe.events)
}

fn keys(tree: &Tree, id: NodeId) -> usize {
    tree.widget::<Probe>(id).map_or(0, |probe| probe.keys)
}

/// The main layer: buttons A and B at the window's top left, one above
/// the other, and under them a scroller 100 by 60 holding 200 of content.
struct Shell {
    tree: Tree,
    a: NodeId,
    b: NodeId,
    scroller: NodeId,
}

fn shell() -> Shell {
    let mut tree = Tree::new();
    let root = tree.root();
    tree.set_semantics(root, Semantics::new(Role::Window, "Tidepool"))
        .expect("named");
    let a = tree.insert(root, probe_button("A")).expect("A");
    let b = tree.insert(root, probe_button("B")).expect("B");
    let scroller = tree
        .insert(
            root,
            Node::new(Style::column().fixed(100.0, 60.0).scroll(false, true)),
        )
        .expect("a scroller");
    tree.insert(scroller, Node::new(Style::default().fixed(100.0, 200.0)))
        .expect("content");
    tree.layout(WINDOW, &METRICS);
    Shell {
        tree,
        a,
        b,
        scroller,
    }
}

/// An overlay of `kind`, 200 by 100 in the window's middle (100, 100), a
/// dialog holding a [`Probe`] and the buttons X and Y, one above the other.
fn dialog(tree: &mut Tree, kind: OverlayKind) -> (NodeId, NodeId, NodeId) {
    let node = Node::new(Style::column().fixed(200.0, 100.0))
        .with_semantics(Semantics::new(Role::Dialog, "Dialog"))
        .with_widget(Probe::default());
    let anchor = Anchor::window(Align::Center, Align::Center, 0.0);
    let root = tree.open_overlay(node, kind, anchor).expect("opened");
    let x = tree.insert(root, probe_button("X")).expect("X");
    let y = tree.insert(root, probe_button("Y")).expect("Y");
    (root, x, y)
}

// ---------- A modal makes the rest inert ----------

#[test]
fn a_modal_makes_the_rest_inert() {
    let Shell {
        mut tree,
        a,
        b,
        scroller,
    } = shell();
    // B pressed and hovered, and A focused from the keyboard.
    let on_b = center(bounds(&tree, b));
    tree.handle(&hover(on_b));
    tree.handle(&press(on_b));
    tree.set_focus(a, true).expect("focused");
    let state = tree.state(b).expect("B");
    assert!(state.hovered && state.pressed);

    let (modal, x, y) = dialog(&mut tree, OverlayKind::Modal);
    assert_eq!(tree.modal(), Some(modal));
    // At once, before any layout: no focus, press or hover below it.
    assert_eq!(tree.focused(), None);
    let state = tree.state(b).expect("B");
    assert!(!state.hovered && !state.pressed && !state.focused);
    assert!(!tree.state(a).expect("A").focused);
    for (node, inert) in [
        (a, true),
        (b, true),
        (scroller, true),
        (modal, false),
        (x, false),
    ] {
        assert_eq!(tree.inert(node), Some(inert), "{node}");
    }
    assert_eq!(tree.inert(tree.root()), Some(true));
    tree.layout(WINDOW, &METRICS);

    // The pointer over B hits the modal's root; B gets nothing.
    let before = events(&tree, b);
    assert_eq!(tree.hit_test(on_b), Some(modal));
    assert_eq!(tree.hit_test(center(bounds(&tree, x))), Some(x));
    tree.handle(&release(on_b));
    tree.handle(&hover(on_b));
    let response = tree.handle(&press(on_b));
    assert!(!response.handled);
    tree.handle(&release(on_b));
    assert_eq!(events(&tree, b), before);
    assert!(!tree.state(b).expect("B").hovered);
    assert!(tree.state(modal).expect("modal").hovered);
    assert_eq!(tree.focused(), None, "a press below takes no focus");
    // The wheel over the scroller below scrolls nothing.
    let on_scroller = center(bounds(&tree, scroller));
    assert!(!tree.handle(&wheel(on_scroller, 2.0)).handled);
    tree.layout(WINDOW, &METRICS);
    assert_eq!(tree.scroll_offset(scroller), Some(Point::ZERO));

    // Keys with no focus go to the modal's root, not below it.
    let (below, root_keys) = (keys(&tree, a), keys(&tree, modal));
    tree.handle(&key(Key::A));
    assert_eq!((keys(&tree, a), keys(&tree, modal)), (below, root_keys + 1));

    // Nothing below can take focus; the tab order is the modal's.
    assert_eq!(
        tree.set_focus(a, true),
        Err(UiError::NotFocusable { id: a })
    );
    assert_eq!(tree.tab_order(), [x, y]);
    // The accessibility tree holds the modal alone.
    assert_eq!(
        tree.accessibility().to_string(),
        "dialog \"Dialog\" modal\n  button \"X\" focusable\n  button \"Y\" focusable\n"
    );

    // Closed, the rest is live again: the scroller, where the wheel left
    // the pointer, hovers at once, and the wheel scrolls it.
    assert_eq!(tree.close_overlay(modal), Ok(Some(a)));
    assert_eq!(tree.modal(), None);
    assert!(tree.state(scroller).expect("the scroller").hovered);
    assert!(tree.handle(&wheel(on_scroller, 2.0)).handled);
    tree.layout(WINDOW, &METRICS);
    assert_eq!(tree.scroll_offset(scroller), Some(Point::new(0.0, 40.0)));
    assert_eq!(
        tree.accessibility().to_string(),
        "window \"Tidepool\"\n  button \"A\" focusable focused\n  button \"B\" focusable\n"
    );
}

#[test]
fn tab_stays_within_the_modal_and_what_is_above_it() {
    let Shell { mut tree, a, b, .. } = shell();
    // A modeless overlay below the modal, with a button: inert once the
    // modal opens.
    let (lower, lower_x, _) = dialog(&mut tree, OverlayKind::Modeless);
    tree.set_overlay_anchor(lower, Anchor::window(Align::End, Align::End, 0.0))
        .expect("anchored");
    let (modal, x, y) = dialog(&mut tree, OverlayKind::Modal);
    let inside = [x, y];
    let mut seen = Vec::new();
    for _ in 0..7 {
        assert!(tree.handle(&key(Key::Tab)).handled);
        let focus = tree.focused().expect("focus");
        assert!(inside.contains(&focus), "Tab left the modal for {focus}");
        seen.push(focus);
    }
    assert_eq!(seen, [x, y, x, y, x, y, x]);
    let mut back = Vec::new();
    for _ in 0..4 {
        tree.handle(&key_with(Key::Tab, Modifiers::SHIFT));
        back.push(tree.focused().expect("focus"));
    }
    assert_eq!(back, [y, x, y, x]);
    for below in [a, b, lower, lower_x] {
        assert_eq!(tree.inert(below), Some(true));
    }

    // A pass-through overlay above takes no focus; a modeless one above
    // joins the cycle after the modal.
    let tip = tree
        .open_overlay(
            probe_button("Tip"),
            OverlayKind::PassThrough,
            Anchor::node(x, Side::Below, Align::Start, 4.0),
        )
        .expect("a tooltip");
    assert_eq!(
        tree.set_focus(tip, true),
        Err(UiError::NotFocusable { id: tip })
    );
    let toast = tree
        .open_overlay(
            probe_button("Toast"),
            OverlayKind::Modeless,
            Anchor::window(Align::End, Align::Start, 8.0),
        )
        .expect("a toast");
    assert_eq!(tree.tab_order(), [x, y, toast]);
    assert_eq!(tree.inert(toast), Some(false));
    assert_eq!(tree.inert(tip), Some(false));
    // The tooltip and toast above it are in the accessibility tree.
    let shown = tree.accessibility().to_string();
    assert!(shown.starts_with("dialog \"Dialog\" modal\n"), "{shown}");
    // A pass-through overlay's button never takes focus.
    assert!(shown.contains("\nbutton \"Tip\"\n"), "{shown}");
    assert!(shown.ends_with("\nbutton \"Toast\" focusable\n"), "{shown}");
    assert!(!shown.contains("\"A\""), "{shown}");
    let _ = modal;
}

// ---------- Focus goes back ----------

#[test]
fn closing_a_modal_gives_focus_back_as_it_showed() {
    let Shell { mut tree, a, b, .. } = shell();
    // From the keyboard: focus shows when it comes back after Escape.
    tree.handle(&key(Key::Tab));
    assert_eq!(tree.focused(), Some(a));
    let (modal, x, _) = dialog(&mut tree, OverlayKind::Modal);
    tree.layout(WINDOW, &METRICS);
    tree.handle(&key(Key::Tab));
    assert_eq!(tree.focused(), Some(x));
    tree.handle(&key(Key::Escape));
    assert_eq!(tree.close_overlay(modal), Ok(Some(a)));
    assert_eq!(tree.focused(), Some(a));
    assert!(tree.focus_visible());

    // After a click: focus comes back hidden, to B, which had it. The
    // dialog closing made presses wait for the pointer to move (ADR-0056).
    let on_b = center(bounds(&tree, b));
    tree.handle(&hover(Point::new(on_b.x + 10.0, on_b.y)));
    tree.handle(&hover(on_b));
    tree.handle(&press(on_b));
    tree.handle(&release(on_b));
    assert_eq!(tree.focused(), Some(b));
    assert!(!tree.focus_visible());
    let (modal, x, _) = dialog(&mut tree, OverlayKind::Modal);
    tree.layout(WINDOW, &METRICS);
    let on_x = center(bounds(&tree, x));
    tree.handle(&hover(on_x));
    tree.handle(&press(on_x));
    tree.handle(&release(on_x));
    assert_eq!(tree.focused(), Some(x));
    // Removing the overlay's root closes it as `close_overlay` does.
    tree.remove(modal).expect("closed");
    assert_eq!(tree.overlays(), []);
    assert_eq!(tree.focused(), Some(b));
    assert!(!tree.focus_visible());
}

#[test]
fn focus_goes_back_to_no_node_when_its_node_cannot_take_it() {
    // Removed, disabled, hidden or not focusable while the modal is open.
    let cases: [fn(&mut Tree, NodeId); 4] = [
        |tree, a| tree.remove(a).expect("removed"),
        |tree, a| tree.set_enabled(a, false).expect("disabled"),
        |tree, a| {
            tree.set_style(a, Style::default().hidden())
                .expect("hidden")
        },
        |tree, a| tree.set_focusable(a, false).expect("not focusable"),
    ];
    for (case, change) in cases.into_iter().enumerate() {
        let Shell { mut tree, a, .. } = shell();
        tree.set_focus(a, true).expect("focused");
        let (modal, x, _) = dialog(&mut tree, OverlayKind::Modal);
        tree.set_focus(x, true).expect("focused");
        change(&mut tree, a);
        assert_eq!(tree.close_overlay(modal), Ok(None), "case {case}");
        assert_eq!(tree.focused(), None, "case {case}");
        // Keys then go to the main root's path, for the owner.
        assert!(!tree.handle(&key(Key::A)).handled);
    }

    // Nothing had focus when it opened.
    let Shell { mut tree, .. } = shell();
    let (modal, x, _) = dialog(&mut tree, OverlayKind::Modal);
    tree.set_focus(x, true).expect("focused");
    assert_eq!(tree.close_overlay(modal), Ok(None));
    assert_eq!(tree.focused(), None);

    // Focus moved to a live overlay above the modal stays there, and that
    // overlay gives focus back where the modal would have when it closes.
    let Shell { mut tree, a, .. } = shell();
    tree.set_focus(a, true).expect("focused");
    let (modal, _, _) = dialog(&mut tree, OverlayKind::Modal);
    let toast = tree
        .open_overlay(
            probe_button("Toast"),
            OverlayKind::Modeless,
            Anchor::window(Align::End, Align::End, 0.0),
        )
        .expect("a toast");
    tree.set_focus(toast, false).expect("focused");
    assert_eq!(tree.close_overlay(modal), Ok(None));
    assert_eq!(tree.focused(), Some(toast));
    assert_eq!(tree.close_overlay(toast), Ok(Some(a)));
    assert_eq!(tree.focused(), Some(a));

    // Closing a modeless overlay that holds focus, with no modal under it,
    // clears it, and gives none back.
    let toast = tree
        .open_overlay(
            probe_button("Toast"),
            OverlayKind::Modeless,
            Anchor::window(Align::End, Align::End, 0.0),
        )
        .expect("a toast");
    tree.set_focus(toast, false).expect("focused");
    assert_eq!(tree.close_overlay(toast), Ok(None));
    assert_eq!(tree.focused(), None);
}

#[test]
fn stacked_modals_give_focus_back_closed_in_either_order() {
    // The top one first: back to the lower modal's button, then to A.
    let Shell { mut tree, a, .. } = shell();
    tree.set_focus(a, true).expect("focused");
    let (lower, lower_x, _) = dialog(&mut tree, OverlayKind::Modal);
    tree.set_focus(lower_x, true).expect("focused");
    let (upper, upper_x, _) = dialog(&mut tree, OverlayKind::Modal);
    assert_eq!(tree.modal(), Some(upper));
    assert_eq!(tree.inert(lower_x), Some(true));
    assert_eq!(tree.inert(a), Some(true));
    assert_eq!(
        tree.set_focus(lower_x, true),
        Err(UiError::NotFocusable { id: lower_x })
    );
    tree.set_focus(upper_x, true).expect("focused");
    assert_eq!(tree.close_overlay(upper), Ok(Some(lower_x)));
    assert_eq!(tree.modal(), Some(lower));
    assert_eq!(tree.close_overlay(lower), Ok(Some(a)));

    // The lower one first: focus stays in the upper one, which now gives
    // focus back to A, which the lower one remembered.
    let Shell { mut tree, a, .. } = shell();
    tree.set_focus(a, true).expect("focused");
    let (lower, lower_x, _) = dialog(&mut tree, OverlayKind::Modal);
    tree.set_focus(lower_x, true).expect("focused");
    let (upper, upper_x, _) = dialog(&mut tree, OverlayKind::Modal);
    tree.set_focus(upper_x, true).expect("focused");
    assert_eq!(tree.close_overlay(lower), Ok(None));
    assert_eq!(tree.focused(), Some(upper_x));
    assert_eq!(tree.inert(a), Some(true));
    assert_eq!(tree.close_overlay(upper), Ok(Some(a)));
    assert_eq!(tree.focused(), Some(a));
}

#[test]
fn a_hidden_modal_makes_nothing_inert_until_it_shows() {
    let Shell { mut tree, a, b, .. } = shell();
    tree.set_focus(a, true).expect("focused");
    let (modal, x, _) = dialog(&mut tree, OverlayKind::Modal);
    tree.set_focus(x, true).expect("focused");
    let style = *tree.style(modal).expect("a style");
    tree.set_style(modal, style.hidden()).expect("hidden");
    // Hiding cleared focus in it; the rest is live.
    assert_eq!(tree.modal(), None);
    assert_eq!(tree.focused(), None);
    assert_eq!(tree.inert(a), Some(false));
    tree.set_focus(b, true).expect("B takes focus");
    assert_eq!(tree.tab_order(), [a, b]);
    tree.layout(WINDOW, &METRICS);
    assert_eq!(tree.hit_test(Point::new(150.0, 150.0)), Some(tree.root()));
    // Shown again, it clears focus below it again.
    tree.set_style(modal, style).expect("shown");
    assert_eq!(tree.modal(), Some(modal));
    assert_eq!(tree.focused(), None);
    // It still remembers A, which had focus when it opened.
    assert_eq!(tree.close_overlay(modal), Ok(Some(a)));
    // Disabled, a modal still makes the rest inert.
    let (modal, x, _) = dialog(&mut tree, OverlayKind::Modal);
    tree.set_enabled(modal, false).expect("disabled");
    assert_eq!(tree.inert(a), Some(true));
    assert_eq!(tree.tab_order(), []);
    assert_eq!(
        tree.set_focus(x, true),
        Err(UiError::NotFocusable { id: x })
    );
    assert!(!tree.handle(&key(Key::Tab)).handled);
}

// ---------- Order, hit testing, paint and the accessibility tree ----------

#[test]
fn overlays_stack_in_the_order_they_opened() {
    let mut tree = Tree::new();
    let root = tree.root();
    tree.insert(
        root,
        Node::new(Style::default().fixed(400.0, 300.0)).with_widget(Fill(Token::Bg)),
    )
    .expect("a fill");
    let fill = |token| {
        Node::new(Style::default().fixed(100.0, 60.0))
            .with_semantics(Semantics::new(Role::Group, "panel"))
            .with_widget(Fill(token))
    };
    let at = |x, y| Anchor::window(x, y, 0.0);
    let lower = tree
        .open_overlay(
            fill(Token::Slate),
            OverlayKind::Modeless,
            at(Align::Start, Align::Start),
        )
        .expect("lower");
    let upper = tree
        .open_overlay(
            fill(Token::Amber),
            OverlayKind::Modeless,
            at(Align::Start, Align::Start),
        )
        .expect("upper");
    let tip = tree
        .open_overlay(
            fill(Token::Teal),
            OverlayKind::PassThrough,
            at(Align::Start, Align::Start),
        )
        .expect("tip");
    assert_eq!(tree.overlays(), [lower, upper, tip]);
    assert_eq!(tree.overlay_kind(tip), Some(OverlayKind::PassThrough));
    assert_eq!(tree.overlay_of(tip), Some(tip));
    let frame = tree.frame(WINDOW, &METRICS).expect("a frame");
    // Painted in order: the main layer, then each overlay.
    let fills: Vec<Color> = frame
        .list
        .commands()
        .iter()
        .filter_map(|command| match *command {
            Command::Rect { color, .. } => Some(color),
            _ => None,
        })
        .collect();
    assert_eq!(
        fills,
        [Token::Bg, Token::Slate, Token::Amber, Token::Teal].map(color)
    );
    // The pass-through overlay is never hit; the one under it is.
    let inside = Point::new(50.0, 30.0);
    assert_eq!(tree.hit_test(inside), Some(upper));
    tree.close_overlay(upper).expect("closed");
    tree.layout(WINDOW, &METRICS);
    assert_eq!(tree.hit_test(inside), Some(lower));
    assert_eq!(
        tree.hit_test(Point::new(200.0, 200.0)),
        Some(tree.children(root).expect("children")[0])
    );
    // The accessibility tree lists them in order, each at depth 0.
    assert_eq!(
        tree.accessibility().to_string(),
        "group \"panel\"\ngroup \"panel\"\n"
    );
    assert_eq!(tree.accessibility().nodes()[1].id, tip);
    // A hidden overlay is neither hit, painted nor listed.
    tree.set_style(lower, Style::default().fixed(100.0, 60.0).hidden())
        .expect("hidden");
    tree.layout(WINDOW, &METRICS);
    assert_ne!(tree.hit_test(inside), Some(lower));
    assert_eq!(tree.accessibility().nodes().len(), 1);
}

#[test]
fn nodes_in_an_overlay_take_input_as_any_others() {
    let Shell { mut tree, .. } = shell();
    let (menu, x, y) = dialog(&mut tree, OverlayKind::Modeless);
    tree.layout(WINDOW, &METRICS);
    // Modeless: the rest stays live, and the overlay's buttons follow the
    // main layer's in the tab order.
    let order = tree.tab_order();
    assert_eq!(&order[2..], [x, y]);
    let on_y = center(bounds(&tree, y));
    tree.handle(&hover(on_y));
    assert!(tree.state(y).expect("Y").hovered);
    assert!(tree.state(menu).expect("menu").hovered);
    tree.handle(&press(on_y));
    assert_eq!(tree.focused(), Some(y));
    assert!(tree.state(y).expect("Y").pressed);
    tree.handle(&release(on_y));
    assert!(events(&tree, y) >= 3);
    // A path never leaves its layer: the menu's root is the top.
    assert_eq!(tree.parent(menu), None);
    assert_eq!(tree.overlay_of(y), Some(menu));
}

// ---------- Anchors and placement ----------

#[test]
fn window_anchors_place_overlays_within_the_margin() {
    let mut tree = Tree::new();
    let node = || Node::new(Style::default().fixed(100.0, 50.0));
    let open = |tree: &mut Tree, x, y, margin| {
        tree.open_overlay(node(), OverlayKind::Modeless, Anchor::window(x, y, margin))
            .expect("opened")
    };
    let centered = open(&mut tree, Align::Center, Align::Center, 10.0);
    let corner = open(&mut tree, Align::Start, Align::Start, 10.0);
    let far = open(&mut tree, Align::End, Align::End, 10.0);
    let nan = open(&mut tree, Align::End, Align::Start, f32::NAN);
    let huge = open(&mut tree, Align::Start, Align::Start, 1e9);
    let filled = tree
        .open_overlay(
            Node::new(Style::default().fill()),
            OverlayKind::Modeless,
            Anchor::window(Align::Start, Align::Start, 20.0),
        )
        .expect("opened");
    let large = tree
        .open_overlay(
            Node::new(Style::default().fixed(900.0, 900.0)),
            OverlayKind::Modeless,
            Anchor::window(Align::Center, Align::Center, 0.0),
        )
        .expect("opened");
    tree.layout(WINDOW, &METRICS);
    assert_eq!(
        bounds(&tree, centered),
        Rect::new(150.0, 125.0, 100.0, 50.0)
    );
    assert_eq!(bounds(&tree, corner), Rect::new(10.0, 10.0, 100.0, 50.0));
    assert_eq!(bounds(&tree, far), Rect::new(290.0, 240.0, 100.0, 50.0));
    assert_eq!(bounds(&tree, nan), Rect::new(300.0, 0.0, 100.0, 50.0));
    // A margin past half the window leaves it no room.
    assert_eq!(bounds(&tree, huge), Rect::new(200.0, 150.0, 0.0, 0.0));
    // A fill takes the window less its margin; a larger size is cut to it.
    assert_eq!(bounds(&tree, filled), Rect::new(20.0, 20.0, 360.0, 260.0));
    assert_eq!(bounds(&tree, large), Rect::new(0.0, 0.0, 400.0, 300.0));
    // A smaller window moves them.
    tree.layout(Size::new(200.0, 100.0), &METRICS);
    assert_eq!(bounds(&tree, far), Rect::new(90.0, 40.0, 100.0, 50.0));
}

#[test]
fn node_anchors_place_overlays_beside_and_follow_their_node() {
    let Shell {
        mut tree,
        a,
        scroller,
        ..
    } = shell();
    // A button inside the scroller, 20 below its top.
    let content = tree.children(scroller).expect("content")[0];
    let inner = tree
        .insert(content, Node::new(Style::default().fixed(80.0, 20.0)))
        .expect("inner");
    let tip_node = || Node::new(Style::default().fixed(120.0, 40.0));
    let below_a = tree
        .open_overlay(
            tip_node(),
            OverlayKind::PassThrough,
            Anchor::node(a, Side::Below, Align::Start, 4.0),
        )
        .expect("opened");
    let right = tree
        .open_overlay(
            tip_node(),
            OverlayKind::PassThrough,
            Anchor::node(inner, Side::Right, Align::Center, 6.0),
        )
        .expect("opened");
    assert_eq!(
        tree.overlay_anchor(right),
        Some(Anchor::node(inner, Side::Right, Align::Center, 6.0))
    );
    let first = tree.frame(WINDOW, &METRICS).expect("a frame");
    assert!(!first.damage.is_empty());
    assert_eq!(bounds(&tree, below_a), Rect::new(0.0, 34.0, 120.0, 40.0));
    // The inner node is at (0, 60, 80, 20): right of it, centered on it.
    assert_eq!(bounds(&tree, right), Rect::new(86.0, 50.0, 120.0, 40.0));
    assert_eq!(tree.frame(WINDOW, &METRICS), None);

    // It follows its node as the scroller scrolls.
    tree.set_scroll_offset(scroller, Point::new(0.0, 30.0))
        .expect("scrolled");
    tree.layout(WINDOW, &METRICS);
    assert_eq!(bounds(&tree, right), Rect::new(86.0, 20.0, 120.0, 40.0));

    // Near the window's bottom it flips above; at its right edge it is
    // kept inside.
    let low = tree
        .insert(tree.root(), Node::new(Style::default().fixed(60.0, 180.0)))
        .expect("low");
    tree.layout(WINDOW, &METRICS);
    let low_rect = bounds(&tree, low);
    assert_eq!(low_rect, Rect::new(0.0, 120.0, 60.0, 180.0));
    tree.set_overlay_anchor(below_a, Anchor::node(low, Side::Below, Align::End, 4.0))
        .expect("re-anchored");
    tree.layout(WINDOW, &METRICS);
    // No room below (0) and 116 above: it flips; lined up at its right
    // edge it would start at -60, so it is moved to 0.
    assert_eq!(bounds(&tree, below_a), Rect::new(0.0, 76.0, 120.0, 40.0));
}

#[test]
fn an_anchor_removed_leaves_the_overlay_where_it_was() {
    let Shell { mut tree, a, b, .. } = shell();
    let tip = tree
        .open_overlay(
            Node::new(Style::default().fixed(50.0, 20.0)).with_widget(Fill(Token::Teal)),
            OverlayKind::PassThrough,
            Anchor::node(b, Side::Right, Align::Start, 0.0),
        )
        .expect("opened");
    tree.frame(WINDOW, &METRICS).expect("a frame");
    let placed = bounds(&tree, tip);
    assert_eq!(placed, Rect::new(100.0, 30.0, 50.0, 20.0));
    tree.remove(b).expect("removed");
    // B painted nothing, and the tooltip stays: nothing is damaged.
    assert_eq!(tree.frame(WINDOW, &METRICS), None);
    assert_eq!(bounds(&tree, tip), placed);
    tree.invalidate_all();
    tree.frame(WINDOW, &METRICS).expect("a frame");
    assert_eq!(bounds(&tree, tip), placed);
    assert_eq!(tree.overlays(), [tip], "the tree never closes it");
    // It cannot be anchored to the removed node, nor to a node of its own.
    assert_eq!(
        tree.set_overlay_anchor(tip, Anchor::node(b, Side::Below, Align::Start, 0.0)),
        Err(UiError::UnknownNode { id: b })
    );
    let inside = tree
        .insert(tip, Node::default())
        .expect("a node in the tooltip");
    assert_eq!(
        tree.set_overlay_anchor(tip, Anchor::node(inside, Side::Below, Align::Start, 0.0)),
        Err(UiError::BadAnchor { id: inside })
    );
    assert_eq!(
        tree.overlay_anchor(tip),
        Some(Anchor::node(b, Side::Right, Align::Start, 0.0))
    );
    tree.set_overlay_anchor(tip, Anchor::node(a, Side::Right, Align::Start, 0.0))
        .expect("anchored");
    tree.layout(WINDOW, &METRICS);
    assert_eq!(bounds(&tree, tip), Rect::new(100.0, 0.0, 50.0, 20.0));
    // Anchored to a node of an overlay opened after it, it moves after that
    // overlay, which hosts it from then on; that overlay cannot then be
    // anchored to it, as each would be placed from the other. A modal one
    // keeps its place, anchored above it or not, but no ring is let in.
    let above = tree
        .open_overlay(
            Node::default(),
            OverlayKind::Modeless,
            Anchor::window(Align::Start, Align::Start, 0.0),
        )
        .expect("opened");
    assert_eq!(tree.overlays(), [tip, above]);
    tree.set_overlay_anchor(tip, Anchor::node(above, Side::Below, Align::Start, 0.0))
        .expect("hosted by the later overlay");
    assert_eq!(tree.overlays(), [above, tip]);
    assert_eq!(
        tree.set_overlay_anchor(above, Anchor::node(inside, Side::Below, Align::Start, 0.0)),
        Err(UiError::BadAnchor { id: inside })
    );
    let modal = tree
        .open_overlay(
            Node::default(),
            OverlayKind::Modal,
            Anchor::window(Align::Start, Align::Start, 0.0),
        )
        .expect("opened");
    let higher = tree
        .open_overlay(
            Node::default(),
            OverlayKind::Modeless,
            Anchor::window(Align::Start, Align::Start, 0.0),
        )
        .expect("opened");
    tree.set_overlay_anchor(modal, Anchor::node(higher, Side::Below, Align::Start, 0.0))
        .expect("a modal one anchored to a node above it");
    assert_eq!(tree.overlays(), [above, tip, modal, higher], "it stays put");
    assert_eq!(
        tree.set_overlay_anchor(higher, Anchor::node(modal, Side::Below, Align::Start, 0.0)),
        Err(UiError::BadAnchor { id: modal })
    );
    // Laid out after the overlay its anchor is in: one layout places it
    // where a second does.
    tree.layout(WINDOW, &METRICS);
    let placed = bounds(&tree, modal);
    assert_eq!(placed.y, bounds(&tree, higher).bottom());
    tree.invalidate_all();
    tree.layout(WINDOW, &METRICS);
    assert_eq!(bounds(&tree, modal), placed);
}

#[test]
fn an_overlay_is_laid_out_after_its_anchor_wherever_that_moves() {
    // A modal one anchored to a node of a tooltip below it; the tooltip,
    // anchored to a node of a later modal, moves above the first: the first
    // still follows it in the same layout.
    let Shell { mut tree, a, .. } = shell();
    let tip = tree
        .open_overlay(
            Node::new(Style::column().fixed(80.0, 40.0)),
            OverlayKind::PassThrough,
            Anchor::node(a, Side::Below, Align::Start, 0.0),
        )
        .expect("a tooltip");
    let inside = tree
        .insert(tip, Node::new(Style::default().fixed(20.0, 10.0)))
        .expect("a node in it");
    let first = tree
        .open_overlay(
            Node::new(Style::default().fixed(30.0, 30.0)),
            OverlayKind::Modal,
            Anchor::node(inside, Side::Right, Align::Start, 0.0),
        )
        .expect("a modal");
    let second = tree
        .open_overlay(
            Node::new(Style::default().fixed(50.0, 50.0)),
            OverlayKind::Modal,
            Anchor::window(Align::End, Align::End, 0.0),
        )
        .expect("a modal");
    tree.set_overlay_anchor(tip, Anchor::node(second, Side::Above, Align::Start, 0.0))
        .expect("re-anchored");
    assert_eq!(tree.overlays(), [first, second, tip]);
    tree.layout(WINDOW, &METRICS);
    let placed = bounds(&tree, first);
    assert_eq!(placed.x, bounds(&tree, inside).right());
    tree.invalidate_all();
    tree.layout(WINDOW, &METRICS);
    assert_eq!(bounds(&tree, first), placed);
}

// ---------- Bad ids, limits and damage ----------

#[test]
fn bad_ids_and_limits_are_errors() {
    let Shell { mut tree, a, .. } = shell();
    let root = tree.root();
    let count = tree.node_count();
    let gone = tree.insert(root, Node::default()).expect("a node");
    tree.remove(gone).expect("removed");
    // An anchor not in the tree: nothing opens.
    let anchor = Anchor::node(gone, Side::Below, Align::Start, 0.0);
    assert_eq!(
        tree.open_overlay(Node::default(), OverlayKind::Modal, anchor),
        Err(UiError::UnknownNode { id: gone })
    );
    // A focusable root with no name: nothing opens.
    let unnamed = Node::new(Style::default()).focusable();
    assert_eq!(
        tree.open_overlay(
            unnamed,
            OverlayKind::Modeless,
            Anchor::window(Align::Start, Align::Start, 0.0)
        ),
        Err(UiError::Unnamed { role: Role::None })
    );
    assert_eq!((tree.node_count(), tree.overlays().len()), (count, 0));
    assert_eq!(
        tree.close_overlay(root),
        Err(UiError::NotOverlay { id: root })
    );
    assert_eq!(tree.close_overlay(a), Err(UiError::NotOverlay { id: a }));
    assert_eq!(
        tree.close_overlay(gone),
        Err(UiError::UnknownNode { id: gone })
    );
    let window = Anchor::window(Align::Start, Align::Start, 0.0);
    assert_eq!(
        tree.set_overlay_anchor(a, window),
        Err(UiError::NotOverlay { id: a })
    );
    assert_eq!(
        tree.set_overlay_anchor(gone, window),
        Err(UiError::UnknownNode { id: gone })
    );
    assert_eq!(tree.overlay_kind(a), None);
    assert_eq!(tree.overlay_anchor(a), None);
    assert_eq!(tree.remove(root), Err(UiError::RootNode));
    // Closed twice: the second is an unknown node.
    let (modal, _, _) = dialog(&mut tree, OverlayKind::Modal);
    tree.close_overlay(modal).expect("closed");
    assert_eq!(
        tree.close_overlay(modal),
        Err(UiError::UnknownNode { id: modal })
    );
    assert_eq!(tree.remove(modal), Err(UiError::UnknownNode { id: modal }));
    // At most `MAX_OVERLAYS` open.
    for _ in 0..MAX_OVERLAYS {
        tree.open_overlay(Node::default(), OverlayKind::PassThrough, window)
            .expect("within the limit");
    }
    assert_eq!(
        tree.open_overlay(Node::default(), OverlayKind::PassThrough, window),
        Err(UiError::TooManyOverlays { max: MAX_OVERLAYS })
    );
    assert_eq!(tree.overlays().len(), MAX_OVERLAYS);
    // Overlays lay out, paint and snapshot at the limit.
    assert!(tree.frame(WINDOW, &METRICS).is_some());
}

#[test]
fn opening_moving_and_closing_damage_what_they_paint() {
    let mut tree = Tree::new();
    let root = tree.root();
    let a = tree
        .insert(root, Node::new(Style::default().fixed(100.0, 30.0)))
        .expect("A");
    let b = tree
        .insert(root, Node::new(Style::default().fixed(100.0, 30.0)))
        .expect("B");
    assert!(tree.frame(WINDOW, &METRICS).is_some());
    assert_eq!(tree.frame(WINDOW, &METRICS), None);
    let menu = tree
        .open_overlay(
            Node::new(Style::default().fixed(80.0, 40.0)).with_widget(Fill(Token::PanelSolid)),
            OverlayKind::Modeless,
            Anchor::node(a, Side::Right, Align::Start, 0.0),
        )
        .expect("opened");
    let opened = Rect::new(100.0, 0.0, 80.0, 40.0);
    let frame = tree.frame(WINDOW, &METRICS).expect("the opening");
    assert_eq!(frame.damage, [opened]);
    assert_eq!(tree.frame(WINDOW, &METRICS), None, "idle with an overlay");
    tree.set_overlay_anchor(menu, Anchor::node(b, Side::Right, Align::Start, 0.0))
        .expect("moved");
    let moved = Rect::new(100.0, 30.0, 80.0, 40.0);
    let frame = tree.frame(WINDOW, &METRICS).expect("the move");
    assert_eq!(frame.damage, [opened, moved]);
    assert_eq!(tree.frame(WINDOW, &METRICS), None);
    tree.close_overlay(menu).expect("closed");
    let frame = tree.frame(WINDOW, &METRICS).expect("the close");
    assert_eq!(frame.damage, [moved]);
    assert_eq!(tree.frame(WINDOW, &METRICS), None);

    // A modal dialog dims the whole window: it damages all of it, as the
    // first of its rects, which hold the rest.
    let dialog = Modal::new("Quit?").open(&mut tree).expect("opened");
    let window = Rect::new(0.0, 0.0, WINDOW.width, WINDOW.height);
    for _ in 0..2 {
        let damage = tree.frame(WINDOW, &METRICS).expect("a frame").damage;
        assert_eq!(damage.first(), Some(&window), "{damage:?}");
        assert_eq!(tree.frame(WINDOW, &METRICS), None);
        if tree.contains(dialog) {
            tree.close_overlay(dialog).expect("closed");
        }
    }
}

#[test]
fn a_collection_in_an_overlay_keeps_its_rows() {
    let mut tree = Tree::new();
    let rows: Vec<Row> = (0..1000).map(|n| Row::new(format!("row {n}"))).collect();
    let list = Collection::list("Commands", rows).node();
    let palette = tree
        .open_overlay(
            Node::new(Style::column().fixed(200.0, 100.0))
                .with_semantics(Semantics::new(Role::Dialog, "Palette")),
            OverlayKind::Modal,
            Anchor::window(Align::Center, Align::Start, 20.0),
        )
        .expect("opened");
    let list = tree.insert(palette, list).expect("a list");
    tree.frame(WINDOW, &METRICS).expect("a frame");
    assert_eq!(bounds(&tree, palette), Rect::new(100.0, 20.0, 200.0, 100.0));
    // The five rows its 96 units show, and no margin past its own view,
    // as anywhere else (ADR-0052).
    let shown = tree.accessibility();
    let items = shown
        .nodes()
        .iter()
        .filter(|node| node.role == Role::ListItem)
        .count();
    assert_eq!(items, 5);
    tree.handle(&key(Key::Tab));
    assert_eq!(tree.focused(), Some(list));
    tree.handle(&key(Key::End));
    tree.frame(WINDOW, &METRICS).expect("a frame");
    assert!(
        tree.accessibility()
            .nodes()
            .iter()
            .any(|node| node.name == "row 999" && node.selected)
    );
}

// ---------- The modal dialog ----------

/// The commands from the first that paints the whole window: the modal
/// dialog's, with what its heading and buttons paint after them.
fn modal_commands(tree: &mut Tree, size: Size) -> Vec<Command> {
    tree.invalidate_all();
    let frame = tree.frame(size, &METRICS).expect("a frame");
    let window = Rect::new(0.0, 0.0, size.width, size.height);
    let commands = frame.list.commands();
    let start = commands
        .iter()
        .position(|command| matches!(command, Command::Rect { rect, .. } if *rect == window))
        .expect("the dimming over the window");
    commands[start..].to_vec()
}

fn discard_dialog(tree: &mut Tree) -> (NodeId, NodeId, NodeId) {
    let dialog = Modal::new("Discard changes?")
        .text("main.rs has changes that are not saved.\nThey are lost if you go on.")
        .open(tree)
        .expect("opened");
    let actions = Modal::actions(tree, dialog).expect("its buttons' row");
    let discard = tree
        .insert(actions, Button::new("Discard", ButtonKind::Amber).node())
        .expect("a button");
    let cancel = tree
        .insert(actions, Button::new("Cancel", ButtonKind::Ghost).node())
        .expect("a button");
    (dialog, discard, cancel)
}

/// Whether `commands` hold the focus outline around `rect`.
fn outlined(commands: &[Command], rect: Rect) -> bool {
    let outline = Rect::new(
        rect.x - 5.0,
        rect.y - 5.0,
        rect.width + 10.0,
        rect.height + 10.0,
    );
    commands.iter().any(|command| {
        *command
            == Command::Border {
                rect: outline,
                width: 2.0,
                color: color(Token::Teal),
            }
    })
}

#[test]
fn the_modal_dialog_takes_focus_itself_and_escape_dismisses_it() {
    let Shell { mut tree, a, .. } = shell();
    tree.set_focus(a, true).expect("focused");
    let (dialog, discard, cancel) = discard_dialog(&mut tree);
    // Focus lands on the dialog (ADR-0053), which is not a tab stop.
    assert_eq!(tree.focused(), Some(dialog));
    assert_eq!(tree.tab_order(), [discard, cancel]);
    let semantics = tree.semantics(dialog).expect("semantics").clone();
    assert_eq!(
        (semantics.role, semantics.name.as_str()),
        (Role::Dialog, "Discard changes?")
    );
    assert_eq!(
        semantics.value.as_deref(),
        Some("main.rs has changes that are not saved.\nThey are lost if you go on.")
    );
    assert_eq!(tree.overlay_kind(dialog), Some(OverlayKind::Modal));
    assert_eq!(tree.modal(), Some(dialog));
    tree.layout(WINDOW, &METRICS);
    assert_eq!(
        tree.accessibility().to_string(),
        "dialog \"Discard changes?\" = \"main.rs has changes that are not saved.\\nThey are lost if you go on.\" modal focusable focused\n  \
         button \"Discard\" focusable\n  button \"Cancel\" focusable\n"
    );

    // Enter on the dialog accepts nothing; it comes back blocked.
    let response = tree.handle(&key(Key::Enter));
    assert!(response.actions.is_empty() && !response.handled && response.blocked);
    // Escape emits `Dismiss` from the dialog; a repeat is taken and emits
    // nothing; with Ctrl it is not the dialog's.
    let dismiss = [Action {
        node: dialog,
        kind: ActionKind::Dismiss,
    }];
    let response = tree.handle(&key(Key::Escape));
    assert_eq!(response.actions, dismiss);
    assert!(response.handled && !response.blocked);
    let repeat = InputEvent::KeyDown {
        key: Key::Escape,
        modifiers: Modifiers::NONE,
        repeat: true,
    };
    let response = tree.handle(&repeat);
    assert!(response.handled && response.actions.is_empty());
    let response = tree.handle(&key_with(Key::Escape, Modifiers::CTRL));
    assert!(!response.handled && response.actions.is_empty() && response.blocked);
    // Shift+Tab from the dialog goes to its last control, Tab to its first.
    tree.handle(&key_with(Key::Tab, Modifiers::SHIFT));
    assert_eq!(tree.focused(), Some(cancel));
    tree.set_focus(dialog, true)
        .expect("the dialog takes focus");
    tree.handle(&key(Key::Tab));
    assert_eq!(tree.focused(), Some(discard));
    // Escape from a button inside bubbles up to it, as Shift+Escape does.
    assert_eq!(tree.handle(&key(Key::Escape)).actions, dismiss);
    assert_eq!(
        tree.handle(&key_with(Key::Escape, Modifiers::SHIFT))
            .actions,
        dismiss
    );
    // Its buttons activate; Tab cycles among them, never back to the
    // dialog.
    let response = tree.handle(&key(Key::Enter));
    assert_eq!(
        response.actions,
        [Action {
            node: discard,
            kind: ActionKind::Activate
        }]
    );
    tree.handle(&key(Key::Tab));
    assert_eq!(tree.focused(), Some(cancel));
    tree.handle(&key(Key::Tab));
    assert_eq!(tree.focused(), Some(discard));
    // A press on the rest of the window comes back unhandled and blocked,
    // and moves focus to the dialog, the nearest node under it that takes
    // focus.
    let response = tree.handle(&press(Point::new(2.0, 2.0)));
    assert!(!response.handled && response.blocked);
    assert_eq!(tree.focused(), Some(dialog));
    // Closed by its owner, focus goes back to A.
    assert_eq!(tree.close_overlay(dialog), Ok(Some(a)));
    assert_eq!(Modal::actions(&tree, dialog), None);
    assert_eq!(Modal::actions(&tree, a), None);
    assert_eq!(
        Modal::body(&mut tree, a),
        Err(UiError::WrongWidget { id: a })
    );
    assert_eq!(
        Modal::body(&mut tree, dialog),
        Err(UiError::UnknownNode { id: dialog })
    );
}

#[test]
fn the_modal_dialog_shows_its_focus_outline_only_after_a_key() {
    // Opened after a key: focus shows, and the dialog draws the outline.
    let Shell { mut tree, a, .. } = shell();
    tree.handle(&key(Key::Tab));
    assert_eq!(tree.focused(), Some(a));
    let (dialog, discard, _) = discard_dialog(&mut tree);
    assert!(tree.focus_visible());
    let commands = modal_commands(&mut tree, WINDOW);
    let placed = bounds(&tree, dialog);
    assert!(outlined(&commands, placed));
    // Tab moves focus, and the outline, to its first control.
    tree.handle(&key(Key::Tab));
    let commands = modal_commands(&mut tree, WINDOW);
    assert!(!outlined(&commands, placed));
    assert!(outlined(&commands, bounds(&tree, discard)));

    // Opened after a click: focus is on it, and does not show.
    let Shell { mut tree, a, .. } = shell();
    let on_a = center(bounds(&tree, a));
    tree.handle(&press(on_a));
    tree.handle(&release(on_a));
    let (dialog, _, _) = discard_dialog(&mut tree);
    assert_eq!(tree.focused(), Some(dialog));
    assert!(!tree.focus_visible());
    let commands = modal_commands(&mut tree, WINDOW);
    assert!(!outlined(&commands, bounds(&tree, dialog)));
}

#[test]
fn a_modal_dialog_needs_a_title() {
    let Shell { mut tree, a, .. } = shell();
    tree.set_focus(a, true).expect("focused");
    let count = tree.node_count();
    for title in ["", "  ", "\u{202E}\u{0}"] {
        assert_eq!(
            Modal::new(title).open(&mut tree),
            Err(UiError::Unnamed { role: Role::Dialog })
        );
    }
    assert_eq!((tree.node_count(), tree.focused()), (count, Some(a)));
    assert_eq!(tree.overlays(), []);
}

#[test]
fn the_modal_dialog_is_centered_and_cut_to_a_narrow_window() {
    let mut tree = Tree::new();
    let (dialog, discard, cancel) = discard_dialog(&mut tree);
    tree.layout(WINDOW, &METRICS);
    // 14 padding, the title's 19.5, 10, two lines of 19.5, 10, the
    // buttons' 30 and 14 padding.
    let height = 14.0 + 19.5 + 10.0 + 2.0 * 19.5 + 10.0 + 30.0 + 14.0;
    let placed = bounds(&tree, dialog);
    assert_eq!(
        placed,
        Rect::new(
            (WINDOW.width - MODAL_WIDTH) / 2.0,
            (WINDOW.height - height) / 2.0,
            MODAL_WIDTH,
            height
        )
    );
    // The buttons sit at its bottom, lined up at its right, 8 apart.
    let (discard, cancel) = (bounds(&tree, discard), bounds(&tree, cancel));
    let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
    assert!(
        near(cancel.right(), placed.right() - MODAL_PADDING),
        "{cancel:?}"
    );
    assert!(
        near(cancel.bottom(), placed.bottom() - MODAL_PADDING),
        "{cancel:?}"
    );
    assert!(
        near(cancel.x - discard.right(), 8.0),
        "{discard:?} {cancel:?}"
    );
    assert_eq!(discard.y, cancel.y);
    // A window narrower than it and its margins cuts it.
    let narrow = Size::new(300.0, 300.0);
    tree.layout(narrow, &METRICS);
    let placed = bounds(&tree, dialog);
    assert_eq!(placed.x, MODAL_MARGIN);
    assert_eq!(placed.width, narrow.width - 2.0 * MODAL_MARGIN);
}

#[test]
fn the_modal_dialog_holds_its_owners_content_between_its_text_and_buttons() {
    let mut tree = Tree::new();
    let (dialog, discard, _) = discard_dialog(&mut tree);
    let body = Modal::body(&mut tree, dialog).expect("a body");
    assert_eq!(Modal::body(&mut tree, dialog), Ok(body), "made once");
    let field = tree
        .insert(body, TextInput::new("New name").node())
        .expect("a field");
    tree.layout(WINDOW, &METRICS);
    let (field, discard, placed) = (
        bounds(&tree, field),
        bounds(&tree, discard),
        bounds(&tree, dialog),
    );
    // Below the text (14 + 19.5 + 10 + 39 + 10), above the buttons.
    assert_eq!(field.y, placed.y + 92.5);
    assert_eq!(discard.y, field.bottom() + 10.0);
    assert_eq!(placed.height, 92.5 + 30.0 + 10.0 + 30.0 + 14.0);
}

#[test]
fn the_modal_dialog_paints_a_dimming_and_a_bracketed_panel_in_tokens() {
    let mut tree = Tree::new();
    let (dialog, _, _) = discard_dialog(&mut tree);
    let commands = modal_commands(&mut tree, WINDOW);
    tree.layout(WINDOW, &METRICS);
    let placed = bounds(&tree, dialog);
    let window = Rect::new(0.0, 0.0, WINDOW.width, WINDOW.height);
    assert_eq!(
        &commands[..4],
        [
            Command::Rect {
                rect: window,
                color: color(Token::Panel)
            },
            Command::Rect {
                rect: placed,
                color: Theme::bundled().solid(Token::Panel)
            },
            Command::Border {
                rect: placed,
                width: 1.0,
                color: color(Token::Line)
            },
            Command::Brackets {
                rect: placed,
                arm: BRACKET_ARM,
                thickness: BRACKET_THICKNESS,
                corners: Corners::TOP_LEFT_AND_BOTTOM_RIGHT,
                color: color(Token::Teal)
            },
        ]
    );
    // The title's rule, 14 in.
    let rule = Rect::new(placed.x + 14.0, placed.y + 14.0, 3.0, 19.5);
    assert!(commands.contains(&Command::Rect {
        rect: rule,
        color: color(Token::Teal)
    }));
    // The title uppercase in `text`, the lines as written in `text-2`.
    let texts: Vec<(String, Color)> = commands
        .iter()
        .filter_map(|command| match command {
            Command::Text { text, color, .. } => Some((text.clone(), *color)),
            _ => None,
        })
        .collect();
    assert_eq!(
        &texts[..3],
        [
            ("DISCARD CHANGES?".to_string(), color(Token::Text)),
            (
                "main.rs has changes that are not saved.".to_string(),
                color(Token::Text2)
            ),
            (
                "They are lost if you go on.".to_string(),
                color(Token::Text2)
            ),
        ]
    );
    // Every color is a token's, or its solid value.
    let theme = Theme::bundled();
    let tokens: Vec<Color> = Token::ALL
        .iter()
        .flat_map(|&token| [theme.color(token), theme.solid(token)])
        .collect();
    for command in &commands {
        if let Command::Rect { color, .. }
        | Command::Border { color, .. }
        | Command::Brackets { color, .. }
        | Command::Text { color, .. } = command
        {
            assert!(tokens.contains(color), "{command:?}");
        }
    }

    // Disabled: the dimming and the fill as they were, the rest in a layer
    // at 40%.
    tree.set_enabled(dialog, false).expect("disabled");
    let commands = modal_commands(&mut tree, WINDOW);
    assert_eq!(
        &commands[..4],
        [
            Command::Rect {
                rect: window,
                color: color(Token::Panel)
            },
            Command::Rect {
                rect: placed,
                color: Theme::bundled().solid(Token::Panel)
            },
            Command::PushLayer {
                opacity: DISABLED_OPACITY
            },
            Command::Border {
                rect: placed,
                width: 1.0,
                color: color(Token::Line)
            },
        ]
    );
    // Its idle frame draws nothing.
    assert_eq!(tree.frame(WINDOW, &METRICS), None);
}

#[test]
fn a_short_window_takes_the_dialogs_text_first_and_never_hides_a_live_control() {
    let mut tree = Tree::new();
    let dialog = Modal::new("Install rust-analyzer?")
        .text("It downloads 18 MB from github.com.\nIts license is MIT or Apache-2.0.\nIt is checked against a pinned SHA-256.\nTidepool keeps it up to date.")
        .open(&mut tree)
        .expect("opened");
    let row = Modal::actions(&tree, dialog).expect("a row");
    let install = tree
        .insert(row, Button::new("Install", ButtonKind::Teal).node())
        .expect("a button");
    tree.insert(row, Button::new("Cancel", ButtonKind::Ghost).node())
        .expect("a button");
    let tab = key(Key::Tab);

    // 150 high: the dialog is 126, its buttons keep their 30, and its
    // heading keeps 58: the title and one line.
    let short = Size::new(800.0, 150.0);
    let frame = tree.frame(short, &METRICS).expect("a frame");
    let placed = bounds(&tree, dialog);
    assert_eq!(placed.height, 126.0);
    assert_eq!(bounds(&tree, install).height, 30.0);
    let lines: Vec<&Command> = frame
        .list
        .commands()
        .iter()
        .filter(|command| matches!(command, Command::Text { .. }))
        .collect();
    // The title, one line, and the two buttons' labels.
    assert_eq!(lines.len(), 4, "{lines:?}");
    // Nothing it holds paints outside it: its children are clipped to it.
    let after_wash = modal_commands(&mut tree, short);
    assert!(after_wash.contains(&Command::PushClip { rect: placed }));
    tree.handle(&tab);
    assert_eq!(tree.focused(), Some(install));
    let response = tree.handle(&key(Key::Enter));
    assert_eq!(response.actions.len(), 1);

    // 60 high: no room for its buttons, so its row is disabled: nothing in
    // it takes focus or activates, and Escape still dismisses it.
    let tiny = Size::new(800.0, 60.0);
    tree.frame(tiny, &METRICS).expect("a frame");
    assert!(bounds(&tree, install).height < 30.0);
    assert_eq!(tree.state(row).map(|state| state.disabled), Some(true));
    assert_eq!(tree.enabled(row), Some(true), "its owner's flag is its own");
    assert_eq!(
        tree.focused(),
        Some(dialog),
        "focus left the squeezed button for the dialog"
    );
    assert_eq!(tree.tab_order(), []);
    assert_eq!(
        tree.set_focus(install, true),
        Err(UiError::NotFocusable { id: install })
    );
    assert!(tree.handle(&key(Key::Enter)).actions.is_empty());
    tree.set_focus(dialog, true).expect("the dialog itself");
    let response = tree.handle(&key(Key::Escape));
    assert_eq!(response.actions.len(), 1);
    assert_eq!(response.actions[0].kind, ActionKind::Dismiss);

    // Room again: its row is enabled.
    tree.frame(WINDOW, &METRICS).expect("a frame");
    assert_eq!(tree.state(row).map(|state| state.disabled), Some(false));
    tree.handle(&tab);
    assert_eq!(tree.focused(), Some(install));
}

#[test]
fn a_part_its_owner_disabled_stays_disabled_when_the_window_grows_back() {
    // The owner keeps Accept's row disabled until a license is read.
    let mut tree = Tree::new();
    let dialog = Modal::new("Accept the Android SDK license?")
        .text("Read it before accepting.")
        .open(&mut tree)
        .expect("opened");
    let body = Modal::body(&mut tree, dialog).expect("a body");
    tree.insert(body, Node::new(Style::default().fixed(200.0, 80.0)))
        .expect("the license");
    let row = Modal::actions(&tree, dialog).expect("a row");
    let accept = tree
        .insert(row, Button::new("Accept", ButtonKind::Teal).node())
        .expect("a button");
    tree.layout(WINDOW, &METRICS);
    tree.set_enabled(row, false).expect("disabled");
    // Squeezed, then room again: the row stays disabled, as its owner
    // left it, while the body comes back.
    tree.layout(Size::new(400.0, 90.0), &METRICS);
    assert_eq!(tree.state(body).map(|state| state.disabled), Some(true));
    tree.layout(WINDOW, &METRICS);
    assert_eq!(tree.enabled(row), Some(false));
    assert_eq!(tree.state(accept).map(|state| state.disabled), Some(true));
    assert_eq!(tree.state(body).map(|state| state.disabled), Some(false));
    let at = center(bounds(&tree, accept));
    arrive(&mut tree, at);
    tree.handle(&press(at));
    assert!(tree.handle(&release(at)).actions.is_empty());
    // Its owner enables it: it works.
    tree.set_enabled(row, true).expect("enabled");
    tree.handle(&press(at));
    let response = tree.handle(&release(at));
    assert_eq!(response.actions.len(), 1);
    assert_eq!(response.actions[0].node, accept);
}

#[test]
fn a_body_made_while_the_dialog_is_squeezed_is_squeezed_too() {
    // C1: in a window too short for its row, the owner adds a body with
    // Accept: it is disabled at once, and stays so at each layout.
    let short = Size::new(800.0, 70.0);
    let mut tree = Tree::new();
    let dialog = Modal::new("Install the NDK?")
        .text("It is 1.2 GB.")
        .open(&mut tree)
        .expect("opened");
    let row = Modal::actions(&tree, dialog).expect("a row");
    let cancel = tree
        .insert(row, Button::new("Cancel", ButtonKind::Ghost).node())
        .expect("a button");
    tree.frame(short, &METRICS);
    assert_eq!(tree.state(cancel).map(|state| state.disabled), Some(true));
    let body = Modal::body(&mut tree, dialog).expect("a body");
    let accept = tree
        .insert(body, Button::new("Accept", ButtonKind::Teal).node())
        .expect("a button");
    assert_eq!(
        tree.state(accept).map(|state| state.disabled),
        Some(true),
        "at once"
    );
    tree.frame(short, &METRICS);
    assert_eq!(tree.state(accept).map(|state| state.disabled), Some(true));
    assert!(!tree.tab_order().contains(&accept));
    tree.handle(&key(Key::Tab));
    let response = tree.handle(&key(Key::Enter));
    assert!(response.actions.is_empty(), "{response:?}");
    // Room again: both work.
    tree.frame(WINDOW, &METRICS);
    assert_eq!(tree.state(accept).map(|state| state.disabled), Some(false));
    assert!(tree.tab_order().contains(&accept));
}

#[test]
fn a_modals_default_enabled_again_takes_the_focus_no_node_has() {
    // C5: disabling the dialog's root leaves focus on no node; enabling or
    // making it focusable again gives it back.
    let Shell { mut tree, .. } = shell();
    let (dialog, _, _) = discard_dialog(&mut tree);
    assert_eq!(tree.focused(), Some(dialog));
    tree.set_enabled(dialog, false).expect("disabled");
    assert_eq!(tree.focused(), None);
    tree.set_enabled(dialog, true).expect("enabled");
    assert_eq!(tree.focused(), Some(dialog));
    tree.set_focusable(dialog, false).expect("not focusable");
    assert_eq!(tree.focused(), None);
    tree.set_focusable(dialog, true).expect("focusable");
    assert_eq!(tree.focused(), Some(dialog));
    // A palette's field, enabled again.
    let palette = Palette::new("Commands").open(&mut tree).expect("opened");
    let field = Palette::field(&tree, palette).expect("a field");
    tree.set_enabled(field, false).expect("disabled");
    tree.set_enabled(field, true).expect("enabled");
    assert_eq!(tree.focused(), Some(field));
}

/// Metrics whose lines are twice as tall as their styles say.
struct Tall;

impl tidepool_ui::metrics::TextMetrics for Tall {
    fn line(&self, style: &tidepool_ui::metrics::TextStyle) -> tidepool_ui::metrics::LineMetrics {
        let line = METRICS.line(style);
        tidepool_ui::metrics::LineMetrics {
            height: 2.0 * line.height,
            ..line
        }
    }

    fn width(&self, text: &str, style: &tidepool_ui::metrics::TextStyle) -> f32 {
        METRICS.width(text, style)
    }
}

#[test]
fn the_modal_dialog_measures_and_paints_its_text_with_the_same_metrics() {
    // Lines twice as tall: its heading is twice as tall too, so the
    // buttons sit below both lines, and both lines are painted.
    let mut tree = Tree::new();
    let (dialog, discard, _) = discard_dialog(&mut tree);
    let frame = tree.frame(WINDOW, &Tall).expect("a frame");
    let placed = bounds(&tree, dialog);
    let heading = 2.0 * (19.5 + 2.0 * 19.5) + 10.0;
    assert_eq!(bounds(&tree, discard).y, placed.y + 14.0 + heading + 10.0);
    let painted = frame
        .list
        .commands()
        .iter()
        .filter(|command| matches!(command, Command::Text { color: ink, .. } if *ink == color(Token::Text2)))
        .count();
    assert_eq!(painted, 2);
}

#[test]
fn the_modal_dialog_keeps_its_text_bounded_and_clean() {
    let long = "x".repeat(3 * MAX_CELL_BYTES);
    let lines: Vec<String> = (0..10).map(|n| format!("line {n}")).collect();
    let modal = Modal::new(&long).text(lines.join("\n"));
    assert_eq!(modal.title().len(), MAX_CELL_BYTES);
    assert_eq!(modal.lines().len(), MAX_MODAL_LINES);
    assert_eq!(modal.lines()[3], "line 3");
    // A line past the limit is cut, and the last read.
    let modal = Modal::new("t").text(format!("{long}\nhunter2"));
    assert_eq!(modal.lines().len(), 1);
    let full = "x".repeat(MAX_CELL_BYTES);
    let modal = Modal::new("Key").text(format!("{full}\nhunter2"));
    let shown = format!("{modal:?}");
    assert_eq!(
        shown,
        "Modal { title: <3 bytes>, lines: [<4096 bytes>, <7 bytes>] }"
    );
    assert!(!shown.contains("hunter2"));
    // Its name and value are cleaned: no direction overrides or controls.
    let mut tree = Tree::new();
    let dialog = Modal::new("Open \u{202E}gpj.exe\u{202C}?")
        .text("from \u{1b}[31mdownloads\u{0}")
        .open(&mut tree)
        .expect("opened");
    let semantics = tree.semantics(dialog).expect("semantics");
    assert_eq!(semantics.name, "Open gpj.exe?");
    assert_eq!(semantics.value.as_deref(), Some("from [31mdownloads"));
    // A title with no text has no value.
    let quit = Modal::new("Quit").open(&mut tree).expect("opened");
    assert_eq!(tree.semantics(quit).expect("semantics").value, None);
}

#[test]
fn one_held_escape_closes_one_dialog() {
    let size = Size::new(800.0, 600.0);
    let mut tree = Tree::new();
    // A dialog with a search field its owner focused, and a second over
    // it, opened while the field had focus.
    let lower = Modal::new("Go to file").open(&mut tree).expect("opened");
    let body = Modal::body(&mut tree, lower).expect("a body");
    let field = tree
        .insert(body, TextInput::new("Search").node())
        .expect("a field");
    tree.set_focus(field, true).expect("focused");
    let upper = Modal::new("Discard changes?")
        .open(&mut tree)
        .expect("opened");
    tree.frame(size, &METRICS);
    let repeat = InputEvent::KeyDown {
        key: Key::Escape,
        modifiers: Modifiers::NONE,
        repeat: true,
    };
    let mut closed = Vec::new();
    for event in [key(Key::Escape), repeat.clone(), repeat.clone(), repeat] {
        let response = tree.handle(&event);
        for action in response.actions {
            if action.kind == ActionKind::Dismiss {
                let dialog = tree.overlay_of(action.node).expect("in a dialog");
                tree.close_overlay(dialog).expect("closed");
                closed.push(dialog);
            }
        }
        tree.frame(size, &METRICS);
    }
    assert_eq!(closed, [upper]);
    assert_eq!(tree.focused(), Some(field));
    // The field's own Escape still dismisses, once a press.
    let response = tree.handle(&key(Key::Escape));
    assert_eq!(response.actions.len(), 1);
    assert_eq!(response.actions[0].node, field);
}

// ---------- What a modal keeps from the window ----------

#[test]
fn events_a_modal_keeps_from_the_window_come_back_blocked() {
    let ctrl_s = key_with(Key::S, Modifiers::CTRL);
    let text = InputEvent::Text("x".to_string());
    let outside = Point::new(5.0, 290.0);
    // No modal: unhandled, not blocked.
    let Shell { mut tree, .. } = shell();
    for event in [
        ctrl_s.clone(),
        key(Key::F5),
        text.clone(),
        wheel(outside, 1.0),
    ] {
        let response = tree.handle(&event);
        assert!(!response.handled && !response.blocked, "{event:?}");
    }
    // A modal open: each is blocked.
    let (modal, x, _) = dialog(&mut tree, OverlayKind::Modal);
    tree.layout(WINDOW, &METRICS);
    for event in [
        ctrl_s.clone(),
        key(Key::F5),
        text.clone(),
        wheel(outside, 1.0),
        press(outside),
        release(outside),
        hover(outside),
    ] {
        let response = tree.handle(&event);
        assert!(!response.handled && response.blocked, "{event:?}");
    }
    // Inside the dialog too, what nothing takes.
    tree.set_focus(x, true).expect("focused");
    assert!(tree.handle(&ctrl_s).blocked);
    // What is handled is not blocked: Tab moves focus.
    let response = tree.handle(&key(Key::Tab));
    assert!(response.handled && !response.blocked);
    // Window focus events never are.
    for event in [InputEvent::FocusLost, InputEvent::FocusGained] {
        assert!(!tree.handle(&event).blocked);
    }
    // Closed, nothing is blocked again.
    tree.close_overlay(modal).expect("closed");
    assert!(!tree.handle(&ctrl_s).blocked);
}

// ---------- A press waits for the pointer to move (ADR-0055) ----------

/// The kinds of the actions a response holds, by node.
fn acted(response: &tidepool_ui::input::Response) -> Vec<(NodeId, ActionKind)> {
    response
        .actions
        .iter()
        .map(|action| (action.node, action.kind))
        .collect()
}

#[test]
fn a_press_waits_until_the_pointer_moves_past_the_slop() {
    let mut tree = Tree::new();
    let (dialog, discard, _) = discard_dialog(&mut tree);
    tree.layout(WINDOW, &METRICS);
    let at = center(bounds(&tree, discard));
    tree.close_overlay(dialog).expect("closed");
    tree.layout(WINDOW, &METRICS);
    // The pointer rests where the dialog's Discard lands; the dialog opens.
    tree.handle(&hover(at));
    let (dialog, discard, _) = discard_dialog(&mut tree);
    tree.layout(WINDOW, &METRICS);
    assert_eq!(tree.hit_test(at), Some(discard));
    // A press and release with no move activate nothing, come back
    // blocked, and leave focus on the dialog.
    let down = tree.handle(&press(at));
    let up = tree.handle(&release(at));
    assert!(down.actions.is_empty() && up.actions.is_empty());
    assert!(down.blocked && !down.handled, "{down:?}");
    assert!(up.blocked, "{up:?}");
    assert_eq!(tree.focused(), Some(dialog));
    // Within the slop, 4 device pixels, which are 4 units at scale 1, a
    // double click's jitter: still nothing.
    for (dx, dy) in [(1.0, 0.0), (3.0, 2.0), (0.0, -4.0)] {
        let near = Point::new(at.x + dx, at.y + dy);
        tree.handle(&hover(near));
        tree.handle(&press(near));
        assert!(acted(&tree.handle(&release(near))).is_empty(), "{dx}, {dy}");
    }
    // Past it, a click is the button's.
    tree.handle(&hover(Point::new(at.x + 3.0, at.y + 3.0)));
    tree.handle(&press(at));
    let up = tree.handle(&release(at));
    assert_eq!(acted(&up), [(discard, ActionKind::Activate)]);

    // A modal that becomes active again, as the one over it closes, waits
    // for the pointer too.
    let over = Modal::new("Sure?").open(&mut tree).expect("opened");
    tree.layout(WINDOW, &METRICS);
    tree.close_overlay(over).expect("closed");
    tree.layout(WINDOW, &METRICS);
    tree.handle(&press(at));
    assert!(acted(&tree.handle(&release(at))).is_empty());
    // So does one shown again.
    let shown = *tree.style(dialog).expect("a style");
    tree.set_style(dialog, shown.hidden()).expect("hidden");
    arrive(&mut tree, at);
    tree.set_style(dialog, shown).expect("shown");
    tree.layout(WINDOW, &METRICS);
    tree.handle(&press(at));
    assert!(acted(&tree.handle(&release(at))).is_empty());
    // A press that drags away and back is not the button's either.
    tree.handle(&press(at));
    tree.handle(&hover(Point::new(at.x + 9.0, at.y)));
    tree.handle(&hover(at));
    assert!(acted(&tree.handle(&release(at))).is_empty());
    // Other buttons' presses are kept from it as well.
    let middle = InputEvent::MouseDown {
        button: MouseButton::Middle,
        position: at,
        modifiers: Modifiers::NONE,
    };
    let other = Modal::new("Again?").open(&mut tree).expect("opened");
    tree.layout(WINDOW, &METRICS);
    assert!(tree.handle(&middle).blocked);
    tree.close_overlay(other).expect("closed");
}

#[test]
fn a_press_waits_after_the_last_modal_closes_or_is_hidden_too() {
    // B under the dialog's OK: the second click of a double click on OK,
    // which closed the dialog, reaches nothing (ADR-0056).
    let Shell { mut tree, .. } = shell();
    let probe = |tree: &mut Tree| {
        let dialog = Modal::new("Saved.").open(tree).expect("opened");
        let row = Modal::actions(tree, dialog).expect("a row");
        let ok = tree
            .insert(row, Button::new("OK", ButtonKind::Teal).node())
            .expect("a button");
        (dialog, ok)
    };
    let (dialog, ok) = probe(&mut tree);
    tree.layout(WINDOW, &METRICS);
    let at = center(bounds(&tree, ok));
    arrive(&mut tree, at);
    tree.handle(&press(at));
    let up = tree.handle(&release(at));
    assert_eq!(acted(&up), [(ok, ActionKind::Activate)]);
    // Its owner closes it; under the pointer now is the window.
    tree.close_overlay(dialog).expect("closed");
    tree.layout(WINDOW, &METRICS);
    let under = tree.hit_test(at).expect("a node");
    let down = tree.handle(&press(at));
    let up = tree.handle(&release(at));
    assert!(down.blocked && !down.handled, "{down:?}");
    assert!(up.blocked && up.actions.is_empty(), "{up:?}");
    assert_eq!(events(&tree, under), 0, "nothing reached it");
    assert_eq!(tree.state(under).map(|state| state.pressed), Some(false));
    // Moved past the slop, presses reach the window again.
    tree.handle(&hover(Point::new(at.x + 5.0, at.y)));
    let down = tree.handle(&press(at));
    assert!(!down.blocked, "{down:?}");
    tree.handle(&release(at));

    // Hiding the active modal waits so too.
    let (dialog, _) = probe(&mut tree);
    tree.layout(WINDOW, &METRICS);
    arrive(&mut tree, at);
    let style = *tree.style(dialog).expect("a style");
    tree.set_style(dialog, style.hidden()).expect("hidden");
    tree.layout(WINDOW, &METRICS);
    assert_eq!(tree.modal(), None);
    assert!(tree.handle(&press(at)).blocked);
    tree.handle(&release(at));
}

/// `METRICS` for a window at scale 2: two device pixels to a unit.
struct Scale2;

impl tidepool_ui::metrics::TextMetrics for Scale2 {
    fn line(&self, style: &tidepool_ui::metrics::TextStyle) -> tidepool_ui::metrics::LineMetrics {
        METRICS.line(style)
    }

    fn width(&self, text: &str, style: &tidepool_ui::metrics::TextStyle) -> f32 {
        METRICS.width(text, style)
    }

    fn scale(&self) -> f32 {
        2.0
    }
}

#[test]
fn the_slop_is_four_device_pixels_at_the_windows_scale() {
    let mut tree = Tree::new();
    let (_, discard, _) = discard_dialog(&mut tree);
    tree.layout(WINDOW, &Scale2);
    let at = center(bounds(&tree, discard));
    tree.handle(&hover(at));
    // At scale 2, 4 device pixels are 2 units: 1.5 is within, 2.5 past.
    tree.handle(&hover(Point::new(at.x + 1.5, at.y)));
    tree.handle(&press(at));
    assert!(acted(&tree.handle(&release(at))).is_empty());
    tree.handle(&hover(Point::new(at.x + 2.5, at.y)));
    tree.handle(&press(at));
    let up = tree.handle(&release(at));
    assert_eq!(acted(&up), [(discard, ActionKind::Activate)]);
}

#[test]
fn with_the_pointer_not_yet_seen_the_first_event_places_it() {
    // No pointer event before the dialog: the first places the pointer,
    // and the wait goes on from there.
    let mut tree = Tree::new();
    let (_, discard, _) = discard_dialog(&mut tree);
    tree.layout(WINDOW, &METRICS);
    let at = center(bounds(&tree, discard));
    tree.handle(&press(at));
    assert!(acted(&tree.handle(&release(at))).is_empty());
    tree.handle(&press(at));
    assert!(acted(&tree.handle(&release(at))).is_empty(), "it stayed");
    // A move to where the press placed it, or within the slop, is no move.
    tree.handle(&hover(at));
    tree.handle(&hover(Point::new(at.x, at.y + 2.0)));
    tree.handle(&press(at));
    assert!(acted(&tree.handle(&release(at))).is_empty());
    tree.handle(&hover(Point::new(at.x, at.y + 5.0)));
    tree.handle(&press(at));
    let up = tree.handle(&release(at));
    assert_eq!(acted(&up), [(discard, ActionKind::Activate)]);
    // A first move places it too.
    let mut tree = Tree::new();
    let (_, discard, _) = discard_dialog(&mut tree);
    tree.layout(WINDOW, &METRICS);
    tree.handle(&hover(at));
    tree.handle(&press(at));
    assert!(acted(&tree.handle(&release(at))).is_empty());
    arrive(&mut tree, at);
    tree.handle(&press(at));
    let up = tree.handle(&release(at));
    assert_eq!(acted(&up), [(discard, ActionKind::Activate)]);
}

#[test]
fn a_position_that_is_not_finite_places_and_moves_nothing() {
    // The pointer rests on where Allow lands; a move to NaN or infinity
    // neither ends the wait nor is kept as where the pointer is.
    for wild in [
        Point::new(f32::NAN, f32::NAN),
        Point::new(f32::INFINITY, 5.0),
        Point::new(5.0, f32::NEG_INFINITY),
    ] {
        let mut tree = Tree::new();
        let (dialog, discard, _) = discard_dialog(&mut tree);
        tree.layout(WINDOW, &METRICS);
        let at = center(bounds(&tree, discard));
        tree.close_overlay(dialog).expect("closed");
        arrive(&mut tree, at);
        let (_, discard, _) = discard_dialog(&mut tree);
        tree.layout(WINDOW, &METRICS);
        tree.handle(&hover(wild));
        tree.handle(&press(wild));
        tree.handle(&release(wild));
        assert_eq!(tree.state(discard).map(|state| state.hovered), Some(true));
        let down = tree.handle(&press(at));
        let up = tree.handle(&release(at));
        assert!(down.blocked && up.actions.is_empty(), "{wild:?}: {up:?}");
    }
    // Before a dialog opens, it is not kept as where the pointer rests, so
    // the wait goes on from the last real place, and ends past the slop.
    let mut tree = Tree::new();
    let (dialog, discard, _) = discard_dialog(&mut tree);
    tree.layout(WINDOW, &METRICS);
    let at = center(bounds(&tree, discard));
    tree.close_overlay(dialog).expect("closed");
    arrive(&mut tree, at);
    tree.handle(&hover(Point::new(f32::NAN, 3.0)));
    let (_, discard, _) = discard_dialog(&mut tree);
    tree.layout(WINDOW, &METRICS);
    tree.handle(&press(at));
    assert!(acted(&tree.handle(&release(at))).is_empty());
    tree.handle(&hover(Point::new(at.x + 10.0, at.y)));
    tree.handle(&press(at));
    let up = tree.handle(&release(at));
    assert_eq!(acted(&up), [(discard, ActionKind::Activate)]);
    // With no pointer seen: it places nothing, so the next event does.
    let mut tree = Tree::new();
    let (_, discard, _) = discard_dialog(&mut tree);
    tree.layout(WINDOW, &METRICS);
    let at = center(bounds(&tree, discard));
    tree.handle(&hover(Point::new(f32::NAN, f32::NAN)));
    tree.handle(&press(at));
    assert!(acted(&tree.handle(&release(at))).is_empty());
}

#[test]
fn a_double_click_on_a_palette_item_cannot_accept_the_prompt_it_opens() {
    // In a window where a consent prompt's Install button lands on a
    // palette item, the first click runs the item, which opens the prompt;
    // the second, with no move or a jitter within the slop, accepts
    // nothing (ADR-0055, ADR-0056).
    let window = Size::new(800.0, 400.0);
    let prompt = |tree: &mut Tree| {
        let dialog = Modal::new("Install the Android SDK?")
            .text("Downloads 1.2 GB")
            .open(tree)
            .expect("opened");
        let row = Modal::actions(tree, dialog).expect("a row");
        tree.insert(row, Button::new("Cancel", ButtonKind::Ghost).node())
            .expect("a button");
        let install = tree
            .insert(row, Button::new("Install", ButtonKind::Teal).node())
            .expect("a button");
        (dialog, install)
    };
    let mut probe = Tree::new();
    let (_, install) = prompt(&mut probe);
    probe.layout(window, &METRICS);
    let at = center(bounds(&probe, install));

    for jitter in [0.0, 1.5] {
        let mut tree = Tree::new();
        let palette = Palette::new("Command palette")
            .open(&mut tree)
            .expect("opened");
        let items: Vec<Row> = (0..10).map(|n| Row::new(format!("Command {n}"))).collect();
        Palette::set_items(&mut tree, palette, items).expect("items");
        tree.layout(window, &METRICS);
        assert!(
            tree.accessibility()
                .nodes()
                .iter()
                .any(|node| node.role == Role::ListItem && node.bounds.contains(at)),
            "an item under where Install lands"
        );
        arrive(&mut tree, at);
        tree.handle(&press(at));
        let up = tree.handle(&release(at));
        assert_eq!(acted(&up), [(palette, ActionKind::Activate)]);
        // The owner runs it: the palette closes, and the prompt opens.
        tree.close_overlay(palette).expect("closed");
        let (dialog, install) = prompt(&mut tree);
        tree.layout(window, &METRICS);
        let second = Point::new(at.x + jitter, at.y - jitter);
        assert_eq!(tree.hit_test(second), Some(install));
        tree.handle(&hover(second));
        let down = tree.handle(&press(second));
        let up = tree.handle(&release(second));
        assert!(down.blocked && up.actions.is_empty(), "{jitter}: {up:?}");
        assert_eq!(tree.modal(), Some(dialog));
        // Moved past the slop, then clicked, it is Install's.
        tree.handle(&hover(Point::new(at.x, at.y + 5.0)));
        tree.handle(&press(at));
        let up = tree.handle(&release(at));
        assert_eq!(acted(&up), [(install, ActionKind::Activate)]);
    }
}

// ---------- Overlays hosted by an inert layer ----------

/// A pass-through, modeless or modal overlay 120 by 60, painting its
/// bounds in `token`, with a button.
fn painted_overlay(
    tree: &mut Tree,
    kind: OverlayKind,
    anchor: Anchor,
    token: Token,
) -> (NodeId, NodeId) {
    let node = Node::new(Style::column().fixed(120.0, 60.0))
        .with_semantics(Semantics::new(Role::Group, "popup"))
        .with_widget(Fill(token));
    let root = tree.open_overlay(node, kind, anchor).expect("opened");
    let button = tree.insert(root, probe_button("Pick")).expect("a button");
    (root, button)
}

/// The index in the frame's list of the first rect painted in `token`.
fn painted_at(commands: &[Command], token: Token) -> Option<usize> {
    commands
        .iter()
        .position(|command| matches!(command, Command::Rect { color: painted, .. } if *painted == color(token)))
}

#[test]
fn an_overlay_anchored_to_an_inert_node_stays_under_the_modal() {
    for kind in [OverlayKind::PassThrough, OverlayKind::Modeless] {
        let Shell { mut tree, a, b, .. } = shell();
        let dialog = Modal::new("Run build script?")
            .open(&mut tree)
            .expect("opened");
        let row = Modal::actions(&tree, dialog).expect("a row");
        let allow = tree
            .insert(row, Button::new("Allow", ButtonKind::Teal).node())
            .expect("a button");
        // Opened after the dialog, anchored to the window's node A, over
        // the dialog.
        let anchor = Anchor::node(a, Side::Below, Align::Start, 0.0);
        let (popup, pick) = painted_overlay(&mut tree, kind, anchor, Token::Amber);
        tree.layout(WINDOW, &METRICS);
        tree.set_overlay_anchor(popup, Anchor::node(b, Side::Right, Align::Start, 100.0))
            .expect("re-anchored within the window");
        let frame = tree.frame(WINDOW, &METRICS).expect("a frame");
        assert_eq!(tree.inert(popup), Some(true), "{kind:?}");
        assert_eq!(tree.inert(pick), Some(true), "{kind:?}");
        assert!(!tree.tab_order().contains(&pick));
        assert_eq!(tree.overlays(), [popup, dialog], "{kind:?}");
        // It paints before the dialog's dimming, and is not hit.
        let commands = frame.list.commands();
        let window = Rect::new(0.0, 0.0, WINDOW.width, WINDOW.height);
        let dimming = commands
            .iter()
            .position(|command| matches!(command, Command::Rect { rect, .. } if *rect == window))
            .expect("the dimming");
        assert!(painted_at(commands, Token::Amber).expect("painted") < dimming);
        let over = center(bounds(&tree, popup));
        assert_eq!(tree.hit_test(over), Some(dialog));
        assert!(!tree.accessibility().to_string().contains("popup"));
        let _ = allow;
    }

    // A modeless one anchored to the window, opened after the dialog, is
    // live and above it (a toast); given a node of the window as its
    // anchor, it moves under the dialog and is inert.
    let Shell { mut tree, a, .. } = shell();
    let dialog = Modal::new("Run build script?")
        .open(&mut tree)
        .expect("opened");
    let anchor = Anchor::window(Align::End, Align::End, 0.0);
    let (toast, pick) = painted_overlay(&mut tree, OverlayKind::Modeless, anchor, Token::Green);
    tree.layout(WINDOW, &METRICS);
    assert_eq!(tree.inert(toast), Some(false));
    assert!(tree.tab_order().contains(&pick));
    assert_eq!(tree.overlays(), [dialog, toast]);
    tree.set_focus(pick, true).expect("the toast's button");
    tree.set_overlay_anchor(toast, Anchor::node(a, Side::Below, Align::Start, 0.0))
        .expect("re-anchored");
    assert_eq!(tree.overlays(), [toast, dialog]);
    assert_eq!(tree.inert(toast), Some(true));
    assert_eq!(
        tree.focused(),
        Some(dialog),
        "focus left the inert toast for the dialog"
    );
    let frame = tree.frame(WINDOW, &METRICS).expect("a frame");
    let window = Rect::new(0.0, 0.0, WINDOW.width, WINDOW.height);
    let commands = frame.list.commands();
    let dimming = commands
        .iter()
        .position(|command| matches!(command, Command::Rect { rect, .. } if *rect == window))
        .expect("the dimming");
    assert!(painted_at(commands, Token::Green).expect("painted") < dimming);
    // Anchored to the window again, it goes back on top, live.
    tree.set_overlay_anchor(toast, anchor).expect("re-anchored");
    assert_eq!(tree.overlays(), [dialog, toast]);
    assert_eq!(tree.inert(toast), Some(false));

    // A tooltip on the dialog's own button is above it and live; opened
    // while the dialog is hidden, one anchored to the window's node is
    // live, and inert under it once it is shown again.
    let Shell { mut tree, a, .. } = shell();
    let dialog = Modal::new("Run build script?")
        .open(&mut tree)
        .expect("opened");
    let row = Modal::actions(&tree, dialog).expect("a row");
    let allow = tree
        .insert(row, Button::new("Allow", ButtonKind::Teal).node())
        .expect("a button");
    let tip_anchor = Anchor::node(allow, Side::Below, Align::Start, 4.0);
    let (tip, _) = painted_overlay(&mut tree, OverlayKind::PassThrough, tip_anchor, Token::Teal);
    assert_eq!(tree.inert(tip), Some(false));
    let shown = *tree.style(dialog).expect("a style");
    tree.set_style(dialog, shown.hidden()).expect("hidden");
    let (menu, item) = painted_overlay(
        &mut tree,
        OverlayKind::Modeless,
        Anchor::node(a, Side::Below, Align::Start, 0.0),
        Token::Violet,
    );
    assert_eq!(tree.inert(menu), Some(false));
    tree.set_focus(item, true)
        .expect("live while the dialog is hidden");
    tree.set_style(dialog, shown).expect("shown");
    assert_eq!(tree.overlays(), [menu, dialog, tip]);
    assert_eq!(tree.inert(menu), Some(true));
    assert_eq!(
        tree.focused(),
        Some(dialog),
        "focus left the menu for the dialog"
    );
    // Closing the dialog, the tooltip it hosted is hosted by the window.
    tree.close_overlay(dialog).expect("closed");
    assert_eq!(tree.overlays(), [menu, tip]);
    assert_eq!(tree.inert(tip), Some(false));
}

// ---------- Focus given back when modals close out of order ----------

#[test]
fn modals_closed_out_of_order_give_focus_back_to_where_it_was() {
    // Dialogs, focus on each as it opens: either order ends on Save.
    for in_order in [true, false] {
        let mut tree = Tree::new();
        let save = tree
            .insert(tree.root(), Button::new("Save", ButtonKind::Ghost).node())
            .expect("a button");
        tree.set_focus(save, true).expect("focused");
        let a = Modal::new("Building").open(&mut tree).expect("opened");
        let b = Modal::new("Build failed").open(&mut tree).expect("opened");
        let (first, second) = if in_order { (b, a) } else { (a, b) };
        tree.close_overlay(first).expect("closed");
        tree.close_overlay(second).expect("closed");
        assert_eq!(tree.focused(), Some(save), "in order: {in_order}");
    }
    // The upper one remembered no node: closing the lower first hands it
    // the lower one's.
    let Shell { mut tree, a, .. } = shell();
    tree.set_focus(a, true).expect("focused");
    let (lower, _, _) = dialog(&mut tree, OverlayKind::Modal);
    assert_eq!(tree.focused(), None);
    let (upper, _, _) = dialog(&mut tree, OverlayKind::Modal);
    assert_eq!(tree.close_overlay(lower), Ok(None));
    assert_eq!(tree.close_overlay(upper), Ok(Some(a)));
    // The upper one remembered a node since removed: the same.
    let Shell { mut tree, a, .. } = shell();
    tree.set_focus(a, true).expect("focused");
    let (lower, x, _) = dialog(&mut tree, OverlayKind::Modal);
    tree.set_focus(x, true).expect("focused");
    let (upper, _, _) = dialog(&mut tree, OverlayKind::Modal);
    tree.remove(x).expect("removed");
    assert_eq!(tree.close_overlay(lower), Ok(None));
    assert_eq!(tree.close_overlay(upper), Ok(Some(a)));
    // A node the upper one remembered that is still there is kept.
    let Shell { mut tree, a, b, .. } = shell();
    tree.set_focus(a, true).expect("focused");
    let (lower, _, _) = dialog(&mut tree, OverlayKind::Modal);
    let hidden = *tree.style(lower).expect("a style");
    tree.set_style(lower, hidden.hidden()).expect("hidden");
    tree.set_focus(b, true)
        .expect("B, while the lower one is hidden");
    let (upper, _, _) = dialog(&mut tree, OverlayKind::Modal);
    assert_eq!(tree.close_overlay(lower), Ok(None));
    assert_eq!(tree.close_overlay(upper), Ok(Some(b)));
}

#[test]
fn a_hidden_overlay_follows_its_anchor() {
    let mut tree = Tree::new();
    let root = tree.root();
    let spacer = tree
        .insert(root, Node::new(Style::default().fixed(10.0, 0.0)))
        .expect("a spacer");
    let anchor = tree
        .insert(root, Node::new(Style::default().fixed(50.0, 20.0)))
        .expect("an anchor");
    let tip = tree
        .open_overlay(
            Node::new(Style::default().fixed(60.0, 20.0)),
            OverlayKind::PassThrough,
            Anchor::node(anchor, Side::Below, Align::Start, 0.0),
        )
        .expect("opened");
    tree.frame(WINDOW, &METRICS);
    let shown = *tree.style(tip).expect("a style");
    tree.set_style(tip, shown.hidden()).expect("hidden");
    // The anchor moves down 100 while the tooltip is hidden, then goes.
    tree.set_style(spacer, Style::default().fixed(10.0, 100.0))
        .expect("taller");
    tree.frame(WINDOW, &METRICS);
    let last = bounds(&tree, anchor);
    tree.remove(anchor).expect("removed");
    tree.set_style(tip, shown).expect("shown");
    tree.frame(WINDOW, &METRICS);
    assert_eq!(bounds(&tree, tip).y, last.bottom());
}

// ---------- Random sequences ----------

/// One step of a random sequence. Nodes are named by an index into every
/// id the sequence made, removed ones too, the main root's first.
#[derive(Clone, Debug)]
enum Op {
    Open {
        kind: OverlayKind,
        anchor: Option<usize>,
        align: Align,
        side: Side,
        size: (f32, f32),
        buttons: u8,
    },
    Close(usize),
    Remove(usize),
    Tab(bool),
    Press(Point),
    Hover(Point),
    Focus(usize),
    Hide(usize, bool),
    Disable(usize, bool),
    Frame(Size),
    /// Gives a node a tooltip and opens it.
    Tooltip(usize),
    /// Shows a toast, with an action or not.
    Toast(bool),
    /// Closes the toast at an index of those shown.
    CloseToast(usize),
    /// Opens a palette with this many items.
    Palette(u8),
    /// Escape, or a letter typed.
    Escape,
    Type,
}

const KINDS: [OverlayKind; 3] = [
    OverlayKind::Modal,
    OverlayKind::Modeless,
    OverlayKind::PassThrough,
];
const ALIGNS: [Align; 3] = [Align::Start, Align::Center, Align::End];
const SIDES: [Side; 4] = [Side::Above, Side::Below, Side::Left, Side::Right];

fn op(rng: &mut Pcg32, made: usize) -> Op {
    let pick = |rng: &mut Pcg32| rng.range(0..made as u32 + 2) as usize;
    let point =
        |rng: &mut Pcg32| Point::new(rng.range(-10.0..420.0f32), rng.range(-10.0..320.0f32));
    match rng.range(0..18u32) {
        12 => Op::Tooltip(pick(rng)),
        13 => Op::Toast(rng.range(0..2u32) == 0),
        14 => Op::CloseToast(rng.range(0..6u32) as usize),
        15 => Op::Palette(rng.range(0..30u32) as u8),
        16 => Op::Escape,
        17 => Op::Type,
        0 | 1 => Op::Open {
            kind: *rng.choose(&KINDS).expect("kinds"),
            anchor: (rng.range(0..2u32) == 0).then(|| pick(rng)),
            align: *rng.choose(&ALIGNS).expect("aligns"),
            side: *rng.choose(&SIDES).expect("sides"),
            size: (rng.range(0.0..500.0f32), rng.range(0.0..400.0f32)),
            buttons: rng.range(0..4u32) as u8,
        },
        2 => Op::Close(pick(rng)),
        3 => Op::Remove(pick(rng)),
        4 => Op::Tab(rng.range(0..2u32) == 0),
        5 => Op::Press(point(rng)),
        6 => Op::Hover(point(rng)),
        7 => Op::Focus(pick(rng)),
        8 => Op::Hide(pick(rng), rng.range(0..2u32) == 0),
        9 => Op::Disable(pick(rng), rng.range(0..3u32) == 0),
        _ => Op::Frame(if rng.range(0..4u32) == 0 {
            Size::new(rng.range(0.0..500.0f32), rng.range(0.0..400.0f32))
        } else {
            WINDOW
        }),
    }
}

/// The tree under test, every id it made, and the model's open overlays:
/// each one's root, kind and the node a modal remembered.
struct Run {
    tree: Tree,
    ids: Vec<NodeId>,
    /// In the order they opened: each root, kind, the node a modal
    /// remembered, and the node it is anchored to, if any.
    open: Vec<Open>,
}

/// An overlay the model holds open.
#[derive(Clone, Copy)]
struct Open {
    root: NodeId,
    kind: OverlayKind,
    remembered: Option<NodeId>,
    anchor: Option<NodeId>,
}

/// Whether `node` can take focus now: a tab stop, or a modal overlay's
/// root that is live, shown, enabled and focusable.
fn can_take(tree: &Tree, node: NodeId) -> bool {
    tree.tab_order().contains(&node)
        || (tree.overlay_kind(node) == Some(OverlayKind::Modal)
            && tree.focusable(node) == Some(true)
            && tree.inert(node) == Some(false)
            && tree.style(node).is_some_and(|style| !style.hidden)
            && tree.state(node).is_some_and(|state| !state.disabled))
}

impl Run {
    fn id(&self, at: usize) -> NodeId {
        self.ids.get(at).copied().unwrap_or(self.ids[0])
    }

    /// The model's active modal: the last modal whose root is shown.
    /// Modals always go on top, so they are in the tree's order as they
    /// opened.
    fn active(&self) -> Option<NodeId> {
        self.open
            .iter()
            .rev()
            .find(|open| {
                open.kind == OverlayKind::Modal
                    && self
                        .tree
                        .style(open.root)
                        .is_some_and(|style| !style.hidden)
            })
            .map(|open| open.root)
    }

    /// Where focus goes while the model's active modal is and no node has
    /// it: a palette's field, or the modal's root, if it can take it.
    fn default_focus(&self) -> Option<NodeId> {
        let modal = self.active()?;
        Palette::field(&self.tree, modal)
            .filter(|&field| can_take(&self.tree, field))
            .or_else(|| can_take(&self.tree, modal).then_some(modal))
    }

    /// Closes the overlay at `root` with `close`, and checks where focus
    /// went against the rules.
    fn close(&mut self, root: NodeId, close: impl FnOnce(&mut Tree)) -> Result<(), String> {
        let Some(at) = self.open.iter().position(|open| open.root == root) else {
            close(&mut self.tree);
            return Ok(());
        };
        let active = self.active() == Some(root);
        let closing = self.open[at];
        let inside =
            |tree: &Tree, node: NodeId| node == root || tree.overlay_of(node) == Some(root);
        // The modals above it, in the tree's order, which is theirs as they
        // opened; the nearest also takes over its node if it remembered
        // none or one since removed.
        let order = self.tree.overlays();
        let place = |root: NodeId| order.iter().position(|&open| open == root);
        let modal = closing.kind == OverlayKind::Modal;
        let mut nearest = modal;
        for above in self.open.iter_mut() {
            let is_modal = above.kind == OverlayKind::Modal;
            if (!is_modal && above.remembered.is_none()) || place(above.root) <= place(root) {
                continue;
            }
            let into = above
                .remembered
                .is_some_and(|node| inside(&self.tree, node));
            let lost = above
                .remembered
                .is_none_or(|node| !self.tree.contains(node));
            if into || (nearest && is_modal && lost) {
                above.remembered = closing.remembered;
            }
            if is_modal {
                nearest = false;
            }
        }
        let focus = self.tree.focused();
        let focus_inside = focus.is_some_and(|node| inside(&self.tree, node));
        // The active modal closing while focus is on an overlay above it
        // hands its node to that overlay.
        if active
            && !focus_inside
            && let Some(focus) = focus
            && let Some(holder) = self.open.iter_mut().find(|open| {
                (open.root == focus || self.tree.overlay_of(focus) == Some(open.root))
                    && place(open.root) > place(root)
            })
            && holder.remembered.is_none()
        {
            holder.remembered = closing.remembered;
        }
        close(&mut self.tree);
        self.open.remove(at);
        let gives_back = active || (focus_inside && !modal);
        let mut expected = if focus.is_none() || focus_inside {
            closing
                .remembered
                .filter(|&node| gives_back && can_take(&self.tree, node))
        } else {
            focus
        };
        // Focus on an overlay the close made inert, a toast that now waits
        // under the modal uncovered, leaves it; while a modal is active,
        // focus rests on its default.
        if expected.is_some_and(|node| self.tree.inert(node) == Some(true)) {
            expected = None;
        }
        if expected.is_none() {
            expected = self.default_focus();
        }
        if self.tree.focused() == expected {
            Ok(())
        } else {
            Err(format!(
                "closing {root}: focus on {:?}, not {expected:?}",
                self.tree.focused()
            ))
        }
    }

    /// Runs `open`, which opens or closes overlays through a widget, and
    /// brings the model up to date: an overlay it opened is recorded, with
    /// the node focused before for a modal one, and one it closed is
    /// dropped. The node it returns, if any, and its nodes join the ids.
    fn widget_op(&mut self, open: impl FnOnce(&mut Tree) -> Option<NodeId>) {
        let focus = self.tree.focused();
        let made = open(&mut self.tree);
        let listed = self.tree.overlays();
        self.open.retain(|open| listed.contains(&open.root));
        for root in listed {
            if self.open.iter().any(|open| open.root == root) {
                continue;
            }
            let kind = self.tree.overlay_kind(root).expect("an overlay");
            let anchor = match self.tree.overlay_anchor(root) {
                Some(Anchor::Node { node, .. }) => Some(node),
                _ => None,
            };
            self.open.push(Open {
                root,
                kind,
                remembered: (kind == OverlayKind::Modal).then_some(focus).flatten(),
                anchor,
            });
            self.ids.push(root);
        }
        if let Some(made) = made {
            let mut stack = vec![made];
            while let Some(id) = stack.pop() {
                if !self.ids.contains(&id) {
                    self.ids.push(id);
                }
                stack.extend(self.tree.children(id).unwrap_or_default());
            }
        }
    }

    fn apply(&mut self, op: &Op) -> Result<(), String> {
        match *op {
            Op::Open {
                kind,
                anchor,
                align,
                side,
                size,
                buttons,
            } => {
                let anchor_node = anchor.map(|at| self.id(at));
                let anchor = match anchor_node {
                    Some(node) => Anchor::node(node, side, align, 4.0),
                    None => Anchor::window(align, Align::Center, 8.0),
                };
                let mut node = Node::new(Style::column().fixed(size.0, size.1))
                    .with_semantics(Semantics::new(Role::Dialog, "d"));
                // Half the modals' roots take focus, as a `Modal`'s does.
                node.focusable = kind == OverlayKind::Modal && buttons % 2 == 1;
                let focus = self.tree.focused();
                let count = self.tree.node_count();
                match self.tree.open_overlay(node, kind, anchor) {
                    Ok(root) => {
                        let remembered = (kind == OverlayKind::Modal).then_some(focus).flatten();
                        self.open.push(Open {
                            root,
                            kind,
                            remembered,
                            anchor: anchor_node,
                        });
                        self.ids.push(root);
                        // A modal's root that can take focus has it.
                        if kind == OverlayKind::Modal
                            && can_take(&self.tree, root)
                            && self.tree.focused() != Some(root)
                        {
                            return Err(format!("{root} opened without focus"));
                        }
                        for n in 0..buttons {
                            let id = self
                                .tree
                                .insert(root, probe_button(&format!("b{n}")))
                                .map_err(|error| error.to_string())?;
                            self.ids.push(id);
                        }
                    }
                    Err(_) => {
                        if self.tree.node_count() != count {
                            return Err("a refused open changed the tree".to_string());
                        }
                    }
                }
            }
            Op::Close(at) => {
                let id = self.id(at);
                if self.open.iter().any(|open| open.root == id) {
                    let mut result = None;
                    self.close(id, |tree| result = Some(tree.close_overlay(id)))?;
                    let returned = result.and_then(Result::ok).flatten();
                    if returned != self.tree.focused().filter(|_| returned.is_some()) {
                        return Err(format!("close returned {returned:?}"));
                    }
                } else if self.tree.close_overlay(id).is_ok() {
                    return Err(format!("{id} closed, but it is not an open overlay"));
                }
            }
            Op::Remove(at) => {
                let id = self.id(at);
                self.close(id, |tree| {
                    let _ = tree.remove(id);
                })?;
            }
            Op::Tab(back) => {
                let modifiers = if back {
                    Modifiers::SHIFT
                } else {
                    Modifiers::NONE
                };
                self.tree.handle(&key_with(Key::Tab, modifiers));
            }
            Op::Press(at) => {
                self.tree.handle(&press(at));
            }
            Op::Hover(at) => {
                self.tree.handle(&hover(at));
            }
            Op::Focus(at) => {
                let id = self.id(at);
                let can = can_take(&self.tree, id);
                if self.tree.set_focus(id, true).is_ok() != can {
                    return Err(format!("set_focus({id}) disagrees with the tab order"));
                }
            }
            Op::Hide(at, hidden) => {
                let id = self.id(at);
                if let Some(style) = self.tree.style(id).copied() {
                    let style = Style { hidden, ..style };
                    self.tree
                        .set_style(id, style)
                        .map_err(|error| error.to_string())?;
                }
            }
            Op::Disable(at, disabled) => {
                let id = self.id(at);
                let _ = self.tree.set_enabled(id, !disabled);
            }
            Op::Tooltip(at) => {
                let id = self.id(at);
                if self.tree.set_tooltip(id, "A tip").is_ok() {
                    self.widget_op(|tree| Tooltip::open(tree, id).ok());
                }
            }
            Op::Toast(action) => {
                self.widget_op(|tree| {
                    let toast = Toast::new("Done.");
                    let toast = if action { toast.action("Show") } else { toast };
                    toast.show(tree).ok()
                });
            }
            Op::CloseToast(at) => {
                if let Some(&toast) = Toast::toasts(&self.tree).get(at) {
                    self.widget_op(|tree| {
                        let _ = Toast::close(tree, toast);
                        None
                    });
                }
            }
            Op::Palette(items) => {
                self.widget_op(|tree| {
                    let palette = Palette::new("Commands").open(tree).ok()?;
                    let rows: Vec<Row> = (0..items)
                        .map(|n| Row::new(format!("item {n}")).matched(Some(0..2)))
                        .collect();
                    Palette::set_items(tree, palette, rows).ok()?;
                    Some(palette)
                });
            }
            Op::Escape => {
                self.tree.handle(&key(Key::Escape));
            }
            Op::Type => {
                self.tree.handle(&InputEvent::Text("a".to_string()));
            }
            Op::Frame(size) => {
                self.tree.frame(size, &METRICS);
                // Every shown overlay lies inside the window, exactly.
                for open in &self.open {
                    let root = open.root;
                    let shown = self.tree.style(root).is_some_and(|style| !style.hidden);
                    let rect = bounds(&self.tree, root);
                    let inside = rect.x >= 0.0
                        && rect.y >= 0.0
                        && rect.right() <= size.width.max(0.0)
                        && rect.bottom() <= size.height.max(0.0);
                    if shown && !inside {
                        return Err(format!("{root} at {rect:?} is outside {size:?}"));
                    }
                }
                if self.tree.frame(size, &METRICS).is_some() {
                    return Err("an idle frame drew".to_string());
                }
                // A hit is a node that takes input.
                for point in [
                    Point::new(5.0, 5.0),
                    Point::new(150.0, 120.0),
                    Point::new(390.0, 290.0),
                ] {
                    if let Some(hit) = self.tree.hit_test(point) {
                        let pass = self
                            .tree
                            .overlay_of(hit)
                            .and_then(|root| self.tree.overlay_kind(root))
                            == Some(OverlayKind::PassThrough);
                        if self.tree.inert(hit) != Some(false) || pass {
                            return Err(format!("{hit} was hit at {point:?}"));
                        }
                    }
                }
            }
        }
        // An overlay handed a closed modal's node forgets it once focus is
        // on a node outside it.
        if let Some(focus) = self.tree.focused() {
            let top = self.tree.overlay_of(focus);
            for open in &mut self.open {
                if open.kind != OverlayKind::Modal && top != Some(open.root) {
                    open.remembered = None;
                }
            }
        }
        self.check()
    }

    /// The invariants that hold after every step.
    fn check(&self) -> Result<(), String> {
        let tree = &self.tree;
        let mut roots: Vec<NodeId> = self.open.iter().map(|open| open.root).collect();
        let mut listed = tree.overlays();
        roots.sort();
        listed.sort();
        if listed != roots {
            return Err(format!("overlays {listed:?}, not {roots:?}"));
        }
        // An overlay that is not modal, anchored to a node still there, is
        // live or inert with it, and painted under the active modal if
        // inert.
        let order = tree.overlays();
        let modal_at = tree
            .modal()
            .and_then(|modal| order.iter().position(|&root| root == modal));
        for open in &self.open {
            let Some(anchor) = open.anchor.filter(|&node| tree.contains(node)) else {
                continue;
            };
            if open.kind == OverlayKind::Modal {
                continue;
            }
            if tree.inert(open.root) != tree.inert(anchor) {
                return Err(format!(
                    "{} is inert {:?}, its anchor {anchor} {:?}",
                    open.root,
                    tree.inert(open.root),
                    tree.inert(anchor)
                ));
            }
            let at = order.iter().position(|&root| root == open.root);
            if tree.inert(open.root) == Some(true) && at >= modal_at {
                return Err(format!("inert {} is painted above the modal", open.root));
            }
        }
        if tree.modal() != self.active() {
            return Err(format!(
                "the active modal is {:?}, not {:?}",
                tree.modal(),
                self.active()
            ));
        }
        let order = tree.tab_order();
        if let Some(focus) = tree.focused()
            && !can_take(tree, focus)
        {
            return Err(format!("focus on {focus}, which cannot take it"));
        }
        // While a modal is active, focus never rests on no node if its
        // default can take it.
        if tree.focused().is_none()
            && let Some(default) = self.default_focus()
        {
            return Err(format!("focus on no node, though {default} can take it"));
        }
        let snapshot = tree.accessibility();
        for node in snapshot.nodes() {
            if tree.inert(node.id) != Some(false) {
                return Err(format!("{} is inert but exposed", node.id));
            }
            if node.modal != (tree.modal() == Some(node.id)) {
                return Err(format!("{} is marked modal wrongly", node.id));
            }
        }
        for &id in &self.ids {
            if tree.inert(id) != Some(true) {
                continue;
            }
            let state = tree.state(id).expect("in the tree");
            if state.hovered || state.pressed || state.focused || state.focus_within {
                return Err(format!("inert {id} is in state {state:?}"));
            }
            if order.contains(&id) {
                return Err(format!("inert {id} is a tab stop"));
            }
        }
        Ok(())
    }
}

#[test]
fn random_overlays_keep_the_rest_inert_and_give_focus_back() {
    check_sized(
        "overlays, inertness and focus",
        200,
        |rng, size| {
            let mut ops = Vec::new();
            // Room for the ids an open makes, counted roughly.
            for made in 0..size as usize {
                ops.push(op(rng, 4 + made * 2));
            }
            ops
        },
        |ops| {
            let Shell {
                tree,
                a,
                b,
                scroller,
            } = shell();
            let mut run = Run {
                ids: vec![tree.root(), a, b, scroller],
                tree,
                open: Vec::new(),
            };
            for (step, op) in ops.iter().enumerate() {
                run.apply(op)
                    .map_err(|error| format!("step {step}, {op:?}: {error}"))?;
            }
            Ok(())
        },
    );
}
