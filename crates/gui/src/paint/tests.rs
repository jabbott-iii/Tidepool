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

//! Unit tests of the painter: the geometry's rounding, the damage, each
//! command against what `gfx` draws for it, clips, layers, icons, images,
//! text, and lists made by hand.

use tidepool_core::geom::Size;
use tidepool_font::layout;
use tidepool_font::outline::OutlinePoint;
use tidepool_ui::draw::{DrawList, PaintContext};
use tidepool_ui::layout::{Length, Style as LayoutStyle};
use tidepool_ui::metrics::{Family, Weight};
use tidepool_ui::{Node, Tree, Widget};

use super::*;

const BG: Color = Color::rgb(0x0b, 0x0c, 0x0d);
const TEAL: Color = Color::rgb(0x5c, 0xcb, 0xcb);
const AMBER: Color = Color::rgb(0xf2, 0xc2, 0x00);
const GLOW: Color = Color::rgba(0x5c, 0xcb, 0xcb, 0x59);
const SENTINEL: Color = Color::rgb(0xff, 0x00, 0xff);

fn scale(factor: f32) -> Scale {
    Scale::new(factor).expect("a scale")
}

fn painter() -> Painter {
    Painter::new().expect("the bundled faces load")
}

/// A framebuffer of `width` × `height` cleared to `color`.
fn canvas(width: i32, height: i32, color: Color) -> Framebuffer {
    let mut frame = Framebuffer::new(DeviceSize::new(width, height)).expect("a framebuffer");
    frame.clear(color.premultiply());
    frame
}

/// A frame of `commands` damaging `damage`.
fn frame(commands: Vec<Command>, damage: Vec<Rect>) -> Frame {
    let mut list = DrawList::new();
    for command in commands {
        list.push(command);
    }
    Frame { list, damage }
}

/// What `commands` paint over [`BG`] in a window of `width` × `height`
/// device pixels, damaging all of it.
fn painted(
    painter: &mut Painter,
    commands: Vec<Command>,
    size: (i32, i32),
    at: Scale,
) -> Framebuffer {
    let base = canvas(size.0, size.1, BG);
    let mut target = canvas(size.0, size.1, SENTINEL);
    let factor = at.factor();
    let window = Rect::new(0.0, 0.0, size.0 as f32 / factor, size.1 as f32 / factor);
    painter
        .paint(
            &mut target,
            &base.view(),
            &frame(commands, vec![window]),
            at,
        )
        .expect("painted");
    target
}

/// The bounding box of the pixels where `a` and `b` differ.
fn changed(a: &Framebuffer, b: &Framebuffer) -> Option<DeviceRect> {
    let size = a.size();
    let (mut left, mut top, mut right, mut bottom) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for y in 0..size.height {
        for x in 0..size.width {
            let at = DevicePoint::new(x, y);
            if a.pixel(at) != b.pixel(at) {
                (left, top) = (left.min(x), top.min(y));
                (right, bottom) = (right.max(x + 1), bottom.max(y + 1));
            }
        }
    }
    (left <= right).then(|| DeviceRect::new(left, top, right - left, bottom - top))
}

/// Draws what a command should, straight with `gfx`.
type Draw = Box<dyn Fn(&mut Framebuffer)>;

fn on(x: f32, y: f32) -> OutlinePoint {
    OutlinePoint {
        x,
        y,
        on_curve: true,
    }
}

#[test]
fn rect_edges_round_to_the_nearest_pixel_so_neighbours_share_them() {
    let at = |x, y, w, h, factor| device_rect(Rect::new(x, y, w, h), scale(factor));
    assert_eq!(at(1.0, 2.0, 3.0, 4.0, 1.0), DeviceRect::new(1, 2, 3, 4));
    assert_eq!(at(1.0, 2.0, 3.0, 4.0, 2.0), DeviceRect::new(2, 4, 6, 8));
    // At 1.5: 1.5 → 2, 6 → 6; 0.3 → 0, 3.3 → 3 (half-way rounds up).
    assert_eq!(at(1.0, 0.2, 3.0, 2.0, 1.5), DeviceRect::new(2, 0, 4, 3));
    // Neighbours at a fractional scale share their edge.
    let left = at(0.0, 0.0, 10.3, 5.0, 1.25);
    let right = at(10.3, 0.0, 10.0, 5.0, 1.25);
    assert_eq!(left.x + left.width, right.x);
    // NaN is empty; an inverted rect is empty; a huge one is clamped to
    // `gfx`'s space.
    assert!(at(f32::NAN, 0.0, 1.0, 1.0, 1.0).is_empty());
    assert!(at(5.0, 5.0, -2.0, 3.0, 1.0).is_empty());
    let huge = at(-f32::MAX, 0.0, f32::INFINITY, 1.0, 2.0);
    assert_eq!((huge.x, huge.width), (-(1 << 30), i32::MAX));
}

#[test]
fn border_widths_snap_as_css_snaps_them() {
    let width = |w, factor| border_width(w, scale(factor));
    assert_eq!(width(1.0, 1.0), 1);
    assert_eq!(width(1.0, 1.5), 1, "1.5 device px is floored");
    assert_eq!(width(1.0, 2.0), 2);
    assert_eq!(width(0.5, 1.0), 1, "under a pixel is one");
    assert_eq!(width(0.25, 2.0), 1);
    assert_eq!(width(2.0, 1.25), 2);
    for none in [0.0, -1.0, f32::NAN, f32::NEG_INFINITY] {
        assert_eq!(width(none, 1.0), 0, "{none}");
    }
    assert_eq!(width(f32::INFINITY, 1.0), i32::MAX);
    // An arm rounds instead.
    assert_eq!(device_length(16.0, scale(1.5)), 24);
    assert_eq!(device_length(2.5, scale(1.0)), 3);
    assert_eq!(device_length(f32::NAN, scale(1.0)), 0);
}

