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

//! The backend of a system that has no window yet: every system but Linux,
//! until macOS (M11.7, M11.8) and Windows (M12.1, M12.2) get theirs.
//!
//! New in Tidepool. Opening a window fails with [`Error::Unsupported`], so
//! no window, and no waker, ever exists here: both types are empty, and
//! their methods cannot be called. The user's and the tools' folders fail
//! the same way.

use core::convert::Infallible;
use std::path::PathBuf;
use std::time::Instant;

use tidepool_core::geom::{DeviceRect, DeviceSize, Scale};

use crate::Error;
use crate::clipboard::Selection;
use crate::events::Event;
use crate::paths::UserDirs;
use crate::surface::Pixels;
use crate::window::{CursorShape, WindowConfig};

/// The error for `what`, which this system does not have yet.
fn unsupported(what: &'static str) -> Error {
    Error::Unsupported {
        what,
        os: std::env::consts::OS,
    }
}

/// Fails: this system has no user folders yet.
pub(crate) fn user_dirs() -> Result<UserDirs, Error> {
    Err(unsupported("user folders"))
}

/// Fails: this system has no tool folders yet.
pub(crate) fn android_sdk() -> Result<PathBuf, Error> {
    Err(unsupported("tool folders"))
}

/// Fails: this system has no tool folders yet.
pub(crate) fn rustup_home() -> Result<PathBuf, Error> {
    Err(unsupported("tool folders"))
}

/// A window that cannot exist.
pub(crate) struct Window {
    never: Infallible,
}

impl Window {
    /// Fails: this system has no window yet.
    pub(crate) fn open(_config: &WindowConfig) -> Result<Self, Error> {
        Err(unsupported("window"))
    }

    pub(crate) fn set_title(&mut self, _title: &str) {
        match self.never {}
    }

    pub(crate) fn size(&self) -> DeviceSize {
        match self.never {}
    }

    pub(crate) fn scale(&self) -> Scale {
        match self.never {}
    }

    pub(crate) fn set_cursor(&mut self, _shape: CursorShape) {
        match self.never {}
    }

    pub(crate) fn is_focused(&self) -> bool {
        match self.never {}
    }

    pub(crate) fn wait_events(
        &mut self,
        _deadline: Option<Instant>,
        _events: &mut Vec<Event>,
    ) -> Result<(), Error> {
        match self.never {}
    }

    pub(crate) fn waker(&self) -> Waker {
        match self.never {}
    }

    pub(crate) fn present(
        &mut self,
        _pixels: &Pixels<'_>,
        _damage: &[DeviceRect],
    ) -> Result<(), Error> {
        match self.never {}
    }

    pub(crate) fn set_selection(&mut self, _selection: Selection, _text: &str) {
        match self.never {}
    }

    pub(crate) fn request_selection(&mut self, _selection: Selection) {
        match self.never {}
    }
}

/// A waker of a window that cannot exist.
#[derive(Clone)]
pub(crate) struct Waker {
    never: Infallible,
}

impl Waker {
    pub(crate) fn wake(&self) {
        match self.never {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_does_not_open_and_says_why() {
        let error = Window::open(&WindowConfig::default()).err();
        assert!(
            matches!(
                error,
                Some(Error::Unsupported { what: "window", os }) if os == std::env::consts::OS
            ),
            "{error:?}"
        );
    }

    #[test]
    fn the_folders_are_not_found_and_say_why() {
        let os = std::env::consts::OS;
        assert!(
            matches!(user_dirs(), Err(Error::Unsupported { what: "user folders", os: o }) if o == os)
        );
        for found in [android_sdk(), rustup_home()] {
            assert!(
                matches!(found, Err(Error::Unsupported { what: "tool folders", os: o }) if o == os),
                "{found:?}"
            );
        }
    }
}
