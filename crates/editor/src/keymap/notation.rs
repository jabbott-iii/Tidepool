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

//! The keys of a binding, written in `ctrl+shift+p` notation and shown as
//! `Ctrl+Shift+P` (ADR-0064): [`Step`], [`Sequence`] and
//! [`NotationError`]. The [`keymap`](super) module documentation gives
//! the notation's rules.
//!
//! New in Tidepool.

use std::error::Error;
use std::fmt;

use tidepool_core::input::{Key, Modifiers};

use super::Limits;
use crate::error::Shown;

/// The modifiers' names, in the order a [`Step`] shows them: the name the
/// notation reads, lowercase, and the one it shows.
const MODIFIERS: [(Modifiers, &str, &str); 4] = [
    (Modifiers::CTRL, "ctrl", "Ctrl"),
    (Modifiers::SHIFT, "shift", "Shift"),
    (Modifiers::ALT, "alt", "Alt"),
    (Modifiers::SUPER, "super", "Super"),
];

/// The modifier keys' names, which no step may end with: `core::input`'s
/// names in lowercase.
const MODIFIER_KEYS: [&str; 8] = [
    "leftcontrol",
    "leftshift",
    "leftalt",
    "leftsuper",
    "rightcontrol",
    "rightshift",
    "rightalt",
    "rightsuper",
];

