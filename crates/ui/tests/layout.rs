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

//! M3.1's layout properties and known answers, through the public API
//! only:
//!
//! 1. Children never overflow a non-scrolling parent: random trees with
//!    random styles of every kind (NaN, infinite, negative and huge values
//!    too), random widget sizes and window sizes; after layout every rect
//!    is finite with a size that is not negative, and on each axis its
//!    parent does not scroll every shown child lies inside the parent's
//!    bounds and content box, exactly, in `f32`.
//! 2. Layout is idempotent and deterministic: the same trees laid out
//!    again after `invalidate_all`, a third time after another
//!    `invalidate_all` (from the second pass's clamped offsets), and as a
//!    second tree built from the same description, give the same rects,
//!    content sizes and offsets.
//! 3. Known answers for each layout, length, alignment, padding, gap,
//!    scrolling, scrolling into view and hiding.

use tidepool_core::geom::{Point, Rect, Size};
use tidepool_core::rng::Pcg32;
use tidepool_core::testing::check_sized;
use tidepool_ui::layout::{Align, Axis, Insets, Layout, Length, MAX_EXTENT, Style};
use tidepool_ui::metrics::{FixedMetrics, TextMetrics};
use tidepool_ui::{Node, NodeId, Tree, UiError, Widget};

const METRICS: FixedMetrics = FixedMetrics::DEFAULT;

/// A widget whose content is a given size.
struct Sized(Size);

impl Widget for Sized {
    fn measure(&self, _available: Size, _metrics: &dyn TextMetrics) -> Size {
        self.0
    }
}

/// A node of a random tree: its parent (an index into the nodes before
/// it, 0 being the root), style, widget size and scroll offset.
#[derive(Clone, Debug)]
struct Spec {
    parent: usize,
    style: Style,
    measure: Option<Size>,
    scroll: Option<Point>,
}

/// A random tree: the window size, the root's style and the other nodes.
#[derive(Clone, Debug)]
struct Description {
    window: Size,
    root: Style,
    nodes: Vec<Spec>,
}

/// A length or coordinate, often wild.
fn value(rng: &mut Pcg32) -> f32 {
    match rng.range(0..20u32) {
        0 => f32::NAN,
        1 => f32::INFINITY,
        2 => f32::NEG_INFINITY,
        3 => -rng.range(0.0..500.0f32),
        4 => rng.range(1e6..1e30f32),
        5 => MAX_EXTENT,
        6 => 0.0,
        7 => rng.range(0.0..1.0f32),
        8 => (rng.range(0..3000) as f32) / 7.0,
        _ => rng.range(0.0..400.0f32),
    }
}

/// A small length, sometimes wild, for padding and gaps.
fn small(rng: &mut Pcg32) -> f32 {
    if rng.range(0..6u32) == 0 {
        value(rng)
    } else {
        rng.range(0.0..20.0f32)
    }
}

fn length(rng: &mut Pcg32) -> Length {
    match rng.range(0..3u32) {
        0 => Length::Fixed(value(rng)),
        1 => Length::Fill(if rng.range(0..5u32) == 0 {
            value(rng)
        } else {
            rng.range(0.0..4.0f32)
        }),
        _ => Length::Content,
    }
}

fn align(rng: &mut Pcg32) -> Align {
    *rng.choose(&[Align::Start, Align::Center, Align::End])
        .expect("not empty")
}

fn style(rng: &mut Pcg32) -> Style {
    let layout = match rng.range(0..5u32) {
        0 => Layout::Column,
        1 => Layout::Row,
        2 => Layout::Stack,
        3 => Layout::Grid {
            columns: rng.range(0..5u32) as u16,
        },
        _ => Layout::Split {
            axis: if rng.range(0..2u32) == 0 {
                Axis::Horizontal
            } else {
                Axis::Vertical
            },
            ratio: match rng.range(0..6u32) {
                0 => f32::NAN,
                1 => -1.0,
                2 => 3.0,
                _ => rng.range(0.0..1.0f32),
            },
        },
    };
    let limits = |rng: &mut Pcg32| {
        if rng.range(0..3u32) == 0 {
            Size::new(value(rng), value(rng))
        } else {
            Size::ZERO
        }
    };
    let min = limits(rng);
    let max = if rng.range(0..3u32) == 0 {
        Size::new(value(rng), value(rng))
    } else {
        Size::new(MAX_EXTENT, MAX_EXTENT)
    };
    Style {
        layout,
        width: length(rng),
        height: length(rng),
        min,
        max,
        padding: Insets::new(small(rng), small(rng), small(rng), small(rng)),
        gap: small(rng),
        align_x: align(rng),
        align_y: align(rng),
        scroll_x: rng.range(0..5u32) == 0,
        scroll_y: rng.range(0..4u32) == 0,
        clip: rng.range(0..4u32) == 0,
        opacity: rng.range(0.0..1.5f32),
        hidden: rng.range(0..10u32) == 0,
    }
}

