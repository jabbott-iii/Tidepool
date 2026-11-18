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

//! TrueType fonts: [`Font::parse`] checks a file's tables, and [`Font`]
//! then maps characters to glyphs, gives metrics, names and kerning, and
//! reads each glyph's outline.
//!
//! New in Tidepool; not ported (Nadir draws text with OpenGL).
//!
//! ```
//! # fn main() -> Result<(), tidepool_font::FontError> {
//! use tidepool_font::Limits;
//! use tidepool_font::ttf::{Font, NameId};
//!
//! let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts/saira/Saira-Regular.ttf");
//! let font = Font::parse(std::fs::read(path).expect("the bundled font"), Limits::DEFAULT)?;
//! assert_eq!(font.units_per_em(), 1000);
//! assert_eq!(font.name(NameId::FAMILY).as_deref(), Some("Saira"));
//! let a = font.glyph_index('A').expect("Saira has an A");
//! assert!(font.h_metrics(a).is_some_and(|metrics| metrics.advance > 0));
//! assert!(!font.outline(a)?.is_empty());
//! # Ok(())
//! # }
//! ```
//!
//! # What is read
//!
//! The file is an sfnt with TrueType outlines: version `0x00010000` or
//! `true`. CFF outlines (`OTTO`) and collections (`ttcf`) are refused,
//! each with its own [`ErrorKind`]. Of its tables:
//!
//! - **Required:** `head`, `hhea`, `maxp`, `hmtx`, `cmap`, `loca` and
//!   `glyf`.
//! - **Read if present:** `kern`, `GPOS`, `OS/2` and `name`. A part of
//!   one of these that is read and malformed fails the whole font; a part
//!   that is not read is not checked. Their directory records are always
//!   checked (each lies inside the file, and is listed once): `kern`'s too
//!   when `GPOS` supplies the kerning, though its contents are then not
//!   read. Of `GPOS` only what [`Font::kerning`] names is read.
//! - **Ignored:** every other, among them the hinting tables (`fpgm`,
//!   `prep`, `cvt `, `gasp`), `post`, `GSUB`, `GDEF`, `DSIG`, and `fvar`
//!   and `gvar`: a variable font reads as its default instance.
//!
//! Table checksums are not verified: a checksum catches a damaged
//! download, and every read is bounds-checked anyway. Glyphs are never
//! hinted: no instructions are run.
//!
//! - `head`: major version 1, the magic number, `unitsPerEm` from 16 to
//!   16,384, the bounding box, `indexToLocFormat` 0 or 1 and
//!   `glyphDataFormat` 0.
//! - `maxp` version 1.0 (`0x00010000`, 32 bytes; 0.5 is for CFF) with at
//!   least one glyph.
//! - `hhea` major version 1: ascender, descender, line gap, the widest
//!   advance and `numberOfHMetrics`, from 1 to the glyph count. `hmtx`
//!   holds that many advances and left side bearings, then a bearing for
//!   each glyph after them, which takes the last advance.
//! - `loca` holds an offset for each glyph and one more; they never go
//!   down, nor past the end of `glyf`. A glyph whose offsets are equal is
//!   empty.
//! - `cmap` version 0, through one subtable: format 12 under platform 3,
//!   encoding 10, or platform 0; else format 4 under platform 3, encoding
//!   1, or platform 0. The first record of the best of the two in the
//!   table's order is used, and no such record is an error. A format 14
//!   (variation sequences) or symbol subtable is never used.
//! - `OS/2`, any version, long enough for its version's fields: version 1
//!   86 bytes, 2 to 4 96, 5 and later 100. Version 0 needs 68 (through
//!   `usLastCharIndex`, the length of some legacy fonts' tables), and its
//!   line metrics are read only when it has its full 78: see [`Os2`].
//! - `name` format 0 or 1: records of platform 3 (Windows) with encoding 1
//!   or 10, or of platform 0 (Unicode), in UTF-16BE. Records of platform 1
//!   (Macintosh) and of any other platform or encoding are ignored. See
//!   [`Font::name`].
//! - `glyf`: simple and composite glyphs; see [`Font::outline`].
//! - Kerning: see [`Font::kerning`].
//!
//! # Checked when parsed, or when looked up
//!
//! [`Font::parse`] checks everything the lookups rely on, so that a lookup
//! cannot go wrong afterwards; what it cannot check without reading every
//! glyph is checked by [`Font::outline`], for the glyph asked for. Every
//! check is made before the memory it guards is allocated, and against the
//! [`Limits`] the font was parsed with.
//!
//! When parsed:
//!
//! - the file's size against [`Limits::max_file_bytes`], first;
//! - the table directory: its records fit in the file; each table that
//!   could be read (the required ones, and `kern`, `GPOS`, `OS/2` and
//!   `name`, `kern` even when `GPOS` supplies the kerning) lies inside the
//!   file, and is listed once (the ranges of other tables are not checked,
//!   and a duplicate of one is allowed);
//! - each field above that is checked, and each table's length against
//!   what it must hold;
//! - `loca`'s offsets, all of them;
//! - the `cmap` subtable used: for format 4, `segCountX2` even, the four
//!   arrays inside its length, which lies inside the table, and the
//!   segments' end codes ascending; for format 12, its groups inside its
//!   length and the table, each with a start no later than its end, and
//!   in ascending order without overlapping;
//! - `name`: every record inside the table, and the string of each record
//!   that could be read;
//! - kerning: every structure a kerning lookup reads, as [`Font::kerning`]
//!   says, and nothing else: of the `kern` table only its directory record
//!   when `GPOS` supplies the kerning.
//!
//! When looked up, and not an error:
//!
//! - in `cmap` format 4, a glyph that an `idRangeOffset` finds outside the
//!   subtable, and in either format, a glyph id that the arithmetic makes
//!   that is not below the glyph count: [`Font::glyph_index`] gives `None`,
//!   as for a character that is not mapped;
//! - kerning's class and coverage indices past the end of their arrays:
//!   the subtable does not apply ([`Font::kerning`]).
//!
//! When read, by [`Font::outline`]: the glyph's data, its points and
//! components against the limits.

