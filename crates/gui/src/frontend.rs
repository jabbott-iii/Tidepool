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

//! [`Gui`]: the window, and the `ui` frames it shows (M3.8, ADR-0059),
//! with the icons registered for them (M3.4, ADR-0060). [`Gui`]'s
//! documentation says what it shows, and why.
//!
//! New in Tidepool.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tidepool_core::geom::{DeviceSize, Scale};
use tidepool_gfx::backdrop::BackdropStyle;
use tidepool_gfx::framebuffer::MAX_DIMENSION;
use tidepool_platform::events::Event;
use tidepool_platform::surface::Pixels;
use tidepool_platform::window::{Window, WindowConfig};
use tidepool_ui::Tree;
use tidepool_ui::draw::IconId;

use crate::builds::{Builds, Threads, Want};
use crate::screen::Screen;
use crate::{GuiError, Icon, Painter};

/// How long the first present waits for the backdrop after the window
/// opened: 250 ms ([`Gui`'s "The first present waits"](Gui#the-first-present-waits)).
pub const FIRST_FRAME_WAIT: Duration = Duration::from_millis(250);

/// The GUI's window, and what paints it: `ui`'s frames, over jabbott.dev's
/// backdrop.
///
/// # A turn of the main loop
///
/// The `tidepool` binary's main loop (`docs/01-architecture.md`, ADR-0012)
/// calls, each turn:
///
/// 1. [`Gui::wait_events`], which waits until the window has events, a
///    thread wakes it, or the deadline passes. The loop passes the earliest
///    deadline it has, [`Gui::next_deadline`] for now. It notes what the
///    window asks for (an [`Event::Exposed`] to show again, an
///    [`Event::Resized`]), takes the backdrop's finished builds, and leaves
///    every event for the loop.
/// 2. [`Tree::handle`] for each [`Event::Input`], before anything else,
///    and it ends on an [`Event::CloseRequested`]. That part is the
///    loop's.
/// 3. [`Gui::render`], which paints the tree's frame and presents it.
///
/// The thread that calls them never waits for anything but the window's
/// events: no backdrop is built on it.
///
/// # The frame
///
/// The window is presented from a framebuffer of its size, at most `gfx`'s
/// [`MAX_DIMENSION`] (16,384 px) a side; a larger window shows the frame in
/// its top-left corner, and its own black background beyond. A `ui` tree
/// is laid out in logical units, at the framebuffer's size over the
/// window's scale, with the [`Painter`]'s
/// [`FontMetrics`](crate::FontMetrics) at that scale. Its frame's damage
/// (the parts that changed and must be drawn again) is painted over the
/// *base*, the image under the tree, and what was painted is presented
/// with what the window asked to have shown again. A new framebuffer
/// (after a resize) or a new base repaints the whole window, through
/// [`Tree::invalidate_all`]. An idle tree paints and presents nothing.
///
/// HiDPI: the scale is the window's, read once when it opens (on Linux,
/// from the `Xft.dpi` resource), and may be fractional, as 1.5.
///
/// # The base: the flat `bg`, then the backdrop
///
/// The base is jabbott.dev's backdrop (`gfx::backdrop`) of the frame's
/// size and scale, in [`BackdropStyle::jabbott_dev`]. Building it takes
/// 99 to 147 ms at 1280 × 800, and 189 ms at 1920 × 1080, in a release
/// build on the reference machine (ADR-0045), so it is built on threads of
/// its own: asked for when the window opens and again on each resize, at
/// most two at once, a waiting request replaced by a newer one, and a
/// result for a size or scale the window no longer has dropped. Each
/// finished build wakes the window's wait, through its
/// [`Waker`](tidepool_platform::events::Waker).
///
/// Until the backdrop of the window's size arrives, the base is a
/// framebuffer cleared to the tree's theme's `bg`, which is the backdrop's
/// own `bg`, so text over either keeps its contrast. A build that fails, or
/// a thread that cannot start, leaves the flat `bg` and logs a warning; the
/// next resize asks again. So a resize repaints the window at once over
/// the flat `bg`, and again when its backdrop arrives.
///
/// A build cannot be stopped. One still running when the `Gui` is dropped
/// is left to finish: its result is dropped, and its wake does nothing
/// anyone sees. The process ending ends it.
///
/// # The first present waits
///
/// So that the window does not show the flat `bg` and then the backdrop at
/// every start, [`Gui::render`] presents nothing until the first backdrop
/// arrives or [`FIRST_FRAME_WAIT`] has passed since the window opened,
/// whichever comes first; [`Gui::next_deadline`] gives the loop that
/// deadline. The window shows its black background meanwhile, and the
/// loop goes on handling events. A window whose backdrop takes longer, as
/// at 2560 × 1440 or larger, shows the flat `bg` first.
#[derive(Debug)]
pub struct Gui {
    window: Window,
    screen: Screen,
    builds: Builds<Threads>,
}

