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
use tidepool_core::input::{InputEvent, TextLength};

use crate::clipboard::Selection;
use crate::window::Window;
use crate::{Error, backend};

/// Something that happened to a window.
///
/// Its `Debug` shows the length of a text the user typed or pasted, never
/// the text, which can be a password: `Pasted { selection: Clipboard,
/// text: Some(<7 bytes>) }`, and `Input(Text(<7 bytes>))` as
/// [`InputEvent`]'s own `Debug` shows it. Every other event shows as a
/// derived `Debug` would.
#[derive(Clone, PartialEq)]
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
    /// The answer to [`Window::request_selection`]: the selection's text,
    /// or `None` if there is none to paste, as that method says.
    Pasted {
        /// The selection asked for.
        selection: Selection,
        /// Its text.
        text: Option<String>,
    },
}

impl fmt::Debug for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // `InputEvent`'s `Debug` keeps typed text out.
            Self::Input(input) => f.debug_tuple("Input").field(input).finish(),
            Self::Resized(size) => f.debug_tuple("Resized").field(size).finish(),
            Self::Exposed(rect) => f.debug_tuple("Exposed").field(rect).finish(),
            Self::CloseRequested => f.write_str("CloseRequested"),
            Self::Woken => f.write_str("Woken"),
            Self::Pasted { selection, text } => f
                .debug_struct("Pasted")
                .field("selection", selection)
                .field("text", &text.as_deref().map(TextLength))
                .finish(),
        }
    }
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
    /// deadline, give or take the system's timer and one turn of taking
    /// events (on Linux at most 10 ms, after Xlib has read what the X
    /// server sent), and ends for nothing else: a signal that interrupts
    /// the wait does not end it.
    ///
    /// While it waits, it also answers other programs that ask for a
    /// selection the window set ([`Window::set_selection`]), and reports
    /// none of that.
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

    #[test]
    fn debug_shows_a_pasted_or_typed_texts_length_and_never_the_text() {
        let pasted = Event::Pasted {
            selection: Selection::Clipboard,
            text: Some("hunter2".to_string()),
        };
        let shown = format!("{pasted:?}");
        assert_eq!(
            shown,
            "Pasted { selection: Clipboard, text: Some(<7 bytes>) }"
        );
        assert!(!shown.contains("hunter2"), "{shown}");
        assert!(!format!("{pasted:#?}").contains("hunter2"));
        let none = Event::Pasted {
            selection: Selection::Primary,
            text: None,
        };
        assert_eq!(
            format!("{none:?}"),
            "Pasted { selection: Primary, text: None }"
        );
        let typed = Event::Input(InputEvent::Text("hunter2".to_string()));
        assert_eq!(format!("{typed:?}"), "Input(Text(<7 bytes>))");
    }

    #[test]
    fn debug_shows_every_other_event_as_derived() {
        let size = DeviceSize::new(640, 480);
        let rect = DeviceRect::new(1, 2, 3, 4);
        let cases = [
            (
                Event::Input(InputEvent::FocusGained),
                "Input(FocusGained)".to_string(),
            ),
            (Event::Resized(size), format!("Resized({size:?})")),
            (Event::Exposed(rect), format!("Exposed({rect:?})")),
            (Event::CloseRequested, "CloseRequested".to_string()),
            (Event::Woken, "Woken".to_string()),
        ];
        for (event, expected) in cases {
            assert_eq!(format!("{event:?}"), expected);
        }
    }
}
