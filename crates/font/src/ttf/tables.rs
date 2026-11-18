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

//! The small tables: `head`, `maxp`, `hhea`, `hmtx` and `OS/2`.

use super::read::{Span, be16, be16_signed};
use super::{BoundingBox, ErrorKind, GlyphId, HMetrics, LineMetrics, Os2, Os2LineMetrics};
use crate::FontError;

/// `head`'s magic number.
const MAGIC: u32 = 0x5F0F_3CF5;

/// What is kept of `head`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Head {
    pub(crate) units_per_em: u16,
    pub(crate) bbox: BoundingBox,
    /// `indexToLocFormat` is 1: `loca` holds 32-bit offsets.
    pub(crate) long_loca: bool,
}

impl Head {
    pub(crate) fn read(head: Span<'_>) -> Result<Self, FontError> {
        head.bytes(0, 54, "the fields of `head`")?;
        let major = head.u16(0, "the major version")?;
        if major != 1 {
            return Err(head.error(
                0,
                ErrorKind::BadVersion {
                    found: u32::from(major),
                },
            ));
        }
        let magic = head.u32(12, "the magic number")?;
        if magic != MAGIC {
            return Err(head.error(12, ErrorKind::BadMagic { found: magic }));
        }
        let units_per_em = head.u16(18, "unitsPerEm")?;
        if !(16..=16_384).contains(&units_per_em) {
            return Err(head.error(
                18,
                ErrorKind::BadUnitsPerEm {
                    found: units_per_em,
                },
            ));
        }
        let bbox = BoundingBox {
            x_min: head.i16(36, "xMin")?,
            y_min: head.i16(38, "yMin")?,
            x_max: head.i16(40, "xMax")?,
            y_max: head.i16(42, "yMax")?,
        };
        let loca_format = head.i16(50, "indexToLocFormat")?;
        let long_loca = match loca_format {
            0 => false,
            1 => true,
            found => return Err(head.error(50, ErrorKind::BadLocaFormat { found })),
        };
        let data_format = head.i16(52, "glyphDataFormat")?;
        if data_format != 0 {
            return Err(head.error(52, ErrorKind::BadGlyphDataFormat { found: data_format }));
        }
        Ok(Self {
            units_per_em,
            bbox,
            long_loca,
        })
    }
}

/// `maxp`'s glyph count, at least 1, and the field's absolute offset.
pub(crate) fn glyph_count(maxp: Span<'_>) -> Result<(u16, usize), FontError> {
    let version = maxp.u32(0, "the version")?;
    if version != 0x0001_0000 {
        return Err(maxp.error(0, ErrorKind::BadVersion { found: version }));
    }
    maxp.bytes(0, 32, "the fields of version 1.0")?;
    let count = maxp.u16(4, "numGlyphs")?;
    if count == 0 {
        return Err(maxp.error(4, ErrorKind::NoGlyphs));
    }
    Ok((count, maxp.absolute(4)))
}

/// `hhea`'s line metrics and `numberOfHMetrics`, from 1 to the glyph count.
pub(crate) fn hhea(hhea: Span<'_>, glyph_count: u16) -> Result<(LineMetrics, u16), FontError> {
    hhea.bytes(0, 36, "the fields of `hhea`")?;
    let major = hhea.u16(0, "the major version")?;
    if major != 1 {
        return Err(hhea.error(
            0,
            ErrorKind::BadVersion {
                found: u32::from(major),
            },
        ));
    }
    let line = LineMetrics {
        ascender: hhea.i16(4, "the ascender")?,
        descender: hhea.i16(6, "the descender")?,
        line_gap: hhea.i16(8, "the line gap")?,
        advance_width_max: hhea.u16(10, "advanceWidthMax")?,
    };
    let count = hhea.u16(34, "numberOfHMetrics")?;
    if count == 0 || count > glyph_count {
        return Err(hhea.error(34, ErrorKind::BadMetricsCount { found: count }));
    }
    Ok((line, count))
}

/// Where `hmtx` is, and how many full records it starts with.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Hmtx {
    /// The absolute offset of the table.
    at: usize,
    /// `numberOfHMetrics`, at least 1.
    long: u16,
}

impl Hmtx {
    /// Checks that `hmtx` holds `long` records of 4 bytes and a bearing of
    /// 2 for each glyph after them.
    pub(crate) fn read(hmtx: Span<'_>, glyph_count: u16, long: u16) -> Result<Self, FontError> {
        let short = usize::from(glyph_count - long.min(glyph_count));
        let len = usize::from(long) * 4 + short * 2;
        hmtx.bytes(0, len, "the horizontal metrics")?;
        Ok(Self {
            at: hmtx.start(),
            long,
        })
    }

    /// The metrics of `glyph`, which is below the glyph count.
    pub(crate) fn get(&self, data: &[u8], glyph: GlyphId) -> Option<HMetrics> {
        let index = usize::from(glyph.0);
        let long = usize::from(self.long);
        if index < long {
            let at = self.at + index * 4;
            return Some(HMetrics {
                advance: be16(data, at)?,
                left_side_bearing: be16_signed(data, at + 2)?,
            });
        }
        let advance = be16(data, self.at + long.saturating_sub(1) * 4)?;
        let bearing = be16_signed(data, self.at + long * 4 + (index - long) * 2)?;
        Some(HMetrics {
            advance,
            left_side_bearing: bearing,
        })
    }
}

/// Reads `OS/2`, which must be as long as its version's fields: 68 bytes
/// for version 0, whose line metrics are read only when it has 78.
pub(crate) fn os2(table: Span<'_>) -> Result<Os2, FontError> {
    let version = table.u16(0, "the version")?;
    let (len, expected) = match version {
        0 => (68, "the fields of version 0"),
        1 => (86, "the fields of version 1"),
        2..=4 => (96, "the fields of versions 2 to 4"),
        _ => (100, "the fields of version 5"),
    };
    table.bytes(0, len, expected)?;
    let selection = table.u16(62, "fsSelection")?;
    let line_metrics = if table.len() >= 78 {
        Some(Os2LineMetrics {
            typo_ascender: table.i16(68, "sTypoAscender")?,
            typo_descender: table.i16(70, "sTypoDescender")?,
            typo_line_gap: table.i16(72, "sTypoLineGap")?,
            win_ascent: table.u16(74, "usWinAscent")?,
            win_descent: table.u16(76, "usWinDescent")?,
        })
    } else {
        None
    };
    let (x_height, cap_height) = if version >= 2 {
        (
            Some(table.i16(86, "sxHeight")?),
            Some(table.i16(88, "sCapHeight")?),
        )
    } else {
        (None, None)
    };
    Ok(Os2 {
        version,
        weight_class: table.u16(4, "usWeightClass")?,
        italic: selection & 0x0001 != 0,
        bold: selection & 0x0020 != 0,
        use_typo_metrics: selection & 0x0080 != 0,
        line_metrics,
        x_height,
        cap_height,
    })
}
