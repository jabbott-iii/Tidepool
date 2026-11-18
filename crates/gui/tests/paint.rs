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

//! M3.5's damage, through the public API: random `ui` trees, changed from
//! frame to frame, painted at scales from 1 to 2. Each frame changes only
//! the pixels inside the damage `Painter::paint` returns, and painting the
//! damage alone gives the same pixels as painting the whole frame anew.

use tidepool_core::color::Color;
use tidepool_core::geom::{DevicePoint, DeviceRect, DeviceSize, Point, Rect, Scale, Size};
use tidepool_core::rng::Pcg32;
use tidepool_core::testing::check;
use tidepool_font::outline::OutlinePoint;
use tidepool_gfx::clip;
use tidepool_gfx::framebuffer::Framebuffer;
use tidepool_gui::{Icon, Painter};
use tidepool_ui::draw::{Command, Corners, DrawList, IconId, ImageId, PaintContext};
use tidepool_ui::layout::{Length, Style};
use tidepool_ui::metrics::{Family, TextStyle, Weight};
use tidepool_ui::{Node, NodeId, Tree, Widget};

/// The window, in logical units.
const WINDOW: Size = Size::new(64.0, 48.0);

const SCALES: [f32; 4] = [1.0, 1.25, 1.5, 2.0];

const TEXTS: [&str; 5] = ["Tidepool", "EXPLORER", "fn main()", "a\tb", "é\u{1F600}"];

/// Paints its commands, moved to its node's top-left corner unless they
/// are in window coordinates already.
struct Scene {
    commands: Vec<Command>,
    absolute: bool,
}

impl Widget for Scene {
    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let origin = cx.bounds().origin();
        for command in &self.commands {
            if self.absolute {
                list.push(command.clone());
            } else {
                list.push(moved(command, origin));
            }
        }
    }
}

/// `command` moved by `by`.
fn moved(command: &Command, by: Point) -> Command {
    let rect = |rect: &Rect| Rect::new(rect.x + by.x, rect.y + by.y, rect.width, rect.height);
    let point = |point: &Point| Point::new(point.x + by.x, point.y + by.y);
    let mut command = command.clone();
    match &mut command {
        Command::Rect { rect: r, .. }
        | Command::CutCornerRect { rect: r, .. }
        | Command::Pill { rect: r, .. }
        | Command::Border { rect: r, .. }
        | Command::Brackets { rect: r, .. }
        | Command::Icon { rect: r, .. }
        | Command::Image { rect: r, .. }
        | Command::PushClip { rect: r } => *r = rect(r),
        Command::Line { from, to, .. } => {
            *from = point(from);
            *to = point(to);
        }
        Command::Text { origin, .. } => *origin = point(origin),
        Command::PopClip | Command::PushLayer { .. } | Command::PopLayer => {}
    }
    command
}

/// One change between frames.
#[derive(Debug)]
enum Change {
    /// The node at this index (modulo the count) paints these instead.
    Repaint(usize, Vec<Command>),
    /// The node takes this style.
    Restyle(usize, Style),
    /// The node is removed.
    Remove(usize),
    /// A node is added, under the root or under another node.
    Insert(Option<usize>, Style, Vec<Command>),
    /// Nothing changes: an idle frame.
    Nothing,
}

/// A node of a case's first frame: under the root or another node, its
/// style, and what its scene paints, in window coordinates or its own.
#[derive(Debug)]
struct Spec {
    parent: Option<usize>,
    style: Style,
    commands: Vec<Command>,
    absolute: bool,
}

/// A case: the scale, the first nodes, and the changes of each frame after
/// the first.
#[derive(Debug)]
struct Case {
    scale: f32,
    nodes: Vec<Spec>,
    frames: Vec<Vec<Change>>,
}

fn color(rng: &mut Pcg32) -> Color {
    let alpha = *rng.choose(&[255, 255, 200, 128, 40]).unwrap_or(&255);
    Color::rgba(
        rng.range(0..256u32) as u8,
        rng.range(0..256u32) as u8,
        rng.range(0..256u32) as u8,
        alpha,
    )
}