#[test]
fn opacity_is_rounded_to_a_byte_and_nan_is_opaque() {
    assert_eq!(opacity_byte(0.0), 0);
    assert_eq!(opacity_byte(1.0), 255);
    assert_eq!(opacity_byte(0.5), 128);
    assert_eq!(opacity_byte(0.25), 64);
    assert_eq!(opacity_byte(f32::NAN), 255);
    assert_eq!(opacity_byte(-3.0), 0);
    assert_eq!(opacity_byte(7.0), 255);
}

#[test]
fn damage_is_rounded_out_to_whole_pixels() {
    let rect = Rect::new(1.2, 1.7, 3.0, 2.0);
    assert_eq!(
        damage_rect(rect, scale(1.0)),
        Some(DeviceRect::new(1, 1, 4, 3))
    );
    // At 2: 2.4 → 2, 3.4 → 3, 8.4 → 9, 7.4 → 8.
    assert_eq!(
        damage_rect(rect, scale(2.0)),
        Some(DeviceRect::new(2, 3, 7, 5))
    );
    assert_eq!(
        damage_rect(Rect::new(0.0, f32::NAN, 1.0, 1.0), scale(1.0)),
        None
    );
    // A rect with no area covers no pixel, whatever its edges round to.
    for empty in [
        Rect::new(10.4, 0.0, 0.0, 5.0),
        Rect::new(3.0, 3.0, 4.0, -1.0),
    ] {
        assert_eq!(
            outward(empty, scale(1.0)),
            Some(DeviceRect::ZERO),
            "{empty:?}"
        );
        assert_eq!(
            damage_rect(empty, scale(1.0)),
            Some(DeviceRect::ZERO),
            "{empty:?}"
        );
    }
}

#[test]
fn only_the_damage_is_painted_and_it_is_returned() {
    let mut painter = painter();
    let base = canvas(40, 30, BG);
    let mut target = canvas(40, 30, SENTINEL);
    let fill = Command::Rect {
        rect: Rect::new(0.0, 0.0, 40.0, 30.0),
        color: TEAL,
    };
    let damage = painter
        .paint(
            &mut target,
            &base.view(),
            &frame(vec![fill], vec![Rect::new(10.0, 10.0, 5.0, 5.0)]),
            Scale::ONE,
        )
        .expect("painted");
    assert_eq!(damage.rects(), [DeviceRect::new(10, 10, 5, 5)]);
    assert_eq!(
        changed(&target, &canvas(40, 30, SENTINEL)),
        Some(DeviceRect::new(10, 10, 5, 5))
    );
    assert_eq!(
        target.pixel(DevicePoint::new(12, 12)),
        Some(TEAL.premultiply())
    );

    // A frame with no damage paints nothing; a NaN damage rect is ignored.
    let before = target.clone();
    for damage in [vec![], vec![Rect::new(f32::NAN, 0.0, 5.0, 5.0)]] {
        let fill = Command::Rect {
            rect: Rect::new(0.0, 0.0, 40.0, 30.0),
            color: AMBER,
        };
        let painted = painter
            .paint(
                &mut target,
                &base.view(),
                &frame(vec![fill], damage),
                Scale::ONE,
            )
            .expect("painted");
        assert!(painted.is_empty());
        assert_eq!(target, before);
    }
}

#[test]
fn the_damage_is_cut_to_the_target_and_the_base() {
    let mut painter = painter();
    // A base smaller than the target: only where both are is painted.
    let base = canvas(20, 10, BG);
    let mut target = canvas(40, 30, SENTINEL);
    let damage = painter
        .paint(
            &mut target,
            &base.view(),
            &frame(vec![], vec![Rect::new(-100.0, -100.0, 1000.0, 1000.0)]),
            Scale::ONE,
        )
        .expect("painted");
    assert_eq!(damage.rects(), [DeviceRect::new(0, 0, 20, 10)]);
    assert_eq!(
        target.pixel(DevicePoint::new(19, 9)),
        Some(BG.premultiply())
    );
    assert_eq!(
        target.pixel(DevicePoint::new(20, 9)),
        Some(SENTINEL.premultiply())
    );
}

#[test]
fn many_damage_rects_merge_into_at_most_eight() {
    let mut painter = painter();
    let base = canvas(200, 20, BG);
    let mut target = canvas(200, 20, SENTINEL);
    let damage: Vec<Rect> = (0..32)
        .map(|i| Rect::new(i as f32 * 6.0, 5.0, 2.0, 2.0))
        .collect();
    let painted = painter
        .paint(
            &mut target,
            &base.view(),
            &frame(vec![], damage.clone()),
            Scale::ONE,
        )
        .expect("painted");
    assert!(painted.rects().len() <= 8, "{painted:?}");
    for rect in damage {
        let device = clip::intersect(
            damage_rect(rect, Scale::ONE).expect("a rect"),
            target.bounds(),
        );
        let covered = painted
            .rects()
            .iter()
            .any(|kept| clip::intersect(*kept, device) == device);
        assert!(covered, "{device:?} in {painted:?}");
    }
}

