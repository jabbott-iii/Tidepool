//! Input types shared by `platform`, `ui`, `editor` and both frontends:
//! [`Key`] (physical keys), [`Modifiers`], [`MouseButton`], [`ScrollDelta`]
//! and [`InputEvent`].
//!
//! Ported in part from nadir-core `keys.rs` (the `keys!` macro, [`Key`] and
//! [`Key::ALL`] unchanged; [`MouseButton`] reordered). [`Modifiers`],
//! [`ScrollDelta`] and [`InputEvent`] are new.
//!
//! A [`Key`] names a place on the keyboard, not the character printed
//! there: the key right of Tab is [`Key::Q`] on every layout, though an
//! AZERTY keyboard prints `A` on it. Text the user types is a separate
//! thing and arrives as [`InputEvent::Text`].

use std::fmt;
use std::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, Not, Sub, SubAssign};

use crate::geom::Point;

/// Declares [`Key`] and [`Key::ALL`] from one list, so that the two cannot
/// disagree.
macro_rules! keys {
    ($( $(#[$attr:meta])* $name:ident = $usage:literal, )+) => {
        /// A physical key.
        ///
        /// The variants are in the order of the USB HID usage tables
        /// (keyboard page, `0x07`), and the value of each is its usage id.
        /// Names follow the US layout.
        ///
        /// Not every keyboard has every key, and not every operating system
        /// reports every key. Each platform backend lists the ones it cannot
        /// report. Keys that only Japanese and Korean keyboards have are not
        /// here yet.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(u8)]
        pub enum Key {
            $( $(#[$attr])* $name = $usage, )+
        }

        impl Key {
            /// Every key, in order.
            pub const ALL: &'static [Key] = &[$( Key::$name, )+];
        }
    };
}

keys! {
    /// `A` on a US keyboard.
    A = 0x04,
    /// `B` on a US keyboard.
    B = 0x05,
    /// `C` on a US keyboard.
    C = 0x06,
    /// `D` on a US keyboard.
    D = 0x07,
    /// `E` on a US keyboard.
    E = 0x08,
    /// `F` on a US keyboard.
    F = 0x09,
    /// `G` on a US keyboard.
    G = 0x0A,
    /// `H` on a US keyboard.
    H = 0x0B,
    /// `I` on a US keyboard.
    I = 0x0C,
    /// `J` on a US keyboard.
    J = 0x0D,
    /// `K` on a US keyboard.
    K = 0x0E,
    /// `L` on a US keyboard.
    L = 0x0F,
    /// `M` on a US keyboard.
    M = 0x10,
    /// `N` on a US keyboard.
    N = 0x11,
    /// `O` on a US keyboard.
    O = 0x12,
    /// `P` on a US keyboard.
    P = 0x13,
    /// `Q` on a US keyboard.
    Q = 0x14,
    /// `R` on a US keyboard.
    R = 0x15,
    /// `S` on a US keyboard.
    S = 0x16,
    /// `T` on a US keyboard.
    T = 0x17,
    /// `U` on a US keyboard.
    U = 0x18,
    /// `V` on a US keyboard.
    V = 0x19,
    /// `W` on a US keyboard.
    W = 0x1A,
    /// `X` on a US keyboard.
    X = 0x1B,
    /// `Y` on a US keyboard.
    Y = 0x1C,
    /// `Z` on a US keyboard.
    Z = 0x1D,
    /// `1` in the top row.
    Digit1 = 0x1E,
    /// `2` in the top row.
    Digit2 = 0x1F,
    /// `3` in the top row.
    Digit3 = 0x20,
    /// `4` in the top row.
    Digit4 = 0x21,
    /// `5` in the top row.
    Digit5 = 0x22,
    /// `6` in the top row.
    Digit6 = 0x23,
    /// `7` in the top row.
    Digit7 = 0x24,
    /// `8` in the top row.
    Digit8 = 0x25,
    /// `9` in the top row.
    Digit9 = 0x26,
    /// `0` in the top row.
    Digit0 = 0x27,
    /// Enter, or Return, in the main block.
    Enter = 0x28,
    /// Escape.
    Escape = 0x29,
    /// Backspace.
    Backspace = 0x2A,
    /// Tab.
    Tab = 0x2B,
    /// The space bar.
    Space = 0x2C,
    /// `-` on a US keyboard.
    Minus = 0x2D,
    /// `=` on a US keyboard.
    Equal = 0x2E,
    /// `[` on a US keyboard.
    LeftBracket = 0x2F,
    /// `]` on a US keyboard.
    RightBracket = 0x30,
    /// `\` on a US keyboard. On an ISO keyboard, the key left of Enter.
    Backslash = 0x31,
    /// `;` on a US keyboard.
    Semicolon = 0x33,
    /// `'` on a US keyboard.
    Quote = 0x34,
    /// `` ` `` on a US keyboard, left of `1`.
    Grave = 0x35,
    /// `,` on a US keyboard.
    Comma = 0x36,
    /// `.` on a US keyboard.
    Period = 0x37,
    /// `/` on a US keyboard.
    Slash = 0x38,
    /// Caps Lock.
    CapsLock = 0x39,
    /// F1.
    F1 = 0x3A,
    /// F2.
    F2 = 0x3B,
    /// F3.
    F3 = 0x3C,
    /// F4.
    F4 = 0x3D,
    /// F5.
    F5 = 0x3E,
    /// F6.
    F6 = 0x3F,
    /// F7.
    F7 = 0x40,
    /// F8.
    F8 = 0x41,
    /// F9.
    F9 = 0x42,
    /// F10.
    F10 = 0x43,
    /// F11.
    F11 = 0x44,
    /// F12.
    F12 = 0x45,
    /// Print Screen.
    PrintScreen = 0x46,
    /// Scroll Lock.
    ScrollLock = 0x47,
    /// Pause.
    Pause = 0x48,
    /// Insert.
    Insert = 0x49,
    /// Home.
    Home = 0x4A,
    /// Page Up.
    PageUp = 0x4B,
    /// Delete, the forward one.
    Delete = 0x4C,
    /// End.
    End = 0x4D,
    /// Page Down.
    PageDown = 0x4E,
    /// The right arrow.
    Right = 0x4F,
    /// The left arrow.
    Left = 0x50,
    /// The down arrow.
    Down = 0x51,
    /// The up arrow.
    Up = 0x52,
    /// Num Lock. On an Apple keyboard, Clear.
    NumLock = 0x53,
    /// `/` on the numeric keypad.
    NumpadDivide = 0x54,
    /// `*` on the numeric keypad.
    NumpadMultiply = 0x55,
    /// `-` on the numeric keypad.
    NumpadSubtract = 0x56,
    /// `+` on the numeric keypad.
    NumpadAdd = 0x57,
    /// Enter on the numeric keypad.
    NumpadEnter = 0x58,
    /// `1` on the numeric keypad.
    Numpad1 = 0x59,
    /// `2` on the numeric keypad.
    Numpad2 = 0x5A,
    /// `3` on the numeric keypad.
    Numpad3 = 0x5B,
    /// `4` on the numeric keypad.
    Numpad4 = 0x5C,
    /// `5` on the numeric keypad.
    Numpad5 = 0x5D,
    /// `6` on the numeric keypad.
    Numpad6 = 0x5E,
    /// `7` on the numeric keypad.
    Numpad7 = 0x5F,
    /// `8` on the numeric keypad.
    Numpad8 = 0x60,
    /// `9` on the numeric keypad.
    Numpad9 = 0x61,
    /// `0` on the numeric keypad.
    Numpad0 = 0x62,
    /// `.` on the numeric keypad.
    NumpadDecimal = 0x63,
    /// The extra key of an ISO keyboard, between the left Shift and `Z`.
    IntlBackslash = 0x64,
    /// The menu key, also called Application.
    Menu = 0x65,
    /// `=` on the numeric keypad, as on an Apple keyboard.
    NumpadEqual = 0x67,
    /// F13.
    F13 = 0x68,
    /// F14.
    F14 = 0x69,
    /// F15.
    F15 = 0x6A,
    /// F16.
    F16 = 0x6B,
    /// F17.
    F17 = 0x6C,
    /// F18.
    F18 = 0x6D,
    /// F19.
    F19 = 0x6E,
    /// F20.
    F20 = 0x6F,
    /// F21.
    F21 = 0x70,
    /// F22.
    F22 = 0x71,
    /// F23.
    F23 = 0x72,
    /// F24.
    F24 = 0x73,
    /// The left Control.
    LeftControl = 0xE0,
    /// The left Shift.
    LeftShift = 0xE1,
    /// The left Alt, or Option.
    LeftAlt = 0xE2,
    /// The left Super: the Windows or Command key.
    LeftSuper = 0xE3,
    /// The right Control.
    RightControl = 0xE4,
    /// The right Shift.
    RightShift = 0xE5,
    /// The right Alt, Option or AltGr.
    RightAlt = 0xE6,
    /// The right Super: the Windows or Command key.
    RightSuper = 0xE7,
}

impl Key {
    /// The key's id in the USB HID usage tables, keyboard page.
    pub const fn usage(self) -> u8 {
        self as u8
    }
}

/// A mouse button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MouseButton {
    /// The main button, under the index finger of a right-handed user.
    Left,
    /// The middle button, which is often the wheel.
    Middle,
    /// The secondary button.
    Right,
    /// The first side button, which browsers use for "back".
    Back,
    /// The second side button, which browsers use for "forward".
    Forward,
}

impl MouseButton {
    /// Every button, in order.
    pub const ALL: &'static [MouseButton] = &[
        MouseButton::Left,
        MouseButton::Middle,
        MouseButton::Right,
        MouseButton::Back,
        MouseButton::Forward,
    ];
}

/// The modifier keys held during an input event: a set of [`SHIFT`],
/// [`CTRL`], [`ALT`] and [`SUPER`].
///
/// Left and right modifiers are not distinguished here; the [`Key`] of a
/// key event does that. On macOS, Command is [`SUPER`] and Option is
/// [`ALT`].
///
/// ```
/// use tidepool_core::input::Modifiers;
///
/// let held = Modifiers::CTRL | Modifiers::SHIFT;
/// assert!(held.contains(Modifiers::CTRL));
/// assert!(!held.contains(Modifiers::CTRL | Modifiers::ALT));
/// assert_eq!(format!("{held:?}"), "Modifiers(SHIFT | CTRL)");
/// ```
///
/// [`SHIFT`]: Modifiers::SHIFT
/// [`CTRL`]: Modifiers::CTRL
/// [`ALT`]: Modifiers::ALT
/// [`SUPER`]: Modifiers::SUPER
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Modifiers(u8);

impl Modifiers {
    /// No modifiers.
    pub const NONE: Self = Self(0);
    /// Shift.
    pub const SHIFT: Self = Self(1 << 0);
    /// Control.
    pub const CTRL: Self = Self(1 << 1);
    /// Alt; Option on macOS.
    pub const ALT: Self = Self(1 << 2);
    /// The Windows key; Command on macOS.
    pub const SUPER: Self = Self(1 << 3);
    /// All four modifiers.
    pub const ALL: Self = Self(0b1111);

    /// Names for `Debug`, in bit order.
    const NAMES: [(Self, &'static str); 4] = [
        (Self::SHIFT, "SHIFT"),
        (Self::CTRL, "CTRL"),
        (Self::ALT, "ALT"),
        (Self::SUPER, "SUPER"),
    ];

    /// Returns the raw bits.
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Returns the set with these bits, or `None` if any bit is not a
    /// modifier.
    pub const fn from_bits(bits: u8) -> Option<Self> {
        if bits & !Self::ALL.0 == 0 {
            Some(Self(bits))
        } else {
            None
        }
    }

    /// Returns the set with these bits, ignoring bits that are not
    /// modifiers.
    pub const fn from_bits_truncate(bits: u8) -> Self {
        Self(bits & Self::ALL.0)
    }

    /// Returns `true` if no modifier is held.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Returns `true` if every modifier in `other` is held.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Returns `true` if any modifier in `other` is held.
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// Returns the modifiers in either set.
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Returns the modifiers in both sets.
    pub const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// Returns the modifiers in `self` but not in `other`.
    pub const fn difference(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// Adds the modifiers in `other`.
    pub fn insert(&mut self, other: Self) {
        *self = self.union(other);
    }

    /// Removes the modifiers in `other`.
    pub fn remove(&mut self, other: Self) {
        *self = self.difference(other);
    }

    /// Adds the modifiers in `other` if `held`, otherwise removes them.
    pub fn set(&mut self, other: Self, held: bool) {
        if held {
            self.insert(other);
        } else {
            self.remove(other);
        }
    }
}

/// Lists the held modifiers by name, for example `Modifiers(SHIFT | CTRL)`,
/// or `Modifiers(NONE)`.
impl fmt::Debug for Modifiers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Modifiers(")?;
        let mut first = true;
        for (modifier, name) in Self::NAMES {
            if self.contains(modifier) {
                if !first {
                    f.write_str(" | ")?;
                }
                f.write_str(name)?;
                first = false;
            }
        }
        if first {
            f.write_str("NONE")?;
        }
        f.write_str(")")
    }
}

impl BitOr for Modifiers {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        self.union(other)
    }
}

impl BitOrAssign for Modifiers {
    fn bitor_assign(&mut self, other: Self) {
        self.insert(other);
    }
}

impl BitAnd for Modifiers {
    type Output = Self;

    fn bitand(self, other: Self) -> Self {
        self.intersection(other)
    }
}

impl BitAndAssign for Modifiers {
    fn bitand_assign(&mut self, other: Self) {
        *self = self.intersection(other);
    }
}

impl Sub for Modifiers {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        self.difference(other)
    }
}

impl SubAssign for Modifiers {
    fn sub_assign(&mut self, other: Self) {
        self.remove(other);
    }
}

/// The modifiers not in the set.
impl Not for Modifiers {
    type Output = Self;

    fn not(self) -> Self {
        Self::ALL.difference(self)
    }
}

/// How far a wheel or trackpad scrolled.
///
/// Positive `y` scrolls toward the end of the content (down); positive `x`
/// scrolls toward the right. The direction is after any OS "natural
/// scrolling" setting has been applied.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScrollDelta {
    /// Whole lines, or notches of a mouse wheel. May be fractional on
    /// high-resolution wheels.
    Lines {
        /// Horizontal lines.
        x: f32,
        /// Vertical lines.
        y: f32,
    },
    /// An exact distance in logical units, from a trackpad or a precise
    /// wheel.
    Logical {
        /// Horizontal distance.
        x: f32,
        /// Vertical distance.
        y: f32,
    },
}