fn description(rng: &mut Pcg32, size: u32) -> Description {
    let count = (size as usize * 40) / 100;
    let window = match rng.range(0..10u32) {
        0 => Size::new(value(rng), value(rng)),
        1 => Size::ZERO,
        _ => Size::new(rng.range(0.0..3000.0f32), rng.range(0.0..3000.0f32)),
    };
    let mut root = style(rng);
    root.hidden = rng.range(0..30u32) == 0;
    // Depth of each node: the root's is 0.
    let mut depths = vec![0usize];
    let mut nodes = Vec::with_capacity(count);
    for _ in 0..count {
        let allowed: Vec<usize> = (0..depths.len()).filter(|&i| depths[i] < 6).collect();
        let parent = *rng.choose(&allowed).expect("the root is allowed");
        depths.push(depths[parent] + 1);
        let measure = (rng.range(0..2u32) == 0).then(|| Size::new(value(rng), value(rng)));
        let scroll = (rng.range(0..4u32) == 0).then(|| Point::new(value(rng), value(rng)));
        nodes.push(Spec {
            parent,
            style: style(rng),
            measure,
            scroll,
        });
    }
    Description {
        window,
        root,
        nodes,
    }
}

/// Builds the tree; the ids are in the order of the description, the
/// root's first.
fn build(description: &Description) -> (Tree, Vec<NodeId>) {
    let mut tree = Tree::new();
    let root = tree.root();
    tree.set_style(root, description.root).expect("the root");
    let mut ids = vec![root];
    for spec in &description.nodes {
        let mut node = Node::new(spec.style);
        if let Some(size) = spec.measure {
            node = node.with_widget(Sized(size));
        }
        let id = tree.insert(ids[spec.parent], node).expect("insert");
        if let Some(offset) = spec.scroll {
            tree.set_scroll_offset(id, offset).expect("scroll");
        }
        ids.push(id);
    }
    (tree, ids)
}

/// A padding made sane, as layout's documentation gives the rule.
fn sane(value: f32) -> f32 {
    if value.is_nan() || value <= 0.0 {
        0.0
    } else {
        value.min(MAX_EXTENT)
    }
}

/// The parent's content box, as layout's documentation defines it.
fn content_box(rect: Rect, padding: Insets) -> Rect {
    let left = sane(padding.left).min(rect.width);
    let right = sane(padding.right).min(rect.width - left);
    let top = sane(padding.top).min(rect.height);
    let bottom = sane(padding.bottom).min(rect.height - top);
    Rect::new(
        rect.x + left,
        rect.y + top,
        rect.width - left - right,
        rect.height - top - bottom,
    )
}

/// Whether the node or an ancestor is hidden.
fn in_hidden(tree: &Tree, id: NodeId) -> bool {
    let mut next = Some(id);
    while let Some(id) = next {
        if tree.style(id).is_some_and(|style| style.hidden) {
            return true;
        }
        next = tree.parent(id);
    }
    false
}

fn finite(rect: Rect) -> bool {
    rect.x.is_finite()
        && rect.y.is_finite()
        && rect.width.is_finite()
        && rect.height.is_finite()
        && rect.width >= 0.0
        && rect.height >= 0.0
}

/// Checks that a child lies inside `outer` on the axes asked for, exactly.
fn within(child: Rect, outer: Rect, x: bool, y: bool) -> bool {
    (!x || (outer.x <= child.x && child.right() <= outer.right()))
        && (!y || (outer.y <= child.y && child.bottom() <= outer.bottom()))
}