/// A coordinate near a node, often on a fraction of a unit.
fn coordinate(rng: &mut Pcg32) -> f32 {
    let whole = rng.range(-8..40i32) as f32;
    match rng.range(0..4u32) {
        0 => whole,
        1 => whole + 0.5,
        _ => whole + rng.range(0.0..1.0f32),
    }
}

fn rect(rng: &mut Pcg32) -> Rect {
    Rect::new(
        coordinate(rng),
        coordinate(rng),
        rng.range(0.0..30.0f32),
        rng.range(0.0..24.0f32),
    )
}

fn corners(rng: &mut Pcg32) -> Corners {
    *rng.choose(&[
        Corners::ALL,
        Corners::NONE,
        Corners::TOP_LEFT_AND_BOTTOM_RIGHT,
        Corners::TOP_RIGHT_AND_BOTTOM_LEFT,
    ])
    .unwrap_or(&Corners::ALL)
}

fn text_style(rng: &mut Pcg32) -> TextStyle {
    let family = *rng
        .choose(&[Family::Ui, Family::Mono])
        .unwrap_or(&Family::Ui);
    let weight = *rng
        .choose(&[
            Weight::Regular,
            Weight::Medium,
            Weight::SemiBold,
            Weight::Bold,
        ])
        .unwrap_or(&Weight::Regular);
    let mut style = TextStyle::new(family, rng.range(6.0..30.0f32))
        .weight(weight)
        .letter_spacing(rng.range(-0.3..0.5f32));
    if rng.range(0..4u32) == 0 {
        style = style.italic();
    }
    style
}

/// Commands of every kind, with clips and layers pushed and popped, not
/// always in pairs.
fn commands(rng: &mut Pcg32) -> Vec<Command> {
    let count = rng.range(0..7u32);
    let mut commands = Vec::new();
    for _ in 0..count {
        let command = match rng.range(0..13u32) {
            0 => Command::Rect {
                rect: rect(rng),
                color: color(rng),
            },
            1 => Command::CutCornerRect {
                rect: rect(rng),
                cut: rng.range(0.0..12.0f32),
                corners: corners(rng),
                color: color(rng),
            },
            2 => Command::Pill {
                rect: rect(rng),
                color: color(rng),
            },
            3 => Command::Border {
                rect: rect(rng),
                width: rng.range(0.0..3.0f32),
                color: color(rng),
            },
            4 => Command::Brackets {
                rect: rect(rng),
                arm: rng.range(0.0..12.0f32),
                thickness: rng.range(0.0..3.0f32),
                corners: corners(rng),
                color: color(rng),
            },
            5 => Command::Line {
                from: Point::new(coordinate(rng), coordinate(rng)),
                to: Point::new(coordinate(rng), coordinate(rng)),
                width: rng.range(0.0..4.0f32),
                color: color(rng),
            },
            6 | 7 => Command::Text {
                origin: Point::new(coordinate(rng), coordinate(rng)),
                text: (*rng.choose(&TEXTS).unwrap_or(&"x")).to_string(),
                style: text_style(rng),
                color: color(rng),
            },
            8 => Command::Icon {
                rect: rect(rng),
                icon: IconId(rng.range(0..2u32) as u16),
                color: color(rng),
            },
            9 => Command::Image {
                rect: rect(rng),
                image: ImageId(rng.range(0..2u32)),
            },
            10 => Command::PushClip { rect: rect(rng) },
            11 => Command::PushLayer {
                opacity: *rng.choose(&[0.0, 0.3, 0.5, 1.0]).unwrap_or(&0.5),
            },
            _ if rng.range(0..2u32) == 0 => Command::PopClip,
            _ => Command::PopLayer,
        };
        commands.push(command);
    }
    commands
}

fn style(rng: &mut Pcg32) -> Style {
    let mut style = Style::default().fixed(rng.range(4.0..40.0f32), rng.range(4.0..30.0f32));
    if rng.range(0..3u32) == 0 {
        style = style.clip();
    }
    if rng.range(0..4u32) == 0 {
        style = style.opacity(*rng.choose(&[0.0, 0.4, 0.75]).unwrap_or(&0.5));
    }
    if rng.range(0..12u32) == 0 {
        style = style.hidden();
    }
    if rng.range(0..3u32) == 0 {
        style = style.width(Length::Fill(1.0));
    }
    style
}