use std::borrow::Cow;
use std::fmt;

use crate::outline::Outline;
use crate::{FontError, Limits};

mod cmap;
mod glyf;
mod gpos;
mod kern;
mod name;
mod read;
mod tables;

use self::cmap::Cmap;
use self::glyf::Glyphs;
use self::gpos::Gpos;
use self::kern::KernTable;
use self::name::Names;
use self::read::{Place, Span};
use self::tables::{Head, Hmtx};

/// A four-byte table or script tag, such as `head` or `DFLT`.
///
/// `Display` writes printable ASCII other than `\` as it is, and any other
/// byte, `\` included, as `\xNN`; `Debug` writes `Tag("head")` the same
/// way.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tag(pub [u8; 4]);

impl Tag {
    /// The font header.
    pub const HEAD: Self = Self(*b"head");
    /// The horizontal header.
    pub const HHEA: Self = Self(*b"hhea");
    /// The maximum profile, with the glyph count.
    pub const MAXP: Self = Self(*b"maxp");
    /// The horizontal metrics.
    pub const HMTX: Self = Self(*b"hmtx");
    /// The character-to-glyph map.
    pub const CMAP: Self = Self(*b"cmap");
    /// The index to the glyphs' data.
    pub const LOCA: Self = Self(*b"loca");
    /// The glyphs' data.
    pub const GLYF: Self = Self(*b"glyf");
    /// The kerning table.
    pub const KERN: Self = Self(*b"kern");
    /// Glyph positioning, whose `kern` feature is read.
    pub const GPOS: Self = Self(*b"GPOS");
    /// OS/2 and Windows metrics.
    pub const OS2: Self = Self(*b"OS/2");
    /// The naming table.
    pub const NAME: Self = Self(*b"name");
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for &byte in &self.0 {
            if (0x20..0x7f).contains(&byte) && byte != b'\\' {
                write!(f, "{}", char::from(byte))?;
            } else {
                write!(f, "\\x{byte:02x}")?;
            }
        }
        Ok(())
    }
}

impl fmt::Debug for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Tag(\"{self}\")")
    }
}

/// A glyph's index in its font, from 0 to one less than
/// [`Font::glyph_count`]. Glyph 0 is `.notdef`, drawn for characters the
/// font lacks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GlyphId(pub u16);

impl GlyphId {
    /// Glyph 0, `.notdef`.
    pub const NOTDEF: Self = Self(0);
}

/// A string's name ID in the `name` table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NameId(pub u16);

impl NameId {
    /// The copyright notice.
    pub const COPYRIGHT: Self = Self(0);
    /// The family name, as grouped in up to four styles (regular, italic,
    /// bold, bold italic): "Saira Medium" for a medium weight.
    pub const FAMILY: Self = Self(1);
    /// The subfamily (style) name within [`NameId::FAMILY`].
    pub const SUBFAMILY: Self = Self(2);
    /// A unique identifier of the font.
    pub const UNIQUE_ID: Self = Self(3);
    /// The full name.
    pub const FULL_NAME: Self = Self(4);
    /// The version string.
    pub const VERSION: Self = Self(5);
    /// The PostScript name.
    pub const POSTSCRIPT_NAME: Self = Self(6);
    /// The trademark notice.
    pub const TRADEMARK: Self = Self(7);
    /// The manufacturer's name.
    pub const MANUFACTURER: Self = Self(8);
    /// The designer's name.
    pub const DESIGNER: Self = Self(9);
    /// A description.
    pub const DESCRIPTION: Self = Self(10);
    /// The vendor's URL.
    pub const VENDOR_URL: Self = Self(11);
    /// The designer's URL.
    pub const DESIGNER_URL: Self = Self(12);
    /// The license description.
    pub const LICENSE: Self = Self(13);
    /// The license's URL.
    pub const LICENSE_URL: Self = Self(14);
    /// The typographic family name, for families of more than four styles:
    /// "Saira" for every weight. Absent when it is [`NameId::FAMILY`].
    pub const TYPOGRAPHIC_FAMILY: Self = Self(16);
    /// The typographic subfamily name within
    /// [`NameId::TYPOGRAPHIC_FAMILY`]: "Medium". Absent when it is
    /// [`NameId::SUBFAMILY`].
    pub const TYPOGRAPHIC_SUBFAMILY: Self = Self(17);
}