fn contained(description: &Description) -> Result<(), String> {
    let (mut tree, ids) = build(description);
    tree.layout(description.window, &METRICS);
    for &id in &ids {
        let rect = tree.bounds(id).ok_or("a node is gone")?;
        if !finite(rect) {
            return Err(format!("{id} has bounds {rect:?}"));
        }
        let content = tree.content_size(id).ok_or("a node is gone")?;
        let offset = tree.scroll_offset(id).ok_or("a node is gone")?;
        let sane_numbers = [content.width, content.height, offset.x, offset.y]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0 && *v <= MAX_EXTENT);
        if !sane_numbers {
            return Err(format!(
                "{id} has content {content:?} and offset {offset:?}"
            ));
        }
        let style = *tree.style(id).ok_or("a node is gone")?;
        // A hidden node keeps its offset until it is shown.
        if !in_hidden(&tree, id)
            && ((!style.scroll_x && offset.x != 0.0) || (!style.scroll_y && offset.y != 0.0))
        {
            return Err(format!(
                "{id} is scrolled to {offset:?} on an axis it does not scroll"
            ));
        }
        let inner = content_box(rect, style.padding);
        for &child in tree.children(id).ok_or("a node is gone")? {
            if tree.style(child).is_some_and(|style| style.hidden) {
                continue;
            }
            let bounds = tree.bounds(child).ok_or("a child is gone")?;
            let (x, y) = (!style.scroll_x, !style.scroll_y);
            if !within(bounds, rect, x, y) {
                return Err(format!(
                    "{child} at {bounds:?} overflows its parent {id} at {rect:?}"
                ));
            }
            if !within(bounds, inner, x, y) {
                return Err(format!(
                    "{child} at {bounds:?} overflows the content box {inner:?} of {id}"
                ));
            }
        }
    }
    Ok(())
}

fn rects(tree: &Tree, ids: &[NodeId]) -> Vec<(Option<Rect>, Option<Size>, Option<Point>)> {
    ids.iter()
        .map(|&id| {
            (
                tree.bounds(id),
                tree.content_size(id),
                tree.scroll_offset(id),
            )
        })
        .collect()
}

fn idempotent(description: &Description) -> Result<(), String> {
    let (mut tree, ids) = build(description);
    tree.layout(description.window, &METRICS);
    let first = rects(&tree, &ids);
    tree.invalidate_all();
    tree.layout(description.window, &METRICS);
    if rects(&tree, &ids) != first {
        return Err("a second layout differs".to_string());
    }
    // A real third pass, from the second's clamped offsets.
    tree.invalidate_all();
    tree.layout(description.window, &METRICS);
    if rects(&tree, &ids) != first {
        return Err("a third layout differs".to_string());
    }
    let (mut again, again_ids) = build(description);
    again.layout(description.window, &METRICS);
    if rects(&again, &again_ids) != first {
        return Err("a second tree from the same description differs".to_string());
    }
    Ok(())
}

#[test]
fn children_never_overflow_a_non_scrolling_parent() {
    check_sized(
        "children never overflow a non-scrolling parent",
        1000,
        description,
        contained,
    );
}

#[test]
fn layout_is_idempotent_and_deterministic() {
    check_sized(
        "layout is idempotent and deterministic",
        300,
        description,
        idempotent,
    );
}

/// A tree whose root has `root` and children of `styles`, laid out in a
/// window of `width` by `height`.
fn laid_out(root: Style, styles: &[Style], width: f32, height: f32) -> (Tree, Vec<NodeId>) {
    let mut tree = Tree::new();
    let root_id = tree.root();
    tree.set_style(root_id, root).expect("the root");
    let ids = styles
        .iter()
        .map(|&style| tree.insert(root_id, Node::new(style)).expect("insert"))
        .collect();
    tree.layout(Size::new(width, height), &METRICS);
    (tree, ids)
}

fn bounds(tree: &Tree, ids: &[NodeId]) -> Vec<Rect> {
    ids.iter()
        .map(|&id| tree.bounds(id).expect("in the tree"))
        .collect()
}

#[test]
fn a_row_shares_free_space_among_fills_by_weight() {
    let tall = Style::default().height(Length::Fill(1.0));
    let (tree, ids) = laid_out(
        Style::row(),
        &[
            tall.width(Length::Fixed(100.0)),
            tall.width(Length::Fill(1.0)),
            tall.width(Length::Fill(3.0)),
        ],
        500.0,
        100.0,
    );
    assert_eq!(
        bounds(&tree, &ids),
        vec![
            Rect::new(0.0, 0.0, 100.0, 100.0),
            Rect::new(100.0, 0.0, 100.0, 100.0),
            Rect::new(200.0, 0.0, 300.0, 100.0),
        ]
    );
    assert_eq!(
        tree.bounds(tree.root()),
        Some(Rect::new(0.0, 0.0, 500.0, 100.0))
    );
}