fn parent(rng: &mut Pcg32) -> Option<usize> {
    (rng.range(0..3u32) == 0).then(|| rng.range(0..8u32) as usize)
}

fn case(rng: &mut Pcg32) -> Case {
    let scale = *rng.choose(&SCALES).unwrap_or(&1.0);
    let nodes = (0..rng.range(1..6u32))
        .map(|_| Spec {
            parent: parent(rng),
            style: style(rng),
            commands: commands(rng),
            absolute: false,
        })
        .collect();
    let frames = (0..rng.range(1..6u32))
        .map(|_| {
            (0..rng.range(1..4u32))
                .map(|_| match rng.range(0..9u32) {
                    0..=2 => Change::Repaint(rng.range(0..8u32) as usize, commands(rng)),
                    3 | 4 => Change::Restyle(rng.range(0..8u32) as usize, style(rng)),
                    5 => Change::Remove(rng.range(0..8u32) as usize),
                    6 | 7 => Change::Insert(parent(rng), style(rng), commands(rng)),
                    _ => Change::Nothing,
                })
                .collect()
        })
        .collect();
    Case {
        scale,
        nodes,
        frames,
    }
}

/// Adds a node as `spec` says to `tree`, and its id to `ids`, the nodes in
/// the tree in the order they were made.
fn insert(tree: &mut Tree, ids: &mut Vec<NodeId>, spec: &Spec) {
    let root = tree.root();
    let parent = spec
        .parent
        .and_then(|index| ids.get(index % ids.len().max(1)).copied())
        .unwrap_or(root);
    let scene = Scene {
        commands: spec.commands.clone(),
        absolute: spec.absolute,
    };
    if let Ok(id) = tree.insert(parent, Node::new(spec.style).with_widget(scene)) {
        ids.push(id);
    }
}

fn apply(tree: &mut Tree, ids: &mut Vec<NodeId>, change: &Change) {
    let at = |index: usize, ids: &[NodeId]| (!ids.is_empty()).then(|| ids[index % ids.len()]);
    match change {
        Change::Repaint(index, commands) => {
            if let Some(id) = at(*index, ids)
                && let Some(scene) = tree.widget_mut::<Scene>(id)
            {
                scene.commands.clone_from(commands);
            }
        }
        Change::Restyle(index, style) => {
            if let Some(id) = at(*index, ids) {
                let _ = tree.set_style(id, *style);
            }
        }
        Change::Remove(index) => {
            if let Some(id) = at(*index, ids) {
                let _ = tree.remove(id);
                ids.retain(|kept| tree.contains(*kept));
            }
        }
        Change::Insert(under, style, commands) => {
            let spec = Spec {
                parent: *under,
                style: *style,
                commands: commands.clone(),
                absolute: false,
            };
            insert(tree, ids, &spec);
        }
        Change::Nothing => {}
    }
}

/// A base that is not one color, so that a pixel restored from the wrong
/// place shows.
fn base(size: DeviceSize) -> Framebuffer {
    let mut base = Framebuffer::new(size).expect("a base");
    let clip = base.bounds();
    for y in 0..size.height {
        for x in 0..size.width {
            let color = Color::rgb((x * 3) as u8, (y * 5) as u8, ((x + y) * 2) as u8);
            tidepool_gfx::raster::fill_rect(
                &mut base,
                clip,
                DeviceRect::new(x, y, 1, 1),
                color.premultiply(),
            );
        }
    }
    base
}

fn painter() -> Painter {
    let mut painter = Painter::new().expect("the bundled faces load");
    let on = |x, y| OutlinePoint {
        x,
        y,
        on_curve: true,
    };
    let curve = |x, y| OutlinePoint {
        x,
        y,
        on_curve: false,
    };
    let points = vec![
        on(1.0, 8.0),
        curve(8.0, 0.0),
        on(15.0, 8.0),
        curve(8.0, 16.0),
    ];
    painter.set_icon(
        IconId(0),
        Icon::new(16.0, points, vec![4]).expect("an icon"),
    );
    let mut image = Framebuffer::new(DeviceSize::new(3, 2)).expect("an image");
    image.clear(Color::rgba(200, 40, 90, 180).premultiply());
    painter.set_image(ImageId(0), image);
    painter
}

