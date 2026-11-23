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

//! The shell's layout and behavior, headless (M3.4, ADR-0060): its areas
//! at the window's sizes, its panels hidden and shown, the activity bar,
//! the palette and its ranking, the shortcuts, the tooltip's timer, the
//! accessibility tree, and an idle shell drawing nothing; and its keymap
//! (M4.6, ADR-0064): the 14 commands of its registry in the palette, a
//! keymap from its owner in place of the default one, and keys that a
//! widget handled or a modal blocked never reaching it.

use std::time::{Duration, Instant};

use tidepool_codec::toml;
use tidepool_core::geom::{Point, Rect, Size};
use tidepool_core::input::{InputEvent, Key, Modifiers, MouseButton};
use tidepool_editor::keymap::{Keymap, Resolution};
use tidepool_editor::shell::parts::Label;
use tidepool_editor::shell::{
    ACTIVITY_WIDTH, Area, BOTTOM_MIN_HEIGHT, Command, EDITOR_MIN_HEIGHT, Flow, MAIN_MIN_WIDTH,
    PanelTab, Panels, SIDE_MIN_WIDTH, STATUS_HEIGHT, Shell, TITLE_HEIGHT, TOOLTIP_DELAY, View,
};
use tidepool_ui::a11y::Role;
use tidepool_ui::metrics::FixedMetrics;
use tidepool_ui::widgets::{Collection, Palette, SectionTitle};
use tidepool_ui::{NodeId, Tree};

const SIZES: [Size; 3] = [
    Size::new(800.0, 600.0),
    Size::new(1280.0, 800.0),
    Size::new(1920.0, 1080.0),
];

fn shell_in(size: Size, panels: Panels) -> (Tree, Shell) {
    let mut tree = Tree::new();
    let shell = Shell::new(&mut tree, panels).expect("the shell builds");
    assert!(tree.frame(size, &FixedMetrics::DEFAULT).is_some());
    (tree, shell)
}

fn bounds(tree: &Tree, id: NodeId) -> Rect {
    tree.bounds(id).expect("laid out")
}

fn key(key: Key, modifiers: Modifiers) -> InputEvent {
    InputEvent::KeyDown {
        key,
        modifiers,
        repeat: false,
    }
}

fn ctrl(k: Key) -> InputEvent {
    key(k, Modifiers::CTRL)
}

fn ctrl_shift(k: Key) -> InputEvent {
    key(k, Modifiers::CTRL.union(Modifiers::SHIFT))
}

fn text(text: &str) -> InputEvent {
    InputEvent::Text(text.to_string())
}

fn move_to(point: Point) -> InputEvent {
    InputEvent::MouseMove {
        position: point,
        modifiers: Modifiers::NONE,
    }
}

fn center(rect: Rect) -> Point {
    Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
}

/// Clicks the middle of `id`.
fn click(tree: &mut Tree, shell: &mut Shell, id: NodeId, now: Instant) -> Flow {
    let at = center(bounds(tree, id));
    let mut flow = Flow::Continue;
    for event in [
        move_to(at),
        InputEvent::MouseDown {
            button: MouseButton::Left,
            position: at,
            modifiers: Modifiers::NONE,
        },
        InputEvent::MouseUp {
            button: MouseButton::Left,
            position: at,
            modifiers: Modifiers::NONE,
        },
    ] {
        if shell.handle(tree, &event, now).expect("handled") == Flow::Quit {
            flow = Flow::Quit;
        }
    }
    flow
}

/// Sends `events` at `now`, laying out after each, as the loop does.
fn send(
    tree: &mut Tree,
    shell: &mut Shell,
    size: Size,
    events: &[InputEvent],
    now: Instant,
) -> Flow {
    let mut flow = Flow::Continue;
    for event in events {
        if shell.handle(tree, event, now).expect("handled") == Flow::Quit {
            flow = Flow::Quit;
        }
        let _ = tree.frame(size, &FixedMetrics::DEFAULT);
    }
    flow
}

fn side_title(tree: &Tree, shell: &Shell) -> String {
    let side = shell.area(Area::SidePanel);
    let first = tree.children(side).expect("children")[0];
    tree.widget::<SectionTitle>(first)
        .expect("a title")
        .text()
        .to_string()
}

