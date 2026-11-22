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

//! M3.7's bound widgets together, through the public API only (ADR-0052):
//! a reveal, the owner's or a collection's, takes effect in the layout the
//! splitters settle on in the same turn, at a resize, in the first frame
//! and with a key; and a property test of random turns of nested splits, a
//! tab strip, two collections (one in a scroller with other content),
//! owner reveals, wheels and resizes, after each of which every split has
//! its ratio in the space it has, the selected tab shows in its track, a
//! selection or node asked for shows, and the next frame is idle and
//! passes no layout.

use std::cell::Cell;
use std::rc::Rc;

use tidepool_core::geom::{Point, Rect, Size};
use tidepool_core::input::{InputEvent, Key, Modifiers, ScrollDelta};
use tidepool_core::rng::Pcg32;
use tidepool_core::testing::check_sized;
use tidepool_ui::layout::{Axis, Layout, Length, Style};
use tidepool_ui::metrics::{FixedMetrics, TextMetrics};
use tidepool_ui::widgets::{
    self, Collection, Control, Row, SPLITTER_THICKNESS, Splitter, Tab, TabKind, TabStrip,
};
use tidepool_ui::{Node, NodeId, Tree, Widget};

const METRICS: FixedMetrics = FixedMetrics::DEFAULT;

fn key(key: Key) -> InputEvent {
    InputEvent::KeyDown {
        key,
        modifiers: Modifiers::NONE,
        repeat: false,
    }
}

/// A node that counts the layout passes, each of which measures it.
struct Passes(Rc<Cell<usize>>);

impl Widget for Passes {
    fn measure(&self, _available: Size, _metrics: &dyn TextMetrics) -> Size {
        self.0.set(self.0.get() + 1);
        Size::ZERO
    }
}

/// Counts the tree's layout passes from now on.
fn count_passes(tree: &mut Tree) -> Rc<Cell<usize>> {
    let passes = Rc::new(Cell::new(0));
    let root = tree.root();
    tree.insert(
        root,
        Node::default().with_widget(Passes(Rc::clone(&passes))),
    )
    .expect("a counter");
    passes
}

/// Whether `inner` lies inside `outer`, give or take layout's rounding.
fn inside(inner: Rect, outer: Rect) -> bool {
    let slack = 0.02;
    inner.x >= outer.x - slack
        && inner.y >= outer.y - slack
        && inner.right() <= outer.right() + slack
        && inner.bottom() <= outer.bottom() + slack
}

fn meet(a: Rect, b: Rect) -> Rect {
    let (x, y) = (a.x.max(b.x), a.y.max(b.y));
    let (right, bottom) = (a.right().min(b.right()), a.bottom().min(b.bottom()));
    Rect::new(x, y, (right - x).max(0.0), (bottom - y).max(0.0))
}

/// A pane at least `min` long on `axis`.
fn pane(axis: Axis, min: f32) -> Node {
    let min = match axis {
        Axis::Horizontal => Size::new(min, 0.0),
        Axis::Vertical => Size::new(0.0, min),
    };
    Node::new(Style::column().fill().min(min))
}

/// The selected row's bounds, if it has a node.
fn selected_row(tree: &Tree) -> Option<Rect> {
    tree.accessibility()
        .nodes()
        .iter()
        .find(|node| node.selected)
        .map(|node| node.bounds)
}

#[test]
fn an_owner_s_reveal_takes_effect_once_nested_splits_settle() {
    // A split whose first pane holds a split whose first pane scrolls 100
    // items: the window shrinks in the turn the owner asks for item 90, and
    // both splits must clamp to it before the reveal.
    let mut tree = Tree::new();
    let root = tree.root();
    let outer = Splitter::new(Axis::Vertical, "Outer", 0.9)
        .insert(
            &mut tree,
            root,
            pane(Axis::Vertical, 0.0),
            pane(Axis::Vertical, 300.0),
        )
        .expect("a split");
    let into = tree.children(outer).expect("panes")[0];
    let scroller = Node::new(Style::column().fill().scroll(false, true));
    let inner = Splitter::new(Axis::Vertical, "Inner", 0.9)
        .insert(&mut tree, into, scroller, pane(Axis::Vertical, 300.0))
        .expect("a split");
    let scroller = tree.children(inner).expect("panes")[0];
    let item = || {
        Node::new(
            Style::stack()
                .width(Length::Fill(1.0))
                .height(Length::Fixed(20.0)),
        )
    };
    let items: Vec<NodeId> = (0..100)
        .map(|_| tree.insert(scroller, item()).expect("an item"))
        .collect();
    tree.frame(Size::new(400.0, 2000.0), &METRICS);
    let small = Size::new(400.0, 1000.0);
    tree.scroll_into_view(items[90]).expect("an item");
    tree.frame(small, &METRICS);
    let view = tree.bounds(scroller).expect("bounds");
    let at = tree.bounds(items[90]).expect("bounds");
    assert!(inside(at, view), "{at:?} in {view:?}");
    assert_eq!(tree.frame(small, &METRICS), None);
}

