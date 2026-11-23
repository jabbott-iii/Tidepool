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

//! The theme: the tokens of a `themes/*.toml` file, read into a [`Theme`]
//! (ADR-0047).
//!
//! New in Tidepool; not ported.
//!
//! The token set and its values are `docs/subsystems/ui-and-theme.md` →
//! Tokens, which `themes/tidepool-dark.toml` writes down and
//! [`Theme::tidepool_dark`] reads, from a copy compiled into the crate.
//! Widgets take their colors from a theme by [`Token`], never as literals.
//!
//! # The file
//!
//! A theme file is TOML (ADR-0013), version 1:
//!
//! - `version = 1` and `name`, the theme's name;
//! - `[colors]`: each token, by its spec name, an inline table with
//!   `value`, in [`Color::parse`]'s syntax, and two optional keys: `solid`,
//!   only for a translucent value, the opaque color the value makes over
//!   `bg`, for surfaces drawn without the backdrop and the terminal UI; and
//!   `ansi`, the 256-color index the terminal UI falls back to. `bg`
//!   itself is opaque;
//! - `[sizes]`: `cut`, the cut-corner size, in logical units, from 0 to
//!   [`MAX_CUT`];
//! - `[fonts]`: `font-ui` and `font-mono`, the families of
//!   [`Family::Ui`] and [`Family::Mono`];
//! - `[contrast]`, optional: the pairs of colors the theme uses for text,
//!   which `cargo xtask theme` checks. This module reads only that it is a
//!   table.
//!
//! Every key is required but those named optional, and a key the format
//! does not have is refused, so a misspelled token is an error, not a
//! default. A name or a font is one line of plain text that is not blank,
//! at most [`MAX_TEXT_BYTES`] bytes.
//!
//! [`Theme::parse`] never panics. It refuses what breaks these rules with a
//! [`UiError::Theme`] that names the key and, unless it is missing, its
//! line, and a file that is
//! not TOML, or breaks its [`Limits`], with [`UiError::ThemeSyntax`].
//!
//! ```
//! use tidepool_core::color::Color;
//! use tidepool_ui::metrics::Family;
//! use tidepool_ui::theme::{Theme, Token};
//!
//! let theme = Theme::tidepool_dark();
//! assert_eq!(theme.name(), "tidepool-dark");
//! assert_eq!(theme.color(Token::Teal), Color::rgb(0x5c, 0xcb, 0xcb));
//! // A translucent token, and the solid color it makes over `bg`.
//! assert_eq!(theme.color(Token::Panel), Color::rgba(16, 19, 21, 199));
//! assert_eq!(theme.solid(Token::Panel), Color::rgb(0x0f, 0x11, 0x13));
//! assert_eq!(theme.ansi(Token::Panel), Some(233));
//! assert_eq!(theme.cut(), 10.0);
//! assert_eq!(theme.font(Family::Mono), "JetBrains Mono");
//! ```

use std::fmt;
use std::sync::{Arc, OnceLock};

use tidepool_codec::Limits;
use tidepool_codec::toml::{self, Span, Table, Value, ValueKind};
use tidepool_core::color::Color;

use crate::error::UiError;
use crate::metrics::Family;

/// The version of the theme format this module reads.
pub const VERSION: i64 = 1;

/// The largest `cut` a theme may give, in logical units.
pub const MAX_CUT: f32 = 64.0;

/// The longest name or font a theme may give, in bytes.
pub const MAX_TEXT_BYTES: usize = 64;

/// The limits [`Theme::tidepool_dark`] parses with, which suit any theme
/// file: 1 MiB of input, 8 levels, strings of 4 KiB, numbers of 64 bytes
/// and 1,024 entries in a table or array. The bundled file is about 9.5 KiB,
/// and 4 levels deep at `contrast.use[i].on[j]`.
pub const LIMITS: Limits = Limits {
    max_input_bytes: 1024 * 1024,
    max_depth: 8,
    max_string_bytes: 4096,
    max_number_bytes: 64,
    max_entries: 1024,
    ..Limits::DEFAULT
};

/// The text of `themes/tidepool-dark.toml`, compiled in.
pub const TIDEPOOL_DARK_TOML: &str = include_str!("../../../themes/tidepool-dark.toml");

