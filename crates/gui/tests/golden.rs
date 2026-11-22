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

//! The golden scenes of `ui`'s draw lists, painted by `tidepool-gui`
//! (ADR-0044, ADR-0048, `docs/04-quality.md` → "Golden frames"): one test
//! per scene, named after it, for each draw command, clips and layers
//! included, at scale 1 and 2, and a mix of them at 1.5; in
//! [`controls`], for each of `ui`'s controls in each state at scale 1 and 2
//! (ADR-0049); in [`components`], for each of M3.7's components, the
//! list, tree view, table, tab strip, splitter, card and code block, in
//! each state at scale 1 and 2 (ADR-0052); and in [`overlays`], for the
//! modal dialog over a window it makes inert, the tooltip, the toast and
//! the command palette, in each state at scale 1 and 2 (M3.3, ADR-0052).
//!
//! Each scene is a `ui` tree whose widgets paint commands in window
//! coordinates, laid out and painted by [`Painter`] over the bundled
//! theme's `bg`, in the theme's colors. When [`DIR_VAR`] names a
//! directory, it writes what it painted there as `<scene>.pam`; `cargo
//! xtask golden` sets it, and compares each file with
//! `tests/golden/<scene>.qoi`, exactly. Unset, as in `cargo test`, the
//! scenes paint and write nothing.
//!
//! The PAM file holds the framebuffer's premultiplied bytes in RGBA order,
//! unchanged, rows from the top: `xtask`'s reader (`xtask/src/golden.rs`)
//! is the other copy of this format. Every number here is a literal, or
//! integer arithmetic or IEEE 754 `+ − × ÷` on literals, which every CPU
//! rounds the same, and never a libm function such as `cos`: so the scenes
//! are the same on every OS.

use std::env;
use std::fs;
use std::path::PathBuf;

use tidepool_core::color::Color;
use tidepool_core::geom::{DeviceSize, Point, Rect, Scale, Size};
use tidepool_font::outline::OutlinePoint;
use tidepool_gfx::framebuffer::Framebuffer;
use tidepool_gui::{Icon, Painter};
use tidepool_ui::draw::{Command, Corners, DrawList, IconId, ImageId, PaintContext};
use tidepool_ui::layout::Style;
use tidepool_ui::metrics::{Family, TextStyle, Weight};
use tidepool_ui::theme::{Theme, Token};
use tidepool_ui::{Node, NodeId, Tree, Widget};

// The controls' scenes (M3.6), the components' (M3.7) and the overlays'
// (M3.3), in modules of this target, so that `cargo xtask golden` runs
// them with the rest.
mod components;
mod controls;
mod overlays;

/// Names the directory the scenes write to.
const DIR_VAR: &str = "TIDEPOOL_GOLDEN_DIR";

/// The size of every scene but the text's, in logical units.
const SIZE: Size = Size::new(160.0, 96.0);

/// Paints its commands as they are, in window coordinates.
struct Raw(Vec<Command>);

impl Widget for Raw {
    fn paint(&self, _cx: &PaintContext<'_>, list: &mut DrawList) {
        for command in &self.0 {
            list.push(command.clone());
        }
    }
}

/// A token's color in the bundled theme.
fn token(token: Token) -> Color {
    Theme::tidepool_dark().color(token)
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect::new(x, y, width, height)
}

fn fill(rect: Rect, color: Token) -> Command {
    Command::Rect {
        rect,
        color: token(color),
    }
}

fn on(x: f32, y: f32) -> OutlinePoint {
    OutlinePoint {
        x,
        y,
        on_curve: true,
    }
}

fn off(x: f32, y: f32) -> OutlinePoint {
    OutlinePoint {
        x,
        y,
        on_curve: false,
    }
}

