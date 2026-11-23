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

//! The geometry of jabbott.dev's two backdrop drawings, from the site's
//! `_includes/backdrop.html` at commit 849b36e of
//! `jabbott-iii/jabbott-iii.github.io` ("feat(layout): replace the
//! backdrop planet with circuits and synapses"; the site is Tidepool's
//! owner's), with the paint of each group from its `assets/css/style.css`
//! at that commit (the comments give its line numbers).
//!
//! Each list below holds one `<g>` of a drawing, an element a line, in the
//! order of the file, in its user units; every path command there is
//! absolute, and its `H x` is written as a line to `x` at the same y. The
//! `<g class="space__pulses">` of each drawing, its sparks, flashes and
//! bursts, is motion and is left out (ADR-0057).

use tidepool_core::color::Color;

use super::drawing::{Cap, Drawing, Fill, Geometry, PathCommand, Shape, Side, Stroke, ViewBox};

const fn m(x: f32, y: f32) -> PathCommand {
    PathCommand::Move { x, y }
}

const fn l(x: f32, y: f32) -> PathCommand {
    PathCommand::Line { x, y }
}

const fn q(x1: f32, y1: f32, x: f32, y: f32) -> PathCommand {
    PathCommand::Quad { x1, y1, x, y }
}

const fn c(x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) -> PathCommand {
    PathCommand::Cubic {
        x1,
        y1,
        x2,
        y2,
        x,
        y,
    }
}

/// `<g class="space__traces">`: the circuits' traces.
#[rustfmt::skip]
const TRACES: &[&[PathCommand]] = &[
    &[m(-10.0, 109.0), l(158.7, 109.0), l(208.7, 159.0), l(300.0, 159.0)],
    &[m(-10.0, 123.0), l(152.9, 123.0), l(202.9, 173.0), l(330.0, 173.0)],
    &[m(-10.0, 137.0), l(147.1, 137.0), l(197.1, 187.0), l(280.0, 187.0)],
    &[m(-10.0, 151.0), l(141.3, 151.0), l(191.3, 201.0), l(315.0, 201.0)],
    &[m(-10.0, 288.0), l(55.0, 288.0), l(115.0, 228.0), l(255.0, 228.0), l(295.0, 268.0), l(370.0, 268.0)],
    &[m(-10.0, 300.0), l(60.0, 300.0), l(120.0, 240.0), l(250.0, 240.0), l(290.0, 280.0), l(335.0, 280.0)],
    &[m(-10.0, 312.0), l(65.0, 312.0), l(125.0, 252.0), l(245.0, 252.0), l(285.0, 292.0), l(355.0, 292.0)],
    &[m(140.0, 450.0), l(201.7, 450.0), l(251.7, 400.0), l(360.0, 400.0)],
    &[m(140.0, 470.0), l(210.0, 470.0), l(260.0, 420.0), l(320.0, 420.0)],
    &[m(140.0, 490.0), l(218.3, 490.0), l(268.3, 440.0), l(340.0, 440.0)],
    &[m(140.0, 520.0), l(238.3, 520.0), l(288.3, 570.0), l(365.0, 570.0)],
    &[m(140.0, 540.0), l(230.0, 540.0), l(280.0, 590.0), l(390.0, 590.0)],
    &[m(140.0, 560.0), l(221.7, 560.0), l(271.7, 610.0), l(340.0, 610.0)],
    &[m(-10.0, 739.0), l(101.3, 739.0), l(161.3, 679.0), l(290.0, 679.0)],
    &[m(-10.0, 753.0), l(107.1, 753.0), l(167.1, 693.0), l(255.0, 693.0)],
    &[m(-10.0, 767.0), l(112.9, 767.0), l(172.9, 707.0), l(300.0, 707.0)],
    &[m(-10.0, 781.0), l(118.7, 781.0), l(178.7, 721.0), l(270.0, 721.0)],
    &[m(-10.0, 873.0), l(202.9, 873.0), l(242.9, 913.0), l(340.0, 913.0)],
    &[m(-10.0, 887.0), l(197.1, 887.0), l(237.1, 927.0), l(300.0, 927.0)],
    &[m(-10.0, 450.0), l(30.0, 450.0)],
    &[m(-10.0, 472.0), l(30.0, 472.0)],
    &[m(-10.0, 494.0), l(30.0, 494.0)],
    &[m(-10.0, 516.0), l(30.0, 516.0)],
    &[m(-10.0, 538.0), l(30.0, 538.0)],
    &[m(-10.0, 560.0), l(30.0, 560.0)],
];