#[test]
fn each_shape_is_what_gfx_draws_for_it() {
    let mut painter = painter();
    let at = scale(1.5);
    let rect = Rect::new(4.3, 3.0, 20.0, 12.0);
    let device = device_rect(rect, at);
    let cases: Vec<(Command, Draw)> = vec![
        (
            Command::Rect { rect, color: GLOW },
            Box::new(move |fb: &mut Framebuffer| {
                let clip = fb.bounds();
                raster::fill_rect(fb, clip, device, GLOW.premultiply());
            }),
        ),
        (
            Command::CutCornerRect {
                rect,
                cut: 4.0,
                corners: Corners::TOP_RIGHT_AND_BOTTOM_LEFT,
                color: AMBER,
            },
            Box::new(move |fb: &mut Framebuffer| {
                let clip = fb.bounds();
                raster::cut_corner_rect(
                    fb,
                    clip,
                    device,
                    6.0,
                    raster::Corners::TOP_RIGHT_AND_BOTTOM_LEFT,
                    AMBER.premultiply(),
                );
            }),
        ),
        (
            Command::Pill { rect, color: TEAL },
            Box::new(move |fb: &mut Framebuffer| {
                let clip = fb.bounds();
                raster::pill(fb, clip, device, TEAL.premultiply());
            }),
        ),
        (
            Command::Border {
                rect,
                width: 1.0,
                color: TEAL,
            },
            Box::new(move |fb: &mut Framebuffer| {
                let clip = fb.bounds();
                raster::border(fb, clip, device, 1, TEAL.premultiply());
            }),
        ),
        (
            Command::Brackets {
                rect,
                arm: 5.0,
                thickness: 1.0,
                corners: Corners::TOP_LEFT_AND_BOTTOM_RIGHT,
                color: TEAL,
            },
            Box::new(move |fb: &mut Framebuffer| {
                let clip = fb.bounds();
                raster::brackets(
                    fb,
                    clip,
                    device,
                    8,
                    1,
                    raster::Corners::TOP_LEFT_AND_BOTTOM_RIGHT,
                    TEAL.premultiply(),
                );
            }),
        ),
        (
            Command::Line {
                from: Point::new(2.0, 2.0),
                to: Point::new(30.0, 18.0),
                width: 1.0,
                color: AMBER,
            },
            Box::new(|fb: &mut Framebuffer| {
                let clip = fb.bounds();
                raster::line(
                    fb,
                    clip,
                    Vertex::new(3.0, 3.0),
                    Vertex::new(45.0, 27.0),
                    1.5,
                    AMBER.premultiply(),
                );
            }),
        ),
    ];
    for (command, draw) in cases {
        let shown = format!("{command:?}");
        let got = painted(&mut painter, vec![command], (60, 36), at);
        let mut expected = canvas(60, 36, BG);
        draw(&mut expected);
        assert_eq!(changed(&got, &expected), None, "{shown}");
    }
}

#[test]
fn a_command_never_inks_outside_its_bounds_cut_to_its_clip() {
    let mut painter = painter();
    // The line's ink reaches column 10 (its end is at 10.1), and its
    // bounds end at 10.35, half its width further; the clip starts at
    // 10.4, which rounds to column 10, but past the bounds: column 10
    // stays.
    let commands = vec![
        Command::PushClip {
            rect: Rect::new(10.4, 0.0, 20.0, 20.0),
        },
        Command::Line {
            from: Point::new(0.0, 5.0),
            to: Point::new(10.1, 5.0),
            width: 0.5,
            color: TEAL,
        },
        Command::PopClip,
    ];
    let got = painted(&mut painter, commands, (30, 20), Scale::ONE);
    assert_eq!(changed(&got, &canvas(30, 20, BG)), None);

    // Glyphs pulled far left of the origin by negative letter-spacing stay
    // inside the text's bounds: half an em left of its origin.
    let style = TextStyle::new(Family::Mono, 10.0).letter_spacing(-3.0);
    let text = Command::Text {
        origin: Point::new(60.0, 20.0),
        text: "abcdef".to_string(),
        style,
        color: TEAL,
    };
    let got = painted(&mut painter, vec![text], (100, 30), Scale::ONE);
    let ink = changed(&got, &canvas(100, 30, BG)).expect("some ink");
    assert!(ink.x >= 55, "{ink:?}");
}

#[test]
fn clips_nest_and_stray_pops_are_ignored() {
    let mut painter = painter();
    let fill = Command::Rect {
        rect: Rect::new(0.0, 0.0, 40.0, 40.0),
        color: TEAL,
    };
    let commands = vec![
        Command::PopClip,
        Command::PopLayer,
        Command::PushClip {
            rect: Rect::new(5.0, 5.0, 20.0, 20.0),
        },
        Command::PushClip {
            rect: Rect::new(10.0, 0.0, 30.0, 12.0),
        },
        // Ends no layer: ignored, so the clips stay.
        Command::PopLayer,
        fill.clone(),
        Command::PopClip,
        // Left open: closed at the end.
        Command::PushClip {
            rect: Rect::new(0.0, 30.0, 4.0, 4.0),
        },
        fill,
    ];
    let got = painted(&mut painter, commands, (40, 40), Scale::ONE);
    let mut expected = canvas(40, 40, BG);
    let clip = expected.bounds();
    raster::fill_rect(
        &mut expected,
        clip,
        DeviceRect::new(10, 5, 15, 7),
        TEAL.premultiply(),
    );
    assert_eq!(changed(&got, &expected), None);
    // The second fill is cut to the first clip and the one left open,
    // which do not meet: nothing more is drawn.
}

#[test]
fn a_layer_is_composited_whole_at_its_opacity() {
    let mut painter = painter();
    let squares = || {
        vec![
            Command::Rect {
                rect: Rect::new(2.0, 2.0, 16.0, 16.0),
                color: TEAL,
            },
            Command::Rect {
                rect: Rect::new(10.0, 10.0, 16.0, 16.0),
                color: AMBER,
            },
        ]
    };
    let layered = |opacity: f32| {
        let mut commands = vec![Command::PushLayer { opacity }];
        commands.extend(squares());
        commands.push(Command::PopLayer);
        commands
    };
    // At 0.5: the group, then composited at 128.
    let got = painted(&mut painter, layered(0.5), (30, 30), Scale::ONE);
    let mut group = Framebuffer::new(DeviceSize::new(30, 30)).expect("a layer");
    let clip = group.bounds();
    raster::fill_rect(
        &mut group,
        clip,
        DeviceRect::new(2, 2, 16, 16),
        TEAL.premultiply(),
    );
    raster::fill_rect(
        &mut group,
        clip,
        DeviceRect::new(10, 10, 16, 16),
        AMBER.premultiply(),
    );
    let mut expected = canvas(30, 30, BG);
    blend::composite(&mut expected, clip, &group, 128);
    assert_eq!(changed(&got, &expected), None);
    // The overlap shows amber only, not teal through it: as where amber is
    // alone.
    assert_eq!(
        got.pixel(DevicePoint::new(12, 12)),
        got.pixel(DevicePoint::new(20, 20))
    );

    // Opaque draws straight through; transparent draws nothing; NaN is
    // opaque.
    let plain = painted(&mut painter, squares(), (30, 30), Scale::ONE);
    assert_eq!(
        painted(&mut painter, layered(1.0), (30, 30), Scale::ONE),
        plain
    );
    assert_eq!(
        painted(&mut painter, layered(f32::NAN), (30, 30), Scale::ONE),
        plain
    );
    assert_eq!(
        painted(&mut painter, layered(0.0), (30, 30), Scale::ONE),
        canvas(30, 30, BG)
    );
    // A layer left open is composited at the end.
    let mut open = layered(0.5);
    open.pop();
    assert_eq!(
        changed(
            &painted(&mut painter, open, (30, 30), Scale::ONE),
            &expected
        ),
        None
    );
}