fn side_text(tree: &Tree, shell: &Shell) -> String {
    let side = shell.area(Area::SidePanel);
    let second = tree.children(side).expect("children")[1];
    tree.widget::<Label>(second)
        .expect("a label")
        .text()
        .to_string()
}

fn within(inner: Rect, outer: Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.x + inner.width <= outer.x + outer.width + 0.001
        && inner.y + inner.height <= outer.y + outer.height + 0.001
}

#[test]
fn the_areas_tile_the_window_at_each_size() {
    for size in SIZES {
        let (tree, shell) = shell_in(size, Panels::default());
        let window = Rect::new(0.0, 0.0, size.width, size.height);
        let activity = bounds(&tree, shell.area(Area::ActivityBar));
        let side = bounds(&tree, shell.area(Area::SidePanel));
        let editor = bounds(&tree, shell.area(Area::Editor));
        let bottom = bounds(&tree, shell.area(Area::BottomPanel));
        let status = bounds(&tree, shell.area(Area::StatusBar));
        for area in [activity, side, editor, bottom, status] {
            assert!(within(area, window), "{size:?}: {area:?}");
        }
        // The title strip above, the status bar below, at their heights.
        assert_eq!(activity.y, TITLE_HEIGHT, "{size:?}");
        assert_eq!(status.height, STATUS_HEIGHT);
        assert_eq!(status.y + status.height, size.height);
        assert_eq!(activity.y + activity.height, status.y);
        // The bar, the side panel and the rest, left to right.
        assert_eq!((activity.x, activity.width), (0.0, ACTIVITY_WIDTH));
        assert!(
            side.x >= ACTIVITY_WIDTH && side.width >= SIDE_MIN_WIDTH,
            "{size:?}: {side:?}"
        );
        assert!(editor.x > side.x + side.width, "a divider between them");
        assert!(editor.width >= MAIN_MIN_WIDTH);
        assert!((editor.x + editor.width - size.width).abs() < 0.001);
        // The editor area above the bottom panel.
        assert_eq!(editor.width, bottom.width);
        assert!(
            bottom.y > editor.y + editor.height,
            "a divider between them"
        );
        assert!(editor.height >= EDITOR_MIN_HEIGHT && bottom.height >= BOTTOM_MIN_HEIGHT);
        assert!((bottom.y + bottom.height - status.y).abs() < 0.001);
    }
}

#[test]
fn a_window_smaller_than_the_shell_keeps_every_area_inside_it() {
    for size in [
        Size::new(320.0, 240.0),
        Size::new(1.0, 1.0),
        Size::new(0.0, 0.0),
    ] {
        // A window with no area has nothing to draw, so it is laid out
        // without asking for a frame.
        let mut tree = Tree::new();
        let shell = Shell::new(&mut tree, Panels::default()).expect("the shell builds");
        tree.layout(size, &FixedMetrics::DEFAULT);
        let window = Rect::new(0.0, 0.0, size.width.max(1.0), size.height.max(1.0));
        for area in [
            Area::ActivityBar,
            Area::SidePanel,
            Area::Editor,
            Area::BottomPanel,
            Area::StatusBar,
        ] {
            let rect = bounds(&tree, shell.area(area));
            assert!(
                within(rect, window) || rect.width == 0.0 || rect.height == 0.0,
                "{size:?} {area:?}: {rect:?}"
            );
        }
    }
}

#[test]
fn hidden_panels_give_their_space_to_the_rest() {
    let size = Size::new(1280.0, 800.0);
    let (tree, shell) = shell_in(
        size,
        Panels {
            side: false,
            bottom: false,
        },
    );
    assert_eq!(
        shell.panels(),
        Panels {
            side: false,
            bottom: false
        }
    );
    let side = bounds(&tree, shell.area(Area::SidePanel));
    let bottom = bounds(&tree, shell.area(Area::BottomPanel));
    let editor = bounds(&tree, shell.area(Area::Editor));
    assert_eq!((side.width, bottom.height), (0.0, 0.0), "hidden");
    assert_eq!(editor.x, ACTIVITY_WIDTH);
    assert_eq!(editor.width, size.width - ACTIVITY_WIDTH);
    assert_eq!(editor.height, size.height - TITLE_HEIGHT - STATUS_HEIGHT);
}

