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

//! Colors: [`Color`] (8-bit sRGB, straight alpha), [`PremultipliedColor`]
//! and source-over blending, and the color syntax theme files use.
//!
//! Ported in part from nadir-core `color.rs` (the `Color` type, constants
//! and hex digits). Adapted for Tidepool: the parser accepts `rgba(…)` as
//! well as hex, is length-bounded and reports byte offsets, and blending is
//! integer source-over on premultiplied colors, as
//! `docs/subsystems/rendering.md` specifies for `gfx`. Nadir's
//! linear-light `LinearColor` and its sRGB transfer functions are not
//! ported: nothing in Tidepool's design calls for them yet.
//!
//! # Syntax
//!
//! [`Color::parse`] accepts, with optional ASCII whitespace around the whole
//! and between tokens:
//!
//! - `#RRGGBB` and `#RRGGBBAA`: hex digits in either case;
//! - `rgba(R, G, B, A)`: `R`, `G` and `B` are integers 0–255, `A` is a
//!   decimal 0–1 with at most 9 decimal places (`1`, `0.78`, `.5`), and
//!   becomes `round(A × 255)`, halves rounding up. The name is
//!   case-insensitive.
//!
//! Input longer than [`MAX_COLOR_LEN`] bytes is rejected before parsing.
//! Parsing never panics; every error carries the byte offset where it
//! failed and what was expected there.

use std::fmt;
use std::str::FromStr;

use crate::error::CoreError;

/// The longest color string [`Color::parse`] accepts, in bytes.
pub const MAX_COLOR_LEN: usize = 64;

/// An sRGB color with 8 bits per channel and straight (not premultiplied)
/// alpha. This is how colors are written in theme files and stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Color {
    /// Red, sRGB-encoded.
    pub r: u8,
    /// Green, sRGB-encoded.
    pub g: u8,
    /// Blue, sRGB-encoded.
    pub b: u8,
    /// Alpha: 0 is transparent, 255 is opaque.
    pub a: u8,
}

impl Color {
    /// Opaque white.
    pub const WHITE: Self = Self::rgb(255, 255, 255);
    /// Opaque black.
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    /// Fully transparent black.
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);

    /// Creates an opaque color.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgba(r, g, b, 255)
    }

    /// Creates a color with alpha.
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Parses a color in the syntax described in the
    /// [module documentation](self): `#5ccbcb`, `#5ccbcb80` or
    /// `rgba(16, 19, 21, 0.78)`.
    ///
    /// ```
    /// use tidepool_core::color::Color;
    ///
    /// assert_eq!(Color::parse("#5ccbcb"), Ok(Color::rgb(0x5c, 0xcb, 0xcb)));
    /// assert_eq!(Color::parse("rgba(16,19,21,0.78)"), Ok(Color::rgba(16, 19, 21, 199)));
    /// ```
    ///
    /// # Errors
    ///
    /// [`CoreError::InvalidColor`] with the byte offset and what was
    /// expected there.
    pub fn parse(text: &str) -> Result<Self, CoreError> {
        if text.len() > MAX_COLOR_LEN {
            return Err(CoreError::InvalidColor {
                offset: MAX_COLOR_LEN,
                expected: "at most 64 bytes",
            });
        }
        let mut parser = Parser {
            bytes: text.as_bytes(),
            pos: 0,
        };
        parser.skip_whitespace();
        let color = match parser.peek() {
            Some(b'#') => parser.hex()?,
            Some(b'r' | b'R') => parser.rgba()?,
            _ => return Err(parser.error("`#` or `rgba(`")),
        };
        parser.skip_whitespace();
        if parser.peek().is_some() {
            return Err(parser.error("the end of the color"));
        }
        Ok(color)
    }

    /// Converts to premultiplied alpha, rounding each channel to nearest.
    pub const fn premultiply(self) -> PremultipliedColor {
        PremultipliedColor {
            r: mul_div_255(self.r, self.a),
            g: mul_div_255(self.g, self.a),
            b: mul_div_255(self.b, self.a),
            a: self.a,
        }
    }
}

