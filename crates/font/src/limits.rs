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

//! Size limits that bound what the font parser and the rasterizer accept.
//!
//! New in Tidepool, in the shape of `tidepool_codec::Limits`: a `Copy`
//! struct with a `const` [`Limits::DEFAULT`], built from the defaults by
//! whoever needs other bounds.

/// Upper bounds that [`ttf::Font`](crate::ttf::Font) and
/// [`outline::rasterize`](crate::outline::rasterize) enforce, so that what
/// a malformed or hostile font can cost is bounded. With the defaults:
///
/// - **Memory:** besides the file itself, a parsed font holds a few small
///   tables' worth, and its kerning at most 50 KiB
///   ([`max_kerning_subtables`](Self::max_kerning_subtables)). While
///   `GPOS` is parsed, its checks take, besides: a byte for each of its
///   bytes, for the pair sets checked (allocated when the first is); at
///   most 1 MiB for the lookup indices its `kern` features name and the
///   lookups they lead to (8 bytes each, at most 65,536 of each), and
///   16 KiB of sets of the features and indices met; and about 130 KiB at
///   most for the at most 512 subtables checked, their coverage tables and
///   class definitions, and what is kept. Measured with a counting
///   allocator: 1,048,760 bytes for 65,535 lookup indices, and 132,944 for
///   512 format 2 subtables each with its own coverage and class
///   definitions. An outline holds at most 1.25 MiB
///   ([`max_glyph_points`](Self::max_glyph_points)), and while it is read
///   its vectors can hold up to twice that before they are shrunk to fit.
///   Rasterizing copies the points (16 bytes each, 1 MiB at most) and
///   makes a bitmap of at most 16 MiB, with 8 MiB of working rows
///   ([`max_bitmap_dimension`](Self::max_bitmap_dimension)).
/// - **Stack:** nothing recurses on the font's data: composites are
///   resolved on a stack on the heap.
/// - **Time:** parsing reads each byte of the tables it checks a bounded
///   number of times (`GPOS` as [`Font::kerning`](crate::ttf::Font::kerning)
///   says), so it takes time that grows with the file, 64 MiB at most; a
///   kerning call tries at most 512 subtables, up to 0.4 ms on the
///   development machine (see
///   [`max_kerning_subtables`](Self::max_kerning_subtables)); reading an
///   outline is bounded by its components and points;
///   rasterizing by [`max_raster_work`](Self::max_raster_work) and the
///   bitmap's area, about 95 ms at most on the development machine. Run
///   them off the main thread for fonts that are not bundled.
///
/// Every bound is checked before the memory it guards is allocated. A
/// [`Font`](crate::ttf::Font) keeps the limits it was parsed with, and
/// reads each glyph's outline with them. Callers that need other bounds
/// build their own from the defaults, so that fields added later keep
/// theirs:
///
/// ```
/// use tidepool_font::Limits;
///
/// let limits = Limits {
///     max_file_bytes: 1024 * 1024,
///     ..Limits::DEFAULT
/// };
/// assert_eq!(limits.max_glyph_points, Limits::DEFAULT.max_glyph_points);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The largest font file accepted, in bytes. Checked first, before any
    /// table is read.
    ///
    /// Default 64 MiB. The largest TrueType fonts in common use (complete
    /// CJK faces) are 20 to 35 MB; the bundled faces are under 300 KB.
    /// Parsing reads each byte of the tables it checks a bounded number of
    /// times (for `GPOS`, as
    /// [`Font::kerning`](crate::ttf::Font::kerning) says), so its time
    /// grows with the file, and this bounds it.
    pub max_file_bytes: usize,
    /// The most points one glyph's outline may have, with its composite's
    /// components resolved: what [`Font::outline`](crate::ttf::Font::outline)
    /// allocates. Checked before each simple glyph's points are added.
    ///
    /// Default 65,536, the most one simple glyph can have (its contours'
    /// last point is a 16-bit index). The bundled faces' largest glyphs
    /// have a few hundred. An outline takes 12 bytes a point and 8 a
    /// contour, and every contour has a point, so at most 20 bytes a
    /// point: this bounds one to 1.25 MiB.
    pub max_glyph_points: usize,
    /// How deeply composite glyphs may nest. A simple glyph has depth 0,
    /// and a composite of simple glyphs depth 1. Bounds the outline
    /// reader's stack (kept on the heap), and so a composite that contains
    /// itself, which is always an error.
    ///
    /// Default 8. Fonts rarely nest deeper than 2 or 3 (`maxp`'s
    /// `maxComponentDepth`; the bundled faces have 1).
    pub max_component_depth: usize,
    /// The most components one glyph may have, counted over every level of
    /// its composites. Bounds the work of a composite whose components are
    /// empty, or nest within the depth limit to make a tree, which adds no
    /// points for `max_glyph_points` to catch.
    ///
    /// Default 1,024. An accented letter has 2 to 4 components, and the
    /// largest composites in common fonts (precomposed Hangul syllables,
    /// stacked marks) a few dozen; each one read is at most 16 bytes of
    /// the file and a little arithmetic.
    pub max_components: usize,
    /// The widest, or tallest, coverage bitmap
    /// [`outline::rasterize`](crate::outline::rasterize) makes, in pixels.
    /// Checked on the scaled control box, before the bitmap is allocated.
    ///
    /// Default 4,096. A glyph at the largest size Tidepool draws text (a
    /// heading of a few dozen pixels at scale 4) is a few hundred pixels,
    /// and even a glyph of several ems at 1,000 pixels an em fits. It
    /// bounds a bitmap to 16 MiB, and the rasterizer's working rows to
    /// 8 MiB more (`outline`'s module docs). Filling a bitmap of 4,096
    /// pixels a side takes about 25 ms in a release build on the
    /// development machine (1.5 ns a pixel), whatever its outline.
    pub max_bitmap_dimension: u32,
    /// The most work [`outline::rasterize`](crate::outline::rasterize) may
    /// do for one bitmap, in steps, estimated from the scaled outline
    /// before the bitmap is allocated: for each edge of its control
    /// polygon (each point to the next), its length in x plus its length in
    /// y, in pixels and rounded up, plus
    /// [`outline::EDGE_WORK`](crate::outline::EDGE_WORK); and for each band
    /// of rows the rasterizer works in, one step a point. This bounds the
    /// rows and cells its lines visit and the lines it flattens curves into
    /// (`outline`'s `work`).
    ///
    /// Default 2^21, 2,097,152. In a release build on the development
    /// machine (an 11th-generation Core i7 at 2.3 GHz), best of 5, a step
    /// took 7 to 40 ns besides the bitmap's area: the least for long
    /// horizontal lines, the most for steep lines far apart across a bitmap
    /// 4,096 pixels wide, each in a column of its own (28 to 32 ns at the
    /// limit; 40 ns for 128 lines 32 pixels apart, which cannot reach it).
    /// The slowest outlines at the limit, 504 lines zigzagging across a
    /// 4,095-pixel square, took 83 to 94 ms in all, about 25 ms of it the
    /// bitmap's area (above); at 40 ns a step the limit is 84 ms of work.
    /// Without the limit, 65,536 such lines took 5 s. The bundled faces'
    /// most is 15,840 steps at 512 pixels an em, and 57,640 at 2,048: at
    /// 2^22 the slowest outlines measured up to 142 ms, so the limit is
    /// 2^21.
    pub max_raster_work: u64,
    /// The most subtables a font's kerning keeps, whichever source it comes
    /// from: from `GPOS`, the entries of the `kern` feature's lookups, each
    /// lookup table counted once however many lookup indices lead to it,
    /// and each subtable once in a lookup; from the `kern` table, its
    /// format 0 subtables that are read. Checked before each is checked
    /// and kept.
    ///
    /// Default 512. Of the 385 distinct `.ttf` files under
    /// `/usr/share/fonts/truetype` on the development machine, the most
    /// any keeps is 13 (FreeSerif); the bundled Saira faces keep 2 and
    /// JetBrains Mono none. A `GPOS` entry takes 4 bytes, and the subtable
    /// and lookup it may bring at most 64 and 32 more; a `kern` subtable
    /// takes 24: so this bounds what a font keeps for kerning to 50 KiB.
    ///
    /// A kerning call tries at most this many subtables, each a few binary
    /// searches, whose cost grows with the structures searched. Measured
    /// with a probe (not a test) in a release build on the development
    /// machine, pinned to one core, a call that tries all 512: 4 to 6 µs
    /// for subtables of one glyph and one pair; 25 to 38 µs for subtables
    /// that share deep structures (a coverage of 65,535 glyphs and class
    /// definitions of 2,000 ranges, or a coverage of 32,000 glyphs and a
    /// pair set of 2,000 records); and for subtables each with its own deep
    /// structures, in files of 31 to 50 MB, about 50 µs with them in the
    /// cache and about 0.2 ms, at most 0.4 ms, without. At 4,096 subtables
    /// such fonts took about 0.3 ms a call in the cache, so the limit is
    /// 512.
    pub max_kerning_subtables: usize,
}

