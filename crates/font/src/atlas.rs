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

//! The glyph atlas: [`Atlas`] keeps rasterized glyphs in one 8-bit
//! coverage image, keyed by face, glyph, size and subpixel offset in
//! quarters of a pixel ([`GlyphKey`]), and evicts the least recently used
//! when it is full.
//!
//! New in Tidepool; not ported (Nadir draws text with OpenGL).
//!
//! ```
//! # fn main() -> Result<(), tidepool_font::FontError> {
//! use tidepool_font::atlas::{Atlas, GlyphKey, Subpixel};
//! use tidepool_font::bundled::{self, Face};
//! use tidepool_font::layout::{self, PxSize, Style};
//!
//! let faces = bundled::load()?;
//! let style = Style::new(Face::SairaRegular.id(), PxSize::new(14.0).expect("a size"));
//! let line = layout::line(&faces, "Tidepool", &style)?;
//! let mut atlas = Atlas::new(256, 256)?;
//! for glyph in &line.glyphs {
//!     let Some(id) = glyph.glyph else { continue };
//!     // The line starts at x = 10.3: each glyph at a whole pixel and a quarter.
//!     let (pixel, subpixel) = Subpixel::split(10.3 + glyph.x);
//!     let key = GlyphKey { face: glyph.face, glyph: id, size: style.size, subpixel };
//!     let image = atlas.glyph(&faces, key)?;
//!     // Its coverage goes at x = pixel + image.left, y = baseline + image.top.
//!     assert!(pixel + image.left >= 10);
//!     assert!(image.top < 0, "above the baseline");
//! }
//! // Seven letters, and the second o on a quarter of its own, or not.
//! assert!((7..=8).contains(&atlas.len()));
//! # Ok(())
//! # }
//! ```
//!
//! # The image
//!
//! The atlas is `width × height` bytes of coverage, one a pixel, rows from
//! the top and `width` bytes apart: the layout of
//! `tidepool_gfx::glyph::Mask`, which `gui` makes of
//! [`AtlasGlyph::pixels`] and cuts each glyph from with `Mask::region`.
//! Glyphs are placed on shelves: rows as tall as the glyph, rounded up to
//! 4 pixels, filled left to right, and a glyph goes on the lowest shelf
//! that holds it and is at most 4 pixels taller than that. An empty glyph
//! (a space) takes no room.
//!
//! # Eviction
//!
//! Each [`Atlas::glyph`] call stamps the glyph it returns as the most
//! recently used. When a new glyph does not fit, or the atlas holds
//! [`MAX_GLYPHS`], the atlas keeps the most recently used glyphs that fill
//! at most half its area (and half of [`MAX_GLYPHS`]), drops the rest, and
//! packs those it keeps again, tallest first; if the new glyph still does
//! not fit, it keeps half as much again, and so on. So a glyph's place can
//! change in any call
//! that adds one: an [`AtlasGlyph`] borrows the atlas, and the borrow
//! checker keeps it from outliving the next call. Draw each glyph as it is
//! returned.
//!
//! An atlas holds the glyphs of one [`Faces`]: handed another, it drops
//! every glyph first, since that set's ids may name other faces.
//!
//! # Large glyphs and failures
//!
//! A glyph's image may be at most half the atlas's shorter side wide and
//! tall ([`Atlas::glyph_limit`]), so that one glyph never takes more than a
//! quarter of the atlas and two used in turn never evict each other. The
//! atlas rasterizes with the face's [`Limits`](crate::Limits) lowered to
//! that, so a glyph too large is refused from its outline's control box,
//! before its image is made or any rasterizing done. A key that failed,
//! for that or because its glyph cannot be read or rasterized, is
//! remembered, with its error, until the atlas is cleared: asked for again
//! it fails at once, so a hostile face costs its work once, not on every
//! frame. At most [`MAX_FAILURES`] are remembered; one more forgets them
//! all.
//!
//! # Cost
//!
//! The image is `width × height` bytes, at most [`MAX_SIDE`] a side
//! (16 MiB); while it repacks, it holds a second image as large. The
//! table of glyphs holds at most [`MAX_GLYPHS`], each about 50 bytes. A
//! glyph not yet in the atlas is read and rasterized
//! ([`rasterize`]), which [`Limits`](crate::Limits) bounds; a
//! glyph at UI sizes takes some microseconds.