/// `<g class="space__pads">`: 19 pads of radius 4.5 at the ends of
/// traces, then 9 of radius 3.5, each `[cx, cy, r]`.
#[rustfmt::skip]
const PADS: &[[f32; 3]] = &[
    [300.0, 159.0, 4.5],
    [330.0, 173.0, 4.5],
    [280.0, 187.0, 4.5],
    [315.0, 201.0, 4.5],
    [370.0, 268.0, 4.5],
    [335.0, 280.0, 4.5],
    [355.0, 292.0, 4.5],
    [360.0, 400.0, 4.5],
    [320.0, 420.0, 4.5],
    [340.0, 440.0, 4.5],
    [365.0, 570.0, 4.5],
    [390.0, 590.0, 4.5],
    [340.0, 610.0, 4.5],
    [290.0, 679.0, 4.5],
    [255.0, 693.0, 4.5],
    [300.0, 707.0, 4.5],
    [270.0, 721.0, 4.5],
    [340.0, 913.0, 4.5],
    [300.0, 927.0, 4.5],
    [205.0, 330.0, 3.5],
    [225.0, 352.0, 3.5],
    [88.0, 640.0, 3.5],
    [112.0, 662.0, 3.5],
    [262.0, 808.0, 3.5],
    [60.0, 962.0, 3.5],
    [184.0, 990.0, 3.5],
    [330.0, 80.0, 3.5],
    [352.0, 102.0, 3.5],
];

/// `<g class="space__chip">`: its two `<rect>`s, each `[x, y, width,
/// height, rx]`.
#[rustfmt::skip]
const CHIP_RECTS: &[[f32; 5]] = &[
    [30.0, 430.0, 100.0, 150.0, 3.0],
    [48.0, 452.0, 64.0, 106.0, 2.0],
];

/// `<g class="space__chip">`: its `<circle>`, `[cx, cy, r]`, after the
/// rects.
const CHIP_DOT: [f32; 3] = [42.0, 442.0, 3.0];

/// `<g class="space__chip">`: its pins, after the dot.
#[rustfmt::skip]
const CHIP_PINS: &[&[PathCommand]] = &[
    &[m(130.0, 450.0), l(140.0, 450.0)],
    &[m(130.0, 470.0), l(140.0, 470.0)],
    &[m(130.0, 490.0), l(140.0, 490.0)],
    &[m(130.0, 520.0), l(140.0, 520.0)],
    &[m(130.0, 540.0), l(140.0, 540.0)],
    &[m(130.0, 560.0), l(140.0, 560.0)],
];

