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

//! The window: [`Window`], which [`Window::open`] makes from a
//! [`WindowConfig`], and the pointer shapes it shows, [`CursorShape`].
//!
//! New in Tidepool. The Linux backend ports parts of Nadir's X11 window;
//! `docs/03-platforms.md` → Linux says what it does on X11.
//!
//! A window's events and its waker are in [`events`](crate::events), and
//! drawing to it, [`Window::present`], in [`surface`](crate::surface).
//!
//! # Threads
//!
//! A [`Window`] is neither `Send` nor `Sync`: it is used on the thread that
//! opened it. All the windows of the process live on one thread: the first
//! [`Window::open`] makes its thread the window thread, for as long as any
//! window is open, and an open on another thread meanwhile fails with
//! [`Error::WrongThread`]. Once every window is dropped, any thread may
//! open the next. (On Linux, Xlib's calls in the process must not overlap;
//! this is how `tidepool-platform` makes sure of it.) Only the
//! [`Waker`](crate::events::Waker) crosses threads.
//!
//! ```compile_fail
//! fn send<T: Send>() {}
//! send::<tidepool_platform::window::Window>();
//! ```

use core::fmt;
use core::marker::PhantomData;

use tidepool_core::geom::{DeviceSize, Scale, Size};

use crate::{Error, backend};

/// How to open a window.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowConfig {
    /// The title the window manager shows.
    pub title: String,
    /// The size of the inside of the window, in logical units. The window
    /// is this times its [scale](Window::scale) in device pixels, rounded
    /// up, and at least 1 by 1; on X11, at most 32,767 by 32,767.
    pub size: Size,
}

impl Default for WindowConfig {
    /// "Tidepool", 1280 by 800 logical units.
    fn default() -> Self {
        Self {
            title: "Tidepool".to_string(),
            size: Size::new(1280.0, 800.0),
        }
    }
}

/// The shape of the pointer over the window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CursorShape {
    /// The arrow, for most of the window. A window opens with it.
    #[default]
    Arrow,
    /// The text beam, over editable text.
    Text,
    /// The pointing hand, over a link or a button.
    Hand,
    /// The left-right arrow, over a splitter between columns.
    ColumnResize,
    /// The up-down arrow, over a splitter between rows.
    RowResize,
}

impl CursorShape {
    /// Every shape, in order.
    pub const ALL: [CursorShape; 5] = [
        CursorShape::Arrow,
        CursorShape::Text,
        CursorShape::Hand,
        CursorShape::ColumnResize,
        CursorShape::RowResize,
    ];

    /// The shape's place in [`CursorShape::ALL`].
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// A window on the desktop, which the system's window manager decorates.
///
/// It shows nothing of its own but a black background until the first
/// [`present`](Window::present). It stays open until it is dropped: when
/// the user asks to close it, [`wait_events`](Window::wait_events) reports
/// [`Event::CloseRequested`](crate::events::Event::CloseRequested) and
/// leaves the choice to the caller.
///
/// See the [module documentation](self) for the thread it lives on.
pub struct Window {
    pub(crate) backend: backend::Window,
    /// Keeps the type `!Send` and `!Sync` on every system, whatever the
    /// backend holds.
    _not_send: PhantomData<*const ()>,
}

impl Window {
    /// Opens a window and shows it.
    ///
    /// On Linux this connects to the X server that `DISPLAY` names, which
    /// must be a local one over a Unix socket (`:N` or `unix:N`), loading
    /// `libX11.so.6` the first time; reads the scale from the `Xft.dpi`
    /// resource, once; and makes the window with a 24-bit TrueColor
    /// visual (`docs/03-platforms.md` → Linux). It waits for the server
    /// before it returns. It also reads files: the locale's Compose files,
    /// for typed text, and, where libX11 loads libXcursor for the pointer
    /// shapes, all made here, files and a cursor theme that any client of
    /// the display can name, which can make it block
    /// (`docs/03-platforms.md` → Input → Cursors).
    ///
    /// # Errors
    ///
    /// - [`Error::Unsupported`] on a system with no window yet (all but
    ///   Linux, until M11 and M12).
    /// - [`Error::WrongThread`] while a window is open on another thread.
    /// - On Linux: [`Error::Display`] if `DISPLAY` is not a local display;
    ///   [`Error::LibraryNotFound`] or [`Error::MissingSymbols`] if
    ///   `libX11.so.6` cannot be loaded or lacks a function;
    ///   [`Error::ConnectionFailed`] if the server cannot be reached;
    ///   [`Error::NoVisual`] if the screen has no 24-bit TrueColor visual
    ///   in BGRA8's byte order; [`Error::Refused`] if the server refused a
    ///   request of the opening; [`Error::CallFailed`] or [`Error::Io`]
    ///   if a call failed.
    pub fn open(config: &WindowConfig) -> Result<Self, Error> {
        Ok(Self {
            backend: backend::Window::open(config)?,
            _not_send: PhantomData,
        })
    }

    /// Sets the title the window manager shows.
    pub fn set_title(&mut self, title: &str) {
        self.backend.set_title(title);
    }

    /// The size of the inside of the window, in device pixels, as of the
    /// last resize that [`wait_events`](Window::wait_events) reported.
    pub fn size(&self) -> DeviceSize {
        self.backend.size()
    }

    /// The size of the inside of the window, in logical units: its
    /// [size](Window::size) over its [scale](Window::scale).
    pub fn logical_size(&self) -> Size {
        self.scale().to_logical(self.size())
    }

    /// The window's device pixels per logical unit.
    ///
    /// On Linux it comes from the `Xft.dpi` X resource over 96, read once
    /// when the window opens, and is 1 when the resource is not set or not
    /// a density between 24 and 1536.
    pub fn scale(&self) -> Scale {
        self.backend.scale()
    }

    /// Sets the shape of the pointer while it is over the window.
    ///
    /// Every shape's cursor was made by [`Window::open`], so this reads no
    /// file: on Linux it queues one `XDefineCursor` request.
    pub fn set_cursor(&mut self, shape: CursorShape) {
        self.backend.set_cursor(shape);
    }

    /// Whether the window has the keyboard focus, as of the last focus
    /// event that [`wait_events`](Window::wait_events) reported. A window
    /// opens without it.
    pub fn is_focused(&self) -> bool {
        self.backend.is_focused()
    }
}

impl fmt::Debug for Window {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Window")
            .field("size", &self.size())
            .field("scale", &self.scale())
            .field("focused", &self.is_focused())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_cursor_shape_is_listed_once_at_its_index() {
        for (index, shape) in CursorShape::ALL.into_iter().enumerate() {
            assert_eq!(shape.index(), index, "{shape:?}");
        }
        assert_eq!(CursorShape::default(), CursorShape::Arrow);
    }

    #[test]
    fn the_default_config_names_tidepool() {
        let config = WindowConfig::default();
        assert_eq!(config.title, "Tidepool");
        assert_eq!(config.size, Size::new(1280.0, 800.0));
    }
}