/// A vertical split at `ratio` filling the window, its second pane at
/// least 300 high, its first holding a list of 100 rows: the tree and the
/// list.
fn list_in_a_split(ratio: f32, selected: Option<usize>) -> (Tree, NodeId) {
    let mut tree = Tree::new();
    let root = tree.root();
    let split = Splitter::new(Axis::Vertical, "Split", ratio)
        .insert(
            &mut tree,
            root,
            pane(Axis::Vertical, 0.0),
            pane(Axis::Vertical, 300.0),
        )
        .expect("a split");
    let into = tree.children(split).expect("panes")[0];
    let rows: Vec<Row> = (0..100).map(|n| Row::new(format!("row {n}"))).collect();
    let mut list = Collection::list("Files", rows);
    list.select(selected);
    let list = tree.insert(into, list.node()).expect("a list");
    (tree, list)
}

/// Whether the selected row has a node inside the list's bounds.
fn selection_shows(tree: &Tree, list: NodeId) -> bool {
    let view = tree.bounds(list).expect("bounds");
    selected_row(tree).is_some_and(|row| row.height > 0.0 && inside(row, view))
}

#[test]
fn a_collection_reveals_its_selection_in_the_space_its_split_settles_on() {
    let (tall, short) = (Size::new(400.0, 2000.0), Size::new(400.0, 1000.0));
    // The owner selects a row in the turn the window shrinks.
    let (mut tree, list) = list_in_a_split(0.9, None);
    tree.frame(tall, &METRICS);
    widgets::update(&mut tree, list, |list: &mut Collection| {
        list.select(Some(90));
    })
    .expect("the list");
    tree.frame(short, &METRICS);
    assert!(selection_shows(&tree, list), "{:?}", selected_row(&tree));
    assert_eq!(tree.frame(short, &METRICS), None);
    // Down moves it in the turn the window shrinks.
    let (mut tree, list) = list_in_a_split(0.9, Some(69));
    tree.frame(tall, &METRICS);
    tree.set_focus(list, true).expect("focus");
    tree.handle(&key(Key::Down));
    tree.frame(short, &METRICS);
    assert_eq!(
        tree.widget::<Collection>(list)
            .and_then(Collection::selected),
        Some(70)
    );
    assert!(selection_shows(&tree, list), "{:?}", selected_row(&tree));
    assert_eq!(tree.frame(short, &METRICS), None);
}

#[test]
fn a_restored_ratio_and_selection_show_in_the_first_frame() {
    // A session saved a ratio the window cannot hold and a selection far
    // down, both set before the first frame.
    for ratio in [0.9, 0.6] {
        let (mut tree, list) = list_in_a_split(ratio, Some(90));
        let size = Size::new(400.0, 1000.0);
        tree.frame(size, &METRICS);
        assert!(
            selection_shows(&tree, list),
            "{ratio}: {:?}",
            selected_row(&tree)
        );
        assert_eq!(tree.frame(size, &METRICS), None, "{ratio}");
    }
}

/// The scene of the property test: splits A, then B in A's second pane,
/// then C in B's second; a tab strip and a list in A's first pane; a
/// scroller in B's first holding other content, a second list and ten
/// nodes the owner may ask for.
struct Scene {
    tree: Tree,
    passes: Rc<Cell<usize>>,
    /// Each split, its divider, its axis and its panes' minimums.
    splits: Vec<(NodeId, NodeId, Axis, f32, f32)>,
    strip: NodeId,
    lists: [NodeId; 2],
    scroller: NodeId,
    targets: Vec<NodeId>,
}