impl FromStr for Color {
    type Err = CoreError;

    fn from_str(text: &str) -> Result<Self, CoreError> {
        Self::parse(text)
    }
}

/// Formats as lowercase `#rrggbb` when opaque and `#rrggbbaa` otherwise,
/// which [`Color::parse`] reads back to the same color.
impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { r, g, b, a } = *self;
        write!(f, "#{r:02x}{g:02x}{b:02x}")?;
        if a != u8::MAX {
            write!(f, "{a:02x}")?;
        }
        Ok(())
    }
}

/// An sRGB color with premultiplied alpha: each color channel is already
/// multiplied by alpha, so it should not exceed `a`.
///
/// Blending happens in this form. Operations saturate rather than overflow
/// if a channel does exceed `a`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PremultipliedColor {
    /// Red times alpha.
    pub r: u8,
    /// Green times alpha.
    pub g: u8,
    /// Blue times alpha.
    pub b: u8,
    /// Alpha: 0 is transparent, 255 is opaque.
    pub a: u8,
}

impl PremultipliedColor {
    /// Fully transparent.
    pub const TRANSPARENT: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };

    /// Converts back to straight alpha, rounding to nearest. A fully
    /// transparent color becomes [`Color::TRANSPARENT`].
    pub const fn unpremultiply(self) -> Color {
        if self.a == 0 {
            return Color::TRANSPARENT;
        }
        Color {
            r: div_alpha(self.r, self.a),
            g: div_alpha(self.g, self.a),
            b: div_alpha(self.b, self.a),
            a: self.a,
        }
    }

    /// Composites `self` over `dst` (Porter–Duff source-over):
    /// `self + dst × (1 − self.a)`, per channel, rounding to nearest.
    pub const fn over(self, dst: Self) -> Self {
        let keep = u8::MAX - self.a;
        Self {
            r: self.r.saturating_add(mul_div_255(dst.r, keep)),
            g: self.g.saturating_add(mul_div_255(dst.g, keep)),
            b: self.b.saturating_add(mul_div_255(dst.b, keep)),
            a: self.a.saturating_add(mul_div_255(dst.a, keep)),
        }
    }
}

/// `round(x × y / 255)`, exactly. Because 255 is odd, `x × y / 255` is never
/// exactly halfway between two integers, so adding 127 before dividing
/// rounds to nearest.
const fn mul_div_255(x: u8, y: u8) -> u8 {
    // At most (255 × 255 + 127) / 255 = 255.
    ((x as u32 * y as u32 + 127) / 255) as u8
}

/// `round(channel × 255 / alpha)`, clamped to 255. `alpha` is not zero.
const fn div_alpha(channel: u8, alpha: u8) -> u8 {
    let value = (channel as u32 * 255 + alpha as u32 / 2) / alpha as u32;
    if value > 255 { 255 } else { value as u8 }
}