/// Every key a step may end with, in [`Key::ALL`]'s order: the key, the
/// name the notation reads (matched without regard to case), and the name
/// a [`Step`] shows. Letters and digits are named by themselves, the
/// punctuation by its unshifted US-layout character, and the rest by
/// [`Key`]'s variant's name, lowercase to read and as written to show.
/// The modifier keys are not here. These names are part of the keymap
/// format: a test holds them to [`Key`]'s, so a renamed variant is noticed.
const KEYS: [(Key, &str, &str); 110] = [
    (Key::A, "a", "A"),
    (Key::B, "b", "B"),
    (Key::C, "c", "C"),
    (Key::D, "d", "D"),
    (Key::E, "e", "E"),
    (Key::F, "f", "F"),
    (Key::G, "g", "G"),
    (Key::H, "h", "H"),
    (Key::I, "i", "I"),
    (Key::J, "j", "J"),
    (Key::K, "k", "K"),
    (Key::L, "l", "L"),
    (Key::M, "m", "M"),
    (Key::N, "n", "N"),
    (Key::O, "o", "O"),
    (Key::P, "p", "P"),
    (Key::Q, "q", "Q"),
    (Key::R, "r", "R"),
    (Key::S, "s", "S"),
    (Key::T, "t", "T"),
    (Key::U, "u", "U"),
    (Key::V, "v", "V"),
    (Key::W, "w", "W"),
    (Key::X, "x", "X"),
    (Key::Y, "y", "Y"),
    (Key::Z, "z", "Z"),
    (Key::Digit1, "1", "1"),
    (Key::Digit2, "2", "2"),
    (Key::Digit3, "3", "3"),
    (Key::Digit4, "4", "4"),
    (Key::Digit5, "5", "5"),
    (Key::Digit6, "6", "6"),
    (Key::Digit7, "7", "7"),
    (Key::Digit8, "8", "8"),
    (Key::Digit9, "9", "9"),
    (Key::Digit0, "0", "0"),
    (Key::Enter, "enter", "Enter"),
    (Key::Escape, "escape", "Escape"),
    (Key::Backspace, "backspace", "Backspace"),
    (Key::Tab, "tab", "Tab"),
    (Key::Space, "space", "Space"),
    (Key::Minus, "-", "-"),
    (Key::Equal, "=", "="),
    (Key::LeftBracket, "[", "["),
    (Key::RightBracket, "]", "]"),
    (Key::Backslash, "\\", "\\"),
    (Key::Semicolon, ";", ";"),
    (Key::Quote, "'", "'"),
    (Key::Grave, "`", "`"),
    (Key::Comma, ",", ","),
    (Key::Period, ".", "."),
    (Key::Slash, "/", "/"),
    (Key::CapsLock, "capslock", "CapsLock"),
    (Key::F1, "f1", "F1"),
    (Key::F2, "f2", "F2"),
    (Key::F3, "f3", "F3"),
    (Key::F4, "f4", "F4"),
    (Key::F5, "f5", "F5"),
    (Key::F6, "f6", "F6"),
    (Key::F7, "f7", "F7"),
    (Key::F8, "f8", "F8"),
    (Key::F9, "f9", "F9"),
    (Key::F10, "f10", "F10"),
    (Key::F11, "f11", "F11"),
    (Key::F12, "f12", "F12"),
    (Key::PrintScreen, "printscreen", "PrintScreen"),
    (Key::ScrollLock, "scrolllock", "ScrollLock"),
    (Key::Pause, "pause", "Pause"),
    (Key::Insert, "insert", "Insert"),
    (Key::Home, "home", "Home"),
    (Key::PageUp, "pageup", "PageUp"),
    (Key::Delete, "delete", "Delete"),
    (Key::End, "end", "End"),
    (Key::PageDown, "pagedown", "PageDown"),
    (Key::Right, "right", "Right"),
    (Key::Left, "left", "Left"),
    (Key::Down, "down", "Down"),
    (Key::Up, "up", "Up"),
    (Key::NumLock, "numlock", "NumLock"),
    (Key::NumpadDivide, "numpaddivide", "NumpadDivide"),
    (Key::NumpadMultiply, "numpadmultiply", "NumpadMultiply"),
    (Key::NumpadSubtract, "numpadsubtract", "NumpadSubtract"),
    (Key::NumpadAdd, "numpadadd", "NumpadAdd"),
    (Key::NumpadEnter, "numpadenter", "NumpadEnter"),
    (Key::Numpad1, "numpad1", "Numpad1"),
    (Key::Numpad2, "numpad2", "Numpad2"),
    (Key::Numpad3, "numpad3", "Numpad3"),
    (Key::Numpad4, "numpad4", "Numpad4"),
    (Key::Numpad5, "numpad5", "Numpad5"),
    (Key::Numpad6, "numpad6", "Numpad6"),
    (Key::Numpad7, "numpad7", "Numpad7"),
    (Key::Numpad8, "numpad8", "Numpad8"),
    (Key::Numpad9, "numpad9", "Numpad9"),
    (Key::Numpad0, "numpad0", "Numpad0"),
    (Key::NumpadDecimal, "numpaddecimal", "NumpadDecimal"),
    (Key::IntlBackslash, "intlbackslash", "IntlBackslash"),
    (Key::Menu, "menu", "Menu"),
    (Key::NumpadEqual, "numpadequal", "NumpadEqual"),
    (Key::F13, "f13", "F13"),
    (Key::F14, "f14", "F14"),
    (Key::F15, "f15", "F15"),
    (Key::F16, "f16", "F16"),
    (Key::F17, "f17", "F17"),
    (Key::F18, "f18", "F18"),
    (Key::F19, "f19", "F19"),
    (Key::F20, "f20", "F20"),
    (Key::F21, "f21", "F21"),
    (Key::F22, "f22", "F22"),
    (Key::F23, "f23", "F23"),
    (Key::F24, "f24", "F24"),
];

/// One step of a binding's keys: the modifiers held, exactly, and one key
/// that is not a modifier key, such as `Ctrl+Shift+P`.
///
/// A step matches a key going down with exactly its modifiers: `Ctrl+P`
/// does not match the P with Ctrl and Shift held.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Step {
    modifiers: Modifiers,
    key: Key,
}

impl Step {
    /// The step of `key` with `modifiers` held, or `None` if `key` is a
    /// modifier key (Shift, Control, Alt or Super, left or right), which
    /// ends no step.
    pub fn new(modifiers: Modifiers, key: Key) -> Option<Self> {
        KEYS.iter()
            .any(|&(known, _, _)| known == key)
            .then_some(Self { modifiers, key })
    }