/// A glyph's horizontal metrics, from `hmtx`, in font units.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HMetrics {
    /// How far the pen moves after the glyph.
    pub advance: u16,
    /// From the pen to the left of the glyph's bounding box.
    pub left_side_bearing: i16,
}

/// The font's line metrics, from `hhea`, in font units, y up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LineMetrics {
    /// From the baseline to the top of the line, usually positive.
    pub ascender: i16,
    /// From the baseline to the bottom of the line, usually negative.
    pub descender: i16,
    /// The gap between one line's bottom and the next one's top.
    pub line_gap: i16,
    /// The widest advance of any glyph.
    pub advance_width_max: u16,
}

/// A box in font units, y up, as `head` gives it for every glyph.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BoundingBox {
    /// The left edge.
    pub x_min: i16,
    /// The bottom edge.
    pub y_min: i16,
    /// The right edge.
    pub x_max: i16,
    /// The top edge.
    pub y_max: i16,
}

/// What [`Font::os2`] reads from the `OS/2` table. Metrics are in font
/// units, y up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Os2 {
    /// The table's version.
    pub version: u16,
    /// The weight: 400 regular, 500 medium, 600 semibold, 700 bold.
    pub weight_class: u16,
    /// Bit 0 of `fsSelection`: the font is italic.
    pub italic: bool,
    /// Bit 5 of `fsSelection`: the font is bold.
    pub bold: bool,
    /// Bit 7 of `fsSelection`: lines should be spaced with the typographic
    /// metrics in [`Os2::line_metrics`] rather than `hhea`'s or the Windows
    /// ones.
    pub use_typo_metrics: bool,
    /// The typographic and Windows line metrics: `None` for a legacy
    /// version 0 table of fewer than 78 bytes, which ends before them.
    pub line_metrics: Option<Os2LineMetrics>,
    /// The height of lowercase `x`, from version 2.
    pub x_height: Option<i16>,
    /// The height of capitals, from version 2.
    pub cap_height: Option<i16>,
}

/// The line metrics of the `OS/2` table, in font units, y up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Os2LineMetrics {
    /// The typographic ascender.
    pub typo_ascender: i16,
    /// The typographic descender, usually negative.
    pub typo_descender: i16,
    /// The typographic line gap.
    pub typo_line_gap: i16,
    /// The Windows ascent: how far above the baseline glyphs reach.
    pub win_ascent: u16,
    /// The Windows descent, positive: how far below the baseline glyphs
    /// reach.
    pub win_descent: u16,
}

