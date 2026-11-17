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

//! A window's events: [`Window::wait_events`], which waits for them, the
//! [`Event`]s it reports, and the [`Waker`] that ends a wait from any
//! thread.
//!
//! New in Tidepool. The Linux backend ports Nadir's translation of X events
//! in part (`docs/03-platforms.md` → Linux).
//!
//! The main loop (`docs/01-architecture.md`) waits here for input, a
//! resize, a wake from a worker or the next timer, and never spins: a
//! window with nothing to report and no deadline waits without using the
//! processor.

use core::fmt;
use std::time::Instant;

use tidepool_core::geom::{DeviceRect, DeviceSize};
use tidepool_core::input::InputEvent;

use crate::window::Window;
use crate::{Error, backend};

/// Something that happened to a window.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Input: a key, typed text, the pointer, the wheel or the keyboard
    /// focus. Positions are in logical units, from the top-left corner of
    /// the inside of the window.
    ///
    /// On Linux: every [`Key`](tidepool_core::input::Key) can be reported,
    /// from the key's code, whatever the keyboard layout; a key with no
    /// `Key` is not. A key held down repeats as
    /// [`InputEvent::KeyDown`] with `repeat` set. Text comes after the key
    /// press that typed it, without control characters, and none comes
    /// while Control, Alt or Super is held, which makes the key a
    /// shortcut; dead keys and Compose sequences type through Xlib's own
    /// input method. When the window loses the focus, every key still down
    /// is reported going up. Consecutive pointer motions are reported as
    /// the last of them.
    Input(InputEvent),
    /// The inside of the window has this size now, in device pixels. What
    /// it showed is gone; the system reports what to draw again with
    /// [`Event::Exposed`].
    Resized(DeviceSize),
    /// This rectangle of the window, in device pixels, must be drawn
    /// again, as when it is first shown or uncovered. The system shows the
    /// window's black background there until then.
    Exposed(DeviceRect),
    /// The user asked to close the window, as with its close button. It
    /// stays open until it is dropped.
    CloseRequested,
    /// A [`Waker`] woke the wait. However many times it was woken since the
    /// last wait, this is reported once.
    Woken,
}

/// Ends a window's [`wait_events`](Window::wait_events) from any thread,
/// with [`Event::Woken`].
///
/// It is cheap to clone, and stays valid after its window is dropped,
/// when waking does nothing anyone sees. A wake that comes while the
/// window is not waiting is kept for the next wait, which then returns at
/// once.
#[derive(Clone)]
pub struct Waker {
    backend: backend::Waker,
}

impl Waker {
    /// Wakes the window's wait, or the next one.
    ///
    /// It never blocks. On Linux it is one `write(2)` of a byte to a
    /// non-blocking pipe, and so is async-signal-safe: a signal handler may
    /// call it, as long as the handler saves and restores `errno` around
    /// it, as around any call it makes, since a full pipe sets it. A full
    /// pipe means a wake is already pending, and is not an error.
    pub fn wake(&self) {
        self.backend.wake();
    }
}

impl fmt::Debug for Waker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Waker").finish_non_exhaustive()
    }
}

impl Window {
    /// Waits until the window has events, a [`Waker`] wakes it, or
    /// `deadline` passes, and appends what happened to `events`, which it
    /// does not clear. `None` waits for as long as it takes.
    ///
    /// It returns as soon as there is something to report, with
    /// everything that is waiting, and otherwise when the deadline passes,
    /// with nothing; a deadline that has passed already reports what is
    /// waiting without waiting. It never waits for longer than the
    /// deadline, give or take the system's timer, and wakes for nothing
    /// else: a signal that interrupts the wait does not end it.
    ///
    /// On Linux it first sends Xlib every request queued since the last
    /// call (as [`present`](Window::present) and
    /// [`set_title`](Window::set_title) queue them), and logs the requests
    /// the X server refused since then.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if waiting or reading the waker failed, and
    /// [`Error::ConnectionLost`] if the X server hung up. Xlib's own
    /// handler of a lost connection ends the process when it sees one, so
    /// the second is reported only if the process lives on.
    pub fn wait_events(
        &mut self,
        deadline: Option<Instant>,
        events: &mut Vec<Event>,
    ) -> Result<(), Error> {
        self.backend.wait_events(deadline, events)
    }

    /// Returns a waker for this window's waits.
    pub fn waker(&self) -> Waker {
        Waker {
            backend: self.backend.waker(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_send_sync_clone<T: Send + Sync + Clone>() {}

    #[test]
    fn a_waker_crosses_threads() {
        is_send_sync_clone::<Waker>();
    }
}