    /// The modifiers it is pressed with.
    pub const fn modifiers(self) -> Modifiers {
        self.modifiers
    }

    /// Its key.
    pub const fn key(self) -> Key {
        self.key
    }

    /// Reads one step in the notation, such as `ctrl+shift+p`.
    ///
    /// ```
    /// use tidepool_core::input::{Key, Modifiers};
    /// use tidepool_editor::keymap::Step;
    ///
    /// let step = Step::parse("Shift+Ctrl+p")?;
    /// assert_eq!(step.modifiers(), Modifiers::CTRL | Modifiers::SHIFT);
    /// assert_eq!(step.key(), Key::P);
    /// assert_eq!(step.to_string(), "Ctrl+Shift+P");
    /// assert!(Step::parse("ctrl+shift").is_err(), "no key");
    /// # Ok::<(), tidepool_editor::keymap::NotationError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// A [`NotationError`] whose offset is a byte offset of `text`: what
    /// [`Sequence::parse`] refuses in a step.
    pub fn parse(text: &str) -> Result<Self, NotationError> {
        parse_step(text, 0)
    }
}

/// Shows the step as the keymap viewer and the palette would: its
/// modifiers in the order Ctrl, Shift, Alt, Super, then its key, joined by
/// `+`, with letters in capitals and named keys as `core::input` names
/// them (`Ctrl+Shift+P`, `Alt+PageUp`, `F1`).
impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (modifier, _, shown) in MODIFIERS {
            if self.modifiers.contains(modifier) {
                write!(f, "{shown}+")?;
            }
        }
        let shown = KEYS
            .iter()
            .find(|&&(key, _, _)| key == self.key)
            .map_or("?", |&(_, _, shown)| shown);
        f.write_str(shown)
    }
}

/// A binding's keys: one or more [`Step`]s, pressed one after another,
/// such as `Ctrl+K Ctrl+S`.
///
/// Sequences compare step by step, so a sequence comes just before every
/// longer one it starts.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Sequence(Vec<Step>);

impl Sequence {
    /// Reads keys in the notation: steps separated by one space, each
    /// modifiers and a key joined by `+`, as the [`keymap`](super) module
    /// documentation says. At most `limits.max_steps` steps are read; the
    /// text past them is not looked at.
    ///
    /// ```
    /// use tidepool_editor::keymap::{Limits, Sequence};
    ///
    /// let keys = Sequence::parse("ctrl+k ctrl+s", Limits::DEFAULT)?;
    /// assert_eq!(keys.steps().len(), 2);
    /// assert_eq!(keys.to_string(), "Ctrl+K Ctrl+S");
    /// assert_eq!(Sequence::parse(&keys.to_string(), Limits::DEFAULT)?, keys);
    /// assert!(Sequence::parse("ctrl+k  ctrl+s", Limits::DEFAULT).is_err());
    /// # Ok::<(), tidepool_editor::keymap::NotationError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// A [`NotationError`] at the byte offset of `text` where it goes
    /// wrong: an empty text, step or name; a name that is not a modifier
    /// or key; a modifier twice in one step; a step without a key, with a
    /// key before its last name, or ending with a modifier key; or more
    /// steps than the limit.
    pub fn parse(text: &str, limits: Limits) -> Result<Self, NotationError> {
        if text.is_empty() {
            return Err(NotationError::new(0, NotationErrorKind::Empty));
        }
        let mut steps = Vec::new();
        let mut offset = 0;
        for part in text.split(' ') {
            if steps.len() >= limits.max_steps {
                return Err(NotationError::new(
                    offset,
                    NotationErrorKind::TooManySteps {
                        max: limits.max_steps,
                    },
                ));
            }
            if part.is_empty() {
                return Err(NotationError::new(offset, NotationErrorKind::EmptyStep));
            }
            steps.push(parse_step(part, offset)?);
            offset += part.len() + 1;
        }
        Ok(Self(steps))
    }

