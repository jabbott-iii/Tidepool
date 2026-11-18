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

//! M3.1's frames and damage, through the public API only: the first frame
//! and its list, idle frames, hover (and hover following content that
//! scrolls under the pointer), removal, resizing, scrolling, style and
//! enablement changes, balancing a fragment's pushes, dropping commands
//! with NaN geometry, and merging damage.

use tidepool_core::color::Color;
use tidepool_core::geom::{Point, Rect, Size};
use tidepool_core::input::{InputEvent, Modifiers, ScrollDelta};
use tidepool_ui::draw::{Command, DrawList, MAX_DAMAGE_RECTS, PaintContext};
use tidepool_ui::layout::{Length, Style};
use tidepool_ui::metrics::FixedMetrics;
use tidepool_ui::{Node, NodeId, Tree, Widget};

const METRICS: FixedMetrics = FixedMetrics::DEFAULT;
const WINDOW: Size = Size::new(200.0, 200.0);

const A: Color = Color::rgb(1, 0, 0);
const B: Color = Color::rgb(0, 1, 0);
const C: Color = Color::rgb(0, 0, 1);
const D: Color = Color::rgb(1, 1, 0);
const HOT: Color = Color::rgb(9, 9, 9);
const GREY: Color = Color::rgb(5, 5, 5);

/// Fills its bounds, in another color when hovered or disabled, if it has
/// one.
struct Paint {
    rest: Color,
    hover: Option<Color>,
    disabled: Option<Color>,
}

impl Paint {
    const fn new(rest: Color) -> Self {
        Self {
            rest,
            hover: None,
            disabled: None,
        }
    }
}

impl Widget for Paint {
    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let state = cx.state();
        let color = match (state.disabled, state.hovered) {
            (true, _) => self.disabled.unwrap_or(self.rest),
            (false, true) => self.hover.unwrap_or(self.rest),
            (false, false) => self.rest,
        };
        list.rect(cx.bounds(), color);
    }
}

/// Paints the same commands wherever it is.
struct Raw(Vec<Command>);

impl Widget for Raw {
    fn paint(&self, _cx: &PaintContext<'_>, list: &mut DrawList) {
        for command in &self.0 {
            list.push(command.clone());
        }
    }
}

fn rect(rect: Rect, color: Color) -> Command {
    Command::Rect { rect, color }
}

fn whole(size: Size) -> Vec<Rect> {
    vec![Rect::new(0.0, 0.0, size.width, size.height)]
}

fn damage(tree: &mut Tree, size: Size) -> Option<Vec<Rect>> {
    tree.frame(size, &METRICS).map(|frame| frame.damage)
}

fn mouse_move(x: f32, y: f32) -> InputEvent {
    InputEvent::MouseMove {
        position: Point::new(x, y),
        modifiers: Modifiers::NONE,
    }
}

/// A column of `count` rows 20 high, each filling its bounds.
fn rows(count: usize) -> (Tree, Vec<NodeId>) {
    let mut tree = Tree::new();
    let root = tree.root();
    let ids = (0..count)
        .map(|_| {
            let row = Style::default()
                .width(Length::Fill(1.0))
                .height(Length::Fixed(20.0));
            tree.insert(root, Node::new(row).with_widget(Paint::new(A)))
                .expect("insert")
        })
        .collect();
    (tree, ids)
}

