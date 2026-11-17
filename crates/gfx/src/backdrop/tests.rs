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

//! Unit tests of the backdrop: the site's values, validation, exact pixels
//! where the layers are known, the cache's rebuilds, and the box blur.

use tidepool_core::color::PremultipliedColor;
use tidepool_core::geom::{DevicePoint, DeviceRect};

use super::*;
use crate::blend::to_bgra as bytes_of;

fn scale(factor: f32) -> Scale {
    Scale::new(factor).expect("a valid scale")
}

fn parse(css: &str) -> Color {
    Color::parse(css).expect("valid CSS color")
}

/// The site's background, #0b0c0d, as pixel bytes: blue, green, red, alpha.
const BG: [u8; 4] = [0x0d, 0x0c, 0x0b, 0xff];

#[test]
fn the_site_style_is_the_stylesheet() {
    let style = BackdropStyle::jabbott_dev();
    assert_eq!(style, BackdropStyle::default());
    assert_eq!(style.validate(), Ok(()));
    // Every color as `Color::parse` reads the CSS text.
    assert_eq!(style.bg, parse("#0b0c0d"));
    let glows: Vec<Color> = style.glows.iter().map(|glow| glow.color).collect();
    assert_eq!(
        glows,
        [
            parse("rgba(92, 203, 203, 0.12)"),
            parse("rgba(242, 194, 0, 0.05)"),
            parse("rgba(38, 70, 82, 0.4)"),
        ]
    );
    let planet = &style.planet;
    assert_eq!(planet.core_color, parse("#182a30"));
    assert_eq!(planet.mid_color, parse("#0d1214"));
    assert_eq!(planet.ring_color, parse("rgba(143, 229, 229, 0.18)"));
    assert_eq!(planet.glow.color, parse("rgba(92, 203, 203, 0.18)"));
    assert_eq!(planet.inner_glow.color, parse("rgba(92, 203, 203, 0.12)"));
    let stars: Vec<Color> = style.stars.iter().map(|star| star.color).collect();
    assert_eq!(
        stars,
        [
            parse("rgba(255, 255, 255, 0.9)"),
            parse("rgba(255, 255, 255, 0.55)"),
            parse("rgba(205, 245, 255, 0.85)"),
            parse("rgba(255, 255, 255, 0.5)"),
            parse("rgba(255, 255, 255, 0.7)"),
            parse("rgba(255, 236, 200, 0.7)"),
            parse("rgba(255, 255, 255, 0.45)"),
        ]
    );
    assert_eq!(style.grid.color, parse("rgba(255, 255, 255, 0.022)"));
}

#[test]
fn validation_names_the_field() {
    let invalid = |field, index| Err(GfxError::InvalidStyle { field, index });
    let mut style = BackdropStyle::jabbott_dev();
    style.glows[0].radius_x = f32::NAN;
    assert_eq!(style.validate(), invalid("glows.radius_x", Some(0)));

    let mut style = BackdropStyle::jabbott_dev();
    style.glows[2].fade_stop = -0.1;
    assert_eq!(style.validate(), invalid("glows.fade_stop", Some(2)));

    let mut style = BackdropStyle::jabbott_dev();
    style.planet.glow.blur = -1.0;
    assert_eq!(style.validate(), invalid("planet.glow.blur", None));

    let mut style = BackdropStyle::jabbott_dev();
    style.planet.opacity = 1.5;
    assert_eq!(style.validate(), invalid("planet.opacity", None));

    let mut style = BackdropStyle::jabbott_dev();
    style.stars[3].x = f32::INFINITY;
    assert_eq!(style.validate(), invalid("stars.x", Some(3)));

    let mut style = BackdropStyle::jabbott_dev();
    style.far_stars.opacity = f32::NAN;
    assert_eq!(style.validate(), invalid("far_stars.opacity", None));

    let mut style = BackdropStyle::jabbott_dev();
    style.grid.cell = -64.0;
    assert_eq!(style.validate(), invalid("grid.cell", None));

    let mut style = BackdropStyle::jabbott_dev();
    style.blur_radius = f32::NEG_INFINITY;
    assert_eq!(style.validate(), invalid("blur_radius", None));

    // Positions and offsets may be negative, as the site's are.
    let mut style = BackdropStyle::jabbott_dev();
    style.planet.inner_glow.offset_x = -5.0;
    style.grid.mask_center_y = -1.0;
    assert_eq!(style.validate(), Ok(()));
    // Zero is allowed everywhere.
    style.planet.diameter = 0.0;
    style.near_stars.width = 0.0;
    style.grid.cell = 0.0;
    style.blur_radius = 0.0;
    assert_eq!(style.validate(), Ok(()));
}