/// One input event from a frontend.
///
/// Positions are in logical units (see [`geom`](crate::geom)), measured
/// from the top-left corner of the window's content area.
#[derive(Clone, Debug, PartialEq)]
pub enum InputEvent {
    /// A key went down, or auto-repeated while held.
    KeyDown {
        /// The physical key.
        key: Key,
        /// The modifiers held, including the key itself if it is one.
        modifiers: Modifiers,
        /// `true` for an auto-repeat of a key that was already down.
        repeat: bool,
    },
    /// A key went up.
    KeyUp {
        /// The physical key.
        key: Key,
        /// The modifiers still held.
        modifiers: Modifiers,
    },
    /// Text the user typed or composed (with an input method), after the
    /// keyboard layout was applied.
    Text(String),
    /// The pointer moved.
    MouseMove {
        /// Where it is now.
        position: Point,
        /// The modifiers held.
        modifiers: Modifiers,
    },
    /// A mouse button went down.
    MouseDown {
        /// The button.
        button: MouseButton,
        /// Where the pointer was.
        position: Point,
        /// The modifiers held.
        modifiers: Modifiers,
    },
    /// A mouse button went up.
    MouseUp {
        /// The button.
        button: MouseButton,
        /// Where the pointer was.
        position: Point,
        /// The modifiers held.
        modifiers: Modifiers,
    },
    /// The wheel or trackpad scrolled.
    Wheel {
        /// How far.
        delta: ScrollDelta,
        /// Where the pointer was.
        position: Point,
        /// The modifiers held.
        modifiers: Modifiers,
    },
    /// The window (or terminal) gained keyboard focus.
    FocusGained,
    /// The window (or terminal) lost keyboard focus.
    FocusLost,
}