#[test]
fn ctrl_b_and_ctrl_j_toggle_the_panels() {
    let size = Size::new(1280.0, 800.0);
    let (mut tree, mut shell) = shell_in(size, Panels::default());
    let now = Instant::now();
    send(&mut tree, &mut shell, size, &[ctrl(Key::B)], now);
    assert!(!shell.panels().side);
    assert_eq!(bounds(&tree, shell.area(Area::Editor)).x, ACTIVITY_WIDTH);
    send(&mut tree, &mut shell, size, &[ctrl(Key::J)], now);
    assert!(!shell.panels().bottom);
    send(
        &mut tree,
        &mut shell,
        size,
        &[ctrl(Key::B), ctrl(Key::J)],
        now,
    );
    assert_eq!(shell.panels(), Panels::default());
    let side = bounds(&tree, shell.area(Area::SidePanel));
    assert!(
        side.width >= SIDE_MIN_WIDTH,
        "shown again at its width: {side:?}"
    );
    // Only those exact keys: no Shift, no repeat.
    send(&mut tree, &mut shell, size, &[ctrl_shift(Key::B)], now);
    let repeat = InputEvent::KeyDown {
        key: Key::B,
        modifiers: Modifiers::CTRL,
        repeat: true,
    };
    send(&mut tree, &mut shell, size, &[repeat], now);
    assert_eq!(shell.panels(), Panels::default());
}

#[test]
fn the_activity_bar_switches_views_and_hides_the_shown_one() {
    let size = Size::new(1280.0, 800.0);
    let (mut tree, mut shell) = shell_in(size, Panels::default());
    let now = Instant::now();
    assert_eq!(
        (shell.view(), side_title(&tree, &shell)),
        (View::Explorer, "EXPLORER".into())
    );
    let search = shell.button(View::Search);
    click(&mut tree, &mut shell, search, now);
    let _ = tree.frame(size, &FixedMetrics::DEFAULT);
    assert_eq!(shell.view(), View::Search);
    assert_eq!(side_title(&tree, &shell), "SEARCH");
    assert_eq!(side_text(&tree, &shell), View::Search.empty_text());
    // The active view's button is the selected tab.
    let snapshot = tree.accessibility();
    let tab = |name: &str| {
        snapshot
            .nodes()
            .iter()
            .find(|node| node.role == Role::Tab && node.name == name)
            .expect("a tab")
            .selected
    };
    assert!(tab("Search") && !tab("Explorer"));
    // Its view shown already: the side panel hides, and shows again.
    click(&mut tree, &mut shell, search, now);
    let _ = tree.frame(size, &FixedMetrics::DEFAULT);
    assert!(!shell.panels().side);
    click(&mut tree, &mut shell, search, now);
    let _ = tree.frame(size, &FixedMetrics::DEFAULT);
    assert!(shell.panels().side && shell.view() == View::Search);
    // Enter on a focused button shows its view too.
    tree.set_focus(shell.button(View::Agents), true)
        .expect("focusable");
    send(
        &mut tree,
        &mut shell,
        size,
        &[key(Key::Enter, Modifiers::NONE)],
        now,
    );
    assert_eq!(shell.view(), View::Agents);
}

#[test]
fn the_palette_ranks_the_commands_as_the_query_changes_and_runs_the_first() {
    let size = Size::new(1280.0, 800.0);
    let (mut tree, mut shell) = shell_in(size, Panels::default());
    let now = Instant::now();
    send(&mut tree, &mut shell, size, &[ctrl_shift(Key::P)], now);
    let palette = shell.palette().expect("open");
    assert_eq!(tree.modal(), Some(palette));
    assert_eq!(Palette::query(&tree, palette), Some(""));
    send(
        &mut tree,
        &mut shell,
        size,
        &[text("t"), text("s"), text("p")],
        now,
    );
    assert_eq!(Palette::query(&tree, palette), Some("tsp"));
    assert_eq!(Palette::selected(&tree, palette), Some(0));
    // Shortcuts are blocked while it is open.
    send(&mut tree, &mut shell, size, &[ctrl(Key::J)], now);
    assert!(shell.panels().bottom);
    // Enter runs the first-ranked command, and closes the palette.
    send(
        &mut tree,
        &mut shell,
        size,
        &[key(Key::Enter, Modifiers::NONE)],
        now,
    );
    assert_eq!(shell.palette(), None);
    assert_eq!(tree.modal(), None);
    assert!(!shell.panels().side, "View: Toggle Side Panel ran");
}