impl Limits {
    /// The defaults: 64 MiB of file, 65,536 points, a depth of 8, 1,024
    /// components, 4,096 pixels a side, 2^21 steps of rasterizing and 512
    /// kerning subtables. Each field's docs say why.
    pub const DEFAULT: Self = Self {
        max_file_bytes: 64 * 1024 * 1024,
        max_glyph_points: 65_536,
        max_component_depth: 8,
        max_components: 1024,
        max_bitmap_dimension: 4096,
        max_raster_work: 1 << 21,
        max_kerning_subtables: 512,
    };
}

impl Default for Limits {
    /// [`Limits::DEFAULT`].
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_documented_values() {
        let limits = Limits::default();
        assert_eq!(limits, Limits::DEFAULT);
        assert_eq!(limits.max_file_bytes, 67_108_864);
        assert_eq!(limits.max_glyph_points, 65_536);
        assert_eq!(limits.max_component_depth, 8);
        assert_eq!(limits.max_components, 1024);
        assert_eq!(limits.max_bitmap_dimension, 4096);
        assert_eq!(limits.max_raster_work, 2_097_152);
        assert_eq!(limits.max_kerning_subtables, 512);
        // A simple glyph's most points: its last point's index is a `u16`.
        assert_eq!(usize::from(u16::MAX) + 1, limits.max_glyph_points);
    }
}