#[test]
fn gaps_go_between_children() {
    let child = Style::default().fixed(50.0, 10.0);
    let (tree, ids) = laid_out(Style::row().gap(10.0), &[child; 3], 400.0, 50.0);
    assert_eq!(
        bounds(&tree, &ids),
        vec![
            Rect::new(0.0, 0.0, 50.0, 10.0),
            Rect::new(60.0, 0.0, 50.0, 10.0),
            Rect::new(120.0, 0.0, 50.0, 10.0),
        ]
    );
    let (tree, ids) = laid_out(Style::column().gap(5.0), &[child; 2], 400.0, 50.0);
    assert_eq!(
        bounds(&tree, &ids),
        vec![
            Rect::new(0.0, 0.0, 50.0, 10.0),
            Rect::new(0.0, 15.0, 50.0, 10.0),
        ]
    );
}

#[test]
fn children_too_large_shrink_in_proportion() {
    let (tree, ids) = laid_out(
        Style::row(),
        &[
            Style::default().fixed(80.0, 10.0),
            Style::default().width(Length::Fill(1.0)),
            Style::default().fixed(120.0, 10.0),
        ],
        100.0,
        50.0,
    );
    assert_eq!(
        bounds(&tree, &ids),
        vec![
            Rect::new(0.0, 0.0, 40.0, 10.0),
            Rect::new(40.0, 0.0, 0.0, 0.0),
            Rect::new(40.0, 0.0, 60.0, 10.0),
        ]
    );
}

#[test]
fn a_fill_outside_its_limits_freezes_at_them() {
    let fill = Style::default().width(Length::Fill(1.0));
    let (tree, ids) = laid_out(
        Style::row(),
        &[fill.max(Size::new(50.0, MAX_EXTENT)), fill],
        300.0,
        10.0,
    );
    assert_eq!(
        bounds(&tree, &ids),
        vec![
            Rect::new(0.0, 0.0, 50.0, 0.0),
            Rect::new(50.0, 0.0, 250.0, 0.0),
        ]
    );
    let (tree, ids) = laid_out(
        Style::row(),
        &[fill.min(Size::new(60.0, 0.0)), fill, fill],
        90.0,
        10.0,
    );
    assert_eq!(
        bounds(&tree, &ids),
        vec![
            Rect::new(0.0, 0.0, 60.0, 0.0),
            Rect::new(60.0, 0.0, 15.0, 0.0),
            Rect::new(75.0, 0.0, 15.0, 0.0),
        ]
    );
    // Minimums larger than the space are scaled down to fit.
    let (tree, ids) = laid_out(
        Style::row(),
        &[
            fill.min(Size::new(100.0, 0.0)),
            fill.min(Size::new(100.0, 0.0)),
        ],
        50.0,
        10.0,
    );
    assert_eq!(
        bounds(&tree, &ids),
        vec![
            Rect::new(0.0, 0.0, 25.0, 0.0),
            Rect::new(25.0, 0.0, 25.0, 0.0),
        ]
    );
}

#[test]
fn a_freeze_round_freezes_only_the_fills_held_the_way_clamping_went() {
    let fill = Style::default().width(Length::Fill(1.0));
    let widths = |styles: &[Style]| -> Vec<f32> {
        let (tree, ids) = laid_out(Style::row(), styles, 100.0, 10.0);
        bounds(&tree, &ids).iter().map(|rect| rect.width).collect()
    };
    // Shares of 50 clamp to 10 and 95, adding 5: only the minimum is
    // frozen, and the other fill shares the 5 left.
    assert_eq!(
        widths(&[
            fill.max(Size::new(10.0, MAX_EXTENT)),
            fill.min(Size::new(95.0, 0.0))
        ]),
        vec![5.0, 95.0]
    );
    // Shares of 50 clamp to 10 and 80, taking 10 away: only the maximum
    // is frozen, and the other fill takes the 90 left.
    assert_eq!(
        widths(&[
            fill.max(Size::new(10.0, MAX_EXTENT)),
            fill.min(Size::new(80.0, 0.0))
        ]),
        vec![10.0, 90.0]
    );
    // In either order.
    assert_eq!(
        widths(&[
            fill.min(Size::new(95.0, 0.0)),
            fill.max(Size::new(10.0, MAX_EXTENT))
        ]),
        vec![95.0, 5.0]
    );
}

#[test]
fn fills_of_no_weight_take_their_minimum() {
    let none = Style::default()
        .width(Length::Fill(0.0))
        .min(Size::new(50.0, 0.0));
    let (tree, ids) = laid_out(Style::row(), &[none], 300.0, 10.0);
    assert_eq!(bounds(&tree, &ids), vec![Rect::new(0.0, 0.0, 50.0, 0.0)]);
    // The same beside a fill with weight, and with a weight that is not
    // a number.
    let (tree, ids) = laid_out(
        Style::row(),
        &[
            none.width(Length::Fill(f32::NAN)),
            Style::default().width(Length::Fill(1.0)),
        ],
        300.0,
        10.0,
    );
    assert_eq!(
        bounds(&tree, &ids),
        vec![
            Rect::new(0.0, 0.0, 50.0, 0.0),
            Rect::new(50.0, 0.0, 250.0, 0.0),
        ]
    );
}