use std::collections::HashMap;
use std::fmt;

use crate::FontError;
use crate::fallback::{self, FaceId, Faces, NOTDEF_ADVANCE};
use crate::layout::PxSize;
use crate::outline::{self, Bitmap, Outline, OutlinePoint};
use crate::ttf::GlyphId;

/// The largest side of an atlas, in pixels: 4,096, so 16 MiB at most.
pub const MAX_SIDE: u32 = 4096;

/// The most glyphs an atlas holds at once: 16,384. Empty glyphs take no
/// room in the image, so this bounds the table when they are many.
pub const MAX_GLYPHS: usize = 16_384;

/// The most keys an atlas remembers as failed (the [module docs](self),
/// "Large glyphs and failures"): 1,024.
pub const MAX_FAILURES: usize = 1024;

/// How far [`Subpixel::split`] lets a pixel go from 0: 2^30, so that a
/// glyph's `left` or `top`, which is within ±2^23 for any outline a font
/// can hold, adds to it without overflow.
pub const MAX_PIXEL: i32 = 1 << 30;

/// A subpixel offset: how far right of a whole pixel a glyph is drawn, in
/// quarters of a pixel, 0 to 3.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Subpixel(u8);

impl Subpixel {
    /// No offset.
    pub const ZERO: Self = Self(0);

    /// `quarters` of a pixel; `None` above 3.
    pub const fn new(quarters: u8) -> Option<Self> {
        if quarters < 4 {
            Some(Self(quarters))
        } else {
            None
        }
    }

    /// The offset in quarters of a pixel, 0 to 3.
    pub const fn quarters(self) -> u8 {
        self.0
    }

    /// The offset in pixels: 0, 0.25, 0.5 or 0.75.
    pub fn shift(self) -> f32 {
        f32::from(self.0) / 4.0
    }

    /// `x` in pixels to the nearest quarter, as a whole pixel and the
    /// quarters right of it: `10.3` is pixel 10 and one quarter, `-0.1`
    /// pixel 0 and none. NaN is pixel 0, and an `x` beyond ±[`MAX_PIXEL`]
    /// is that pixel, with no quarters.
    pub fn split(x: f32) -> (i32, Self) {
        let quarters = (f64::from(x) * 4.0 + 0.5).floor();
        let limit = f64::from(MAX_PIXEL) * 4.0;
        let quarters = if quarters.is_nan() {
            0.0
        } else {
            quarters.clamp(-limit, limit)
        };
        // Whole, and within ±2^32: exact as an i64.
        let quarters = quarters as i64;
        let pixel = i32::try_from(quarters.div_euclid(4)).unwrap_or(MAX_PIXEL);
        // 0 to 3.
        (pixel, Self(quarters.rem_euclid(4) as u8))
    }
}

/// What a glyph in the atlas is: the face, the glyph, the size and the
/// subpixel offset it is drawn at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GlyphKey {
    /// The face, of the [`Faces`] the atlas is given.
    pub face: FaceId,
    /// The glyph in it; glyph 0 is drawn as `.notdef`'s box.
    pub glyph: GlyphId,
    /// Pixels per em.
    pub size: PxSize,
    /// How far right of a whole pixel it is drawn.
    pub subpixel: Subpixel,
}

/// Why an atlas cannot be made, or cannot hold a glyph. See
/// [`FontError::Atlas`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// [`Atlas::new`] was given a side of 0 or above [`MAX_SIDE`].
    BadSize {
        /// The width given.
        width: u32,
        /// The height given.
        height: u32,
    },
    /// The glyph's image would be wider or taller than
    /// [`Atlas::glyph_limit`], half the atlas's shorter side. A painter may
    /// draw it from [`rasterize`] instead, at its own cost.
    GlyphTooLarge {
        /// The limit, in pixels.
        limit: u32,
    },
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadSize { width, height } => write!(
                f,
                "an atlas of {width} × {height} pixels has a side of 0 or above {MAX_SIDE}"
            ),
            Self::GlyphTooLarge { limit } => write!(
                f,
                "the glyph's image would be wider or taller than {limit} pixels, half the \
                 atlas's shorter side"
            ),
        }
    }
}

fn error(kind: ErrorKind) -> FontError {
    FontError::Atlas { kind }
}