fn scene(ratios: [f32; 3], a_axis: Axis) -> Scene {
    let mut tree = Tree::new();
    let passes = count_passes(&mut tree);
    let root = tree.root();
    let top = tree
        .insert(root, Node::new(Style::column().fill()))
        .expect("a column");
    let a = Splitter::new(a_axis, "A", ratios[0])
        .insert(&mut tree, top, pane(a_axis, 150.0), pane(a_axis, 200.0))
        .expect("a split");
    let (a_first, a_second) = {
        let panes = tree.children(a).expect("panes");
        (panes[0], panes[2])
    };
    let tabs = (0..12).map(|n| Tab::new(format!("file_{n}.rs")).closable());
    let strip = tree
        .insert(
            a_first,
            TabStrip::new("Editors", TabKind::Content, tabs).node(),
        )
        .expect("a strip");
    let rows: Vec<Row> = (0..200).map(|n| Row::new(format!("a{n}"))).collect();
    let first = tree
        .insert(a_first, Collection::list("L1", rows).node())
        .expect("a list");
    let b = Splitter::new(Axis::Vertical, "B", ratios[1])
        .insert(
            &mut tree,
            a_second,
            pane(Axis::Vertical, 100.0),
            pane(Axis::Vertical, 120.0),
        )
        .expect("a split");
    let (b_first, b_second) = {
        let panes = tree.children(b).expect("panes");
        (panes[0], panes[2])
    };
    let scroller = tree
        .insert(
            b_first,
            Node::new(Style::column().fill().scroll(false, true)),
        )
        .expect("a scroller");
    let filler = |height: f32| {
        Node::new(
            Style::stack()
                .width(Length::Fill(1.0))
                .height(Length::Fixed(height)),
        )
    };
    tree.insert(scroller, filler(150.0)).expect("content");
    let rows: Vec<Row> = (0..80).map(|n| Row::new(format!("b{n}"))).collect();
    let second = Collection::list("L2", rows).node();
    let second = Node {
        style: Style {
            width: Length::Fill(1.0),
            height: Length::Fixed(300.0),
            ..second.style
        },
        ..second
    };
    let second = tree.insert(scroller, second).expect("a list");
    let targets = (0..10)
        .map(|_| tree.insert(scroller, filler(40.0)).expect("a node"))
        .collect();
    let c = Splitter::new(Axis::Horizontal, "C", ratios[2])
        .insert(
            &mut tree,
            b_second,
            pane(Axis::Horizontal, 0.0),
            pane(Axis::Horizontal, 100.0),
        )
        .expect("a split");
    let divider = |tree: &Tree, split| Splitter::divider(tree, split).expect("a divider");
    let splits = vec![
        (a, divider(&tree, a), a_axis, 150.0, 200.0),
        (b, divider(&tree, b), Axis::Vertical, 100.0, 120.0),
        (c, divider(&tree, c), Axis::Horizontal, 0.0, 100.0),
    ];
    Scene {
        tree,
        passes,
        splits,
        strip,
        lists: [first, second],
        scroller,
        targets,
    }
}

/// One thing done in a turn.
#[derive(Clone, Debug)]
enum Op {
    Resize(f32, f32),
    /// A key on a list, the first or the second.
    ListKey(usize, Key),
    /// The owner selects a row of a list.
    Select(usize, usize),
    SelectTab(usize),
    InsertTab(usize),
    RemoveTab(usize),
    RelabelTab(usize, usize),
    TabKey(Key),
    /// A key on a split's divider.
    SplitKey(usize, Key),
    /// The owner asks for a node in the scroller, or the second list.
    Reveal(usize),
    /// The wheel over the scroller.
    Wheel(f32),
}

fn op(rng: &mut Pcg32) -> Op {
    let list_keys = [
        Key::Up,
        Key::Down,
        Key::PageUp,
        Key::PageDown,
        Key::Home,
        Key::End,
    ];
    let index = |rng: &mut Pcg32, to: u32| rng.range(0..to) as usize;
    match rng.range(0..12u32) {
        0 | 1 => Op::Resize(
            rng.range(200.0..1600.0f32).round(),
            rng.range(150.0..1200.0f32).round(),
        ),
        2 => Op::ListKey(index(rng, 2), *rng.choose(&list_keys).expect("keys")),
        3 => Op::Select(index(rng, 2), index(rng, 220)),
        4 => Op::SelectTab(index(rng, 20)),
        5 => Op::InsertTab(index(rng, 20)),
        6 => Op::RemoveTab(index(rng, 20)),
        7 => Op::RelabelTab(index(rng, 20), index(rng, 40)),
        8 => Op::TabKey(
            *rng.choose(&[Key::Left, Key::Right, Key::Home, Key::End])
                .expect("keys"),
        ),
        9 => Op::SplitKey(
            index(rng, 3),
            *rng.choose(&[
                Key::Left,
                Key::Right,
                Key::Up,
                Key::Down,
                Key::Home,
                Key::End,
            ])
            .expect("keys"),
        ),
        10 => Op::Reveal(index(rng, 11)),
        _ => Op::Wheel(rng.range(-10.0..10.0f32)),
    }
}