/// `<g class="space__axons">`: the synapses' axons.
#[rustfmt::skip]
const AXONS: &[&[PathCommand]] = &[
    &[m(470.0, 90.0), c(437.0, 139.5, 368.3, 129.7, 330.0, 170.0)],
    &[m(470.0, 90.0), c(516.2, 133.2, 490.0, 203.0, 525.0, 250.0)],
    &[m(330.0, 170.0), c(375.6, 213.0, 367.8, 283.1, 405.0, 330.0)],
    &[m(330.0, 170.0), c(327.5, 225.7, 266.4, 249.4, 255.0, 300.0)],
    &[m(525.0, 250.0), c(505.2, 306.1, 432.7, 285.8, 405.0, 330.0)],
    &[m(525.0, 250.0), c(486.1, 312.6, 513.6, 390.4, 485.0, 455.0)],
    &[m(405.0, 330.0), c(453.7, 357.0, 445.2, 422.3, 485.0, 455.0)],
    &[m(405.0, 330.0), c(357.0, 377.8, 368.7, 453.2, 330.0, 505.0)],
    &[m(255.0, 300.0), c(306.0, 358.0, 289.5, 443.1, 330.0, 505.0)],
    &[m(255.0, 300.0), c(277.7, 363.8, 198.0, 396.3, 205.0, 455.0)],
    &[m(485.0, 455.0), c(432.7, 500.1, 468.6, 575.5, 430.0, 625.0)],
    &[m(485.0, 455.0), c(534.3, 528.9, 503.2, 623.3, 540.0, 700.0)],
    &[m(330.0, 505.0), c(378.2, 531.9, 387.9, 593.0, 430.0, 625.0)],
    &[m(330.0, 505.0), c(348.2, 571.7, 288.7, 625.6, 295.0, 690.0)],
    &[m(205.0, 455.0), c(188.7, 550.2, 292.9, 601.9, 295.0, 690.0)],
    &[m(430.0, 625.0), c(481.2, 627.9, 494.8, 688.3, 540.0, 700.0)],
    &[m(430.0, 625.0), c(464.5, 686.0, 431.0, 757.7, 455.0, 820.0)],
    &[m(295.0, 690.0), c(370.2, 705.4, 388.8, 793.6, 455.0, 820.0)],
    &[m(295.0, 690.0), c(299.8, 750.1, 231.4, 775.4, 225.0, 830.0)],
    &[m(455.0, 820.0), c(402.6, 827.2, 391.0, 889.5, 345.0, 905.0)],
    &[m(455.0, 820.0), c(448.4, 878.9, 519.7, 897.5, 525.0, 950.0)],
    &[m(345.0, 905.0), c(320.5, 856.0, 255.5, 869.3, 225.0, 830.0)],
    &[m(345.0, 905.0), c(412.1, 889.1, 461.0, 953.6, 525.0, 950.0)],
    &[m(540.0, 700.0), c(592.9, 786.0, 495.3, 865.4, 525.0, 950.0)],
];

