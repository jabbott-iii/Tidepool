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

//! The error type for `tidepool-gui`.
//!
//! New in Tidepool.

use std::fmt;

use tidepool_font::FontError;
use tidepool_gfx::GfxError;
use tidepool_platform::surface::PixelsError;

/// Errors produced by `tidepool-gui`: an error of a crate it draws or
/// presents with, kept whole as its [source](std::error::Error::source).
///
/// Non-exhaustive: later tasks may add variants.
#[derive(Debug)]
#[non_exhaustive]
pub enum GuiError {
    /// The window could not be opened, waited on or presented to.
    Platform(tidepool_platform::Error),
    /// A framebuffer, the backdrop or a glyph's mask could not be made.
    Gfx(GfxError),
    /// The bundled faces, the glyph atlas or a line of text failed.
    Font(FontError),
    /// The frame could not be viewed as the window's pixels.
    Pixels(PixelsError),
}

impl fmt::Display for GuiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Platform(error) => write!(f, "the window: {error}"),
            Self::Gfx(error) => write!(f, "drawing: {error}"),
            Self::Font(error) => write!(f, "setting text: {error}"),
            Self::Pixels(error) => write!(f, "presenting: {error}"),
        }
    }
}

impl std::error::Error for GuiError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Platform(error) => Some(error),
            Self::Gfx(error) => Some(error),
            Self::Font(error) => Some(error),
            Self::Pixels(error) => Some(error),
        }
    }
}

impl From<tidepool_platform::Error> for GuiError {
    fn from(error: tidepool_platform::Error) -> Self {
        Self::Platform(error)
    }
}

impl From<GfxError> for GuiError {
    fn from(error: GfxError) -> Self {
        Self::Gfx(error)
    }
}

impl From<FontError> for GuiError {
    fn from(error: FontError) -> Self {
        Self::Font(error)
    }
}

impl From<PixelsError> for GuiError {
    fn from(error: PixelsError) -> Self {
        Self::Pixels(error)
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use tidepool_core::geom::DeviceSize;

    use super::*;

    #[test]
    fn each_error_names_what_failed_and_keeps_its_source() {
        let gfx = GuiError::from(GfxError::NegativeSize {
            size: DeviceSize::new(-1, 2),
        });
        assert_eq!(
            gfx.to_string(),
            "drawing: a size of -1 by 2 pixels: a side is negative"
        );
        assert!(gfx.source().is_some());

        let pixels = GuiError::from(PixelsError::TooFewBytes { len: 3, needed: 4 });
        assert!(pixels.to_string().starts_with("presenting: "), "{pixels}");
        assert!(pixels.source().is_some());

        let platform = GuiError::from(tidepool_platform::Error::Unsupported {
            what: "window",
            os: "plan9",
        });
        assert!(
            platform.to_string().starts_with("the window: "),
            "{platform}"
        );
        assert!(platform.source().is_some());
    }
}