#[test]
fn alignment_places_children_in_the_free_space() {
    let child = [Style::default().fixed(100.0, 20.0)];
    let cases = [
        (Align::Start, Align::Start, Rect::new(0.0, 0.0, 100.0, 20.0)),
        (
            Align::Center,
            Align::Center,
            Rect::new(100.0, 40.0, 100.0, 20.0),
        ),
        (Align::End, Align::End, Rect::new(200.0, 80.0, 100.0, 20.0)),
    ];
    for (x, y, expected) in cases {
        for root in [Style::row(), Style::column(), Style::stack()] {
            let (tree, ids) = laid_out(root.align(x, y), &child, 300.0, 100.0);
            assert_eq!(bounds(&tree, &ids), vec![expected], "{root:?}");
        }
    }
}

#[test]
fn padding_insets_the_children() {
    let (tree, ids) = laid_out(
        Style::stack().padding(Insets::new(10.0, 20.0, 30.0, 40.0)),
        &[Style::default().fill()],
        200.0,
        100.0,
    );
    assert_eq!(
        bounds(&tree, &ids),
        vec![Rect::new(40.0, 10.0, 140.0, 60.0)]
    );
    // Padding wider than the node leaves an empty content box at its
    // left edge.
    let (tree, ids) = laid_out(
        Style::stack().padding(Insets::all(500.0)),
        &[Style::default().fill()],
        200.0,
        100.0,
    );
    assert_eq!(bounds(&tree, &ids), vec![Rect::new(200.0, 100.0, 0.0, 0.0)]);
}

#[test]
fn a_stack_places_each_child_alone() {
    let (tree, ids) = laid_out(
        Style::stack().align(Align::Center, Align::End),
        &[
            Style::default().fixed(50.0, 20.0),
            Style::default().fill(),
            Style::default().fixed(500.0, 500.0),
        ],
        200.0,
        100.0,
    );
    assert_eq!(
        bounds(&tree, &ids),
        vec![
            Rect::new(75.0, 80.0, 50.0, 20.0),
            Rect::new(0.0, 0.0, 200.0, 100.0),
            Rect::new(0.0, 0.0, 200.0, 100.0),
        ]
    );
}

#[test]
fn a_grid_has_equal_columns_and_rows_as_tall_as_their_tallest() {
    let cell = |height: f32| {
        Style::default()
            .width(Length::Fill(1.0))
            .height(Length::Fixed(height))
    };
    let (tree, ids) = laid_out(
        Style::grid(3).gap(10.0),
        &[cell(20.0), cell(30.0), cell(10.0), cell(40.0), cell(5.0)],
        320.0,
        200.0,
    );
    assert_eq!(
        bounds(&tree, &ids),
        vec![
            Rect::new(0.0, 0.0, 100.0, 20.0),
            Rect::new(110.0, 0.0, 100.0, 30.0),
            Rect::new(220.0, 0.0, 100.0, 10.0),
            Rect::new(0.0, 40.0, 100.0, 40.0),
            Rect::new(110.0, 40.0, 100.0, 5.0),
        ]
    );
    // Rows that do not fit are scaled to: 30 and 40 into 35, less the gap.
    let (tree, ids) = laid_out(
        Style::grid(3).gap(10.0),
        &[cell(20.0), cell(30.0), cell(10.0), cell(40.0), cell(5.0)],
        320.0,
        45.0,
    );
    let rows: Vec<f32> = bounds(&tree, &ids).iter().map(|rect| rect.y).collect();
    assert_eq!(rows, vec![0.0, 0.0, 0.0, 25.0, 25.0]);
}