/// `<g class="space__dendrites">`.
#[rustfmt::skip]
const DENDRITES: &[&[PathCommand]] = &[
    &[m(470.0, 90.0), q(458.4, 78.8, 441.7, 75.7), m(458.4, 78.8), l(444.2, 78.5)],
    &[m(470.0, 90.0), q(456.5, 100.4, 437.2, 108.5), m(456.5, 100.4), l(450.7, 116.3)],
    &[m(470.0, 90.0), q(448.4, 76.4, 433.5, 71.9), m(448.4, 76.4), l(436.8, 62.1)],
    &[m(330.0, 170.0), q(346.4, 158.2, 358.5, 159.7)],
    &[m(330.0, 170.0), q(322.3, 154.3, 303.6, 135.0), m(322.3, 154.3), l(317.7, 135.1)],
    &[m(330.0, 170.0), q(320.0, 177.8, 308.3, 175.3)],
    &[m(525.0, 250.0), q(530.3, 234.5, 522.9, 221.7), m(530.3, 234.5), l(536.9, 223.6)],
    &[m(525.0, 250.0), q(545.2, 232.4, 554.2, 223.7), m(545.2, 232.4), l(562.8, 231.3)],
    &[m(525.0, 250.0), q(519.8, 261.8, 507.5, 266.4)],
    &[m(405.0, 330.0), q(387.7, 340.2, 371.8, 333.1), m(387.7, 340.2), l(374.9, 332.4)],
    &[m(405.0, 330.0), q(396.7, 337.4, 396.1, 349.3)],
    &[m(405.0, 330.0), q(410.0, 314.8, 403.1, 300.2)],
    &[m(255.0, 300.0), q(247.5, 308.0, 232.9, 317.8), m(247.5, 308.0), l(246.3, 320.8)],
    &[m(255.0, 300.0), q(244.0, 308.0, 235.6, 307.6), m(244.0, 308.0), l(240.1, 316.6)],
    &[m(255.0, 300.0), q(245.1, 293.9, 240.0, 287.0)],
    &[m(255.0, 300.0), q(272.7, 288.6, 287.4, 290.7), m(272.7, 288.6), l(287.1, 293.2)],
    &[m(255.0, 300.0), q(247.9, 309.7, 246.8, 320.1), m(247.9, 309.7), l(250.0, 319.2)],
    &[m(485.0, 455.0), q(498.8, 450.4, 515.2, 445.4), m(498.8, 450.4), l(511.4, 457.1)],
    &[m(485.0, 455.0), q(494.0, 438.4, 508.6, 427.7), m(494.0, 438.4), l(495.7, 422.2)],
    &[m(485.0, 455.0), q(483.6, 442.1, 489.8, 428.9)],
    &[m(330.0, 505.0), q(342.1, 483.5, 343.6, 468.1), m(342.1, 483.5), l(357.3, 474.6)],
    &[m(330.0, 505.0), q(351.1, 497.3, 368.5, 502.5)],
    &[m(330.0, 505.0), q(314.4, 524.4, 308.2, 537.4)],
    &[m(205.0, 455.0), q(211.2, 466.7, 209.4, 478.5)],
    &[m(205.0, 455.0), q(222.5, 445.0, 237.3, 431.6)],
    &[m(205.0, 455.0), q(216.4, 469.1, 235.3, 472.9)],
    &[m(430.0, 625.0), q(443.6, 610.6, 452.4, 606.2)],
    &[m(430.0, 625.0), q(431.1, 611.3, 424.7, 595.4), m(431.1, 611.3), l(423.6, 600.0)],
    &[m(430.0, 625.0), q(417.6, 614.6, 404.7, 608.7), m(417.6, 614.6), l(413.1, 601.8)],
    &[m(430.0, 625.0), q(425.4, 642.6, 411.0, 651.1)],
    &[m(540.0, 700.0), q(559.8, 718.7, 569.9, 722.6), m(559.8, 718.7), l(576.2, 722.6)],
    &[m(540.0, 700.0), q(539.5, 715.7, 547.2, 730.2), m(539.5, 715.7), l(548.0, 726.7)],
    &[m(540.0, 700.0), q(542.3, 678.0, 537.4, 658.7), m(542.3, 678.0), l(530.6, 663.5)],
    &[m(540.0, 700.0), q(529.9, 701.7, 521.7, 697.3), m(529.9, 701.7), l(522.0, 704.3)],
    &[m(540.0, 700.0), q(519.9, 706.8, 503.7, 706.1), m(519.9, 706.8), l(505.7, 698.1)],
    &[m(295.0, 690.0), q(300.4, 680.0, 309.5, 677.0), m(300.4, 680.0), l(309.1, 679.1)],
    &[m(295.0, 690.0), q(295.1, 667.4, 297.6, 648.4), m(295.1, 667.4), l(284.3, 652.1)],
    &[m(295.0, 690.0), q(278.4, 687.5, 264.6, 699.4)],
    &[m(455.0, 820.0), q(471.1, 808.4, 491.5, 800.8), m(471.1, 808.4), l(489.5, 810.1)],
    &[m(455.0, 820.0), q(468.3, 827.8, 476.8, 830.7)],
    &[m(455.0, 820.0), q(466.0, 814.3, 472.6, 806.7), m(466.0, 814.3), l(467.4, 804.5)],
    &[m(455.0, 820.0), q(461.8, 843.0, 463.1, 862.0)],
    &[m(345.0, 905.0), q(353.3, 917.1, 360.4, 929.8), m(353.3, 917.1), l(354.8, 930.2)],
    &[m(345.0, 905.0), q(335.7, 913.8, 327.2, 925.0), m(335.7, 913.8), l(323.9, 916.6)],
    &[m(345.0, 905.0), q(363.5, 923.4, 385.1, 922.1), m(363.5, 923.4), l(375.5, 938.9)],
    &[m(525.0, 950.0), q(524.4, 960.1, 522.3, 971.2)],
    &[m(525.0, 950.0), q(519.5, 961.9, 523.8, 971.9)],
    &[m(525.0, 950.0), q(532.8, 955.0, 541.5, 960.4), m(532.8, 955.0), l(541.6, 954.2)],
    &[m(225.0, 830.0), q(247.0, 847.2, 259.8, 850.2), m(247.0, 847.2), l(251.5, 864.8)],
    &[m(225.0, 830.0), q(203.8, 849.0, 188.7, 850.7)],
    &[m(225.0, 830.0), q(244.2, 814.6, 265.0, 813.6)],
    &[m(225.0, 830.0), q(209.8, 818.0, 203.0, 807.0)],
];