/// A circle of 8 quadratic curves round (12, 12) in a 24-unit viewbox,
/// `r` 10 or 6 units: its points on the curve at every 45°, and its
/// control points where the tangents meet, as literals (`r` and
/// `r × tan 22.5°`), so that no libm function is called.
fn circle(r: f32, t: f32, d: f32) -> Vec<OutlinePoint> {
    // `d` is `r × cos 45°`; `t` is `r × tan 22.5°`.
    let c = 12.0;
    vec![
        on(c + r, c),
        off(c + r, c + t),
        on(c + d, c + d),
        off(c + t, c + r),
        on(c, c + r),
        off(c - t, c + r),
        on(c - d, c + d),
        off(c - r, c + t),
        on(c - r, c),
        off(c - r, c - t),
        on(c - d, c - d),
        off(c - t, c - r),
        on(c, c - r),
        off(c + t, c - r),
        on(c + d, c - d),
        off(c + r, c - t),
    ]
}

/// A painter with the scenes' icons and images registered: a ring (two
/// circles, the inner winding the other way), a check mark, a play
/// triangle and a folder, each in a 24-unit viewbox; and an 8 × 8 pattern
/// and a 4 × 4 translucent image.
fn painter() -> Painter {
    let mut painter = Painter::new().expect("the bundled faces load");
    let mut ring = circle(10.0, 4.142_136, 7.071_068);
    let mut hole = circle(6.0, 2.485_281, 4.242_641);
    hole.reverse();
    ring.extend(hole);
    let icons = [
        (ring, vec![16, 32]),
        (
            vec![
                on(3.0, 12.0),
                on(6.0, 9.0),
                on(10.0, 13.0),
                on(18.0, 5.0),
                on(21.0, 8.0),
                on(10.0, 19.0),
            ],
            vec![6],
        ),
        (vec![on(7.0, 4.0), on(19.0, 12.0), on(7.0, 20.0)], vec![3]),
        (
            vec![
                on(2.0, 5.0),
                on(9.0, 5.0),
                on(11.0, 7.0),
                on(22.0, 7.0),
                on(22.0, 19.0),
                on(2.0, 19.0),
            ],
            vec![6],
        ),
    ];
    for (id, (points, ends)) in (0..).zip(icons) {
        let icon = Icon::new(24.0, points, ends).expect("an icon");
        painter.set_icon(IconId(id), icon);
    }
    let mut pattern = Framebuffer::new(DeviceSize::new(8, 8)).expect("an image");
    for y in 0..8u8 {
        for x in 0..8u8 {
            let color = if (x / 2 + y / 2) % 2 == 0 {
                Color::rgb(0x5c, 0xcb, 0xcb)
            } else {
                Color::rgb(32 * x, 0xc2, 32 * y)
            };
            fill_pixel(&mut pattern, i32::from(x), i32::from(y), color);
        }
    }
    painter.set_image(ImageId(0), pattern);
    let mut glass = Framebuffer::new(DeviceSize::new(4, 4)).expect("an image");
    for y in 0..4u8 {
        for x in 0..4u8 {
            fill_pixel(
                &mut glass,
                i32::from(x),
                i32::from(y),
                Color::rgba(255, 255, 255, 40 + 50 * x),
            );
        }
    }
    painter.set_image(ImageId(1), glass);
    painter
}

fn fill_pixel(image: &mut Framebuffer, x: i32, y: i32, color: Color) {
    let clip = image.bounds();
    tidepool_gfx::raster::fill_rect(
        image,
        clip,
        tidepool_core::geom::DeviceRect::new(x, y, 1, 1),
        color.premultiply(),
    );
}

/// A tree of one node over the window, painting `commands`.
fn flat(commands: Vec<Command>, size: Size) -> Tree {
    let mut tree = Tree::new();
    let root = tree.root();
    tree.insert(
        root,
        Node::new(Style::default().fixed(size.width, size.height)).with_widget(Raw(commands)),
    )
    .expect("a node");
    tree
}

