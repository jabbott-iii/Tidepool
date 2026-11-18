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

//! `Theme::parse` on the bundled file with random edits: it never panics,
//! and what it accepts keeps the format's rules (ADR-0047).

use tidepool_core::rng::Pcg32;
use tidepool_core::testing::check_sized;
use tidepool_ui::UiError;
use tidepool_ui::metrics::Family;
use tidepool_ui::theme::{LIMITS, MAX_CUT, MAX_TEXT_BYTES, TIDEPOOL_DARK_TOML, Theme, Token};

/// Text an edit may insert: TOML's punctuation, numbers, keys of the
/// format, and characters a name may not hold.
const PIECES: &[&str] = &[
    "=",
    "\"",
    "'",
    "[",
    "]",
    "[[",
    "{",
    "}",
    ",",
    ".",
    "\n",
    "#",
    " ",
    "\\",
    "0",
    "1",
    "-1",
    "255",
    "256",
    "64.5",
    "nan",
    "inf",
    "1e9",
    "true",
    "x",
    "bg",
    "solid",
    "ansi",
    "value",
    "\"#0b0c0d\"",
    "\"#0b0c0d80\"",
    "\"rgba(1,2,3,0.5)\"",
    "\"\"",
    "\u{202e}",
    "\u{2028}",
    "\u{1b}",
    "\u{2029}",
    "\u{2067}",
    "\u{a0}",
    "é",
    // Enough to take a name or a font past its 64 bytes.
    "nnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnn",
    "version",
    "colors",
    "contrast",
];

/// The format's rule for a name or a font, written out here rather than
/// taken from the reader: at most 64 bytes, not only white space, and no
/// control character, line or paragraph separator, or bidirectional
/// control.
fn is_plain_line(text: &str) -> bool {
    text.len() <= MAX_TEXT_BYTES
        && !text.chars().all(char::is_whitespace)
        && !text.chars().any(|c| {
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
        })
}

/// The bundled file with `edits` random edits, each at a character
/// boundary: a deletion, a duplication or an insertion.
fn mutate(rng: &mut Pcg32, edits: u32) -> String {
    let mut text: Vec<char> = TIDEPOOL_DARK_TOML.chars().collect();
    // A position or length from 0 to `max`; the file is a few thousand
    // characters, far inside `u32`.
    let upto = |rng: &mut Pcg32, max: usize| {
        let max = u32::try_from(max).unwrap_or(u32::MAX - 1);
        rng.range(0..max + 1) as usize
    };
    for _ in 0..edits.max(1) {
        let at = upto(rng, text.len());
        match rng.range(0..3u32) {
            0 => {
                let len = upto(rng, 16).min(text.len() - at);
                text.drain(at..at + len);
            }
            1 => {
                let len = upto(rng, 32).min(text.len() - at);
                let copy: Vec<char> = text[at..at + len].to_vec();
                text.splice(at..at, copy);
            }
            _ => {
                let piece = rng.choose(PIECES).copied().unwrap_or("=");
                text.splice(at..at, piece.chars());
            }
        }
    }
    text.into_iter().collect()
}

#[test]
fn mutated_files_never_panic_and_accepted_ones_keep_the_rules() {
    check_sized(
        "theme files with random edits",
        2000,
        |rng, size| mutate(rng, size / 10),
        |text| match Theme::parse(text, LIMITS) {
            Ok(theme) => {
                if theme.color(Token::Bg).a != u8::MAX {
                    return Err("a translucent bg was accepted".into());
                }
                if !(0.0..=MAX_CUT).contains(&theme.cut()) {
                    return Err(format!("cut {} was accepted", theme.cut()));
                }
                for text in [
                    theme.name(),
                    theme.font(Family::Ui),
                    theme.font(Family::Mono),
                ] {
                    if !is_plain_line(text) {
                        return Err(format!("{text:?} was accepted"));
                    }
                }
                for token in Token::ALL {
                    let entry = theme.token(token);
                    if entry.solid.is_some() && entry.value.a == u8::MAX {
                        return Err(format!("`{token}` is opaque and has a solid"));
                    }
                    if theme.solid(token).a != u8::MAX {
                        return Err(format!("`{token}`'s solid is translucent"));
                    }
                }
                Ok(())
            }
            Err(UiError::Theme(error)) => {
                // The message is built, and every line it names is in the text.
                let _ = error.to_string();
                match error.line {
                    Some(line) if line > text.matches('\n').count() + 1 => {
                        Err(format!("line {line} is past the end: {error}"))
                    }
                    _ => Ok(()),
                }
            }
            Err(UiError::ThemeSyntax(_)) => Ok(()),
            Err(other) => Err(format!("an unexpected error: {other}")),
        },
    );
}

#[test]
fn the_bundled_file_reads_within_its_limits() {
    let theme = Theme::parse(TIDEPOOL_DARK_TOML, LIMITS).expect("the bundled theme");
    assert_eq!(theme, Theme::tidepool_dark());
    assert!(TIDEPOOL_DARK_TOML.len() < LIMITS.max_input_bytes / 64);
}