/// What a turn asked to be shown.
#[derive(Default)]
struct Asked {
    /// Each list's selection.
    lists: [bool; 2],
    /// The owner's node.
    owner: Option<NodeId>,
    /// Whether the wheel scrolled the scroller.
    wheel: bool,
}

fn apply(scene: &mut Scene, size: &mut Size, op: &Op, asked: &mut Asked) -> Result<(), String> {
    let tree = &mut scene.tree;
    let error = |error: tidepool_ui::UiError| error.to_string();
    match *op {
        Op::Resize(width, height) => *size = Size::new(width, height),
        Op::ListKey(which, pressed) => {
            let list = scene.lists[which];
            tree.set_focus(list, true).map_err(error)?;
            tree.handle(&key(pressed));
            let len = tree.widget::<Collection>(list).map_or(0, Collection::len);
            asked.lists[which] |= len > 0;
        }
        Op::Select(which, row) => {
            let list = scene.lists[which];
            widgets::update(tree, list, |list: &mut Collection| list.select(Some(row)))
                .map_err(error)?;
            asked.lists[which] = true;
        }
        Op::SelectTab(index) => {
            widgets::update(tree, scene.strip, |tabs: &mut TabStrip| tabs.select(index))
                .map_err(error)?;
        }
        Op::InsertTab(at) => {
            widgets::update(tree, scene.strip, |tabs: &mut TabStrip| {
                tabs.insert(at, Tab::new(format!("new_{at}.rs")).closable())
            })
            .map_err(error)?;
        }
        Op::RemoveTab(index) => {
            widgets::update(tree, scene.strip, |tabs: &mut TabStrip| tabs.remove(index))
                .map_err(error)?;
        }
        Op::RelabelTab(index, long) => {
            widgets::update(tree, scene.strip, |tabs: &mut TabStrip| {
                tabs.set_label(index, "y".repeat(long + 1))
            })
            .map_err(error)?;
        }
        Op::TabKey(pressed) => {
            tree.set_focus(scene.strip, true).map_err(error)?;
            tree.handle(&key(pressed));
        }
        Op::SplitKey(which, pressed) => {
            tree.set_focus(scene.splits[which].1, true).map_err(error)?;
            tree.handle(&key(pressed));
        }
        Op::Reveal(index) => {
            let id = scene.targets.get(index).copied().unwrap_or(scene.lists[1]);
            tree.scroll_into_view(id).map_err(error)?;
            asked.owner = Some(id);
        }
        Op::Wheel(lines) => {
            let at = tree.bounds(scene.scroller).unwrap_or_default();
            tree.handle(&InputEvent::Wheel {
                delta: ScrollDelta::Lines { x: 0.0, y: lines },
                position: Point::new(at.x + 5.0, at.y + 5.0),
                modifiers: Modifiers::NONE,
            });
            // The wheel may scroll the second list, which then shows where
            // it leaves it.
            asked.lists[1] = false;
            asked.wheel = true;
        }
    }
    Ok(())
}