#[test]
fn the_first_frame_damages_the_window_and_lists_every_fragment_in_pre_order() {
    let mut tree = Tree::new();
    let root = tree.root();
    let panel = tree
        .insert(
            root,
            Node::new(
                Style::stack()
                    .width(Length::Fill(1.0))
                    .height(Length::Fixed(100.0))
                    .clip()
                    .opacity(0.5),
            )
            .with_widget(Paint::new(A)),
        )
        .expect("insert");
    tree.insert(
        panel,
        Node::new(Style::default().fixed(50.0, 50.0)).with_widget(Paint::new(B)),
    )
    .expect("insert");
    tree.insert(
        root,
        Node::new(Style::default().fixed(10.0, 10.0).hidden()).with_widget(Paint::new(C)),
    )
    .expect("insert");
    tree.insert(
        root,
        Node::new(
            Style::default()
                .width(Length::Fill(1.0))
                .height(Length::Fixed(20.0)),
        )
        .with_widget(Paint::new(D)),
    )
    .expect("insert");

    let frame = tree.frame(WINDOW, &METRICS).expect("the first frame");
    assert_eq!(frame.damage, whole(WINDOW));
    let panel_bounds = Rect::new(0.0, 0.0, 200.0, 100.0);
    assert_eq!(
        frame.list.commands(),
        &[
            Command::PushLayer { opacity: 0.5 },
            rect(panel_bounds, A),
            Command::PushClip { rect: panel_bounds },
            rect(Rect::new(0.0, 0.0, 50.0, 50.0), B),
            Command::PopClip,
            Command::PopLayer,
            rect(Rect::new(0.0, 100.0, 200.0, 20.0), D),
        ]
    );

    assert_eq!(tree.frame(WINDOW, &METRICS), None, "an idle frame");
    assert_eq!(tree.frame(WINDOW, &METRICS), None);
}

#[test]
fn a_hover_damages_only_the_widget_whose_paint_changed() {
    let mut tree = Tree::new();
    let root = tree.root();
    tree.set_style(root, Style::row()).expect("the root");
    let hot = Paint {
        hover: Some(HOT),
        ..Paint::new(A)
    };
    let first = tree
        .insert(
            root,
            Node::new(Style::default().fixed(50.0, 50.0)).with_widget(hot),
        )
        .expect("insert");
    tree.insert(
        root,
        Node::new(Style::default().fixed(50.0, 50.0)).with_widget(Paint::new(B)),
    )
    .expect("insert");
    assert!(tree.frame(WINDOW, &METRICS).is_some());

    tree.handle(&mouse_move(10.0, 10.0));
    let frame = tree
        .frame(WINDOW, &METRICS)
        .expect("the hover changed paint");
    assert_eq!(frame.damage, vec![Rect::new(0.0, 0.0, 50.0, 50.0)]);
    assert_eq!(
        frame.list.commands().first(),
        Some(&rect(Rect::new(0.0, 0.0, 50.0, 50.0), HOT))
    );
    assert_eq!(tree.bounds(first), Some(Rect::new(0.0, 0.0, 50.0, 50.0)));

    // A hover over the other widget changes the first back, and nothing
    // else.
    tree.handle(&mouse_move(60.0, 10.0));
    assert_eq!(
        damage(&mut tree, WINDOW),
        Some(vec![Rect::new(0.0, 0.0, 50.0, 50.0)])
    );
    // A hover that changes no paint damages nothing.
    tree.handle(&mouse_move(70.0, 10.0));
    tree.handle(&mouse_move(150.0, 150.0));
    assert_eq!(damage(&mut tree, WINDOW), None);
}

#[test]
fn the_hover_follows_rows_the_wheel_scrolls_under_the_pointer() {
    let mut tree = Tree::new();
    let root = tree.root();
    let scroller = tree
        .insert(
            root,
            Node::new(Style::column().fixed(100.0, 100.0).scroll(false, true)),
        )
        .expect("insert");
    let colors = [
        (A, Color::rgb(1, 9, 9)),
        (B, Color::rgb(2, 9, 9)),
        (C, Color::rgb(3, 9, 9)),
        (D, Color::rgb(4, 9, 9)),
    ];
    let rows: Vec<NodeId> = colors
        .iter()
        .map(|&(rest, hover)| {
            let row = Style::default()
                .width(Length::Fill(1.0))
                .height(Length::Fixed(50.0));
            let paint = Paint {
                hover: Some(hover),
                ..Paint::new(rest)
            };
            tree.insert(scroller, Node::new(row).with_widget(paint))
                .expect("insert")
        })
        .collect();
    tree.handle(&mouse_move(10.0, 10.0));
    let frame = tree.frame(WINDOW, &METRICS).expect("the first frame");
    assert!(
        frame
            .list
            .commands()
            .contains(&rect(Rect::new(0.0, 0.0, 100.0, 50.0), colors[0].1))
    );

    // The wheel at the same point scrolls row 1 under the pointer.
    let response = tree.handle(&InputEvent::Wheel {
        delta: ScrollDelta::Logical { x: 0.0, y: 50.0 },
        position: Point::new(10.0, 10.0),
        modifiers: Modifiers::NONE,
    });
    assert!(response.handled);
    let frame = tree.frame(WINDOW, &METRICS).expect("the scroll");
    assert_eq!(tree.hit_test(Point::new(10.0, 10.0)), Some(rows[1]));
    assert!(tree.state(rows[1]).expect("in the tree").hovered);
    assert!(!tree.state(rows[0]).expect("in the tree").hovered);
    let commands = frame.list.commands();
    assert!(commands.contains(&rect(Rect::new(0.0, 0.0, 100.0, 50.0), colors[1].1)));
    assert!(commands.contains(&rect(Rect::new(0.0, -50.0, 100.0, 50.0), colors[0].0)));
}