/// Lays `tree` out in a window of `size` logical units, paints it at
/// `factor` over the theme's `bg`, and writes it as `name`.
fn render(name: &str, size: Size, factor: f32, tree: &mut Tree) {
    let scale = Scale::new(factor).expect("a scale");
    let mut painter = painter();
    let frame = tree
        .frame(size, &painter.metrics(scale))
        .expect("the first frame damages the window");
    let device = DeviceSize::new((size.width * factor) as i32, (size.height * factor) as i32);
    let mut base = Framebuffer::new(device).expect("a base");
    base.clear(token(Token::Bg).premultiply());
    let mut target = Framebuffer::new(device).expect("a window");
    let damage = painter
        .paint(&mut target, &base.view(), &frame, scale)
        .expect("painted");
    assert_eq!(
        damage.rects(),
        [target.bounds()],
        "the first frame is whole"
    );
    assert_ne!(target, base, "the scene drew something");
    assert!(
        tree.frame(size, &painter.metrics(scale)).is_none(),
        "the scene is idle after it"
    );
    save(name, &target);
}

/// Writes `frame` as `<name>.pam` into the directory [`DIR_VAR`] names, if
/// it names one.
fn save(name: &str, frame: &Framebuffer) {
    let Some(dir) = env::var_os(DIR_VAR).filter(|dir| !dir.is_empty()) else {
        return;
    };
    let path = PathBuf::from(dir).join(format!("{name}.pam"));
    fs::write(&path, pam(frame)).unwrap_or_else(|error| {
        panic!("cannot write {}: {error}", path.display());
    });
}

/// `frame` as a PAM file: RGBA, rows from the top, the premultiplied bytes
/// unchanged.
fn pam(frame: &Framebuffer) -> Vec<u8> {
    let size = frame.size();
    let mut file = format!(
        "P7\nWIDTH {}\nHEIGHT {}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n",
        size.width, size.height
    )
    .into_bytes();
    let row_bytes = frame.stride();
    for row in frame.bytes().chunks_exact(row_bytes) {
        for &[b, g, r, a] in row.as_chunks::<4>().0 {
            file.extend_from_slice(&[r, g, b, a]);
        }
    }
    file
}

/// Fills: opaque, on fractions of a unit, sharing fractional edges, and
/// translucent over others; one partly outside the window.
fn rects() -> Vec<Command> {
    vec![
        fill(rect(8.0, 8.0, 64.0, 40.0), Token::Slate),
        fill(rect(80.0, 8.0, 72.0, 40.0), Token::PanelSolid),
        fill(rect(40.0, 22.0, 80.0, 14.0), Token::TealGlow),
        fill(rect(100.0, 28.0, 44.0, 26.0), Token::Panel),
        fill(rect(8.25, 56.0, 20.5, 8.0), Token::Teal),
        fill(rect(40.5, 56.0, 20.5, 8.0), Token::Teal),
        fill(rect(72.75, 56.0, 20.5, 8.0), Token::Teal),
        fill(rect(104.0, 56.0, 0.4, 8.0), Token::Error),
        fill(rect(8.0, 72.0, 48.333, 16.0), Token::Teal),
        fill(rect(56.333, 72.0, 48.333, 16.0), Token::Amber),
        fill(rect(104.666, 72.0, 48.334, 16.0), Token::Green),
        fill(rect(-10.0, 90.0, 30.0, 20.0), Token::Error),
    ]
}

fn cut(rect: Rect, cut: f32, corners: Corners, color: Token) -> Command {
    Command::CutCornerRect {
        rect,
        cut,
        corners,
        color: token(color),
    }
}

/// Cut-corner rects: the amber button's 10-unit cuts, every corner, the
/// other diagonal, a cut too long for its rect, a translucent one over
/// another, and one on fractions.
fn cut_corner_rects() -> Vec<Command> {
    vec![
        cut(
            rect(8.0, 8.0, 72.0, 24.0),
            10.0,
            Corners::TOP_RIGHT_AND_BOTTOM_LEFT,
            Token::Amber,
        ),
        cut(rect(88.0, 8.0, 64.0, 24.0), 6.0, Corners::ALL, Token::Slate),
        cut(
            rect(8.0, 40.0, 40.0, 20.0),
            4.0,
            Corners::TOP_LEFT_AND_BOTTOM_RIGHT,
            Token::Teal,
        ),
        cut(
            rect(56.0, 40.0, 30.0, 20.0),
            40.0,
            Corners::ALL,
            Token::TealHi,
        ),
        cut(
            rect(96.0, 40.0, 56.0, 40.0),
            10.0,
            Corners::TOP_RIGHT_AND_BOTTOM_LEFT,
            Token::Amber,
        ),
        cut(
            rect(104.0, 48.0, 56.0, 40.0),
            10.0,
            Corners::TOP_RIGHT_AND_BOTTOM_LEFT,
            Token::TealGlow,
        ),
        cut(
            rect(8.5, 68.25, 60.5, 20.0),
            7.5,
            Corners::ALL,
            Token::AmberHi,
        ),
    ]
}