    /// Its steps, in order: never empty.
    pub fn steps(&self) -> &[Step] {
        &self.0
    }

    /// Whether `self` starts with all of `other`'s steps, and has more.
    pub fn is_extension_of(&self, other: &Self) -> bool {
        self.0.len() > other.0.len() && self.0.starts_with(&other.0)
    }
}

/// Shows the steps as [`Step`] does, separated by a space: the form
/// [`Sequence::parse`] reads back to the same sequence.
impl fmt::Display for Sequence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (n, step) in self.0.iter().enumerate() {
            if n > 0 {
                f.write_str(" ")?;
            }
            step.fmt(f)?;
        }
        Ok(())
    }
}

/// Keys that do not read in the notation: where, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotationError {
    /// The byte offset in the text of the step or name that is wrong.
    pub offset: usize,
    /// What is wrong.
    pub kind: NotationErrorKind,
}

impl NotationError {
    const fn new(offset: usize, kind: NotationErrorKind) -> Self {
        Self { offset, kind }
    }
}

/// What is wrong with keys that do not read ([`NotationError`]).
///
/// Non-exhaustive: later versions may add variants.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum NotationErrorKind {
    /// The keys are empty.
    Empty,
    /// A step is empty: the keys start or end with a space, or have two
    /// together.
    EmptyStep,
    /// A name is empty: a step starts or ends with `+`, or has two
    /// together.
    EmptyName,
    /// A name that is neither a modifier nor a key, as written.
    UnknownName(String),
    /// A modifier named twice in one step: its name, lowercase.
    RepeatedModifier(&'static str),
    /// A step's last name is a modifier, so it has no key (`ctrl+shift`).
    MissingKey,
    /// A key before a step's last name, as written (`p+ctrl`, `a+b`): a
    /// step has one key, after its modifiers.
    KeyNotLast(String),
    /// A modifier key, such as `leftshift`, as written: no step ends with
    /// one, so a binding names the modifier instead (`shift+…`).
    ModifierKey(String),
    /// More steps than the limit.
    TooManySteps {
        /// The most steps a binding may have.
        max: usize,
    },
}

impl fmt::Display for NotationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let at = self.offset;
        match &self.kind {
            NotationErrorKind::Empty => f.write_str("no keys are given"),
            NotationErrorKind::EmptyStep => write!(
                f,
                "an empty step at byte {at}: steps are separated by one space"
            ),
            NotationErrorKind::EmptyName => write!(
                f,
                "an empty name at byte {at}: a step's modifiers and key are joined by one `+`"
            ),
            NotationErrorKind::UnknownName(name) => write!(
                f,
                "`{}` at byte {at} is not a modifier (`ctrl`, `shift`, `alt`, `super`) or a \
                 key's name",
                Shown(name)
            ),
            NotationErrorKind::RepeatedModifier(name) => {
                write!(f, "`{name}` is named twice in the step at byte {at}")
            }
            NotationErrorKind::MissingKey => {
                write!(f, "the step at byte {at} ends with a modifier, not a key")
            }
            NotationErrorKind::KeyNotLast(name) => write!(
                f,
                "`{}` at byte {at} is a key, but a step has one key, after its modifiers",
                Shown(name)
            ),
            NotationErrorKind::ModifierKey(name) => write!(
                f,
                "`{}` at byte {at} is a modifier key, which ends no step: name the modifier \
                 instead",
                Shown(name)
            ),
            NotationErrorKind::TooManySteps { max } => {
                write!(f, "more than {max} steps, from byte {at}")
            }
        }
    }
}

impl Error for NotationError {}