/// A color token of the theme, named as the spec and the file name it.
///
/// The theme's length, `cut`, and its fonts are read with
/// [`Theme::cut`] and [`Theme::font`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Token {
    /// `bg`: the window background.
    Bg,
    /// `bg-raise`: raised surfaces.
    BgRaise,
    /// `panel`, translucent: side panels, the palette, popups.
    Panel,
    /// `panel-solid`: menus, tooltips.
    PanelSolid,
    /// `slate`: inputs, chips.
    Slate,
    /// `slate-2`: media wells, the image viewer.
    Slate2,
    /// `line`, translucent: hairline borders, separators.
    Line,
    /// `line-2`, translucent: stronger borders, code blocks.
    Line2,
    /// `text`: primary text.
    Text,
    /// `text-2`: secondary text.
    Text2,
    /// `text-3`: muted text, eyebrows, line numbers.
    Text3,
    /// `teal`: the accent: links, focus, active state, keywords.
    Teal,
    /// `teal-hi`: hover and active text.
    TealHi,
    /// `teal-wash`, translucent: a hover wash.
    TealWash,
    /// `teal-glow`, translucent: glows, the selection.
    TealGlow,
    /// `amber`: the primary action, warnings.
    Amber,
    /// `amber-hi`: the site's primary action's hover, which no widget
    /// paints since buttons became tiles (ADR-0058).
    AmberHi,
    /// `green`: ready, success, git added.
    Green,
    /// `rust`: numbers in code, Rust-flavored accents.
    Rust,
    /// `violet`: attributes, properties.
    Violet,
    /// `bronze`: the backdrop's circuits (ADR-0057).
    Bronze,
    /// `bronze-hi`: the highlight of `bronze`, the circuits' sparks
    /// (ADR-0057).
    BronzeHi,
    /// `purple`: the backdrop's synapses (ADR-0057).
    Purple,
    /// `purple-hi`: the highlight of `purple`, the synapses' sparks
    /// (ADR-0057).
    PurpleHi,
    /// `error`: errors, git deleted.
    Error,
    /// `editor-bg`: the editor and code blocks.
    EditorBg,
    /// `bar`, translucent: the status bar, the command bar.
    Bar,
    /// `on-amber`: text on `amber`.
    OnAmber,
    /// `on-teal`: text on `teal`, as an active chip's label.
    OnTeal,
    /// `comment`: comments in code, only on `editor-bg`.
    Comment,
    /// `ghost`, translucent: the site's ghost button's fill, and a card's
    /// (ADR-0049).
    Ghost,
    /// `teal-line`, translucent: a lit teal tile's border (ADR-0058); a
    /// hovered chip's, field's or card's border (ADR-0049).
    TealLine,
    /// `teal-fill`, translucent: the site's teal button's hover fill; a
    /// hovered card's ring, a pressed close control or COPY (ADR-0049).
    TealFill,
    /// `slate-hi`: a pressed chip's fill (ADR-0049).
    SlateHi,
    /// `teal-ring`, translucent: a focused field's ring (ADR-0049).
    TealRing,
    /// `green-halo`, translucent: the halo of a status dot that is ready
    /// (ADR-0049).
    GreenHalo,
    /// `amber-halo`, translucent: the halo of a status dot that is busy
    /// (ADR-0049).
    AmberHalo,
    /// `error-halo`, translucent: the halo of a status dot that failed
    /// (ADR-0049).
    ErrorHalo,
    /// `table-head`, translucent: a table's header row, `slate` at 60%
    /// (ADR-0052).
    TableHead,
    /// `row-hover`, translucent: the wash of a hovered row of a list, tree
    /// view or table, white at 2.5% (ADR-0052).
    RowHover,
    /// `card-tab`: a card's tab, its pill label (`.card__tab`, ADR-0052).
    CardTab,
    /// `on-card-tab`: text on `card-tab`, a card tab's label (ADR-0052).
    OnCardTab,
    /// `code-bar`, translucent: a code block's bar, `slate` at 45%
    /// (`.code-bar`, ADR-0052).
    CodeBar,
    /// `neutral-label`: the label of a lit neutral tile, a ghost or slate
    /// button hovered, pressed or focused: `text-2` mixed 55% with white
    /// (`.nav-tile`, ADR-0058).
    NeutralLabel,
    /// `teal-label`: the label of a lit teal tile, `teal` mixed 55% with
    /// white (ADR-0058).
    TealLabel,
    /// `amber-label`: the label of a lit amber tile, `amber` mixed 55%
    /// with white (ADR-0058).
    AmberLabel,
    /// `neutral-line`, translucent: the border of a lit neutral tile,
    /// `text-2` at 55% (ADR-0058).
    NeutralLine,
    /// `amber-line`, translucent: the border of a lit amber tile, `amber`
    /// at 55% (ADR-0058).
    AmberLine,
    /// `neutral-haze`, translucent: one coat of a ghost button's corner
    /// glow, `text-2` at 5/255 (2%); nine coats make the site's 16%
    /// (ADR-0058).
    NeutralHaze,
    /// `teal-haze`, translucent: one coat of a teal tile's corner glow,
    /// `teal` at 5/255 (ADR-0058).
    TealHaze,
    /// `amber-haze`, translucent: one coat of an amber tile's corner glow,
    /// `amber` at 5/255 (ADR-0058).
    AmberHaze,
    /// `current-line`, translucent: the wash of the editor's current line,
    /// white at 2.5%, as `row-hover` is, but a token of its own, so that
    /// the two can change apart (ADR-0063).
    CurrentLine,
}

impl Token {
    /// How many tokens there are.
    pub const COUNT: usize = 52;

    /// Every token, in the order the spec lists them.
    pub const ALL: [Self; Self::COUNT] = [
        Self::Bg,
        Self::BgRaise,
        Self::Panel,
        Self::PanelSolid,
        Self::Slate,
        Self::Slate2,
        Self::Line,
        Self::Line2,
        Self::Text,
        Self::Text2,
        Self::Text3,
        Self::Teal,
        Self::TealHi,
        Self::TealWash,
        Self::TealGlow,
        Self::Amber,
        Self::AmberHi,
        Self::Green,
        Self::Rust,
        Self::Violet,
        Self::Bronze,
        Self::BronzeHi,
        Self::Purple,
        Self::PurpleHi,
        Self::Error,
        Self::EditorBg,
        Self::Bar,
        Self::OnAmber,
        Self::OnTeal,
        Self::Comment,
        Self::Ghost,
        Self::TealLine,
        Self::TealFill,
        Self::SlateHi,
        Self::TealRing,
        Self::GreenHalo,
        Self::AmberHalo,
        Self::ErrorHalo,
        Self::TableHead,
        Self::RowHover,
        Self::CardTab,
        Self::OnCardTab,
        Self::CodeBar,
        Self::NeutralLabel,
        Self::TealLabel,
        Self::AmberLabel,
        Self::NeutralLine,
        Self::AmberLine,
        Self::NeutralHaze,
        Self::TealHaze,
        Self::AmberHaze,
        Self::CurrentLine,
    ];