fn pill(rect: Rect, color: Token) -> Command {
    Command::Pill {
        rect,
        color: token(color),
    }
}

/// Pills: chips, a translucent one, status dots, an upright one, and one
/// on fractions.
fn pills() -> Vec<Command> {
    vec![
        pill(rect(8.0, 8.0, 64.0, 20.0), Token::Slate),
        pill(rect(80.0, 8.0, 48.0, 20.0), Token::Teal),
        pill(rect(8.0, 34.0, 72.0, 20.0), Token::TealWash),
        pill(rect(40.0, 40.0, 60.0, 20.0), Token::TealGlow),
        pill(rect(136.0, 12.0, 10.0, 10.0), Token::Green),
        pill(rect(136.0, 30.0, 10.0, 10.0), Token::Amber),
        pill(rect(136.0, 48.0, 10.0, 10.0), Token::Error),
        pill(rect(8.0, 62.0, 6.0, 6.0), Token::Green),
        pill(rect(108.0, 34.0, 14.0, 54.0), Token::Violet),
        pill(rect(20.5, 70.25, 60.5, 17.5), Token::Rust),
    ]
}

fn border(rect: Rect, width: f32, color: Token) -> Command {
    Command::Border {
        rect,
        width,
        color: token(color),
    }
}

/// Borders: a panel's 1-unit line, a 2-unit focus outline, half a unit
/// (one device pixel), 3 units, a translucent one, and 1.5 units on
/// fractions (one device pixel at scale 1, three at 2).
fn borders() -> Vec<Command> {
    vec![
        fill(rect(8.0, 8.0, 70.0, 40.0), Token::PanelSolid),
        border(rect(8.0, 8.0, 70.0, 40.0), 1.0, Token::Line2),
        border(rect(86.0, 8.0, 66.0, 40.0), 2.0, Token::Teal),
        border(rect(8.0, 56.0, 40.0, 32.0), 0.5, Token::Text3),
        border(rect(56.0, 56.0, 40.0, 32.0), 3.0, Token::Amber),
        border(rect(104.0, 56.0, 48.0, 32.0), 1.0, Token::Line),
        border(rect(108.5, 60.25, 40.5, 20.5), 1.5, Token::TealHi),
    ]
}

fn brackets(rect: Rect, arm: f32, thickness: f32, corners: Corners, color: Token) -> Command {
    Command::Brackets {
        rect,
        arm,
        thickness,
        corners,
        color: token(color),
    }
}

/// Brackets: a focused panel's (16-unit arms, 1 thick, top-left and
/// bottom-right), every corner, arms too long for their rect, a translucent
/// set, and a set on fractions half a unit thick.
fn bracket_sets() -> Vec<Command> {
    vec![
        fill(rect(8.0, 8.0, 70.0, 50.0), Token::PanelSolid),
        brackets(
            rect(8.0, 8.0, 70.0, 50.0),
            16.0,
            1.0,
            Corners::TOP_LEFT_AND_BOTTOM_RIGHT,
            Token::Teal,
        ),
        brackets(
            rect(86.0, 8.0, 66.0, 50.0),
            8.0,
            2.0,
            Corners::ALL,
            Token::Amber,
        ),
        brackets(
            rect(8.0, 66.0, 20.0, 22.0),
            16.0,
            3.0,
            Corners::ALL,
            Token::TealHi,
        ),
        brackets(
            rect(36.0, 66.0, 60.0, 22.0),
            10.0,
            1.0,
            Corners::ALL,
            Token::TealGlow,
        ),
        brackets(
            rect(104.5, 66.5, 48.0, 22.0),
            7.5,
            0.5,
            Corners::TOP_RIGHT_AND_BOTTOM_LEFT,
            Token::Text2,
        ),
    ]
}