/// What is wrong with a font file, a table or a glyph. See
/// [`FontError::Ttf`] and [`FontError::Glyph`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// The file is larger than [`Limits::max_file_bytes`]. Its offset is
    /// the limit, the first byte past it.
    FileTooLarge {
        /// The limit that was exceeded.
        limit: usize,
    },
    /// The file is a font collection (`ttcf`), which is not supported. At
    /// offset 0.
    Collection,
    /// The font has CFF outlines (`OTTO`), which are not supported. At
    /// offset 0.
    Cff,
    /// The sfnt version is none of `0x00010000`, `true`, `OTTO` and `ttcf`:
    /// the file is not a font. At offset 0.
    BadSfntVersion {
        /// The version found.
        found: u32,
    },
    /// The data ends before what should be there: the file, a table, a
    /// subtable within it as its length says, or a glyph's data, as the
    /// error's place says. At the offset of the read that does not fit,
    /// or the end of the data if that is past it.
    UnexpectedEnd {
        /// What should have been there.
        expected: &'static str,
    },
    /// A table's record in the table directory reaches past the end of the
    /// file. At the record.
    TableOutOfBounds,
    /// A table that is read is listed twice in the table directory. At the
    /// second record.
    DuplicateTable,
    /// A required table is missing. At the end of the table directory.
    MissingTable,
    /// A table's version, or format, is not one that is supported. At the
    /// field.
    BadVersion {
        /// The version or format found.
        found: u32,
    },
    /// `head`'s magic number is not `0x5F0F3CF5`. At the field.
    BadMagic {
        /// The number found.
        found: u32,
    },
    /// `head`'s `unitsPerEm` is not from 16 to 16,384. At the field.
    BadUnitsPerEm {
        /// The value found.
        found: u16,
    },
    /// `head`'s `indexToLocFormat` is neither 0 (short offsets) nor 1
    /// (long). At the field.
    BadLocaFormat {
        /// The value found.
        found: i16,
    },
    /// `head`'s `glyphDataFormat` is not 0. At the field.
    BadGlyphDataFormat {
        /// The value found.
        found: i16,
    },
    /// `maxp` says the font has no glyphs. At the field.
    NoGlyphs,
    /// `hhea`'s `numberOfHMetrics` is 0 or more than the glyph count. At
    /// the field.
    BadMetricsCount {
        /// The value found.
        found: u16,
    },
    /// A `loca` offset is less than the one before it. At the offset.
    LocaOutOfOrder,
    /// A `loca` offset is past the end of `glyf`. At the offset.
    LocaPastGlyf,
    /// `cmap` has no subtable that maps Unicode in format 4 or 12. At the
    /// start of the table.
    NoUnicodeCmap,
    /// A `cmap` format 4 subtable's `segCountX2` is odd. At the field.
    OddSegCount {
        /// The value found.
        found: u16,
    },
    /// A subtable's length is too short for what it holds. At the length.
    BadLength {
        /// The subtable.
        what: &'static str,
    },
    /// Entries that must be in ascending order, without repeating or
    /// overlapping, are not. At the first entry out of order.
    NotSorted {
        /// The entries.
        what: &'static str,
    },
    /// A range ends before it starts. At the range.
    BackwardRange {
        /// What the range is part of.
        what: &'static str,
    },
    /// A subtable's format is not one that is supported. At the field.
    BadFormat {
        /// The subtable.
        what: &'static str,
        /// The format found.
        found: u16,
    },
    /// An index into a list is past its end. At the index.
    IndexOutOfRange {
        /// What it indexes, singular.
        what: &'static str,
        /// The index found.
        index: u16,
        /// How many there are.
        count: u16,
    },
    /// The kerning structures overlap so much that checking them would read
    /// more bytes than `GPOS` holds, as [`Font::kerning`] counts them. At
    /// the structure where the count passed the table's length.
    OverlappingStructures,
    /// A simple glyph's contours' end points do not go up. At the first
    /// that does not.
    ContoursOutOfOrder,
    /// A simple glyph's repeated flag runs past its last point. At the
    /// repeat count.
    FlagsPastPoints,
    /// The outline has more points than [`Limits::max_glyph_points`]. At
    /// the end point (of the glyph whose points cross it) that says how
    /// many it has.
    TooManyPoints {
        /// The limit that was exceeded.
        limit: usize,
    },
    /// Composite glyphs nest deeper than
    /// [`Limits::max_component_depth`]: so does any composite that contains
    /// itself. At the component that goes too deep, or, with a limit of 0,
    /// at the start of the composite asked for.
    TooDeep {
        /// The limit that was exceeded.
        limit: usize,
    },
    /// The glyph has more components than [`Limits::max_components`],
    /// counted over every level. At the component past the limit.
    TooManyComponents {
        /// The limit that was exceeded.
        limit: usize,
    },
    /// A glyph id is not below the glyph count: one asked for, at `maxp`'s
    /// glyph count, or a component's, at its glyph index.
    GlyphOutOfRange {
        /// The glyph id found.
        found: u16,
        /// The glyph count.
        count: u16,
    },
    /// The kerning read holds more subtables than
    /// [`Limits::max_kerning_subtables`]: the `kern` feature's lookups, each
    /// lookup table counted once and each subtable once in it, or the `kern`
    /// table's format 0 subtables that are read. At the offset of the
    /// subtable past the limit (in `GPOS`, of the subtable's offset).
    TooManyKerningSubtables {
        /// The limit that was exceeded.
        limit: usize,
    },
    /// A component's transform and offset take one of its points beyond
    /// the range of `f32`, which only composites nested deeper than the
    /// default [`Limits::max_component_depth`] can do. At the component.
    CoordinateOutOfRange,
    /// A component placed by matching points names a point that does not
    /// exist: of the composite so far, or of the component. At the
    /// argument.
    PointOutOfRange {
        /// The point index found.
        index: u16,
        /// How many points there are.
        count: usize,
    },
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileTooLarge { limit } => {
                write!(f, "the file is larger than the limit of {limit} bytes")
            }
            Self::Collection => {
                f.write_str("the file is a font collection (`ttcf`), which is not supported")
            }
            Self::Cff => f.write_str("the font has CFF outlines (`OTTO`), which are not supported"),
            Self::BadSfntVersion { found } => write!(
                f,
                "the sfnt version is {found:#010x}, not 0x00010000 or `true`: not a TrueType font"
            ),
            Self::UnexpectedEnd { expected } => write!(f, "the data ends before {expected}"),
            Self::TableOutOfBounds => {
                f.write_str("the table's record reaches past the end of the file")
            }
            Self::DuplicateTable => f.write_str("the table is listed twice"),
            Self::MissingTable => f.write_str("the table is missing, and it is required"),
            Self::BadVersion { found } => {
                write!(
                    f,
                    "the version or format is {found:#x}, which is not supported"
                )
            }
            Self::BadMagic { found } => {
                write!(f, "the magic number is {found:#010x}, not 0x5f0f3cf5")
            }
            Self::BadUnitsPerEm { found } => {
                write!(f, "the units per em are {found}, not 16 to 16384")
            }
            Self::BadLocaFormat { found } => {
                write!(f, "indexToLocFormat is {found}, not 0 or 1")
            }
            Self::BadGlyphDataFormat { found } => {
                write!(f, "glyphDataFormat is {found}, not 0")
            }
            Self::NoGlyphs => f.write_str("the font has no glyphs"),
            Self::BadMetricsCount { found } => write!(
                f,
                "numberOfHMetrics is {found}, not from 1 to the glyph count"
            ),
            Self::LocaOutOfOrder => f.write_str("a glyph's offset is less than the one before"),
            Self::LocaPastGlyf => f.write_str("a glyph's offset is past the end of `glyf`"),
            Self::NoUnicodeCmap => f.write_str("no subtable maps Unicode in format 4 or 12"),
            Self::OddSegCount { found } => write!(f, "segCountX2 is {found}, which is odd"),
            Self::BadLength { what } => {
                write!(f, "the length of {what} is too short for what it holds")
            }
            Self::NotSorted { what } => write!(f, "the {what} are not in ascending order"),
            Self::BackwardRange { what } => write!(f, "a range of {what} ends before it starts"),
            Self::BadFormat { what, found } => {
                write!(f, "{what} has format {found}, which is not supported")
            }
            Self::IndexOutOfRange { what, index, count } => {
                write!(f, "{what} {index} does not exist: there are {count}")
            }
            Self::OverlappingStructures => f.write_str(
                "the kerning structures overlap: checking them would read more bytes than the \
                 table holds",
            ),
            Self::ContoursOutOfOrder => {
                f.write_str("the contours' end points are not in ascending order")
            }
            Self::FlagsPastPoints => f.write_str("a repeated flag runs past the last point"),
            Self::TooManyPoints { limit } => {
                write!(f, "the outline has more points than the limit of {limit}")
            }
            Self::TooDeep { limit } => {
                write!(f, "composite glyphs nest deeper than the limit of {limit}")
            }
            Self::TooManyComponents { limit } => {
                write!(f, "the glyph has more components than the limit of {limit}")
            }
            Self::GlyphOutOfRange { found, count } => {
                write!(f, "glyph {found} does not exist: the font has {count}")
            }
            Self::TooManyKerningSubtables { limit } => write!(
                f,
                "the kerning reads more subtables than the limit of {limit}"
            ),
            Self::CoordinateOutOfRange => {
                f.write_str("a component's transform takes a point beyond the range of f32")
            }
            Self::PointOutOfRange { index, count } => {
                write!(f, "point {index} does not exist: there are {count}")
            }
        }
    }
}

