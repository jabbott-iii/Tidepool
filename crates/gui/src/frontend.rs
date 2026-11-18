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

//! [`Gui`]: the window, and what it shows, a turn at a time.
//!
//! New in Tidepool.
//!
//! The window is `tidepool-platform`'s. Each [`Gui::turn`] waits for its
//! events, which come with everything else that is waiting, and answers
//! them: an [`Event::Exposed`] is presented from the frame, which is drawn
//! anew first, at the size the window has then, when the window's size or
//! scale differs from the frame's (ADR-0045); [`Event::CloseRequested`] is
//! passed on. A frame drawn anew is presented whole. Input is not used
//! until M3.4's shell.
//!
//! The frame is at most `gfx`'s [`MAX_DIMENSION`] (16,384 px) a side; a
//! larger window shows it in its top-left corner, and the window's own
//! background beyond.
//!
//! Drawing happens on the calling thread: the first frame and a resize cost
//! what [`FirstFrame::draw`] costs, most of it the backdrop. A resize that
//! a window manager makes right after the window is shown, as COSMIC does
//! about 1 ms after the first expose, comes while the first frame is being
//! drawn, so the frame is drawn again at the new size.

use std::time::Instant;

use tidepool_core::geom::{DeviceSize, Scale};
use tidepool_gfx::damage::Damage;
use tidepool_gfx::framebuffer::{Framebuffer, MAX_DIMENSION};
use tidepool_platform::events::Event;
use tidepool_platform::surface::Pixels;
use tidepool_platform::window::{Window, WindowConfig};

use crate::GuiError;
use crate::first_frame::FirstFrame;

/// The GUI's window and what draws it.
#[derive(Debug)]
pub struct Gui {
    window: Window,
    first_frame: FirstFrame,
    /// The frame last drawn and the scale it was drawn at; `None` until
    /// the window is first exposed.
    frame: Option<(Framebuffer, Scale)>,
    /// What the window asked for since the last present.
    damage: Damage,
    /// The events of a turn; cleared each turn, its allocation reused.
    events: Vec<Event>,
}

/// What one [`Gui::turn`] did.
///
/// Non-exhaustive: later tasks may report more.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Turn {
    /// Whether it presented a frame, or part of one.
    pub presented: bool,
    /// Whether the user asked to close the window, as with its close
    /// button. It stays open until the [`Gui`] is dropped.
    pub close_requested: bool,
}

impl Gui {
    /// Opens a window as `config` says, and loads what draws it: the
    /// bundled faces, an empty glyph atlas and the backdrop's cache.
    /// Nothing is drawn until a [`turn`](Gui::turn) sees the window
    /// exposed.
    ///
    /// # Errors
    ///
    /// - [`GuiError::Platform`] with the errors of
    ///   [`Window::open`]: on a system with no window yet, with no local X
    ///   server, or one that refuses the window.
    /// - [`GuiError::Font`] if the bundled faces do not load, which would
    ///   be a bug.
    pub fn open(config: &WindowConfig) -> Result<Self, GuiError> {
        let window = Window::open(config)?;
        Ok(Self {
            window,
            first_frame: FirstFrame::new()?,
            frame: None,
            damage: Damage::new(),
            events: Vec::new(),
        })
    }

    /// The inside of the window, in device pixels, as of the last resize a
    /// turn took.
    pub fn size(&self) -> DeviceSize {
        self.window.size()
    }

    /// Waits until the window has events or `deadline` passes (`None`
    /// waits for as long as it takes), then answers them and says what it
    /// did: the exposes are presented from the frame, drawn anew first,
    /// and presented whole, if the window's size or scale is not the
    /// frame's; a close request is reported. Input is not used yet.
    ///
    /// # Errors
    ///
    /// - [`GuiError::Platform`] if waiting on the window or presenting to
    ///   it failed.
    /// - [`GuiError::Gfx`], [`GuiError::Font`] or [`GuiError::Pixels`] if
    ///   the frame could not be drawn. Nothing is presented then; the
    ///   damage is kept, and the next turn that gets an event or reaches
    ///   its deadline tries again.
    pub fn turn(&mut self, deadline: Option<Instant>) -> Result<Turn, GuiError> {
        let mut turn = Turn::default();
        self.events.clear();
        self.window.wait_events(deadline, &mut self.events)?;
        take(&self.events, &mut self.damage, &mut turn);
        if self.damage.is_empty() {
            return Ok(turn);
        }
        let (size, scale) = (frame_size(self.window.size()), self.window.scale());
        let current = matches!(
            &self.frame,
            Some((frame, drawn)) if frame.size() == size && *drawn == scale
        );
        if !current {
            // Dropped first, so the old frame and the new are not both held.
            self.frame = None;
            let frame = self.first_frame.draw(size, scale)?;
            self.damage.add(frame.bounds());
            self.frame = Some((frame, scale));
        }
        let Some((frame, _)) = &self.frame else {
            return Ok(turn);
        };
        self.damage.clip_to(frame.bounds());
        if !self.damage.is_empty() {
            let pixels = Pixels::new(frame.bytes(), frame.size(), frame.stride())?;
            self.window.present(&pixels, self.damage.rects())?;
            turn.presented = true;
        }
        self.damage.clear();
        Ok(turn)
    }
}

/// Adds the exposes among `events` to `damage` and notes a close request
/// in `turn`; a resize is read from the window when the frame is drawn.
fn take(events: &[Event], damage: &mut Damage, turn: &mut Turn) {
    for event in events {
        match event {
            Event::Exposed(rect) => damage.add(*rect),
            Event::CloseRequested => turn.close_requested = true,
            Event::Resized(_) | Event::Input(_) | Event::Woken | Event::Pasted { .. } => {}
        }
    }
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

    #[test]
    fn exposes_are_damage_a_close_request_is_reported_and_the_rest_is_ignored() {
        let (mut damage, mut turn) = (Damage::new(), Turn::default());
        take(
            &[
                Event::Exposed(DeviceRect::new(0, 0, 10, 10)),
                Event::Resized(DeviceSize::new(640, 480)),
                Event::Input(InputEvent::FocusGained),
                Event::Woken,
                Event::Exposed(DeviceRect::new(100, 100, 10, 10)),
            ],
            &mut damage,
            &mut turn,
        );
        assert_eq!(
            damage.rects(),
            [
                DeviceRect::new(0, 0, 10, 10),
                DeviceRect::new(100, 100, 10, 10)
            ]
        );
        assert_eq!(turn, Turn::default());

        take(&[Event::CloseRequested], &mut damage, &mut turn);
        assert!(turn.close_requested && !turn.presented);
        assert_eq!(damage.rects().len(), 2, "a close request damages nothing");
    }

    #[test]
    fn a_resize_alone_damages_nothing() {
        let (mut damage, mut turn) = (Damage::new(), Turn::default());
        take(
            &[Event::Resized(DeviceSize::new(1232, 662))],
            &mut damage,
            &mut turn,
        );
        assert!(damage.is_empty());
        assert_eq!(turn, Turn::default());
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