/// Where a glyph is in the atlas, and when it was last used.
#[derive(Clone, Copy, Debug)]
struct Entry {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    left: i32,
    top: i32,
    last_used: u64,
}

impl Entry {
    fn area(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }
}

/// A row of glyphs: its top, its height, and where its free part starts.
#[derive(Clone, Copy, Debug)]
struct Shelf {
    y: u32,
    height: u32,
    x: u32,
}

/// A cache of rasterized glyphs in one coverage image (the [module
/// docs](self)).
///
/// `Debug` shows the size and the number of glyphs, not the image.
pub struct Atlas {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    entries: HashMap<GlyphKey, Entry>,
    shelves: Vec<Shelf>,
    /// The bottom of the lowest shelf.
    bottom: u32,
    /// Counts calls, to stamp the glyphs used.
    clock: u64,
    /// The identity of the [`Faces`] the glyphs are from.
    faces: Option<u64>,
    /// Keys that failed, with their errors.
    failures: HashMap<GlyphKey, FontError>,
    evictions: u64,
}

/// A glyph in the atlas, as [`Atlas::glyph`] returns it: where its image
/// is in the atlas's, and where it goes from the pen.
///
/// It borrows the atlas, whose next call that adds a glyph may move it.
#[derive(Clone, Copy)]
pub struct AtlasGlyph<'a> {
    atlas: &'a [u8],
    atlas_width: u32,
    atlas_height: u32,
    /// The column of the image's left edge in the atlas.
    pub x: u32,
    /// The row of the image's top edge in the atlas.
    pub y: u32,
    /// The image's width; 0 for an empty glyph.
    pub width: u32,
    /// The image's height; 0 for an empty glyph.
    pub height: u32,
    /// The device x of the image's left edge from the pen, as
    /// [`Bitmap::left`].
    pub left: i32,
    /// The device y of the image's top edge from the pen on the baseline,
    /// growing downwards, as [`Bitmap::top`].
    pub top: i32,
}

impl<'a> AtlasGlyph<'a> {
    /// The whole atlas's image, from its first pixel: `width × height`
    /// bytes, rows [`AtlasGlyph::atlas_width`] apart.
    pub fn pixels(&self) -> &'a [u8] {
        self.atlas
    }

    /// The atlas's width, which is also its rows' stride.
    pub fn atlas_width(&self) -> u32 {
        self.atlas_width
    }

    /// The atlas's height.
    pub fn atlas_height(&self) -> u32 {
        self.atlas_height
    }

    /// Row `row` of the glyph's image, `width` bytes, or `None` past its
    /// last row.
    pub fn row(&self, row: u32) -> Option<&'a [u8]> {
        if row >= self.height {
            return None;
        }
        let start = (self.y as usize + row as usize) * self.atlas_width as usize + self.x as usize;
        self.atlas.get(start..start + self.width as usize)
    }

    /// The coverage at `column`, `row` of the glyph's image, or `None`
    /// outside it.
    pub fn coverage(&self, column: u32, row: u32) -> Option<u8> {
        if column >= self.width {
            return None;
        }
        self.row(row)?.get(column as usize).copied()
    }
}

impl fmt::Debug for AtlasGlyph<'_> {
    /// Its place, not the pixels.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AtlasGlyph")
            .field("x", &self.x)
            .field("y", &self.y)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("left", &self.left)
            .field("top", &self.top)
            .finish_non_exhaustive()
    }
}

