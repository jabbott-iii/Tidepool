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

//! The user's X resources, of which the window reads one: `Xft.dpi`, for
//! its scale.
//!
//! Ported from nadir-platform `linux/xresources.rs`, with its tests. Adapted
//! for Tidepool: the lookup reads bytes, not UTF-8 text, since the
//! resources are a property any client of the display can set to
//! anything; the scale is a [`Scale`]; and a property test checks that no
//! bytes make it panic or give a scale outside the range.
//!
//! Desktop environments set `Xft.dpi` to say how dense the display is, and
//! toolkits scale their interfaces by it. 96 is the standard density. The
//! resources are text, one `name: value` to a line. A line that starts
//! with `!` is a comment. If a name is set twice, the later line wins.
//!
//! This is a lookup of one name, not a parser of the resource format, so
//! it has no fuzz target (ADR-0027); its property test stands in for one.
//! Xlib's own resource parser does run in the process, over the same
//! property and the user's `~/.Xdefaults` files, when libXcursor, which
//! libX11 loads for the window's first cursor where it is installed, asks
//! for its settings (`docs/03-platforms.md` → Input → Cursors).

use core::ops::RangeInclusive;

use tidepool_core::geom::Scale;

/// The density at which the scale is 1.
const STANDARD_DPI: f32 = 96.0;

/// The densities we believe. Anything outside is a broken setting.
const PLAUSIBLE_DPI: RangeInclusive<f32> = 24.0..=1536.0;

/// Returns the scale that the resources ask for, or 1 if they do not say
/// or there are none.
pub(super) fn scale(resources: Option<&[u8]>) -> Scale {
    resources
        .and_then(xft_dpi)
        .and_then(|dpi| Scale::new(dpi / STANDARD_DPI))
        .unwrap_or(Scale::ONE)
}

/// Returns the value of `Xft.dpi`, if the resources set it to a density we
/// believe.
///
/// Only the exact name `Xft.dpi` counts. The loose forms that the resource
/// manager also accepts, such as `*dpi`, are not looked for.
fn xft_dpi(resources: &[u8]) -> Option<f32> {
    let value = resources
        .split(|&byte| byte == b'\n')
        .filter_map(|line| line.trim_ascii_start().strip_prefix(b"Xft.dpi"))
        .filter_map(|rest| rest.trim_ascii_start().strip_prefix(b":"))
        .next_back()?;
    let dpi: f32 = core::str::from_utf8(value.trim_ascii())
        .ok()?
        .parse()
        .ok()?;
    PLAUSIBLE_DPI.contains(&dpi).then_some(dpi)
}

#[cfg(test)]
mod tests {
    use tidepool_core::rng::Pcg32;
    use tidepool_core::testing::check;

    use super::*;

    fn dpi(resources: &str) -> Option<f32> {
        xft_dpi(resources.as_bytes())
    }

    fn factor(resources: Option<&str>) -> f32 {
        scale(resources.map(str::as_bytes)).factor()
    }

    #[test]
    fn the_dpi_is_read_from_its_line() {
        assert_eq!(dpi("Xft.dpi:\t96"), Some(96.0));
        assert_eq!(dpi("Xft.dpi: 144\n"), Some(144.0));
        assert_eq!(dpi("Xft.dpi:192"), Some(192.0));
        assert_eq!(dpi("  Xft.dpi  :  120  "), Some(120.0));
        assert_eq!(dpi("Xft.dpi:\t110.5"), Some(110.5));
        assert_eq!(dpi("Xft.dpi:\t144\r\n"), Some(144.0));
    }

    #[test]
    fn the_dpi_is_found_among_other_resources() {
        let resources = "\
Xcursor.size:\t24
Xcursor.theme:\tAdwaita
Xft.antialias:\t1
Xft.dpi:\t168
Xft.hinting:\t1
Xft.rgba:\trgb
";
        assert_eq!(dpi(resources), Some(168.0));
        assert_eq!(factor(Some(resources)), 1.75);
    }

    #[test]
    fn the_later_of_two_lines_wins() {
        assert_eq!(dpi("Xft.dpi: 96\nXft.dpi: 192\n"), Some(192.0));
        // A broken later line does not fall back to an earlier one, just as
        // it would not in the resource manager.
        assert_eq!(dpi("Xft.dpi: 96\nXft.dpi: big\n"), None);
    }