/// Paints each frame of `case`, and checks that only the damage changed
/// and that the damage was enough.
fn frames_paint_only_their_damage(case: &Case) -> Result<(), String> {
    let mut painter = painter();
    let scale = Scale::new(case.scale).ok_or("a scale")?;
    let size = DeviceSize::new(
        (WINDOW.width * case.scale).ceil() as i32,
        (WINDOW.height * case.scale).ceil() as i32,
    );
    let base = base(size);
    // The tree painted frame by frame, and a shadow given the same changes
    // and painted whole after each, idle frames too: a frame that damages
    // nothing must leave nothing stale.
    let (mut tree, mut ids) = (Tree::new(), Vec::new());
    let (mut shadow, mut shadow_ids) = (Tree::new(), Vec::new());
    for spec in &case.nodes {
        insert(&mut tree, &mut ids, spec);
        insert(&mut shadow, &mut shadow_ids, spec);
    }
    let mut target = Framebuffer::new(size).map_err(|error| error.to_string())?;
    let mut changes: Vec<&[Change]> = vec![&[]];
    changes.extend(case.frames.iter().map(Vec::as_slice));
    for (index, frame_changes) in changes.into_iter().enumerate() {
        for change in frame_changes {
            apply(&mut tree, &mut ids, change);
            apply(&mut shadow, &mut shadow_ids, change);
        }
        let mut damage = None;
        if let Some(frame) = tree.frame(WINDOW, &painter.metrics(scale)) {
            let before = target.clone();
            let painted = painter
                .paint(&mut target, &base.view(), &frame, scale)
                .map_err(|error| error.to_string())?;
            // Only the damage changed.
            for y in 0..size.height {
                for x in 0..size.width {
                    let at = DevicePoint::new(x, y);
                    let inside = painted.rects().iter().any(|rect| {
                        clip::intersect(*rect, DeviceRect::new(x, y, 1, 1)) != DeviceRect::ZERO
                    });
                    if !inside && target.pixel(at) != before.pixel(at) {
                        return Err(format!(
                            "frame {index}: ({x}, {y}) changed outside {painted:?}"
                        ));
                    }
                }
            }
            damage = Some(painted);
        }
        // The damage was enough: the shadow, painted whole, gives the same
        // pixels.
        shadow.invalidate_all();
        let whole = shadow
            .frame(WINDOW, &painter.metrics(scale))
            .ok_or("the shadow damages the window")?;
        let mut fresh = Framebuffer::new(size).map_err(|error| error.to_string())?;
        painter
            .paint(&mut fresh, &base.view(), &whole, scale)
            .map_err(|error| error.to_string())?;
        if fresh != target {
            let first = (0..size.height)
                .flat_map(|y| (0..size.width).map(move |x| DevicePoint::new(x, y)))
                .find(|at| fresh.pixel(*at) != target.pixel(*at));
            return Err(format!(
                "frame {index}: painting only the damage ({damage:?}) differs from painting \
                 the whole frame, first at {first:?}"
            ));
        }
    }
    Ok(())
}

#[test]
fn a_frame_changes_only_its_damage_and_the_damage_is_enough() {
    check(
        "a_frame_changes_only_its_damage_and_the_damage_is_enough",
        300,
        case,
        frames_paint_only_their_damage,
    );
}

/// Where a [`sliver_case`]'s clip has its edge, in device pixels: the
/// clipping node starts there (`before`, its line ends before it) or ends
/// there (its line starts after it), across the window or down it.
#[derive(Clone, Copy, Debug)]
struct Edge {
    at: f32,
    before: bool,
    vertical: bool,
}