#[test]
fn layers_nested_past_the_limit_draw_into_the_one_around_them() {
    let mut painter = painter();
    let nested = |depth: usize| {
        let mut commands = vec![Command::PushLayer { opacity: 0.5 }; depth];
        commands.push(Command::Rect {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            color: Color::WHITE,
        });
        commands.extend(vec![Command::PopLayer; depth]);
        commands
    };
    let limit = painted(&mut painter, nested(MAX_LAYERS), (10, 10), Scale::ONE);
    // Each layer halves the white once; past the limit, no more.
    assert_ne!(
        limit,
        painted(&mut painter, nested(MAX_LAYERS - 1), (10, 10), Scale::ONE)
    );
    for depth in [MAX_LAYERS + 1, 100, 10_000] {
        assert_eq!(
            painted(&mut painter, nested(depth), (10, 10), Scale::ONE),
            limit,
            "{depth}"
        );
    }
    // Inside a hidden layer nothing draws, at any depth.
    let mut hidden = vec![Command::PushLayer { opacity: 0.0 }];
    hidden.extend(nested(50));
    hidden.push(Command::PopLayer);
    assert_eq!(
        painted(&mut painter, hidden, (10, 10), Scale::ONE),
        canvas(10, 10, BG)
    );
}

#[test]
fn a_layer_is_cut_to_its_clip_and_its_clip_to_the_damage() {
    let mut painter = painter();
    let base = canvas(30, 30, BG);
    let mut target = canvas(30, 30, SENTINEL);
    let commands = vec![
        Command::PushClip {
            rect: Rect::new(0.0, 0.0, 12.0, 30.0),
        },
        Command::PushLayer { opacity: 0.5 },
        Command::Rect {
            rect: Rect::new(0.0, 0.0, 30.0, 30.0),
            color: Color::WHITE,
        },
        Command::PopLayer,
        Command::PopClip,
    ];
    painter
        .paint(
            &mut target,
            &base.view(),
            &frame(commands, vec![Rect::new(5.0, 5.0, 20.0, 20.0)]),
            Scale::ONE,
        )
        .expect("painted");
    let half = blend::scale(Color::WHITE.premultiply(), 128).over(BG.premultiply());
    assert_eq!(target.pixel(DevicePoint::new(5, 5)), Some(half));
    assert_eq!(target.pixel(DevicePoint::new(11, 20)), Some(half));
    assert_eq!(
        target.pixel(DevicePoint::new(12, 20)),
        Some(BG.premultiply())
    );
    assert_eq!(
        target.pixel(DevicePoint::new(4, 4)),
        Some(SENTINEL.premultiply())
    );
}

#[test]
fn text_is_set_on_a_whole_pixel_baseline_from_a_quarter_pixel_pen() {
    let mut painter = painter();
    let style = TextStyle::new(Family::Ui, 13.0).weight(Weight::SemiBold);
    let text = |x: f32, y: f32| Command::Text {
        origin: Point::new(x, y),
        text: "Tidepool".to_string(),
        style,
        color: TEAL,
    };
    let at = |x, y| {
        painted(
            &mut Painter::new().expect("faces"),
            vec![text(x, y)],
            (120, 40),
            Scale::ONE,
        )
    };
    let whole = at(10.0, 20.0);
    // The baseline rounds: 20.4 is row 20, 20.5 row 21.
    assert_eq!(at(10.0, 20.4), whole);
    let lower = at(10.0, 21.0);
    assert_eq!(at(10.0, 20.5), lower);
    // A quarter pixel changes the coverage; a whole one moves it.
    assert_ne!(at(10.25, 20.0), whole);
    let next = at(11.0, 20.0);
    for y in 0..40 {
        for x in 0..119 {
            assert_eq!(
                whole.pixel(DevicePoint::new(x, y)),
                next.pixel(DevicePoint::new(x + 1, y)),
                "({x}, {y})"
            );
        }
    }

    // What it draws is each glyph of `font`'s layout, as `font`
    // rasterizes it, its origin on the pen's whole pixel and the baseline.
    let font = crate::metrics::font_style(&style, Scale::ONE).expect("a style");
    let line = layout::line(painter.faces(), "Tidepool", &font).expect("a line");
    let mut expected = canvas(120, 40, BG);
    let clip = expected.bounds();
    for placed in &line.glyphs {
        let id = placed.glyph.expect("no tabs");
        let (pixel, subpixel) = Subpixel::split(10.0 + placed.x);
        let key = GlyphKey {
            face: placed.face,
            glyph: id,
            size: font.size,
            subpixel,
        };
        let bitmap = atlas::rasterize(painter.faces(), key).expect("a glyph");
        let size = DeviceSize::new(side(bitmap.width), side(bitmap.height));
        let mask = Mask::new(&bitmap.pixels, size, bitmap.width as usize).expect("a mask");
        let at = DevicePoint::new(pixel + bitmap.left, 20 + bitmap.top);
        glyph::draw(&mut expected, clip, &mask, at, TEAL.premultiply());
    }
    assert_eq!(changed(&whole, &expected), None);

    // And where: a flat-based capital sits on the baseline, its last row
    // the one above it, and starts at its pen, its bearing under a pixel.
    let capitals = Command::Text {
        origin: Point::new(10.0, 20.0),
        text: "TH".to_string(),
        style,
        color: TEAL,
    };
    let got = painted(&mut painter, vec![capitals], (120, 40), Scale::ONE);
    let ink = changed(&got, &canvas(120, 40, BG)).expect("ink");
    assert_eq!(ink.y + ink.height, 20, "{ink:?}");
    assert!((10..=11).contains(&ink.x), "{ink:?}");
    // Saira's capitals are about 0.7 em tall: 9 px at 13 px.
    assert!((8..=11).contains(&ink.height), "{ink:?}");
}