fn line(from: (f32, f32), to: (f32, f32), width: f32, color: Token) -> Command {
    Command::Line {
        from: Point::new(from.0, from.1),
        to: Point::new(to.0, to.1),
        width,
        color: token(color),
    }
}

/// Lines: diagonals 0.5, 1 and 2 wide; level and upright ones on a whole
/// unit and between two; a translucent star; one leaving the window.
fn lines() -> Vec<Command> {
    let mut commands = vec![
        line((8.0, 8.0), (72.0, 40.0), 0.5, Token::Text),
        line((8.0, 16.0), (72.0, 48.0), 1.0, Token::Teal),
        line((8.0, 24.0), (72.0, 56.0), 2.0, Token::Amber),
        line((8.0, 64.5), (72.0, 64.5), 1.0, Token::Text2),
        line((8.0, 70.0), (72.0, 70.0), 1.0, Token::Text2),
        line((80.0, 8.0), (80.0, 88.0), 1.0, Token::Line2),
        line((86.5, 8.0), (86.5, 88.0), 1.0, Token::Line2),
        line((40.0, 90.0), (200.0, 120.0), 3.0, Token::Error),
    ];
    let center = (122.0, 40.0);
    for (dx, dy) in [
        (28.0, 0.0),
        (20.0, 20.0),
        (0.0, 28.0),
        (-20.0, 20.0),
        (-28.0, 0.0),
        (-20.0, -20.0),
        (0.0, -28.0),
        (20.0, -20.0),
    ] {
        commands.push(line(
            center,
            (center.0 + dx, center.1 + dy),
            1.5,
            Token::TealGlow,
        ));
    }
    commands
}

fn text(origin: (f32, f32), text: &str, style: TextStyle, color: Token) -> Command {
    Command::Text {
        origin: Point::new(origin.0, origin.1),
        text: text.to_string(),
        style,
        color: token(color),
    }
}

/// The size of the text scenes, in logical units.
const TEXT_SIZE: Size = Size::new(320.0, 176.0);

/// Text: each role of the typography table in its face, the bundled
/// faces' seven, and fallback (an arrow and λ Saira lacks, an emoji no
/// face has), a tab, and an origin on a fraction of a unit.
fn texts() -> Vec<Command> {
    let ui = |size| TextStyle::new(Family::Ui, size);
    let mono = TextStyle::new(Family::Mono, 14.0);
    vec![
        text(
            (12.0, 20.0),
            "UI body text: lists, the file tree and dialogs",
            ui(13.0),
            Token::Text,
        ),
        text(
            (12.0, 40.0),
            "EXPLORER · SOURCE CONTROL",
            ui(11.0).weight(Weight::SemiBold).letter_spacing(0.28),
            Token::Text3,
        ),
        text(
            (12.0, 60.0),
            "SECTION TITLE",
            ui(13.0).weight(Weight::Medium).letter_spacing(0.04),
            Token::Text2,
        ),
        text(
            (12.0, 80.0),
            "RUST · MARKDOWN · ANDROID · IOS",
            ui(12.0).weight(Weight::Bold).letter_spacing(0.10),
            Token::Amber,
        ),
        text(
            (12.0, 104.0),
            "fn main() -> Result<(), Error> {",
            mono,
            Token::Text,
        ),
        text(
            (12.0, 124.0),
            "// a comment, in italic",
            mono.italic(),
            Token::Comment,
        ),
        text(
            (12.0, 144.0),
            "let emphasis = 600;",
            mono.weight(Weight::SemiBold),
            Token::Teal,
        ),
        text(
            (12.25, 166.5),
            "Saira → λ \u{1F600}\tafter a tab",
            ui(13.0),
            Token::Text2,
        ),
        text(
            (200.0, 60.0),
            "TIDEPOOL",
            ui(15.0).weight(Weight::SemiBold).letter_spacing(0.28),
            Token::TealHi,
        ),
    ]
}

