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

//! The first frame's content: a line in each of the seven bundled faces
//! (M2.6, ADR-0045), as a `ui` tree that [`Gui`](crate::Gui) paints over
//! jabbott.dev's backdrop (M3.8, ADR-0059). The `tidepool` binary showed it
//! until it showed `editor`'s shell (M3.4, ADR-0060); `gui`'s window test
//! still shows it.
//!
//! New in Tidepool.
//!
//! [`tree`] gives a [`Tree`] whose one node fills the window. Its widget
//! paints the lines: each names its face in `text-3`, then shows a sample
//! in `text`, both in that face, at a size and tracking from the
//! typography table of `docs/subsystems/ui-and-theme.md`. Both colors come
//! from the tree's theme ([`PaintContext::theme`], the bundled
//! `tidepool-dark` unless the tree is given another, ADR-0047), and the
//! faces from each line's [`TextStyle`], which the
//! [painter](crate::paint::Painter) sets in a bundled face
//! ([`metrics::face`](crate::metrics::face)). Saira's chrome is uppercase;
//! Saira 400's body text and JetBrains Mono's code are as written.
//!
//! The labels start 48 logical units from the left, the samples in one
//! column 32 units after the widest label, as the tree's metrics measure
//! it, and the baselines are 32 units apart from 72 down. What falls
//! outside the window is clipped.
//!
//! The tree is idle after its first frame: nothing in it changes, so
//! [`Tree::frame`] damages nothing until the window's size changes or
//! [`Tree::invalidate_all`] repaints it.

use tidepool_core::geom::Point;
use tidepool_ui::draw::{DrawList, PaintContext};
use tidepool_ui::layout::Style;
use tidepool_ui::metrics::{Family, TextStyle, Weight};
use tidepool_ui::theme::Token;
use tidepool_ui::{Node, Tree, Widget};

/// One line of the frame: how it is set, and what it says.
struct Sample {
    /// The face, size and tracking of both its label and its text.
    style: TextStyle,
    /// The face's name, in `text-3`.
    label: &'static str,
    /// What it shows, in `text`.
    text: &'static str,
}

/// The lines, one a face, in the order of `font`'s
/// [`Face::ALL`](tidepool_font::bundled::Face::ALL), each set as its role
/// in the typography table: UI body, section title, the brand, a tracked
/// label (Saira 700 has no role of its own), and code, a comment and
/// emphasis in code.
const SAMPLES: [Sample; 7] = [
    Sample {
        style: TextStyle::new(Family::Ui, 13.0),
        label: "Saira Regular 400",
        text: "UI body: lists, the file tree and dialogs, as written",
    },
    Sample {
        style: TextStyle::new(Family::Ui, 13.0)
            .weight(Weight::Medium)
            .letter_spacing(0.04),
        label: "SAIRA MEDIUM 500",
        text: "SECTION TITLE",
    },
    Sample {
        style: TextStyle::new(Family::Ui, 15.0)
            .weight(Weight::SemiBold)
            .letter_spacing(0.28),
        label: "SAIRA SEMIBOLD 600",
        text: "TIDEPOOL",
    },
    Sample {
        style: TextStyle::new(Family::Ui, 12.0)
            .weight(Weight::Bold)
            .letter_spacing(0.10),
        label: "SAIRA BOLD 700",
        text: "RUST · MARKDOWN · ANDROID · IOS",
    },
    Sample {
        style: TextStyle::new(Family::Mono, 14.0),
        label: "JetBrains Mono Regular 400",
        text: "fn main() -> Result<(), Error> { Ok(()) }",
    },
    Sample {
        style: TextStyle::new(Family::Mono, 14.0).italic(),
        label: "JetBrains Mono Italic 400",
        text: "// a comment, in italic",
    },
    Sample {
        style: TextStyle::new(Family::Mono, 14.0).weight(Weight::SemiBold),
        label: "JetBrains Mono SemiBold 600",
        text: "let emphasis = 600;",
    },
];

/// The labels' left edge, in logical units.
const LEFT: f32 = 48.0;
/// The first line's baseline, in logical units from the top.
const TOP: f32 = 72.0;
/// From one baseline to the next, in logical units.
const PITCH: f32 = 32.0;
/// From the right of the widest label to the samples, in logical units.
const GAP: f32 = 32.0;

