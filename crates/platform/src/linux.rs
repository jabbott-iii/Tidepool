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

//! The Linux backend: an X11 window through Xlib, which is loaded from
//! `libX11.so.6` with `dlopen` (ADR-0010). Wayland sessions reach it
//! through XWayland.
//!
//! Ported in part from nadir-platform's `linux` backend, as each module
//! says; `docs/03-platforms.md` → Linux describes what it does.
//!
//! - [`window`]: the window, its events and its presentation, which the
//!   public types wrap.
//! - [`selection`]: the window's clipboard: the selections it owns and the
//!   pastes it asks for (ADR-0042).
//! - [`connection`]: the connection to the X server, one safe method for
//!   each Xlib call, and the input method that types text.
//! - [`claim`]: the one thread that may call Xlib, and the X error handler.
//! - [`display`]: which `DISPLAY` values the window connects to (ADR-0039).
//! - [`input`]: from X events to [`Event`](crate::events::Event)s.
//! - [`keymap`]: from key codes to [`Key`](tidepool_core::input::Key)s.
//! - [`xresources`]: the scale, from `Xft.dpi`.
//! - [`waker`]: the self-pipe that wakes a wait.
//! - [`library`]: loading a shared library, and Xlib's table, once.
//! - [`xdg`]: the user's and the tools' folders, from the XDG variables and
//!   the tools' own, for [`paths`](crate::paths) and [`dirs`](crate::dirs).
//! - [`file`](mod@file): a file's metadata and a new file's owner, for
//!   [`file`](crate::file) (M4.10).
//!
//! Everything that needs no X server (the display's rules, the key map,
//! the modifiers, the translation of events, the resource lookup and the
//! file metadata) is tested on its own, always. The tests that open windows run only with
//! `TIDEPOOL_IT_X11=1` (ADR-0039), under `xvfb-run`.

mod claim;
mod connection;
mod display;
mod file;
mod input;
mod keymap;
mod library;
mod selection;
mod waker;
mod window;
mod xdg;
mod xresources;

pub(crate) use self::file::{file_metadata, new_file_owner};
pub(crate) use self::waker::Waker;
pub(crate) use self::window::Window;
pub(crate) use self::xdg::{android_sdk, rustup_home, user_dirs};