impl Atlas {
    /// An empty atlas of `width × height` pixels.
    ///
    /// # Errors
    ///
    /// [`FontError::Atlas`] with [`ErrorKind::BadSize`] for a side of 0 or
    /// above [`MAX_SIDE`].
    pub fn new(width: u32, height: u32) -> Result<Self, FontError> {
        if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
            return Err(error(ErrorKind::BadSize { width, height }));
        }
        Ok(Self {
            width,
            height,
            pixels: vec![0; width as usize * height as usize],
            entries: HashMap::new(),
            shelves: Vec::new(),
            bottom: 0,
            clock: 0,
            faces: None,
            failures: HashMap::new(),
            evictions: 0,
        })
    }

    /// The widest, and tallest, glyph image the atlas holds: half its
    /// shorter side.
    pub fn glyph_limit(&self) -> u32 {
        self.width.min(self.height) / 2
    }

    /// The width, which is also the stride of the image's rows.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// The height.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The image: `width × height` bytes of coverage.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// How many glyphs it holds.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether it holds no glyph.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Whether it holds the glyph `key`, without stamping it as used.
    pub fn contains(&self, key: &GlyphKey) -> bool {
        self.entries.contains_key(key)
    }

    /// How many glyphs it has evicted to make room since it was made.
    pub fn evictions(&self) -> u64 {
        self.evictions
    }

    /// How many keys it remembers as failed (the [module docs](self),
    /// "Large glyphs and failures").
    pub fn failures(&self) -> usize {
        self.failures.len()
    }

    /// Drops every glyph, and every failure remembered, and clears the
    /// image to 0.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.failures.clear();
        self.shelves.clear();
        self.bottom = 0;
        self.pixels.fill(0);
    }

    /// The glyph `key` of `faces`, rasterizing it into the atlas first if
    /// it is not there, evicting others to make room (the [module
    /// docs](self), "Eviction").
    ///
    /// # Errors
    ///
    /// - [`FontError::Faces`] when `faces` does not hold the key's face;
    /// - what [`rasterize`] returns for a glyph that cannot be read or
    ///   rasterized;
    /// - [`FontError::Atlas`] with [`ErrorKind::GlyphTooLarge`] for one
    ///   whose image would be wider or taller than
    ///   [`Atlas::glyph_limit`].
    ///
    /// A key that fails is remembered, and fails at once when asked for
    /// again (the [module docs](self), "Large glyphs and failures"). The
    /// glyphs held are unchanged by an error, but for those dropped when
    /// another [`Faces`] is handed in.
    pub fn glyph(&mut self, faces: &Faces, key: GlyphKey) -> Result<AtlasGlyph<'_>, FontError> {
        if self.faces != Some(faces.identity()) {
            self.clear();
            self.faces = Some(faces.identity());
        }
        self.clock += 1;
        let clock = self.clock;
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.last_used = clock;
            let entry = *entry;
            return Ok(self.view(entry));
        }
        if let Some(failure) = self.failures.get(&key) {
            return Err(failure.clone());
        }
        let limit = self.glyph_limit();
        let result = rasterize_within(faces, key, limit).and_then(|bitmap| self.insert(&bitmap));
        match result {
            Ok(entry) => {
                self.entries.insert(key, entry);
                Ok(self.view(entry))
            }
            Err(failure) => {
                if self.failures.len() >= MAX_FAILURES {
                    self.failures.clear();
                }
                self.failures.insert(key, failure.clone());
                Err(failure)
            }
        }
    }

    fn view(&self, entry: Entry) -> AtlasGlyph<'_> {
        AtlasGlyph {
            atlas: &self.pixels,
            atlas_width: self.width,
            atlas_height: self.height,
            x: entry.x,
            y: entry.y,
            width: entry.width,
            height: entry.height,
            left: entry.left,
            top: entry.top,
        }
    }

    /// Finds room for `bitmap`, evicting if it must, and copies it in.
    fn insert(&mut self, bitmap: &Bitmap) -> Result<Entry, FontError> {
        let (width, height) = (bitmap.width, bitmap.height);
        let limit = self.glyph_limit();
        if width > limit || height > limit {
            // `rasterize_within` refuses these first: not reached.
            return Err(error(ErrorKind::GlyphTooLarge { limit }));
        }
        let area = u64::from(self.width) * u64::from(self.height);
        if self.entries.len() >= MAX_GLYPHS {
            self.evict(area / 2);
        }
        let mut budget = Some(area / 2);
        let (x, y) = loop {
            if let Some(at) = self.allocate(width, height) {
                break at;
            }
            // After a budget of 0 only empty glyphs are left, which take no
            // room, and the glyph is no larger than the atlas: not reached.
            let Some(this) = budget else {
                return Err(error(ErrorKind::GlyphTooLarge { limit }));
            };
            self.evict(this);
            budget = (this > 0).then_some(this / 2);
        };
        if width > 0 && height > 0 {
            let rows = bitmap
                .pixels
                .chunks_exact(width as usize)
                .take(height as usize);
            for (row, source) in rows.enumerate() {
                let start = (y as usize + row) * self.width as usize + x as usize;
                if let Some(target) = self.pixels.get_mut(start..start + width as usize) {
                    target.copy_from_slice(source);
                }
            }
        }
        Ok(Entry {
            x,
            y,
            width,
            height,
            left: bitmap.left,
            top: bitmap.top,
            last_used: self.clock,
        })
    }

    /// The top-left corner of room for a `width × height` image, on a shelf
    /// (the [module docs](self), "The image"), or `None` if there is none.
    fn allocate(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        if width == 0 || height == 0 {
            return Some((0, 0));
        }
        if width > self.width || height > self.height {
            return None;
        }
        let class = height.next_multiple_of(4).min(self.height);
        let mut best: Option<usize> = None;
        for (index, shelf) in self.shelves.iter().enumerate() {
            let fits = shelf.height >= height
                && shelf.height <= class + 4
                && self.width - shelf.x >= width;
            let lower = best
                .and_then(|best| self.shelves.get(best))
                .is_none_or(|best| shelf.height < best.height);
            if fits && lower {
                best = Some(index);
            }
        }
        if let Some(shelf) = best.and_then(|best| self.shelves.get_mut(best)) {
            let at = (shelf.x, shelf.y);
            shelf.x += width;
            return Some(at);
        }
        if self.height - self.bottom < class {
            return None;
        }
        let y = self.bottom;
        self.shelves.push(Shelf {
            y,
            height: class,
            x: width,
        });
        self.bottom += class;
        Some((0, y))
    }

    /// Keeps the most recently used glyphs that fill at most `budget`
    /// pixels, and half of [`MAX_GLYPHS`], and packs them again.
    fn evict(&mut self, budget: u64) {
        let mut entries: Vec<(GlyphKey, Entry)> = self.entries.drain().collect();
        // Most recent first: each call has its own stamp.
        entries.sort_unstable_by(|a, b| b.1.last_used.cmp(&a.1.last_used).then(a.0.cmp(&b.0)));
        let mut area = 0;
        let mut kept = entries.len();
        for (index, (_, entry)) in entries.iter().enumerate() {
            if index >= MAX_GLYPHS / 2 || area + entry.area() > budget {
                kept = index;
                break;
            }
            area += entry.area();
        }
        self.evictions += (entries.len() - kept) as u64;
        entries.truncate(kept);
        // Tallest first packs shelves best; the key makes the order total.
        entries.sort_unstable_by(|a, b| {
            b.1.height
                .cmp(&a.1.height)
                .then(b.1.width.cmp(&a.1.width))
                .then(a.0.cmp(&b.0))
        });
        let fresh = vec![0; self.pixels.len()];
        let old = std::mem::replace(&mut self.pixels, fresh);
        self.shelves.clear();
        self.bottom = 0;
        let stride = self.width as usize;
        for (key, mut entry) in entries {
            let Some((x, y)) = self.allocate(entry.width, entry.height) else {
                self.evictions += 1;
                continue;
            };
            for row in 0..entry.height as usize {
                let from = (entry.y as usize + row) * stride + entry.x as usize;
                let to = (y as usize + row) * stride + x as usize;
                let width = entry.width as usize;
                if let (Some(source), Some(target)) = (
                    old.get(from..from + width),
                    self.pixels.get_mut(to..to + width),
                ) {
                    target.copy_from_slice(source);
                }
            }
            entry.x = x;
            entry.y = y;
            self.entries.insert(key, entry);
        }
    }
}

