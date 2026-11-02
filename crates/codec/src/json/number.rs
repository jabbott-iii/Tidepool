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

//! [`Number`]: a JSON number, kept as an integer or a finite float.
//!
//! New in Tidepool: Nadir's `Value` had `Int(i64)` and `Float(f64)`
//! variants. The float formatting is ported from nadir-codec
//! `json/write.rs` (`write_float`).

use std::fmt;

/// A JSON number: an integer (`u64` or negative `i64`) or a finite `f64`.
///
/// How the parser stores a number depends on how it is written:
///
/// - Without a fraction or exponent (`42`, `-7`): an integer, if it fits a
///   `u64` (from 0 up) or an `i64` (below 0). These round-trip exactly.
/// - With a fraction or exponent (`1.5`, `1e3`, `2.0`), as `-0` (to keep
///   its sign), or an integer too large for both: the nearest `f64`. The
///   written form is the shortest that reads back as the same `f64`, so the
///   value round-trips, though its spelling may change (`1e3` is written
///   back as `1000.0`). Digits beyond what an `f64` holds are lost.
/// - A magnitude too large for an `f64` (`1e400`) is an error
///   ([`ErrorKind::NumberOutOfRange`](super::ErrorKind::NumberOutOfRange));
///   one too small (`1e-400`) becomes zero.
///
/// A number is never NaN or infinite, which JSON cannot express, so the
/// writers never fail and equality is a true equivalence (`Eq`). An integer
/// and a float are different numbers even when numerically equal: `2` and
/// `2.0` compare unequal, because they are written differently. `0.0` and
/// `-0.0` compare equal, as `f64`s do.
#[derive(Clone, Copy, PartialEq)]
pub struct Number(Repr);

/// The stored form. Each integer has exactly one representation, so the
/// derived equality is numeric equality for integers.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Repr {
    /// An integer from 0 up.
    PosInt(u64),
    /// An integer below 0. Never 0 or more.
    NegInt(i64),
    /// Anything else. Always finite.
    Float(f64),
}

// `Repr::Float` is never NaN, so `==` is reflexive.
impl Eq for Number {}

impl Number {
    /// A float number, or `None` if `value` is NaN or infinite.
    ///
    /// The result is a float even when `value` is a whole number: it is
    /// written as `2.0`, not `2`.
    pub fn from_f64(value: f64) -> Option<Self> {
        value.is_finite().then_some(Self(Repr::Float(value)))
    }

    /// Returns `true` if this is an integer, `false` if it is a float.
    pub fn is_integer(&self) -> bool {
        !matches!(self.0, Repr::Float(_))
    }

    /// Returns the integer as an `i64`, if this is an integer that fits.
    /// A float is never converted, even when it is a whole number.
    pub fn as_i64(&self) -> Option<i64> {
        match self.0 {
            Repr::PosInt(value) => i64::try_from(value).ok(),
            Repr::NegInt(value) => Some(value),
            Repr::Float(_) => None,
        }
    }

    /// Returns the integer as a `u64`, if this is an integer from 0 up. A
    /// float is never converted, even when it is a whole number.
    pub fn as_u64(&self) -> Option<u64> {
        match self.0 {
            Repr::PosInt(value) => Some(value),
            Repr::NegInt(_) | Repr::Float(_) => None,
        }
    }

    /// Returns the number as an `f64`. Integers beyond ±2⁵³ are rounded to
    /// the nearest `f64`.
    pub fn as_f64(&self) -> f64 {
        match self.0 {
            Repr::PosInt(value) => value as f64,
            Repr::NegInt(value) => value as f64,
            Repr::Float(value) => value,
        }
    }

