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

//! M3.3's tooltip, toast and command palette (ADR-0052, ADR-0053,
//! ADR-0054), through the public API only.
//!
//! - The tooltip: the target its owner's timer follows, for the pointer
//!   and for keyboard focus; WCAG 1.4.13's dismissible (Escape first,
//!   without moving the pointer or focus, also from a field and a dialog),
//!   hoverable (the pointer resting on it) and persistent; its place, look,
//!   role, the description it gives its node, its caps and errors; and
//!   over a modal.
//! - The toast: shown in the corner without taking focus, stacked, at most
//!   `MAX_TOASTS`, the signal that holds its timeout, its action, close
//!   control and Escape, roles, look, caps, and over a modal, live when
//!   shown after it.
//! - The palette: focus in its field, keyboard navigation (the roadmap's
//!   acceptance), Enter, Escape, Change on each edit, the combo box and its
//!   active option, the matched ranges drawn and their cuts (with a
//!   property test of random ranges), the pointer, the empty line, caps,
//!   and over a modal dialog.

use tidepool_core::color::Color;
use tidepool_core::geom::{Point, Rect, Size};
use tidepool_core::input::{InputEvent, Key, Modifiers, MouseButton};
use tidepool_core::rng::Pcg32;
use tidepool_core::testing::check_sized;
use tidepool_ui::a11y::Role;
use tidepool_ui::draw::Command;
use tidepool_ui::input::{Action, ActionKind, Response};
use tidepool_ui::layout::{Align, Style};
use tidepool_ui::metrics::{FixedMetrics, TextMetrics};
use tidepool_ui::overlay::{Anchor, OverlayKind, Side};
use tidepool_ui::theme::{Theme, Token};
use tidepool_ui::widgets::{
    self, Button, ButtonKind, Control, MAX_CELL_BYTES, MAX_MATCHES, MAX_TOAST_LINES, MAX_TOASTS,
    MAX_TOOLTIP_LINES, Modal, PALETTE_ROWS, PALETTE_WIDTH, Palette, ROW_HEIGHT, Row, TOAST_GAP,
    TOAST_MARGIN, TOAST_WIDTH, TOOLTIP_GAP, TOOLTIP_MAX_WIDTH, TextInput, Toast, ToastKind,
    Tooltip, TooltipCause, TooltipTarget,
};
use tidepool_ui::{Node, NodeId, Tree, UiError};

const METRICS: FixedMetrics = FixedMetrics::DEFAULT;
const WINDOW: Size = Size::new(800.0, 600.0);

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

