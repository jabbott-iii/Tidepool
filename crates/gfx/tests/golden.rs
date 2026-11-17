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

//! The golden scenes of `tidepool-gfx` (ADR-0044, `docs/04-quality.md` →
//! "Golden frames"): one test per scene, named after it, for every
//! primitive and the backdrop.
//!
//! Each scene draws into a framebuffer of a fixed size, over an opaque
//! background, so that its image looks in a viewer as it is drawn. When
//! [`DIR_VAR`] names a directory, it writes what it drew there as
//! `<scene>.pam`; `cargo xtask golden` sets it, and compares each file with
//! `tests/golden/<scene>.qoi`, exactly. Unset, as in `cargo test`, the
//! scenes draw and write nothing.
//!
//! The PAM file holds the framebuffer's premultiplied bytes in RGBA order,
//! unchanged, rows from the top: `xtask`'s reader (`xtask/src/golden.rs`)
//! is the other copy of this format.

use std::env;
use std::fs;
use std::path::PathBuf;

use tidepool_core::color::{Color, PremultipliedColor};
use tidepool_core::geom::{DevicePoint, DeviceRect, DeviceSize, Scale};
use tidepool_gfx::backdrop::{Backdrop, BackdropStyle};
use tidepool_gfx::blend;
use tidepool_gfx::clip::ClipStack;
use tidepool_gfx::framebuffer::{Framebuffer, View};
use tidepool_gfx::glyph::{self, Mask};
use tidepool_gfx::image::{self, Filter};
use tidepool_gfx::raster::{self, Corners, Orientation, Vertex};

/// Names the directory the scenes write to.
const DIR_VAR: &str = "TIDEPOOL_GOLDEN_DIR";

/// The background every scene but the backdrop's starts from: jabbott.dev's
/// panel color, opaque.
const BACKGROUND: Color = Color::rgb(0x10, 0x13, 0x15);

const TEAL: Color = Color::rgb(0x5c, 0xcb, 0xcb);
const AMBER: Color = Color::rgb(0xf2, 0xc2, 0x00);
const WHITE: Color = Color::rgb(0xff, 0xff, 0xff);
const RED: Color = Color::rgb(0xe0, 0x4f, 0x4f);

/// A color with its alpha replaced, premultiplied.
const fn with_alpha(color: Color, a: u8) -> PremultipliedColor {
    Color::rgba(color.r, color.g, color.b, a).premultiply()
}

/// An opaque color, premultiplied.
const fn opaque(color: Color) -> PremultipliedColor {
    color.premultiply()
}