#[test]
fn a_glyph_too_large_for_the_atlas_is_drawn_on_its_own() {
    let mut painter = painter();
    // At 900 px an `H` is over 512 px tall: over half the atlas's side.
    let style = TextStyle::new(Family::Ui, 900.0);
    let text = Command::Text {
        origin: Point::new(10.0, 800.0),
        text: "H".to_string(),
        style,
        color: TEAL,
    };
    let got = painted(&mut painter, vec![text], (800, 1000), Scale::ONE);
    let ink = changed(&got, &canvas(800, 1000, BG)).expect("the glyph is drawn");
    assert!(ink.height > 512 && ink.width > 300, "{ink:?}");
    assert!(painter.atlas.is_empty(), "not cached");
}

#[test]
fn text_font_cannot_set_draws_nothing() {
    let mut painter = painter();
    for size in [0.0, -1.0, f32::NAN, 5000.0] {
        let text = Command::Text {
            origin: Point::new(10.0, 20.0),
            text: "abc".to_string(),
            style: TextStyle::new(Family::Mono, size),
            color: TEAL,
        };
        let got = painted(&mut painter, vec![text], (60, 30), Scale::ONE);
        assert_eq!(got, canvas(60, 30, BG), "{size}");
    }
}

#[test]
fn an_icon_is_scaled_to_fit_centred_and_clipped_to_its_rect() {
    let mut painter = painter();
    let square = vec![on(0.0, 0.0), on(8.0, 0.0), on(8.0, 8.0), on(0.0, 8.0)];
    let icon = Icon::new(8.0, square, vec![4]).expect("an icon");
    assert_eq!(painter.set_icon(IconId(3), icon.clone()), None);
    assert_eq!(
        painter.set_icon(IconId(3), icon.clone()),
        Some(icon.clone())
    );
    assert_eq!(painter.icon(IconId(3)), Some(&icon));
    // In a 10 × 6 rect, the square is 6 across, centred: columns 2 to 8.
    let command = Command::Icon {
        rect: Rect::new(4.0, 4.0, 10.0, 6.0),
        icon: IconId(3),
        color: TEAL,
    };
    let got = painted(&mut painter, vec![command.clone()], (20, 20), Scale::ONE);
    let mut expected = canvas(20, 20, BG);
    let clip = expected.bounds();
    raster::fill_rect(
        &mut expected,
        clip,
        DeviceRect::new(6, 4, 6, 6),
        TEAL.premultiply(),
    );
    assert_eq!(changed(&got, &expected), None);
    // At scale 2 it is 12 across, at (12, 8).
    let got = painted(&mut painter, vec![command], (40, 40), scale(2.0));
    let mut expected = canvas(40, 40, BG);
    let clip = expected.bounds();
    raster::fill_rect(
        &mut expected,
        clip,
        DeviceRect::new(12, 8, 12, 12),
        TEAL.premultiply(),
    );
    assert_eq!(changed(&got, &expected), None);
}

#[test]
fn an_unknown_icon_is_a_hollow_box_and_an_unknown_image_nothing() {
    let mut painter = painter();
    let rect = Rect::new(2.0, 3.0, 10.0, 8.0);
    let got = painted(
        &mut painter,
        vec![Command::Icon {
            rect,
            icon: IconId(99),
            color: TEAL,
        }],
        (20, 20),
        Scale::ONE,
    );
    let mut expected = canvas(20, 20, BG);
    let clip = expected.bounds();
    raster::border(
        &mut expected,
        clip,
        DeviceRect::new(2, 3, 10, 8),
        1,
        TEAL.premultiply(),
    );
    assert_eq!(changed(&got, &expected), None);

    let got = painted(
        &mut painter,
        vec![Command::Image {
            rect,
            image: ImageId(7),
        }],
        (20, 20),
        Scale::ONE,
    );
    assert_eq!(got, canvas(20, 20, BG));
    assert!(painter.remove_icon(IconId(99)).is_none());
}

#[test]
fn an_image_is_scaled_to_its_rect_bilinearly() {
    let mut painter = painter();
    let mut image = canvas(4, 4, TEAL);
    let clip = image.bounds();
    raster::fill_rect(
        &mut image,
        clip,
        DeviceRect::new(0, 0, 2, 2),
        AMBER.premultiply(),
    );
    // Its pixels are drawn from its top-left, wherever its bounds are.
    let mut moved = Framebuffer::new_at(DeviceRect::new(100, 100, 4, 4)).expect("an image");
    image::copy(
        &mut moved,
        DeviceRect::new(100, 100, 4, 4),
        &image.view(),
        DevicePoint::new(100, 100),
    );
    assert!(painter.set_image(ImageId(1), moved).is_none());
    let rect = Rect::new(2.0, 2.0, 12.0, 8.0);
    let command = Command::Image {
        rect,
        image: ImageId(1),
    };
    let got = painted(&mut painter, vec![command], (20, 20), Scale::ONE);
    let mut expected = canvas(20, 20, BG);
    let clip = expected.bounds();
    image::blit_scaled(
        &mut expected,
        clip,
        &image.view(),
        DeviceRect::new(2, 2, 12, 8),
        Filter::Bilinear,
    );
    assert_eq!(changed(&got, &expected), None);
    assert!(painter.image(ImageId(1)).is_some());
    assert!(painter.remove_image(ImageId(1)).is_some());
    assert!(painter.image(ImageId(1)).is_none());
}