fn repeat(key: Key) -> InputEvent {
    InputEvent::KeyDown {
        key,
        modifiers: Modifiers::NONE,
        repeat: true,
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

fn color(token: Token) -> Color {
    Theme::bundled().color(token)
}

fn center(rect: Rect) -> Point {
    Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
}

fn bounds(tree: &Tree, id: NodeId) -> Rect {
    tree.bounds(id).expect("in the tree")
}

fn kinds(response: &Response) -> Vec<(NodeId, ActionKind)> {
    response
        .actions
        .iter()
        .map(|action| (action.node, action.kind))
        .collect()
}

/// The commands a whole frame lists now.
fn painted(tree: &mut Tree) -> Vec<Command> {
    tree.invalidate_all();
    tree.frame(WINDOW, &METRICS)
        .expect("a frame")
        .list
        .commands()
        .to_vec()
}

/// A row of three buttons at the window's top left: Save and Run with
/// tooltips, Build without.
struct Bar {
    tree: Tree,
    save: NodeId,
    build: NodeId,
    run: NodeId,
}

fn bar() -> Bar {
    let mut tree = Tree::new();
    let root = tree.root();
    let row = tree
        .insert(root, Node::new(Style::row().gap(8.0)))
        .expect("a row");
    let save = tree
        .insert(row, Button::new("Save", ButtonKind::Ghost).node())
        .expect("a button");
    let build = tree
        .insert(row, Button::new("Build", ButtonKind::Ghost).node())
        .expect("a button");
    let run = tree
        .insert(row, Button::new("Run", ButtonKind::Ghost).node())
        .expect("a button");
    tree.set_tooltip(save, "Write main.rs to disk (Ctrl+S)")
        .expect("a tooltip");
    tree.set_tooltip(run, "Run the binary").expect("a tooltip");
    tree.layout(WINDOW, &METRICS);
    Bar {
        tree,
        save,
        build,
        run,
    }
}

fn pointer(node: NodeId) -> Option<TooltipTarget> {
    Some(TooltipTarget {
        node,
        cause: TooltipCause::Pointer,
    })
}

fn focus(node: NodeId) -> Option<TooltipTarget> {
    Some(TooltipTarget {
        node,
        cause: TooltipCause::Focus,
    })
}

// ---------- The tooltip ----------

#[test]
fn the_tooltip_target_follows_the_pointer_and_keyboard_focus() {
    let Bar {
        mut tree,
        save,
        build,
        run,
    } = bar();
    assert_eq!(tree.tooltip_target(), None);
    tree.handle(&hover(center(bounds(&tree, save))));
    assert_eq!(tree.tooltip_target(), pointer(save));
    tree.handle(&hover(center(bounds(&tree, build))));
    assert_eq!(tree.tooltip_target(), None, "Build has none");
    tree.handle(&hover(center(bounds(&tree, run))));
    assert_eq!(tree.tooltip_target(), pointer(run));
    // A key last: keyboard focus, which shows, is what counts.
    tree.handle(&key(Key::Tab));
    assert_eq!(tree.focused(), Some(save));
    assert_eq!(tree.tooltip_target(), focus(save));
    tree.handle(&key(Key::Tab));
    assert_eq!(
        tree.tooltip_target(),
        pointer(run),
        "Build has none; the pointer's"
    );
    tree.handle(&key(Key::Tab));
    assert_eq!(tree.tooltip_target(), focus(run));
    // The pointer moves off everything: focus still shows on Run.
    tree.handle(&hover(Point::new(700.0, 500.0)));
    assert_eq!(tree.tooltip_target(), focus(run));
    // A click on Build: focus does not show, and Build has none.
    let on_build = center(bounds(&tree, build));
    tree.handle(&press(on_build));
    tree.handle(&release(on_build));
    assert_eq!(tree.tooltip_target(), None);
    // The pointer leaves the window.
    tree.handle(&hover(Point::new(-5.0, -5.0)));
    assert_eq!(tree.tooltip_target(), None);
}

#[test]
fn the_pointer_can_rest_on_an_open_tooltip_and_a_press_on_it_reaches_nothing() {
    let Bar { mut tree, save, .. } = bar();
    // A button under where the tooltip goes.
    let under = tree
        .insert(tree.root(), Button::new("Hidden", ButtonKind::Ghost).node())
        .expect("a button");
    tree.handle(&hover(center(bounds(&tree, save))));
    let tip = Tooltip::open(&mut tree, save).expect("opened");
    assert_eq!(tree.overlay_kind(tip), Some(OverlayKind::Modeless));
    tree.layout(WINDOW, &METRICS);
    let placed = bounds(&tree, tip);
    assert_eq!(placed.y, bounds(&tree, save).bottom() + TOOLTIP_GAP);
    let covered = bounds(&tree, under);
    assert!(
        placed.y < covered.bottom() && covered.y < placed.bottom(),
        "{placed:?} {covered:?}"
    );
    // The pointer moves onto it: the target stays.
    let on_tip = Point::new(placed.x + 4.0, placed.y + 4.0);
    tree.handle(&hover(on_tip));
    assert_eq!(tree.hit_test(on_tip), Some(tip));
    assert_eq!(tree.tooltip_target(), pointer(save));
    // A press on it is its own, and reaches the button it covers no more.
    let response = tree.handle(&press(on_tip));
    assert!(response.handled && response.actions.is_empty());
    let response = tree.handle(&release(on_tip));
    assert!(response.actions.is_empty());
    assert_eq!(tree.focused(), None, "nothing it covers took focus");
    // Off it and off Save: the target changes.
    tree.handle(&hover(Point::new(700.0, 500.0)));
    assert_eq!(tree.tooltip_target(), None);
    // It holds nothing Tab reaches.
    assert!(!tree.tab_order().contains(&tip));
}

#[test]
fn escape_dismisses_a_tooltip_before_anything_else() {
    // From a field with text: the tooltip goes; the field's text, focus
    // and the pointer stay.
    let Bar { mut tree, save, .. } = bar();
    let field = tree
        .insert(tree.root(), TextInput::new("Search").node())
        .expect("a field");
    tree.set_focus(field, true).expect("focused");
    tree.handle(&InputEvent::Text("abc".to_string()));
    let on_save = center(bounds(&tree, save));
    tree.handle(&hover(on_save));
    let tip = Tooltip::open(&mut tree, save).expect("opened");
    tree.layout(WINDOW, &METRICS);
    let response = tree.handle(&key(Key::Escape));
    assert_eq!(kinds(&response), [(tip, ActionKind::Dismiss)]);
    assert_eq!(tree.focused(), Some(field));
    assert_eq!(
        tree.tooltip_target(),
        None,
        "dismissed, though the pointer stayed on Save"
    );
    // A repeat of the held Escape dismisses nothing more.
    let response = tree.handle(&repeat(Key::Escape));
    assert!(response.handled && response.actions.is_empty());
    tree.close_overlay(tip).expect("closed");
    // With no tooltip open, Escape is the field's again.
    let response = tree.handle(&key(Key::Escape));
    assert_eq!(kinds(&response), [(field, ActionKind::Dismiss)]);

    // In a dialog: a tooltip on its button goes, not the dialog.
    let mut tree = Tree::new();
    let dialog = Modal::new("Discard changes?")
        .open(&mut tree)
        .expect("opened");
    let row = Modal::actions(&tree, dialog).expect("a row");
    let cancel = tree
        .insert(row, Button::new("Cancel", ButtonKind::Ghost).node())
        .expect("a button");
    tree.set_tooltip(cancel, "Keep the changes")
        .expect("a tooltip");
    tree.layout(WINDOW, &METRICS);
    tree.handle(&key(Key::Tab));
    assert_eq!(tree.tooltip_target(), focus(cancel));
    let tip = Tooltip::open(&mut tree, cancel).expect("opened");
    assert_eq!(tree.inert(tip), Some(false), "above the dialog it is on");
    let response = tree.handle(&key(Key::Escape));
    assert_eq!(kinds(&response), [(tip, ActionKind::Dismiss)]);
    assert_eq!(tree.modal(), Some(dialog));
    tree.close_overlay(tip).expect("closed");
    let response = tree.handle(&key(Key::Escape));
    assert_eq!(kinds(&response), [(dialog, ActionKind::Dismiss)]);
}

#[test]
fn a_tooltip_describes_its_node_and_keeps_its_text_bounded() {
    let Bar {
        mut tree,
        save,
        build,
        ..
    } = bar();
    let shown = tree.accessibility().to_string();
    assert!(
        shown.contains("button \"Save\" description \"Write main.rs to disk (Ctrl+S)\" focusable"),
        "{shown}"
    );
    // A change of the button keeps its tooltip.
    widgets::update::<Button, _>(&mut tree, save, |button| button.set_label("Save all"))
        .expect("updated");
    assert_eq!(tree.tooltip(save), Some("Write main.rs to disk (Ctrl+S)"));
    // Open, it is a tooltip named by its text, not focusable.
    let tip = Tooltip::open(&mut tree, save).expect("opened");
    tree.layout(WINDOW, &METRICS);
    let snapshot = tree.accessibility();
    let node = snapshot.get(tip).expect("exposed");
    assert_eq!(node.role, Role::Tooltip);
    assert_eq!(node.name, "Write main.rs to disk (Ctrl+S)");
    assert!(!node.focusable);
    // Cleaned and capped: no direction overrides or controls, at most
    // `MAX_TOOLTIP_LINES` lines, each at most `MAX_CELL_BYTES`; a longer
    // line is cut and the last read, as a dialog's.
    let full = "x".repeat(MAX_CELL_BYTES);
    let text = format!("A\u{202E}B\u{0}\tC\n{full}\n3\n4\n5\n6");
    tree.set_tooltip(build, &text).expect("a tooltip");
    let kept = tree.tooltip(build).expect("kept");
    let lines: Vec<&str> = kept.split('\n').collect();
    assert_eq!(lines.len(), MAX_TOOLTIP_LINES);
    assert_eq!(lines[0], "AB C");
    assert_eq!(lines[1], full);
    let long = "x".repeat(2 * MAX_CELL_BYTES);
    tree.set_tooltip(build, format!("{long}\nnext"))
        .expect("a tooltip");
    assert_eq!(tree.tooltip(build).map(str::len), Some(MAX_CELL_BYTES));
    // Blank is none, and opening none is an error; so is a stale node.
    tree.set_tooltip(build, " \u{200E} ").expect("set");
    assert_eq!(tree.tooltip(build), None);
    assert_eq!(
        Tooltip::open(&mut tree, build),
        Err(UiError::NoTooltip { id: build })
    );
    tree.remove(build).expect("removed");
    assert_eq!(
        tree.set_tooltip(build, "x"),
        Err(UiError::UnknownNode { id: build })
    );
    assert_eq!(
        Tooltip::open(&mut tree, build),
        Err(UiError::UnknownNode { id: build })
    );
    assert_eq!(Tooltip::anchor(&tree, save), None);
}

#[test]
fn a_tooltip_sits_below_its_node_or_above_it_and_paints_in_tokens() {
    let mut tree = Tree::new();
    let root = tree.root();
    tree.set_style(root, Style::column().align(Align::Start, Align::End))
        .expect("a style");
    let low = tree
        .insert(root, Button::new("Low", ButtonKind::Ghost).node())
        .expect("a button");
    tree.set_tooltip(low, "wide ".repeat(100))
        .expect("a tooltip");
    let tip = Tooltip::open(&mut tree, low).expect("opened");
    let commands = painted(&mut tree);
    let placed = bounds(&tree, tip);
    // At the window's bottom: above it, at most the widest it gets.
    assert_eq!(placed.bottom(), bounds(&tree, low).y - TOOLTIP_GAP);
    assert_eq!(placed.width, TOOLTIP_MAX_WIDTH);
    assert!(commands.contains(&Command::Rect {
        rect: placed,
        color: color(Token::PanelSolid)
    }));
    assert!(commands.contains(&Command::Border {
        rect: placed,
        width: 1.0,
        color: color(Token::Line)
    }));
    assert!(commands.iter().any(|command| matches!(
        command,
        Command::Text { color: ink, .. } if *ink == color(Token::Text)
    )));
}

#[test]
fn an_inert_node_has_no_tooltip_target() {
    let Bar { mut tree, save, .. } = bar();
    tree.handle(&hover(center(bounds(&tree, save))));
    assert_eq!(tree.tooltip_target(), pointer(save));
    Modal::new("Quit?").open(&mut tree).expect("opened");
    tree.layout(WINDOW, &METRICS);
    assert_eq!(
        tree.tooltip_target(),
        None,
        "Save is inert under the dialog"
    );
}

#[test]
fn a_key_that_moves_no_focus_leaves_the_tooltip_on_the_pointers_node() {
    // Run has focus from a click, so focus does not show; the pointer is
    // on Save.
    let Bar {
        mut tree,
        save,
        run,
        ..
    } = bar();
    let on_run = center(bounds(&tree, run));
    tree.handle(&hover(on_run));
    tree.handle(&press(on_run));
    tree.handle(&release(on_run));
    assert_eq!(tree.focused(), Some(run));
    let on_save = center(bounds(&tree, save));
    tree.handle(&hover(on_save));
    assert_eq!(tree.tooltip_target(), pointer(save));
    // A modifier alone, and a letter, which makes focus show, move no
    // focus: Save stays the target.
    tree.handle(&key(Key::LeftShift));
    assert_eq!(tree.tooltip_target(), pointer(save));
    tree.handle(&key(Key::A));
    assert!(tree.focus_visible());
    assert_eq!(tree.tooltip_target(), pointer(save));
    // Escape dismisses Save's tooltip; then no node is the target, not Run
    // either, though its focus shows.
    let tip = Tooltip::open(&mut tree, save).expect("opened");
    tree.layout(WINDOW, &METRICS);
    let response = tree.handle(&key(Key::Escape));
    assert_eq!(kinds(&response), [(tip, ActionKind::Dismiss)]);
    assert_eq!(tree.tooltip_target(), None);
    tree.close_overlay(tip).expect("closed");
    tree.layout(WINDOW, &METRICS);
    assert_eq!(tree.tooltip_target(), None, "the owner closed it");
    // A move within Save keeps it dismissed; one onto another node ends
    // that, and back on Save it shows again.
    tree.handle(&hover(Point::new(on_save.x + 1.0, on_save.y)));
    assert_eq!(tree.tooltip_target(), None);
    tree.handle(&hover(on_run));
    assert_eq!(tree.tooltip_target(), pointer(run));
    tree.handle(&hover(on_save));
    assert_eq!(tree.tooltip_target(), pointer(save));
    // Focus that moves, by Tab, is followed: from Run it wraps to Save.
    tree.handle(&key(Key::Tab));
    assert_eq!(tree.focused(), Some(save));
    assert_eq!(tree.tooltip_target(), focus(save));
    // Dismissed by focus, it stays so while focus stays and the pointer is
    // still on Save; a move onto Run ends that.
    let tip = Tooltip::open(&mut tree, save).expect("opened");
    tree.layout(WINDOW, &METRICS);
    tree.handle(&key(Key::Escape));
    tree.close_overlay(tip).expect("closed");
    assert_eq!(tree.tooltip_target(), None);
    tree.handle(&hover(on_run));
    assert_eq!(tree.tooltip_target(), pointer(run));
}

#[test]
fn a_stale_tooltip_names_no_target_and_takes_no_escape() {
    // Its node is hidden while the pointer rests on the tooltip.
    let Bar { mut tree, save, .. } = bar();
    tree.set_tooltip(save, "one\ntwo\nthree\nfour")
        .expect("a tooltip");
    tree.handle(&hover(center(bounds(&tree, save))));
    let tip = Tooltip::open(&mut tree, save).expect("opened");
    tree.layout(WINDOW, &METRICS);
    let placed = bounds(&tree, tip);
    tree.handle(&hover(Point::new(placed.x + 4.0, placed.y + 20.0)));
    assert_eq!(tree.tooltip_target(), pointer(save));
    let style = *tree.style(save).expect("a style");
    tree.set_style(save, style.hidden()).expect("hidden");
    tree.layout(WINDOW, &METRICS);
    assert_eq!(tree.tooltip_target(), None, "its node is hidden");
    let response = tree.handle(&key(Key::Escape));
    assert!(response.actions.is_empty(), "{response:?}");

    // Moved above a modal, anchored to another node: the node it was
    // opened for, inert under the modal, is no target, and Escape is the
    // modal's.
    let Bar { mut tree, save, .. } = bar();
    let tip = Tooltip::open(&mut tree, save).expect("opened");
    let palette = Palette::new("Command palette")
        .open(&mut tree)
        .expect("opened");
    let field = Palette::field(&tree, palette).expect("a field");
    tree.set_overlay_anchor(tip, Anchor::node(field, Side::Below, Align::Start, 4.0))
        .expect("anchored again");
    tree.layout(WINDOW, &METRICS);
    tree.handle(&hover(center(bounds(&tree, tip))));
    assert_eq!(tree.inert(save), Some(true));
    assert_eq!(tree.inert(tip), Some(false));
    assert_eq!(tree.tooltip_target(), None);
    let response = tree.handle(&key(Key::Escape));
    assert_eq!(kinds(&response), [(palette, ActionKind::Dismiss)]);
}

#[test]
fn an_open_tooltip_says_its_nodes_new_text() {
    let Bar { mut tree, save, .. } = bar();
    let tip = Tooltip::open(&mut tree, save).expect("opened");
    tree.layout(WINDOW, &METRICS);
    let before = bounds(&tree, tip);
    tree.set_tooltip(save, "Saving\nmain.rs")
        .expect("a tooltip");
    let commands = painted(&mut tree);
    let said: Vec<&str> = commands
        .iter()
        .filter_map(|command| match command {
            Command::Text { origin, text, .. } if bounds(&tree, tip).contains(*origin) => {
                Some(text.as_str())
            }
            _ => None,
        })
        .collect();
    assert_eq!(said, ["Saving", "main.rs"]);
    let snapshot = tree.accessibility();
    assert_eq!(snapshot.get(tip).expect("exposed").name, "Saving main.rs");
    assert!(bounds(&tree, tip).height > before.height, "two lines now");
    // Given none, it keeps what it says, and its node is no longer one
    // with a tooltip, so its owner closes it.
    tree.set_tooltip(save, " ").expect("none");
    assert_eq!(tree.tooltip(save), None);
    let snapshot = tree.accessibility();
    assert_eq!(snapshot.get(tip).expect("exposed").name, "Saving main.rs");
}

#[test]
fn a_tooltip_forced_over_its_node_never_keeps_the_pointer_from_it() {
    let Bar { mut tree, save, .. } = bar();
    tree.set_tooltip(
        save,
        "Write the file to disk\nwith Ctrl+S\nor the menu\nnow",
    )
    .expect("a tooltip");
    // Too small for it below or above Save: it is moved over Save.
    let tiny = Size::new(120.0, 60.0);
    tree.layout(tiny, &METRICS);
    let tip = Tooltip::open(&mut tree, save).expect("opened");
    tree.layout(tiny, &METRICS);
    let node = bounds(&tree, save);
    let placed = bounds(&tree, tip);
    let at = center(node);
    assert!(placed.contains(at), "{placed:?} covers {node:?}");
    // Over Save, the pointer and a press reach Save.
    assert_eq!(tree.hit_test(at), Some(save));
    tree.handle(&hover(at));
    assert_eq!(tree.tooltip_target(), pointer(save));
    tree.handle(&press(at));
    let response = tree.handle(&release(at));
    assert_eq!(kinds(&response), [(save, ActionKind::Activate)]);
    // Where it covers anything else, it is the tooltip's.
    let beside = Point::new(placed.right() - 2.0, placed.bottom() - 2.0);
    assert!(!node.contains(beside));
    assert_eq!(tree.hit_test(beside), Some(tip));
}

// ---------- The toast ----------

#[test]
fn a_toast_shows_in_the_corner_without_taking_focus_and_stacks() {
    let Bar { mut tree, save, .. } = bar();
    tree.set_focus(save, true).expect("focused");
    let first = Toast::new("Build finished.")
        .show(&mut tree)
        .expect("shown");
    assert_eq!(tree.focused(), Some(save), "it takes no focus");
    tree.layout(WINDOW, &METRICS);
    let placed = bounds(&tree, first);
    assert_eq!(placed.width, TOAST_WIDTH);
    assert_eq!(placed.right(), WINDOW.width - TOAST_MARGIN);
    assert_eq!(placed.bottom(), WINDOW.height - TOAST_MARGIN);
    // The next goes below it, at the corner, and pushes it up.
    let second = Toast::new("Tests passed.").show(&mut tree).expect("shown");
    tree.layout(WINDOW, &METRICS);
    assert_eq!(Toast::toasts(&tree), [first, second]);
    assert_eq!(bounds(&tree, second).bottom(), WINDOW.height - TOAST_MARGIN);
    assert_eq!(
        bounds(&tree, first).bottom(),
        bounds(&tree, second).y - TOAST_GAP
    );
    // Each is an overlay of its own; at most `MAX_TOASTS` show.
    assert_eq!(tree.overlays(), [first, second]);
    assert_eq!(tree.overlay_kind(first), Some(OverlayKind::Modeless));
    let mut all = vec![first, second];
    for n in 0..MAX_TOASTS {
        all.push(
            Toast::new(format!("Toast {n}"))
                .show(&mut tree)
                .expect("shown"),
        );
    }
    let shown = Toast::toasts(&tree);
    assert_eq!(shown.len(), MAX_TOASTS);
    assert_eq!(shown, all[all.len() - MAX_TOASTS..]);
    assert!(!tree.contains(first), "the oldest went");
    // Closing each closes its overlay.
    for toast in shown {
        Toast::close(&mut tree, toast).expect("closed");
    }
    assert_eq!(tree.overlays(), []);
    assert_eq!(Toast::toasts(&tree), []);
    assert_eq!(
        Toast::close(&mut tree, save),
        Err(UiError::WrongWidget { id: save })
    );
    assert_eq!(
        Toast::close(&mut tree, first),
        Err(UiError::UnknownNode { id: first })
    );
}

#[test]
fn a_toasts_timeout_is_held_while_the_pointer_or_focus_is_on_it() {
    let mut tree = Tree::new();
    let toast = Toast::new("Indexing done.")
        .action("Show")
        .show(&mut tree)
        .expect("shown");
    tree.layout(WINDOW, &METRICS);
    assert_eq!(Toast::held(&tree, toast), Some(false));
    tree.handle(&hover(center(bounds(&tree, toast))));
    assert_eq!(Toast::held(&tree, toast), Some(true));
    tree.handle(&hover(Point::new(5.0, 5.0)));
    assert_eq!(Toast::held(&tree, toast), Some(false));
    // Tab reaches its controls, and focus there holds it too.
    tree.handle(&key(Key::Tab));
    assert_eq!(tree.focused(), Toast::close_button(&tree, toast));
    assert_eq!(Toast::held(&tree, toast), Some(true));
    tree.handle(&key(Key::Tab));
    assert_eq!(tree.focused(), Toast::action_button(&tree, toast));
    assert_eq!(Toast::held(&tree, tree.root()), None);
}

#[test]
fn a_toasts_action_close_control_and_escape() {
    let mut tree = Tree::new();
    let toast = Toast::new("Update available.")
        .kind(ToastKind::Warning)
        .action("Install")
        .show(&mut tree)
        .expect("shown");
    tree.layout(WINDOW, &METRICS);
    let action = Toast::action_button(&tree, toast).expect("an action");
    let close = Toast::close_button(&tree, toast).expect("a close control");
    assert_eq!(Toast::of(&tree, action), Some(toast));
    assert_eq!(Toast::of(&tree, tree.root()), None);
    let at = center(bounds(&tree, action));
    tree.handle(&press(at));
    let response = tree.handle(&release(at));
    assert_eq!(kinds(&response), [(action, ActionKind::Activate)]);
    let at = center(bounds(&tree, close));
    tree.handle(&press(at));
    let response = tree.handle(&release(at));
    assert_eq!(kinds(&response), [(close, ActionKind::Dismiss)]);
    let response = tree.handle(&key(Key::Escape));
    assert_eq!(kinds(&response), [(toast, ActionKind::Dismiss)]);
    let shown = tree.accessibility().to_string();
    assert!(
        shown.contains("status \"Update available.\"\n  button \"Close\" focusable"),
        "{shown}"
    );
    // An error is an alert, its stripe `error`.
    let error = Toast::new("Build failed.")
        .kind(ToastKind::Error)
        .show(&mut tree)
        .expect("shown");
    let commands = painted(&mut tree);
    let placed = bounds(&tree, error);
    assert_eq!(tree.semantics(error).expect("semantics").role, Role::Alert);
    assert!(commands.contains(&Command::Rect {
        rect: Rect::new(placed.x, placed.y, 3.0, placed.height),
        color: color(Token::Error)
    }));
    assert!(commands.contains(&Command::Rect {
        rect: placed,
        color: Theme::bundled().solid(Token::Panel)
    }));
    assert!(Toast::action_button(&tree, error).is_none());
}

#[test]
fn a_toasts_text_is_bounded_and_clean() {
    let full = "y".repeat(MAX_CELL_BYTES);
    let toast = Toast::new(format!("a\u{202E}b\u{1b}\n{full}\n3\n4\n5")).action("\u{0}  ");
    assert_eq!(toast.lines().len(), MAX_TOAST_LINES);
    assert_eq!(toast.lines()[0], "ab");
    assert_eq!(toast.lines()[1], full);
    let shown = format!("{toast:?}");
    assert!(
        shown.contains("action: None"),
        "a blank action is none: {shown}"
    );
    assert!(!shown.contains("yyyy"), "{shown}");
}

#[test]
fn a_toast_shown_over_a_modal_is_live_and_tab_reaches_it() {
    let mut tree = Tree::new();
    // One shown before the dialog is inert under it.
    let before = Toast::new("Saved.").show(&mut tree).expect("shown");
    let dialog = Modal::new("Discard changes?")
        .open(&mut tree)
        .expect("opened");
    let row = Modal::actions(&tree, dialog).expect("a row");
    let cancel = tree
        .insert(row, Button::new("Cancel", ButtonKind::Ghost).node())
        .expect("a button");
    assert_eq!(tree.inert(before), Some(true));
    // One shown now goes above the dialog and is live; the one before
    // stays inert under it (ADR-0055).
    let after = Toast::new("Indexing done.")
        .action("Show")
        .show(&mut tree)
        .expect("shown");
    tree.layout(WINDOW, &METRICS);
    assert_eq!(tree.inert(after), Some(false));
    assert_eq!(tree.inert(before), Some(true));
    assert_eq!(tree.overlays(), [before, dialog, after]);
    assert_eq!(tree.modal(), Some(dialog));
    assert_eq!(tree.focused(), Some(dialog), "it took no focus");
    let order = tree.tab_order();
    assert_eq!(order[0], cancel);
    assert!(order.contains(&Toast::action_button(&tree, after).expect("an action")));
    // It paints over the dialog's dimming.
    let commands = painted(&mut tree);
    let window = Rect::new(0.0, 0.0, WINDOW.width, WINDOW.height);
    let dimming = commands
        .iter()
        .position(|command| matches!(command, Command::Rect { rect, .. } if *rect == window))
        .expect("the dimming");
    let toast = commands
        .iter()
        .position(|command| matches!(command, Command::Rect { rect, .. } if *rect == bounds(&tree, after)))
        .expect("the toast");
    assert!(toast > dimming);
}

#[test]
fn a_blank_toast_is_refused() {
    let mut tree = Tree::new();
    assert_eq!(
        Toast::new("\u{202E}\u{0}\u{200B}\u{FEFF}").show(&mut tree),
        Err(UiError::Unnamed { role: Role::Status })
    );
    assert_eq!(
        Toast::new(" \n\t\n ")
            .kind(ToastKind::Error)
            .action("Retry")
            .show(&mut tree),
        Err(UiError::Unnamed { role: Role::Alert })
    );
    assert_eq!(tree.overlays(), []);
    assert_eq!(tree.node_count(), 1);
}

#[test]
fn a_fifth_toast_closes_the_oldest_not_held_and_moves_none() {
    let mut tree = Tree::new();
    let first = Toast::new("Build failed.")
        .action("Show")
        .show(&mut tree)
        .expect("shown");
    let rest: Vec<NodeId> = ["two", "three", "four"]
        .into_iter()
        .map(|text| Toast::new(text).show(&mut tree).expect("shown"))
        .collect();
    tree.layout(WINDOW, &METRICS);
    // Focus on the oldest's action holds it.
    let show = Toast::action_button(&tree, first).expect("an action");
    tree.set_focus(show, true).expect("focused");
    assert_eq!(Toast::held(&tree, first), Some(true));
    let placed = bounds(&tree, first);
    let fifth = Toast::new("five").show(&mut tree).expect("shown");
    tree.layout(WINDOW, &METRICS);
    assert!(tree.contains(first), "the held one stays");
    assert!(!tree.contains(rest[0]), "the oldest not held went");
    assert_eq!(tree.focused(), Some(show));
    assert_eq!(bounds(&tree, first), placed, "and it did not move");
    // While one is held, the new one goes above the others.
    assert_eq!(bounds(&tree, fifth).bottom(), placed.y - TOAST_GAP);
    // Once none is held, they settle: the newest at the corner.
    tree.clear_focus();
    tree.layout(WINDOW, &METRICS);
    assert_eq!(Toast::toasts(&tree), [first, rest[1], rest[2], fifth]);
    assert_eq!(bounds(&tree, fifth).bottom(), WINDOW.height - TOAST_MARGIN);
    assert_eq!(
        bounds(&tree, rest[2]).bottom(),
        bounds(&tree, fifth).y - TOAST_GAP
    );

    // The pointer holds one as well: on the oldest's action, a new one
    // closes the next oldest, and the click lands where it was aimed.
    let at = center(bounds(&tree, show));
    tree.handle(&hover(at));
    assert_eq!(Toast::held(&tree, first), Some(true));
    let placed = bounds(&tree, first);
    Toast::new("six").show(&mut tree).expect("shown");
    tree.layout(WINDOW, &METRICS);
    assert!(tree.contains(first) && !tree.contains(rest[1]));
    assert_eq!(bounds(&tree, first), placed);
    assert_eq!(tree.hit_test(at), Some(show));
    tree.handle(&press(at));
    let response = tree.handle(&release(at));
    assert_eq!(kinds(&response), [(show, ActionKind::Activate)]);
}

#[test]
fn a_toast_that_would_cover_a_dialog_waits_under_it() {
    let mut tree = Tree::new();
    let dialog = Modal::new("Discard changes?")
        .text("main.rs has changes that are not saved.")
        .open(&mut tree)
        .expect("opened");
    let row = Modal::actions(&tree, dialog).expect("a row");
    let discard = tree
        .insert(row, Button::new("Discard", ButtonKind::Amber).node())
        .expect("a button");
    let toast = Toast::new("Indexing done.")
        .action("Show")
        .show(&mut tree)
        .expect("shown");
    let action = Toast::action_button(&tree, toast).expect("an action");
    // A window where the corner of the column is over the dialog.
    let small = Size::new(360.0, 220.0);
    tree.layout(small, &METRICS);
    let (box_, over) = (bounds(&tree, dialog), bounds(&tree, toast));
    assert!(
        over.y < box_.bottom() && over.x < box_.right(),
        "{over:?} meets {box_:?}"
    );
    assert_eq!(Toast::waiting(&tree, toast), Some(true));
    assert_eq!(Toast::held(&tree, toast), Some(true), "its timeout waits");
    assert_eq!(tree.inert(toast), Some(true));
    assert_eq!(tree.overlays(), [toast, dialog], "under the dialog");
    assert!(!tree.tab_order().contains(&action));
    assert_eq!(tree.hit_test(center(over)), Some(dialog));
    assert_eq!(tree.hit_test(center(bounds(&tree, discard))), Some(discard));
    // It paints under the dimming.
    let commands = {
        tree.invalidate_all();
        tree.frame(small, &METRICS)
            .expect("a frame")
            .list
            .commands()
            .to_vec()
    };
    let window = Rect::new(0.0, 0.0, small.width, small.height);
    let dimming = commands
        .iter()
        .position(|command| matches!(command, Command::Rect { rect, .. } if *rect == window))
        .expect("the dimming");
    let fill = commands
        .iter()
        .position(|command| matches!(command, Command::Rect { rect, .. } if *rect == over))
        .expect("the toast");
    assert!(fill < dimming);
    // A second one goes to the corner, where it stands clear of the
    // dialog, and is live; the first, lifted above it, still waits.
    let second = Toast::new("Done.").show(&mut tree).expect("shown");
    tree.layout(small, &METRICS);
    assert!(bounds(&tree, second).y >= bounds(&tree, dialog).bottom());
    assert_eq!(Toast::waiting(&tree, second), Some(false));
    assert_eq!(Toast::waiting(&tree, toast), Some(true));
    assert_eq!(tree.overlays(), [toast, dialog, second]);
    // The window grows: they stand clear, and go above the dialog, live.
    tree.layout(WINDOW, &METRICS);
    for shown in [toast, second] {
        assert_eq!(Toast::waiting(&tree, shown), Some(false));
        assert_eq!(tree.inert(shown), Some(false));
        assert_eq!(Toast::held(&tree, shown), Some(false));
    }
    assert_eq!(
        tree.overlays(),
        [dialog, toast, second],
        "in the order they showed"
    );
    assert!(tree.tab_order().contains(&action));
    Toast::close(&mut tree, second).expect("closed");
    // Small again, it waits again; the dialog closes, and it waits no more.
    tree.layout(small, &METRICS);
    assert_eq!(Toast::waiting(&tree, toast), Some(true));
    tree.close_overlay(dialog).expect("closed");
    tree.layout(small, &METRICS);
    assert_eq!(Toast::waiting(&tree, toast), Some(false));
    assert_eq!(tree.inert(toast), Some(false));
    assert_eq!(Toast::waiting(&tree, tree.root()), None);
}

#[test]
fn a_modal_closed_while_focus_is_on_a_toast_above_it_hands_its_node_on() {
    let Bar { mut tree, save, .. } = bar();
    tree.set_focus(save, true).expect("focused");
    let dialog = Modal::new("Discard changes?")
        .open(&mut tree)
        .expect("opened");
    let toast = Toast::new("Saved.")
        .action("Show")
        .show(&mut tree)
        .expect("shown");
    tree.layout(WINDOW, &METRICS);
    let action = Toast::action_button(&tree, toast).expect("an action");
    tree.set_focus(action, true).expect("focused");
    // Focus stays on the toast; the toast gives it back to Save.
    assert_eq!(tree.close_overlay(dialog), Ok(None));
    assert_eq!(tree.focused(), Some(action));
    Toast::close(&mut tree, toast).expect("closed");
    assert_eq!(tree.focused(), Some(save));
}

// ---------- The command palette ----------

fn commands(count: usize) -> Vec<Row> {
    (0..count)
        .map(|n| Row::new(format!("Command {n}")).matched(Some(0..1)))
        .collect()
}

/// A palette open over a window, with `count` items.
fn palette(count: usize) -> (Tree, NodeId, NodeId) {
    let mut tree = Tree::new();
    let palette = Palette::new("Command palette")
        .placeholder("Type a command")
        .open(&mut tree)
        .expect("opened");
    Palette::set_items(&mut tree, palette, commands(count)).expect("items");
    tree.frame(WINDOW, &METRICS);
    let field = Palette::field(&tree, palette).expect("a field");
    (tree, palette, field)
}

/// The names of the list's rows on screen, the selected marked.
fn rows(tree: &Tree) -> Vec<(String, bool)> {
    tree.accessibility()
        .nodes()
        .iter()
        .filter(|node| node.role == Role::ListItem)
        .map(|node| (node.name.clone(), node.selected))
        .collect()
}

fn step(tree: &mut Tree, palette: NodeId, pressed: Key) -> Option<usize> {
    let response = tree.handle(&key(pressed));
    assert_eq!(
        kinds(&response),
        [(palette, ActionKind::Select)],
        "{pressed:?}"
    );
    tree.frame(WINDOW, &METRICS);
    Palette::selected(tree, palette)
}

#[test]
fn the_palette_opens_at_the_top_with_focus_in_its_field() {
    let mut tree = Tree::new();
    let save = tree
        .insert(tree.root(), Button::new("Save", ButtonKind::Ghost).node())
        .expect("a button");
    tree.set_focus(save, true).expect("focused");
    let palette = Palette::new("Command palette")
        .placeholder("Type a command")
        .open(&mut tree)
        .expect("opened");
    let field = Palette::field(&tree, palette).expect("a field");
    assert_eq!(tree.focused(), Some(field));
    assert_eq!(tree.modal(), Some(palette));
    assert_eq!(tree.inert(save), Some(true));
    tree.frame(WINDOW, &METRICS);
    let placed = bounds(&tree, palette);
    assert_eq!(placed.width, PALETTE_WIDTH);
    assert_eq!(placed.x, (WINDOW.width - PALETTE_WIDTH) / 2.0);
    assert_eq!(placed.y, 12.0);
    // No items: the empty line shows in place of the list.
    assert_eq!(
        tree.accessibility().to_string(),
        "dialog \"Command palette\" modal\n  \
         combo box \"Command palette\" = \"\" focusable focused expanded\n  \
         status \"No results\"\n"
    );
    Palette::set_items(&mut tree, palette, commands(3)).expect("items");
    tree.frame(WINDOW, &METRICS);
    let shown = tree.accessibility().to_string();
    assert!(
        shown.contains(
            "combo box \"Command palette\" = \"\" active \"Command 0\" focusable focused expanded"
        ),
        "{shown}"
    );
    assert!(
        shown.contains("list \"Results\" = \"row 1 of 3\"\n"),
        "{shown}"
    );
    assert!(!shown.contains("No results"), "{shown}");
    assert_eq!(rows(&tree)[0], ("Command 0".to_string(), true));
    // Tab stays in the field: the list takes no focus.
    tree.handle(&key(Key::Tab));
    assert_eq!(tree.focused(), Some(field));
    // Escape asks to close it; closing gives focus back.
    let response = tree.handle(&key(Key::Escape));
    assert_eq!(kinds(&response), [(palette, ActionKind::Dismiss)]);
    assert_eq!(tree.close_overlay(palette), Ok(Some(save)));
}

#[test]
fn the_palettes_keys_move_its_selection_and_enter_activates_it() {
    let (mut tree, palette, field) = palette(25);
    assert_eq!(Palette::selected(&tree, palette), Some(0));
    assert_eq!(step(&mut tree, palette, Key::Down), Some(1));
    assert_eq!(step(&mut tree, palette, Key::Up), Some(0));
    // Up from the first wraps to the last, scrolled into view.
    assert_eq!(step(&mut tree, palette, Key::Up), Some(24));
    let shown = rows(&tree);
    assert!(
        shown.contains(&("Command 24".to_string(), true)),
        "{shown:?}"
    );
    assert_eq!(step(&mut tree, palette, Key::Down), Some(0));
    // A page is `PALETTE_ROWS`, stopping at the ends.
    assert_eq!(step(&mut tree, palette, Key::PageDown), Some(PALETTE_ROWS));
    assert_eq!(
        step(&mut tree, palette, Key::PageDown),
        Some(2 * PALETTE_ROWS)
    );
    assert_eq!(step(&mut tree, palette, Key::PageDown), Some(24));
    assert!(rows(&tree).contains(&("Command 24".to_string(), true)));
    assert_eq!(
        step(&mut tree, palette, Key::PageUp),
        Some(24 - PALETTE_ROWS)
    );
    // Focus stayed in the field, whose active option follows.
    assert_eq!(tree.focused(), Some(field));
    let active = tree.accessibility().get(field).and_then(|node| node.active);
    let name = active
        .and_then(|id| tree.semantics(id))
        .map(|semantics| semantics.name.clone());
    assert_eq!(name.as_deref(), Some("Command 14"));
    // Enter activates it, at once, before any layout too.
    tree.handle(&key(Key::Down));
    let response = tree.handle(&key(Key::Enter));
    assert_eq!(kinds(&response), [(palette, ActionKind::Activate)]);
    assert_eq!(Palette::selected(&tree, palette), Some(15));
    assert!(tree.handle(&repeat(Key::Enter)).actions.is_empty());
    // Home and End stay the field's: its caret moves.
    tree.handle(&InputEvent::Text("abc".to_string()));
    let response = tree.handle(&key(Key::Home));
    assert!(response.actions.is_empty() && response.handled);
    assert_eq!(Palette::selected(&tree, palette), Some(15));
}

#[test]
fn the_palette_emits_change_as_its_query_is_edited() {
    let (mut tree, palette, field) = palette(3);
    let response = tree.handle(&InputEvent::Text("op".to_string()));
    assert_eq!(kinds(&response), [(field, ActionKind::Change)]);
    assert_eq!(Palette::query(&tree, palette), Some("op"));
    let response = tree.handle(&key(Key::Backspace));
    assert_eq!(kinds(&response), [(field, ActionKind::Change)]);
    assert_eq!(Palette::query(&tree, palette), Some("o"));
    // The owner ranks anew: no items, the empty line.
    Palette::set_items(&mut tree, palette, Vec::<Row>::new()).expect("items");
    tree.frame(WINDOW, &METRICS);
    assert_eq!(Palette::selected(&tree, palette), None);
    assert!(tree.handle(&key(Key::Enter)).actions.is_empty());
    let shown = tree.accessibility().to_string();
    assert!(
        shown.contains("combo box \"Command palette\" = \"o\" focusable"),
        "{shown}"
    );
    assert!(shown.contains("status \"No results\""), "{shown}");
    // A palette's field is the combo box the snapshot names.
    assert_eq!(
        tree.semantics(field).expect("semantics").role,
        Role::ComboBox
    );
}

#[test]
fn the_palette_draws_what_the_query_matched() {
    let mut tree = Tree::new();
    let palette = Palette::new("Go to file").open(&mut tree).expect("opened");
    let items = vec![
        Row::new("src/main.rs").matched(Some(4..8)),
        // Out of bounds, reversed, empty, and off a char boundary: cut.
        Row::new("é dit").matched([1..2, 9..20, std::ops::Range { start: 3, end: 1 }, 2..2]),
        // Inside a cluster: widened to it; overlapping: joined.
        Row::new("e\u{301}x").matched([1..2, 0..1, 2..4]),
    ];
    Palette::set_items(&mut tree, palette, items).expect("items");
    let commands = painted(&mut tree);
    let runs: Vec<(String, bool)> = commands
        .iter()
        .filter_map(|command| match command {
            Command::Text {
                text, color: ink, ..
            } if *ink == color(Token::TealHi) => Some((text.clone(), true)),
            Command::Text {
                text, color: ink, ..
            } if *ink == color(Token::Text) => Some((text.clone(), false)),
            _ => None,
        })
        .collect();
    let expected: Vec<(String, bool)> = [
        ("src/", false),
        ("main", true),
        (".rs", false),
        ("é", true),
        (" dit", false),
        ("e\u{301}x", true),
    ]
    .iter()
    .map(|&(text, matched)| (text.to_string(), matched))
    .collect();
    assert_eq!(runs, expected);
    // The runs sit where the whole label would: each starts at the width
    // of what comes before it.
    let pens: Vec<f32> = commands
        .iter()
        .filter_map(|command| match command {
            Command::Text { origin, text, .. }
                if text.as_str() == "main" || text.as_str() == "src/" =>
            {
                Some(origin.x)
            }
            _ => None,
        })
        .collect();
    let gap = pens[1] - pens[0] - METRICS.width("src/", &widgets::ROW_TEXT);
    assert!(gap.abs() < 1e-3, "{pens:?}");
    // At most `MAX_MATCHES` ranges are kept of a row.
    let many = Row::new("x".repeat(200)).matched((0..200).map(|n| n..n + 1));
    assert_eq!(many.matched_ranges().len(), MAX_MATCHES);
}

#[test]
fn random_matched_ranges_draw_the_label_as_far_as_it_shows_and_never_panic() {
    const PIECES: [&str; 6] = [
        "a",
        "é",
        "e\u{301}",
        "日本",
        "\u{1F468}\u{200D}\u{1F469}",
        " ",
    ];
    check_sized(
        "matched ranges",
        150,
        |rng: &mut Pcg32, size| {
            let label: String = (0..size.max(1))
                .map(|_| *rng.choose(&PIECES).expect("pieces"))
                .collect();
            let len = label.len() as u32 + 4;
            let ranges: Vec<std::ops::Range<usize>> = (0..rng.range(0..6u32))
                .map(|_| rng.range(0..len) as usize..rng.range(0..len) as usize)
                .collect();
            (label, ranges)
        },
        |(label, ranges)| {
            let mut tree = Tree::new();
            let palette = Palette::new("p")
                .open(&mut tree)
                .map_err(|e| e.to_string())?;
            let row = Row::new(label.clone()).matched(ranges.iter().cloned());
            Palette::set_items(&mut tree, palette, vec![row]).map_err(|e| e.to_string())?;
            let commands = painted(&mut tree);
            let text: String = commands
                .iter()
                .filter_map(|command| match command {
                    Command::Text {
                        text, color: ink, ..
                    } if *ink == color(Token::Text) || *ink == color(Token::TealHi) => {
                        Some(text.as_str())
                    }
                    _ => None,
                })
                .collect();
            // All of it, or as much as reaches the row's right edge, where
            // the row is cut.
            let shown = tree.accessibility();
            let row = shown
                .nodes()
                .iter()
                .find(|node| node.role == Role::ListItem)
                .map(|node| node.bounds)
                .ok_or("no row")?;
            let reach = row.x + widgets::ROW_PADDING + METRICS.width(&text, &widgets::ROW_TEXT);
            if text == *label || (label.starts_with(&text) && reach >= row.right()) {
                Ok(())
            } else {
                Err(format!("drew {text:?}"))
            }
        },
    );
}

#[test]
fn the_pointer_selects_and_activates_the_palettes_items() {
    let (mut tree, palette, field) = palette(5);
    let shown = tree.accessibility();
    let third = shown
        .nodes()
        .iter()
        .find(|node| node.name == "Command 2")
        .expect("a row")
        .id;
    let at = center(bounds(&tree, third));
    // Hovering it washes it.
    arrive(&mut tree, at);
    assert!(tree.state(third).expect("a row").hovered);
    // A press selects it, before any layout tells the palette; its Select
    // comes from the palette, as a key's does.
    let response = tree.handle(&press(at));
    assert_eq!(kinds(&response), [(palette, ActionKind::Select)]);
    assert_eq!(Palette::selected(&tree, palette), Some(2));
    assert_eq!(tree.focused(), Some(field), "focus stays in the field");
    // Its release over the list activates it.
    let response = tree.handle(&release(at));
    assert_eq!(kinds(&response), [(palette, ActionKind::Activate)]);
    tree.frame(WINDOW, &METRICS);
    assert_eq!(Palette::selected(&tree, palette), Some(2));
    // A press on a row released outside the list does not.
    tree.handle(&press(at));
    let response = tree.handle(&release(center(bounds(&tree, field))));
    assert!(response.actions.is_empty());
    // A press on the field does not.
    let on_field = center(bounds(&tree, field));
    tree.handle(&press(on_field));
    assert!(tree.handle(&release(on_field)).actions.is_empty());
}

#[test]
fn the_palette_over_a_modal_dialog_and_its_caps() {
    let mut tree = Tree::new();
    let dialog = Modal::new("Discard changes?")
        .open(&mut tree)
        .expect("opened");
    let palette = Palette::new("Command palette")
        .open(&mut tree)
        .expect("opened");
    assert_eq!(tree.modal(), Some(palette));
    assert_eq!(tree.inert(dialog), Some(true));
    assert_eq!(tree.close_overlay(palette), Ok(Some(dialog)));
    // A blank name is refused, as its field takes focus; nothing changes.
    let count = tree.node_count();
    assert_eq!(
        Palette::new(" \u{202E}").open(&mut tree),
        Err(UiError::Unnamed {
            role: Role::ComboBox
        })
    );
    assert_eq!(tree.node_count(), count);
    // Its items' text is capped as any collection's: what a row shows.
    let palette = Palette::new("p").open(&mut tree).expect("opened");
    let long = "z".repeat(3 * MAX_CELL_BYTES);
    Palette::set_items(
        &mut tree,
        palette,
        vec![Row::new(long).matched(Some(0..usize::MAX))],
    )
    .expect("items");
    tree.frame(WINDOW, &METRICS);
    let shown = rows(&tree);
    assert_eq!(shown[0].0.len(), MAX_CELL_BYTES);
    assert_eq!(
        Palette::set_items(&mut tree, dialog, Vec::<Row>::new()),
        Err(UiError::WrongWidget { id: dialog })
    );
    assert_eq!(Palette::field(&tree, dialog), None);
    assert_eq!(Palette::selected(&tree, dialog), None);
    let _ = ROW_HEIGHT;
}

#[test]
fn every_popups_idle_frame_draws_nothing() {
    let Bar { mut tree, save, .. } = bar();
    tree.handle(&hover(center(bounds(&tree, save))));
    Tooltip::open(&mut tree, save).expect("opened");
    Toast::new("Done.")
        .action("Show")
        .show(&mut tree)
        .expect("shown");
    let palette = Palette::new("p").open(&mut tree).expect("opened");
    Palette::set_items(&mut tree, palette, commands(40)).expect("items");
    assert!(tree.frame(WINDOW, &METRICS).is_some());
    assert_eq!(tree.frame(WINDOW, &METRICS), None);
    tree.handle(&key(Key::Up));
    assert!(tree.frame(WINDOW, &METRICS).is_some());
    assert_eq!(tree.frame(WINDOW, &METRICS), None);
    let _ = Action {
        node: save,
        kind: ActionKind::Activate,
    };
}

#[test]
fn in_a_short_window_the_palettes_list_gives_up_room_and_its_field_keeps_it() {
    let (mut tree, palette, field) = palette(30);
    let short = Size::new(800.0, 150.0);
    tree.frame(short, &METRICS);
    assert_eq!(bounds(&tree, field).height, 30.0);
    let list = Palette::list(&tree, palette).expect("a list");
    // 150 less two margins of 12, two paddings of 10, the field and the
    // gap: 66 for the list.
    assert_eq!(bounds(&tree, list).height, 66.0);
    // Its selection still scrolls into view.
    tree.handle(&key(Key::Up));
    tree.frame(short, &METRICS);
    let shown = tree.accessibility();
    let last = shown
        .nodes()
        .iter()
        .find(|node| node.name == "Command 29")
        .expect("on screen");
    assert!(last.selected);
    let view = bounds(&tree, list);
    assert!(
        last.bounds.bottom() <= view.bottom() && last.bounds.y >= view.y,
        "{last:?}"
    );
}

// ---------- The palette's pointer, items, keys and rows ----------

/// The node of the list item named `name`.
fn item(tree: &Tree, name: &str) -> NodeId {
    tree.accessibility()
        .nodes()
        .iter()
        .find(|node| node.role == Role::ListItem && node.name == name)
        .map(|node| node.id)
        .expect("an item on screen")
}

#[test]
fn items_given_between_a_press_and_its_release_activate_nothing() {
    for same in [true, false] {
        for frame in [false, true] {
            let (mut tree, palette, _) = palette(5);
            let at = center(bounds(&tree, item(&tree, "Command 2")));
            arrive(&mut tree, at);
            tree.handle(&press(at));
            assert_eq!(Palette::selected(&tree, palette), Some(2));
            // A ranking for the last keystroke arrives mid-click.
            let items = if same {
                commands(5)
            } else {
                (0..5).map(|n| Row::new(format!("Result {n}"))).collect()
            };
            Palette::set_items(&mut tree, palette, items).expect("items");
            if frame {
                tree.frame(WINDOW, &METRICS);
            }
            let up = tree.handle(&release(at));
            assert!(up.actions.is_empty(), "same {same}, frame {frame}: {up:?}");
            // The same items keep the selection; others select the first.
            let kept = if same { 2 } else { 0 };
            assert_eq!(Palette::selected(&tree, palette), Some(kept));
            // The next click, with no items given meanwhile, is whole.
            tree.frame(WINDOW, &METRICS);
            tree.handle(&press(at));
            let up = tree.handle(&release(at));
            assert_eq!(kinds(&up), [(palette, ActionKind::Activate)]);
            assert_eq!(Palette::selected(&tree, palette), Some(2));
        }
    }
}

#[test]
fn new_items_keep_the_selection_on_its_item_if_it_is_there_once() {
    let names = |names: &[&str]| -> Vec<Row> { names.iter().map(|&name| Row::new(name)).collect() };
    let (mut tree, palette, _) = palette(0);
    Palette::set_items(
        &mut tree,
        palette,
        names(&["Git: Fetch", "Git: Pull", "Git: Push"]),
    )
    .expect("items");
    tree.frame(WINDOW, &METRICS);
    tree.handle(&key(Key::Down));
    assert_eq!(Palette::selected(&tree, palette), Some(1));
    // Still among them, elsewhere: it stays on it.
    Palette::set_items(
        &mut tree,
        palette,
        names(&["Git: Push --force", "Git: Pull"]),
    )
    .expect("items");
    assert_eq!(Palette::selected(&tree, palette), Some(1));
    tree.frame(WINDOW, &METRICS);
    assert_eq!(
        rows(&tree),
        [
            ("Git: Push --force".to_string(), false),
            ("Git: Pull".to_string(), true)
        ]
    );
    let response = tree.handle(&key(Key::Enter));
    assert_eq!(kinds(&response), [(palette, ActionKind::Activate)]);
    // Twice among them: nothing tells them apart, so the first.
    Palette::set_items(
        &mut tree,
        palette,
        names(&["Git: Stash", "Git: Pull", "Git: Pull"]),
    )
    .expect("items");
    assert_eq!(Palette::selected(&tree, palette), Some(0));
    // Gone: the first.
    tree.handle(&key(Key::Down));
    Palette::set_items(&mut tree, palette, names(&["Git: Fetch"])).expect("items");
    assert_eq!(Palette::selected(&tree, palette), Some(0));
}

#[test]
fn a_press_whose_release_is_lost_leaves_the_next_click_whole() {
    for lost in ["focus", "modal", "outside"] {
        let (mut tree, palette, _) = palette(5);
        let first = center(bounds(&tree, item(&tree, "Command 1")));
        let outside = Point::new(5.0, 590.0);
        let at = if lost == "outside" { outside } else { first };
        arrive(&mut tree, at);
        tree.handle(&press(at));
        match lost {
            "focus" => {
                tree.handle(&InputEvent::FocusLost);
                tree.handle(&InputEvent::FocusGained);
            }
            "modal" => {
                let dialog = Modal::new("Sure?").open(&mut tree).expect("opened");
                tree.frame(WINDOW, &METRICS);
                tree.handle(&release(at));
                tree.close_overlay(dialog).expect("closed");
            }
            _ => {}
        }
        tree.frame(WINDOW, &METRICS);
        // Another item, clicked: it activates.
        let third = center(bounds(&tree, item(&tree, "Command 3")));
        tree.handle(&hover(Point::new(third.x, third.y + 1.0)));
        tree.handle(&press(third));
        let up = tree.handle(&release(third));
        assert_eq!(kinds(&up), [(palette, ActionKind::Activate)], "{lost}");
        assert_eq!(Palette::selected(&tree, palette), Some(3));
    }
}

#[test]
fn a_press_outside_the_palettes_controls_comes_back_blocked() {
    let (mut tree, palette, field) = palette(5);
    let outside = Point::new(5.0, 590.0);
    let placed = bounds(&tree, palette);
    let padding = Point::new(placed.x + 2.0, placed.y + 2.0);
    for at in [outside, padding] {
        arrive(&mut tree, at);
        let down = tree.handle(&press(at));
        let up = tree.handle(&release(at));
        assert!(down.blocked && !down.handled, "{at:?}: {down:?}");
        assert!(up.blocked && up.actions.is_empty(), "{at:?}: {up:?}");
        assert_eq!(tree.focused(), Some(field));
    }
}

#[test]
fn keys_that_move_nothing_emit_no_select() {
    for count in [0, 1] {
        let (mut tree, _, _) = palette(count);
        for moving in [Key::Up, Key::Down, Key::PageUp, Key::PageDown] {
            let response = tree.handle(&key(moving));
            assert!(
                response.handled && response.actions.is_empty(),
                "{moving:?} with {count}"
            );
        }
    }
    let (mut tree, many, _) = palette(25);
    assert!(
        tree.handle(&key(Key::PageUp)).actions.is_empty(),
        "at the start"
    );
    let response = tree.handle(&key(Key::Down));
    assert_eq!(kinds(&response), [(many, ActionKind::Select)]);
}

#[test]
fn focus_comes_back_to_the_palettes_field_and_never_rests_on_no_node() {
    let (mut tree, palette, field) = palette(3);
    tree.handle(&InputEvent::Text("op".to_string()));
    let toast = Toast::new("Indexing done.").show(&mut tree).expect("shown");
    tree.layout(WINDOW, &METRICS);
    let close = Toast::close_button(&tree, toast).expect("a close control");
    let at = center(bounds(&tree, close));
    arrive(&mut tree, at);
    tree.handle(&press(at));
    assert_eq!(tree.focused(), Some(close));
    let up = tree.handle(&release(at));
    assert_eq!(kinds(&up), [(close, ActionKind::Dismiss)]);
    // Its owner closes it: focus comes back to the field, which takes what
    // the user types.
    Toast::close(&mut tree, toast).expect("closed");
    assert_eq!(tree.focused(), Some(field));
    let response = tree.handle(&InputEvent::Text("en".to_string()));
    assert!(response.handled);
    assert_eq!(Palette::query(&tree, palette), Some("open"));
    // Cleared by its owner, it comes back too.
    tree.clear_focus();
    assert_eq!(tree.focused(), Some(field));
    // A dialog's comes back to the dialog.
    let dialog = Modal::new("Sure?").open(&mut tree).expect("opened");
    tree.clear_focus();
    assert_eq!(tree.focused(), Some(dialog));
}

/// `METRICS` with a wider advance for bold text, which counts what it
/// measures of the texts holding `ж`.
struct Bolder {
    calls: std::cell::Cell<usize>,
    bytes: std::cell::Cell<usize>,
}

impl Bolder {
    const fn new() -> Self {
        Self {
            calls: std::cell::Cell::new(0),
            bytes: std::cell::Cell::new(0),
        }
    }
}

impl TextMetrics for Bolder {
    fn line(&self, style: &tidepool_ui::metrics::TextStyle) -> tidepool_ui::metrics::LineMetrics {
        METRICS.line(style)
    }

    fn width(&self, text: &str, style: &tidepool_ui::metrics::TextStyle) -> f32 {
        if text.contains('ж') {
            self.calls.set(self.calls.get() + 1);
            self.bytes.set(self.bytes.get() + text.len());
        }
        let advance = if style.weight == tidepool_ui::metrics::Weight::Bold {
            10.0
        } else {
            7.0
        };
        text.chars().count() as f32 * advance
    }
}

#[test]
fn matched_characters_are_saira_bold_and_the_text_runs_at_their_widths() {
    let metrics = Bolder::new();
    let mut tree = Tree::new();
    let palette = Palette::new("Go to file").open(&mut tree).expect("opened");
    Palette::set_items(
        &mut tree,
        palette,
        vec![Row::new("src/main.rs").matched(Some(4..8))],
    )
    .expect("items");
    let frame = tree.frame(WINDOW, &metrics).expect("a frame");
    let runs: Vec<(String, f32, tidepool_ui::metrics::Weight)> = frame
        .list
        .commands()
        .iter()
        .filter_map(|command| match command {
            Command::Text {
                origin,
                text,
                style,
                color: ink,
            } if *ink == color(Token::Text) || *ink == color(Token::TealHi) => {
                Some((text.clone(), origin.x, style.weight))
            }
            _ => None,
        })
        .collect();
    let names: Vec<&str> = runs.iter().map(|run| run.0.as_str()).collect();
    assert_eq!(names, ["src/", "main", ".rs"]);
    assert_eq!(runs[1].2, tidepool_ui::metrics::Weight::Bold);
    assert_eq!(runs[0].2, tidepool_ui::metrics::Weight::Regular);
    assert_eq!(
        widgets::MATCH_TEXT,
        widgets::ROW_TEXT.weight(tidepool_ui::metrics::Weight::Bold)
    );
    // `src/` at 7 a character, `main` at the bold 10.
    assert_eq!(runs[1].1 - runs[0].1, 4.0 * 7.0);
    assert_eq!(runs[2].1 - runs[1].1, 4.0 * 10.0);
}

#[test]
fn a_long_label_with_many_matches_costs_only_what_shows_of_it() {
    // 4 KiB of `жa`, 64 ranges across it: each run is measured once, and
    // none past the row's right edge.
    let label: String = "жa".repeat(4096 / 3);
    let ranges: Vec<std::ops::Range<usize>> = (0..64).map(|k| k * 60 + 3..k * 60 + 33).collect();
    let metrics = Bolder::new();
    let mut tree = Tree::new();
    let palette = Palette::new("Go to file").open(&mut tree).expect("opened");
    let items: Vec<Row> = (0..PALETTE_ROWS)
        .map(|_| Row::new(label.clone()).matched(ranges.iter().cloned()))
        .collect();
    Palette::set_items(&mut tree, palette, items).expect("items");
    tree.frame(WINDOW, &metrics);
    metrics.calls.set(0);
    metrics.bytes.set(0);
    tree.invalidate_all();
    tree.frame(WINDOW, &metrics).expect("a frame");
    // A row shows 456 units: its first four runs, 14, 200, 140 and 200
    // units wide, reach past that, so each row paints and measures four,
    // 93 bytes, and the frame measures each it paints once more for its
    // bounds.
    let (calls, bytes) = (metrics.calls.get(), metrics.bytes.get());
    let rows = PALETTE_ROWS;
    assert_eq!(calls, rows * 4 * 2, "measures");
    assert_eq!(bytes, rows * 93 * 2, "bytes measured");
    assert!(bytes < label.len(), "less than one label in all");
}

#[test]
fn a_rows_hidden_characters_show_as_replacement_characters() {
    let (mut tree, palette, field) = palette(0);
    let items = vec![
        // `fdp`: bytes 10 to 13 of each.
        Row::new("invoice\u{202E}fdp.exe").matched(Some(10..13)),
        // `[2J`: bytes 4 to 7 here, 6 to 9 once ESC is three bytes.
        Row::new("run\u{1b}[2J\u{7}.sh").matched(Some(4..7)),
        Row::new("a\u{200B}b"),
        Row::new("ab"),
    ];
    Palette::set_items(&mut tree, palette, items).expect("items");
    tree.frame(WINDOW, &METRICS);
    let names: Vec<String> = rows(&tree).into_iter().map(|(name, _)| name).collect();
    assert_eq!(
        names,
        [
            "invoice\u{FFFD}fdp.exe",
            "run\u{FFFD}[2J\u{FFFD}.sh",
            "a\u{FFFD}b",
            "ab"
        ]
    );
    let snapshot = tree.accessibility();
    let active = snapshot
        .get(field)
        .and_then(|node| node.active)
        .and_then(|id| snapshot.get(id))
        .map(|node| node.name.clone());
    assert_eq!(active.as_deref(), Some("invoice\u{FFFD}fdp.exe"));
    let hidden = |c: char| {
        c.is_control()
            || matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200B}' | '\u{200E}' | '\u{200F}')
    };
    let commands = painted(&mut tree);
    let mut matched = Vec::new();
    for command in &commands {
        if let Command::Text {
            text, color: ink, ..
        } = command
        {
            assert!(!text.chars().any(hidden), "{text:?}");
            if *ink == color(Token::TealHi) {
                matched.push(text.as_str());
            }
        }
    }
    assert_eq!(matched, ["fdp", "[2J"], "the matched ranges still line up");
    for node in snapshot.nodes() {
        assert!(!node.name.chars().any(hidden), "{:?}", node.name);
    }
    // A table's cells and names too.
    let mut tree = Tree::new();
    let table = widgets::Collection::table(
        "Problems",
        [widgets::Column::new("File", 200.0)],
        vec![Row::cells(["main\u{202E}sr.rs"])],
    );
    tree.insert(tree.root(), table.node()).expect("a table");
    tree.frame(WINDOW, &METRICS);
    let snapshot = tree.accessibility();
    assert!(
        snapshot
            .nodes()
            .iter()
            .any(|node| node.role == Role::Cell && node.name == "main\u{FFFD}sr.rs"),
        "{snapshot}"
    );
}

// ---------- Round four: toasts' order, column, clip and pin ----------

/// A consent dialog, its Allow and Cancel, in `tree`.
fn consent(tree: &mut Tree, title: &str) -> (NodeId, NodeId, NodeId) {
    let dialog = Modal::new(title).open(tree).expect("opened");
    let row = Modal::actions(tree, dialog).expect("a row");
    let allow = tree
        .insert(row, Button::new("Allow", ButtonKind::Teal).node())
        .expect("a button");
    let cancel = tree
        .insert(row, Button::new("Cancel", ButtonKind::Ghost).node())
        .expect("a button");
    (dialog, allow, cancel)
}

/// Where `id` is in the order of the overlays.
fn place(tree: &Tree, id: NodeId) -> usize {
    tree.overlays()
        .iter()
        .position(|&root| root == id)
        .expect("an overlay")
}

#[test]
fn a_toast_stands_above_the_last_modal_opened_before_it_hidden_or_not() {
    // B, then a toast, then C. In a short window the toast waits under B;
    // C opens and is hidden; the window grows: the toast goes back above
    // B, under C, and stays inert under C when C shows (C3, B2).
    let short = Size::new(500.0, 250.0);
    let mut tree = Tree::new();
    let (b, _, _) = consent(&mut tree, "Run the build script?");
    let toast = Toast::new("rust-analyzer: indexing failed")
        .action("Open log")
        .show(&mut tree)
        .expect("shown");
    tree.layout(short, &METRICS);
    assert_eq!(Toast::waiting(&tree, toast), Some(true));
    assert!(place(&tree, toast) < place(&tree, b));
    let (c, _, _) = consent(&mut tree, "Allow?");
    let style = *tree.style(c).expect("a style");
    tree.set_style(c, style.hidden()).expect("hidden");
    tree.layout(Size::new(1400.0, 900.0), &METRICS);
    assert_eq!(Toast::waiting(&tree, toast), Some(false));
    assert_eq!(tree.inert(toast), Some(false));
    assert_eq!(tree.overlays(), [b, toast, c]);
    tree.set_style(c, style).expect("shown");
    tree.layout(Size::new(1400.0, 900.0), &METRICS);
    assert_eq!(tree.inert(toast), Some(true), "shown before C opened");
    assert_eq!(Toast::waiting(&tree, toast), Some(false));
    assert_eq!(
        Toast::held(&tree, toast),
        Some(true),
        "inert, its timeout waits"
    );
    // Working the order out again changes nothing.
    let order = tree.overlays();
    tree.invalidate_all();
    tree.layout(Size::new(1400.0, 900.0), &METRICS);
    assert_eq!(tree.overlays(), order);
    assert_eq!(
        tree.frame(Size::new(1400.0, 900.0), &METRICS).map(|_| ()),
        Some(())
    );
    assert_eq!(tree.frame(Size::new(1400.0, 900.0), &METRICS), None, "idle");
}

#[test]
fn a_toast_shown_while_a_dialog_is_hidden_is_live_above_it_once_shown() {
    // D is hidden; the palette opens; a toast shows, waits under the
    // palette, which closes; D shows again: the toast, shown after D
    // opened, is live above it (C3, p8).
    let mut tree = Tree::new();
    let (d, _, _) = consent(&mut tree, "Discard changes?");
    let style = *tree.style(d).expect("a style");
    tree.set_style(d, style.hidden()).expect("hidden");
    let palette = Palette::new("Command palette")
        .open(&mut tree)
        .expect("opened");
    Palette::set_items(&mut tree, palette, commands(10)).expect("items");
    let toast = Toast::new("Saved.").show(&mut tree).expect("shown");
    tree.layout(Size::new(500.0, 320.0), &METRICS);
    assert_eq!(
        Toast::waiting(&tree, toast),
        Some(true),
        "{:?}",
        tree.overlays()
    );
    tree.close_overlay(palette).expect("closed");
    tree.set_style(d, style).expect("shown");
    tree.layout(WINDOW, &METRICS);
    assert_eq!(tree.modal(), Some(d));
    assert_eq!(tree.inert(toast), Some(false));
    assert_eq!(Toast::waiting(&tree, toast), Some(false));
    assert_eq!(Toast::held(&tree, toast), Some(false));
    assert_eq!(tree.overlays(), [d, toast]);
}

#[test]
fn a_toast_waiting_for_a_modal_that_is_hidden_waits_no_more() {
    let small = Size::new(360.0, 220.0);
    let mut tree = Tree::new();
    let (dialog, _, _) = consent(&mut tree, "Discard changes?");
    let toast = Toast::new("Indexing done.")
        .action("Show")
        .show(&mut tree)
        .expect("shown");
    tree.layout(small, &METRICS);
    assert_eq!(Toast::waiting(&tree, toast), Some(true));
    let style = *tree.style(dialog).expect("a style");
    tree.set_style(dialog, style.hidden()).expect("hidden");
    // At once, before any layout, and after one.
    assert_eq!(Toast::waiting(&tree, toast), Some(false));
    assert_eq!(tree.inert(toast), Some(false));
    tree.layout(small, &METRICS);
    assert_eq!(Toast::waiting(&tree, toast), Some(false));
    assert_eq!(Toast::held(&tree, toast), Some(false));
}

#[test]
fn a_toast_keeps_clear_of_what_its_and_the_dialogs_nodes_host() {
    // A tooltip on a live toast's button that would cover the dialog's
    // Cancel makes the toast wait (B3).
    let window = Size::new(500.0, 300.0);
    let mut tree = Tree::new();
    let (dialog, _, cancel) = consent(&mut tree, "Run the build script?");
    let toast = Toast::new("rust-analyzer: indexing failed")
        .action("Open log")
        .show(&mut tree)
        .expect("shown");
    tree.layout(window, &METRICS);
    assert_eq!(Toast::waiting(&tree, toast), Some(false));
    let action = Toast::action_button(&tree, toast).expect("an action");
    tree.set_tooltip(
        action,
        "Opens the log of rust-analyzer\nin a new tab\nnow\nplease",
    )
    .expect("a tooltip");
    let tip = Tooltip::open(&mut tree, action).expect("opened");
    tree.set_overlay_anchor(tip, Anchor::node(action, Side::Above, Align::End, 4.0))
        .expect("anchored");
    tree.layout(window, &METRICS);
    let (over, under) = (bounds(&tree, tip), bounds(&tree, cancel));
    if over.x < under.right()
        && under.x < over.right()
        && over.y < under.bottom()
        && under.y < over.bottom()
    {
        assert_eq!(
            Toast::waiting(&tree, toast),
            Some(true),
            "its tooltip covers Cancel"
        );
        assert_eq!(tree.inert(tip), Some(true));
        assert_eq!(tree.hit_test(center(under)), Some(cancel));
    } else {
        panic!("the tooltip {over:?} does not meet Cancel {under:?}");
    }
    tree.close_overlay(tip).expect("closed");

    // A menu the dialog's node hosts, which a live toast would cover,
    // makes it wait too (B4).
    tree.layout(window, &METRICS);
    assert_eq!(Toast::waiting(&tree, toast), Some(false));
    let menu = tree
        .open_overlay(
            Node::new(Style::column().fixed(160.0, 120.0)),
            OverlayKind::Modeless,
            Anchor::node(cancel, Side::Below, Align::Start, 0.0),
        )
        .expect("a menu");
    tree.layout(window, &METRICS);
    let (menu_at, toast_at) = (bounds(&tree, menu), bounds(&tree, toast));
    assert!(menu_at.y < toast_at.bottom() && toast_at.y < menu_at.bottom());
    assert_eq!(
        Toast::waiting(&tree, toast),
        Some(true),
        "it would cover the menu"
    );
    assert!(place(&tree, toast) < place(&tree, dialog));
}

#[test]
fn an_owner_cannot_anchor_a_toast_elsewhere() {
    let mut tree = Tree::new();
    let (_, allow, _) = consent(&mut tree, "Allow?");
    let toast = Toast::new("Saved.").show(&mut tree).expect("shown");
    let before = tree.overlay_anchor(toast);
    assert_eq!(
        tree.set_overlay_anchor(toast, Anchor::node(allow, Side::Below, Align::Start, 0.0)),
        Err(UiError::FixedAnchor { id: toast })
    );
    assert_eq!(tree.overlay_anchor(toast), before);
    assert_eq!(
        tree.set_overlay_anchor(toast, Anchor::window(Align::Center, Align::Center, 0.0)),
        Err(UiError::FixedAnchor { id: toast })
    );
}

#[test]
fn toasts_never_overlap_and_one_with_no_room_waits_out_of_view() {
    // While one is held, a burst of toasts goes above it until there is no
    // room; the rest wait out of view, held (C4, p5).
    let window = Size::new(800.0, 220.0);
    let mut tree = Tree::new();
    let first = Toast::new("one\ntwo\nthree")
        .action("Show")
        .show(&mut tree)
        .expect("shown");
    tree.layout(window, &METRICS);
    let show = Toast::action_button(&tree, first).expect("an action");
    let on_show = center(bounds(&tree, show));
    arrive(&mut tree, on_show);
    assert_eq!(Toast::held(&tree, first), Some(true));
    let placed = bounds(&tree, first);
    for text in ["a", "b", "c"] {
        Toast::new(text).show(&mut tree).expect("shown");
        tree.layout(window, &METRICS);
    }
    assert_eq!(bounds(&tree, first), placed, "the held one did not move");
    let toasts = Toast::toasts(&tree);
    let shown: Vec<NodeId> = toasts
        .iter()
        .copied()
        .filter(|&toast| Toast::waiting(&tree, toast) == Some(false))
        .collect();
    for (k, &a) in shown.iter().enumerate() {
        for &b in &shown[k + 1..] {
            let (ra, rb) = (bounds(&tree, a), bounds(&tree, b));
            assert!(
                !(ra.y < rb.bottom() && rb.y < ra.bottom()),
                "{a} {ra:?} and {b} {rb:?} overlap"
            );
        }
    }
    let waiting: Vec<NodeId> = toasts
        .iter()
        .copied()
        .filter(|&toast| Toast::waiting(&tree, toast) == Some(true))
        .collect();
    assert!(!waiting.is_empty(), "220 units hold no four toasts");
    for &toast in &waiting {
        assert_eq!(Toast::held(&tree, toast), Some(true));
        assert!(tree.accessibility().get(toast).is_none(), "out of view");
        assert_eq!(tree.bounds(toast).map(|rect| rect.height), Some(0.0));
    }
    // Once none is held, they settle, the newest at the corner, as many as
    // fit.
    tree.handle(&hover(Point::new(5.0, 5.0)));
    tree.layout(window, &METRICS);
    let last = *Toast::toasts(&tree).last().expect("a toast");
    assert_eq!(Toast::waiting(&tree, last), Some(false));
    assert_eq!(bounds(&tree, last).bottom(), window.height - TOAST_MARGIN);
}

#[test]
fn a_new_toast_takes_its_place_before_it_shows_so_none_overlaps() {
    // The pointer rests where a new toast's first place would be: it
    // never shows there, so it is not held there over an older one (p9).
    let mut tree = Tree::new();
    let older = Toast::new("Build finished.")
        .show(&mut tree)
        .expect("shown");
    tree.layout(WINDOW, &METRICS);
    let corner = Point::new(WINDOW.width - 40.0, WINDOW.height - 20.0);
    tree.handle(&hover(corner));
    let newer = Toast::new("one\ntwo\nthree\nfour")
        .show(&mut tree)
        .expect("shown");
    assert_eq!(
        Toast::waiting(&tree, newer),
        Some(true),
        "until a layout places it"
    );
    assert!(tree.accessibility().get(newer).is_none());
    tree.layout(WINDOW, &METRICS);
    let (a, b) = (bounds(&tree, older), bounds(&tree, newer));
    assert!(!(a.y < b.bottom() && b.y < a.bottom()), "{a:?} {b:?}");
    assert_eq!(Toast::waiting(&tree, newer), Some(false));
}

#[test]
fn a_toast_hidden_and_shown_again_while_another_is_held_takes_a_new_place() {
    // C10: shown again while the column is held, it goes above the others.
    let mut tree = Tree::new();
    let a = Toast::new("a").show(&mut tree).expect("shown");
    let b = Toast::new("b")
        .action("Open")
        .show(&mut tree)
        .expect("shown");
    let c = Toast::new("c").show(&mut tree).expect("shown");
    tree.layout(WINDOW, &METRICS);
    let style = *tree.style(b).expect("a style");
    tree.set_style(b, style.hidden()).expect("hidden");
    tree.layout(WINDOW, &METRICS);
    // `a` drops into `b`'s place; the pointer comes to rest on it.
    let on_a = center(bounds(&tree, a));
    arrive(&mut tree, on_a);
    assert_eq!(Toast::held(&tree, a), Some(true));
    tree.set_style(b, style).expect("shown");
    tree.layout(WINDOW, &METRICS);
    let (ra, rb, rc) = (bounds(&tree, a), bounds(&tree, b), bounds(&tree, c));
    for (x, y) in [(ra, rb), (rb, rc), (ra, rc)] {
        assert!(!(x.y < y.bottom() && y.y < x.bottom()), "{x:?} {y:?}");
    }
    assert!(rb.bottom() <= ra.y, "above the held one");
}

#[test]
fn a_double_click_on_a_toasts_close_reaches_no_other_toast() {
    // The first click closes the newest; the column keeps still until the
    // pointer moves past the slop, so the second lands where it was (B6).
    for modal in [false, true] {
        let mut tree = Tree::new();
        if modal {
            consent(&mut tree, "Allow?");
        }
        let older = Toast::new("one")
            .action("Open")
            .show(&mut tree)
            .expect("shown");
        let newer = Toast::new("two")
            .action("Open")
            .show(&mut tree)
            .expect("shown");
        tree.layout(WINDOW, &METRICS);
        let close = Toast::close_button(&tree, newer).expect("a close");
        let at = center(bounds(&tree, close));
        arrive(&mut tree, at);
        tree.handle(&press(at));
        let up = tree.handle(&release(at));
        assert_eq!(kinds(&up), [(close, ActionKind::Dismiss)]);
        let placed = bounds(&tree, older);
        Toast::close(&mut tree, newer).expect("closed");
        tree.layout(WINDOW, &METRICS);
        assert_eq!(
            bounds(&tree, older),
            placed,
            "modal {modal}: it did not slide"
        );
        let hit = tree.hit_test(at);
        assert!(
            hit.is_none_or(|hit| Toast::of(&tree, hit).is_none()),
            "modal {modal}"
        );
        tree.handle(&press(at));
        let up = tree.handle(&release(at));
        assert!(up.actions.is_empty(), "modal {modal}: {up:?}");
        // Past the slop, the column settles.
        tree.handle(&hover(Point::new(at.x - 10.0, at.y)));
        tree.layout(WINDOW, &METRICS);
        assert_eq!(bounds(&tree, older).bottom(), WINDOW.height - TOAST_MARGIN);
    }
}

/// Each draw command of `commands` from `from` on, with the clip in force
/// for it, cut to it.
fn clipped_from(commands: &[Command], from: usize, window: Rect) -> Vec<Rect> {
    let mut clips = vec![window];
    let mut out = Vec::new();
    for (at, command) in commands.iter().enumerate() {
        let top = *clips.last().expect("the window");
        match command {
            Command::PushClip { rect } => {
                let x = rect.x.max(top.x);
                let y = rect.y.max(top.y);
                let right = rect.right().min(top.right());
                let bottom = rect.bottom().min(top.bottom());
                clips.push(Rect::new(x, y, (right - x).max(0.0), (bottom - y).max(0.0)));
            }
            Command::PopClip => {
                clips.pop();
            }
            Command::PushLayer { .. } | Command::PopLayer => {}
            other if at >= from => {
                if let Some(rect) = other.bounds(&METRICS) {
                    let x = rect.x.max(top.x);
                    let y = rect.y.max(top.y);
                    let right = rect.right().min(top.right());
                    let bottom = rect.bottom().min(top.bottom());
                    if right > x && bottom > y {
                        out.push(Rect::new(x, y, right - x, bottom - y));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

#[test]
fn a_toast_with_a_4_kib_action_label_paints_nothing_outside_it() {
    // Beside a dialog it keeps clear of, its action's label, 4 KiB of
    // words and spaces, paints only inside the toast (B1).
    let window = Size::new(1400.0, 250.0);
    let mut tree = Tree::new();
    consent(&mut tree, "Run the build script?");
    let label = format!("CANCEL IS UNSAFE - PRESS ALLOW{}", " ".repeat(4096));
    let toast = Toast::new("rust-analyzer")
        .action(&label)
        .show(&mut tree)
        .expect("shown");
    tree.layout(window, &METRICS);
    assert_eq!(Toast::waiting(&tree, toast), Some(false));
    tree.invalidate_all();
    let frame = tree.frame(window, &METRICS).expect("a frame");
    let commands = frame.list.commands();
    let placed = bounds(&tree, toast);
    let from = commands
        .iter()
        .position(|command| matches!(command, Command::Rect { rect, .. } if *rect == placed))
        .expect("the toast's fill");
    let whole = Rect::new(0.0, 0.0, window.width, window.height);
    let inside = |rect: Rect| {
        rect.x >= placed.x - 1e-3
            && rect.y >= placed.y - 1e-3
            && rect.right() <= placed.right() + 1e-3
            && rect.bottom() <= placed.bottom() + 1e-3
    };
    for rect in clipped_from(commands, from, whole) {
        assert!(inside(rect), "{rect:?} outside the toast {placed:?}");
    }
    // Whatever a node its owner puts in it paints, it paints nothing
    // outside the toast either: the toast cuts what it holds.
    tree.insert(
        toast,
        Node::new(Style::default().fixed(20.0, 10.0)).with_widget(Spill),
    )
    .expect("a node");
    tree.invalidate_all();
    let frame = tree.frame(window, &METRICS).expect("a frame");
    let commands = frame.list.commands();
    let placed = bounds(&tree, toast);
    let from = commands
        .iter()
        .position(|command| matches!(command, Command::Rect { rect, .. } if *rect == placed))
        .expect("the toast's fill");
    assert!(commands.iter().any(
        |command| matches!(command, Command::Rect { color: ink, .. } if *ink == color(Token::Amber))
    ));
    for rect in clipped_from(commands, from, whole) {
        assert!(
            rect.x >= placed.x - 1e-3
                && rect.y >= placed.y - 1e-3
                && rect.right() <= placed.right() + 1e-3
                && rect.bottom() <= placed.bottom() + 1e-3,
            "{rect:?} outside the toast {placed:?}"
        );
    }
}

/// Paints an `amber` rect far wider than its bounds.
struct Spill;

impl tidepool_ui::Widget for Spill {
    fn paint(
        &self,
        cx: &tidepool_ui::draw::PaintContext<'_>,
        list: &mut tidepool_ui::draw::DrawList,
    ) {
        let bounds = cx.bounds();
        list.rect(
            Rect::new(bounds.x - 600.0, bounds.y, 1200.0, bounds.height),
            cx.theme().color(Token::Amber),
        );
    }
}

#[test]
fn toasts_settle_in_one_layout_when_the_window_changes_under_a_resting_pointer() {
    // The pointer rests where, once the window grows, the newest toast
    // comes: one layout settles them, and the next frame draws nothing.
    let mut tree = Tree::new();
    Toast::new("toast 0").show(&mut tree).expect("shown");
    Toast::new("toast 1")
        .action("Go")
        .show(&mut tree)
        .expect("shown");
    Toast::new("toast 2").show(&mut tree).expect("shown");
    tree.frame(Size::new(400.0, 300.0), &METRICS);
    tree.handle(&hover(Point::new(228.0, 141.25)));
    tree.frame(Size::new(400.0, 300.0), &METRICS);
    tree.frame(Size::new(120.0, 60.0), &METRICS);
    Toast::new("toast 5\nsecond line")
        .show(&mut tree)
        .expect("shown");
    tree.frame(Size::new(120.0, 60.0), &METRICS);
    let grown = Size::new(360.0, 200.0);
    tree.frame(grown, &METRICS);
    let order = tree.overlays();
    assert_eq!(tree.frame(grown, &METRICS), None, "settled in one layout");
    assert_eq!(tree.overlays(), order);
}

// ---------- Round four: focus, the palette, tooltips and text ----------

#[test]
fn a_toast_forgets_a_dialogs_node_once_focus_leaves_it() {
    // C8: handed Save when the dialog closed, the toast forgets it once
    // focus moves elsewhere; focus that comes back later and closes it
    // goes back to no old node.
    let Bar {
        mut tree,
        save,
        run,
        ..
    } = bar();
    tree.set_focus(save, true).expect("focused");
    let dialog = Modal::new("Discard changes?")
        .open(&mut tree)
        .expect("opened");
    let toast = Toast::new("Saved.")
        .action("Show")
        .show(&mut tree)
        .expect("shown");
    tree.layout(WINDOW, &METRICS);
    let action = Toast::action_button(&tree, toast).expect("an action");
    tree.set_focus(action, true).expect("focused");
    tree.close_overlay(dialog).expect("closed");
    // Within the toast, it keeps it.
    let close = Toast::close_button(&tree, toast).expect("a close");
    tree.set_focus(close, true).expect("focused");
    tree.set_focus(run, true).expect("focused");
    tree.set_focus(action, true).expect("back on the toast");
    Toast::close(&mut tree, toast).expect("closed");
    assert_eq!(tree.focused(), None, "not Save, long after");
}

#[test]
fn a_key_right_after_a_press_on_an_item_moves_on_from_it() {
    // C7: a press on item 3, then Down in the same batch, with no layout
    // between: item 4.
    let (mut tree, palette, _) = palette(10);
    let at = center(bounds(&tree, item(&tree, "Command 3")));
    arrive(&mut tree, at);
    let down = tree.handle(&press(at));
    assert_eq!(kinds(&down), [(palette, ActionKind::Select)]);
    tree.handle(&release(Point::new(5.0, 590.0)));
    assert_eq!(Palette::selected(&tree, palette), Some(3));
    let response = tree.handle(&key(Key::Down));
    assert_eq!(kinds(&response), [(palette, ActionKind::Select)]);
    assert_eq!(Palette::selected(&tree, palette), Some(4));
    tree.frame(WINDOW, &METRICS);
    assert_eq!(
        Palette::selected(&tree, palette),
        Some(4),
        "kept after the layout"
    );
}

#[test]
fn a_range_at_or_past_the_end_of_a_label_with_a_hidden_character_marks_nothing() {
    // C9.
    let (mut tree, palette, _) = palette(0);
    let label = "abc\u{202E}d";
    let rows = vec![
        Row::new(label).matched(Some(label.len()..label.len() + 4)),
        Row::new(label).matched(Some(label.len() + 2..label.len() + 9)),
        Row::new("plain").matched(Some(5..9)),
    ];
    Palette::set_items(&mut tree, palette, rows).expect("items");
    let commands = painted(&mut tree);
    let marked: Vec<&str> = commands
        .iter()
        .filter_map(|command| match command {
            Command::Text {
                text, color: ink, ..
            } if *ink == color(Token::TealHi) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(marked.is_empty(), "{marked:?}");
}

#[test]
fn a_tooltip_lets_the_pointer_through_only_where_its_node_shows() {
    // C2: its node is scrolled mostly out of view; over the part that is
    // cut off, the tooltip takes the press, and Delete under it gets
    // nothing.
    let window = Size::new(400.0, 220.0);
    let mut tree = Tree::new();
    let root = tree.root();
    tree.set_style(root, Style::column()).expect("a column");
    let scroller = tree
        .insert(
            root,
            Node::new(Style::column().fixed(200.0, 80.0).scroll(false, true)),
        )
        .expect("a scroller");
    let long = tree
        .insert(
            scroller,
            Node::new(Style::default().fixed(150.0, 500.0))
                .with_semantics(tidepool_ui::a11y::Semantics::new(Role::Button, "Outline"))
                .focusable(),
        )
        .expect("a node");
    tree.set_tooltip(long, "The document's outline")
        .expect("a tooltip");
    tree.insert(root, Node::new(Style::default().fixed(10.0, 110.0)))
        .expect("a gap");
    let delete = tree
        .insert(root, Button::new("Delete", ButtonKind::Amber).node())
        .expect("a button");
    tree.set_scroll_offset(scroller, Point::new(0.0, 200.0))
        .expect("scrolled");
    tree.frame(window, &METRICS);
    let tip = Tooltip::open(&mut tree, long).expect("opened");
    tree.frame(window, &METRICS);
    let (over, under, node) = (
        bounds(&tree, tip),
        bounds(&tree, delete),
        bounds(&tree, long),
    );
    let on = Point::new(
        over.x + 10.0,
        (over.y.max(under.y) + over.bottom().min(under.bottom())) / 2.0,
    );
    assert!(over.contains(on) && under.contains(on) && node.contains(on));
    assert_eq!(tree.hit_test(on), Some(tip));
    arrive(&mut tree, on);
    tree.handle(&press(on));
    let up = tree.handle(&release(on));
    assert!(up.actions.is_empty(), "{up:?}");
    // Where its node shows, the pointer reaches the node.
    let shown = Point::new(node.x + 5.0, 40.0);
    if over.contains(shown) {
        assert_eq!(tree.hit_test(shown), Some(long));
    }
}

#[test]
fn a_field_keeps_no_bidi_or_invisible_character_typed_or_pasted() {
    // B7: the palette's query and its combo box's value.
    let (mut tree, palette, field) = palette(0);
    tree.handle(&InputEvent::Text(
        "ab\u{202E}cd\u{200B}e\u{2066}f\u{FEFF}\u{3164}g\u{E0041}\u{2028}h".to_string(),
    ));
    assert_eq!(Palette::query(&tree, palette), Some("abcdefg h"));
    let snapshot = tree.accessibility();
    assert_eq!(
        snapshot
            .get(field)
            .and_then(|node| node.value.clone())
            .as_deref(),
        Some("abcdefg h")
    );
    // Emoji joiners and presentation stay.
    let mut input = TextInput::new("Name");
    input.set_text("\u{1F469}\u{200D}\u{1F467}\u{2764}\u{FE0F}");
    assert_eq!(input.text(), "\u{1F469}\u{200D}\u{1F467}\u{2764}\u{FE0F}");
}

#[test]
fn a_toast_that_grows_while_held_pushes_the_ones_above_it_up() {
    // The held one keeps its place; the one above it moves up rather than
    // be overlapped (C4).
    let mut tree = Tree::new();
    let older = Toast::new("older").show(&mut tree).expect("shown");
    let newer = Toast::new("newer")
        .action("Go")
        .show(&mut tree)
        .expect("shown");
    tree.layout(WINDOW, &METRICS);
    let go = Toast::action_button(&tree, newer).expect("an action");
    let at = center(bounds(&tree, go));
    arrive(&mut tree, at);
    tree.handle(&press(at));
    assert_eq!(Toast::held(&tree, newer), Some(true), "pressed");
    let placed = bounds(&tree, newer);
    // Its owner adds a node to it, which makes it taller.
    tree.insert(newer, Node::new(Style::default().fixed(20.0, 60.0)))
        .expect("a node");
    tree.layout(WINDOW, &METRICS);
    let (held, above) = (bounds(&tree, newer), bounds(&tree, older));
    assert_eq!(
        held.bottom(),
        placed.bottom(),
        "it grew up, from where it stood"
    );
    assert!(
        above.bottom() + TOAST_GAP <= held.y + 1e-3,
        "{above:?} over {held:?}"
    );
    tree.handle(&release(at));
}

#[test]
fn the_column_keeps_still_under_the_pointer_even_over_an_inert_toast() {
    // Where the pointer is, not what it hovers: over a toast inert under a
    // dialog, the column keeps still, so that a toast starting or stopping
    // to wait, which changes what is hovered, moves no other.
    let mut tree = Tree::new();
    let before = Toast::new("Saved.").show(&mut tree).expect("shown");
    consent(&mut tree, "Allow?");
    tree.layout(WINDOW, &METRICS);
    assert_eq!(tree.inert(before), Some(true));
    let placed = bounds(&tree, before);
    arrive(&mut tree, center(placed));
    assert_eq!(tree.state(before).map(|state| state.hovered), Some(false));
    let after = Toast::new("Indexing done.").show(&mut tree).expect("shown");
    tree.layout(WINDOW, &METRICS);
    assert_eq!(bounds(&tree, before), placed, "it kept still");
    assert!(bounds(&tree, after).bottom() + TOAST_GAP <= placed.y + 1e-3);
    // Away from it, they settle: the newest at the corner.
    tree.handle(&hover(Point::new(5.0, 5.0)));
    tree.layout(WINDOW, &METRICS);
    assert_eq!(bounds(&tree, after).bottom(), WINDOW.height - TOAST_MARGIN);
}

// ---------- The flags of England, Scotland and Wales ----------

/// The three RGI emoji tag sequences: a black flag, the tags of a
/// subdivision code, and the cancel tag.
const FLAGS: [&str; 3] = [
    "\u{1F3F4}\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F}",
    "\u{1F3F4}\u{E0067}\u{E0062}\u{E0073}\u{E0063}\u{E0074}\u{E007F}",
    "\u{1F3F4}\u{E0067}\u{E0062}\u{E0077}\u{E006C}\u{E0073}\u{E007F}",
];

/// `text` spelled in tag characters.
fn tags(text: &str) -> String {
    text.chars()
        .map(|c| char::from_u32(0xE0000 + u32::from(c)).expect("a tag"))
        .collect()
}

/// Strings with tag characters that are no RGI flag, each with what a
/// dialog keeps of it.
fn not_flags() -> Vec<(String, String)> {
    let cancel = '\u{E007F}';
    vec![
        // Well formed, but not recommended: California.
        (
            format!("\u{1F3F4}{}{cancel}", tags("usca")),
            "\u{1F3F4}".to_string(),
        ),
        // A lone tag.
        (tags("g"), String::new()),
        // England's tags, with no cancel tag.
        (
            format!("\u{1F3F4}{}", tags("gbeng")),
            "\u{1F3F4}".to_string(),
        ),
        // England's tags after another base.
        (
            format!("\u{1F600}{}{cancel}", tags("gbeng")),
            "\u{1F600}".to_string(),
        ),
        // Hidden ASCII riding in tags.
        (
            format!("report{}{cancel}", tags("evil.exe")),
            "report".to_string(),
        ),
    ]
}

#[test]
fn the_three_flags_survive_every_cleaner() {
    for flag in FLAGS {
        let text = format!("{flag} build");
        assert_eq!(Modal::new(&text).title(), text);
        assert_eq!(Modal::new("t").text(&text).lines(), [text.as_str()]);
        assert_eq!(Toast::new(&text).lines(), [text.as_str()]);
        assert_eq!(
            Toast::new("t")
                .action(&text)
                .show(&mut Tree::new())
                .map(|_| ()),
            Ok(())
        );
        assert_eq!(Palette::new(&text).name(), text);
        let Bar { mut tree, save, .. } = bar();
        tree.set_tooltip(save, &text).expect("a tooltip");
        assert_eq!(tree.tooltip(save), Some(text.as_str()));
        // A row: its drawn text and its name.
        let (mut tree, palette, field) = palette(0);
        Palette::set_items(&mut tree, palette, vec![Row::new(text.as_str())]).expect("items");
        tree.frame(WINDOW, &METRICS);
        assert_eq!(rows(&tree), [(text.clone(), true)]);
        let commands = painted(&mut tree);
        assert!(commands.iter().any(|command| matches!(
            command,
            Command::Text { text: drawn, .. } if *drawn == text
        )));
        // Typed or pasted whole into a field.
        tree.handle(&InputEvent::Text(flag.to_string()));
        assert_eq!(Palette::query(&tree, palette), Some(flag));
        let _ = field;
    }
}

#[test]
fn tags_that_are_no_flag_are_still_left_out() {
    for (text, kept) in not_flags() {
        assert_eq!(Modal::new(&text).title(), kept, "{text:?}");
        assert_eq!(Palette::new(format!("p{text}")).name(), format!("p{kept}"));
        let lines = Toast::new(format!("t{text}")).lines().to_vec();
        assert_eq!(lines, [format!("t{kept}")]);
        let Bar { mut tree, save, .. } = bar();
        tree.set_tooltip(save, format!("x{text}"))
            .expect("a tooltip");
        assert_eq!(tree.tooltip(save), Some(format!("x{kept}").as_str()));
        // A field drops them too.
        let (mut tree, palette, _) = palette(0);
        tree.handle(&InputEvent::Text(format!("q{text}")));
        assert_eq!(
            Palette::query(&tree, palette),
            Some(format!("q{kept}").as_str())
        );
        // A row shows each tag as U+FFFD, its name too.
        Palette::set_items(&mut tree, palette, vec![Row::new(text.as_str())]).expect("items");
        tree.frame(WINDOW, &METRICS);
        let tagged = text
            .chars()
            .filter(|&c| ('\u{E0000}'..='\u{E0FFF}').contains(&c))
            .count();
        let shown: String = text
            .chars()
            .map(|c| {
                if ('\u{E0000}'..='\u{E0FFF}').contains(&c) {
                    '\u{FFFD}'
                } else {
                    c
                }
            })
            .collect();
        assert_eq!(rows(&tree)[0].0, shown, "{text:?}");
        assert_eq!(shown.matches('\u{FFFD}').count(), tagged);
        let commands = painted(&mut tree);
        for command in commands {
            if let Command::Text { text: drawn, .. } = command {
                assert!(
                    !drawn
                        .chars()
                        .any(|c| ('\u{E0000}'..='\u{E0FFF}').contains(&c)),
                    "{drawn:?}"
                );
            }
        }
    }
    // Tags arriving one by one in a field are each dropped.
    let (mut tree, palette, _) = palette(0);
    for piece in FLAGS[0].chars() {
        tree.handle(&InputEvent::Text(piece.to_string()));
    }
    assert_eq!(Palette::query(&tree, palette), Some("\u{1F3F4}"));
}

#[test]
fn a_matched_range_after_a_flag_marks_the_right_characters() {
    let (mut tree, palette, _) = palette(0);
    let england = FLAGS[0];
    let wales = FLAGS[2];
    let first = format!("{england} main.rs");
    // ESC, shown as three bytes, before the flag shifts what follows.
    let second = format!("\u{1b}{wales}x.rs");
    let at = first.find("main").expect("main");
    let x = second.find('x').expect("x");
    let items = vec![
        Row::new(first.as_str()).matched(Some(at..at + 4)),
        Row::new(second.as_str()).matched(Some(x..x + 1)),
        // The flag itself, matched.
        Row::new(format!("{england}.md")).matched(Some(0..england.len())),
    ];
    Palette::set_items(&mut tree, palette, items).expect("items");
    let commands = painted(&mut tree);
    let marked: Vec<&str> = commands
        .iter()
        .filter_map(|command| match command {
            Command::Text {
                text, color: ink, ..
            } if *ink == color(Token::TealHi) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(marked, ["main", "x", england]);
}