    /// The token's name in the spec and in theme files: `bg-raise`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Bg => "bg",
            Self::BgRaise => "bg-raise",
            Self::Panel => "panel",
            Self::PanelSolid => "panel-solid",
            Self::Slate => "slate",
            Self::Slate2 => "slate-2",
            Self::Line => "line",
            Self::Line2 => "line-2",
            Self::Text => "text",
            Self::Text2 => "text-2",
            Self::Text3 => "text-3",
            Self::Teal => "teal",
            Self::TealHi => "teal-hi",
            Self::TealWash => "teal-wash",
            Self::TealGlow => "teal-glow",
            Self::Amber => "amber",
            Self::AmberHi => "amber-hi",
            Self::Green => "green",
            Self::Rust => "rust",
            Self::Violet => "violet",
            Self::Bronze => "bronze",
            Self::BronzeHi => "bronze-hi",
            Self::Purple => "purple",
            Self::PurpleHi => "purple-hi",
            Self::Error => "error",
            Self::EditorBg => "editor-bg",
            Self::Bar => "bar",
            Self::OnAmber => "on-amber",
            Self::OnTeal => "on-teal",
            Self::Comment => "comment",
            Self::Ghost => "ghost",
            Self::TealLine => "teal-line",
            Self::TealFill => "teal-fill",
            Self::SlateHi => "slate-hi",
            Self::TealRing => "teal-ring",
            Self::GreenHalo => "green-halo",
            Self::AmberHalo => "amber-halo",
            Self::ErrorHalo => "error-halo",
            Self::TableHead => "table-head",
            Self::RowHover => "row-hover",
            Self::CardTab => "card-tab",
            Self::OnCardTab => "on-card-tab",
            Self::CodeBar => "code-bar",
            Self::NeutralLabel => "neutral-label",
            Self::TealLabel => "teal-label",
            Self::AmberLabel => "amber-label",
            Self::NeutralLine => "neutral-line",
            Self::AmberLine => "amber-line",
            Self::NeutralHaze => "neutral-haze",
            Self::TealHaze => "teal-haze",
            Self::AmberHaze => "amber-haze",
            Self::CurrentLine => "current-line",
        }
    }

    /// The token named `name`, if there is one.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|token| token.name() == name)
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// What a theme gives for one color token.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TokenColor {
    /// The color widgets paint with, translucent or not.
    pub value: Color,
    /// For a translucent value, the opaque color the theme gives for it
    /// over `bg`, if it gives one; never set for an opaque value.
    pub solid: Option<Color>,
    /// The 256-color index the terminal UI falls back to, if the theme
    /// gives one.
    pub ansi: Option<u8>,
}

/// A theme: a value for every [`Token`], the cut-corner size and the two
/// fonts, read from a theme file (see the [module documentation](self)).
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    name: String,
    colors: [TokenColor; Token::COUNT],
    cut: f32,
    font_ui: String,
    font_mono: String,
}

impl Theme {
    /// Reads a theme file, parsing it within `limits`.
    ///
    /// # Errors
    ///
    /// [`UiError::ThemeSyntax`] when the text is not TOML or breaks
    /// `limits`, and [`UiError::Theme`] when it is not a theme of
    /// [`VERSION`], as the [module documentation](self) describes.
    pub fn parse(text: &str, limits: Limits) -> Result<Self, UiError> {
        let root = toml::parse(text, limits).map_err(UiError::ThemeSyntax)?;
        Reader { text }.theme(&root)
    }

    /// The bundled `tidepool-dark` theme, jabbott.dev's tokens
    /// ([`TIDEPOOL_DARK_TOML`]), read again at each call.
    pub fn tidepool_dark() -> Self {
        Self::parse(TIDEPOOL_DARK_TOML, LIMITS)
            .expect("the bundled theme is valid: `ui`'s tests read it")
    }

    /// The bundled `tidepool-dark` theme, read once for the process and
    /// shared: what a new [`Tree`](crate::Tree) paints with (ADR-0049).
    ///
    /// ```
    /// use std::sync::Arc;
    /// use tidepool_ui::theme::Theme;
    ///
    /// assert!(Arc::ptr_eq(&Theme::bundled(), &Theme::bundled()));
    /// assert_eq!(*Theme::bundled(), Theme::tidepool_dark());
    /// ```
    pub fn bundled() -> Arc<Self> {
        static BUNDLED: OnceLock<Arc<Theme>> = OnceLock::new();
        Arc::clone(BUNDLED.get_or_init(|| Arc::new(Self::tidepool_dark())))
    }

    /// The theme's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What the theme gives for `token`.
    pub fn token(&self, token: Token) -> TokenColor {
        self.colors[token as usize]
    }

    /// The color to paint `token` with.
    pub fn color(&self, token: Token) -> Color {
        self.token(token).value
    }

    /// `token` as an opaque color over `bg`: its value when that is
    /// opaque; otherwise the solid color the theme gives, or, when it
    /// gives none (as for `teal-glow`), the value composited over `bg` by
    /// source-over.
    pub fn solid(&self, token: Token) -> Color {
        let entry = self.token(token);
        if entry.value.a == u8::MAX {
            return entry.value;
        }
        entry
            .solid
            .unwrap_or_else(|| over(entry.value, self.color(Token::Bg)))
    }

    /// The 256-color index of `token`, if the theme gives one.
    pub fn ansi(&self, token: Token) -> Option<u8> {
        self.token(token).ansi
    }

    /// The cut-corner size, in logical units: the site's `--cut`, which no
    /// widget paints with since buttons became tiles (ADR-0058).
    pub fn cut(&self) -> f32 {
        self.cut
    }

    /// The font family the theme names for `family`.
    pub fn font(&self, family: Family) -> &str {
        match family {
            Family::Ui => &self.font_ui,
            Family::Mono => &self.font_mono,
        }
    }
}

/// `top` composited over `below` by source-over, as `gfx` blends it.
fn over(top: Color, below: Color) -> Color {
    top.premultiply().over(below.premultiply()).unpremultiply()
}