fn icon(rect: Rect, id: u16, color: Token) -> Command {
    Command::Icon {
        rect,
        icon: IconId(id),
        color: token(color),
    }
}

/// Icons: the ring at 12, 16, 24 and 32 units, the check, the play
/// triangle, the folder centred in a wide rect, a translucent one, one
/// with no icon registered (a hollow box), and one leaving the window.
fn icons() -> Vec<Command> {
    vec![
        icon(rect(8.0, 8.0, 12.0, 12.0), 0, Token::Teal),
        icon(rect(28.0, 8.0, 16.0, 16.0), 0, Token::Teal),
        icon(rect(52.0, 8.0, 24.0, 24.0), 0, Token::Teal),
        icon(rect(84.0, 8.0, 32.0, 32.0), 0, Token::TealHi),
        icon(rect(8.0, 44.0, 16.0, 16.0), 1, Token::Green),
        icon(rect(32.0, 40.0, 24.0, 24.0), 1, Token::Green),
        icon(rect(64.0, 40.0, 24.0, 24.0), 2, Token::Amber),
        icon(rect(96.0, 44.0, 48.0, 24.0), 3, Token::Text2),
        fill(rect(8.0, 70.0, 40.0, 20.0), Token::Slate),
        icon(rect(12.0, 68.0, 24.0, 24.0), 0, Token::TealGlow),
        icon(rect(64.0, 72.0, 16.0, 16.0), 9, Token::Text3),
        icon(rect(148.0, 72.0, 24.0, 24.0), 2, Token::Error),
    ]
}

fn image(rect: Rect, id: u32) -> Command {
    Command::Image {
        rect,
        image: ImageId(id),
    }
}

/// Images: the 8 × 8 pattern at its size in units, enlarged, shrunk,
/// stretched and on fractions; a translucent one over a fill; and one with
/// no image registered, which draws nothing.
fn images() -> Vec<Command> {
    vec![
        image(rect(8.0, 8.0, 8.0, 8.0), 0),
        image(rect(24.0, 8.0, 32.0, 32.0), 0),
        image(rect(64.0, 8.0, 4.0, 4.0), 0),
        image(rect(76.0, 8.0, 72.0, 16.0), 0),
        image(rect(8.5, 48.5, 40.25, 30.5), 0),
        fill(rect(64.0, 40.0, 40.0, 40.0), Token::Amber),
        image(rect(72.0, 48.0, 48.0, 24.0), 1),
        image(rect(120.0, 40.0, 30.0, 30.0), 9),
    ]
}

/// Clips: a fragment's clip cutting a pill and a line of text, clips
/// nested, one on fractions, and a clipping node whose child paints past
/// its bounds.
fn clips() -> Tree {
    let mut tree = Tree::new();
    let root = tree.root();
    let row = tree
        .insert(root, Node::new(Style::row().fixed(SIZE.width, SIZE.height)))
        .expect("a row");
    let push = |rect| Command::PushClip { rect };
    let left = vec![
        push(rect(8.0, 8.0, 60.0, 40.0)),
        fill(rect(0.0, 0.0, 200.0, 200.0), Token::Slate2),
        pill(rect(40.0, 20.0, 60.0, 20.0), Token::Amber),
        text(
            (10.0, 34.0),
            "CLIPPED TEXT",
            TextStyle::new(Family::Ui, 13.0).weight(Weight::SemiBold),
            Token::Text,
        ),
        Command::PopClip,
        push(rect(8.5, 56.25, 40.5, 30.5)),
        fill(rect(0.0, 50.0, 80.0, 50.0), Token::Slate),
        line((0.0, 50.0), (80.0, 100.0), 2.0, Token::Teal),
        Command::PopClip,
        push(rect(52.0, 52.0, 24.0, 40.0)),
        push(rect(60.0, 60.0, 30.0, 20.0)),
        fill(rect(40.0, 40.0, 60.0, 60.0), Token::TealHi),
        Command::PopClip,
        line((52.0, 52.0), (76.0, 92.0), 2.0, Token::Error),
        Command::PopClip,
    ];
    tree.insert(
        row,
        Node::new(Style::default().fixed(80.0, SIZE.height)).with_widget(Raw(left)),
    )
    .expect("the left half");
    let right: NodeId = tree
        .insert(
            row,
            Node::new(Style::default().fixed(80.0, SIZE.height).clip()),
        )
        .expect("the clipping half");
    let overflow = vec![
        fill(rect(60.0, 8.0, 80.0, 30.0), Token::Teal),
        cut(
            rect(60.0, 46.0, 120.0, 40.0),
            10.0,
            Corners::TOP_RIGHT_AND_BOTTOM_LEFT,
            Token::Amber,
        ),
    ];
    tree.insert(
        right,
        Node::new(Style::default().fixed(1.0, 1.0)).with_widget(Raw(overflow)),
    )
    .expect("the overflowing child");
    tree
}