/// A parsed TrueType font. It owns its file's bytes, or borrows them for
/// the program's life (`include_bytes!`), keeps where its tables are and
/// their small headers, and reads each glyph when asked.
///
/// `Debug` shows the units per em, the glyph count and which optional
/// tables are read, not the bytes.
#[derive(Clone)]
pub struct Font {
    data: Cow<'static, [u8]>,
    limits: Limits,
    head: Head,
    glyph_count: u16,
    /// The absolute offset of `maxp`'s glyph count, where a glyph id past
    /// it is reported.
    glyph_count_at: usize,
    line: LineMetrics,
    hmtx: Hmtx,
    glyphs: Glyphs,
    cmap: Cmap,
    kerning: Kerning,
    os2: Option<Os2>,
    names: Option<Names>,
}

/// Where kerning comes from.
#[derive(Clone, Debug)]
enum Kerning {
    /// Nowhere: every pair is 0.
    None,
    /// The `kern` feature of `GPOS`.
    Gpos(Gpos),
    /// The `kern` table.
    Kern(KernTable),
}

/// Where each table read is: its tag and its range in the file.
#[derive(Clone, Copy, Debug)]
struct Record {
    tag: Tag,
    start: usize,
    end: usize,
}

/// The tables read, required first.
const REQUIRED: [Tag; 7] = [
    Tag::HEAD,
    Tag::HHEA,
    Tag::MAXP,
    Tag::HMTX,
    Tag::CMAP,
    Tag::LOCA,
    Tag::GLYF,
];
const OPTIONAL: [Tag; 4] = [Tag::KERN, Tag::GPOS, Tag::OS2, Tag::NAME];

impl Font {
    /// Parses a font file, checking what [the module docs](self) list.
    /// `data` may be owned (`Vec<u8>`, a file read at run time) or borrowed
    /// for the program's life (`&'static [u8]`, from `include_bytes!`).
    ///
    /// # Errors
    ///
    /// [`FontError::Ttf`], with the table and byte offset of the problem,
    /// for a file that is malformed, unsupported or larger than
    /// [`Limits::max_file_bytes`].
    pub fn parse(data: impl Into<Cow<'static, [u8]>>, limits: Limits) -> Result<Self, FontError> {
        let data = data.into();
        let parsed = Parsed::read(&data, limits)?;
        Ok(Self {
            data,
            limits,
            head: parsed.head,
            glyph_count: parsed.glyph_count,
            glyph_count_at: parsed.glyph_count_at,
            line: parsed.line,
            hmtx: parsed.hmtx,
            glyphs: parsed.glyphs,
            cmap: parsed.cmap,
            kerning: parsed.kerning,
            os2: parsed.os2,
            names: parsed.names,
        })
    }

    /// The file's bytes.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// The limits the font was parsed with, which [`Font::outline`] reads
    /// glyphs with.
    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// Font units per em, from 16 to 16,384. A glyph is drawn at `px`
    /// pixels per em with a scale of `px / units_per_em` pixels per font
    /// unit.
    pub fn units_per_em(&self) -> u16 {
        self.head.units_per_em
    }

    /// How many glyphs the font has, at least 1. Glyph ids go from 0 to
    /// one less.
    pub fn glyph_count(&self) -> u16 {
        self.glyph_count
    }