/// `<g class="space__somas">`: each cell's centre, its halo's radius and
/// its body's, `[cx, cy, halo, body]`. The file lists each halo just
/// before its body.
#[rustfmt::skip]
const CELLS: &[[f32; 4]] = &[
    [470.0, 90.0, 18.0, 6.0],
    [330.0, 170.0, 13.5, 4.5],
    [525.0, 250.0, 15.0, 5.0],
    [405.0, 330.0, 19.5, 6.5],
    [255.0, 300.0, 12.0, 4.0],
    [485.0, 455.0, 16.5, 5.5],
    [330.0, 505.0, 15.0, 5.0],
    [205.0, 455.0, 10.5, 3.5],
    [430.0, 625.0, 18.0, 6.0],
    [540.0, 700.0, 13.5, 4.5],
    [295.0, 690.0, 15.0, 5.0],
    [455.0, 820.0, 18.0, 6.0],
    [345.0, 905.0, 13.5, 4.5],
    [525.0, 950.0, 15.0, 5.0],
    [225.0, 830.0, 12.0, 4.0],
];
/// A path of `commands`.
fn path(commands: &[PathCommand]) -> Geometry {
    Geometry::Path(commands.to_vec())
}

/// A circle `[cx, cy, r]`.
const fn circle([cx, cy, r]: [f32; 3]) -> Geometry {
    Geometry::Circle { cx, cy, r }
}

