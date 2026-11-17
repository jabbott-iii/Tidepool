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

//! M2.3's properties, through the public API only:
//!
//! 1. Drawing inside a clip never writes outside it: random primitives of
//!    every kind, with geometry partly or wholly outside the clip and the
//!    framebuffer, at the ends of the `i32` range, huge, NaN or infinite,
//!    over a random background with a random clip, leave every pixel
//!    outside the clip as it was.
//! 2. Merged damage covers every changed pixel: every pixel that random
//!    draws change lies in a rect of `damage::diff`; and random
//!    `Damage::add` sequences keep at most `MAX_RECTS` rects that neither
//!    overlap nor touch and that hold every pixel of every rect added.
//! 3. The same draw list gives the same bytes on two runs: a fixed list
//!    using every primitive, a layer, an image and a glyph mask, over the
//!    backdrop, drawn into two fresh framebuffers.
//!
//! Each case's input is a seed, so a failure report stays short; the
//! message holds the draw list.

use tidepool_core::color::{Color, PremultipliedColor};
use tidepool_core::geom::{DevicePoint, DeviceRect, DeviceSize, Scale};
use tidepool_core::rng::Pcg32;
use tidepool_core::testing::{check, check_sized};
use tidepool_gfx::backdrop::{Backdrop, BackdropStyle};
use tidepool_gfx::blend;
use tidepool_gfx::clip::{ClipStack, SPACE, intersect};
use tidepool_gfx::damage::{self, Damage, MAX_RECTS};
use tidepool_gfx::framebuffer::{Framebuffer, View};
use tidepool_gfx::glyph::{self, Mask};
use tidepool_gfx::image::{self, Filter};
use tidepool_gfx::raster::{self, Corners, Orientation, Vertex};

/// Image or mask bytes with their shape, and the region drawn from them.
#[derive(Clone, Debug)]
struct Pixels {
    bytes: Vec<u8>,
    size: DeviceSize,
    stride: usize,
    region: Option<DeviceRect>,
}

/// A draw call and its arguments.
#[derive(Clone, Debug)]
enum Op {
    Fill(DeviceRect, PremultipliedColor),
    Hairline(DevicePoint, i32, Orientation, PremultipliedColor),
    Border(DeviceRect, i32, PremultipliedColor),
    Brackets(DeviceRect, i32, i32, Corners, PremultipliedColor),
    Polygon(Vec<Vertex>, PremultipliedColor),
    Line(Vertex, Vertex, f32, PremultipliedColor),
    CutCorner(DeviceRect, f32, Corners, PremultipliedColor),
    Pill(DeviceRect, PremultipliedColor),
    Copy(Pixels, DevicePoint),
    Blit(Pixels, DevicePoint),
    BlitScaled(Pixels, DeviceRect, Filter),
    Glyph(Pixels, DevicePoint, PremultipliedColor),
    /// A layer covering the rect, cleared to the first color with a pill
    /// of the second, composited at the opacity.
    Composite(DeviceRect, PremultipliedColor, PremultipliedColor, u8),
}

fn coordinate(rng: &mut Pcg32) -> i32 {
    match rng.range(0..12u32) {
        0 => i32::MIN,
        1 => i32::MAX,
        2 => rng.range(-100_000..100_000),
        3 => rng.range(i32::MIN..i32::MIN + 100),
        _ => rng.range(-20..60),
    }
}

fn extent(rng: &mut Pcg32) -> i32 {
    match rng.range(0..12u32) {
        0 => i32::MAX,
        1 => -rng.range(0..20i32),
        2 => 0,
        3 => rng.range(0..100_000),
        _ => rng.range(1..50),
    }
}

fn rect(rng: &mut Pcg32) -> DeviceRect {
    DeviceRect::new(coordinate(rng), coordinate(rng), extent(rng), extent(rng))
}