impl Gui {
    /// Opens a window as `config` says, starts building the backdrop for
    /// its size and scale, and loads what paints it: the bundled faces and
    /// an empty glyph atlas. Nothing is painted until a
    /// [`render`](Gui::render).
    ///
    /// # Errors
    ///
    /// - [`GuiError::Platform`] with the errors of [`Window::open`]: on a
    ///   system with no window yet, with no local X server, or one that
    ///   refuses the window.
    /// - [`GuiError::Font`] if the bundled faces do not load, which would
    ///   be a bug.
    pub fn open(config: &WindowConfig) -> Result<Self, GuiError> {
        let window = Window::open(config)?;
        let opened = Instant::now();
        let want = Want {
            size: frame_size(window.size()),
            scale: window.scale(),
        };
        let waker = window.waker();
        let wake = Arc::new(move || waker.wake());
        let mut builds = Builds::new(Threads::new(BackdropStyle::jabbott_dev(), wake));
        builds.request(want);
        let painter = Painter::new()?;
        let screen = Screen::new(
            painter,
            want.size,
            want.scale,
            opened.checked_add(FIRST_FRAME_WAIT),
        );
        Ok(Self {
            window,
            screen,
            builds,
        })
    }

    /// The inside of the window, in device pixels, as of the last resize
    /// that [`wait_events`](Gui::wait_events) reported.
    pub fn size(&self) -> DeviceSize {
        self.window.size()
    }

    /// The window's device pixels per logical unit, read once when it
    /// opened.
    pub fn scale(&self) -> Scale {
        self.window.scale()
    }

    /// The deadline of the first present's wait for the backdrop
    /// ([`FIRST_FRAME_WAIT`] after the window opened), for the loop to wait
    /// until; `None` once [`render`](Gui::render) no longer waits, because
    /// the backdrop arrived or a render went past the deadline.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.screen.next_deadline()
    }

    /// Waits until the window has events, a thread wakes it, or `deadline`
    /// passes (`None` waits for as long as it takes), and appends them to
    /// `events`, which it does not clear.
    ///
    /// It notes what they ask of the window: an [`Event::Exposed`] is shown
    /// again by the next [`render`](Gui::render); after an
    /// [`Event::Resized`], that render paints the whole window at its new
    /// size over the flat `bg`, and the backdrop for that size is asked
    /// for. Then it takes the backdrop's builds that finished, without
    /// waiting; one for the window's size and scale is painted under the
    /// tree from the next render, which repaints the whole window. Every
    /// event stays in `events` for the caller, who routes the input and
    /// answers a close request.
    ///
    /// # Errors
    ///
    /// [`GuiError::Platform`] if waiting on the window failed
    /// ([`Window::wait_events`]).
    pub fn wait_events(
        &mut self,
        deadline: Option<Instant>,
        events: &mut Vec<Event>,
    ) -> Result<(), GuiError> {
        let from = events.len();
        self.window.wait_events(deadline, events)?;
        let resized = note(&events[from..], &mut self.screen);
        if resized {
            let size = frame_size(self.window.size());
            if self.screen.resize(size) {
                self.builds.request(Want {
                    size,
                    scale: self.window.scale(),
                });
            }
        }
        if let Some(backdrop) = self.builds.poll() {
            self.screen.set_backdrop(backdrop);
        }
        Ok(())
    }

    /// Lays `tree` out at the window's size in logical units, paints its
    /// frame's damage over the base, and presents that and what the window
    /// asked to have shown again. Returns whether it presented anything:
    /// not for an idle tree in a window that asked for nothing, nor while
    /// the first present waits for the backdrop, when it leaves the tree
    /// alone (the [type's documentation](Gui)).
    ///
    /// # Errors
    ///
    /// - [`GuiError::Gfx`] if the framebuffer or the flat base cannot be
    ///   allocated, or the [`Painter`] fails. After the painter fails, the
    ///   tree's next frame repaints the whole window.
    /// - [`GuiError::Pixels`] or [`GuiError::Platform`] if presenting
    ///   failed. What was to be presented is kept, and the next render
    ///   presents it.
    pub fn render(&mut self, tree: &mut Tree) -> Result<bool, GuiError> {
        let window = &mut self.window;
        self.screen.render(tree, Instant::now(), |frame, rects| {
            let pixels = Pixels::new(frame.bytes(), frame.size(), frame.stride())?;
            window.present(&pixels, rects)?;
            Ok(())
        })
    }

    /// Whether the base is the backdrop of the window's size, rather than
    /// the flat `bg` that stands in until its build arrives.
    pub fn backdrop_ready(&self) -> bool {
        self.screen.backdrop_ready()
    }

    /// Registers `icon` for `id` with the window's [`Painter`], so that a
    /// `ui` icon command naming `id` draws it, and returns the icon it
    /// replaces. [`icons`](crate::icons) has Tidepool's.
    ///
    /// Unless `icon` is the one `id` already had, the next
    /// [`render`](Gui::render) repaints the whole window
    /// ([`Tree::invalidate_all`]): `ui` cannot know that a command naming
    /// `id` now draws other pixels, and the painter leaves that damage to
    /// whoever changed the registration ([`Painter::set_icon`], ADR-0048).
    /// The whole window, because which nodes draw `id` is the tree's
    /// owner's to know, not the window's. An icon registered before the
    /// first render costs no extra paint: the first frame paints the whole
    /// window anyway.
    pub fn set_icon(&mut self, id: IconId, icon: Icon) -> Option<Icon> {
        self.screen.set_icon(id, icon)
    }
}