/// Layers: two translucent fills overlapping, the same opaque in a layer
/// at 0.5 (the overlap shows only the upper), layers nested at 0.5 each, a
/// layer of text at 0.6, and a node of opacity 0.4.
fn layers() -> Tree {
    let mut tree = Tree::new();
    let root = tree.root();
    let half = |color: Token| {
        let color = token(color);
        Color::rgba(color.r, color.g, color.b, 128)
    };
    let commands = vec![
        Command::Rect {
            rect: rect(8.0, 8.0, 32.0, 32.0),
            color: half(Token::Teal),
        },
        Command::Rect {
            rect: rect(24.0, 24.0, 32.0, 32.0),
            color: half(Token::Amber),
        },
        Command::PushLayer { opacity: 0.5 },
        fill(rect(64.0, 8.0, 32.0, 32.0), Token::Teal),
        fill(rect(80.0, 24.0, 32.0, 32.0), Token::Amber),
        Command::PopLayer,
        Command::PushLayer { opacity: 0.5 },
        fill(rect(120.0, 8.0, 32.0, 24.0), Token::Text),
        Command::PushLayer { opacity: 0.5 },
        fill(rect(120.0, 32.0, 32.0, 24.0), Token::Text),
        Command::PopLayer,
        Command::PopLayer,
        Command::PushLayer { opacity: 0.6 },
        text(
            (8.0, 80.0),
            "ghost text",
            TextStyle::new(Family::Mono, 14.0).italic(),
            Token::Text,
        ),
        Command::PopLayer,
    ];
    let column = tree
        .insert(
            root,
            Node::new(Style::default().fixed(SIZE.width, SIZE.height)).with_widget(Raw(commands)),
        )
        .expect("a node");
    let faded = tree
        .insert(
            column,
            Node::new(Style::default().fixed(1.0, 1.0).opacity(0.4)),
        )
        .expect("a faded node");
    let shapes = vec![
        fill(rect(104.0, 64.0, 48.0, 24.0), Token::Violet),
        pill(rect(116.0, 70.0, 24.0, 12.0), Token::Text),
    ];
    tree.insert(
        faded,
        Node::new(Style::default().fixed(1.0, 1.0)).with_widget(Raw(shapes)),
    )
    .expect("its child");
    tree
}

/// A mix at 1.5: fills sharing fractional edges, a 1-unit border (one
/// device pixel), brackets, a pill, a line, text and an icon.
fn mix() -> Vec<Command> {
    vec![
        fill(rect(8.0, 8.0, 40.333, 24.0), Token::Teal),
        fill(rect(48.333, 8.0, 40.333, 24.0), Token::Amber),
        fill(rect(88.666, 8.0, 40.334, 24.0), Token::Green),
        fill(rect(8.0, 40.0, 94.0, 48.0), Token::PanelSolid),
        border(rect(8.0, 40.0, 94.0, 48.0), 1.0, Token::Line2),
        brackets(
            rect(8.0, 40.0, 94.0, 48.0),
            16.0,
            1.0,
            Corners::TOP_LEFT_AND_BOTTOM_RIGHT,
            Token::Teal,
        ),
        text(
            (16.0, 60.0),
            "EXPLORER",
            TextStyle::new(Family::Ui, 11.0)
                .weight(Weight::SemiBold)
                .letter_spacing(0.28),
            Token::Text3,
        ),
        text(
            (16.0, 78.0),
            "main.rs",
            TextStyle::new(Family::Mono, 13.0),
            Token::Text,
        ),
        pill(rect(110.0, 44.0, 42.0, 18.0), Token::Slate),
        icon(rect(110.0, 66.0, 20.0, 20.0), 0, Token::Teal),
        line((134.0, 68.0), (152.0, 86.0), 1.0, Token::Text2),
    ]
}

