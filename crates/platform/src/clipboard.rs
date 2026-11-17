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

//! The clipboard: the system's [`Selection`]s of text, which a window sets
//! with [`Window::set_selection`] and asks for with
//! [`Window::request_selection`], whose answer comes as an
//! [`Event::Pasted`](crate::events::Event::Pasted).
//!
//! New in Tidepool; not ported (Nadir has no clipboard). ADR-0042 records
//! the design, and `docs/03-platforms.md` → Clipboard what it does on X11.
//!
//! # Asked, not waited for
//!
//! A selection's text is held by the program that set it, which answers
//! other programs from its own event loop. So a paste is a question to
//! that program, and the answer comes later: the main thread never waits
//! for another program (`AGENTS.md` rule 6), and a window can paste what
//! another window of the same process copied, on the same thread. The
//! window that set a selection answers for it inside
//! [`wait_events`](Window::wait_events), which reports nothing of that,
//! until another program sets the selection or the window is dropped.
//!
//! ```no_run
//! use tidepool_platform::Error;
//! use tidepool_platform::clipboard::Selection;
//! use tidepool_platform::events::Event;
//! use tidepool_platform::window::{Window, WindowConfig};
//!
//! fn main() -> Result<(), Error> {
//!     let mut window = Window::open(&WindowConfig::default())?;
//!     window.set_selection(Selection::Clipboard, "copied");
//!     window.request_selection(Selection::Clipboard);
//!     let mut events = Vec::new();
//!     loop {
//!         window.wait_events(None, &mut events)?;
//!         for event in events.drain(..) {
//!             if let Event::Pasted { text, .. } = event {
//!                 assert_eq!(text.as_deref(), Some("copied"));
//!                 return Ok(());
//!             }
//!         }
//!     }
//! }
//! ```

use crate::window::Window;

/// The most bytes of text a paste takes, 64 MiB, counted once bytes that
/// are not UTF-8 are replaced: a larger text pastes as nothing, so that
/// another program cannot make Tidepool hold as much as it likes
/// (ADR-0042).
pub const MAX_PASTE_BYTES: usize = 64 * 1024 * 1024;

/// A selection of text that programs share.
///
/// X11 has both. What [`Selection::Primary`] does on macOS and Windows,
/// which have one clipboard, is for M11.8 and M12.1 to decide.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Selection {
    /// The clipboard, which Copy sets and Paste reads: X11's `CLIPBOARD`.
    Clipboard,
    /// What the user selected last, which a middle click pastes on X11:
    /// `PRIMARY`.
    Primary,
}

impl Selection {
    /// Every selection, in order.
    pub const ALL: [Selection; 2] = [Selection::Clipboard, Selection::Primary];

    /// The selection's place in [`Selection::ALL`].
    pub const fn index(self) -> usize {
        self as usize
    }
}

impl Window {
    /// Makes `text` the selection's text, which other programs, and this
    /// window, then paste.
    ///
    /// The window keeps a copy, and answers for it inside
    /// [`wait_events`](Window::wait_events) until another program sets
    /// the selection or the window is dropped; nothing keeps the text
    /// after that, unless the desktop's clipboard manager took a copy
    /// first.
    ///
    /// On Linux the window takes the selection before this returns, at the
    /// time the X server gives it (ICCCM: a zero-length write to a
    /// property of its own), which it waits for: a round trip to the
    /// server, not to another program, unless one holds a server grab.
    /// It handles the events queued before the server's answer, however
    /// many another program caused, and the next `wait_events` reports
    /// them. Another program that took the selection
    /// later keeps it, and the log says so. The window
    /// answers `UTF8_STRING` with the text, by `INCR` in parts of at most
    /// 256 KiB, `TARGETS` and `TIMESTAMP`, and refuses every other target
    /// (ADR-0042).
    pub fn set_selection(&mut self, selection: Selection, text: &str) {
        self.backend.set_selection(selection, text);
    }

    /// Asks for the selection's text, which a later
    /// [`wait_events`](Window::wait_events) reports as
    /// [`Event::Pasted`](crate::events::Event::Pasted).
    ///
    /// It waits for nothing. A request for a selection that is already
    /// asked for joins it: one event answers both. After
    /// [`set_selection`](Window::set_selection) it gets the text just set,
    /// from this window or another.
    ///
    /// The text is `None` when no program holds the selection, the one
    /// that does refused, no answer came within 5 seconds (or 5 seconds
    /// passed between two parts of a large text, or a minute in all), the
    /// text was larger than [`MAX_PASTE_BYTES`], or it came in a form other
    /// than UTF-8 text. Bytes that are not UTF-8 come out as U+FFFD.
    ///
    /// On Linux it asks for `UTF8_STRING` at `CurrentTime`, into a
    /// property of the window's own that no earlier request's answer can
    /// still come to; a late refusal of an earlier request can still end
    /// it with `None` (ADR-0042).
    pub fn request_selection(&mut self, selection: Selection) {
        self.backend.request_selection(selection);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_selection_is_listed_once_at_its_index() {
        for (index, selection) in Selection::ALL.into_iter().enumerate() {
            assert_eq!(selection.index(), index, "{selection:?}");
        }
    }
}
