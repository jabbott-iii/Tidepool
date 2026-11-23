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

//! The error type for `tidepool-editor`.
//!
//! New in Tidepool.
//!
//! The [`shell`](crate::shell) returns `ui`'s [`UiError`] alone, as it did
//! before this type existed (M3.4). The [editor view](crate::view) can
//! also be given a position or a selection its text does not have, which is
//! an error, never a panic (ADR-0019), so it returns [`EditorError`],
//! which holds either.

use std::error::Error;
use std::fmt;

use tidepool_text::TextError;
use tidepool_ui::UiError;

/// Errors produced by `tidepool-editor`'s [editor view](crate::view).
///
/// A call that returns one changes nothing. Messages hold numbers and node
/// ids only, never any of the text.
///
/// Non-exhaustive: later versions may add variants.
#[derive(Debug)]
#[non_exhaustive]
pub enum EditorError {
    /// The tree refused a change, or no longer holds the view's nodes: it
    /// was removed, or the id came from another view.
    Ui(UiError),
    /// A byte offset or a selection the text does not have: past its end,
    /// or inside a character (ADR-0019).
    Text(TextError),
}

impl fmt::Display for EditorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ui(error) => error.fmt(f),
            Self::Text(error) => error.fmt(f),
        }
    }
}

impl Error for EditorError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Ui(error) => Some(error),
            Self::Text(error) => Some(error),
        }
    }
}

impl From<UiError> for EditorError {
    fn from(error: UiError) -> Self {
        Self::Ui(error)
    }
}

impl From<TextError> for EditorError {
    fn from(error: TextError) -> Self {
        Self::Text(error)
    }
}