fn float(rng: &mut Pcg32) -> f32 {
    match rng.range(0..14u32) {
        0 => f32::NAN,
        1 => f32::INFINITY,
        2 => f32::NEG_INFINITY,
        3 => 3e38,
        4 => -3e38,
        5 => rng.range(-1e6..1e6f32),
        6 => rng.range(-1e-3..1e-3f32),
        _ => rng.range(-20.0..60.0f32),
    }
}

fn vertex(rng: &mut Pcg32) -> Vertex {
    Vertex::new(float(rng), float(rng))
}

fn color(rng: &mut Pcg32) -> PremultipliedColor {
    let [r, g, b, a] = rng.next_u32().to_le_bytes();
    match rng.range(0..6u32) {
        0 => PremultipliedColor::TRANSPARENT,
        // Outside the premultiplied invariant, which must not panic either.
        1 => PremultipliedColor { r, g, b, a },
        2 => Color::rgb(r, g, b).premultiply(),
        _ => Color::rgba(r, g, b, a).premultiply(),
    }
}

fn corners(rng: &mut Pcg32) -> Corners {
    let bits = rng.range(0..16u32);
    Corners {
        top_left: bits & 1 != 0,
        top_right: bits & 2 != 0,
        bottom_right: bits & 4 != 0,
        bottom_left: bits & 8 != 0,
    }
}

/// Random bytes shaped as `bytes_per_pixel`-byte pixels, sometimes with
/// padding, and sometimes drawn from a region of them.
fn pixels(rng: &mut Pcg32, bytes_per_pixel: usize) -> Pixels {
    let size = DeviceSize::new(rng.range(0..7), rng.range(0..7));
    let width = usize::try_from(size.width).unwrap_or(0);
    let height = usize::try_from(size.height).unwrap_or(0);
    let stride = width * bytes_per_pixel + rng.range(0..3u32) as usize;
    let len = stride * height + rng.range(0..3u32) as usize;
    let bytes = (0..len).map(|_| rng.next_u32() as u8).collect();
    let region = (rng.range(0..3u32) == 0 && size.width > 0 && size.height > 0).then(|| {
        let x = rng.range(0..size.width);
        let y = rng.range(0..size.height);
        DeviceRect::new(
            x,
            y,
            rng.range(0..size.width - x + 1),
            rng.range(0..size.height - y + 1),
        )
    });
    Pixels {
        bytes,
        size,
        stride,
        region,
    }
}

fn op(rng: &mut Pcg32) -> Op {
    match rng.range(0..13u32) {
        0 => Op::Fill(rect(rng), color(rng)),
        1 => {
            let orientation = if rng.range(0..2u32) == 0 {
                Orientation::Horizontal
            } else {
                Orientation::Vertical
            };
            Op::Hairline(
                DevicePoint::new(coordinate(rng), coordinate(rng)),
                extent(rng),
                orientation,
                color(rng),
            )
        }
        2 => Op::Border(rect(rng), extent(rng), color(rng)),
        3 => Op::Brackets(
            rect(rng),
            extent(rng),
            extent(rng),
            corners(rng),
            color(rng),
        ),
        4 => {
            let points = (0..rng.range(0..9u32)).map(|_| vertex(rng)).collect();
            Op::Polygon(points, color(rng))
        }
        5 => Op::Line(vertex(rng), vertex(rng), float(rng), color(rng)),
        6 => Op::CutCorner(rect(rng), float(rng), corners(rng), color(rng)),
        7 => Op::Pill(rect(rng), color(rng)),
        8 => Op::Copy(
            pixels(rng, 4),
            DevicePoint::new(coordinate(rng), coordinate(rng)),
        ),
        9 => Op::Blit(
            pixels(rng, 4),
            DevicePoint::new(coordinate(rng), coordinate(rng)),
        ),
        10 => {
            let filter = if rng.range(0..2u32) == 0 {
                Filter::Nearest
            } else {
                Filter::Bilinear
            };
            Op::BlitScaled(pixels(rng, 4), rect(rng), filter)
        }
        11 => Op::Glyph(
            pixels(rng, 1),
            DevicePoint::new(coordinate(rng), coordinate(rng)),
            color(rng),
        ),
        _ => {
            // Small layers, so that most can be made.
            let bounds = DeviceRect::new(
                rng.range(-20..50),
                rng.range(-20..40),
                rng.range(-2..40),
                rng.range(-2..30),
            );
            Op::Composite(bounds, color(rng), color(rng), rng.next_u32() as u8)
        }
    }
}