/// Reads one step, `text`, which starts at byte `base` of the keys.
fn parse_step(text: &str, base: usize) -> Result<Step, NotationError> {
    let mut modifiers = Modifiers::NONE;
    let mut offset = base;
    let mut names = text.split('+').peekable();
    while let Some(name) = names.next() {
        let at = offset;
        offset += name.len() + 1;
        if name.is_empty() {
            return Err(NotationError::new(at, NotationErrorKind::EmptyName));
        }
        let last = names.peek().is_none();
        if let Some(&(modifier, lower, _)) = MODIFIERS
            .iter()
            .find(|(_, lower, _)| name.eq_ignore_ascii_case(lower))
        {
            if last {
                return Err(NotationError::new(at, NotationErrorKind::MissingKey));
            }
            if modifiers.contains(modifier) {
                return Err(NotationError::new(
                    at,
                    NotationErrorKind::RepeatedModifier(lower),
                ));
            }
            modifiers.insert(modifier);
            continue;
        }
        if MODIFIER_KEYS
            .iter()
            .any(|known| name.eq_ignore_ascii_case(known))
        {
            return Err(NotationError::new(
                at,
                NotationErrorKind::ModifierKey(name.to_string()),
            ));
        }
        let Some(&(key, _, _)) = KEYS
            .iter()
            .find(|(_, known, _)| name.eq_ignore_ascii_case(known))
        else {
            return Err(NotationError::new(
                at,
                NotationErrorKind::UnknownName(name.to_string()),
            ));
        };
        if !last {
            return Err(NotationError::new(
                at,
                NotationErrorKind::KeyNotLast(name.to_string()),
            ));
        }
        return Ok(Step { modifiers, key });
    }
    // `split` gives at least one name, and the last one returns above.
    Err(NotationError::new(base, NotationErrorKind::EmptyName))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use tidepool_core::rng::Pcg32;
    use tidepool_core::testing::check;

    use super::*;

    fn keys(text: &str) -> Result<Sequence, NotationError> {
        Sequence::parse(text, Limits::DEFAULT)
    }

    fn kind(text: &str) -> NotationErrorKind {
        keys(text).expect_err(text).kind
    }

    fn step(modifiers: Modifiers, key: Key) -> Step {
        Step::new(modifiers, key).expect("not a modifier key")
    }

    #[test]
    fn the_key_table_holds_every_key_but_the_modifier_keys_once() {
        let modifier_keys = [
            Key::LeftControl,
            Key::LeftShift,
            Key::LeftAlt,
            Key::LeftSuper,
            Key::RightControl,
            Key::RightShift,
            Key::RightAlt,
            Key::RightSuper,
        ];
        let listed: Vec<Key> = KEYS.iter().map(|&(key, _, _)| key).collect();
        let expected: Vec<Key> = Key::ALL
            .iter()
            .copied()
            .filter(|key| !modifier_keys.contains(key))
            .collect();
        assert_eq!(listed, expected, "Key::ALL's order, without the modifiers");
        for key in modifier_keys {
            assert_eq!(Step::new(Modifiers::NONE, key), None, "{key:?}");
            assert!(MODIFIER_KEYS.contains(&format!("{key:?}").to_ascii_lowercase().as_str()));
        }
        let mut names = HashSet::new();
        for (key, name, shown) in KEYS {
            assert!(names.insert(name), "{name} twice");
            assert_eq!(name, name.to_ascii_lowercase(), "{key:?}");
            assert!(name.eq_ignore_ascii_case(shown), "{key:?}");
            assert!(!name.contains(['+', ' ']), "{key:?}");
            assert!(MODIFIERS.iter().all(|(_, m, _)| *m != name));
            let debug = format!("{key:?}");
            if name.len() > 1 {
                // A named key: `Key`'s own name.
                assert_eq!(shown, debug, "{key:?}");
            } else if let Some(digit) = debug.strip_prefix("Digit") {
                assert_eq!(name, digit);
            } else if debug.len() == 1 {
                assert_eq!(shown, debug, "a letter, in capitals");
            }
        }
        // The punctuation, by its unshifted US character.
        for (key, name) in [
            (Key::Minus, "-"),
            (Key::Equal, "="),
            (Key::LeftBracket, "["),
            (Key::RightBracket, "]"),
            (Key::Backslash, "\\"),
            (Key::Semicolon, ";"),
            (Key::Quote, "'"),
            (Key::Grave, "`"),
            (Key::Comma, ","),
            (Key::Period, "."),
            (Key::Slash, "/"),
        ] {
            assert_eq!(keys(name).map(|k| k.steps()[0].key()), Ok(key), "{name}");
        }
    }

    #[test]
    fn steps_read_modifiers_in_any_order_and_case_and_show_one_form() {
        let ctrl_shift_p = step(Modifiers::CTRL | Modifiers::SHIFT, Key::P);
        for text in [
            "ctrl+shift+p",
            "shift+ctrl+p",
            "CTRL+SHIFT+P",
            "Ctrl+Shift+P",
            "cTrL+sHiFt+p",
        ] {
            assert_eq!(Step::parse(text), Ok(ctrl_shift_p), "{text}");
        }
        assert_eq!(ctrl_shift_p.to_string(), "Ctrl+Shift+P");
        let all = step(Modifiers::ALL, Key::PageUp);
        assert_eq!(Step::parse("super+alt+shift+ctrl+pageup"), Ok(all));
        assert_eq!(all.to_string(), "Ctrl+Shift+Alt+Super+PageUp");
        assert_eq!(Step::parse("f1").map(|s| s.to_string()), Ok("F1".into()));
        assert_eq!(
            Step::parse("ESCAPE").map(|s| s.to_string()),
            Ok("Escape".into())
        );
        assert_eq!(
            Step::parse("alt+numpad0").map(|s| s.to_string()),
            Ok("Alt+Numpad0".into())
        );
        assert_eq!(
            Step::parse("ctrl+=").map(|s| (s.key(), s.to_string())),
            Ok((Key::Equal, "Ctrl+=".into()))
        );
        assert_eq!(Step::parse("ctrl+\\").map(|s| s.key()), Ok(Key::Backslash));
        assert_eq!(Step::parse("1").map(|s| s.key()), Ok(Key::Digit1));
        assert_eq!(Step::parse("space").map(|s| s.key()), Ok(Key::Space));
    }

    #[test]
    fn sequences_are_steps_separated_by_one_space() {
        let save = keys("ctrl+k ctrl+s").expect("reads");
        assert_eq!(
            save.steps(),
            [step(Modifiers::CTRL, Key::K), step(Modifiers::CTRL, Key::S)]
        );
        assert_eq!(save.to_string(), "Ctrl+K Ctrl+S");
        let prefix = keys("ctrl+k").expect("reads");
        assert!(save.is_extension_of(&prefix));
        assert!(!prefix.is_extension_of(&save) && !save.is_extension_of(&save));
        assert!(prefix < save, "a prefix sorts first");
        let eight = ["g"; 8].join(" ");
        assert_eq!(keys(&eight).map(|k| k.steps().len()), Ok(8));
    }

    #[test]
    fn every_kind_of_error_is_reported_with_its_offset() {
        assert_eq!(kind(""), NotationErrorKind::Empty);
        for (text, offset) in [(" a", 0), ("a ", 2), ("a  b", 2)] {
            let error = keys(text).expect_err(text);
            assert_eq!(
                (error.kind, error.offset),
                (NotationErrorKind::EmptyStep, offset),
                "{text:?}"
            );
        }
        for (text, offset) in [("+a", 0), ("ctrl+", 5), ("ctrl++a", 5), ("a ctrl+", 7)] {
            let error = keys(text).expect_err(text);
            assert_eq!(
                (error.kind, error.offset),
                (NotationErrorKind::EmptyName, offset),
                "{text:?}"
            );
        }
        let error = keys("ctrl+k ctrl+q?").expect_err("unknown");
        assert_eq!(
            (error.kind, error.offset),
            (NotationErrorKind::UnknownName("q?".into()), 12)
        );
        for text in ["plus", "ctrl+control+a", "é", "ctrl+P1", "digit1", "+"] {
            assert!(
                matches!(
                    kind(text),
                    NotationErrorKind::UnknownName(_) | NotationErrorKind::EmptyName
                ),
                "{text}"
            );
        }
        assert_eq!(
            kind("ctrl+shift+CTRL+p"),
            NotationErrorKind::RepeatedModifier("ctrl")
        );
        assert_eq!(kind("ctrl+shift"), NotationErrorKind::MissingKey);
        assert_eq!(kind("alt"), NotationErrorKind::MissingKey);
        assert_eq!(kind("p+ctrl"), NotationErrorKind::KeyNotLast("p".into()));
        assert_eq!(kind("a+b"), NotationErrorKind::KeyNotLast("a".into()));
        for name in ["leftshift", "RightControl", "leftalt", "rightsuper"] {
            assert_eq!(
                kind(&format!("ctrl+{name}")),
                NotationErrorKind::ModifierKey(name.into())
            );
        }
        let nine = ["g"; 9].join(" ");
        let error = keys(&nine).expect_err("nine steps");
        assert_eq!(
            (error.kind, error.offset),
            (NotationErrorKind::TooManySteps { max: 8 }, 16)
        );
        let few = Limits {
            max_steps: 1,
            ..Limits::DEFAULT
        };
        assert!(Sequence::parse("a", few).is_ok());
        assert!(Sequence::parse("a b", few).is_err());
        // The text past the limit is not looked at.
        let tail = format!("{} {}", ["a"; 8].join(" "), "x".repeat(1 << 20));
        assert_eq!(
            keys(&tail).map_err(|e| e.kind),
            Err(NotationErrorKind::TooManySteps { max: 8 })
        );
    }

    #[test]
    fn errors_show_names_escaped() {
        let message = keys("ctrl+\u{1b}[31m").expect_err("unknown").to_string();
        assert!(message.contains("`\\u{1b}[31m` at byte 5"), "{message}");
        assert!(!message.contains('\u{1b}'));
    }

    /// A random step, of any key a step may end with and any modifiers.
    fn random_step(rng: &mut Pcg32) -> Step {
        let &(key, _, _) = rng.choose(&KEYS).expect("not empty");
        let bits = u8::try_from(rng.range(0..16_u32)).unwrap_or(0);
        step(Modifiers::from_bits_truncate(bits), key)
    }

    /// The text of `steps` in a random spelling: modifiers in any order, and
    /// every name in any case.
    fn spelled(rng: &mut Pcg32, steps: &[Step]) -> String {
        let mut text = String::new();
        for (n, step) in steps.iter().enumerate() {
            if n > 0 {
                text.push(' ');
            }
            let mut names: Vec<&str> = MODIFIERS
                .iter()
                .filter(|(m, _, _)| step.modifiers().contains(*m))
                .map(|(_, lower, _)| *lower)
                .collect();
            rng.shuffle(&mut names);
            let key = KEYS
                .iter()
                .find(|(k, _, _)| *k == step.key())
                .map_or("", |(_, lower, _)| *lower);
            names.push(key);
            let joined = names.join("+");
            for c in joined.chars() {
                if rng.range(0..2_u32) == 0 {
                    text.push(c.to_ascii_uppercase());
                } else {
                    text.push(c);
                }
            }
        }
        text
    }

    #[test]
    fn any_spelling_reads_and_shows_one_form_that_reads_back() {
        check(
            "keys in any spelling read, and their shown form reads back the same",
            500,
            |rng| {
                let count = usize::try_from(rng.range(1..9_u32)).unwrap_or(1);
                let steps: Vec<Step> = (0..count).map(|_| random_step(rng)).collect();
                let text = spelled(rng, &steps);
                (steps, text)
            },
            |(steps, text)| {
                let read = keys(text).map_err(|e| format!("{text:?}: {e}"))?;
                if read.steps() != steps.as_slice() {
                    return Err(format!("{text:?} read as {read:?}"));
                }
                let shown = read.to_string();
                let again = keys(&shown).map_err(|e| format!("{shown:?}: {e}"))?;
                if again != read || again.to_string() != shown {
                    return Err(format!("{shown:?} read back as {again:?}"));
                }
                Ok(())
            },
        );
    }
}