/// Why a theme file is not a theme. See [`ThemeError`] for where.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ThemeErrorKind {
    /// A key the format requires is not there.
    Missing,
    /// A key the format does not have.
    Unknown,
    /// The value is of the wrong type.
    WrongType {
        /// What it should be, as `"a string"`.
        expected: &'static str,
    },
    /// `version` is not [`VERSION`].
    Version {
        /// The version given.
        found: i64,
    },
    /// A color does not parse.
    Color {
        /// The byte offset in the color's string where it failed.
        offset: usize,
        /// What was expected there.
        expected: &'static str,
    },
    /// `solid` is given for a value that is opaque.
    SolidForOpaque,
    /// `solid` is translucent.
    TranslucentSolid,
    /// `bg` is translucent, though translucent colors are composited over
    /// it.
    TranslucentBg,
    /// `ansi` is outside 0–255.
    Ansi {
        /// The index given.
        found: i64,
    },
    /// `cut` is not a finite number from 0 to [`MAX_CUT`].
    Cut,
    /// A name or font is blank (empty, or only white space), longer than
    /// [`MAX_TEXT_BYTES`], or not one line of plain text: it holds a
    /// control character, a line or paragraph separator, or a
    /// bidirectional control.
    Text,
}

/// A theme file that is TOML but not a theme: which key, where, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeError {
    /// The key, as a TOML dotted key from the file's root, such as
    /// `colors.teal.value`, with a name that is not a bare key quoted
    /// (`colors."teal.value"`). `Display` writes its control characters,
    /// line and paragraph separators and bidirectional controls as
    /// escapes (`\u{1b}`), since it may come from the file.
    pub key: String,
    /// The line of the key's value, from 1, or of the key itself when it
    /// is unknown; `None` when the key is missing.
    pub line: Option<usize>,
    /// What is wrong.
    pub kind: ThemeErrorKind,
}

impl fmt::Display for ThemeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "theme line {line}: ")?,
            None => f.write_str("theme: ")?,
        }
        f.write_str("`")?;
        for c in self.key.chars() {
            if is_escaped(c) {
                write!(f, "{}", c.escape_default())?;
            } else {
                write!(f, "{c}")?;
            }
        }
        f.write_str("` ")?;
        match self.kind {
            ThemeErrorKind::Missing => f.write_str("is missing"),
            ThemeErrorKind::Unknown => f.write_str("is not a key of the theme format"),
            ThemeErrorKind::WrongType { expected } => write!(f, "should be {expected}"),
            ThemeErrorKind::Version { found } => {
                write!(f, "is {found}, but this reader reads version {VERSION}")
            }
            ThemeErrorKind::Color { offset, expected } => {
                write!(f, "is not a color: expected {expected} at byte {offset}")
            }
            ThemeErrorKind::SolidForOpaque => f.write_str(
                "is given for an opaque value; only a translucent one has a solid color",
            ),
            ThemeErrorKind::TranslucentSolid => f.write_str("should be opaque"),
            ThemeErrorKind::TranslucentBg => {
                f.write_str("should be opaque: translucent colors are composited over it")
            }
            ThemeErrorKind::Ansi { found } => {
                write!(f, "is {found}, but a 256-color index is from 0 to 255")
            }
            ThemeErrorKind::Cut => write!(f, "should be a finite number from 0 to {MAX_CUT}"),
            ThemeErrorKind::Text => write!(
                f,
                "should be one line of plain text that is not blank, of at most \
                 {MAX_TEXT_BYTES} bytes"
            ),
        }
    }
}

/// The keys a theme file's root may have.
const ROOT_KEYS: &[&str] = &["version", "name", "colors", "sizes", "fonts", "contrast"];

/// The keys a color token's table may have.
const COLOR_KEYS: &[&str] = &["value", "solid", "ansi"];

/// Reads a parsed theme file, knowing its text for the lines of errors.
struct Reader<'a> {
    text: &'a str,
}