#[test]
fn a_pixel_far_from_every_layer_is_the_background_exactly() {
    // At 2000 by 1200 CSS px, the pixel (1000, 400) is outside the three
    // glows: (1000.5 − 1700) / 1100 and (400.5 + 120) / 620 give
    // t = 1.05 > 0.62 for the first; (1000.5 + 160) / 900 = 1.29 > 0.60
    // for the second; (1500 − 400.5) / 900 = 1.22 > 0.62 for the third.
    // It is 1298 px from the planet's centre (1720, −680), past its glow,
    // which ends 900 + 140 + 40 px out. In the near tiles it is at
    // (60.5, 140.5), and in the far ones at (120.5, 150.5), over 14 px
    // from every dot. And 1000 and 400 are not multiples of 64, so no grid
    // line crosses it.
    let style = BackdropStyle::jabbott_dev();
    let painter = Painter::new(DeviceSize::new(2000, 1200), scale(1.0), &style);
    assert_eq!(painter.pixel(1000, 400), BG);
    // At scale 2 the same CSS point is the same color.
    let painter = Painter::new(DeviceSize::new(4000, 2400), scale(2.0), &style);
    assert_eq!(painter.pixel(2000, 800), BG);
    assert_eq!(painter.pixel(2001, 801), BG);
}

#[test]
fn the_layers_look_like_the_site() {
    // These check the shape of the look, not exact values.
    let style = BackdropStyle::jabbott_dev();
    let painter = Painter::new(DeviceSize::new(1600, 900), scale(1.0), &style);
    let rgb = |x, y| {
        let [b, g, r, a] = painter.pixel(x, y);
        assert_eq!(a, 255, "the backdrop is opaque");
        (i32::from(r), i32::from(g), i32::from(b))
    };
    let brightness = |(r, g, b): (i32, i32, i32)| r + g + b;
    let bg = (0x0b, 0x0c, 0x0d);

    // The planet's bottom, inside its circle near (1320, 219): its fill's
    // teal-grey core, under the inner glow, brighter than the background.
    let planet = rgb(1320, 210);
    assert!(planet.1 > planet.0 && planet.2 > planet.0, "{planet:?}");
    assert!(brightness(planet) > brightness(bg) + 60, "{planet:?}");
    // Just below it, the ring and the outer glow: teal-tinted too.
    let glow = rgb(1320, 240);
    assert!(
        glow.1 > glow.0 && glow.2 > glow.0 && brightness(glow) > brightness(bg),
        "{glow:?}"
    );
    // Far below-left of the planet, the glows fade: darker than the glow.
    assert!(brightness(rgb(400, 450)) < brightness(glow));
    // A grid line near the top centre, under the mask, is lighter than the
    // pixels beside it.
    let line = rgb(832, 2);
    let beside = rgb(833, 2);
    assert!(brightness(line) > brightness(beside), "{line:?} {beside:?}");
    // The bottom centre is lit by the third glow, slate blue.
    let bottom = rgb(800, 899);
    assert!(
        bottom.2 > bottom.0 && brightness(bottom) > brightness(bg) + 30,
        "{bottom:?}"
    );
}

#[test]
fn stars_land_where_the_tiles_put_them() {
    // The near tiles, 300 by 260, start at (−260, −260), so the first
    // dot, at (25, 35) in a tile, is at (25 − 260 + 300, 35 − 260 + 260)
    // = (65, 35) in the frame. It fades out 0.6 px from there, so at
    // scale 1 it lights parts of the four pixels around that point, and
    // none 3 px away. No other dot of either layer is near.
    let style = BackdropStyle::jabbott_dev();
    let mut plain = style.clone();
    plain.stars.clear();
    let size = DeviceSize::new(1600, 900);
    let with = Painter::new(size, scale(1.0), &style);
    let without = Painter::new(size, scale(1.0), &plain);
    for (x, y) in [(64, 34), (65, 34), (64, 35), (65, 35)] {
        assert_ne!(with.pixel(x, y), without.pixel(x, y), "({x}, {y})");
    }
    for (x, y) in [(62, 35), (68, 35), (65, 32), (65, 38)] {
        assert_eq!(with.pixel(x, y), without.pixel(x, y), "({x}, {y})");
    }
}