/// The image `p` describes, if its shape is valid.
fn view(p: &Pixels) -> Option<View<'_>> {
    let view = View::new(&p.bytes, p.size, p.stride).ok()?;
    p.region
        .map_or(Some(view), |region| view.region(region).ok())
}

/// The mask `p` describes, if its shape is valid.
fn mask(p: &Pixels) -> Option<Mask<'_>> {
    let mask = Mask::new(&p.bytes, p.size, p.stride).ok()?;
    p.region
        .map_or(Some(mask), |region| mask.region(region).ok())
}

/// Draws `op` into `fb`, clipped to `clip`.
fn apply(fb: &mut Framebuffer, clip: DeviceRect, op: &Op) {
    match op {
        Op::Fill(rect, color) => raster::fill_rect(fb, clip, *rect, *color),
        Op::Hairline(start, length, orientation, color) => {
            raster::hairline(fb, clip, *start, *length, *orientation, *color);
        }
        Op::Border(rect, width, color) => raster::border(fb, clip, *rect, *width, *color),
        Op::Brackets(rect, arm, thickness, corners, color) => {
            raster::brackets(fb, clip, *rect, *arm, *thickness, *corners, *color);
        }
        Op::Polygon(points, color) => raster::polygon(fb, clip, points, *color),
        Op::Line(from, to, width, color) => raster::line(fb, clip, *from, *to, *width, *color),
        Op::CutCorner(rect, cut, corners, color) => {
            raster::cut_corner_rect(fb, clip, *rect, *cut, *corners, *color);
        }
        Op::Pill(rect, color) => raster::pill(fb, clip, *rect, *color),
        Op::Copy(p, at) => {
            if let Some(view) = view(p) {
                image::copy(fb, clip, &view, *at);
            }
        }
        Op::Blit(p, at) => {
            if let Some(view) = view(p) {
                image::blit(fb, clip, &view, *at);
            }
        }
        Op::BlitScaled(p, dst, filter) => {
            if let Some(view) = view(p) {
                image::blit_scaled(fb, clip, &view, *dst, *filter);
            }
        }
        Op::Glyph(p, at, color) => {
            if let Some(mask) = mask(p) {
                glyph::draw(fb, clip, &mask, *at, *color);
            }
        }
        Op::Composite(bounds, fill, dot, opacity) => {
            if let Ok(mut layer) = Framebuffer::new_at(*bounds) {
                layer.clear(*fill);
                raster::pill(&mut layer, SPACE, *bounds, *dot);
                blend::composite(fb, clip, &layer, *opacity);
            }
        }
    }
}

/// A framebuffer of random bounds near the origin, filled with random
/// bytes (not necessarily premultiplied).
fn background(rng: &mut Pcg32) -> Framebuffer {
    let bounds = DeviceRect::new(
        rng.range(-10..10),
        rng.range(-10..10),
        rng.range(1..40),
        rng.range(1..30),
    );
    let mut fb = Framebuffer::new_at(bounds).expect("small");
    let pixels: Vec<u8> = (0..fb.bytes().len())
        .map(|_| rng.next_u32() as u8)
        .collect();
    let view = View::new(&pixels, fb.size(), fb.stride()).expect("its own shape");
    image::copy(&mut fb, SPACE, &view, DevicePoint::new(bounds.x, bounds.y));
    fb
}