    /// The box `head` gives for every glyph, in font units. It is what the
    /// font says, not checked against the glyphs.
    pub fn bounding_box(&self) -> BoundingBox {
        self.head.bbox
    }

    /// The line metrics from `hhea`.
    pub fn line_metrics(&self) -> LineMetrics {
        self.line
    }

    /// The `OS/2` table's style and metrics, if the font has one.
    pub fn os2(&self) -> Option<Os2> {
        self.os2
    }

    /// A glyph's horizontal metrics, or `None` for a glyph id that is not
    /// below the glyph count.
    pub fn h_metrics(&self, glyph: GlyphId) -> Option<HMetrics> {
        if glyph.0 >= self.glyph_count {
            return None;
        }
        self.hmtx.get(&self.data, glyph)
    }

    /// The glyph `c` maps to, or `None` if it maps to none, or to glyph 0
    /// (`.notdef`), or to a glyph that does not exist (the module docs,
    /// "Checked when parsed, or when looked up").
    pub fn glyph_index(&self, c: char) -> Option<GlyphId> {
        self.cmap.lookup(&self.data, u32::from(c), self.glyph_count)
    }

    /// The string with name ID `id`, if the font has one that can be read.
    ///
    /// Of the records with that ID, the first of platform 3 (Windows),
    /// encoding 1 (BMP) or 10 (full Unicode), language 0x409 (US English)
    /// is read; else the first of platform 3, encoding 1 or 10, in any
    /// language; else the first of platform 0 (Unicode), any encoding and
    /// language. Platform 1 (Macintosh) records, in Mac Roman and other
    /// legacy encodings, are never read. The UTF-16BE text is decoded
    /// lossily: an unpaired surrogate, or an odd byte at the end, becomes
    /// U+FFFD.
    pub fn name(&self, id: NameId) -> Option<String> {
        self.names.as_ref()?.get(&self.data, id)
    }

    /// The kerning between `left` and `right`, in font units, added to
    /// `left`'s advance when `right` follows it: usually negative, and 0
    /// when the font has none for the pair, or either glyph id is not
    /// below the glyph count.
    ///
    /// # From `GPOS`
    ///
    /// When the font has a `GPOS` table and the language system read from
    /// it has a `kern` feature, kerning comes from `GPOS` alone: the `kern`
    /// table's contents are then not read, nor checked (its directory
    /// record still is, like every table's that is read: it must lie inside
    /// the file and be listed once). The language system is the
    /// default one of script `latn`, else of `DFLT`, else of the first
    /// script: the OpenType specification has `DFLT` used only for text in
    /// a script the font has no table for, and Tidepool's text is taken to
    /// be Latin. The `kern` features are the ones it names (its required
    /// feature too, when that is one). Their lookups, each index once, add
    /// up; two lookup indices that lead to one lookup table apply it twice:
    ///
    /// - Lookups of type 2 (pair adjustment), and of type 9 (extension)
    ///   wrapping type 2. Of every lookup a `kern` feature names, the type,
    ///   the subtable count and the subtables' offsets (inside the table)
    ///   are read whatever its type, and of a type 9 lookup each extension
    ///   subtable's format (which must be 1) and lookup type; then a lookup
    ///   of any other type, and an extension wrapping any other type, is
    ///   skipped. Lookup flags are ignored: only pairs are kerned, and no
    ///   glyph is skipped as a mark.
    /// - In a lookup, the first subtable that applies gives the value. A
    ///   format 1 subtable (pairs of glyphs) applies when the first glyph
    ///   is in its coverage and its pair set has a record for the second;
    ///   when it has none, the next subtable is tried. A format 2 subtable
    ///   (pairs of classes) applies when the first glyph is in its
    ///   coverage, the second glyph's class is not 0, and both classes are
    ///   within its counts (a glyph a class definition does not list is in
    ///   class 0); otherwise the next subtable is tried. So a covered first
    ///   glyph in class 0 is kerned, and a second glyph in class 0 is not,
    ///   as HarfBuzz does.
    /// - The value is the first glyph's `XAdvance`, when its value format
    ///   has one. Every other value field, and the offsets to device
    ///   tables, are skipped by their size and not applied.
    ///
    /// [`Font::parse`] checks every structure these lookups read: the
    /// header, the script and its language system, the features, the
    /// lookups and their subtables, coverage tables (format 1 sorted, or
    /// format 2 ranges sorted and not overlapping), class definitions
    /// (format 1, or format 2 like coverage ranges), every pair set (inside
    /// the table, its records sorted by second glyph) and the class
    /// records' array. Nothing else in `GPOS` is read: other scripts and
    /// language systems, other features and the lookups only they name.
    ///
    /// Each feature table, lookup table, subtable, coverage table, class
    /// definition and pair set is checked once however many refer to it,
    /// and the language system's feature indices are read once. When the
    /// bytes so read add up to more than the table holds, the structures
    /// must overlap, and parsing fails with
    /// [`ErrorKind::OverlappingStructures`]: a font whose structures do not
    /// overlap never reads more, so the checks take time that grows with
    /// the table.
    ///
    /// What is kept is bounded apart from the table's size: each distinct
    /// subtable once, and each distinct lookup table once, as the list of
    /// its subtables with each repeat dropped (only the first of two can
    /// apply), at most [`Limits::max_kerning_subtables`] entries in all
    /// ([`ErrorKind::TooManyKerningSubtables`] past it, checked before the
    /// subtable is checked). So a call tries at most that many subtables,
    /// each a binary search of its coverage and of its pair set or class
    /// definitions: the limit's docs give what that costs.
    ///
    /// # From `kern`
    ///
    /// Otherwise kerning comes from the `kern` table, if any: Microsoft's
    /// version 0 (Apple's version 1.0 is ignored, and gives none). Its
    /// format 0 subtables that are horizontal, and neither minimum nor
    /// cross-stream, are read; the others are skipped. A subtable's value
    /// adds to those before it, or replaces them when its override bit is
    /// set. A format 0 subtable is taken to end right after its pairs, as
    /// `nPairs` says: its 16-bit length overflows in large fonts. Its pairs
    /// must be in ascending order of the two glyphs, which parsing checks.
    /// At most [`Limits::max_kerning_subtables`] of its subtables are read
    /// (the same limit as `GPOS`'s), so a call does at most that many
    /// binary searches of their pairs.
    pub fn kerning(&self, left: GlyphId, right: GlyphId) -> i32 {
        if left.0 >= self.glyph_count || right.0 >= self.glyph_count {
            return 0;
        }
        match &self.kerning {
            Kerning::None => 0,
            Kerning::Gpos(gpos) => gpos.kerning(&self.data, left, right),
            Kerning::Kern(kern) => kern.kerning(&self.data, left, right),
        }
    }