#[test]
fn a_rendered_frame_is_its_pixels() {
    // The frame is painted a row at a time, with the terms that depend
    // only on x or y worked out once; it must give what each pixel alone
    // gives. These sizes and scales show the planet's edge, the glows,
    // the grid and stars of both layers.
    let style = BackdropStyle::jabbott_dev();
    for (width, height, factor) in [(160, 90, 0.1), (300, 120, 0.5), (64, 48, 2.0)] {
        let size = DeviceSize::new(width, height);
        let painter = Painter::new(size, scale(factor), &style);
        let mut fb = Framebuffer::new(size).expect("small");
        assert_eq!(render(&mut fb, &painter), Ok(()));
        for y in 0..height {
            for x in 0..width {
                let pixel = fb.pixel(DevicePoint::new(x, y)).map(bytes_of);
                assert_eq!(
                    pixel,
                    Some(painter.pixel(i64::from(x), i64::from(y))),
                    "({x}, {y}) at {factor}"
                );
            }
        }
    }
}

#[test]
fn ensure_builds_once_per_size_scale_and_style() {
    let style = BackdropStyle::jabbott_dev();
    let mut backdrop = Backdrop::new();
    assert_eq!(backdrop.base().size(), DeviceSize::ZERO);
    let size = DeviceSize::new(48, 32);
    assert_eq!(backdrop.ensure(size, scale(1.0), &style), Ok(true));
    assert_eq!(backdrop.ensure(size, scale(1.0), &style), Ok(false));
    assert_eq!(backdrop.base().size(), size);
    assert_eq!(backdrop.blurred().size(), size);
    // The base is what the painter gives, pixel by pixel.
    let painter = Painter::new(size, scale(1.0), &style);
    for (x, y) in [(0, 0), (47, 31), (20, 10)] {
        let pixel = backdrop.base().pixel(DevicePoint::new(x, y)).map(bytes_of);
        assert_eq!(pixel, Some(painter.pixel(i64::from(x), i64::from(y))));
    }
    let first = backdrop.base().clone();

    assert_eq!(
        backdrop.ensure(DeviceSize::new(48, 33), scale(1.0), &style),
        Ok(true)
    );
    assert_eq!(
        backdrop.ensure(DeviceSize::new(48, 33), scale(2.0), &style),
        Ok(true)
    );
    let mut other = style.clone();
    other.bg = Color::rgb(1, 2, 3);
    assert_eq!(
        backdrop.ensure(DeviceSize::new(48, 33), scale(2.0), &other),
        Ok(true)
    );
    assert_eq!(
        backdrop.ensure(DeviceSize::new(48, 33), scale(2.0), &other),
        Ok(false)
    );
    assert_eq!(backdrop.ensure(size, scale(1.0), &style), Ok(true));
    assert_eq!(
        backdrop.base(),
        &first,
        "the same inputs give the same bytes"
    );

    // A failure leaves the cache as it was.
    let mut bad = style.clone();
    bad.planet.ring_width = f32::NAN;
    assert_eq!(
        backdrop.ensure(size, scale(3.0), &bad),
        Err(GfxError::InvalidStyle {
            field: "planet.ring_width",
            index: None
        })
    );
    assert_eq!(
        backdrop.ensure(DeviceSize::new(-1, 3), scale(1.0), &style),
        Err(GfxError::NegativeSize {
            size: DeviceSize::new(-1, 3)
        })
    );
    assert_eq!(backdrop.base(), &first);
    assert_eq!(backdrop.ensure(size, scale(1.0), &style), Ok(false));

    // An empty size builds empty frames.
    assert_eq!(
        backdrop.ensure(DeviceSize::ZERO, scale(1.0), &style),
        Ok(true)
    );
    assert!(backdrop.base().bytes().is_empty() && backdrop.blurred().bytes().is_empty());
}