/// Gives `screen` the exposes among `events`, and returns whether one of
/// them is a resize, whose size is read from the window.
fn note(events: &[Event], screen: &mut Screen) -> bool {
    let mut resized = false;
    for event in events {
        match event {
            Event::Exposed(rect) => screen.expose(*rect),
            Event::Resized(_) => resized = true,
            Event::Input(_) | Event::CloseRequested | Event::Woken | Event::Pasted { .. } => {}
        }
    }
    resized
}

/// The size of the frame for a window of `window` device pixels: the
/// window's, up to `gfx`'s [`MAX_DIMENSION`] a side.
fn frame_size(window: DeviceSize) -> DeviceSize {
    DeviceSize::new(
        window.width.min(MAX_DIMENSION),
        window.height.min(MAX_DIMENSION),
    )
}

#[cfg(test)]
mod tests {
    use tidepool_core::geom::DeviceRect;
    use tidepool_core::input::InputEvent;

    use super::*;

    fn screen() -> Screen {
        let painter = Painter::new().expect("the bundled faces load");
        Screen::new(painter, DeviceSize::new(320, 200), Scale::ONE, None)
    }

    #[test]
    fn exposes_go_to_the_screen_a_resize_is_reported_and_the_rest_is_left() {
        let mut screen = screen();
        let resized = note(
            &[
                Event::Exposed(DeviceRect::new(0, 0, 10, 10)),
                Event::Input(InputEvent::FocusGained),
                Event::Woken,
                Event::CloseRequested,
                Event::Exposed(DeviceRect::new(100, 100, 10, 10)),
            ],
            &mut screen,
        );
        assert!(!resized);
        let mut tree = Tree::new();
        let mut shown = Vec::new();
        // The tree's first frame paints the whole window, which holds the
        // exposes.
        screen
            .render(&mut tree, Instant::now(), |_, rects| {
                shown.extend_from_slice(rects);
                Ok(())
            })
            .expect("a render");
        assert_eq!(shown, [DeviceRect::new(0, 0, 320, 200)]);

        let resized = note(
            &[
                Event::Resized(DeviceSize::new(640, 480)),
                Event::Exposed(DeviceRect::new(5, 5, 10, 10)),
            ],
            &mut screen,
        );
        assert!(resized);
        shown.clear();
        screen
            .render(&mut tree, Instant::now(), |_, rects| {
                shown.extend_from_slice(rects);
                Ok(())
            })
            .expect("a render");
        assert_eq!(shown, [DeviceRect::new(5, 5, 10, 10)], "the expose alone");
    }

    #[test]
    fn the_frame_is_the_windows_size_up_to_gfxs_limit() {
        assert_eq!(
            frame_size(DeviceSize::new(1232, 662)),
            DeviceSize::new(1232, 662)
        );
        assert_eq!(
            frame_size(DeviceSize::new(32_767, 900)),
            DeviceSize::new(MAX_DIMENSION, 900)
        );
        assert_eq!(
            frame_size(DeviceSize::new(800, 20_000)),
            DeviceSize::new(800, MAX_DIMENSION)
        );
    }
}