    #[test]
    fn other_names_are_not_the_dpi() {
        for resources in [
            "",
            "Xft.dpix: 192",
            "Xft.dpi.other: 192",
            "MyXft.dpi: 192",
            "! Xft.dpi: 192",
            "*dpi: 192",
            "Xft*dpi: 192",
            "xft.dpi: 192",
            "Xft.dpi 192",
            "Xft.antialias: 1\nXcursor.size: 24",
        ] {
            assert_eq!(dpi(resources), None, "{resources:?}");
        }
    }

    #[test]
    fn a_value_that_is_not_a_plausible_density_is_ignored() {
        for value in [
            "", "abc", "96dpi", "0", "-96", "23.9", "1537", "1e30", "inf", "NaN", "96 96",
        ] {
            let resources = format!("Xft.dpi: {value}");
            assert_eq!(dpi(&resources), None, "{value:?}");
            assert_eq!(factor(Some(&resources)), 1.0, "{value:?}");
        }
        assert_eq!(dpi("Xft.dpi: 24"), Some(24.0));
        assert_eq!(dpi("Xft.dpi: 1536"), Some(1536.0));
        // Bytes that are not UTF-8 in the value are no number.
        assert_eq!(xft_dpi(b"Xft.dpi: 9\xff6"), None);
    }

    #[test]
    fn the_scale_is_the_dpi_over_96() {
        assert_eq!(factor(Some("Xft.dpi: 96")), 1.0);
        assert_eq!(factor(Some("Xft.dpi: 120")), 1.25);
        assert_eq!(factor(Some("Xft.dpi: 144")), 1.5);
        assert_eq!(factor(Some("Xft.dpi: 192")), 2.0);
        assert_eq!(factor(Some("Xft.dpi: 72")), 0.75);
    }

    #[test]
    fn without_resources_or_a_dpi_the_scale_is_one() {
        assert_eq!(factor(None), 1.0);
        assert_eq!(factor(Some("")), 1.0);
        assert_eq!(factor(Some("Xcursor.size: 24")), 1.0);
    }

    /// Random resources: lines of fragments that look like resources, and
    /// random bytes.
    fn resources(rng: &mut Pcg32) -> Vec<u8> {
        const PIECES: [&[u8]; 14] = [
            b"Xft.dpi",
            b":",
            b" ",
            b"\t",
            b"\n",
            b"\r",
            b"!",
            b"96",
            b"1e3",
            b".5",
            b"-",
            b"inf",
            b"Xft.antialias: 1\n",
            b"\xff\xfe",
        ];
        let mut bytes = Vec::new();
        for _ in 0..rng.range(0..40_u32) {
            if rng.range(0..4_u32) == 0 {
                bytes.push(rng.next_u32().to_le_bytes()[0]);
            } else if let Some(piece) = rng.choose(&PIECES) {
                bytes.extend_from_slice(piece);
            }
        }
        bytes
    }

    #[test]
    fn no_bytes_give_a_scale_out_of_range() {
        let lowest = 24.0 / STANDARD_DPI;
        let highest = 1536.0 / STANDARD_DPI;
        check(
            "any resources give a scale in range",
            5000,
            resources,
            |bytes| {
                let factor = scale(Some(bytes.as_slice())).factor();
                let in_range = factor == 1.0 || (lowest..=highest).contains(&factor);
                if factor.is_finite() && in_range {
                    Ok(())
                } else {
                    Err(format!("scale {factor}"))
                }
            },
        );
    }

    #[test]
    fn a_last_line_with_a_plausible_dpi_decides_whatever_comes_before() {
        let generate = |rng: &mut Pcg32| {
            let dpi = rng.range(24_u32..1537);
            let mut bytes = resources(rng);
            bytes.extend_from_slice(format!("\nXft.dpi:\t{dpi}\n").as_bytes());
            (bytes, dpi)
        };
        check(
            "the last Xft.dpi line wins",
            2000,
            generate,
            |(bytes, dpi)| {
                let expected = *dpi as f32 / STANDARD_DPI;
                let factor = scale(Some(bytes.as_slice())).factor();
                if factor == expected {
                    Ok(())
                } else {
                    Err(format!("scale {factor}, expected {expected}"))
                }
            },
        );
    }
}