    /// Converts the text of a number that has already been checked against
    /// the JSON grammar. `integer` says it has no fraction or exponent.
    /// Returns `None` if its magnitude is too large for an `f64`.
    pub(crate) fn from_lexeme(text: &str, integer: bool) -> Option<Self> {
        if integer {
            if text.starts_with('-') {
                // `-0` falls through to the float, which keeps its sign.
                if let Ok(value) = text.parse::<i64>()
                    && value < 0
                {
                    return Some(Self(Repr::NegInt(value)));
                }
            } else if let Ok(value) = text.parse::<u64>() {
                return Some(Self(Repr::PosInt(value)));
            }
        }
        // Rust's float syntax is a superset of JSON's, and parsing never
        // fails on a valid JSON number; it rounds to an infinity when the
        // magnitude is too large, however long the exponent.
        Self::from_f64(text.parse().ok()?)
    }
}

impl From<u64> for Number {
    /// An integer number.
    fn from(value: u64) -> Self {
        Self(Repr::PosInt(value))
    }
}

impl From<i64> for Number {
    /// An integer number.
    fn from(value: i64) -> Self {
        match u64::try_from(value) {
            Ok(value) => Self(Repr::PosInt(value)),
            Err(_) => Self(Repr::NegInt(value)),
        }
    }
}

impl From<u32> for Number {
    /// An integer number.
    fn from(value: u32) -> Self {
        Self::from(u64::from(value))
    }
}

impl From<i32> for Number {
    /// An integer number.
    fn from(value: i32) -> Self {
        Self::from(i64::from(value))
    }
}

impl fmt::Display for Number {
    /// Writes the number as JSON: integers as their digits, floats as the
    /// shortest digits that read back as the same `f64`, always with a `.`
    /// or an exponent so that they read back as floats.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Repr::PosInt(value) => write!(f, "{value}"),
            Repr::NegInt(value) => write!(f, "{value}"),
            Repr::Float(value) => write_float(f, value),
        }
    }
}

impl fmt::Debug for Number {
    /// `Number(<JSON text>)`, so that `2` and `2.0` are told apart.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Number({self})")
    }
}

