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
//! which holds either. The [command registry](crate::command) and the
//! [keymap](crate::keymap) reader return it too (M4.6).

use std::error::Error;
use std::fmt::{self, Write as _};

use tidepool_codec::CodecError;
use tidepool_text::TextError;
use tidepool_ui::UiError;

use crate::keymap::FileError;

/// Errors produced by `tidepool-editor`'s [editor view](crate::view),
/// [command registry](crate::command) and [keymap](crate::keymap) reader.
///
/// A call that returns one changes nothing. Messages hold numbers, node
/// ids, command ids and the names of a keymap's keys, never any of a
/// view's text. Ids and names may come from a file, so a message writes
/// their control characters, line and paragraph separators and
/// bidirectional controls as escapes (`\u{1b}`), and shows at most their
/// first 512 characters.
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
    /// A command id that is not one
    /// ([`command::is_valid_id`](crate::command::is_valid_id)).
    BadCommandId {
        /// The id, as given.
        id: String,
    },
    /// A command id the registry has already, or one of the keymap
    /// engine's, whose first segment is `mode`.
    TakenCommandId {
        /// The id.
        id: String,
    },
    /// A keymap file that is not TOML, or breaks the TOML limits of its
    /// [`keymap::Limits`](crate::keymap::Limits).
    KeymapSyntax(CodecError),
    /// A keymap file that is TOML but not a keymap of
    /// [`keymap::VERSION`](crate::keymap::VERSION).
    Keymap(FileError),
}

impl fmt::Display for EditorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ui(error) => error.fmt(f),
            Self::Text(error) => error.fmt(f),
            Self::BadCommandId { id } => write!(
                f,
                "`{}` is not a command id: one or more segments joined by `.`, each of ASCII \
                 letters and digits and starting with a lowercase letter, at most {} bytes",
                Shown(id),
                crate::command::MAX_ID_BYTES
            ),
            Self::TakenCommandId { id } => write!(
                f,
                "the command id `{}` is taken: by a command registered before, or by the \
                 keymap engine, whose ids start with `mode.`",
                Shown(id)
            ),
            Self::KeymapSyntax(error) => write!(f, "the keymap is not TOML: {error}"),
            Self::Keymap(error) => error.fmt(f),
        }
    }
}

impl Error for EditorError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Ui(error) => Some(error),
            Self::Text(error) => Some(error),
            Self::KeymapSyntax(error) => Some(error),
            Self::Keymap(error) => Some(error),
            Self::BadCommandId { .. } | Self::TakenCommandId { .. } => None,
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

/// The most characters of a text from outside, such as an id or a key's
/// name, that a message shows; [`Shown`] cuts the rest, with `…`.
pub(crate) const MAX_SHOWN_CHARS: usize = 512;

/// A text from outside, shown in a message: its control characters, line
/// and paragraph separators and bidirectional controls written as escapes
/// (`\u{1b}`), as `xtask` and the binary write them, so that a message
/// cannot move a terminal's cursor, change its colors or reorder its line;
/// and cut after [`MAX_SHOWN_CHARS`] characters, so that a long one cannot
/// flood the log.
pub(crate) struct Shown<'a>(pub(crate) &'a str);

impl fmt::Display for Shown<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (n, c) in self.0.chars().enumerate() {
            if n == MAX_SHOWN_CHARS {
                return f.write_char('…');
            }
            if is_escaped(c) {
                write!(f, "{}", c.escape_default())?;
            } else {
                f.write_char(c)?;
            }
        }
        Ok(())
    }
}

/// Whether [`Shown`] writes `c` as an escape: a control character, a line
/// or paragraph separator, or a bidirectional control.
pub(crate) fn is_escaped(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{2028}'
                | '\u{2029}'
                | '\u{061c}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shown_text_is_escaped_and_cut() {
        assert_eq!(Shown("plain ctrl+b").to_string(), "plain ctrl+b");
        assert_eq!(
            Shown("a\u{1b}[2J\n\u{2028}\u{202e}\u{2066}b").to_string(),
            "a\\u{1b}[2J\\n\\u{2028}\\u{202e}\\u{2066}b"
        );
        let long = "é".repeat(MAX_SHOWN_CHARS + 10);
        let shown = Shown(&long).to_string();
        assert_eq!(shown.chars().count(), MAX_SHOWN_CHARS + 1);
        assert!(shown.ends_with('…'));
        let exact = "x".repeat(MAX_SHOWN_CHARS);
        assert_eq!(Shown(&exact).to_string(), exact, "nothing to cut");
    }
}
