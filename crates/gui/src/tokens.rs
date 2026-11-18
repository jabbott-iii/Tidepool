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

//! Stand-ins for the theme's tokens that the first frame uses, until
//! M3.2's theme replaces them (ADR-0045).
//!
//! They are `gui`'s only literal colors. Each is named for its token and
//! holds the token's value in `docs/subsystems/ui-and-theme.md` → Tokens,
//! which a test reads. The backdrop's colors are `gfx`'s
//! [`BackdropStyle::jabbott_dev`](tidepool_gfx::backdrop::BackdropStyle::jabbott_dev).

use tidepool_core::color::Color;

/// `text`, primary text: `#eef2f3`.
pub(crate) const TEXT: Color = Color::rgb(0xee, 0xf2, 0xf3);

/// `text-3`, muted text: `#7d898e`.
pub(crate) const TEXT_3: Color = Color::rgb(0x7d, 0x89, 0x8e);

#[cfg(test)]
mod tests {
    use super::*;

    /// The value of `token` in the spec's token table: its row
    /// `` | `token` | `#rrggbb` | ``.
    fn spec_value(token: &str) -> Color {
        let spec = include_str!("../../../docs/subsystems/ui-and-theme.md");
        let start = format!("| `{token}` | `");
        let row = spec
            .lines()
            .find_map(|line| line.strip_prefix(start.as_str()))
            .unwrap_or_else(|| panic!("the token table has no row for `{token}`"));
        let value = row.split('`').next().unwrap_or_default();
        Color::parse(value).unwrap_or_else(|error| panic!("`{token}` is {value:?}: {error}"))
    }

    #[test]
    fn each_stand_in_is_its_tokens_value_in_the_spec() {
        assert_eq!(TEXT, spec_value("text"));
        assert_eq!(TEXT_3, spec_value("text-3"));
    }
}