/// A cursor over the bytes of a color string.
struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn error(&self, expected: &'static str) -> CoreError {
        CoreError::InvalidColor {
            offset: self.pos,
            expected,
        }
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
            self.pos += 1;
        }
    }

    /// Consumes `byte`, or fails with `expected`.
    fn expect(&mut self, byte: u8, expected: &'static str) -> Result<(), CoreError> {
        if self.peek() == Some(byte) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.error(expected))
        }
    }

    /// `#RRGGBB` or `#RRGGBBAA`, at the `#`.
    fn hex(&mut self) -> Result<Color, CoreError> {
        self.expect(b'#', "`#`")?;
        let mut channels = [0, 0, 0, u8::MAX];
        let mut digits = 0;
        while let Some(value) = self.peek().and_then(hex_digit) {
            if digits == 8 {
                return Err(self.error("at most 8 hex digits"));
            }
            let channel = &mut channels[digits / 2];
            *channel = if digits % 2 == 0 {
                value << 4
            } else {
                *channel | value
            };
            digits += 1;
            self.pos += 1;
        }
        match digits {
            6 | 8 => {
                let [r, g, b, a] = channels;
                Ok(Color::rgba(r, g, b, a))
            }
            0 => Err(self.error("a hex digit")),
            _ => Err(self.error("6 or 8 hex digits")),
        }
    }

    /// `rgba(R, G, B, A)`, at the `r`.
    fn rgba(&mut self) -> Result<Color, CoreError> {
        let name = self.bytes.get(self.pos..self.pos + 4);
        if !name.is_some_and(|name| name.eq_ignore_ascii_case(b"rgba")) {
            return Err(self.error("`rgba(`"));
        }
        self.pos += 4;
        self.skip_whitespace();
        self.expect(b'(', "`(`")?;
        let mut channels = [0; 3];
        for channel in &mut channels {
            self.skip_whitespace();
            *channel = self.channel()?;
            self.skip_whitespace();
            self.expect(b',', "`,`")?;
        }
        self.skip_whitespace();
        let alpha = self.alpha()?;
        self.skip_whitespace();
        self.expect(b')', "`)`")?;
        let [r, g, b] = channels;
        Ok(Color::rgba(r, g, b, alpha))
    }

    /// An integer from 0 to 255.
    fn channel(&mut self) -> Result<u8, CoreError> {
        const EXPECTED: &str = "a channel value from 0 to 255";
        let start = self.pos;
        let mut value: u32 = 0;
        while let Some(digit) = self.peek().filter(u8::is_ascii_digit) {
            // Saturates long before overflowing: once above 255 it stays so.
            value = (value * 10 + u32::from(digit - b'0')).min(1000);
            self.pos += 1;
        }
        if self.pos == start || value > 255 {
            self.pos = start;
            return Err(self.error(EXPECTED));
        }
        Ok(value as u8)
    }

    /// A decimal from 0 to 1, scaled to `round(A × 255)` with exact integer
    /// arithmetic.
    fn alpha(&mut self) -> Result<u8, CoreError> {
        const EXPECTED: &str = "an alpha value from 0 to 1";
        const MAX_PLACES: u32 = 9;
        let start = self.pos;
        let mut whole: u64 = 0;
        let mut any_digit = false;
        while let Some(digit) = self.peek().filter(u8::is_ascii_digit) {
            whole = (whole * 10 + u64::from(digit - b'0')).min(10);
            any_digit = true;
            self.pos += 1;
        }
        let mut fraction: u64 = 0;
        let mut places: u32 = 0;
        if self.peek() == Some(b'.') {
            self.pos += 1;
            while let Some(digit) = self.peek().filter(u8::is_ascii_digit) {
                if places == MAX_PLACES {
                    return Err(self.error("at most 9 decimal places"));
                }
                fraction = fraction * 10 + u64::from(digit - b'0');
                places += 1;
                any_digit = true;
                self.pos += 1;
            }
        }
        // value = whole + fraction / 10^places, and must be at most 1.
        let denominator = 10u64.pow(places);
        let numerator = whole * denominator + fraction;
        if !any_digit || numerator > denominator {
            self.pos = start;
            return Err(self.error(EXPECTED));
        }
        // round(numerator × 255 / denominator), halves up. At most
        // 10⁹ × 510 + 10⁹, far below u64::MAX.
        let scaled = (numerator * 510 + denominator) / (2 * denominator);
        Ok(scaled.min(255) as u8)
    }
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Pcg32;
    use crate::testing::check;

    fn invalid(offset: usize, expected: &'static str) -> Result<Color, CoreError> {
        Err(CoreError::InvalidColor { offset, expected })
    }

    fn random_color(rng: &mut Pcg32) -> Color {
        let [r, g, b, a] = rng.next_u32().to_le_bytes();
        Color::rgba(r, g, b, a)
    }

    /// A premultiplied color that obeys the invariant.
    fn random_premultiplied(rng: &mut Pcg32) -> PremultipliedColor {
        random_color(rng).premultiply()
    }

    #[test]
    fn constructors_and_constants() {
        assert_eq!(
            Color::rgb(1, 2, 3),
            Color {
                r: 1,
                g: 2,
                b: 3,
                a: 255
            }
        );
        assert_eq!(Color::WHITE, Color::rgba(255, 255, 255, 255));
        assert_eq!(Color::BLACK, Color::rgba(0, 0, 0, 255));
        assert_eq!(Color::TRANSPARENT, Color::rgba(0, 0, 0, 0));
    }

    #[test]
    fn parses_hex() {
        assert_eq!(Color::parse("#5ccbcb"), Ok(Color::rgb(0x5c, 0xcb, 0xcb)));
        assert_eq!(Color::parse("#FF8000"), Ok(Color::rgb(255, 128, 0)));
        assert_eq!(Color::parse("#0a1B2c3D"), Ok(Color::rgba(10, 27, 44, 61)));
        assert_eq!(Color::parse("#00000000"), Ok(Color::TRANSPARENT));
        assert_eq!(Color::parse("  #ffffff \t"), Ok(Color::WHITE));
        assert_eq!("#ffffff".parse::<Color>(), Ok(Color::WHITE));
    }

    #[test]
    fn parses_rgba() {
        // The theme's panel color.
        assert_eq!(
            Color::parse("rgba(16,19,21,0.78)"),
            Ok(Color::rgba(16, 19, 21, 199))
        );
        assert_eq!(
            Color::parse(" RGBA ( 0 , 128 ,255 , 1 ) "),
            Ok(Color::rgba(0, 128, 255, 255))
        );
        assert_eq!(Color::parse("rgba(1,2,3,0)"), Ok(Color::rgba(1, 2, 3, 0)));
        assert_eq!(
            Color::parse("rgba(1,2,3,1.000)"),
            Ok(Color::rgba(1, 2, 3, 255))
        );
        assert_eq!(
            Color::parse("rgba(007,2,3,.5)"),
            Ok(Color::rgba(7, 2, 3, 128))
        );
        // Halves round up, exactly: 0.1 × 255 = 25.5 and 0.3 × 255 = 76.5.
        assert_eq!(Color::parse("rgba(0,0,0,0.1)").map(|c| c.a), Ok(26));
        assert_eq!(Color::parse("rgba(0,0,0,0.3)").map(|c| c.a), Ok(77));
        assert_eq!(
            Color::parse("rgba(0,0,0,0.999999999)").map(|c| c.a),
            Ok(255)
        );
    }

    #[test]
    fn errors_carry_the_offset_and_expectation() {
        assert_eq!(Color::parse(""), invalid(0, "`#` or `rgba(`"));
        assert_eq!(Color::parse("   "), invalid(3, "`#` or `rgba(`"));
        assert_eq!(Color::parse("5ccbcb"), invalid(0, "`#` or `rgba(`"));
        assert_eq!(Color::parse("#"), invalid(1, "a hex digit"));
        assert_eq!(Color::parse("#gg0000"), invalid(1, "a hex digit"));
        assert_eq!(Color::parse("#fff"), invalid(4, "6 or 8 hex digits"));
        assert_eq!(Color::parse("#fffffff"), invalid(8, "6 or 8 hex digits"));
        assert_eq!(Color::parse("#ff 000"), invalid(3, "6 or 8 hex digits"));
        assert_eq!(
            Color::parse("#fffffffff"),
            invalid(9, "at most 8 hex digits")
        );
        assert_eq!(Color::parse("#ffffff#"), invalid(7, "the end of the color"));
        assert_eq!(Color::parse("##ffffff"), invalid(1, "a hex digit"));
        assert_eq!(Color::parse("rgb(1,2,3)"), invalid(0, "`rgba(`"));
        assert_eq!(Color::parse("rgba 1,2,3,1)"), invalid(5, "`(`"));
        assert_eq!(
            Color::parse("rgba(256,0,0,1)"),
            invalid(5, "a channel value from 0 to 255")
        );
        assert_eq!(
            Color::parse("rgba(1,-2,0,1)"),
            invalid(7, "a channel value from 0 to 255")
        );
        assert_eq!(Color::parse("rgba(1,2 3,1)"), invalid(9, "`,`"));
        assert_eq!(
            Color::parse("rgba(1,2,3,1.5)"),
            invalid(11, "an alpha value from 0 to 1")
        );
        assert_eq!(
            Color::parse("rgba(1,2,3,.)"),
            invalid(11, "an alpha value from 0 to 1")
        );
        assert_eq!(
            Color::parse("rgba(1,2,3,0.1234567891)"),
            invalid(22, "at most 9 decimal places")
        );
        assert_eq!(Color::parse("rgba(1,2,3,1"), invalid(12, "`)`"));
        assert_eq!(
            Color::parse("rgba(1,2,3,1) x"),
            invalid(14, "the end of the color")
        );
        assert_eq!(Color::parse("#ééé"), invalid(1, "a hex digit"));
        let long = format!("#ffffff{}", " ".repeat(MAX_COLOR_LEN));
        assert_eq!(Color::parse(&long), invalid(64, "at most 64 bytes"));
    }

    #[test]
    fn long_runs_of_digits_are_errors_not_overflows() {
        // A channel or an alpha with more digits than its integer type can
        // hold must saturate while it is read, not wrap or panic.
        for channel in [
            "99999999999",
            "4294967296",
            "4294967551",
            "18446744073709551617",
        ] {
            assert_eq!(
                Color::parse(&format!("rgba({channel},0,0,1)")),
                invalid(5, "a channel value from 0 to 255"),
                "{channel}"
            );
            assert_eq!(
                Color::parse(&format!("rgba(0,0,{channel},1)")),
                invalid(9, "a channel value from 0 to 255"),
                "{channel}"
            );
        }
        for alpha in ["99999999999999999999", "18446744073709551617", "10", "11.0"] {
            assert_eq!(
                Color::parse(&format!("rgba(0,0,0,{alpha})")),
                invalid(11, "an alpha value from 0 to 1"),
                "{alpha}"
            );
        }
        // Leading zeros are digits too, and do not count against the value
        // (20 of them twice, to stay within `MAX_COLOR_LEN`).
        let zeros = "0".repeat(20);
        assert_eq!(
            Color::parse(&format!("rgba({zeros}255,0,0,{zeros}1)")),
            Ok(Color::rgba(255, 0, 0, 255))
        );
        assert_eq!(
            Color::parse(&format!("rgba({zeros}256,0,0,1)")),
            invalid(5, "a channel value from 0 to 255")
        );
    }

    #[test]
    fn the_length_limit_is_inclusive() {
        let exactly = format!("#ffffff{}", " ".repeat(MAX_COLOR_LEN - 7));
        assert_eq!(exactly.len(), MAX_COLOR_LEN);
        assert_eq!(Color::parse(&exactly), Ok(Color::WHITE));
        let one_more = format!("{exactly} ");
        assert_eq!(one_more.len(), MAX_COLOR_LEN + 1);
        assert_eq!(Color::parse(&one_more), invalid(64, "at most 64 bytes"));
    }

    #[test]
    fn display_round_trips_through_parse() {
        assert_eq!(Color::rgb(0x5c, 0xcb, 0xcb).to_string(), "#5ccbcb");
        assert_eq!(Color::rgba(16, 19, 21, 199).to_string(), "#101315c7");
        check("color display round trip", 2000, random_color, |&color| {
            let text = color.to_string();
            match Color::parse(&text) {
                Ok(back) if back == color => Ok(()),
                other => Err(format!("{text} parsed as {other:?}")),
            }
        });
    }

    #[test]
    fn parse_never_panics() {
        // Random bytes drawn mostly from the syntax's own alphabet, so that
        // inputs get deep into the parser.
        const ALPHABET: &[u8] = b"#rgbaRGBA(),. 0123456789abcdefABCDEF-+x\t\xc3\xa9";
        check(
            "color parse never panics",
            5000,
            |rng| {
                let len = rng.range(0..80u32);
                let mut bytes: Vec<u8> = (0..len)
                    .map(|_| *rng.choose(ALPHABET).expect("not empty"))
                    .collect();
                match rng.range(0..3u32) {
                    0 => {
                        // Start from a valid color and corrupt it.
                        bytes = b"rgba(16, 19, 21, 0.78)".to_vec();
                        let at = rng.range(0..bytes.len() as u32) as usize;
                        bytes[at] = *rng.choose(ALPHABET).expect("not empty");
                    }
                    1 => {
                        // Put a run of 10 to 40 digits, far more than any
                        // integer type holds, in a channel or the alpha.
                        let digits: String = (0..rng.range(10..41u32))
                            .map(|_| char::from(b'0' + rng.range(0..10u32) as u8))
                            .collect();
                        let mut slots = ["16", "19", "21", "0.78"];
                        slots[rng.range(0..4u32) as usize] = &digits;
                        let [r, g, b, a] = slots;
                        bytes = format!("rgba({r},{g},{b},{a})").into_bytes();
                    }
                    _ => {}
                }
                String::from_utf8_lossy(&bytes).into_owned()
            },
            |text| {
                if let Err(CoreError::InvalidColor { offset, .. }) = Color::parse(text)
                    && offset > text.len().max(MAX_COLOR_LEN)
                {
                    return Err(format!("offset {offset} is past the input"));
                }
                Ok(())
            },
        );
    }

    #[test]
    fn premultiply_known_values() {
        assert_eq!(
            Color::rgba(255, 128, 0, 128).premultiply(),
            PremultipliedColor {
                r: 128,
                g: 64,
                b: 0,
                a: 128
            }
        );
        assert_eq!(Color::WHITE.premultiply().unpremultiply(), Color::WHITE);
        assert_eq!(
            Color::rgba(9, 9, 9, 0).premultiply(),
            PremultipliedColor::TRANSPARENT
        );
        assert_eq!(
            PremultipliedColor::TRANSPARENT.unpremultiply(),
            Color::TRANSPARENT
        );
        // A channel above alpha (outside the invariant) clamps.
        let odd = PremultipliedColor {
            r: 200,
            g: 0,
            b: 0,
            a: 100,
        };
        assert_eq!(odd.unpremultiply().r, 255);
    }

    /// `round(x / y)` for `y > 0`, halves up, written independently of the
    /// code under test.
    fn round_div(x: u32, y: u32) -> u32 {
        (2 * x + y) / (2 * y)
    }

    #[test]
    fn premultiply_rounds_to_nearest_exhaustively() {
        // 255 is odd, so `c × a / 255` is never exactly a half and rounding
        // to nearest has one answer.
        for a in 0..=255u8 {
            for c in 0..=255u8 {
                let expected = round_div(u32::from(c) * u32::from(a), 255);
                let got = Color::rgba(c, c, c, a).premultiply();
                assert_eq!(u32::from(got.r), expected, "c = {c}, a = {a}");
                assert_eq!((got.g, got.b, got.a), (got.r, got.r, a));
            }
        }
    }

    #[test]
    fn unpremultiply_rounds_to_nearest_exhaustively() {
        // Every channel value, including those above alpha (outside the
        // invariant), which clamp to 255.
        for a in 1..=255u8 {
            for c in 0..=255u8 {
                let expected = round_div(u32::from(c) * 255, u32::from(a)).min(255);
                let got = PremultipliedColor {
                    r: c,
                    g: c,
                    b: c,
                    a,
                }
                .unpremultiply();
                assert_eq!(u32::from(got.r), expected, "c = {c}, a = {a}");
                assert_eq!((got.g, got.b, got.a), (got.r, got.r, a));
            }
        }
    }

    #[test]
    fn source_over_rounds_the_destination_to_nearest_exhaustively() {
        // With a black source of alpha `a`, every channel of the result is
        // the destination channel times `1 - a`, rounded.
        for a in 0..=255u8 {
            for d in 0..=255u8 {
                let src = PremultipliedColor {
                    r: 0,
                    g: 0,
                    b: 0,
                    a,
                };
                let dst = PremultipliedColor {
                    r: d,
                    g: d,
                    b: d,
                    a: d,
                };
                let keep = 255 - u32::from(a);
                let expected = round_div(u32::from(d) * keep, 255);
                let out = src.over(dst);
                assert_eq!(u32::from(out.r), expected, "a = {a}, d = {d}");
                assert_eq!(
                    u32::from(out.a),
                    u32::from(a) + expected,
                    "a = {a}, d = {d}"
                );
            }
        }
    }

    #[test]
    fn source_over_matches_the_formula() {
        check(
            "source-over is src + round(dst × (255 − src.a) / 255)",
            5000,
            |rng| (random_premultiplied(rng), random_premultiplied(rng)),
            |&(src, dst)| {
                let keep = 255 - u32::from(src.a);
                let channel =
                    |s: u8, d: u8| (u32::from(s) + round_div(u32::from(d) * keep, 255)).min(255);
                let expected = (
                    channel(src.r, dst.r),
                    channel(src.g, dst.g),
                    channel(src.b, dst.b),
                    channel(src.a, dst.a),
                );
                let out = src.over(dst);
                let got = (out.r.into(), out.g.into(), out.b.into(), out.a.into());
                if got == expected {
                    Ok(())
                } else {
                    Err(format!("got {out:?}, expected {expected:?}"))
                }
            },
        );
    }

    #[test]
    fn opaque_colors_survive_premultiplication_exactly() {
        check("premultiply opaque is identity", 1000, random_color, |&c| {
            let opaque = Color { a: 255, ..c };
            let back = opaque.premultiply().unpremultiply();
            if back != opaque {
                return Err(format!("came back as {back:?}"));
            }
            Ok(())
        });
    }

    #[test]
    fn unpremultiply_inverts_premultiply_within_rounding() {
        check(
            "unpremultiply inverts premultiply",
            2000,
            random_color,
            |&c| {
                let back = c.premultiply().unpremultiply();
                if c.a == 0 {
                    return if back == Color::TRANSPARENT {
                        Ok(())
                    } else {
                        Err(format!("transparent came back as {back:?}"))
                    };
                }
                // Premultiplying loses up to half a step of 255/a in each channel.
                let tolerance = 255 / u32::from(c.a) / 2 + 1;
                for (before, after) in [(c.r, back.r), (c.g, back.g), (c.b, back.b)] {
                    if u32::from(before.abs_diff(after)) > tolerance {
                        return Err(format!("came back as {back:?}"));
                    }
                }
                if back.a != c.a {
                    return Err(format!("alpha changed: {back:?}"));
                }
                Ok(())
            },
        );
    }

    #[test]
    fn opaque_source_over_replaces_the_destination() {
        check(
            "source-over with opaque source",
            2000,
            |rng| (random_color(rng), random_premultiplied(rng)),
            |&(src, dst)| {
                let src = Color { a: 255, ..src }.premultiply();
                let out = src.over(dst);
                if out != src {
                    return Err(format!("got {out:?}"));
                }
                Ok(())
            },
        );
    }

    #[test]
    fn transparent_source_over_keeps_the_destination() {
        check(
            "source-over with transparent source",
            2000,
            |rng| (random_color(rng), random_premultiplied(rng)),
            |&(src, dst)| {
                let src = Color { a: 0, ..src }.premultiply();
                let out = src.over(dst);
                if out != dst {
                    return Err(format!("got {out:?}"));
                }
                Ok(())
            },
        );
    }

    #[test]
    fn source_over_stays_in_range() {
        check(
            "source-over stays a valid premultiplied color",
            5000,
            |rng| (random_premultiplied(rng), random_premultiplied(rng)),
            |&(src, dst)| {
                let out = src.over(dst);
                if out.r > out.a || out.g > out.a || out.b > out.a {
                    return Err(format!("a channel exceeds alpha: {out:?}"));
                }
                if out.a < src.a || out.a < dst.a {
                    return Err(format!("alpha went down: {out:?}"));
                }
                Ok(())
            },
        );
        // Out-of-invariant inputs saturate instead of overflowing.
        let bad = PremultipliedColor {
            r: 255,
            g: 255,
            b: 255,
            a: 0,
        };
        assert_eq!(bad.over(bad).r, 255);
    }
}