/// A tree showing the first frame's lines (the [module
/// documentation](self)): under its root, one node that fills the window,
/// whose widget paints them.
pub fn tree() -> Tree {
    let mut tree = Tree::new();
    let lines = Node::new(Style::default().fill()).with_widget(Lines);
    tree.insert(tree.root(), lines)
        .expect("a new tree takes a node under its root: one level down, two nodes in all");
    tree
}

/// Paints [`SAMPLES`] from its bounds' top-left corner.
struct Lines;

impl Widget for Lines {
    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let (theme, metrics, bounds) = (cx.theme(), cx.metrics(), cx.bounds());
        let (muted, text) = (theme.color(Token::Text3), theme.color(Token::Text));
        let widest = SAMPLES
            .iter()
            .map(|sample| metrics.width(sample.label, &sample.style))
            .fold(0.0, f32::max);
        let left = bounds.x + LEFT;
        let column = left + widest + GAP;
        let mut baseline = bounds.y + TOP;
        for sample in &SAMPLES {
            list.text(
                Point::new(left, baseline),
                sample.label,
                sample.style,
                muted,
            );
            list.text(
                Point::new(column, baseline),
                sample.text,
                sample.style,
                text,
            );
            baseline += PITCH;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use tidepool_core::color::{Color, PremultipliedColor};
    use tidepool_core::geom::{DevicePoint, DeviceRect, DeviceSize, Rect, Scale, Size};
    use tidepool_core::input::{InputEvent, Modifiers};
    use tidepool_font::bundled::Face;
    use tidepool_gfx::framebuffer::Framebuffer;
    use tidepool_ui::draw::Command;
    use tidepool_ui::theme::Theme;
    use tidepool_ui::{Frame, TextMetrics};

    use super::*;
    use crate::Painter;
    use crate::metrics::face;

    fn bg() -> PremultipliedColor {
        Theme::bundled().color(Token::Bg).premultiply()
    }

    /// The first frame of [`tree`] at `size` and `scale`, and its pixels
    /// painted over the flat `bg`.
    fn paint(size: DeviceSize, scale: f32) -> (Frame, Framebuffer) {
        let scale = Scale::new(scale).expect("a scale");
        let mut painter = Painter::new().expect("the bundled faces load");
        let mut tree = tree();
        let frame = tree
            .frame(scale.to_logical(size), &painter.metrics(scale))
            .expect("the first frame damages the window");
        let mut base = Framebuffer::new(size).expect("a base");
        base.clear(bg());
        let mut target = Framebuffer::new(size).expect("a target");
        painter
            .paint(&mut target, &base.view(), &frame, scale)
            .expect("painted");
        (frame, target)
    }

    /// The bounding box of the pixels that are not the flat `bg`.
    fn ink(fb: &Framebuffer) -> Option<DeviceRect> {
        let (size, bg) = (fb.size(), Some(bg()));
        let (mut left, mut top, mut right, mut bottom) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for y in 0..size.height {
            for x in 0..size.width {
                if fb.pixel(DevicePoint::new(x, y)) != bg {
                    (left, top) = (left.min(x), top.min(y));
                    (right, bottom) = (right.max(x + 1), bottom.max(y + 1));
                }
            }
        }
        (left <= right).then(|| DeviceRect::new(left, top, right - left, bottom - top))
    }

    #[test]
    fn every_bundled_face_has_a_line_in_order() {
        let faces: Vec<Face> = SAMPLES.iter().map(|sample| face(&sample.style)).collect();
        assert_eq!(faces, Face::ALL);
    }

    #[test]
    fn saira_chrome_is_uppercase_and_body_and_code_are_as_written() {
        for sample in &SAMPLES {
            let chrome = matches!(
                face(&sample.style),
                Face::SairaMedium | Face::SairaSemiBold | Face::SairaBold
            );
            for text in [sample.label, sample.text] {
                assert_eq!(chrome, text == text.to_uppercase(), "{text}");
            }
        }
    }

    #[test]
    fn the_node_fills_the_window() {
        let mut tree = tree();
        let painter = Painter::new().expect("the bundled faces load");
        let size = Size::new(1232.0, 662.0);
        assert!(tree.frame(size, &painter.metrics(Scale::ONE)).is_some());
        let root = tree.root();
        let lines = tree.children(root).expect("the root")[0];
        assert_eq!(tree.bounds(lines), Some(Rect::new(0.0, 0.0, 1232.0, 662.0)));
        assert_eq!(tree.node_count(), 2);
    }

    #[test]
    fn the_labels_and_samples_go_in_two_columns_on_baselines_32_apart() {
        let (frame, _) = paint(DeviceSize::new(1232, 662), 1.0);
        let painter = Painter::new().expect("the bundled faces load");
        let metrics = painter.metrics(Scale::ONE);
        let widest = SAMPLES
            .iter()
            .map(|sample| metrics.width(sample.label, &sample.style))
            .fold(0.0, f32::max);
        let texts: Vec<(Point, &str, TextStyle)> = frame
            .list
            .commands()
            .iter()
            .filter_map(|command| match command {
                Command::Text {
                    origin,
                    text,
                    style,
                    ..
                } => Some((*origin, text.as_str(), *style)),
                _ => None,
            })
            .collect();
        assert_eq!(texts.len(), 2 * SAMPLES.len());
        let mut baseline = TOP;
        for (pair, sample) in texts.chunks(2).zip(&SAMPLES) {
            assert_eq!(
                pair[0],
                (Point::new(LEFT, baseline), sample.label, sample.style)
            );
            assert_eq!(
                pair[1],
                (
                    Point::new(LEFT + widest + GAP, baseline),
                    sample.text,
                    sample.style
                )
            );
            baseline += PITCH;
        }
    }

    #[test]
    fn the_lines_are_painted_where_they_go_at_scale_1_and_2() {
        let (_, one) = paint(DeviceSize::new(1232, 662), 1.0);
        let ink = ink(&one).expect("the lines ink");
        // From the labels' left edge, about the first line's ascent above
        // its baseline, to just below the last.
        let last = 72 + 32 * 6;
        assert!((48..=50).contains(&ink.x), "{ink:?}");
        assert!((56..72).contains(&ink.y), "{ink:?}");
        assert!(ink.bottom() > last && ink.bottom() <= last + 8, "{ink:?}");
        assert!(ink.right() < 1232, "{ink:?}");

        let (_, two) = paint(DeviceSize::new(1280, 800), 2.0);
        let ink = self::ink(&two).expect("the lines ink");
        assert!((96..=100).contains(&ink.x), "{ink:?}");
        assert!((112..144).contains(&ink.y), "{ink:?}");
    }

    #[test]
    fn the_lines_use_the_themes_text_colors() {
        let size = DeviceSize::new(800, 300);
        let (_, painted) = paint(size, 1.0);
        let mut colors = HashSet::new();
        for y in 0..size.height {
            for x in 0..size.width {
                if let Some(pixel) = painted.pixel(DevicePoint::new(x, y)) {
                    colors.insert(pixel);
                }
            }
        }
        // Fully covered pixels of a glyph are the tokens' colors exactly:
        // the spec's `#eef2f3` and `#7d898e`.
        let theme = Theme::tidepool_dark();
        assert_eq!(theme.color(Token::Text), Color::rgb(0xee, 0xf2, 0xf3));
        assert_eq!(theme.color(Token::Text3), Color::rgb(0x7d, 0x89, 0x8e));
        assert!(colors.contains(&theme.color(Token::Text).premultiply()));
        assert!(colors.contains(&theme.color(Token::Text3).premultiply()));
    }

    #[test]
    fn it_is_idle_after_its_first_frame() {
        let painter = Painter::new().expect("the bundled faces load");
        let metrics = painter.metrics(Scale::ONE);
        let size = Size::new(640.0, 400.0);
        let mut tree = tree();
        assert!(tree.frame(size, &metrics).is_some());
        assert!(tree.frame(size, &metrics).is_none());
        // The pointer passing over it repaints the node, which paints the
        // same, so nothing is damaged.
        let _ = tree.handle(&InputEvent::MouseMove {
            position: Point::new(100.0, 80.0),
            modifiers: Modifiers::NONE,
        });
        assert!(tree.frame(size, &metrics).is_none());
        // A new size lays it out and paints it again.
        let larger = Size::new(700.0, 400.0);
        let frame = tree.frame(larger, &metrics).expect("a new size");
        assert_eq!(frame.damage, [Rect::new(0.0, 0.0, 700.0, 400.0)]);
    }

    #[test]
    fn painting_again_gives_the_same_bytes() {
        let size = DeviceSize::new(640, 400);
        assert_eq!(paint(size, 1.0).1.bytes(), paint(size, 1.0).1.bytes());
        assert_eq!(paint(size, 1.5).1.bytes(), paint(size, 1.5).1.bytes());
    }
}