#[test]
fn escape_closes_the_palette_and_f1_opens_it() {
    let size = Size::new(1280.0, 800.0);
    let (mut tree, mut shell) = shell_in(size, Panels::default());
    let now = Instant::now();
    send(
        &mut tree,
        &mut shell,
        size,
        &[key(Key::F1, Modifiers::NONE)],
        now,
    );
    assert!(shell.palette().is_some());
    // A second opening keeps the one open.
    let open = shell.palette();
    shell.open_palette(&mut tree).expect("opens");
    assert_eq!(shell.palette(), open);
    send(
        &mut tree,
        &mut shell,
        size,
        &[key(Key::Escape, Modifiers::NONE)],
        now,
    );
    assert_eq!(shell.palette(), None);
    assert_eq!(shell.panels(), Panels::default(), "nothing ran");
}

#[test]
fn the_palette_runs_a_tab_and_closes_the_window() {
    let size = Size::new(1280.0, 800.0);
    let (mut tree, mut shell) = shell_in(
        size,
        Panels {
            side: true,
            bottom: false,
        },
    );
    let now = Instant::now();
    let events: Vec<InputEvent> = [ctrl_shift(Key::P)]
        .into_iter()
        .chain("terminal".chars().map(|c| text(&c.to_string())))
        .chain([key(Key::Enter, Modifiers::NONE)])
        .collect();
    send(&mut tree, &mut shell, size, &events, now);
    assert_eq!(shell.tab(), PanelTab::Terminal);
    assert!(shell.panels().bottom, "showing a tab shows the panel");
    let events: Vec<InputEvent> = [ctrl_shift(Key::P)]
        .into_iter()
        .chain("wclose".chars().map(|c| text(&c.to_string())))
        .collect();
    send(&mut tree, &mut shell, size, &events, now);
    let flow = send(
        &mut tree,
        &mut shell,
        size,
        &[key(Key::Enter, Modifiers::NONE)],
        now,
    );
    assert_eq!(flow, Flow::Quit);
}

#[test]
fn every_command_runs_from_the_shell() {
    let size = Size::new(1280.0, 800.0);
    let (mut tree, mut shell) = shell_in(size, Panels::default());
    for command in Command::ALL {
        let flow = shell.run(&mut tree, command).expect("runs");
        let _ = tree.frame(size, &FixedMetrics::DEFAULT);
        match command {
            Command::Show(view) => assert!(shell.view() == view && shell.panels().side),
            Command::ShowTab(tab) => assert!(shell.tab() == tab && shell.panels().bottom),
            Command::CloseWindow => assert_eq!(flow, Flow::Quit),
            Command::OpenPalette => assert!(shell.palette().is_some()),
            Command::ToggleSidePanel | Command::ToggleBottomPanel => {}
        }
    }
}

#[test]
fn a_tooltip_shows_after_the_delay_for_the_pointer_and_at_once_for_focus() {
    let size = Size::new(1280.0, 800.0);
    let (mut tree, mut shell) = shell_in(size, Panels::default());
    let start = Instant::now();
    let git = shell.button(View::SourceControl);
    let on_git = move_to(center(bounds(&tree, git)));
    send(&mut tree, &mut shell, size, &[on_git], start);
    assert_eq!(shell.next_deadline(), Some(start + TOOLTIP_DELAY));
    assert!(
        !shell
            .update(&mut tree, start + TOOLTIP_DELAY - Duration::from_millis(1))
            .unwrap()
    );
    assert_eq!(shell.tooltip(), None);
    assert!(
        shell.update(&mut tree, start + TOOLTIP_DELAY).unwrap(),
        "it opened"
    );
    let tip = shell.tooltip().expect("open");
    assert_eq!(shell.next_deadline(), None);
    let _ = tree.frame(size, &FixedMetrics::DEFAULT);
    // The pointer leaves: it closes.
    let later = start + Duration::from_secs(1);
    send(
        &mut tree,
        &mut shell,
        size,
        &[move_to(Point::new(800.0, 300.0))],
        later,
    );
    assert_eq!(shell.tooltip(), None);
    assert!(!tree.contains(tip));
    // Keyboard focus, shown: at once.
    tree.set_focus(shell.button(View::Toolchains), true)
        .expect("focusable");
    assert!(shell.update(&mut tree, later).unwrap());
    assert!(shell.tooltip().is_some());
}