#[test]
fn degenerate_styles_render_without_panicking() {
    let mut style = BackdropStyle::jabbott_dev();
    style.planet.diameter = 0.0;
    style.near_stars.width = 0.0;
    style.far_stars.height = 0.0;
    style.grid.cell = 0.0;
    style.glows[0].radius_x = 0.0;
    style.blur_radius = 0.0;
    let mut backdrop = Backdrop::new();
    assert_eq!(
        backdrop.ensure(DeviceSize::new(16, 8), scale(1.0), &style),
        Ok(true)
    );
    // No blur: the copy is the base.
    assert_eq!(backdrop.blurred(), backdrop.base());

    let mut huge = BackdropStyle::jabbott_dev();
    huge.planet.diameter = f32::MAX;
    huge.glows[1].radius_y = f32::MAX;
    huge.near_stars.width = 1e-30;
    huge.stars[0].radius = f32::MAX;
    huge.blur_radius = f32::MAX;
    assert_eq!(
        backdrop.ensure(DeviceSize::new(8, 4), scale(1.0), &huge),
        Ok(true)
    );
    let tiny = scale(1e-30);
    assert_eq!(
        backdrop.ensure(DeviceSize::new(8, 4), tiny, &huge),
        Ok(true)
    );
    assert_eq!(
        backdrop.ensure(DeviceSize::new(8, 4), scale(f32::MAX), &huge),
        Ok(true)
    );
}

#[test]
fn box_radii_match_the_gaussian() {
    // σ = 10: the ideal width sqrt(12 × 100 / 3 + 1) = 20.02 gives odd
    // widths 19 and 21, and m = round((1200 − 3 × 361 − 12 × 19 − 9) /
    // (−80)) = round(1.5) = 2 boxes of 19: radii 9, 9 and 10, whose
    // variances (19² − 1) / 12 + (19² − 1) / 12 + (21² − 1) / 12 = 96.7
    // are close to σ² = 100.
    assert_eq!(box_radii(10.0), [9, 9, 10]);
    // At scale 2, σ = 20: sqrt(1601) = 40.01, widths 39 and 41, and
    // m = round((4800 − 4563 − 468 − 9) / (−160)) = round(1.5) = 2.
    assert_eq!(box_radii(20.0), [19, 19, 20]);
    assert_eq!(box_radii(0.0), [0; 3]);
    assert_eq!(box_radii(-3.0), [0; 3]);
    assert_eq!(box_radii(f64::NAN), [0; 3]);
    // Tiny blurs are no blur.
    assert_eq!(box_radii(0.3), [0; 3]);
    // Huge ones are capped.
    assert_eq!(box_radii(f64::MAX), box_radii(MAX_SIGMA));
}

#[test]
fn blurring_a_line_averages_with_the_edges_repeated() {
    // [0, 255] with radius 1: (0 + 0 + 255) / 3 = 85 and
    // (0 + 255 + 255) / 3 = 170, the edge pixels counted again past the
    // ends.
    let line: Vec<u8> = [0, 255].iter().flat_map(|&v| [v, v, v, v]).collect();
    let mut out = vec![0; line.len()];
    blur_line(&line, &mut out, 1);
    assert_eq!(out, [85, 85, 85, 85, 170, 170, 170, 170]);
    // [10, 20, 30, 40] with radius 2, a window wider than half the line:
    // 10+10+10+20+30 = 80 → 16, 10+10+20+30+40 = 110 → 22,
    // 10+20+30+40+40 = 140 → 28, 20+30+40+40+40 = 170 → 34.
    let line: Vec<u8> = [10, 20, 30, 40]
        .iter()
        .flat_map(|&v| [v, 0, 0, 255])
        .collect();
    let mut out = vec![0; line.len()];
    blur_line(&line, &mut out, 2);
    let blue: Vec<u8> = out.chunks(4).map(|pixel| pixel[0]).collect();
    assert_eq!(blue, [16, 22, 28, 34]);
    assert!(out.chunks(4).all(|pixel| pixel[3] == 255));
    // A radius far past the line is the mean with the edges weighed.
    let mut out = vec![0; line.len()];
    blur_line(&line, &mut out, 1000);
    assert!(out.chunks(4).all(|pixel| pixel[3] == 255));
}

#[test]
fn a_uniform_frame_stays_uniform() {
    let mut fb = Framebuffer::new(DeviceSize::new(9, 7)).expect("small");
    let color = PremultipliedColor {
        r: 17,
        g: 99,
        b: 201,
        a: 230,
    };
    fb.clear(color);
    let before = fb.clone();
    assert_eq!(box_blur(&mut fb, [2, 1, 30]), Ok(()));
    assert_eq!(fb, before);
}

