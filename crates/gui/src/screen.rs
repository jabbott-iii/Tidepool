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

//! [`Screen`]: the part of [`Gui`](crate::Gui) that needs no window
//! (M3.8, ADR-0059). It keeps the framebuffer the window is presented from,
//! the base painted under the tree, the [`Painter`], and what the window
//! asked to have shown again; [`Screen::render`] paints a `ui` tree's frame
//! and hands what changed to a function that presents it. `Gui` adds the
//! window and the backdrop's builds; [`Gui`](crate::Gui)'s documentation
//! says what the window shows, and why.
//!
//! New in Tidepool.

use std::time::Instant;

use tidepool_core::color::PremultipliedColor;
use tidepool_core::geom::{DeviceRect, DeviceSize, Scale};
use tidepool_gfx::backdrop::Backdrop;
use tidepool_gfx::damage::Damage;
use tidepool_gfx::framebuffer::Framebuffer;
use tidepool_ui::Tree;
use tidepool_ui::theme::Token;

use crate::{GuiError, Painter};

/// What the window shows, without the window.
#[derive(Debug)]
pub(crate) struct Screen {
    painter: Painter,
    /// The frame's size, in device pixels.
    size: DeviceSize,
    /// The window's scale.
    scale: Scale,
    /// What the window is presented from; `None` until a render makes it,
    /// and after a resize until the next one.
    target: Option<Framebuffer>,
    /// What the tree is painted over.
    base: Base,
    /// Whether the base changed since the last paint, which then repaints
    /// the whole window.
    base_changed: bool,
    /// What the window asked to have shown again since the last present.
    exposes: Damage,
    /// Until when the first present waits for the backdrop; `None` once a
    /// render went past the wait, or if there is none.
    wait_until: Option<Instant>,
}

/// The image the tree is painted over.
#[derive(Debug)]
enum Base {
    /// None yet: made by the next render.
    None,
    /// A framebuffer cleared to the theme's `bg`, in that color.
    Flat(Framebuffer, PremultipliedColor),
    /// jabbott.dev's backdrop, for the frame's size and scale. Boxed: its
    /// cache key holds a copy of its style, which makes it several hundred
    /// bytes, and the other variants are small.
    Backdrop(Box<Backdrop>),
}

impl Base {
    /// The pixels to paint over; `None` if there are none yet.
    const fn pixels(&self) -> Option<&Framebuffer> {
        match self {
            Self::None => None,
            Self::Flat(fb, _) => Some(fb),
            Self::Backdrop(backdrop) => Some(backdrop.base()),
        }
    }
}

impl Screen {
    /// A screen of `size` device pixels at `scale`, painting with
    /// `painter`; its first present waits for the backdrop until
    /// `wait_until`, or not at all if that is `None`. Nothing is allocated
    /// until the first render.
    pub(crate) const fn new(
        painter: Painter,
        size: DeviceSize,
        scale: Scale,
        wait_until: Option<Instant>,
    ) -> Self {
        Self {
            painter,
            size,
            scale,
            target: None,
            base: Base::None,
            base_changed: false,
            exposes: Damage::new(),
            wait_until,
        }
    }

    /// Makes the frame `size` device pixels, and returns `true` if that is
    /// a new size: the framebuffer and the base are dropped then, and the
    /// next render makes them at the new size, over the flat `bg`, and
    /// repaints the whole window.
    pub(crate) fn resize(&mut self, size: DeviceSize) -> bool {
        if size == self.size {
            return false;
        }
        self.size = size;
        // Dropped now, so that the old and the new are never held at once.
        self.target = None;
        self.base = Base::None;
        true
    }

    /// Notes that the window asked to have `rect` shown again; the next
    /// render presents it, cut to the frame.
    pub(crate) fn expose(&mut self, rect: DeviceRect) {
        self.exposes.add(rect);
    }