#[test]
fn no_geometry_panics_or_draws_outside_the_damage() {
    let mut painter = painter();
    let square = vec![on(0.0, 0.0), on(1.0, 0.0), on(1.0, 1.0)];
    painter.set_icon(IconId(1), Icon::new(1.0, square, vec![3]).expect("an icon"));
    painter.set_image(ImageId(1), canvas(3, 3, TEAL));
    let odd = [
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::MAX,
        -f32::MAX,
        1e9,
        -1e9,
        0.0,
        3.5,
    ];
    let base = canvas(16, 16, BG);
    for &a in &odd {
        for &b in &odd {
            let rect = Rect::new(a, 2.0, b, 5.0);
            let other = Rect::new(1.0, a, 6.0, b);
            let commands = vec![
                Command::Rect { rect, color: TEAL },
                Command::CutCornerRect {
                    rect: other,
                    cut: a,
                    corners: Corners::ALL,
                    color: TEAL,
                },
                Command::Pill { rect, color: TEAL },
                Command::Border {
                    rect: other,
                    width: b,
                    color: TEAL,
                },
                Command::Brackets {
                    rect,
                    arm: a,
                    thickness: b,
                    corners: Corners::ALL,
                    color: TEAL,
                },
                Command::Line {
                    from: Point::new(a, b),
                    to: Point::new(b, a),
                    width: a,
                    color: TEAL,
                },
                Command::Text {
                    origin: Point::new(a, b),
                    text: "x\u{1F600}".to_string(),
                    style: TextStyle::new(Family::Ui, b).letter_spacing(a),
                    color: TEAL,
                },
                Command::Icon {
                    rect,
                    icon: IconId(1),
                    color: TEAL,
                },
                Command::Image {
                    rect: other,
                    image: ImageId(1),
                },
                Command::PushClip { rect },
                Command::PushLayer { opacity: a },
                Command::Rect {
                    rect: other,
                    color: TEAL,
                },
            ];
            for factor in [1.0, 2.0, 1e-3, 1e6] {
                let mut target = canvas(16, 16, SENTINEL);
                let damage = Rect::new(4.0 / factor, 4.0 / factor, 4.0 / factor, 4.0 / factor);
                let painted = painter
                    .paint(
                        &mut target,
                        &base.view(),
                        &frame(commands.clone(), vec![damage]),
                        scale(factor),
                    )
                    .expect("painted");
                let outside = |at: DevicePoint| {
                    !painted.rects().iter().any(|rect| {
                        clip::intersect(*rect, DeviceRect::new(at.x, at.y, 1, 1))
                            != DeviceRect::ZERO
                    })
                };
                for y in 0..16 {
                    for x in 0..16 {
                        let at = DevicePoint::new(x, y);
                        if outside(at) {
                            assert_eq!(
                                target.pixel(at),
                                Some(SENTINEL.premultiply()),
                                "{a} {b} {factor}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn neighbours_ui_places_share_their_edge_at_any_scale() {
    // `ui` places a sibling at the `f32` sum of the one before; summed in
    // `f64` that edge rounds to 51 at scale 1, and the next starts at 52.
    let left = Rect::new(34.333_332, 0.0, 17.166_666, 10.0);
    let right = Rect::new(left.x + left.width, 0.0, 17.0, 10.0);
    assert_eq!(right.x, 51.5);
    for factor in [1.0, 1.25, 1.5, 2.0] {
        let (a, b) = (
            device_rect(left, scale(factor)),
            device_rect(right, scale(factor)),
        );
        assert_eq!(a.x + a.width, b.x, "at {factor}");
    }

    // A row of equal fills that `ui` lays out leaves no seam: every pixel
    // of the row is teal over a base that is not.
    // A grid's columns too: 12 in 121 units put a cell's edge at 60.5.
    let grid = |width: f32, columns: u16| LayoutStyle::grid(columns).fixed(width, 10.0);
    let rows = [
        (LayoutStyle::row().fixed(103.0, 10.0), 103.0, 6, 1.0),
        (LayoutStyle::row().fixed(100.0, 10.0), 100.0, 6, 1.25),
        (LayoutStyle::row().fixed(137.0, 10.0), 137.0, 7, 1.5),
        (grid(121.0, 12), 121.0, 12, 1.0),
        (grid(173.0, 9), 173.0, 9, 1.5),
    ];
    for (container, width, count, factor) in rows {
        let mut tree = Tree::new();
        let root = tree.root();
        let row = tree.insert(root, Node::new(container)).expect("a row");
        for _ in 0..count {
            let fill = LayoutStyle::default()
                .width(Length::Fill(1.0))
                .height(Length::Fixed(10.0));
            tree.insert(row, Node::new(fill).with_widget(Fill(TEAL)))
                .expect("a fill");
        }
        let mut painter = painter();
        let at = scale(factor);
        let window = Size::new(width, 10.0);
        let frame = tree.frame(window, &painter.metrics(at)).expect("a frame");
        let device = (width * factor) as i32;
        let base = canvas(device, 12, BG);
        let mut target = canvas(device, 12, SENTINEL);
        painter
            .paint(&mut target, &base.view(), &frame, at)
            .expect("painted");
        for x in 0..device {
            assert_eq!(
                target.pixel(DevicePoint::new(x, 5)),
                Some(TEAL.premultiply()),
                "column {x} of {width} at {factor}"
            );
        }
    }
}

/// Fills its bounds.
struct Fill(Color);

impl Widget for Fill {
    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        list.rect(cx.bounds(), self.0);
    }
}

#[test]
fn a_layer_is_made_where_it_is_inked_and_grows_to_keep_what_it_holds() {
    let limit = DeviceRect::new(0, 0, 200, 100);
    let mut layer = Layer {
        fb: None,
        alpha: 128,
        limit,
    };
    let first = DeviceRect::new(10, 10, 4, 4);
    let fb = layer.cover(first).expect("made");
    assert_eq!(fb.bounds(), first);
    let clip = fb.bounds();
    raster::fill_rect(fb, clip, first, TEAL.premultiply());
    // Inside what it covers: the same framebuffer.
    assert_eq!(
        layer
            .cover(DeviceRect::new(11, 11, 2, 2))
            .expect("kept")
            .bounds(),
        first
    );
    // Past it: grown by at least its own size each way it must, what it
    // held kept, the rest transparent.
    let fb = layer.cover(DeviceRect::new(15, 2, 2, 2)).expect("grown");
    assert_eq!(fb.bounds(), DeviceRect::new(10, 2, 8, 12));
    assert_eq!(fb.pixel(DevicePoint::new(12, 12)), Some(TEAL.premultiply()));
    assert_eq!(
        fb.pixel(DevicePoint::new(16, 5)),
        Some(PremultipliedColor::TRANSPARENT)
    );
    // Far past it: to what it must cover, which is more than its size.
    let fb = layer.cover(DeviceRect::new(150, 90, 4, 4)).expect("grown");
    assert_eq!(fb.bounds(), DeviceRect::new(10, 2, 144, 92));
    // Never past its limit, the clip it was pushed in, though doubling
    // would take it there.
    let fb = layer.cover(DeviceRect::new(198, 98, 1, 1)).expect("grown");
    assert_eq!(fb.bounds(), DeviceRect::new(10, 2, 190, 98));
    assert_eq!(fb.pixel(DevicePoint::new(12, 12)), Some(TEAL.premultiply()));

    // Inked row after row, it is copied a few times, not once a row.
    let mut layer = Layer {
        fb: None,
        alpha: 128,
        limit: DeviceRect::new(0, 0, 400, 1000),
    };
    let mut copies = 0;
    let mut bounds = DeviceRect::ZERO;
    for row in 0..1000 {
        let fb = layer
            .cover(DeviceRect::new(0, row, 400, 1))
            .expect("covered");
        if fb.bounds() != bounds {
            copies += 1;
            bounds = fb.bounds();
        }
    }
    assert!(copies <= 11, "{copies} framebuffers for 1,000 rows");

    // Through the walk: a small faded command in a large damage rect gets a
    // layer of its size.
    let mut walk = Walk {
        clips: ClipStack::new(DeviceRect::new(0, 0, 200, 200)),
        logical: vec![EVERYTHING],
        open: Vec::new(),
        layers: Vec::new(),
        hidden: 0,
    };
    walk.push_layer(0.5);
    let mut target = canvas(200, 200, BG);
    let ink = DeviceRect::new(40, 40, 3, 3);
    assert_eq!(
        walk.target(ink, &mut target).expect("a layer").bounds(),
        ink
    );
    assert_eq!(walk.layers.len(), 1);

    // And paints as compositing it would.
    let mut painter = painter();
    let commands = vec![
        Command::PushLayer { opacity: 0.5 },
        Command::Rect {
            rect: Rect::new(40.0, 40.0, 3.0, 3.0),
            color: TEAL,
        },
        Command::PopLayer,
    ];
    let got = painted(&mut painter, commands, (200, 200), Scale::ONE);
    let half = blend::scale(TEAL.premultiply(), 128).over(BG.premultiply());
    assert_eq!(got.pixel(DevicePoint::new(41, 41)), Some(half));
    assert_eq!(
        changed(&got, &canvas(200, 200, BG)),
        Some(DeviceRect::new(40, 40, 3, 3))
    );
}

#[test]
fn a_path_fills_the_same_whatever_clips_it() {
    // A slanted line and a cut corner, painted whole and in many damage
    // rects of odd sizes, give the same pixels: their coverage is summed in
    // fixed tiles, not from wherever a damage rect starts.
    let mut painter = painter();
    let commands = vec![
        Command::Line {
            from: Point::new(3.3, 2.7),
            to: Point::new(150.9, 97.1),
            width: 1.7,
            color: TEAL,
        },
        Command::CutCornerRect {
            rect: Rect::new(20.0, 40.0, 130.0, 50.0),
            cut: 13.3,
            corners: Corners::ALL,
            color: GLOW,
        },
    ];
    let whole = painted(&mut painter, commands.clone(), (160, 100), Scale::ONE);
    let base = canvas(160, 100, BG);
    for step in [3, 7, 13, 29, 61] {
        let mut target = canvas(160, 100, SENTINEL);
        let mut y = 0;
        while y < 100 {
            let mut x = 0;
            while x < 160 {
                let damage = vec![Rect::new(x as f32, y as f32, step as f32, step as f32)];
                painter
                    .paint(
                        &mut target,
                        &base.view(),
                        &frame(commands.clone(), damage),
                        Scale::ONE,
                    )
                    .expect("painted");
                x += step;
            }
            y += step;
        }
        assert_eq!(changed(&target, &whole), None, "in {step} px pieces");
    }

    // A line reaching past the window's left edge, found by the property
    // test (`TIDEPOOL_SEED=36`): without tiles, the damage rect starting at
    // column 2 moved a pixel of it by a level.
    let line = vec![Command::Line {
        from: Point::new(-3.108_850_7, 25.010_767),
        to: Point::new(11.5, 28.5),
        width: 3.633_711,
        color: Color::rgba(77, 203, 17, 200),
    }];
    let whole = painted(&mut painter, line.clone(), (64, 48), Scale::ONE);
    let base = canvas(64, 48, BG);
    let mut target = whole.clone();
    let damage = vec![Rect::new(2.0, 1.0, 62.0, 47.0)];
    painter
        .paint(&mut target, &base.view(), &frame(line, damage), Scale::ONE)
        .expect("painted");
    assert_eq!(changed(&target, &whole), None);
}

#[test]
fn a_tile_is_skipped_only_where_a_line_cannot_reach_it() {
    let tile = DeviceRect::new(64, 64, 64, 64);
    let (from, to) = (Vertex::new(0.0, 0.0), Vertex::new(300.0, 300.0));
    // Through it, and past it within the margin, but not beyond it.
    assert!(segment_reaches(from, to, tile, 1.5));
    let beside = (Vertex::new(0.0, 60.0), Vertex::new(300.0, 60.0));
    assert!(segment_reaches(beside.0, beside.1, tile, 4.5));
    assert!(!segment_reaches(beside.0, beside.1, tile, 3.5));
    let short = (Vertex::new(0.0, 100.0), Vertex::new(60.0, 100.0));
    assert!(!segment_reaches(short.0, short.1, tile, 3.5));
    assert!(segment_reaches(short.0, short.1, tile, 4.5));
    // A point, and a level or upright line, are segments too.
    assert!(segment_reaches(
        Vertex::new(70.0, 70.0),
        Vertex::new(70.0, 70.0),
        tile,
        1.0
    ));
    assert!(segment_reaches(
        Vertex::new(100.0, 0.0),
        Vertex::new(100.0, 500.0),
        tile,
        1.0
    ));
    assert!(!segment_reaches(
        Vertex::new(200.0, 0.0),
        Vertex::new(200.0, 500.0),
        tile,
        1.0
    ));
    // Not finite: nothing, as `gfx` draws it.
    assert!(!segment_reaches(Vertex::new(f32::NAN, 0.0), to, tile, 1.0));
    assert!(!segment_reaches(from, to, tile, f64::INFINITY));

    // A skipped tile is one the line inks nothing in: a line 5 px wide 4
    // px above a tile, its margin 3.5, covers none of its pixels.
    let mut fb = canvas(200, 200, BG);
    let clip = fb.bounds();
    raster::line(&mut fb, clip, beside.0, beside.1, 5.0, TEAL.premultiply());
    for x in 64..128 {
        assert_eq!(fb.pixel(DevicePoint::new(x, 64)), Some(BG.premultiply()));
    }
}

#[test]
fn a_cut_corner_tile_is_solid_only_clear_of_the_cuts() {
    let rect = DeviceRect::new(0, 0, 256, 128);
    let corners = raster::Corners::TOP_RIGHT_AND_BOTTOM_LEFT;
    let tile = |x, y| DeviceRect::new(x, y, 64, 64);
    // A cut of 10 at the top right and bottom left.
    assert_eq!(
        cut_corner_tile(tile(0, 0), rect, 10.0, corners),
        Tile::Solid
    );
    assert_eq!(
        cut_corner_tile(tile(192, 0), rect, 10.0, corners),
        Tile::Path
    );
    assert_eq!(
        cut_corner_tile(tile(0, 64), rect, 10.0, corners),
        Tile::Path
    );
    assert_eq!(
        cut_corner_tile(tile(192, 64), rect, 10.0, corners),
        Tile::Solid
    );
    // Past the rect, and across its edge.
    assert_eq!(
        cut_corner_tile(tile(256, 0), rect, 10.0, corners),
        Tile::Empty
    );
    assert_eq!(
        cut_corner_tile(DeviceRect::new(230, 100, 64, 64), rect, 10.0, corners),
        Tile::Path
    );
    // A cut too long for the rect is shortened to half its shorter side,
    // 64: the tile beside the corner's square is solid.
    assert_eq!(
        cut_corner_tile(tile(128, 0), rect, 500.0, corners),
        Tile::Solid
    );
    assert_eq!(
        cut_corner_tile(tile(192, 0), rect, 500.0, corners),
        Tile::Path
    );

    // A solid tile's pixels are those the path covers whole: drawn by
    // `gfx` whole, the tile at (0, 0) is all the color.
    let mut fb = canvas(256, 128, BG);
    let clip = fb.bounds();
    raster::cut_corner_rect(&mut fb, clip, rect, 10.0, corners, GLOW.premultiply());
    let solid = GLOW.premultiply().over(BG.premultiply());
    for y in 0..64 {
        for x in 0..64 {
            assert_eq!(fb.pixel(DevicePoint::new(x, y)), Some(solid), "({x}, {y})");
        }
    }
}

#[test]
fn a_text_with_a_nan_origin_draws_nothing() {
    let mut painter = painter();
    for origin in [Point::new(f32::NAN, 20.0), Point::new(10.0, f32::NAN)] {
        let text = Command::Text {
            origin,
            text: "Tidepool".to_string(),
            style: TextStyle::new(Family::Ui, 13.0),
            color: TEAL,
        };
        assert_eq!(
            painted(&mut painter, vec![text], (120, 40), Scale::ONE),
            canvas(120, 40, BG),
            "{origin:?}"
        );
    }
}

#[test]
fn a_nan_cut_draws_nothing() {
    let mut painter = painter();
    let command = Command::CutCornerRect {
        rect: Rect::new(2.0, 2.0, 10.0, 10.0),
        cut: f32::NAN,
        corners: Corners::ALL,
        color: TEAL,
    };
    assert_eq!(
        painted(&mut painter, vec![command], (16, 16), Scale::ONE),
        canvas(16, 16, BG)
    );
}

#[test]
fn the_metrics_measure_at_the_scale_given() {
    let painter = painter();
    let style = TextStyle::new(Family::Ui, 13.0);
    assert_eq!(painter.metrics(scale(2.0)).scale(), scale(2.0));
    let one = painter.metrics(Scale::ONE).width("Explorer", &style);
    let two = painter.metrics(scale(2.0)).width("Explorer", &style);
    assert!(one > 40.0 && (one - two).abs() < 0.5, "{one} {two}");
}