/// The circuits, `svg.space__art--circuits`, on the left.
pub(super) fn circuits() -> Drawing {
    // Line 173: `.space__traces { fill: none; stroke: var(--art);
    // stroke-width: 2; stroke-opacity: 0.34; stroke-linejoin: round }`.
    let trace = Stroke {
        width: 2.0,
        opacity: 0.34,
        cap: Cap::Butt,
    };
    // Line 174: `.space__pads { fill: none; stroke-width: 2;
    // stroke-opacity: 0.42 }`.
    let pad = Stroke {
        width: 2.0,
        opacity: 0.42,
        cap: Cap::Butt,
    };
    // Line 175: `.space__chip { fill: color-mix(in srgb, var(--art) 6%,
    // transparent); stroke-width: 1.5; stroke-opacity: 0.4 }`. Its pins
    // are lines, which enclose nothing to fill.
    let chip = Stroke {
        width: 1.5,
        opacity: 0.4,
        cap: Cap::Butt,
    };
    let chip_fill = Fill::Solid { opacity: 0.06 };
    let stroked = |geometry, stroke| Shape {
        geometry,
        fill: Fill::None,
        stroke: Some(stroke),
    };
    let shapes = TRACES
        .iter()
        .map(|&commands| stroked(path(commands), trace))
        .chain(
            PADS.iter()
                .map(|&pad_circle| stroked(circle(pad_circle), pad)),
        )
        .chain(CHIP_RECTS.iter().map(|&[x, y, width, height, rx]| Shape {
            geometry: Geometry::Rect {
                x,
                y,
                width,
                height,
                rx,
            },
            fill: chip_fill,
            stroke: Some(chip),
        }))
        .chain([Shape {
            geometry: circle(CHIP_DOT),
            fill: chip_fill,
            stroke: Some(chip),
        }])
        .chain(
            CHIP_PINS
                .iter()
                .map(|&commands| stroked(path(commands), chip)),
        )
        .collect();
    Drawing {
        // Line 25: `--bronze: #cd7f32`, the circuits' `--art` (line 151).
        color: Color::rgb(0xcd, 0x7f, 0x32),
        // Line 159: `left: 0`; and the include's
        // `preserveAspectRatio="xMinYMid slice"`.
        side: Side::Left,
        // The include's `viewBox="0 0 400 1000"`.
        view_box: ViewBox {
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: 1000.0,
        },
        // Lines 154–156: `height: 100%; aspect-ratio: 2 / 5;
        // max-width: 40vw`; 2:5 is the view box's shape.
        max_width: 0.4,
        // Line 161: `mask-image: linear-gradient(90deg, #000 40%,
        // transparent)`.
        mask_solid: 0.4,
        shapes,
    }
}

/// The synapses, `svg.space__art--synapses`, on the right.
pub(super) fn synapses() -> Drawing {
    // Line 176: `.space__axons { fill: none; stroke-width: 1.2;
    // stroke-opacity: 0.38 }`.
    let axon = Stroke {
        width: 1.2,
        opacity: 0.38,
        cap: Cap::Butt,
    };
    // Line 177: `.space__dendrites { fill: none; stroke-width: 1;
    // stroke-opacity: 0.24; stroke-linecap: round }`.
    let dendrite = Stroke {
        width: 1.0,
        opacity: 0.24,
        cap: Cap::Round,
    };
    // Line 178: `.space__somas { fill: var(--art); fill-opacity: 0.45 }`.
    // A halo's `fill="url(#bg-glow-purple)"` takes the place of the fill,
    // and it inherits the `fill-opacity`; the gradient's stops are
    // `--art` at `stop-opacity: 0.55` and at 0 (lines 184–185).
    let halo = Fill::Radial {
        opacity: 0.45,
        center: 0.55,
    };
    let body = Fill::Solid { opacity: 0.45 };
    let stroked = |commands, stroke| Shape {
        geometry: path(commands),
        fill: Fill::None,
        stroke: Some(stroke),
    };
    let filled = |cx, cy, r, fill| Shape {
        geometry: Geometry::Circle { cx, cy, r },
        fill,
        stroke: None,
    };
    let shapes = AXONS
        .iter()
        .map(|&commands| stroked(commands, axon))
        .chain(
            DENDRITES
                .iter()
                .map(|&commands| stroked(commands, dendrite)),
        )
        .chain(CELLS.iter().flat_map(|&[cx, cy, halo_r, body_r]| {
            [filled(cx, cy, halo_r, halo), filled(cx, cy, body_r, body)]
        }))
        .collect();
    Drawing {
        // Line 27: `--purple: #9a6bff`, the synapses' `--art` (line 164).
        color: Color::rgb(0x9a, 0x6b, 0xff),
        // Line 166: `right: 0`; and `preserveAspectRatio="xMaxYMid slice"`.
        side: Side::Right,
        // The include's `viewBox="160 0 400 1000"`.
        view_box: ViewBox {
            x: 160.0,
            y: 0.0,
            width: 400.0,
            height: 1000.0,
        },
        // Lines 154–156, as for the circuits.
        max_width: 0.4,
        // Line 168: `linear-gradient(270deg, #000 40%, transparent)`.
        mask_solid: 0.4,
        shapes,
    }
}