/// A clip near the framebuffer: inside, overlapping, empty or outside.
fn clip(rng: &mut Pcg32, bounds: DeviceRect) -> DeviceRect {
    match rng.range(0..8u32) {
        0 => DeviceRect::ZERO,
        1 => SPACE,
        2 => rect(rng),
        _ => DeviceRect::new(
            bounds.x + rng.range(-5..bounds.width),
            bounds.y + rng.range(-5..bounds.height),
            rng.range(-2..bounds.width + 5),
            rng.range(-2..bounds.height + 5),
        ),
    }
}

/// Every pixel of `fb`, with its position.
fn pixels_of(fb: &Framebuffer) -> impl Iterator<Item = (DevicePoint, PremultipliedColor)> + '_ {
    let bounds = fb.bounds();
    (bounds.y..bounds.bottom()).flat_map(move |y| {
        (bounds.x..bounds.right()).filter_map(move |x| {
            let at = DevicePoint::new(x, y);
            fb.pixel(at).map(|color| (at, color))
        })
    })
}

#[test]
fn drawing_inside_the_clip_never_writes_outside_it() {
    check_sized(
        "gfx draws only inside the clip",
        400,
        |rng, size| rng.next_u32() as u64 | (u64::from(size) << 32),
        |&seed| {
            let mut rng = Pcg32::new(seed, 1);
            let mut fb = background(&mut rng);
            let clip = clip(&mut rng, fb.bounds());
            let ops: Vec<Op> = (0..(seed >> 32) / 4 + 1).map(|_| op(&mut rng)).collect();
            let before = fb.clone();
            for op in &ops {
                apply(&mut fb, clip, op);
            }
            for ((at, after), (_, was)) in pixels_of(&fb).zip(pixels_of(&before)) {
                if after != was && !clip.contains(at) {
                    return Err(format!(
                        "{at:?} outside {clip:?} changed from {was:?} to {after:?}; ops {ops:?}"
                    ));
                }
            }
            Ok(())
        },
    );
}