#[test]
fn a_grid_sized_to_its_content_reserves_every_column() {
    let mut tree = Tree::new();
    let root = tree.root();
    tree.set_style(root, Style::stack()).expect("the root");
    let grid = tree
        .insert(root, Node::new(Style::grid(4).gap(5.0)))
        .expect("insert");
    let cells: Vec<NodeId> = (0..2)
        .map(|_| {
            tree.insert(grid, Node::new(Style::default().fixed(40.0, 10.0)))
                .expect("insert")
        })
        .collect();
    tree.layout(Size::new(640.0, 480.0), &METRICS);
    // Four columns of 40 and three gaps, though two are empty.
    assert_eq!(tree.bounds(grid), Some(Rect::new(0.0, 0.0, 175.0, 10.0)));
    assert_eq!(
        bounds(&tree, &cells),
        vec![
            Rect::new(0.0, 0.0, 40.0, 10.0),
            Rect::new(45.0, 0.0, 40.0, 10.0),
        ]
    );
    // With no visible child it takes no space.
    for &cell in &cells {
        tree.set_style(cell, Style::default().fixed(40.0, 10.0).hidden())
            .expect("hide");
    }
    tree.layout(Size::new(640.0, 480.0), &METRICS);
    assert_eq!(tree.bounds(grid), Some(Rect::ZERO));
}

#[test]
fn a_split_gives_its_first_child_the_ratio() {
    let fill = Style::default().fill();
    let (tree, ids) = laid_out(
        Style::split(Axis::Horizontal, 0.25),
        &[fill, fill, fill],
        400.0,
        100.0,
    );
    assert_eq!(
        bounds(&tree, &ids),
        vec![
            Rect::new(0.0, 0.0, 100.0, 100.0),
            Rect::new(100.0, 0.0, 150.0, 100.0),
            Rect::new(250.0, 0.0, 150.0, 100.0),
        ]
    );
    let (tree, ids) = laid_out(
        Style::split(Axis::Vertical, 0.5).gap(20.0),
        &[fill, fill],
        100.0,
        220.0,
    );
    assert_eq!(
        bounds(&tree, &ids),
        vec![
            Rect::new(0.0, 0.0, 100.0, 100.0),
            Rect::new(0.0, 120.0, 100.0, 100.0),
        ]
    );
    // The first child's limits hold.
    let (tree, ids) = laid_out(
        Style::split(Axis::Horizontal, 0.9),
        &[fill.max(Size::new(150.0, MAX_EXTENT)), fill],
        400.0,
        100.0,
    );
    assert_eq!(
        bounds(&tree, &ids),
        vec![
            Rect::new(0.0, 0.0, 150.0, 100.0),
            Rect::new(150.0, 0.0, 250.0, 100.0),
        ]
    );
}

#[test]
fn content_sizes_come_from_widgets_and_children() {
    let mut tree = Tree::new();
    let root = tree.root();
    let measured = tree
        .insert(
            root,
            Node::new(Style::default().padding(Insets::all(3.0)))
                .with_widget(Sized(Size::new(37.0, 12.0))),
        )
        .expect("insert");
    let row = tree
        .insert(root, Node::new(Style::row().gap(4.0)))
        .expect("insert");
    for size in [Size::new(10.0, 20.0), Size::new(30.0, 5.0)] {
        tree.insert(
            row,
            Node::new(Style::default().fixed(size.width, size.height)),
        )
        .expect("insert");
    }
    // A widget larger than its children wins on that axis.
    let both = tree
        .insert(
            root,
            Node::new(Style::row()).with_widget(Sized(Size::new(5.0, 50.0))),
        )
        .expect("insert");
    tree.insert(both, Node::new(Style::default().fixed(60.0, 10.0)))
        .expect("insert");
    tree.layout(Size::new(500.0, 500.0), &METRICS);
    assert_eq!(tree.bounds(measured), Some(Rect::new(0.0, 0.0, 43.0, 18.0)));
    assert_eq!(tree.bounds(row), Some(Rect::new(0.0, 18.0, 44.0, 20.0)));
    assert_eq!(tree.bounds(both), Some(Rect::new(0.0, 38.0, 60.0, 50.0)));
    // A widget's wild sizes are sanitized.
    let wild = tree
        .insert(
            root,
            Node::new(Style::default()).with_widget(Sized(Size::new(f32::NAN, f32::INFINITY))),
        )
        .expect("insert");
    tree.layout(Size::new(500.0, 500.0), &METRICS);
    assert_eq!(tree.bounds(wild).map(|rect| rect.width), Some(0.0));
}