#[test]
fn removing_a_node_damages_what_it_painted() {
    let (mut tree, ids) = rows(3);
    assert!(tree.frame(WINDOW, &METRICS).is_some());
    tree.remove(ids[2]).expect("remove");
    assert_eq!(
        damage(&mut tree, WINDOW),
        Some(vec![Rect::new(0.0, 40.0, 200.0, 20.0)])
    );
    // Removing the first moves the second up: both places are damaged.
    tree.remove(ids[0]).expect("remove");
    let mut damaged = damage(&mut tree, WINDOW).expect("damage");
    damaged.sort_by(|a, b| a.y.total_cmp(&b.y));
    assert_eq!(
        damaged,
        vec![
            Rect::new(0.0, 0.0, 200.0, 20.0),
            Rect::new(0.0, 20.0, 200.0, 20.0),
        ]
    );
}

#[test]
fn a_resize_or_invalidate_all_damages_the_whole_window() {
    let (mut tree, _) = rows(2);
    assert!(tree.frame(WINDOW, &METRICS).is_some());
    let smaller = Size::new(300.0, 100.0);
    assert_eq!(damage(&mut tree, smaller), Some(whole(smaller)));
    assert_eq!(damage(&mut tree, smaller), None);
    tree.invalidate_all();
    assert_eq!(damage(&mut tree, smaller), Some(whole(smaller)));
    assert_eq!(damage(&mut tree, smaller), None);
    // An empty window has nothing to damage.
    assert_eq!(damage(&mut tree, Size::ZERO), None);
}

#[test]
fn scrolling_damages_the_children_it_moves() {
    let mut tree = Tree::new();
    let root = tree.root();
    let scroller = tree
        .insert(
            root,
            Node::new(Style::column().fixed(100.0, 100.0).scroll(false, true)),
        )
        .expect("insert");
    for color in [A, B, C, D] {
        let row = Style::default()
            .width(Length::Fill(1.0))
            .height(Length::Fixed(50.0));
        tree.insert(scroller, Node::new(row).with_widget(Paint::new(color)))
            .expect("insert");
    }
    assert!(tree.frame(WINDOW, &METRICS).is_some());
    let response = tree.handle(&InputEvent::Wheel {
        delta: ScrollDelta::Logical { x: 0.0, y: 50.0 },
        position: Point::new(10.0, 10.0),
        modifiers: Modifiers::NONE,
    });
    assert!(response.handled);
    let mut damaged = damage(&mut tree, WINDOW).expect("damage");
    damaged.sort_by(|a, b| a.y.total_cmp(&b.y));
    // What the rows painted inside the scroller's clip, before and after.
    assert_eq!(
        damaged,
        vec![
            Rect::new(0.0, 0.0, 100.0, 50.0),
            Rect::new(0.0, 50.0, 100.0, 50.0),
        ]
    );
}