impl Reader<'_> {
    fn theme(&self, root: &Table) -> Result<Theme, UiError> {
        self.only(root, "", ROOT_KEYS)?;
        let version = self.get(root, "", "version")?;
        match version.as_integer() {
            Some(VERSION) => {}
            Some(found) => {
                return Err(self.error(
                    "version",
                    Some(version.span),
                    ThemeErrorKind::Version { found },
                ));
            }
            None => return Err(self.wrong("version", version, "an integer")),
        }
        let name = self.text_value(root, "", "name")?;

        let colors = self.table(root, "", "colors")?;
        for (key, _) in colors.entries() {
            if Token::from_name(&key.name).is_none() {
                return Err(self.unknown("colors", key.name.as_str(), key.span));
            }
        }
        let mut read = Vec::with_capacity(Token::COUNT);
        for token in Token::ALL {
            read.push(self.color_token(colors, token)?);
        }
        let read: [TokenColor; Token::COUNT] =
            read.try_into().expect("one entry was read for each token");
        if read[Token::Bg as usize].value.a != u8::MAX {
            let at = colors
                .get("bg")
                .and_then(Value::as_table)
                .and_then(|bg| bg.get("value"))
                .map(|value| value.span);
            return Err(self.error("colors.bg.value", at, ThemeErrorKind::TranslucentBg));
        }

        let sizes = self.table(root, "", "sizes")?;
        self.only(sizes, "sizes", &["cut"])?;
        let cut = self.cut(sizes)?;

        let fonts = self.table(root, "", "fonts")?;
        self.only(fonts, "fonts", &["font-ui", "font-mono"])?;
        let font_ui = self.text_value(fonts, "fonts", "font-ui")?;
        let font_mono = self.text_value(fonts, "fonts", "font-mono")?;

        if let Some(contrast) = root.get("contrast")
            && contrast.as_table().is_none()
        {
            return Err(self.wrong("contrast", contrast, "a table"));
        }

        Ok(Theme {
            name,
            colors: read,
            cut,
            font_ui,
            font_mono,
        })
    }

    /// Reads `colors.<token>`.
    fn color_token(&self, colors: &Table, token: Token) -> Result<TokenColor, UiError> {
        let path = join("colors", token.name());
        let table = self.table(colors, "colors", token.name())?;
        self.only(table, &path, COLOR_KEYS)?;
        let value = self.color(table, &path, "value")?;
        let solid = match table.get("solid") {
            None => None,
            Some(_) => {
                let solid = self.color(table, &path, "solid")?;
                let at = table.get("solid").map(|solid| solid.span);
                if value.a == u8::MAX {
                    let key = join(&path, "solid");
                    return Err(self.error(&key, at, ThemeErrorKind::SolidForOpaque));
                }
                if solid.a != u8::MAX {
                    let key = join(&path, "solid");
                    return Err(self.error(&key, at, ThemeErrorKind::TranslucentSolid));
                }
                Some(solid)
            }
        };
        let ansi = match table.get("ansi") {
            None => None,
            Some(ansi) => {
                let key = join(&path, "ansi");
                let Some(found) = ansi.as_integer() else {
                    return Err(self.wrong(&key, ansi, "an integer"));
                };
                match u8::try_from(found) {
                    Ok(index) => Some(index),
                    Err(_) => {
                        let kind = ThemeErrorKind::Ansi { found };
                        return Err(self.error(&key, Some(ansi.span), kind));
                    }
                }
            }
        };
        Ok(TokenColor { value, solid, ansi })
    }

    /// Reads `sizes.cut`.
    fn cut(&self, sizes: &Table) -> Result<f32, UiError> {
        let cut = self.get(sizes, "sizes", "cut")?;
        let value = match cut.kind {
            // Exact: the range checked below is far inside `f32`'s.
            ValueKind::Integer(cut) if (0..=MAX_CUT as i64).contains(&cut) => cut as f32,
            ValueKind::Float(cut) if (0.0..=f64::from(MAX_CUT)).contains(&cut) => cut as f32,
            ValueKind::Integer(_) | ValueKind::Float(_) => {
                return Err(self.error("sizes.cut", Some(cut.span), ThemeErrorKind::Cut));
            }
            _ => return Err(self.wrong("sizes.cut", cut, "a number")),
        };
        Ok(value)
    }

    /// Reads a color at `path.name`.
    fn color(&self, table: &Table, path: &str, name: &str) -> Result<Color, UiError> {
        let value = self.get(table, path, name)?;
        let Some(text) = value.as_str() else {
            return Err(self.wrong(&join(path, name), value, "a string"));
        };
        Color::parse(text).map_err(|error| {
            let kind = match error {
                tidepool_core::error::CoreError::InvalidColor { offset, expected } => {
                    ThemeErrorKind::Color { offset, expected }
                }
                // `Color::parse` returns no other error; this keeps the
                // reader total if it ever does.
                _ => ThemeErrorKind::Color {
                    offset: 0,
                    expected: "a color",
                },
            };
            self.error(&join(path, name), Some(value.span), kind)
        })
    }

    /// Reads a name or a font at `path.name`.
    fn text_value(&self, table: &Table, path: &str, name: &str) -> Result<String, UiError> {
        let value = self.get(table, path, name)?;
        let Some(text) = value.as_str() else {
            return Err(self.wrong(&join(path, name), value, "a string"));
        };
        if !is_plain_line(text) {
            return Err(self.error(&join(path, name), Some(value.span), ThemeErrorKind::Text));
        }
        Ok(text.to_owned())
    }

    /// The table at `path.name`.
    fn table<'t>(&self, table: &'t Table, path: &str, name: &str) -> Result<&'t Table, UiError> {
        let value = self.get(table, path, name)?;
        value
            .as_table()
            .ok_or_else(|| self.wrong(&join(path, name), value, "a table"))
    }

    /// The value at `path.name`, which the format requires.
    fn get<'t>(&self, table: &'t Table, path: &str, name: &str) -> Result<&'t Value, UiError> {
        table
            .get(name)
            .ok_or_else(|| self.error(&join(path, name), None, ThemeErrorKind::Missing))
    }

    /// Refuses a key of `table`, at `path`, that is not in `allowed`.
    fn only(&self, table: &Table, path: &str, allowed: &[&str]) -> Result<(), UiError> {
        match table
            .entries()
            .iter()
            .find(|(key, _)| !allowed.contains(&key.name.as_str()))
        {
            Some((key, _)) => Err(self.unknown(path, &key.name, key.span)),
            None => Ok(()),
        }
    }

    fn unknown(&self, path: &str, name: &str, at: Span) -> UiError {
        self.error(&join(path, name), Some(at), ThemeErrorKind::Unknown)
    }

    fn wrong(&self, key: &str, value: &Value, expected: &'static str) -> UiError {
        self.error(
            key,
            Some(value.span),
            ThemeErrorKind::WrongType { expected },
        )
    }

    fn error(&self, key: &str, at: Option<Span>, kind: ThemeErrorKind) -> UiError {
        UiError::Theme(ThemeError {
            key: key.to_owned(),
            line: at.map(|span| self.line(span.start)),
            kind,
        })
    }

    /// The line of byte `offset`, from 1.
    fn line(&self, offset: usize) -> usize {
        let before = self
            .text
            .as_bytes()
            .get(..offset)
            .unwrap_or(self.text.as_bytes());
        before.iter().filter(|&&byte| byte == b'\n').count() + 1
    }
}

/// `path.name`, or `name` at the root, as a TOML dotted key: a name that
/// is not a bare key (ASCII letters, digits, `-` and `_`) is quoted, so
/// that a quoted `"teal.value"` under `[colors]` shows as
/// `colors."teal.value"`, not as the format's `colors.teal.value`.
fn join(path: &str, name: &str) -> String {
    let bare = !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    let part = if bare {
        name.to_owned()
    } else {
        let mut quoted = String::with_capacity(name.len() + 2);
        quoted.push('"');
        for c in name.chars() {
            if matches!(c, '"' | '\\') {
                quoted.push('\\');
            }
            quoted.push(c);
        }
        quoted.push('"');
        quoted
    };
    if path.is_empty() {
        part
    } else {
        format!("{path}.{part}")
    }
}