#[test]
fn every_primitive_reaches_inside_the_clip() {
    // So that the property above is not passing vacuously: each kind of
    // primitive, drawn opaque and large across a clip, changes it.
    let opaque = PremultipliedColor {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    let image = Pixels {
        bytes: vec![255; 64],
        size: DeviceSize::new(4, 4),
        stride: 16,
        region: None,
    };
    let mask = Pixels {
        bytes: vec![255; 16],
        size: DeviceSize::new(4, 4),
        stride: 4,
        region: None,
    };
    let big = DeviceRect::new(-5, -5, 40, 40);
    let ops = [
        Op::Fill(big, opaque),
        Op::Hairline(DevicePoint::new(-5, 6), 40, Orientation::Horizontal, opaque),
        Op::Border(DeviceRect::new(4, 4, 10, 10), 3, opaque),
        Op::Brackets(DeviceRect::new(4, 4, 10, 10), 5, 3, Corners::ALL, opaque),
        Op::Polygon(
            vec![
                Vertex::new(-5.0, -5.0),
                Vertex::new(40.0, -5.0),
                Vertex::new(-5.0, 40.0),
            ],
            opaque,
        ),
        Op::Line(Vertex::new(-5.0, 7.0), Vertex::new(40.0, 7.0), 4.0, opaque),
        Op::CutCorner(big, 3.0, Corners::ALL, opaque),
        Op::Pill(big, opaque),
        Op::Copy(image.clone(), DevicePoint::new(5, 5)),
        Op::Blit(image.clone(), DevicePoint::new(5, 5)),
        Op::BlitScaled(image, big, Filter::Bilinear),
        Op::Glyph(mask, DevicePoint::new(5, 5), opaque),
        Op::Composite(big, opaque, opaque, 255),
    ];
    for op in &ops {
        let mut fb = Framebuffer::new(DeviceSize::new(16, 16)).expect("small");
        let clip = DeviceRect::new(5, 5, 6, 6);
        apply(&mut fb, clip, op);
        let inside = pixels_of(&fb)
            .filter(|(at, color)| clip.contains(*at) && color.a != 0)
            .count();
        assert!(inside > 0, "{op:?} drew nothing inside the clip");
        let outside = pixels_of(&fb)
            .filter(|(at, color)| !clip.contains(*at) && color.a != 0)
            .count();
        assert_eq!(outside, 0, "{op:?}");
    }
}

#[test]
fn merged_damage_covers_every_changed_pixel() {
    check_sized(
        "damage::diff covers every changed pixel",
        300,
        |rng, size| rng.next_u32() as u64 | (u64::from(size) << 32),
        |&seed| {
            let mut rng = Pcg32::new(seed, 2);
            let mut fb = background(&mut rng);
            let before = fb.clone();
            let clip = if rng.range(0..2u32) == 0 {
                SPACE
            } else {
                clip(&mut rng, fb.bounds())
            };
            let ops: Vec<Op> = (0..(seed >> 32) / 4 + 1).map(|_| op(&mut rng)).collect();
            for op in &ops {
                apply(&mut fb, clip, op);
            }
            let damage = damage::diff(&before, &fb);
            if damage.rects().len() > MAX_RECTS {
                return Err(format!("{} rects", damage.rects().len()));
            }
            for ((at, after), (_, was)) in pixels_of(&fb).zip(pixels_of(&before)) {
                if after != was && !damage.rects().iter().any(|rect| rect.contains(at)) {
                    return Err(format!(
                        "{at:?} changed outside {:?}; ops {ops:?}",
                        damage.rects()
                    ));
                }
            }
            Ok(())
        },
    );
}

/// Returns `true` if `a` and `b` overlap or share an edge segment.
fn touch(a: DeviceRect, b: DeviceRect) -> bool {
    let (al, ar) = (i64::from(a.x), i64::from(a.x) + i64::from(a.width));
    let (at, ab) = (i64::from(a.y), i64::from(a.y) + i64::from(a.height));
    let (bl, br) = (i64::from(b.x), i64::from(b.x) + i64::from(b.width));
    let (bt, bb) = (i64::from(b.y), i64::from(b.y) + i64::from(b.height));
    let closed = al <= br && bl <= ar && at <= bb && bt <= ab;
    closed && ((al < br && bl < ar) || (at < bb && bt < ab))
}

#[test]
fn merged_damage_covers_every_rect_added() {
    // Pixels are checked in a window around where the rects fall; rects
    // reaching far outside it are clipped to it, as the property is about
    // the pixels of the window.
    let window = DeviceRect::new(-30, -30, 120, 100);
    check_sized(
        "Damage::add covers every rect added",
        400,
        |rng, size| {
            (0..size / 3 + 1)
                .map(|_| match rng.range(0..6u32) {
                    0 => rect(rng),
                    _ => DeviceRect::new(
                        rng.range(-40..100),
                        rng.range(-40..80),
                        rng.range(-3..30),
                        rng.range(-3..30),
                    ),
                })
                .collect::<Vec<_>>()
        },
        |added| {
            let mut damage = Damage::new();
            for (count, &rect) in added.iter().enumerate() {
                damage.add(rect);
                let rects = damage.rects();
                if rects.len() > MAX_RECTS {
                    return Err(format!("{} rects after {} adds", rects.len(), count + 1));
                }
                for (i, &a) in rects.iter().enumerate() {
                    for &b in &rects[i + 1..] {
                        if touch(a, b) {
                            return Err(format!("{a:?} and {b:?} touch"));
                        }
                    }
                    if a.is_empty() {
                        return Err(format!("an empty rect {a:?} is kept"));
                    }
                }
            }
            for &rect in added {
                let part = intersect(rect, window);
                for y in part.y..part.bottom() {
                    for x in part.x..part.right() {
                        let at = DevicePoint::new(x, y);
                        if !damage.rects().iter().any(|kept| kept.contains(at)) {
                            return Err(format!(
                                "{at:?} of {rect:?} is not in {:?}",
                                damage.rects()
                            ));
                        }
                    }
                }
            }
            // Clipping to the window keeps exactly the window's part of it.
            let mut clipped = damage;
            clipped.clip_to(window);
            for &rect in clipped.rects() {
                if intersect(rect, window) != rect {
                    return Err(format!("{rect:?} reaches outside the window"));
                }
            }
            Ok(())
        },
    );
}

#[test]
fn the_clip_stack_only_narrows() {
    check(
        "the clip stack is the intersection of what it holds",
        500,
        |rng| {
            (0..rng.range(0..20u32))
                .map(|_| (rng.range(0..3u32) == 0, rect(rng)))
                .collect::<Vec<_>>()
        },
        |steps| {
            let base = DeviceRect::new(0, 0, 50, 40);
            let mut stack = ClipStack::new(base);
            let mut model = vec![base];
            for &(pop, rect) in steps {
                if pop {
                    let popped = stack.pop();
                    if popped != (model.len() > 1) {
                        return Err(format!(
                            "pop returned {popped} at depth {}",
                            model.len() - 1
                        ));
                    }
                    if model.len() > 1 {
                        model.pop();
                    }
                } else {
                    stack.push(rect);
                    let top = *model.last().unwrap_or(&base);
                    model.push(intersect(top, rect));
                }
                let expected = *model.last().unwrap_or(&base);
                if stack.current() != expected || stack.depth() != model.len() - 1 {
                    return Err(format!(
                        "{:?} at depth {}, expected {expected:?}",
                        stack.current(),
                        stack.depth()
                    ));
                }
                if intersect(stack.current(), base) != stack.current() {
                    return Err(format!("{:?} reaches outside the base", stack.current()));
                }
            }
            Ok(())
        },
    );
}

/// One fixed draw list using every primitive, a layer, an image and a
/// glyph mask, over the backdrop, into a fresh 200 by 120 framebuffer.
fn draw_fixed(backdrop: &Backdrop) -> Framebuffer {
    let mut fb = Framebuffer::new(DeviceSize::new(200, 120)).expect("small");
    let all = fb.bounds();
    let color = |r, g, b, a| Color::rgba(r, g, b, a).premultiply();
    let teal = color(0x5c, 0xcb, 0xcb, 255);
    image::copy(&mut fb, all, &backdrop.base().view(), DevicePoint::ZERO);
    let panel = DeviceRect::new(20, 10, 150, 90);
    image::copy(
        &mut fb,
        panel,
        &backdrop.blurred().view(),
        DevicePoint::ZERO,
    );
    raster::fill_rect(&mut fb, all, panel, color(16, 19, 21, 199));
    raster::border(&mut fb, all, panel, 1, color(255, 255, 255, 20));
    raster::brackets(
        &mut fb,
        all,
        panel,
        16,
        1,
        Corners::TOP_LEFT_AND_BOTTOM_RIGHT,
        teal,
    );
    raster::hairline(
        &mut fb,
        all,
        DevicePoint::new(20, 30),
        150,
        Orientation::Horizontal,
        color(255, 255, 255, 41),
    );
    raster::hairline(
        &mut fb,
        all,
        DevicePoint::new(60, 30),
        70,
        Orientation::Vertical,
        color(255, 255, 255, 41),
    );
    raster::cut_corner_rect(
        &mut fb,
        all,
        DeviceRect::new(30, 70, 60, 20),
        6.5,
        Corners::TOP_RIGHT_AND_BOTTOM_LEFT,
        color(0xf2, 0xc2, 0x00, 255),
    );
    raster::pill(
        &mut fb,
        all,
        DeviceRect::new(100, 72, 50, 16),
        color(0x29, 0x35, 0x3a, 255),
    );
    raster::pill(
        &mut fb,
        all,
        DeviceRect::new(30, 18, 7, 7),
        color(0x3d, 0xdc, 0x97, 255),
    );
    raster::line(
        &mut fb,
        all,
        Vertex::new(70.3, 60.7),
        Vertex::new(160.9, 33.1),
        1.5,
        teal,
    );
    raster::polygon(
        &mut fb,
        DeviceRect::new(0, 0, 120, 120),
        &[
            Vertex::new(100.2, 40.0),
            Vertex::new(140.0, 45.5),
            Vertex::new(118.0, 66.25),
            Vertex::new(95.0, 58.0),
        ],
        color(0xa5, 0x8c, 0xff, 180),
    );
    // A layer at 60% opacity.
    let disc = DeviceRect::new(150, 60, 40, 40);
    let mut layer = Framebuffer::new_at(disc).expect("small");
    raster::pill(&mut layer, SPACE, disc, color(0xf0, 0x9a, 0x4a, 255));
    blend::composite(&mut fb, all, &layer, 153);
    // A 3 by 2 image, copied, blitted and scaled both ways.
    let pixels: Vec<u8> = (0..24u8).map(|i| i.wrapping_mul(37) | 0x80).collect();
    let view = View::new(&pixels, DeviceSize::new(3, 2), 12).expect("fits");
    image::copy(&mut fb, all, &view, DevicePoint::new(2, 100));
    image::blit(&mut fb, all, &view, DevicePoint::new(8, 100));
    image::blit_scaled(
        &mut fb,
        all,
        &view,
        DeviceRect::new(2, 104, 13, 9),
        Filter::Nearest,
    );
    image::blit_scaled(
        &mut fb,
        all,
        &view,
        DeviceRect::new(16, 104, 13, 9),
        Filter::Bilinear,
    );
    // A glyph-like mask, from a region of a larger atlas.
    let atlas: Vec<u8> = (0..64u32).map(|i| (i * 53 % 256) as u8).collect();
    let mask = Mask::new(&atlas, DeviceSize::new(8, 8), 8)
        .and_then(|mask| mask.region(DeviceRect::new(2, 1, 5, 6)))
        .expect("inside");
    glyph::draw(
        &mut fb,
        all,
        &mask,
        DevicePoint::new(180, 10),
        color(0xee, 0xf2, 0xf3, 255),
    );
    fb
}

#[test]
fn the_same_draw_list_gives_the_same_bytes() {
    let style = BackdropStyle::jabbott_dev();
    let scale = Scale::new(1.5).expect("valid");
    let size = DeviceSize::new(200, 120);
    let mut first = Backdrop::new();
    let mut second = Backdrop::new();
    assert_eq!(first.ensure(size, scale, &style), Ok(true));
    assert_eq!(second.ensure(size, scale, &style), Ok(true));
    assert_eq!(first.base().bytes(), second.base().bytes());
    assert_eq!(first.blurred().bytes(), second.blurred().bytes());

    let one = draw_fixed(&first);
    let two = draw_fixed(&second);
    assert_eq!(one.bytes(), two.bytes());
    // And on another thread, from the same inputs.
    let three = std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let mut backdrop = Backdrop::new();
                backdrop
                    .ensure(size, scale, &style)
                    .map(|_| draw_fixed(&backdrop))
            })
            .join()
    });
    match three {
        Ok(Ok(three)) => assert_eq!(three.bytes(), one.bytes()),
        Ok(Err(error)) => panic!("the backdrop failed on another thread: {error}"),
        Err(_) => panic!("the draw panicked on another thread"),
    }
    // The list draws something of every layer: the frame is not the
    // backdrop alone.
    assert_ne!(one.bytes(), first.base().bytes());
}