/// Writes a finite float. Very large and very small magnitudes are shorter
/// with an exponent; the rest are written plainly, with `.0` added to whole
/// numbers.
fn write_float(f: &mut fmt::Formatter<'_>, value: f64) -> fmt::Result {
    let magnitude = value.abs();
    if magnitude != 0.0 && !(1e-5..1e16).contains(&magnitude) {
        // Rust's `{:e}` is the shortest round-trip form, such as `1.5e300`
        // or `5e-324`: valid JSON as it is.
        write!(f, "{value:e}")
    } else if value.fract() == 0.0 {
        // Below 1e16 a whole number is printed as its digits alone.
        write!(f, "{value}.0")
    } else {
        write!(f, "{value}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn float(value: f64) -> String {
        Number::from_f64(value).expect("finite").to_string()
    }

    #[test]
    fn only_finite_floats_are_numbers() {
        assert_eq!(Number::from_f64(f64::NAN), None);
        assert_eq!(Number::from_f64(f64::INFINITY), None);
        assert_eq!(Number::from_f64(f64::NEG_INFINITY), None);
        assert!(Number::from_f64(f64::MAX).is_some());
        assert!(Number::from_f64(-0.0).is_some());
    }

    #[test]
    fn integers_have_one_representation() {
        assert_eq!(Number::from(5i64), Number::from(5u64));
        assert_eq!(Number::from(5i32), Number::from(5u32));
        assert_eq!(Number::from(0i64), Number::from(0u64));
        assert_ne!(Number::from(-1i64), Number::from(1u64));
        // An integer and a float are different numbers.
        assert_ne!(Number::from(2u64), Number::from_f64(2.0).expect("finite"));
        // Floats compare as `f64`s do.
        assert_eq!(Number::from_f64(0.0), Number::from_f64(-0.0));
    }

    #[test]
    fn conversions() {
        let big = Number::from(u64::MAX);
        assert_eq!(big.as_u64(), Some(u64::MAX));
        assert_eq!(big.as_i64(), None);
        assert_eq!(big.as_f64(), 18_446_744_073_709_551_615.0);
        let negative = Number::from(i64::MIN);
        assert_eq!(negative.as_i64(), Some(i64::MIN));
        assert_eq!(negative.as_u64(), None);
        assert!(negative.is_integer());
        let whole = Number::from_f64(3.0).expect("finite");
        assert!(!whole.is_integer());
        assert_eq!(whole.as_i64(), None);
        assert_eq!(whole.as_u64(), None);
        assert_eq!(whole.as_f64(), 3.0);
    }

    #[test]
    fn lexemes_become_the_narrowest_exact_form() {
        let lexeme = |text: &str, integer| Number::from_lexeme(text, integer);
        assert_eq!(lexeme("0", true), Some(Number::from(0u64)));
        assert_eq!(lexeme("-12", true), Some(Number::from(-12i64)));
        assert_eq!(
            lexeme("18446744073709551615", true),
            Some(Number::from(u64::MAX))
        );
        assert_eq!(
            lexeme("-9223372036854775808", true),
            Some(Number::from(i64::MIN))
        );
        // Past the integer types: the nearest float.
        assert_eq!(
            lexeme("18446744073709551616", true),
            Number::from_f64(1.8446744073709552e19)
        );
        assert_eq!(
            lexeme("-9223372036854775809", true),
            Number::from_f64(-9.223372036854776e18)
        );
        // `-0` keeps its sign as a float.
        let zero = lexeme("-0", true).expect("a number");
        assert!(!zero.is_integer() && zero.as_f64().is_sign_negative());
        assert_eq!(lexeme("1.5", false), Number::from_f64(1.5));
        assert_eq!(lexeme("1e400", false), None);
        assert_eq!(lexeme("-1e400", false), None);
        assert_eq!(lexeme(&"9".repeat(400), true), None);
        assert_eq!(lexeme("1e-400", false), Number::from_f64(0.0));
        assert_eq!(lexeme("1e99999999999999999999999", false), None);
        assert_eq!(
            lexeme("0e99999999999999999999999", false),
            Number::from_f64(0.0)
        );
    }

    #[test]
    fn integers_are_written_as_their_digits() {
        assert_eq!(Number::from(0u64).to_string(), "0");
        assert_eq!(Number::from(-5i64).to_string(), "-5");
        assert_eq!(Number::from(u64::MAX).to_string(), "18446744073709551615");
        assert_eq!(Number::from(i64::MIN).to_string(), "-9223372036854775808");
    }

    #[test]
    fn floats_always_look_like_floats() {
        assert_eq!(float(1.0), "1.0");
        assert_eq!(float(-2.0), "-2.0");
        assert_eq!(float(0.0), "0.0");
        assert_eq!(float(-0.0), "-0.0");
        assert_eq!(float(0.5), "0.5");
        assert_eq!(float(0.1), "0.1");
        assert_eq!(float(123_456_789.125), "123456789.125");
        assert_eq!(float(1.0 / 3.0), "0.3333333333333333");
    }

    #[test]
    fn very_large_and_very_small_floats_use_an_exponent() {
        // The plain form runs from 1e-5 up to, but not including, 1e16.
        assert_eq!(float(1e-5), "0.00001");
        assert_eq!(float(9.999e-6), "9.999e-6");
        assert_eq!(float(1e-6), "1e-6");
        assert_eq!(float(9_999_999_999_999_998.0), "9999999999999998.0");
        assert_eq!(float(1e16), "1e16");
        assert_eq!(float(-1e16), "-1e16");
        assert_eq!(float(1.5e300), "1.5e300");
        assert_eq!(float(f64::MAX), "1.7976931348623157e308");
        assert_eq!(float(f64::MIN_POSITIVE), "2.2250738585072014e-308");
        assert_eq!(float(5e-324), "5e-324");
    }

    #[test]
    fn debug_shows_the_json_text() {
        assert_eq!(format!("{:?}", Number::from(2u64)), "Number(2)");
        assert_eq!(
            format!("{:?}", Number::from_f64(2.0).expect("finite")),
            "Number(2.0)"
        );
    }
}