/// A line whose anti-aliased ink reaches into the pixel of `edge`, past
/// the clip, while its bounds end before it: under a device pixel wide,
/// its end within half a pixel of the edge. `ui` damages nothing for it,
/// so its ink there must not be drawn. In window coordinates, logical.
fn sliver(rng: &mut Pcg32, edge: Edge, scale: f32) -> Command {
    let k = edge.at.floor();
    let clip = edge.at - k;
    let (end, width) = if edge.before {
        let end = rng.range(0.02..clip - 0.05);
        (k + end, rng.range(0.02..2.0 * (clip - end) - 0.02))
    } else {
        let start = rng.range(clip + 0.05..0.98);
        (k + start, rng.range(0.02..2.0 * (start - clip) - 0.02))
    };
    let reach = rng.range(4.0..20.0f32);
    let (from, to) = if edge.before {
        (end - reach, end)
    } else {
        (end, end + reach)
    };
    let across = rng.range(2.0..40.0f32);
    let logical = |device: f32| device / scale;
    let point = |along: f32| {
        if edge.vertical {
            Point::new(logical(across), logical(along))
        } else {
            Point::new(logical(along), logical(across))
        }
    };
    Command::Line {
        from: point(from),
        to: point(to),
        width: logical(width),
        color: color(rng),
    }
}

/// Cases of [`sliver`]s: a row or column filling the window, a spacer, a
/// clipping node with an edge on a fraction of a device pixel, and in it a
/// node whose scene paints slivers past that edge; then the slivers
/// repainted, cleared or removed. `ui` damages nothing for them, so those
/// frames are idle, and the whole repaint after each finds any sliver ink
/// left behind.
fn sliver_case(rng: &mut Pcg32) -> Case {
    let scale = *rng.choose(&SCALES).unwrap_or(&1.0);
    let k = rng.range(8..40i32) as f32;
    let before = rng.range(0..2u32) == 0;
    let fraction = if before {
        rng.range(0.3..0.49f32)
    } else {
        rng.range(0.51..0.7f32)
    };
    let edge = Edge {
        at: k + fraction,
        before,
        vertical: rng.range(0..2u32) == 0,
    };
    let logical = edge.at / scale;
    let along = |length: f32| {
        if edge.vertical {
            Style::default().fixed(WINDOW.width, length)
        } else {
            Style::default().fixed(length, WINDOW.height)
        }
    };
    let container = if edge.vertical {
        Style::column()
    } else {
        Style::row()
    };
    // The clipper takes what the spacer leaves, so that nothing shrinks
    // and its edge stays where the slivers are aimed.
    let rest = if edge.vertical {
        Style::default()
            .width(Length::Fixed(WINDOW.width))
            .height(Length::Fill(1.0))
    } else {
        Style::default()
            .width(Length::Fill(1.0))
            .height(Length::Fixed(WINDOW.height))
    };
    let (spacer, clipper) = if before {
        (along(logical), rest.clip())
    } else {
        (along(0.0), along(logical).clip())
    };
    let node = |parent, style, commands, absolute| Spec {
        parent,
        style,
        commands,
        absolute,
    };
    let slivers = |rng: &mut Pcg32| {
        (0..rng.range(1..4u32))
            .map(|_| sliver(rng, edge, scale))
            .collect()
    };
    let nodes = vec![
        node(
            None,
            container.fixed(WINDOW.width, WINDOW.height),
            Vec::new(),
            false,
        ),
        node(Some(0), spacer, Vec::new(), false),
        node(Some(0), clipper, Vec::new(), false),
        node(
            Some(2),
            Style::default().fixed(1.0, 1.0),
            slivers(rng),
            true,
        ),
    ];
    // Every change is to the slivers' node, and none follows its removal,
    // so that an index never names another node.
    let mut frames = Vec::new();
    for _ in 0..rng.range(1..4u32) {
        let change = match rng.range(0..4u32) {
            0 | 1 => Change::Repaint(3, slivers(rng)),
            2 => Change::Repaint(3, Vec::new()),
            _ => Change::Remove(3),
        };
        let removed = matches!(change, Change::Remove(_));
        frames.push(vec![change]);
        if removed {
            break;
        }
    }
    Case {
        scale,
        nodes,
        frames,
    }
}

#[test]
fn ink_beyond_a_commands_bounds_in_its_clip_is_never_drawn() {
    check(
        "ink_beyond_a_commands_bounds_in_its_clip_is_never_drawn",
        300,
        sliver_case,
        frames_paint_only_their_damage,
    );
}