#[test]
fn hidden_nodes_take_no_space() {
    let child = Style::default().fixed(50.0, 10.0);
    let mut tree = Tree::new();
    let root = tree.root();
    tree.set_style(root, Style::row().gap(10.0))
        .expect("the root");
    let a = tree.insert(root, Node::new(child)).expect("insert");
    let hidden = tree
        .insert(root, Node::new(child.hidden()))
        .expect("insert");
    let below = tree.insert(hidden, Node::new(child)).expect("insert");
    let c = tree.insert(root, Node::new(child)).expect("insert");
    tree.layout(Size::new(400.0, 50.0), &METRICS);
    assert_eq!(tree.bounds(a), Some(Rect::new(0.0, 0.0, 50.0, 10.0)));
    assert_eq!(tree.bounds(c), Some(Rect::new(60.0, 0.0, 50.0, 10.0)));
    assert_eq!(tree.bounds(hidden), Some(Rect::ZERO));
    assert_eq!(tree.bounds(below), Some(Rect::ZERO));
    // Shown again, it takes its place.
    tree.set_style(hidden, child).expect("show");
    tree.layout(Size::new(400.0, 50.0), &METRICS);
    assert_eq!(tree.bounds(hidden), Some(Rect::new(60.0, 0.0, 50.0, 10.0)));
    assert_eq!(tree.bounds(below), Some(Rect::new(60.0, 0.0, 50.0, 10.0)));
    assert_eq!(tree.bounds(c), Some(Rect::new(120.0, 0.0, 50.0, 10.0)));
}

#[test]
fn scrolling_lays_children_out_in_a_larger_area_and_clamps_the_offset() {
    let mut tree = Tree::new();
    let root = tree.root();
    let scroller = tree
        .insert(
            root,
            Node::new(Style::column().fixed(100.0, 100.0).scroll(false, true)),
        )
        .expect("insert");
    let rows: Vec<NodeId> = (0..5)
        .map(|_| {
            let row = Style::default()
                .width(Length::Fill(1.0))
                .height(Length::Fixed(50.0));
            tree.insert(scroller, Node::new(row)).expect("insert")
        })
        .collect();
    tree.set_scroll_offset(scroller, Point::new(30.0, 500.0))
        .expect("scroll");
    tree.layout(Size::new(300.0, 300.0), &METRICS);
    assert_eq!(tree.content_size(scroller), Some(Size::new(100.0, 250.0)));
    assert_eq!(tree.scroll_offset(scroller), Some(Point::new(0.0, 150.0)));
    for (i, &row) in rows.iter().enumerate() {
        let y = i as f32 * 50.0 - 150.0;
        assert_eq!(tree.bounds(row), Some(Rect::new(0.0, y, 100.0, 50.0)));
    }
    // Bringing the first row into view scrolls back to the top; the last
    // scrolls the least that shows it.
    tree.scroll_into_view(rows[0]).expect("in the tree");
    tree.layout(Size::new(300.0, 300.0), &METRICS);
    assert_eq!(tree.scroll_offset(scroller), Some(Point::new(0.0, 0.0)));
    tree.scroll_into_view(rows[3]).expect("in the tree");
    tree.layout(Size::new(300.0, 300.0), &METRICS);
    assert_eq!(tree.scroll_offset(scroller), Some(Point::new(0.0, 100.0)));
    assert_eq!(
        tree.bounds(rows[3]),
        Some(Rect::new(0.0, 50.0, 100.0, 50.0))
    );
    // Wild offsets are made sane, and clamped by the next layout.
    tree.set_scroll_offset(scroller, Point::new(f32::NAN, f32::INFINITY))
        .expect("scroll");
    tree.layout(Size::new(300.0, 300.0), &METRICS);
    assert_eq!(tree.scroll_offset(scroller), Some(Point::new(0.0, 150.0)));
}

#[test]
fn a_hidden_node_keeps_its_scroll_offset_until_it_is_shown() {
    let mut tree = Tree::new();
    let root = tree.root();
    let panel = Style::column().fixed(100.0, 100.0).scroll(false, true);
    let scroller = tree.insert(root, Node::new(panel)).expect("insert");
    let tall = Style::default()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(300.0));
    tree.insert(scroller, Node::new(tall)).expect("insert");
    tree.set_scroll_offset(scroller, Point::new(0.0, 120.0))
        .expect("scroll");
    tree.layout(Size::new(300.0, 300.0), &METRICS);
    tree.set_style(scroller, panel.hidden()).expect("hide");
    tree.layout(Size::new(300.0, 300.0), &METRICS);
    assert_eq!(tree.bounds(scroller), Some(Rect::ZERO));
    assert_eq!(tree.scroll_offset(scroller), Some(Point::new(0.0, 120.0)));
    tree.set_style(scroller, panel).expect("show");
    tree.layout(Size::new(300.0, 300.0), &METRICS);
    assert_eq!(tree.scroll_offset(scroller), Some(Point::new(0.0, 120.0)));
    assert_eq!(tree.content_size(scroller), Some(Size::new(100.0, 300.0)));
}