    /// The outline of `glyph`, in font units, y up, with its composite's
    /// components resolved.
    ///
    /// - An empty glyph (whose `loca` offsets are equal, or which has no
    ///   contours) gives an empty outline.
    /// - A simple glyph: its contours' end points must go up, its
    ///   instructions are skipped, its flags' repeats may not run past its
    ///   last point, and its coordinates' deltas are added up in 64 bits,
    ///   where they cannot overflow.
    /// - A composite glyph: each component is another glyph, transformed by
    ///   a scale, an x and y scale or a 2 × 2 matrix (`F2Dot14`; the first
    ///   of the three its flags give), then moved. It is moved by its
    ///   arguments as an offset (`ARGS_ARE_XY_VALUES`), scaled by the
    ///   matrix only when `SCALED_COMPONENT_OFFSET` is set and
    ///   `UNSCALED_COMPONENT_OFFSET` is not; or so that a point of the
    ///   component, transformed, lands on a point of the composite so far
    ///   (point matching). `ROUND_XY_TO_GRID`, `USE_MY_METRICS` and
    ///   `OVERLAP_COMPOUND` are ignored, since glyphs are not hinted, and
    ///   the composite's instructions are skipped. Points are kept as `f32`,
    ///   rounded after each component's transform and offset.
    ///
    /// Components nest at most [`Limits::max_component_depth`] deep, so a
    /// glyph that contains itself is an error; a glyph has at most
    /// [`Limits::max_components`] components over every level, and at most
    /// [`Limits::max_glyph_points`] points, checked before they are added.
    ///
    /// # Errors
    ///
    /// [`FontError::Glyph`], naming the glyph whose data holds the problem
    /// and the byte offset, for a glyph id not below the glyph count, data
    /// that is malformed, or a limit exceeded.
    pub fn outline(&self, glyph: GlyphId) -> Result<Outline, FontError> {
        if glyph.0 >= self.glyph_count {
            return Err(FontError::Glyph {
                glyph,
                offset: self.glyph_count_at,
                kind: ErrorKind::GlyphOutOfRange {
                    found: glyph.0,
                    count: self.glyph_count,
                },
            });
        }
        self.glyphs.outline(&self.data, glyph, self.limits)
    }
}

impl fmt::Debug for Font {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kerning = match self.kerning {
            Kerning::None => "none",
            Kerning::Gpos(_) => "GPOS",
            Kerning::Kern(_) => "kern",
        };
        f.debug_struct("Font")
            .field("bytes", &self.data.len())
            .field("units_per_em", &self.head.units_per_em)
            .field("glyph_count", &self.glyph_count)
            .field("kerning", &kerning)
            .field("os2", &self.os2.is_some())
            .field("name", &self.names.is_some())
            .finish_non_exhaustive()
    }
}

/// Everything [`Font::parse`] reads, before it takes the bytes.
struct Parsed {
    head: Head,
    glyph_count: u16,
    glyph_count_at: usize,
    line: LineMetrics,
    hmtx: Hmtx,
    glyphs: Glyphs,
    cmap: Cmap,
    kerning: Kerning,
    os2: Option<Os2>,
    names: Option<Names>,
}