#[test]
fn one_bright_pixel_spreads() {
    // A white pixel in the middle of a transparent 7 by 7 frame, blurred
    // with three boxes of radius 1 each way.
    //
    // Across its row: 255 / 3 = 85 over three pixels; then
    // [28, 57, 85, 57, 28] (85 / 3 = 28.3 → 28, 170 / 3 = 56.7 → 57,
    // 255 / 3 = 85); then [9, 28, 57, 66, 57, 28, 9] (28 / 3 = 9.3 → 9,
    // 85 / 3 = 28.3 → 28, 170 / 3 → 57, 199 / 3 = 66.3 → 66).
    //
    // Then down each column. The middle one holds only 66: 66 / 3 = 22;
    // [7, 15, 22, 15, 7] (22 / 3 = 7.3 → 7, 44 / 3 = 14.7 → 15); then
    // [2, 7, 15, 17, 15, 7, 2] (7 / 3 = 2.3 → 2, 22 / 3 = 7.3 → 7,
    // 44 / 3 → 15, 52 / 3 = 17.3 → 17). The other columns go the same way
    // from 57, 28 and 9.
    let mut fb = Framebuffer::new(DeviceSize::new(7, 7)).expect("small");
    let white = PremultipliedColor {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    crate::raster::fill_rect(
        &mut fb,
        DeviceRect::new(3, 3, 1, 1),
        DeviceRect::new(3, 3, 1, 1),
        white,
    );
    assert_eq!(box_blur(&mut fb, [1, 1, 1]), Ok(()));
    let expected = [
        [0, 1, 2, 2, 2, 1, 0],
        [1, 3, 6, 7, 6, 3, 1],
        [2, 6, 13, 15, 13, 6, 2],
        [2, 7, 15, 17, 15, 7, 2],
        [2, 6, 13, 15, 13, 6, 2],
        [1, 3, 6, 7, 6, 3, 1],
        [0, 1, 2, 2, 2, 1, 0],
    ];
    for (y, row) in (0..).zip(expected) {
        for (x, value) in (0..).zip(row) {
            let pixel = fb.pixel(DevicePoint::new(x, y)).expect("inside");
            assert_eq!(
                pixel,
                PremultipliedColor {
                    r: value,
                    g: value,
                    b: value,
                    a: value
                },
                "({x}, {y})"
            );
        }
    }
}

#[test]
fn the_blurred_copy_is_the_base_blurred() {
    let style = BackdropStyle::jabbott_dev();
    let mut backdrop = Backdrop::new();
    let size = DeviceSize::new(40, 24);
    assert_eq!(backdrop.ensure(size, scale(1.0), &style), Ok(true));
    let mut expected = backdrop.base().clone();
    assert_eq!(box_blur(&mut expected, box_radii(10.0)), Ok(()));
    assert_eq!(backdrop.blurred(), &expected);
    assert_ne!(backdrop.blurred(), backdrop.base());
}

#[test]
fn helpers() {
    assert_eq!(wrap(-1.0, 300.0), 299.0);
    assert_eq!(wrap(600.0, 300.0), 0.0);
    assert_eq!(wrap(25.5, 300.0), 25.5);
    // A 1 px line every 64 px; at scale 1.5 a 1.5 px line every 96 px
    // covers pixel 96 fully and half of pixel 97.
    assert_eq!(line_coverage(64.0, 64.0, 1.0), 1.0);
    assert_eq!(line_coverage(65.0, 64.0, 1.0), 0.0);
    assert_eq!(line_coverage(96.0, 96.0, 1.5), 1.0);
    assert_eq!(line_coverage(97.0, 96.0, 1.5), 0.5);
    assert_eq!(edge(-100.0, 10.0), 0.0);
    assert_eq!(edge(100.0, 10.0), 1.0);
    assert_eq!(edge(0.0, 10.0), 0.5);
    // One σ inside: 3 × 0.75² − 2 × 0.75³ = 0.84375; the Gaussian's is
    // 0.8413.
    assert_eq!(edge(10.0, 10.0), 0.84375);
    assert_eq!(to_bgra([0.2, 0.4, 0.6, 1.0]), [153, 102, 51, 255]);
    assert_eq!(
        to_bgra([0.9, 0.0, 0.0, 0.5]),
        [0, 0, 128, 128],
        "channels never exceed alpha"
    );
}