#[test]
fn scrolling_a_node_in_a_hidden_panel_into_view_keeps_the_panels_offset() {
    let mut tree = Tree::new();
    let root = tree.root();
    let panel = Style::column().fixed(100.0, 100.0).scroll(false, true);
    let scroller = tree.insert(root, Node::new(panel)).expect("insert");
    let tall = Style::default()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(300.0));
    let child = tree.insert(scroller, Node::new(tall)).expect("insert");
    tree.set_scroll_offset(scroller, Point::new(0.0, 120.0))
        .expect("scroll");
    tree.layout(Size::new(300.0, 300.0), &METRICS);
    tree.set_style(scroller, panel.hidden()).expect("hide");
    tree.layout(Size::new(300.0, 300.0), &METRICS);
    tree.scroll_into_view(child).expect("in the tree");
    tree.layout(Size::new(300.0, 300.0), &METRICS);
    assert_eq!(tree.scroll_offset(scroller), Some(Point::new(0.0, 120.0)));
    tree.set_style(scroller, panel).expect("show");
    tree.layout(Size::new(300.0, 300.0), &METRICS);
    assert_eq!(tree.scroll_offset(scroller), Some(Point::new(0.0, 120.0)));
    assert_eq!(
        tree.bounds(child),
        Some(Rect::new(0.0, -120.0, 100.0, 300.0))
    );

    // A request for a node removed before the layout is dropped, and one
    // for a node not in the tree is refused.
    let short = tree
        .insert(scroller, Node::new(tall.height(Length::Fixed(10.0))))
        .expect("insert");
    tree.layout(Size::new(300.0, 300.0), &METRICS);
    tree.scroll_into_view(short).expect("in the tree");
    tree.remove(short).expect("remove");
    tree.layout(Size::new(300.0, 300.0), &METRICS);
    assert_eq!(tree.scroll_offset(scroller), Some(Point::new(0.0, 120.0)));
    assert_eq!(
        tree.scroll_into_view(short),
        Err(UiError::UnknownNode { id: short })
    );
}

#[test]
fn gaps_larger_than_the_space_shrink_to_fit() {
    let child = Style::default().fixed(10.0, 10.0);
    let (tree, ids) = laid_out(Style::row().gap(200.0), &[child; 3], 100.0, 50.0);
    assert_eq!(
        bounds(&tree, &ids),
        vec![
            Rect::new(0.0, 0.0, 0.0, 10.0),
            Rect::new(50.0, 0.0, 0.0, 10.0),
            Rect::new(100.0, 0.0, 0.0, 10.0),
        ]
    );
}

#[test]
fn the_root_fills_the_window_whatever_its_lengths() {
    let (tree, _) = laid_out(Style::default().fixed(10.0, 10.0), &[], 640.0, 480.0);
    assert_eq!(
        tree.bounds(tree.root()),
        Some(Rect::new(0.0, 0.0, 640.0, 480.0))
    );
    let (tree, _) = laid_out(Style::default(), &[], f32::NAN, f32::INFINITY);
    assert_eq!(
        tree.bounds(tree.root()),
        Some(Rect::new(0.0, 0.0, 0.0, MAX_EXTENT))
    );
}

#[test]
fn a_hidden_root_still_fills_the_window() {
    let mut tree = Tree::new();
    let root = tree.root();
    tree.set_style(root, Style::row().hidden())
        .expect("the root");
    let child = tree
        .insert(root, Node::new(Style::default().fixed(50.0, 10.0)))
        .expect("insert");
    let below = tree
        .insert(child, Node::new(Style::default().fixed(5.0, 5.0)))
        .expect("insert");
    let frame = tree
        .frame(Size::new(640.0, 480.0), &METRICS)
        .expect("the first frame");
    assert_eq!(tree.bounds(root), Some(Rect::new(0.0, 0.0, 640.0, 480.0)));
    // Its subtree is laid out, painted and hit as a hidden one.
    assert_eq!(tree.bounds(child), Some(Rect::ZERO));
    assert_eq!(tree.bounds(below), Some(Rect::ZERO));
    assert!(frame.list.is_empty());
    assert_eq!(tree.hit_test(Point::new(10.0, 5.0)), None);
    // Shown again, it lays its children out.
    tree.set_style(root, Style::row()).expect("the root");
    tree.layout(Size::new(640.0, 480.0), &METRICS);
    assert_eq!(tree.bounds(root), Some(Rect::new(0.0, 0.0, 640.0, 480.0)));
    assert_eq!(tree.bounds(child), Some(Rect::new(0.0, 0.0, 50.0, 10.0)));
}