    /// Paints over `backdrop` from the next render, which repaints the
    /// whole window, if it is the frame's size; drops it otherwise.
    pub(crate) fn set_backdrop(&mut self, backdrop: Backdrop) {
        if backdrop.base().size() == self.size {
            self.base = Base::Backdrop(Box::new(backdrop));
            self.base_changed = true;
        }
    }

    /// Whether the base is the backdrop of the frame's size.
    pub(crate) fn backdrop_ready(&self) -> bool {
        matches!(&self.base, Base::Backdrop(backdrop) if backdrop.base().size() == self.size)
    }

    /// When the first present stops waiting for the backdrop; `None` once
    /// it does not wait.
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        self.wait_until.filter(|_| !self.backdrop_ready())
    }

    /// Paints `tree`'s frame, if it has one, and calls `present` with the
    /// framebuffer and the rects to show: those painted and those exposed,
    /// cut to the frame. Returns whether it called `present`.
    ///
    /// It presents nothing, and leaves the tree alone, while the first
    /// present waits: until the backdrop arrives, or `now` reaches the
    /// deadline. It makes the framebuffer and the flat base when they are
    /// missing, the base again when the theme's `bg` changed, and repaints
    /// the whole window ([`Tree::invalidate_all`]) when either is new or
    /// the backdrop arrived.
    ///
    /// # Errors
    ///
    /// - [`GuiError::Gfx`] if the framebuffer or the flat base cannot be
    ///   allocated, or the painter fails ([`Painter::paint`]); after the
    ///   painter fails, the next frame repaints the whole window.
    /// - What `present` returns. What it was to show is kept, and the next
    ///   render shows it.
    pub(crate) fn render(
        &mut self,
        tree: &mut Tree,
        now: Instant,
        present: impl FnOnce(&Framebuffer, &[DeviceRect]) -> Result<(), GuiError>,
    ) -> Result<bool, GuiError> {
        if self
            .wait_until
            .is_some_and(|until| now < until && !self.backdrop_ready())
        {
            return Ok(false);
        }
        self.wait_until = None;

        let bg = tree.theme().color(Token::Bg).premultiply();
        self.make_base(bg)?;
        let new_target = self.make_target()?;
        let new_base = std::mem::take(&mut self.base_changed);
        if new_target || new_base {
            tree.invalidate_all();
        }

        let logical = self.scale.to_logical(self.size);
        let frame = tree.frame(logical, &self.painter.metrics(self.scale));
        let (Some(target), Some(base)) = (self.target.as_mut(), self.base.pixels()) else {
            // Both were made above.
            return Ok(false);
        };
        let mut damage = match frame {
            Some(frame) => match self.painter.paint(target, &base.view(), &frame, self.scale) {
                Ok(painted) => painted,
                Err(error) => {
                    tree.invalidate_all();
                    return Err(error);
                }
            },
            None => Damage::new(),
        };
        for &rect in self.exposes.rects() {
            damage.add(rect);
        }
        damage.clip_to(target.bounds());
        if damage.is_empty() {
            self.exposes.clear();
            return Ok(false);
        }
        match present(target, damage.rects()) {
            Ok(()) => {
                self.exposes.clear();
                Ok(true)
            }
            Err(error) => {
                self.exposes = damage;
                Err(error)
            }
        }
    }

    /// Makes the flat base, cleared to `bg`, unless the base is the
    /// backdrop or a flat one of the frame's size and that color.
    fn make_base(&mut self, bg: PremultipliedColor) -> Result<(), GuiError> {
        match &self.base {
            Base::Backdrop(backdrop) if backdrop.base().size() == self.size => return Ok(()),
            Base::Flat(fb, color) if fb.size() == self.size && *color == bg => return Ok(()),
            _ => {}
        }
        self.base = Base::None;
        let mut flat = Framebuffer::new(self.size)?;
        flat.clear(bg);
        self.base = Base::Flat(flat, bg);
        self.base_changed = true;
        Ok(())
    }

    /// Makes the framebuffer, unless it is there at the frame's size, and
    /// returns `true` if it did.
    fn make_target(&mut self) -> Result<bool, GuiError> {
        if self
            .target
            .as_ref()
            .is_some_and(|target| target.size() == self.size)
        {
            return Ok(false);
        }
        self.target = None;
        self.target = Some(Framebuffer::new(self.size)?);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::sync::Arc;
    use std::time::Duration;

    use tidepool_core::color::Color;
    use tidepool_core::geom::{DevicePoint, Rect};
    use tidepool_gfx::backdrop::BackdropStyle;
    use tidepool_platform::surface::PixelsError;
    use tidepool_ui::draw::{DrawList, PaintContext};
    use tidepool_ui::layout::Style;
    use tidepool_ui::theme::{LIMITS, TIDEPOOL_DARK_TOML, Theme};
    use tidepool_ui::{Node, NodeId, Widget};

    use super::*;
    use crate::FIRST_FRAME_WAIT;

    /// Fills its bounds.
    struct Fill(Color);

    impl Widget for Fill {
        fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
            list.rect(cx.bounds(), self.0);
        }
    }

    const TEAL: Color = Color::rgb(0x5c, 0xcb, 0xcb);

    /// A tree with one node of 10 × 10 logical units at the top-left
    /// corner, filled in teal.
    fn square() -> (Tree, NodeId) {
        let mut tree = Tree::new();
        let node = Node::new(Style::default().fixed(10.0, 10.0)).with_widget(Fill(TEAL));
        let id = tree.insert(tree.root(), node).expect("a node");
        (tree, id)
    }

    /// A screen of `size` at `scale` whose first present does not wait.
    fn new_screen(size: DeviceSize, scale: f32) -> Screen {
        let painter = Painter::new().expect("the bundled faces load");
        Screen::new(painter, size, Scale::new(scale).expect("a scale"), None)
    }

    /// What one render presented: the rects and a copy of the
    /// framebuffer, or `None` if it presented nothing.
    type Shown = Option<(Vec<DeviceRect>, Framebuffer)>;

    /// Renders `tree` at `now`, and says what was presented.
    fn render_at(screen: &mut Screen, tree: &mut Tree, now: Instant) -> Shown {
        let shown = RefCell::new(None);
        let presented = screen
            .render(tree, now, |frame, rects| {
                let copy = frame.try_clone().expect("a copy");
                *shown.borrow_mut() = Some((rects.to_vec(), copy));
                Ok(())
            })
            .expect("the frame renders");
        let shown = shown.into_inner();
        assert_eq!(presented, shown.is_some());
        shown
    }

    fn render(screen: &mut Screen, tree: &mut Tree) -> Shown {
        render_at(screen, tree, Instant::now())
    }

    fn whole(size: DeviceSize) -> Vec<DeviceRect> {
        vec![DeviceRect::new(0, 0, size.width, size.height)]
    }

    fn backdrop(size: DeviceSize, scale: Scale) -> Backdrop {
        let mut backdrop = Backdrop::new();
        backdrop
            .ensure(size, scale, &BackdropStyle::jabbott_dev())
            .expect("the backdrop");
        backdrop
    }

    fn bg() -> PremultipliedColor {
        Theme::bundled().color(Token::Bg).premultiply()
    }

    #[test]
    fn the_tree_is_laid_out_in_logical_units_and_painted_in_device_pixels() {
        let size = DeviceSize::new(301, 157);
        for factor in [1.0, 1.5, 2.0] {
            let mut screen = new_screen(size, factor);
            let (mut tree, node) = square();
            let filler = tree
                .insert(
                    tree.root(),
                    Node::new(Style::default().fill()).with_widget(Fill(TEAL)),
                )
                .expect("a node");
            let (rects, frame) = render(&mut screen, &mut tree).expect("a present");

            // The window's logical size is its device size over the scale.
            let logical = Scale::new(factor).expect("a scale").to_logical(size);
            assert_eq!(
                tree.bounds(tree.root()),
                Some(Rect::new(0.0, 0.0, logical.width, logical.height)),
                "at {factor}"
            );
            assert_eq!(tree.bounds(node), Some(Rect::new(0.0, 0.0, 10.0, 10.0)));
            assert!(
                tree.bounds(filler)
                    .is_some_and(|rect| (rect.bottom() - logical.height).abs() < 1e-3),
                "at {factor}"
            );
            // Painted at the device size: the whole window, the filler
            // reaching its last row, and the square `10 × factor` pixels.
            assert_eq!(frame.size(), size, "at {factor}");
            assert_eq!(rects, whole(size), "at {factor}");
            let teal = Some(TEAL.premultiply());
            assert_eq!(frame.pixel(DevicePoint::new(0, 156)), teal, "at {factor}");
            let edge = (10.0 * factor) as i32;
            assert_eq!(frame.pixel(DevicePoint::new(edge - 1, edge - 1)), teal);
            assert_eq!(
                frame.pixel(DevicePoint::new(edge, edge - 1)),
                Some(bg()),
                "at {factor}: the square ends at {edge} px"
            );
        }
    }

    #[test]
    fn an_idle_tree_paints_and_presents_nothing() {
        let size = DeviceSize::new(64, 48);
        let mut screen = new_screen(size, 1.0);
        let (mut tree, _) = square();
        assert!(render(&mut screen, &mut tree).is_some());
        for _ in 0..3 {
            let called = RefCell::new(false);
            let presented = screen
                .render(&mut tree, Instant::now(), |_, _| {
                    *called.borrow_mut() = true;
                    Ok(())
                })
                .expect("a render");
            assert!(!presented && !called.into_inner());
        }
    }

    #[test]
    fn a_new_size_repaints_the_whole_window() {
        let mut screen = new_screen(DeviceSize::new(64, 48), 1.5);
        let (mut tree, _) = square();
        assert!(render(&mut screen, &mut tree).is_some());
        assert!(!screen.resize(DeviceSize::new(64, 48)), "the same size");
        assert!(render(&mut screen, &mut tree).is_none());

        let larger = DeviceSize::new(90, 70);
        assert!(screen.resize(larger));
        assert_eq!(screen.size, larger);
        let (rects, frame) = render(&mut screen, &mut tree).expect("a present");
        assert_eq!(rects, whole(larger));
        assert_eq!(frame.size(), larger);
        assert_eq!(frame.pixel(DevicePoint::new(89, 69)), Some(bg()));
        assert!(render(&mut screen, &mut tree).is_none());
    }

    #[test]
    fn a_new_base_repaints_the_whole_window_though_the_tree_did_not_change() {
        for factor in [1.0, 1.5, 2.0] {
            let scale = Scale::new(factor).expect("a scale");
            let size = DeviceSize::new(96, 64);
            let mut screen = new_screen(size, factor);
            let (mut tree, _) = square();
            let (_, flat) = render(&mut screen, &mut tree).expect("over the flat bg");
            assert_eq!(flat.pixel(DevicePoint::new(60, 40)), Some(bg()));
            assert!(!screen.backdrop_ready());

            // A backdrop of another size is dropped.
            screen.set_backdrop(backdrop(DeviceSize::new(95, 64), scale));
            assert!(!screen.backdrop_ready());
            assert!(render(&mut screen, &mut tree).is_none());

            let arrived = backdrop(size, scale);
            let expected = arrived.base().try_clone().expect("a copy");
            screen.set_backdrop(arrived);
            assert!(screen.backdrop_ready());
            let (rects, frame) = render(&mut screen, &mut tree).expect("over the backdrop");
            assert_eq!(rects, whole(size), "at {factor}");
            // Outside the square, the backdrop's pixels exactly.
            let edge = (10.0 * factor) as i32;
            for y in 0..size.height {
                for x in 0..size.width {
                    let at = DevicePoint::new(x, y);
                    if x >= edge || y >= edge {
                        assert_eq!(frame.pixel(at), expected.pixel(at), "at {at:?}");
                    } else {
                        assert_eq!(frame.pixel(at), Some(TEAL.premultiply()));
                    }
                }
            }
            assert_ne!(flat.bytes(), frame.bytes(), "the backdrop is not flat");
            assert!(render(&mut screen, &mut tree).is_none(), "and idle again");

            // A resize goes back to the flat bg until the next backdrop.
            assert!(screen.resize(DeviceSize::new(100, 64)));
            assert!(!screen.backdrop_ready());
            let (_, frame) = render(&mut screen, &mut tree).expect("over the flat bg");
            assert_eq!(frame.pixel(DevicePoint::new(60, 40)), Some(bg()));
        }
    }

    #[test]
    fn the_first_present_waits_for_the_backdrop_until_the_deadline() {
        let size = DeviceSize::new(48, 32);
        let opened = Instant::now();
        let deadline = opened + FIRST_FRAME_WAIT;
        let painter = Painter::new().expect("the bundled faces load");
        let mut screen = Screen::new(painter, size, Scale::ONE, Some(deadline));
        let (mut tree, _) = square();
        assert_eq!(screen.next_deadline(), Some(deadline));
        assert!(render_at(&mut screen, &mut tree, opened).is_none());
        let just_before = deadline - Duration::from_millis(1);
        assert!(render_at(&mut screen, &mut tree, just_before).is_none());

        // The backdrop arrives first: the wait ends, and it is shown.
        let arrived = backdrop(size, Scale::ONE);
        let expected = arrived.base().try_clone().expect("a copy");
        screen.set_backdrop(arrived);
        assert_eq!(screen.next_deadline(), None);
        let (rects, frame) = render_at(&mut screen, &mut tree, opened).expect("a present");
        assert_eq!(rects, whole(size));
        let at = DevicePoint::new(40, 20);
        assert_eq!(frame.pixel(at), expected.pixel(at));

        // Or the deadline comes first: the flat bg is shown, and the wait
        // is over for good, the backdrop or not.
        let painter = Painter::new().expect("the bundled faces load");
        let mut screen = Screen::new(painter, size, Scale::ONE, Some(deadline));
        let (mut tree, _) = square();
        let (_, frame) = render_at(&mut screen, &mut tree, deadline).expect("a present");
        assert_eq!(frame.pixel(at), Some(bg()));
        assert_eq!(screen.next_deadline(), None);
        assert!(screen.resize(DeviceSize::new(50, 32)));
        assert!(render_at(&mut screen, &mut tree, opened).is_some());
    }

    #[test]
    fn exposes_are_kept_until_a_present_shows_them() {
        let size = DeviceSize::new(64, 48);
        let opened = Instant::now();
        let deadline = opened + FIRST_FRAME_WAIT;
        let painter = Painter::new().expect("the bundled faces load");
        let mut screen = Screen::new(painter, size, Scale::ONE, Some(deadline));
        let (mut tree, _) = square();
        // Before the first paint, kept while the present waits.
        screen.expose(DeviceRect::new(0, 0, 64, 48));
        assert!(render_at(&mut screen, &mut tree, opened).is_none());
        assert_eq!(screen.exposes.rects(), whole(size));
        let (rects, _) = render_at(&mut screen, &mut tree, deadline).expect("the first paint");
        assert_eq!(rects, whole(size));
        assert!(screen.exposes.is_empty());

        // After it, an idle tree presents what was exposed, cut to the
        // frame, from the framebuffer.
        screen.expose(DeviceRect::new(50, 40, 30, 30));
        screen.expose(DeviceRect::new(5, 5, 10, 10));
        let (rects, _) = render(&mut screen, &mut tree).expect("the exposes");
        assert_eq!(
            rects,
            [
                DeviceRect::new(50, 40, 14, 8),
                DeviceRect::new(5, 5, 10, 10)
            ]
        );
        screen.expose(DeviceRect::new(64, 0, 10, 10));
        assert!(
            render(&mut screen, &mut tree).is_none(),
            "outside the frame"
        );
        assert!(render(&mut screen, &mut tree).is_none());
    }

    #[test]
    fn a_failed_present_is_presented_again_by_the_next_render() {
        let size = DeviceSize::new(40, 30);
        let mut screen = new_screen(size, 1.0);
        let (mut tree, _) = square();
        let error = screen
            .render(&mut tree, Instant::now(), |_, _| {
                Err(GuiError::Pixels(PixelsError::TooFewBytes {
                    len: 0,
                    needed: 4,
                }))
            })
            .expect_err("the present failed");
        assert!(matches!(error, GuiError::Pixels(_)), "{error:?}");
        let (rects, _) = render(&mut screen, &mut tree).expect("again");
        assert_eq!(rects, whole(size));
        assert!(render(&mut screen, &mut tree).is_none());
    }

    #[test]
    fn the_flat_base_is_the_themes_bg_and_follows_it() {
        let size = DeviceSize::new(40, 30);
        let mut screen = new_screen(size, 1.0);
        let (mut tree, _) = square();
        let at = DevicePoint::new(30, 20);
        let (_, frame) = render(&mut screen, &mut tree).expect("a present");
        assert_eq!(frame.pixel(at), Some(bg()));

        let lighter =
            TIDEPOOL_DARK_TOML.replace("bg = { value = \"#0b0c0d\"", "bg = { value = \"#202428\"");
        assert_ne!(lighter, TIDEPOOL_DARK_TOML);
        tree.set_theme(Arc::new(Theme::parse(&lighter, LIMITS).expect("a theme")));
        let (rects, frame) = render(&mut screen, &mut tree).expect("a present");
        assert_eq!(rects, whole(size));
        assert_eq!(
            frame.pixel(at),
            Some(Color::rgb(0x20, 0x24, 0x28).premultiply())
        );
    }

    #[test]
    fn the_backdrops_bg_is_the_themes() {
        // The theme carries no backdrop style yet (ADR-0047), so the
        // backdrop is `gfx`'s jabbott.dev style. Its `bg` must stay the
        // token the flat base is cleared to, which `cargo xtask theme`
        // measures text against, so that the backdrop arriving changes no
        // contrast.
        let theme = Theme::tidepool_dark();
        let style = BackdropStyle::jabbott_dev();
        assert_eq!(style.bg, theme.color(Token::Bg));
        // Its drawings keep their own colors too, which are the theme's
        // `bronze` and `purple` (ADR-0057).
        assert_eq!(style.circuits.color, theme.color(Token::Bronze));
        assert_eq!(style.synapses.color, theme.color(Token::Purple));
    }

    #[test]
    fn small_and_empty_frames_render_and_a_size_gfx_refuses_is_an_error() {
        for (width, height) in [(1, 1), (7, 3), (40, 40)] {
            let size = DeviceSize::new(width, height);
            let mut screen = new_screen(size, 2.0);
            let (mut tree, _) = square();
            let (rects, frame) = render(&mut screen, &mut tree).expect("a present");
            assert_eq!((rects, frame.size()), (whole(size), size));
        }
        let mut screen = new_screen(DeviceSize::new(0, 0), 1.0);
        let (mut tree, _) = square();
        assert!(render(&mut screen, &mut tree).is_none(), "nothing to show");
        assert_eq!(screen.next_deadline(), None);

        let mut screen = new_screen(DeviceSize::new(-1, 10), 1.0);
        let error = screen
            .render(&mut tree, Instant::now(), |_, _| Ok(()))
            .expect_err("a negative side");
        assert!(matches!(error, GuiError::Gfx(_)), "{error:?}");
        // It goes on at a size it can draw.
        assert!(screen.resize(DeviceSize::new(8, 8)));
        assert!(render(&mut screen, &mut tree).is_some());
    }
}