#[test]
fn style_and_enablement_changes_repaint() {
    let mut tree = Tree::new();
    let root = tree.root();
    let paint = Paint {
        disabled: Some(GREY),
        ..Paint::new(A)
    };
    let node = tree
        .insert(
            root,
            Node::new(Style::default().fixed(40.0, 40.0)).with_widget(paint),
        )
        .expect("insert");
    let plain = tree
        .insert(
            root,
            Node::new(Style::default().fixed(40.0, 40.0)).with_widget(Paint::new(B)),
        )
        .expect("insert");
    assert!(tree.frame(WINDOW, &METRICS).is_some());

    tree.set_enabled(node, false).expect("in the tree");
    let frame = tree.frame(WINDOW, &METRICS).expect("disabled paint");
    assert_eq!(frame.damage, vec![Rect::new(0.0, 0.0, 40.0, 40.0)]);
    assert!(
        frame
            .list
            .commands()
            .contains(&rect(Rect::new(0.0, 0.0, 40.0, 40.0), GREY))
    );
    // Disabling a node whose paint does not show it damages nothing.
    tree.set_enabled(plain, false).expect("in the tree");
    assert_eq!(damage(&mut tree, WINDOW), None);

    // A new opacity paints the same fragment, but is damaged.
    let style = *tree.style(plain).expect("in the tree");
    tree.set_style(plain, style.opacity(0.5))
        .expect("in the tree");
    let frame = tree.frame(WINDOW, &METRICS).expect("forced damage");
    assert_eq!(frame.damage, vec![Rect::new(0.0, 40.0, 40.0, 40.0)]);
    assert!(
        frame
            .list
            .commands()
            .contains(&Command::PushLayer { opacity: 0.5 })
    );

    // Hiding it damages what it painted.
    tree.set_style(plain, style.hidden()).expect("in the tree");
    assert_eq!(
        damage(&mut tree, WINDOW),
        Some(vec![Rect::new(0.0, 40.0, 40.0, 40.0)])
    );

    // A widget changed without changing its paint damages nothing.
    if let Some(paint) = tree.widget_mut::<Paint>(node) {
        paint.hover = Some(HOT);
    }
    assert_eq!(damage(&mut tree, WINDOW), None);
    if let Some(paint) = tree.widget_mut::<Paint>(node) {
        paint.disabled = Some(C);
    }
    assert_eq!(
        damage(&mut tree, WINDOW),
        Some(vec![Rect::new(0.0, 0.0, 40.0, 40.0)])
    );
}

#[test]
fn a_fragments_pushes_and_pops_are_balanced() {
    let mut tree = Tree::new();
    let root = tree.root();
    let clip = Rect::new(0.0, 0.0, 10.0, 10.0);
    tree.insert(
        root,
        Node::new(Style::default().fixed(10.0, 10.0)).with_widget(Raw(vec![
            Command::PopLayer,
            Command::PushClip { rect: clip },
            rect(clip, A),
            Command::PushLayer { opacity: 0.25 },
            Command::PopClip,
        ])),
    )
    .expect("insert");
    let frame = tree.frame(WINDOW, &METRICS).expect("the first frame");
    assert_eq!(
        frame.list.commands(),
        &[
            Command::PushClip { rect: clip },
            rect(clip, A),
            Command::PushLayer { opacity: 0.25 },
            Command::PopLayer,
            Command::PopClip,
        ]
    );
}

#[test]
fn commands_with_nan_geometry_never_reach_the_list() {
    let mut tree = Tree::new();
    let root = tree.root();
    let nan = Rect::new(f32::NAN, 0.0, 10.0, 10.0);
    let fine = Rect::new(0.0, 0.0, 10.0, 10.0);
    let node = tree
        .insert(
            root,
            Node::new(Style::default().fixed(10.0, 10.0)).with_widget(Raw(vec![
                rect(nan, A),
                Command::PushClip { rect: nan },
                rect(fine, B),
                Command::PopClip,
                Command::PushLayer { opacity: f32::NAN },
                rect(fine, C),
                Command::PushLayer { opacity: 7.0 },
                Command::PushLayer { opacity: -1.0 },
            ])),
        )
        .expect("insert");
    let frame = tree.frame(WINDOW, &METRICS).expect("the first frame");
    // The NaN rect and the NaN clip are gone, and the clip's pop with it.
    assert_eq!(
        frame.list.commands(),
        &[
            rect(fine, B),
            Command::PushLayer { opacity: 1.0 },
            rect(fine, C),
            Command::PushLayer { opacity: 1.0 },
            Command::PushLayer { opacity: 0.0 },
            Command::PopLayer,
            Command::PopLayer,
            Command::PopLayer,
        ]
    );

    // A widget painting only a NaN rect paints nothing, and changing or
    // removing it damages nothing, as nothing was drawn.
    if let Some(raw) = tree.widget_mut::<Raw>(node) {
        raw.0 = vec![rect(nan, A)];
    }
    let frame = tree.frame(WINDOW, &METRICS).expect("the rects went");
    assert!(frame.list.is_empty());
    assert_eq!(frame.damage, vec![fine]);
    if let Some(raw) = tree.widget_mut::<Raw>(node) {
        raw.0 = vec![rect(nan, D)];
    }
    assert_eq!(damage(&mut tree, WINDOW), None);
    tree.remove(node).expect("remove");
    assert_eq!(damage(&mut tree, WINDOW), None);
}