/// Checks the scene after a turn's frame, then that the next is idle.
fn check(scene: &Scene, size: Size, asked: &Asked) -> Result<(), String> {
    let tree = &scene.tree;
    let window = Rect::new(0.0, 0.0, size.width, size.height);
    for &(split, divider, axis, low, high) in &scene.splits {
        let bounds = tree.bounds(split).ok_or("a split")?;
        let extent = match axis {
            Axis::Horizontal => bounds.width,
            Axis::Vertical => bounds.height,
        };
        let kept = tree
            .widget::<Splitter>(divider)
            .map(Splitter::ratio)
            .ok_or("a divider")?;
        let Some(Layout::Split { ratio, .. }) = tree.style(split).map(|style| style.layout) else {
            return Err("not a split".into());
        };
        let wanted = if extent > 0.0 {
            let least = (low / extent).clamp(0.0, 1.0);
            let most = ((extent - SPLITTER_THICKNESS - high) / extent)
                .clamp(0.0, 1.0)
                .max(least);
            kept.clamp(least, most)
        } else {
            kept
        };
        if (ratio - wanted).abs() > 1e-4 {
            return Err(format!("split ratio {ratio}, not {wanted} of {extent}"));
        }
    }
    let tabs = tree.widget::<TabStrip>(scene.strip).ok_or("a strip")?;
    let track = tree.children(scene.strip).ok_or("a track")?[0];
    let shown = tree.bounds(track).ok_or("bounds")?;
    if let Some(selected) = tabs.selected() {
        let node = tree.children(track).ok_or("tabs")?[selected];
        let tab = tree.bounds(node).ok_or("bounds")?;
        if tab.width <= shown.width && shown.width > 0.0 && !inside(tab, shown) {
            return Err(format!(
                "selected tab at {tab:?} out of the track {shown:?}"
            ));
        }
    }
    let scroller = tree.bounds(scene.scroller).ok_or("bounds")?;
    let snapshot = tree.accessibility();
    for (which, &list) in scene.lists.iter().enumerate() {
        // The owner's node outside the second list is revealed after its
        // row, in the scroller they share, and wins there.
        let overruled = which == 1 && asked.owner.is_some_and(|owner| owner != list);
        let Some(selected) = tree
            .widget::<Collection>(list)
            .and_then(Collection::selected)
        else {
            continue;
        };
        if !asked.lists[which] || overruled {
            continue;
        }
        let bounds = tree.bounds(list).ok_or("bounds")?;
        let mut screen = meet(window, bounds);
        if which == 1 {
            screen = meet(screen, scroller);
        }
        // Room for a whole row inside the list's padding.
        if screen.height < 24.0 || bounds.height < 24.0 {
            continue;
        }
        let name = format!("{}{selected}", if which == 0 { "a" } else { "b" });
        let row = snapshot
            .nodes()
            .iter()
            .find(|node| node.selected && node.name == name);
        match row {
            None => {
                return Err(format!(
                    "list {which}: row {selected} asked for has no node"
                ));
            }
            Some(row) if !inside(row.bounds, screen) => {
                return Err(format!(
                    "list {which}: row {selected} at {:?} off {screen:?}",
                    row.bounds
                ));
            }
            Some(_) => {}
        }
    }
    // The owner's ask for the second list itself is replaced by its row.
    if let Some(owner) = asked.owner
        && !(owner == scene.lists[1] && (asked.lists[1] || asked.wheel))
    {
        let at = tree.bounds(owner).ok_or("bounds")?;
        let view = meet(scroller, window);
        if at.height <= scroller.height && view.height > 0.0 && !inside(at, scroller) {
            return Err(format!("the owner's node at {at:?} off {scroller:?}"));
        }
    }
    Ok(())
}

#[test]
fn random_turns_of_splits_tabs_lists_and_reveals_show_what_they_ask_for_and_idle() {
    check_sized(
        "bound widgets in one tree",
        150,
        |rng, size| {
            let ratios = [(); 3].map(|()| rng.range(0.0..1.0f32));
            let axis = if rng.range(0..2u32) == 0 {
                Axis::Horizontal
            } else {
                Axis::Vertical
            };
            // Each op, and whether a frame follows it: several to a turn.
            let ops: Vec<(Op, bool)> = (0..size.max(1))
                .map(|_| (op(rng), rng.range(0..2u32) == 0))
                .collect();
            (ratios, axis, ops)
        },
        |(ratios, axis, ops)| {
            let mut scene = scene(*ratios, *axis);
            let mut size = Size::new(1000.0, 700.0);
            scene.tree.frame(size, &METRICS);
            let mut asked = Asked::default();
            for (step, (op, framed)) in ops.iter().enumerate() {
                apply(&mut scene, &mut size, op, &mut asked)
                    .map_err(|error| format!("step {step}, {op:?}: {error}"))?;
                if !framed && step + 1 < ops.len() {
                    continue;
                }
                scene.tree.frame(size, &METRICS);
                let at = |error: String| format!("after step {step}, {op:?}: {error}");
                check(&scene, size, &asked).map_err(at)?;
                scene.passes.set(0);
                if scene.tree.frame(size, &METRICS).is_some() {
                    return Err(at("an idle frame drew".into()));
                }
                if scene.passes.get() != 0 {
                    return Err(at(format!(
                        "an idle frame ran {} passes",
                        scene.passes.get()
                    )));
                }
                asked = Asked::default();
            }
            Ok(())
        },
    );
}