impl fmt::Debug for Atlas {
    /// The size and the glyphs held, not the image.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Atlas")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("glyphs", &self.entries.len())
            .field("evictions", &self.evictions)
            .finish_non_exhaustive()
    }
}

/// Rasterizes the glyph `key` of `faces`, as the atlas does: its outline
/// at the key's size, moved right by its subpixel offset, with the limits
/// its face was parsed with. Glyph 0, `.notdef`, is a box when the face
/// [draws its own](Faces::draws_own_notdef), or when its glyph 0 cannot be
/// read or rasterized: from 0.05 to 0.55 of an em across and from the
/// baseline to 0.7 of an em, its stroke 0.06 of an em, or a pixel if that
/// is more (its advance is [`NOTDEF_ADVANCE`]).
///
/// For a glyph too large for an atlas, which a painter may draw from this
/// bitmap directly, at the cost of rasterizing it each time.
///
/// # Errors
///
/// [`FontError::Faces`] when `faces` does not hold the key's face; what
/// [`Font::outline`](crate::ttf::Font::outline) and
/// [`outline::rasterize`] return.
pub fn rasterize(faces: &Faces, key: GlyphKey) -> Result<Bitmap, FontError> {
    rasterize_within(faces, key, u32::MAX)
}

/// [`rasterize`], with the face's limit on a bitmap's side lowered to
/// `limit`; a bitmap over it is [`ErrorKind::GlyphTooLarge`] when that
/// limit is the lower.
fn rasterize_within(faces: &Faces, key: GlyphKey, limit: u32) -> Result<Bitmap, FontError> {
    let font = faces.font(key.face).ok_or(FontError::Faces {
        kind: fallback::ErrorKind::UnknownFace { face: key.face },
    })?;
    let own = font.limits();
    let limits = crate::Limits {
        max_bitmap_dimension: own.max_bitmap_dimension.min(limit),
        ..own
    };
    let px = key.size.px();
    let em = f32::from(font.units_per_em());
    let scale = px / em;
    let shift = key.subpixel.shift();
    let notdef = key.glyph == GlyphId(0);
    let result = if notdef && faces.draws_own_notdef(key.face) {
        notdef_box(em, px).and_then(|outline| outline::rasterize(&outline, scale, shift, limits))
    } else {
        let drawn = font
            .outline(key.glyph)
            .and_then(|outline| outline::rasterize(&outline, scale, shift, limits));
        match drawn {
            Err(_) if notdef => notdef_box(em, px)
                .and_then(|outline| outline::rasterize(&outline, scale, shift, limits)),
            drawn => drawn,
        }
    };
    match result {
        Err(FontError::Outline {
            kind: outline::ErrorKind::BitmapTooLarge { .. },
        }) if limit < own.max_bitmap_dimension => Err(error(ErrorKind::GlyphTooLarge { limit })),
        result => result,
    }
}