#[test]
fn ui_rect() {
    render("ui_rect", SIZE, 1.0, &mut flat(rects(), SIZE));
}

#[test]
fn ui_rect_scale_2() {
    render("ui_rect_scale_2", SIZE, 2.0, &mut flat(rects(), SIZE));
}

#[test]
fn ui_cut_corner_rect() {
    render(
        "ui_cut_corner_rect",
        SIZE,
        1.0,
        &mut flat(cut_corner_rects(), SIZE),
    );
}

#[test]
fn ui_cut_corner_rect_scale_2() {
    render(
        "ui_cut_corner_rect_scale_2",
        SIZE,
        2.0,
        &mut flat(cut_corner_rects(), SIZE),
    );
}

#[test]
fn ui_pill() {
    render("ui_pill", SIZE, 1.0, &mut flat(pills(), SIZE));
}

#[test]
fn ui_pill_scale_2() {
    render("ui_pill_scale_2", SIZE, 2.0, &mut flat(pills(), SIZE));
}

#[test]
fn ui_border() {
    render("ui_border", SIZE, 1.0, &mut flat(borders(), SIZE));
}

#[test]
fn ui_border_scale_2() {
    render("ui_border_scale_2", SIZE, 2.0, &mut flat(borders(), SIZE));
}

#[test]
fn ui_brackets() {
    render("ui_brackets", SIZE, 1.0, &mut flat(bracket_sets(), SIZE));
}

#[test]
fn ui_brackets_scale_2() {
    render(
        "ui_brackets_scale_2",
        SIZE,
        2.0,
        &mut flat(bracket_sets(), SIZE),
    );
}

#[test]
fn ui_line() {
    render("ui_line", SIZE, 1.0, &mut flat(lines(), SIZE));
}

#[test]
fn ui_line_scale_2() {
    render("ui_line_scale_2", SIZE, 2.0, &mut flat(lines(), SIZE));
}

#[test]
fn ui_text() {
    render("ui_text", TEXT_SIZE, 1.0, &mut flat(texts(), TEXT_SIZE));
}

#[test]
fn ui_text_scale_2() {
    render(
        "ui_text_scale_2",
        TEXT_SIZE,
        2.0,
        &mut flat(texts(), TEXT_SIZE),
    );
}

#[test]
fn ui_icon() {
    render("ui_icon", SIZE, 1.0, &mut flat(icons(), SIZE));
}

#[test]
fn ui_icon_scale_2() {
    render("ui_icon_scale_2", SIZE, 2.0, &mut flat(icons(), SIZE));
}

#[test]
fn ui_image() {
    render("ui_image", SIZE, 1.0, &mut flat(images(), SIZE));
}

#[test]
fn ui_image_scale_2() {
    render("ui_image_scale_2", SIZE, 2.0, &mut flat(images(), SIZE));
}

#[test]
fn ui_clip() {
    render("ui_clip", SIZE, 1.0, &mut clips());
}

#[test]
fn ui_clip_scale_2() {
    render("ui_clip_scale_2", SIZE, 2.0, &mut clips());
}

#[test]
fn ui_layer() {
    render("ui_layer", SIZE, 1.0, &mut layers());
}

#[test]
fn ui_layer_scale_2() {
    render("ui_layer_scale_2", SIZE, 2.0, &mut layers());
}

#[test]
fn ui_scale_1_5() {
    render("ui_scale_1_5", SIZE, 1.5, &mut flat(mix(), SIZE));
}