/// A framebuffer of `width` × `height`, cleared to [`BACKGROUND`].
fn canvas(width: i32, height: i32) -> Framebuffer {
    let mut frame = Framebuffer::new(DeviceSize::new(width, height)).expect("a small framebuffer");
    frame.clear(opaque(BACKGROUND));
    frame
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

/// An image of `width` × `height` with a pattern in it: a gradient in red
/// and green, a blue checker of `cell` px, and alpha falling from 255 at
/// the top to `bottom_alpha` at the bottom row. Premultiplied BGRA.
fn pattern(width: i32, height: i32, cell: i32, bottom_alpha: u8) -> Vec<u8> {
    let mut bytes = Vec::new();
    for y in 0..height {
        let fall = (255 - i32::from(bottom_alpha)) * y / (height - 1).max(1);
        let alpha = u8::try_from(255 - fall).expect("between the two alphas");
        for x in 0..width {
            let red = u8::try_from(x * 255 / (width - 1).max(1)).expect("at most 255");
            let green = u8::try_from(y * 255 / (height - 1).max(1)).expect("at most 255");
            let blue = if (x / cell + y / cell) % 2 == 0 {
                200
            } else {
                40
            };
            let color = Color::rgba(red, green, blue, alpha).premultiply();
            bytes.extend_from_slice(&blend::to_bgra(color));
        }
    }
    bytes
}

/// A coverage mask of `size` × `size`: a disc, anti-aliased by a ramp one
/// pixel wide, with a hole of a third of its radius.
fn disc(size: i32) -> Vec<u8> {
    let centre = f64::from(size) / 2.0;
    let outer = centre - 0.5;
    let inner = outer / 3.0;
    let mut bytes = Vec::new();
    for y in 0..size {
        for x in 0..size {
            let dx = f64::from(x) + 0.5 - centre;
            let dy = f64::from(y) + 0.5 - centre;
            let distance = (dx * dx + dy * dy).sqrt();
            let outside = (distance - outer + 0.5).clamp(0.0, 1.0);
            let inside = (inner + 0.5 - distance).clamp(0.0, 1.0);
            let coverage = (1.0 - outside - inside).clamp(0.0, 1.0);
            // At most 255.5, rounded down: a byte.
            bytes.push((coverage * 255.0 + 0.5).floor() as u8);
        }
    }
    bytes
}

#[test]
fn gfx_fill_rect() {
    let mut frame = canvas(64, 48);
    let all = frame.bounds();
    raster::fill_rect(&mut frame, all, DeviceRect::new(4, 4, 28, 20), opaque(TEAL));
    raster::fill_rect(
        &mut frame,
        all,
        DeviceRect::new(20, 14, 28, 20),
        with_alpha(AMBER, 128),
    );
    raster::fill_rect(
        &mut frame,
        all,
        DeviceRect::new(36, 28, 40, 30),
        with_alpha(WHITE, 51),
    );
    // Empty, inverted and off-frame rects draw nothing.
    raster::fill_rect(&mut frame, all, DeviceRect::new(2, 40, 0, 6), opaque(RED));
    raster::fill_rect(&mut frame, all, DeviceRect::new(2, 40, -6, 6), opaque(RED));
    raster::fill_rect(
        &mut frame,
        all,
        DeviceRect::new(-20, -20, 10, 10),
        opaque(RED),
    );
    // A rect partly off the left edge.
    raster::fill_rect(
        &mut frame,
        all,
        DeviceRect::new(-6, 34, 16, 10),
        opaque(RED),
    );
    save("gfx_fill_rect", &frame);
}

#[test]
fn gfx_hairline() {
    let mut frame = canvas(48, 32);
    let all = frame.bounds();
    for (i, alpha) in [255u8, 160, 64].into_iter().enumerate() {
        let i = i32::try_from(i).expect("three lines");
        let color = with_alpha(TEAL, alpha);
        raster::hairline(
            &mut frame,
            all,
            DevicePoint::new(2, 2 + 3 * i),
            40,
            Orientation::Horizontal,
            color,
        );
        raster::hairline(
            &mut frame,
            all,
            DevicePoint::new(4 + 4 * i, 12),
            18,
            Orientation::Vertical,
            color,
        );
    }
    // Crossing lines blend where they cross.
    raster::hairline(
        &mut frame,
        all,
        DevicePoint::new(20, 20),
        24,
        Orientation::Horizontal,
        with_alpha(AMBER, 128),
    );
    raster::hairline(
        &mut frame,
        all,
        DevicePoint::new(32, 12),
        18,
        Orientation::Vertical,
        with_alpha(AMBER, 128),
    );
    // A length of zero or less draws nothing; a line off the edge is cut.
    raster::hairline(
        &mut frame,
        all,
        DevicePoint::new(2, 28),
        0,
        Orientation::Horizontal,
        opaque(RED),
    );
    raster::hairline(
        &mut frame,
        all,
        DevicePoint::new(2, 28),
        -5,
        Orientation::Horizontal,
        opaque(RED),
    );
    raster::hairline(
        &mut frame,
        all,
        DevicePoint::new(40, 30),
        20,
        Orientation::Horizontal,
        opaque(WHITE),
    );
    save("gfx_hairline", &frame);
}

#[test]
fn gfx_border() {
    let mut frame = canvas(72, 40);
    let all = frame.bounds();
    raster::border(
        &mut frame,
        all,
        DeviceRect::new(2, 2, 20, 16),
        1,
        opaque(TEAL),
    );
    raster::border(
        &mut frame,
        all,
        DeviceRect::new(26, 2, 20, 16),
        3,
        with_alpha(AMBER, 128),
    );
    // Wider than half the rect: it fills it.
    raster::border(
        &mut frame,
        all,
        DeviceRect::new(50, 2, 18, 10),
        6,
        opaque(WHITE),
    );
    // jabbott.dev's panel edge: white at 8%.
    raster::fill_rect(
        &mut frame,
        all,
        DeviceRect::new(2, 22, 66, 16),
        opaque(Color::rgb(0x1a, 0x1e, 0x21)),
    );
    raster::border(
        &mut frame,
        all,
        DeviceRect::new(2, 22, 66, 16),
        1,
        with_alpha(WHITE, 20),
    );
    save("gfx_border", &frame);
}

#[test]
fn gfx_brackets() {
    let mut frame = canvas(96, 48);
    let all = frame.bounds();
    let sets = [
        Corners::TOP_LEFT_AND_BOTTOM_RIGHT,
        Corners::TOP_RIGHT_AND_BOTTOM_LEFT,
        Corners::ALL,
        Corners::NONE,
    ];
    for (i, corners) in sets.into_iter().enumerate() {
        let x = 2 + 24 * i32::try_from(i).expect("four sets");
        raster::brackets(
            &mut frame,
            all,
            DeviceRect::new(x, 2, 20, 20),
            6,
            1,
            corners,
            opaque(TEAL),
        );
    }
    // Arms longer than half the side are shortened; a thick arm.
    raster::brackets(
        &mut frame,
        all,
        DeviceRect::new(2, 26, 30, 18),
        16,
        2,
        Corners::ALL,
        opaque(AMBER),
    );
    raster::brackets(
        &mut frame,
        all,
        DeviceRect::new(36, 26, 56, 18),
        16,
        1,
        Corners::TOP_LEFT_AND_BOTTOM_RIGHT,
        with_alpha(WHITE, 160),
    );
    save("gfx_brackets", &frame);
}

#[test]
fn gfx_polygon() {
    let mut frame = canvas(96, 64);
    let all = frame.bounds();
    let triangle = [
        Vertex::new(4.0, 60.0),
        Vertex::new(20.5, 4.25),
        Vertex::new(38.0, 52.0),
    ];
    raster::polygon(&mut frame, all, &triangle, opaque(TEAL));
    // Concave, translucent, over the triangle.
    let arrow = [
        Vertex::new(26.0, 20.0),
        Vertex::new(56.0, 20.0),
        Vertex::new(56.0, 10.0),
        Vertex::new(74.0, 30.0),
        Vertex::new(56.0, 50.0),
        Vertex::new(56.0, 40.0),
        Vertex::new(26.0, 40.0),
    ];
    raster::polygon(&mut frame, all, &arrow, with_alpha(AMBER, 160));
    // A star that crosses itself.
    let star: Vec<Vertex> = STAR_POINTS
        .iter()
        .map(|&(sin, cos)| Vertex::new(80.0 + 14.0 * sin, 46.0 - 14.0 * cos))
        .collect();
    raster::polygon(&mut frame, all, &star, opaque(WHITE));
    // Sub-pixel slivers: thinner than a pixel.
    let sliver = [
        Vertex::new(2.0, 2.0),
        Vertex::new(60.0, 2.4),
        Vertex::new(60.0, 2.8),
    ];
    raster::polygon(&mut frame, all, &sliver, opaque(RED));
    save("gfx_polygon", &frame);
}

/// The points of a five-pointed star, every second vertex of a regular
/// pentagon: the sine and cosine of 0°, 144°, 288°, 72° and 216°, to six
/// places, from a table rather than `sin` and `cos`, whose results differ
/// by libm (`docs/subsystems/rendering.md` → Invariants).
const STAR_POINTS: [(f32, f32); 5] = [
    (0.0, 1.0),
    (0.587_785, -0.809_017),
    (-0.951_057, 0.309_017),
    (0.951_057, 0.309_017),
    (-0.587_785, -0.809_017),
];

#[test]
fn gfx_line() {
    let mut frame = canvas(80, 56);
    let all = frame.bounds();
    let widths = [0.5f32, 1.0, 1.5, 3.0];
    for (i, width) in widths.into_iter().enumerate() {
        let offset = 6.0 + 10.0 * f32::from(u8::try_from(i).expect("four widths"));
        raster::line(
            &mut frame,
            all,
            Vertex::new(4.0, offset),
            Vertex::new(76.0, offset + 12.0),
            width,
            opaque(TEAL),
        );
    }
    // Steep, translucent, and crossing the others.
    raster::line(
        &mut frame,
        all,
        Vertex::new(20.25, 2.0),
        Vertex::new(28.75, 54.0),
        2.0,
        with_alpha(AMBER, 160),
    );
    raster::line(
        &mut frame,
        all,
        Vertex::new(70.0, 4.0),
        Vertex::new(40.0, 52.0),
        1.0,
        opaque(WHITE),
    );
    // Exactly horizontal and vertical lines on half pixels.
    raster::line(
        &mut frame,
        all,
        Vertex::new(4.0, 52.5),
        Vertex::new(36.0, 52.5),
        1.0,
        opaque(RED),
    );
    raster::line(
        &mut frame,
        all,
        Vertex::new(76.5, 20.0),
        Vertex::new(76.5, 52.0),
        1.0,
        opaque(RED),
    );
    save("gfx_line", &frame);
}

#[test]
fn gfx_cut_corner_rect() {
    let mut frame = canvas(112, 48);
    let all = frame.bounds();
    // The amber primary button: 10 px cuts at the top right and bottom left.
    raster::cut_corner_rect(
        &mut frame,
        all,
        DeviceRect::new(4, 4, 96, 30),
        10.0,
        Corners::TOP_RIGHT_AND_BOTTOM_LEFT,
        opaque(AMBER),
    );
    // Every corner, a fractional cut, translucent over the button.
    raster::cut_corner_rect(
        &mut frame,
        all,
        DeviceRect::new(70, 18, 36, 26),
        6.5,
        Corners::ALL,
        with_alpha(TEAL, 192),
    );
    // A cut longer than half the shorter side is shortened; none is a rect.
    raster::cut_corner_rect(
        &mut frame,
        all,
        DeviceRect::new(4, 38, 30, 8),
        20.0,
        Corners::TOP_LEFT_AND_BOTTOM_RIGHT,
        opaque(WHITE),
    );
    raster::cut_corner_rect(
        &mut frame,
        all,
        DeviceRect::new(40, 38, 24, 8),
        4.0,
        Corners::NONE,
        opaque(RED),
    );
    save("gfx_cut_corner_rect", &frame);
}

#[test]
fn gfx_pill() {
    let mut frame = canvas(80, 48);
    let all = frame.bounds();
    raster::pill(&mut frame, all, DeviceRect::new(4, 4, 56, 16), opaque(TEAL));
    raster::pill(
        &mut frame,
        all,
        DeviceRect::new(64, 4, 12, 40),
        with_alpha(AMBER, 192),
    );
    // Small and odd-sized, and one that is a circle.
    raster::pill(
        &mut frame,
        all,
        DeviceRect::new(4, 26, 21, 7),
        opaque(WHITE),
    );
    raster::pill(
        &mut frame,
        all,
        DeviceRect::new(30, 24, 17, 17),
        opaque(RED),
    );
    raster::pill(
        &mut frame,
        all,
        DeviceRect::new(4, 38, 40, 6),
        with_alpha(WHITE, 96),
    );
    save("gfx_pill", &frame);
}

#[test]
fn gfx_clip() {
    let mut frame = canvas(80, 56);
    let mut clips = ClipStack::new(frame.bounds());
    clips.push(DeviceRect::new(8, 8, 48, 40));
    // Nested: the second clip is cut to the first.
    clips.push(DeviceRect::new(24, 0, 80, 32));
    let clip = clips.current();
    raster::fill_rect(
        &mut frame,
        clip,
        DeviceRect::new(0, 0, 80, 56),
        with_alpha(TEAL, 128),
    );
    let wedge = [
        Vertex::new(0.0, 0.0),
        Vertex::new(80.0, 10.0),
        Vertex::new(10.0, 56.0),
    ];
    raster::polygon(&mut frame, clip, &wedge, with_alpha(AMBER, 160));
    assert!(clips.pop());
    let clip = clips.current();
    raster::pill(
        &mut frame,
        clip,
        DeviceRect::new(0, 36, 80, 16),
        opaque(WHITE),
    );
    raster::line(
        &mut frame,
        clip,
        Vertex::new(0.0, 56.0),
        Vertex::new(80.0, 0.0),
        2.0,
        opaque(RED),
    );
    assert!(clips.pop());
    // The base clip again: the outline of the first clip.
    raster::border(
        &mut frame,
        clips.current(),
        DeviceRect::new(7, 7, 50, 42),
        1,
        with_alpha(WHITE, 64),
    );
    save("gfx_clip", &frame);
}

#[test]
fn gfx_layer() {
    let mut frame = canvas(72, 48);
    let all = frame.bounds();
    // Stripes under the layers, to show what they let through.
    for x in (0..72).step_by(8) {
        raster::fill_rect(
            &mut frame,
            all,
            DeviceRect::new(x, 0, 4, 48),
            opaque(Color::rgb(0x26, 0x46, 0x52)),
        );
    }
    for (i, opacity) in [255u8, 160, 64].into_iter().enumerate() {
        let x = 2 + 24 * i32::try_from(i).expect("three layers");
        let mut layer = Framebuffer::new_at(DeviceRect::new(x, 4, 20, 40)).expect("a small layer");
        layer.clear(with_alpha(TEAL, 96));
        let bounds = layer.bounds();
        raster::pill(
            &mut layer,
            bounds,
            DeviceRect::new(x + 2, 8, 16, 30),
            opaque(AMBER),
        );
        blend::composite(&mut frame, all, &layer, opacity);
    }
    save("gfx_layer", &frame);
}

#[test]
fn gfx_image() {
    let mut frame = canvas(128, 72);
    let all = frame.bounds();
    let (width, height) = (16, 12);
    let opaque_source = pattern(width, height, 4, 255);
    let translucent_source = pattern(width, height, 4, 0);
    let size = DeviceSize::new(width, height);
    let stride = 4 * usize::try_from(width).expect("positive");
    let solid = View::new(&opaque_source, size, stride).expect("a well-formed view");
    let fading = View::new(&translucent_source, size, stride).expect("a well-formed view");
    // Copy replaces, blit composites: over a stripe of white. The fading
    // copy leaves translucent pixels in the frame, the one place a scene
    // does, so a viewer shows them darker than they are (ADR-0044).
    raster::fill_rect(
        &mut frame,
        all,
        DeviceRect::new(0, 4, 128, 6),
        opaque(WHITE),
    );
    image::copy(&mut frame, all, &solid, DevicePoint::new(2, 2));
    image::copy(&mut frame, all, &fading, DevicePoint::new(20, 2));
    image::blit(&mut frame, all, &fading, DevicePoint::new(38, 2));
    // A region of the source.
    let region = solid
        .region(DeviceRect::new(4, 4, 8, 6))
        .expect("inside the source");
    image::blit(&mut frame, all, &region, DevicePoint::new(56, 2));
    // Enlarged, nearest and bilinear; then shrunk.
    image::blit_scaled(
        &mut frame,
        all,
        &solid,
        DeviceRect::new(2, 18, 48, 36),
        Filter::Nearest,
    );
    image::blit_scaled(
        &mut frame,
        all,
        &solid,
        DeviceRect::new(52, 18, 48, 36),
        Filter::Bilinear,
    );
    image::blit_scaled(
        &mut frame,
        all,
        &fading,
        DeviceRect::new(104, 18, 21, 50),
        Filter::Bilinear,
    );
    image::blit_scaled(
        &mut frame,
        all,
        &solid,
        DeviceRect::new(2, 58, 7, 5),
        Filter::Nearest,
    );
    image::blit_scaled(
        &mut frame,
        all,
        &solid,
        DeviceRect::new(12, 58, 7, 5),
        Filter::Bilinear,
    );
    save("gfx_image", &frame);
}

#[test]
fn gfx_glyph() {
    let mut frame = canvas(72, 40);
    let all = frame.bounds();
    let side = 16;
    let coverage = disc(side);
    let stride = usize::try_from(side).expect("positive");
    let mask =
        Mask::new(&coverage, DeviceSize::new(side, side), stride).expect("a well-formed mask");
    glyph::draw(&mut frame, all, &mask, DevicePoint::new(2, 2), opaque(TEAL));
    glyph::draw(
        &mut frame,
        all,
        &mask,
        DevicePoint::new(12, 10),
        with_alpha(AMBER, 160),
    );
    glyph::draw(
        &mut frame,
        all,
        &mask,
        DevicePoint::new(22, 2),
        opaque(WHITE),
    );
    // A region, as the atlas hands out, and a ramp of every coverage.
    let half = mask
        .region(DeviceRect::new(0, 0, 8, 16))
        .expect("inside the mask");
    glyph::draw(&mut frame, all, &half, DevicePoint::new(42, 2), opaque(RED));
    let ramp: Vec<u8> = (0..=255).collect();
    let ramp = Mask::new(&ramp, DeviceSize::new(64, 4), 64).expect("a well-formed mask");
    glyph::draw(
        &mut frame,
        all,
        &ramp,
        DevicePoint::new(4, 30),
        opaque(WHITE),
    );
    save("gfx_glyph", &frame);
}

/// The backdrop at `width` × `height` and `scale`: its base; its blurred
/// copy under a translucent panel with a border and brackets, as a focused
/// panel shows it; and the blurred copy alone, in a band.
fn backdrop_scene(width: i32, height: i32, scale: f32) -> Framebuffer {
    let size = DeviceSize::new(width, height);
    let scale = Scale::new(scale).expect("a valid scale");
    let mut backdrop = Backdrop::new();
    assert!(
        backdrop
            .ensure(size, scale, &BackdropStyle::jabbott_dev())
            .expect("a valid backdrop")
    );
    let mut frame = Framebuffer::new(size).expect("a small framebuffer");
    let all = frame.bounds();
    image::copy(&mut frame, all, &backdrop.base().view(), DevicePoint::ZERO);
    let panel = DeviceRect::new(width / 2, height / 4, width / 2 - width / 16, height / 2);
    let mut clips = ClipStack::new(all);
    clips.push(panel);
    let clip = clips.current();
    image::copy(
        &mut frame,
        clip,
        &backdrop.blurred().view(),
        DevicePoint::ZERO,
    );
    raster::fill_rect(
        &mut frame,
        clip,
        panel,
        with_alpha(Color::rgb(16, 19, 21), 199),
    );
    raster::border(&mut frame, clip, panel, 1, with_alpha(WHITE, 20));
    raster::brackets(
        &mut frame,
        clip,
        panel,
        16,
        1,
        Corners::TOP_LEFT_AND_BOTTOM_RIGHT,
        opaque(TEAL),
    );
    clips.pop();
    // The blurred copy alone, in a band at the bottom left, where the
    // planet's rim crosses at scale 1.
    let band = DeviceRect::new(width / 16, height * 5 / 8, width * 3 / 8, height / 4);
    clips.push(band);
    image::copy(
        &mut frame,
        clips.current(),
        &backdrop.blurred().view(),
        DevicePoint::ZERO,
    );
    raster::border(&mut frame, clips.current(), band, 1, with_alpha(WHITE, 20));
    frame
}

#[test]
fn gfx_backdrop() {
    save("gfx_backdrop", &backdrop_scene(480, 300, 1.0));
}

#[test]
fn gfx_backdrop_scale_2() {
    save("gfx_backdrop_scale_2", &backdrop_scene(480, 300, 2.0));
}