impl Parsed {
    fn read(data: &[u8], limits: Limits) -> Result<Self, FontError> {
        let file = Span::file(data);
        if data.len() > limits.max_file_bytes {
            return Err(file.error(
                limits.max_file_bytes,
                ErrorKind::FileTooLarge {
                    limit: limits.max_file_bytes,
                },
            ));
        }
        let version = file.u32(0, "the sfnt version")?;
        match &version.to_be_bytes() {
            [0, 1, 0, 0] | b"true" => {}
            b"OTTO" => return Err(file.error(0, ErrorKind::Cff)),
            b"ttcf" => return Err(file.error(0, ErrorKind::Collection)),
            _ => return Err(file.error(0, ErrorKind::BadSfntVersion { found: version })),
        }
        let (records, directory_end) = directory(file)?;
        let find = |tag: Tag| records.iter().find(|record| record.tag == tag);
        let table = |tag: Tag| -> Result<Span<'_>, FontError> {
            match find(tag) {
                Some(record) => Ok(Span::new(
                    data,
                    record.start,
                    record.end,
                    Place::Table(Some(tag)),
                )),
                None => Err(FontError::Ttf {
                    table: Some(tag),
                    offset: directory_end,
                    kind: ErrorKind::MissingTable,
                }),
            }
        };
        let optional = |tag: Tag| {
            find(tag)
                .map(|record| Span::new(data, record.start, record.end, Place::Table(Some(tag))))
        };
        for tag in REQUIRED {
            table(tag)?;
        }

        let head = Head::read(table(Tag::HEAD)?)?;
        let maxp = table(Tag::MAXP)?;
        let (glyph_count, glyph_count_at) = tables::glyph_count(maxp)?;
        let (line, metrics_count) = tables::hhea(table(Tag::HHEA)?, glyph_count)?;
        let hmtx = Hmtx::read(table(Tag::HMTX)?, glyph_count, metrics_count)?;
        let glyphs = Glyphs::read(
            table(Tag::LOCA)?,
            table(Tag::GLYF)?,
            head.long_loca,
            glyph_count,
        )?;
        let cmap = Cmap::read(table(Tag::CMAP)?)?;
        let os2 = optional(Tag::OS2).map(tables::os2).transpose()?;
        let names = optional(Tag::NAME).map(Names::read).transpose()?;
        let mut kerning = Kerning::None;
        if let Some(gpos) = optional(Tag::GPOS)
            && let Some(gpos) = Gpos::read(gpos, limits.max_kerning_subtables)?
        {
            kerning = Kerning::Gpos(gpos);
        }
        if matches!(kerning, Kerning::None)
            && let Some(kern) = optional(Tag::KERN)
            && let Some(kern) = KernTable::read(kern, limits.max_kerning_subtables)?
        {
            kerning = Kerning::Kern(kern);
        }
        Ok(Self {
            head,
            glyph_count,
            glyph_count_at,
            line,
            hmtx,
            glyphs,
            cmap,
            kerning,
            os2,
            names,
        })
    }
}

/// Reads the table directory: the records of the tables read, each inside
/// the file and listed once, and the offset where the directory ends.
fn directory(file: Span<'_>) -> Result<(Vec<Record>, usize), FontError> {
    let count = usize::from(file.u16(4, "the number of tables")?);
    // At most 65,535 records of 16 bytes: no overflow.
    let records = file.sub(12, count * 16, "the table records")?;
    let mut found: Vec<Record> = Vec::new();
    for index in 0..count {
        let at = index * 16;
        let tag = records.tag(at, "a table's tag")?;
        if !REQUIRED.contains(&tag) && !OPTIONAL.contains(&tag) {
            continue;
        }
        let place = records.absolute(at);
        let offset = records.u32(at + 8, "a table's offset")?;
        let length = records.u32(at + 12, "a table's length")?;
        let start = usize::try_from(offset).ok();
        let end = start
            .zip(usize::try_from(length).ok())
            .and_then(|(start, length)| start.checked_add(length));
        let (Some(start), Some(end)) = (start, end) else {
            return Err(out_of_bounds(tag, place));
        };
        if end > file.len() {
            return Err(out_of_bounds(tag, place));
        }
        if found.iter().any(|record| record.tag == tag) {
            return Err(FontError::Ttf {
                table: Some(tag),
                offset: place,
                kind: ErrorKind::DuplicateTable,
            });
        }
        found.push(Record { tag, start, end });
    }
    Ok((found, records.end()))
}

fn out_of_bounds(tag: Tag, at: usize) -> FontError {
    FontError::Ttf {
        table: Some(tag),
        offset: at,
        kind: ErrorKind::TableOutOfBounds,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_display_printable_ascii_and_escape_the_rest() {
        assert_eq!(Tag::OS2.to_string(), "OS/2");
        assert_eq!(Tag(*b"cvt ").to_string(), "cvt ");
        assert_eq!(Tag([0, b'a', 0x7f, b'\\']).to_string(), "\\x00a\\x7f\\x5c");
        assert_eq!(format!("{:?}", Tag::GLYF), "Tag(\"glyf\")");
    }

    #[test]
    fn every_table_read_is_required_or_optional_once() {
        for tag in REQUIRED {
            assert!(!OPTIONAL.contains(&tag), "{tag}");
        }
        let mut all: Vec<Tag> = REQUIRED.iter().chain(&OPTIONAL).copied().collect();
        all.sort();
        all.dedup();
        assert_eq!(all.len(), REQUIRED.len() + OPTIONAL.len());
    }
}