#[test]
fn the_accessibility_tree_names_the_shell_s_parts() {
    let (tree, _shell) = shell_in(Size::new(1280.0, 800.0), Panels::default());
    let snapshot = tree.accessibility();
    let has = |role: Role, name: &str| {
        snapshot
            .nodes()
            .iter()
            .any(|node| node.role == role && node.name == name)
    };
    assert!(has(Role::Heading, "Tidepool"));
    assert!(has(Role::TabList, "Views"));
    for view in View::ALL {
        assert!(has(Role::Tab, view.name()), "{view:?}");
    }
    assert!(
        has(Role::Pane, "Explorer") && has(Role::Pane, "Editor") && has(Role::Pane, "Bottom panel")
    );
    assert!(has(Role::StatusBar, "Status"));
    assert!(has(Role::Splitter, "Side panel") && has(Role::Splitter, "Bottom panel"));
    assert!(has(Role::TabList, "Bottom panel"));
    let tooltips: Vec<_> = snapshot
        .nodes()
        .iter()
        .filter(|node| node.role == Role::Tab && node.description.is_some())
        .map(|node| node.description.clone().unwrap())
        .collect();
    assert_eq!(tooltips, View::ALL.map(|view| view.name().to_string()));
}

#[test]
fn an_idle_shell_draws_nothing_and_a_change_draws_only_what_changed() {
    let size = Size::new(1280.0, 800.0);
    let (mut tree, mut shell) = shell_in(size, Panels::default());
    assert!(tree.frame(size, &FixedMetrics::DEFAULT).is_none(), "idle");
    shell
        .run(&mut tree, Command::ShowTab(PanelTab::Output))
        .expect("runs");
    let frame = tree
        .frame(size, &FixedMetrics::DEFAULT)
        .expect("the tab changed");
    let window = size.width * size.height;
    let damaged: f32 = frame.damage.iter().map(|r| r.width * r.height).sum();
    assert!(damaged < window / 2.0, "{:?}", frame.damage);
    assert!(
        tree.frame(size, &FixedMetrics::DEFAULT).is_none(),
        "idle again"
    );
}

/// How many rows the open palette lists.
fn palette_rows(tree: &Tree, shell: &Shell) -> usize {
    let palette = shell.palette().expect("open");
    let list = Palette::list(tree, palette).expect("a list");
    tree.widget::<Collection>(list).expect("a collection").len()
}

#[test]
fn the_palette_lists_the_registry_s_fourteen_commands_opening_it_among_them() {
    let size = Size::new(1280.0, 800.0);
    let (mut tree, mut shell) = shell_in(size, Panels::default());
    let now = Instant::now();
    assert_eq!(shell.registry().len(), 14);
    send(&mut tree, &mut shell, size, &[ctrl_shift(Key::P)], now);
    assert_eq!(palette_rows(&tree, &shell), Command::ALL.len());
    let events: Vec<InputEvent> = "command palette"
        .chars()
        .map(|c| text(&c.to_string()))
        .collect();
    send(&mut tree, &mut shell, size, &events, now);
    assert_eq!(palette_rows(&tree, &shell), 1);
    let before = shell.palette();
    send(
        &mut tree,
        &mut shell,
        size,
        &[key(Key::Enter, Modifiers::NONE)],
        now,
    );
    // It ran: the palette closed, and opened again, its query empty.
    let palette = shell.palette().expect("open again");
    assert_ne!(Some(palette), before);
    assert_eq!(Palette::query(&tree, palette), Some(""));
    assert_eq!(palette_rows(&tree, &shell), 14);
}