/// The `.notdef` box, in font units of `em` to the em, y up, at `px`
/// pixels per em: an outer contour clockwise, and a hole counter-clockwise
/// when the stroke leaves room for one.
fn notdef_box(em: f32, px: f32) -> Result<Outline, FontError> {
    let stroke = (0.06 * em).max(em / px);
    let (left, right, bottom, top) = (0.05 * em, (NOTDEF_ADVANCE - 0.05) * em, 0.0, 0.7 * em);
    let point = |x, y| OutlinePoint {
        x,
        y,
        on_curve: true,
    };
    let mut points = vec![
        point(left, bottom),
        point(left, top),
        point(right, top),
        point(right, bottom),
    ];
    let mut ends = vec![4];
    if right - left > 2.0 * stroke && top - bottom > 2.0 * stroke {
        points.extend([
            point(left + stroke, bottom + stroke),
            point(right - stroke, bottom + stroke),
            point(right - stroke, top - stroke),
            point(left + stroke, top - stroke),
        ]);
        ends.push(8);
    }
    Outline::new(points, ends)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subpixel_splits_to_the_nearest_quarter() {
        let split = |x: f32| {
            let (pixel, subpixel) = Subpixel::split(x);
            (pixel, subpixel.quarters())
        };
        assert_eq!(split(0.0), (0, 0));
        assert_eq!(split(10.3), (10, 1));
        assert_eq!(split(10.375), (10, 2), "a half rounds up");
        assert_eq!(split(10.9), (11, 0));
        assert_eq!(split(-0.1), (0, 0));
        assert_eq!(split(-0.2), (-1, 3));
        assert_eq!(split(-1.5), (-2, 2));
        assert_eq!(split(f32::NAN), (0, 0));
        assert_eq!(split(f32::INFINITY), (MAX_PIXEL, 0));
        assert_eq!(split(f32::NEG_INFINITY), (-MAX_PIXEL, 0));
        assert_eq!(split(1e30), (MAX_PIXEL, 0));
        assert_eq!(split(-3e9), (-MAX_PIXEL, 0));
        assert_eq!(split(1e9), (1_000_000_000, 0));
        // A glyph's offset adds to the furthest pixel without overflow.
        assert!(MAX_PIXEL.checked_add(1 << 23).is_some());
        assert_eq!(Subpixel::new(3).map(Subpixel::shift), Some(0.75));
        assert_eq!(Subpixel::new(4), None);
    }

    #[test]
    fn shelves_hold_glyphs_of_about_their_height() {
        let mut atlas = Atlas::new(32, 32).expect("a size");
        assert_eq!(atlas.allocate(10, 7), Some((0, 0)), "a shelf 8 tall");
        assert_eq!(atlas.allocate(10, 8), Some((10, 0)), "on it");
        assert_eq!(
            atlas.allocate(10, 3),
            Some((20, 0)),
            "4 taller than 3's class"
        );
        assert_eq!(atlas.allocate(10, 12), Some((0, 8)), "a shelf 12 tall");
        assert_eq!(
            atlas.allocate(2, 6),
            Some((30, 0)),
            "the lowest that holds it"
        );
        assert_eq!(atlas.allocate(10, 8), Some((10, 8)), "the first is full");
        assert_eq!(
            atlas.allocate(10, 2),
            Some((0, 20)),
            "the 12 is too tall: 4"
        );
        assert_eq!(atlas.allocate(20, 9), None, "no shelf, and no room for one");
        assert_eq!(
            atlas.allocate(0, 9),
            Some((0, 0)),
            "an empty glyph takes no room"
        );
        assert_eq!(atlas.allocate(33, 1), None, "wider than the atlas");
        assert_eq!(atlas.allocate(1, 33), None, "taller than the atlas");
        assert_eq!(atlas.allocate(20, 8), Some((0, 24)), "the last 8 rows");
        assert_eq!(atlas.allocate(1, 1), Some((10, 20)));
    }

    #[test]
    fn eviction_repacks_the_most_recent_and_keeps_their_pixels() {
        let mut atlas = Atlas::new(32, 32).expect("a size");
        assert_eq!(atlas.glyph_limit(), 16);
        let square = |value: u8| Bitmap {
            left: 0,
            top: 0,
            width: 16,
            height: 16,
            pixels: vec![value; 256],
        };
        let key = |value: u8| GlyphKey {
            face: FaceId(0),
            glyph: GlyphId(1),
            size: PxSize::from_units(u32::from(value)).expect("a size"),
            subpixel: Subpixel::ZERO,
        };
        for value in 1..=5 {
            atlas.clock += 1;
            let entry = atlas.insert(&square(value)).expect("fits");
            atlas.entries.insert(key(value), entry);
        }
        // Four fill the atlas; for the fifth, the two used last were kept
        // and packed again.
        assert_eq!((atlas.evictions(), atlas.len()), (2, 3));
        for value in 3..=5 {
            let view = atlas.view(atlas.entries[&key(value)]);
            let rows = (0..16).filter_map(|row| view.row(row));
            assert_eq!(rows.flatten().filter(|&&c| c == value).count(), 256);
        }
        // Over the limit: refused, and nothing changes.
        let wide = Bitmap {
            width: 17,
            height: 1,
            pixels: vec![1; 17],
            ..Bitmap::default()
        };
        assert_eq!(
            atlas.insert(&wide).map(|_| ()),
            Err(error(ErrorKind::GlyphTooLarge { limit: 16 }))
        );
        assert_eq!(atlas.len(), 3);
        atlas.clear();
        assert!(atlas.pixels().iter().all(|&c| c == 0), "cleared to 0");
    }

    #[test]
    fn atlases_check_their_size() {
        for (width, height) in [(0, 1), (1, 0), (MAX_SIDE + 1, 1), (1, MAX_SIDE + 1)] {
            let error = Atlas::new(width, height).expect_err("a bad size");
            assert_eq!(
                error,
                FontError::Atlas {
                    kind: ErrorKind::BadSize { width, height }
                }
            );
        }
        let atlas = Atlas::new(MAX_SIDE, 1).expect("the largest side");
        assert_eq!(atlas.pixels().len(), MAX_SIDE as usize);
        assert!(atlas.is_empty());
    }

    #[test]
    fn the_notdef_box_has_a_hole_when_its_stroke_leaves_room() {
        let outline = notdef_box(1000.0, 14.0).expect("finite");
        assert_eq!(outline.contour_ends(), [4, 8]);
        // At 2 px an em the stroke is half an em: no room for a hole.
        let outline = notdef_box(1000.0, 2.0).expect("finite");
        assert_eq!(outline.contour_ends(), [4]);
    }
}