impl InputEvent {
    /// The modifiers held during the event, or [`Modifiers::NONE`] for text
    /// and focus events.
    pub const fn modifiers(&self) -> Modifiers {
        match self {
            Self::KeyDown { modifiers, .. }
            | Self::KeyUp { modifiers, .. }
            | Self::MouseMove { modifiers, .. }
            | Self::MouseDown { modifiers, .. }
            | Self::MouseUp { modifiers, .. }
            | Self::Wheel { modifiers, .. } => *modifiers,
            Self::Text(_) | Self::FocusGained | Self::FocusLost => Modifiers::NONE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_in_usage_order_without_repeats() {
        assert_eq!(Key::ALL.len(), 118);
        for pair in Key::ALL.windows(2) {
            assert!(pair[0].usage() < pair[1].usage(), "{pair:?}");
            assert!(pair[0] < pair[1], "{pair:?}");
        }
    }

    #[test]
    fn usages_match_the_hid_usage_tables() {
        // Spot checks against "HID Usage Tables", section 10, at the start
        // and end of each run of keys.
        let expected = [
            (Key::A, 0x04),
            (Key::Q, 0x14),
            (Key::W, 0x1A),
            (Key::Z, 0x1D),
            (Key::Digit1, 0x1E),
            (Key::Digit9, 0x26),
            (Key::Digit0, 0x27),
            (Key::Enter, 0x28),
            (Key::Space, 0x2C),
            (Key::Backslash, 0x31),
            (Key::Semicolon, 0x33),
            (Key::Slash, 0x38),
            (Key::CapsLock, 0x39),
            (Key::F1, 0x3A),
            (Key::F12, 0x45),
            (Key::PrintScreen, 0x46),
            (Key::Delete, 0x4C),
            (Key::Right, 0x4F),
            (Key::Up, 0x52),
            (Key::NumLock, 0x53),
            (Key::NumpadEnter, 0x58),
            (Key::Numpad1, 0x59),
            (Key::Numpad9, 0x61),
            (Key::Numpad0, 0x62),
            (Key::NumpadDecimal, 0x63),
            (Key::IntlBackslash, 0x64),
            (Key::Menu, 0x65),
            (Key::NumpadEqual, 0x67),
            (Key::F13, 0x68),
            (Key::F24, 0x73),
            (Key::LeftControl, 0xE0),
            (Key::LeftSuper, 0xE3),
            (Key::RightControl, 0xE4),
            (Key::RightSuper, 0xE7),
        ];
        for (key, usage) in expected {
            assert_eq!(key.usage(), usage, "{key:?}");
        }
    }

    #[test]
    fn letters_and_function_keys_are_contiguous() {
        let run = |first: Key, last: Key| {
            Key::ALL
                .iter()
                .filter(|key| (first..=last).contains(key))
                .count()
        };
        assert_eq!(run(Key::A, Key::Z), 26);
        assert_eq!(Key::Z.usage() - Key::A.usage(), 25);
        assert_eq!(run(Key::Digit1, Key::Digit0), 10);
        assert_eq!(run(Key::F1, Key::F12), 12);
        assert_eq!(run(Key::F13, Key::F24), 12);
        assert_eq!(run(Key::Numpad1, Key::Numpad0), 10);
        assert_eq!(run(Key::LeftControl, Key::RightSuper), 8);
    }

    #[test]
    fn every_mouse_button_is_listed_once_in_order() {
        assert_eq!(
            MouseButton::ALL,
            [
                MouseButton::Left,
                MouseButton::Middle,
                MouseButton::Right,
                MouseButton::Back,
                MouseButton::Forward
            ]
        );
        for pair in MouseButton::ALL.windows(2) {
            assert!(pair[0] < pair[1], "{pair:?}");
        }
    }

    #[test]
    fn modifier_bits_are_distinct_and_cover_all() {
        let each = [
            Modifiers::SHIFT,
            Modifiers::CTRL,
            Modifiers::ALT,
            Modifiers::SUPER,
        ];
        let mut union = Modifiers::NONE;
        for modifier in each {
            assert_eq!(modifier.bits().count_ones(), 1, "{modifier:?}");
            assert!(!union.intersects(modifier), "{modifier:?} overlaps");
            union |= modifier;
        }
        assert_eq!(union, Modifiers::ALL);
        assert_eq!(Modifiers::default(), Modifiers::NONE);
        assert!(Modifiers::NONE.is_empty());
    }

    #[test]
    fn modifier_set_operations() {
        let mut held = Modifiers::CTRL | Modifiers::SHIFT;
        assert!(held.contains(Modifiers::CTRL));
        assert!(held.contains(Modifiers::NONE));
        assert!(!held.contains(Modifiers::CTRL | Modifiers::ALT));
        assert!(held.intersects(Modifiers::CTRL | Modifiers::ALT));
        assert_eq!(held & Modifiers::CTRL, Modifiers::CTRL);
        assert_eq!(held - Modifiers::CTRL, Modifiers::SHIFT);
        assert_eq!(!held, Modifiers::ALT | Modifiers::SUPER);
        assert_eq!(!Modifiers::ALL, Modifiers::NONE);

        held.insert(Modifiers::ALT);
        held.remove(Modifiers::SHIFT);
        assert_eq!(held, Modifiers::CTRL | Modifiers::ALT);
        held.set(Modifiers::SUPER, true);
        held.set(Modifiers::CTRL, false);
        assert_eq!(held, Modifiers::ALT | Modifiers::SUPER);
        held -= Modifiers::ALT;
        held &= Modifiers::SUPER | Modifiers::SHIFT;
        assert_eq!(held, Modifiers::SUPER);
    }

    #[test]
    fn modifiers_from_bits() {
        assert_eq!(
            Modifiers::from_bits(0b0101),
            Some(Modifiers::SHIFT | Modifiers::ALT)
        );
        assert_eq!(Modifiers::from_bits(0b1_0000), None);
        assert_eq!(Modifiers::from_bits_truncate(0b1111_0010), Modifiers::CTRL);
        for bits in 0..=u8::MAX {
            let truncated = Modifiers::from_bits_truncate(bits);
            assert_eq!(Modifiers::from_bits(truncated.bits()), Some(truncated));
        }
    }

    #[test]
    fn modifiers_debug_lists_names() {
        assert_eq!(format!("{:?}", Modifiers::NONE), "Modifiers(NONE)");
        assert_eq!(format!("{:?}", Modifiers::SUPER), "Modifiers(SUPER)");
        assert_eq!(
            format!("{:?}", Modifiers::ALL),
            "Modifiers(SHIFT | CTRL | ALT | SUPER)"
        );
    }

    #[test]
    fn events_report_their_modifiers() {
        let ctrl = Modifiers::CTRL;
        let at = Point::new(1.0, 2.0);
        let events = [
            InputEvent::KeyDown {
                key: Key::S,
                modifiers: ctrl,
                repeat: false,
            },
            InputEvent::KeyUp {
                key: Key::S,
                modifiers: ctrl,
            },
            InputEvent::MouseMove {
                position: at,
                modifiers: ctrl,
            },
            InputEvent::MouseDown {
                button: MouseButton::Left,
                position: at,
                modifiers: ctrl,
            },
            InputEvent::MouseUp {
                button: MouseButton::Left,
                position: at,
                modifiers: ctrl,
            },
            InputEvent::Wheel {
                delta: ScrollDelta::Lines { x: 0.0, y: 3.0 },
                position: at,
                modifiers: ctrl,
            },
        ];
        for event in &events {
            assert_eq!(event.modifiers(), ctrl, "{event:?}");
        }
        for event in [
            InputEvent::Text("é".to_string()),
            InputEvent::FocusGained,
            InputEvent::FocusLost,
        ] {
            assert_eq!(event.modifiers(), Modifiers::NONE, "{event:?}");
        }
    }
}