#[test]
fn a_keymap_from_the_owner_replaces_the_default_one() {
    let size = Size::new(1280.0, 800.0);
    let (mut tree, mut shell) = shell_in(size, Panels::default());
    let now = Instant::now();
    let settings = toml::parse(
        r#"
[keymap.insert]
"ctrl+b" = ""
"ctrl+k ctrl+b" = "view.toggleSidePanel"
"alt+t" = "panel.showTerminal"
"alt+w" = "window.close"
"#,
        tidepool_codec::Limits::DEFAULT,
    )
    .expect("TOML");
    let user = settings
        .get("keymap")
        .and_then(|keymap| keymap.as_table())
        .expect("a table");
    let keymap = Keymap::bundled(shell.registry(), Some(user));
    assert_eq!(keymap.problems().len(), 1, "the one override");
    shell.set_keymap(keymap);
    assert_eq!(shell.keymap_engine().mode(), "insert");

    send(&mut tree, &mut shell, size, &[ctrl(Key::B)], now);
    assert!(shell.panels().side, "Ctrl+B is unbound");
    send(&mut tree, &mut shell, size, &[ctrl(Key::K)], now);
    assert_eq!(shell.keymap_engine().pending().len(), 1, "waiting");
    send(&mut tree, &mut shell, size, &[ctrl(Key::B)], now);
    assert!(!shell.panels().side, "Ctrl+K Ctrl+B toggled it");
    let alt = |k| key(k, Modifiers::ALT);
    send(&mut tree, &mut shell, size, &[alt(Key::T)], now);
    assert_eq!(shell.tab(), PanelTab::Terminal);
    // The default's other keys still work, and an auto-repeat of a
    // window command does nothing.
    let repeat = InputEvent::KeyDown {
        key: Key::J,
        modifiers: Modifiers::CTRL,
        repeat: true,
    };
    send(&mut tree, &mut shell, size, &[repeat], now);
    assert!(shell.panels().bottom, "a repeat is ignored");
    send(&mut tree, &mut shell, size, &[ctrl(Key::J)], now);
    assert!(!shell.panels().bottom);
    let flow = send(&mut tree, &mut shell, size, &[alt(Key::W)], now);
    assert_eq!(flow, Flow::Quit);
}

#[test]
fn keys_a_widget_handled_or_a_modal_blocked_never_reach_the_keymap() {
    let size = Size::new(1280.0, 800.0);
    let (mut tree, mut shell) = shell_in(size, Panels::default());
    let now = Instant::now();
    let settings = toml::parse(
        "[keymap.insert]\n\"ctrl+k ctrl+b\" = \"view.toggleSidePanel\"\n\"ctrl+k enter\" = \"window.close\"\n",
        tidepool_codec::Limits::DEFAULT,
    )
    .expect("TOML");
    let user = settings.get("keymap").and_then(|keymap| keymap.as_table());
    shell.set_keymap(Keymap::bundled(shell.registry(), user));
    // Enter on a focused button is the button's: after Ctrl+K, it shows
    // the button's view, and does not finish `ctrl+k enter`.
    tree.set_focus(shell.button(View::Search), true)
        .expect("focusable");
    send(&mut tree, &mut shell, size, &[ctrl(Key::K)], now);
    assert_eq!(shell.keymap_engine().pending().len(), 1);
    let flow = send(
        &mut tree,
        &mut shell,
        size,
        &[key(Key::Enter, Modifiers::NONE)],
        now,
    );
    assert_eq!((flow, shell.view()), (Flow::Continue, View::Search));
    assert_eq!(shell.keymap_engine().pending().len(), 1, "still waiting");
    shell.set_keymap(Keymap::bundled(shell.registry(), user));
    // While the palette is open, keys are blocked: the engine never sees
    // Ctrl+K, so nothing is pending when it closes.
    send(&mut tree, &mut shell, size, &[ctrl_shift(Key::P)], now);
    send(&mut tree, &mut shell, size, &[ctrl(Key::K)], now);
    assert_eq!(shell.keymap_engine().pending(), []);
    send(
        &mut tree,
        &mut shell,
        size,
        &[key(Key::Escape, Modifiers::NONE)],
        now,
    );
    assert_eq!(shell.palette(), None);
    send(&mut tree, &mut shell, size, &[ctrl(Key::B)], now);
    assert!(!shell.panels().side, "Ctrl+B, the default's, still works");
    send(
        &mut tree,
        &mut shell,
        size,
        &[ctrl(Key::K), ctrl(Key::B)],
        now,
    );
    assert!(shell.panels().side, "and so does the chord");
    // A modifier key going down alone is nothing to the keymap.
    let mut keys = shell.keymap_engine().clone();
    assert_eq!(
        keys.handle(&key(Key::LeftControl, Modifiers::CTRL)),
        Resolution::Ignored
    );
}