/// Whether `text` is 1 to [`MAX_TEXT_BYTES`] bytes of one line of plain
/// text that is not blank: something but white space, and no control
/// character, line or paragraph separator, or bidirectional control.
fn is_plain_line(text: &str) -> bool {
    text.len() <= MAX_TEXT_BYTES
        && !text.chars().all(char::is_whitespace)
        && !text.chars().any(is_escaped)
}

/// Whether `c` is a control character, a line or paragraph separator, or a
/// bidirectional control: what a name may not hold, and what
/// [`ThemeError`]'s `Display` writes as an escape.
fn is_escaped(c: char) -> bool {
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

    /// The spec's token table: each row's name and its value, solid and
    /// 256-color cells, `—` where it gives none.
    fn spec_rows() -> Vec<(String, String, String, String)> {
        let spec = include_str!("../../../docs/subsystems/ui-and-theme.md");
        let table = spec
            .split("## Tokens")
            .nth(1)
            .and_then(|rest| rest.split("\n## ").next())
            .expect("the spec has a Tokens section");
        table
            .lines()
            .filter(|line| line.starts_with("| `"))
            .map(|line| {
                let cells: Vec<String> = line
                    .split('|')
                    .map(|cell| cell.trim().trim_matches('`').to_owned())
                    .collect();
                (
                    cells[1].clone(),
                    cells[2].clone(),
                    cells[3].clone(),
                    cells[4].clone(),
                )
            })
            .collect()
    }

    #[test]
    fn the_bundled_theme_is_the_spec_token_table() {
        let theme = Theme::tidepool_dark();
        let rows = spec_rows();
        let mut seen = std::collections::BTreeSet::new();
        for (name, value, solid, ansi) in &rows {
            match name.as_str() {
                "cut" => assert_eq!(value, &format!("{} px", theme.cut())),
                "font-ui" => assert_eq!(value, theme.font(Family::Ui)),
                "font-mono" => assert_eq!(value, theme.font(Family::Mono)),
                _ => {
                    let token = Token::from_name(name)
                        .unwrap_or_else(|| panic!("the spec's `{name}` is no token"));
                    let entry = theme.token(token);
                    assert_eq!(Color::parse(value), Ok(entry.value), "`{name}`'s value");
                    let solid = (solid != "—").then(|| Color::parse(solid).expect("a color"));
                    assert_eq!(solid, entry.solid, "`{name}`'s solid");
                    let ansi = (ansi != "—").then(|| ansi.parse::<u8>().expect("an index"));
                    assert_eq!(ansi, entry.ansi, "`{name}`'s 256-color index");
                    assert!(seen.insert(token), "the spec lists `{name}` twice");
                }
            }
        }
        let all: std::collections::BTreeSet<Token> = Token::ALL.into_iter().collect();
        assert_eq!(seen, all, "the spec lists every token once");
        assert_eq!(rows.len(), Token::COUNT + 3);
    }

    #[test]
    fn names_round_trip_and_are_unique() {
        for (index, token) in Token::ALL.into_iter().enumerate() {
            assert_eq!(token as usize, index);
            assert_eq!(Token::from_name(token.name()), Some(token));
            assert_eq!(token.to_string(), token.name());
        }
        assert_eq!(Token::from_name("Bg"), None);
        assert_eq!(Token::from_name(""), None);
    }

    #[test]
    fn solid_is_the_value_the_written_solid_or_the_composite() {
        let theme = Theme::tidepool_dark();
        // Opaque: the value.
        assert_eq!(theme.solid(Token::Slate), theme.color(Token::Slate));
        // Translucent with a solid written: that solid, which for `line`
        // is the spec's #1f1f20, one level from the 8-bit composite.
        assert_eq!(theme.solid(Token::Line), Color::rgb(0x1f, 0x1f, 0x20));
        assert_eq!(
            over(theme.color(Token::Line), theme.color(Token::Bg)),
            Color::rgb(0x1e, 0x1f, 0x20)
        );
        // Translucent with none: composited over `bg`.
        assert_eq!(
            theme.solid(Token::TealGlow),
            over(theme.color(Token::TealGlow), theme.color(Token::Bg))
        );
        // The editor's selection, `teal-glow` over `editor-bg`.
        assert_eq!(
            over(theme.color(Token::TealGlow), theme.color(Token::EditorBg)),
            Color::rgb(0x27, 0x51, 0x52)
        );
        // The editor's current line, `current-line` over `editor-bg`, the
        // surface `cargo xtask theme` checks its code and line number on.
        assert_eq!(
            over(
                theme.color(Token::CurrentLine),
                theme.color(Token::EditorBg)
            ),
            Color::rgb(0x10, 0x15, 0x17)
        );
    }

    /// The bundled file with `from` replaced by `to`, which must be there.
    fn edited(from: &str, to: &str) -> String {
        assert!(TIDEPOOL_DARK_TOML.contains(from), "the file has {from:?}");
        TIDEPOOL_DARK_TOML.replacen(from, to, 1)
    }

    fn theme_error(text: &str) -> ThemeError {
        match Theme::parse(text, LIMITS) {
            Err(UiError::Theme(error)) => error,
            other => panic!("expected a theme error, got {other:?}"),
        }
    }

    /// The line of the first `needle` in the bundled file.
    fn line_of(needle: &str) -> usize {
        let at = TIDEPOOL_DARK_TOML.find(needle).expect("the file has it");
        TIDEPOOL_DARK_TOML[..at].matches('\n').count() + 1
    }

    #[test]
    fn each_rule_is_refused_naming_the_key_and_line() {
        let teal = "teal = { value = \"#5ccbcb\", ansi = 80 }";
        let cases: Vec<(String, &str, Option<usize>, ThemeErrorKind)> = vec![
            (
                edited("version = 1", "version = 2"),
                "version",
                Some(line_of("version = 1")),
                ThemeErrorKind::Version { found: 2 },
            ),
            (
                edited("version = 1", "version = \"1\""),
                "version",
                Some(line_of("version = 1")),
                ThemeErrorKind::WrongType {
                    expected: "an integer",
                },
            ),
            (
                edited("version = 1\n", ""),
                "version",
                None,
                ThemeErrorKind::Missing,
            ),
            (
                edited("name = \"tidepool-dark\"", "name = \"\""),
                "name",
                Some(line_of("name = \"tidepool-dark\"")),
                ThemeErrorKind::Text,
            ),
            (
                edited("name = \"tidepool-dark\"", "name = \"a\\nb\""),
                "name",
                Some(line_of("name = \"tidepool-dark\"")),
                ThemeErrorKind::Text,
            ),
            (
                edited("name = \"tidepool-dark\"", "name = \"a\\u202Eb\""),
                "name",
                Some(line_of("name = \"tidepool-dark\"")),
                ThemeErrorKind::Text,
            ),
            // Blank: only white space, a no-break space included.
            (
                edited("name = \"tidepool-dark\"", "name = \"   \""),
                "name",
                Some(line_of("name = \"tidepool-dark\"")),
                ThemeErrorKind::Text,
            ),
            (
                edited("font-ui = \"Saira\"", "font-ui = \"\\u00A0\""),
                "fonts.font-ui",
                Some(line_of("font-ui = \"Saira\"")),
                ThemeErrorKind::Text,
            ),
            // One byte over the length.
            (
                edited(
                    "name = \"tidepool-dark\"",
                    &format!("name = \"{}\"", "n".repeat(MAX_TEXT_BYTES + 1)),
                ),
                "name",
                Some(line_of("name = \"tidepool-dark\"")),
                ThemeErrorKind::Text,
            ),
            // A line separator, a paragraph separator and the other
            // bidirectional controls.
            (
                edited(
                    "font-mono = \"JetBrains Mono\"",
                    "font-mono = \"a\\u2028b\"",
                ),
                "fonts.font-mono",
                Some(line_of("font-mono = \"JetBrains Mono\"")),
                ThemeErrorKind::Text,
            ),
            (
                edited(
                    "font-mono = \"JetBrains Mono\"",
                    "font-mono = \"a\\u2029b\"",
                ),
                "fonts.font-mono",
                Some(line_of("font-mono = \"JetBrains Mono\"")),
                ThemeErrorKind::Text,
            ),
            (
                edited(
                    "font-mono = \"JetBrains Mono\"",
                    "font-mono = \"a\\u2067b\"",
                ),
                "fonts.font-mono",
                Some(line_of("font-mono = \"JetBrains Mono\"")),
                ThemeErrorKind::Text,
            ),
            (
                edited(
                    "font-mono = \"JetBrains Mono\"",
                    "font-mono = \"a\\u200Fb\"",
                ),
                "fonts.font-mono",
                Some(line_of("font-mono = \"JetBrains Mono\"")),
                ThemeErrorKind::Text,
            ),
            // A quoted key that is not bare is quoted in the path, so it
            // is not taken for the format's `colors.teal.value`.
            (
                edited(teal, &format!("{teal}\n\"teal.value\" = 1")),
                "colors.\"teal.value\"",
                Some(line_of(teal) + 1),
                ThemeErrorKind::Unknown,
            ),
            (
                edited(teal, "tael = { value = \"#5ccbcb\" }"),
                "colors.tael",
                Some(line_of(teal)),
                ThemeErrorKind::Unknown,
            ),
            (
                edited(&format!("{teal}\n"), ""),
                "colors.teal",
                None,
                ThemeErrorKind::Missing,
            ),
            (
                edited(teal, "teal = \"#5ccbcb\""),
                "colors.teal",
                Some(line_of(teal)),
                ThemeErrorKind::WrongType {
                    expected: "a table",
                },
            ),
            (
                edited(teal, "teal = { value = \"#5ccbcg\" }"),
                "colors.teal.value",
                Some(line_of(teal)),
                ThemeErrorKind::Color {
                    offset: 6,
                    expected: "6 or 8 hex digits",
                },
            ),
            (
                edited(teal, "teal = { value = \"#5ccbcb\", solid = \"#5ccbcb\" }"),
                "colors.teal.solid",
                Some(line_of(teal)),
                ThemeErrorKind::SolidForOpaque,
            ),
            (
                edited("solid = \"#0f1113\"", "solid = \"#0f111380\""),
                "colors.panel.solid",
                Some(line_of("solid = \"#0f1113\"")),
                ThemeErrorKind::TranslucentSolid,
            ),
            (
                edited(teal, "teal = { value = \"#5ccbcb\", ansi = 256 }"),
                "colors.teal.ansi",
                Some(line_of(teal)),
                ThemeErrorKind::Ansi { found: 256 },
            ),
            (
                edited(teal, "teal = { value = \"#5ccbcb\", ansi = -1 }"),
                "colors.teal.ansi",
                Some(line_of(teal)),
                ThemeErrorKind::Ansi { found: -1 },
            ),
            (
                edited(teal, "teal = { value = \"#5ccbcb\", ansi = \"80\" }"),
                "colors.teal.ansi",
                Some(line_of(teal)),
                ThemeErrorKind::WrongType {
                    expected: "an integer",
                },
            ),
            (
                edited(teal, "teal = { value = \"#5ccbcb\", hue = 1 }"),
                "colors.teal.hue",
                Some(line_of(teal)),
                ThemeErrorKind::Unknown,
            ),
            (
                edited(
                    "bg = { value = \"#0b0c0d\", ansi = 232 }",
                    "bg = { value = \"#0b0c0d80\" }",
                ),
                "colors.bg.value",
                Some(line_of("bg = { value = \"#0b0c0d\"")),
                ThemeErrorKind::TranslucentBg,
            ),
            (
                edited("cut = 10", "cut = 65"),
                "sizes.cut",
                Some(line_of("cut = 10")),
                ThemeErrorKind::Cut,
            ),
            (
                edited("cut = 10", "cut = nan"),
                "sizes.cut",
                Some(line_of("cut = 10")),
                ThemeErrorKind::Cut,
            ),
            (
                edited("cut = 10", "cut = -0.5"),
                "sizes.cut",
                Some(line_of("cut = 10")),
                ThemeErrorKind::Cut,
            ),
            (
                edited("cut = 10", "cut = \"10\""),
                "sizes.cut",
                Some(line_of("cut = 10")),
                ThemeErrorKind::WrongType {
                    expected: "a number",
                },
            ),
            (
                edited("font-mono = \"JetBrains Mono\"", "font-mono = 1"),
                "fonts.font-mono",
                Some(line_of("font-mono = \"JetBrains Mono\"")),
                ThemeErrorKind::WrongType {
                    expected: "a string",
                },
            ),
            (
                edited("version = 1", "version = 1\nshade = 1"),
                "shade",
                Some(line_of("version = 1") + 1),
                ThemeErrorKind::Unknown,
            ),
            (
                {
                    let text = edited("version = 1", "version = 1\ncontrast = 1");
                    let end = text.find("\n[contrast]\n").expect("the file has the table");
                    text[..end].to_owned()
                },
                "contrast",
                Some(line_of("version = 1") + 1),
                ThemeErrorKind::WrongType {
                    expected: "a table",
                },
            ),
        ];
        for (text, key, line, kind) in cases {
            let error = theme_error(&text);
            assert_eq!(
                (error.key.as_str(), error.line, error.kind),
                (key, line, kind),
                "{error}"
            );
        }
    }

    #[test]
    fn what_the_format_allows_is_read() {
        // A float `cut`, and no `[contrast]`.
        let text = edited("cut = 10", "cut = 12.5");
        let cut = text.find("\n[contrast]\n").expect("the file has the table");
        let theme = Theme::parse(&text[..cut], LIMITS).expect("a theme");
        assert_eq!(theme.cut(), 12.5);
        // A name of exactly the most bytes, with white space inside it.
        let longest = format!("{} x", "n".repeat(MAX_TEXT_BYTES - 2));
        let text = edited("name = \"tidepool-dark\"", &format!("name = \"{longest}\""));
        assert_eq!(
            Theme::parse(&text, LIMITS).map(|theme| theme.name().to_owned()),
            Ok(longest)
        );
        // The edges of `cut` and `ansi`.
        for (from, to) in [("cut = 10", "cut = 0"), ("cut = 10", "cut = 64.0")] {
            assert!(Theme::parse(&edited(from, to), LIMITS).is_ok(), "{to}");
        }
        let teal = "teal = { value = \"#5ccbcb\", ansi = 80 }";
        let text = edited(teal, "teal = { value = \"#5ccbcb\", ansi = 255 }");
        assert_eq!(
            Theme::parse(&text, LIMITS).map(|theme| theme.ansi(Token::Teal)),
            Ok(Some(255))
        );
        // A translucent token with no solid, and one with no index.
        let theme = Theme::tidepool_dark();
        assert_eq!(theme.token(Token::TealGlow).solid, None);
        assert_eq!(theme.ansi(Token::TealGlow), None);
    }

    #[test]
    fn syntax_and_limits_are_codecs_errors() {
        assert!(matches!(
            Theme::parse("version = ", LIMITS),
            Err(UiError::ThemeSyntax(_))
        ));
        let small = Limits {
            max_input_bytes: 16,
            ..LIMITS
        };
        assert!(matches!(
            Theme::parse(TIDEPOOL_DARK_TOML, small),
            Err(UiError::ThemeSyntax(_))
        ));
    }

    #[test]
    fn messages_name_the_key_and_escape_it() {
        let cases = [
            (
                ThemeError {
                    key: "colors.teal".into(),
                    line: None,
                    kind: ThemeErrorKind::Missing,
                },
                "theme: `colors.teal` is missing",
            ),
            (
                ThemeError {
                    key: "colors.t\u{1b}[31m".into(),
                    line: Some(4),
                    kind: ThemeErrorKind::Unknown,
                },
                "theme line 4: `colors.t\\u{1b}[31m` is not a key of the theme format",
            ),
            (
                ThemeError {
                    key: "colors.\"teal.value\"".into(),
                    line: Some(32),
                    kind: ThemeErrorKind::Unknown,
                },
                "theme line 32: `colors.\"teal.value\"` is not a key of the theme format",
            ),
            (
                ThemeError {
                    key: "a\u{202e}b\u{2028}".into(),
                    line: None,
                    kind: ThemeErrorKind::Missing,
                },
                "theme: `a\\u{202e}b\\u{2028}` is missing",
            ),
            (
                ThemeError {
                    key: "version".into(),
                    line: Some(1),
                    kind: ThemeErrorKind::Version { found: 2 },
                },
                "theme line 1: `version` is 2, but this reader reads version 1",
            ),
            (
                ThemeError {
                    key: "colors.bg.value".into(),
                    line: Some(20),
                    kind: ThemeErrorKind::Color {
                        offset: 6,
                        expected: "a hex digit",
                    },
                },
                "theme line 20: `colors.bg.value` is not a color: expected a hex digit at byte 6",
            ),
            (
                ThemeError {
                    key: "colors.bg.ansi".into(),
                    line: Some(20),
                    kind: ThemeErrorKind::Ansi { found: 300 },
                },
                "theme line 20: `colors.bg.ansi` is 300, but a 256-color index is from 0 to 255",
            ),
            (
                ThemeError {
                    key: "sizes.cut".into(),
                    line: Some(9),
                    kind: ThemeErrorKind::Cut,
                },
                "theme line 9: `sizes.cut` should be a finite number from 0 to 64",
            ),
            (
                ThemeError {
                    key: "name".into(),
                    line: Some(2),
                    kind: ThemeErrorKind::Text,
                },
                "theme line 2: `name` should be one line of plain text that is not blank, of at \
                 most 64 bytes",
            ),
        ];
        for (error, message) in cases {
            assert_eq!(error.to_string(), message);
            assert_eq!(UiError::Theme(error).to_string(), message);
        }
    }
}