/// How many clips are open at `index` of `commands`.
fn open_clips(commands: &[Command], index: usize) -> usize {
    commands
        .iter()
        .take(index)
        .fold(0, |open, command| match command {
            Command::PushClip { .. } => open + 1,
            Command::PopClip => open.saturating_sub(1),
            _ => open,
        })
}

#[test]
fn a_nan_clip_inside_another_takes_its_own_pop_with_it() {
    let outer = Rect::new(0.0, 0.0, 10.0, 10.0);
    let nan = Rect::new(f32::NAN, 0.0, 5.0, 5.0);
    let inner = rect(Rect::new(1.0, 1.0, 2.0, 2.0), A);
    let after = rect(Rect::new(0.0, 0.0, 100.0, 100.0), B);
    for layered in [false, true] {
        let mut commands = vec![
            Command::PushClip { rect: outer },
            Command::PushClip { rect: nan },
        ];
        if layered {
            commands.extend([
                Command::PushLayer { opacity: 0.5 },
                inner.clone(),
                Command::PopLayer,
            ]);
        } else {
            commands.push(inner.clone());
        }
        commands.extend([Command::PopClip, after.clone(), Command::PopClip]);
        let mut tree = Tree::new();
        let root = tree.root();
        tree.insert(
            root,
            Node::new(Style::default().fixed(10.0, 10.0)).with_widget(Raw(commands)),
        )
        .expect("insert");
        let frame = tree.frame(WINDOW, &METRICS).expect("the first frame");
        let list = frame.list.commands();
        let mut expected = vec![Command::PushClip { rect: outer }];
        if layered {
            expected.extend([
                Command::PushLayer { opacity: 0.5 },
                inner.clone(),
                Command::PopLayer,
            ]);
        } else {
            expected.push(inner.clone());
        }
        expected.extend([after.clone(), Command::PopClip]);
        assert_eq!(list, expected.as_slice(), "layered: {layered}");
        // The rect after the bad clip's pop is still inside the outer clip.
        let at = list
            .iter()
            .position(|command| *command == after)
            .expect("in the list");
        assert_eq!(open_clips(list, at), 1, "layered: {layered}");
    }
}

#[test]
fn more_damage_rects_than_the_limit_merge_into_one() {
    let mut tree = Tree::new();
    let root = tree.root();
    tree.set_style(root, Style::grid(10)).expect("the root");
    let count = MAX_DAMAGE_RECTS + 8;
    let ids: Vec<NodeId> = (0..count)
        .map(|_| {
            tree.insert(
                root,
                Node::new(Style::default().fixed(10.0, 10.0)).with_widget(Paint::new(A)),
            )
            .expect("insert")
        })
        .collect();
    assert!(tree.frame(WINDOW, &METRICS).is_some());
    // Changing a few gives a rect each.
    for &id in &ids[..3] {
        if let Some(paint) = tree.widget_mut::<Paint>(id) {
            paint.rest = B;
        }
    }
    assert_eq!(damage(&mut tree, WINDOW).map(|rects| rects.len()), Some(3));
    // Changing them all gives their bounding rect: four rows of cells 20
    // wide, the last ten wide.
    for &id in &ids {
        if let Some(paint) = tree.widget_mut::<Paint>(id) {
            paint.rest = C;
        }
    }
    assert_eq!(
        damage(&mut tree, WINDOW),
        Some(vec![Rect::new(0.0, 0.0, 190.0, 40.0)])
    );
}
