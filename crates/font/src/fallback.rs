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

//! The faces text is set in, and the fallback chain: [`Faces`] holds
//! parsed fonts, each addressed by a [`FaceId`], and [`Faces::resolve`]
//! finds the face and glyph that draw a character.
//!
//! New in Tidepool; not ported (Nadir draws text with OpenGL).
//!
//! A character is drawn by the first of these that maps it:
//!
//! 1. the face asked for (a [`Style`](crate::layout::Style)'s);
//! 2. each face of the fallback chain, in order, skipping the face asked
//!    for: [`bundled::load`](crate::bundled::load) makes the chain the
//!    bundled JetBrains Mono, and the caller adds a system face after it
//!    when one is configured ([`Faces::add`], then
//!    [`Faces::set_fallback`]);
//! 3. else the `.notdef` glyph (glyph 0) of the face asked for: a visible
//!    box. When that face's glyph 0 has no outline, one that cannot be
//!    read, one that is flat, or one that reaches more than an em beyond
//!    the em square, a box is made for it instead
//!    ([`Faces::draws_own_notdef`]); and the
//!    [`atlas`](crate::atlas::rasterize) draws that box too when glyph 0
//!    cannot be rasterized; so that a missing character is not invisible.
//!
//! Only the character map decides: there is no shaping, so a character a
//! face maps is drawn by it even where another face would draw it better.
//!
//! This crate reads no file: a system face is read and parsed by the
//! caller, off the main thread (the [`Limits`](crate::Limits) docs say
//! what parsing costs), and added here.
//!
//! ```
//! # fn main() -> Result<(), tidepool_font::FontError> {
//! use tidepool_font::bundled::{self, Face};
//!
//! let faces = bundled::load()?;
//! let saira = Face::SairaRegular.id();
//! // Saira has an A; it has no λ, which JetBrains Mono has; and neither has
//! // 中, which falls back to Saira's `.notdef` box.
//! assert_eq!(faces.resolve(saira, 'A')?.face, saira);
//! assert_eq!(faces.resolve(saira, 'λ')?.face, Face::MonoRegular.id());
//! let missing = faces.resolve(saira, '中')?;
//! assert_eq!(missing.face, saira);
//! assert!(missing.is_notdef());
//! # Ok(())
//! # }
//! ```

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::FontError;
use crate::outline::Outline;
use crate::ttf::{Font, GlyphId};

/// The most faces a [`Faces`] holds: the seven bundled ones, and room for
/// the system faces a user may configure.
pub const MAX_FACES: usize = 64;

/// A face of a [`Faces`]: the index it was added at. Ids are never reused
/// within one [`Faces`]; an id from another [`Faces`] may name another
/// face, or none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FaceId(pub u16);

/// The face and glyph that draw a character: what [`Faces::resolve`]
/// gives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Resolved {
    /// The face.
    pub face: FaceId,
    /// The glyph in it: glyph 0, `.notdef`, when no face maps the
    /// character.
    pub glyph: GlyphId,
}

impl Resolved {
    /// Whether the glyph is `.notdef`: no face maps the character.
    pub fn is_notdef(&self) -> bool {
        self.glyph == GlyphId(0)
    }
}

/// Why [`Faces`] refused a call. See [`FontError::Faces`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The face is not one of this [`Faces`]' faces.
    UnknownFace {
        /// The id given.
        face: FaceId,
    },
    /// [`Faces::add`] would hold more than [`MAX_FACES`] faces.
    TooManyFaces {
        /// The limit.
        limit: usize,
    },
    /// [`Faces::set_fallback`] was given a face twice.
    RepeatedFallback {
        /// The face given twice.
        face: FaceId,
    },
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownFace { face } => write!(f, "there is no face {}", face.0),
            Self::TooManyFaces { limit } => {
                write!(f, "a set of faces holds at most {limit}")
            }
            Self::RepeatedFallback { face } => {
                write!(f, "face {} is in the fallback chain twice", face.0)
            }
        }
    }
}

fn error(kind: ErrorKind) -> FontError {
    FontError::Faces { kind }
}

/// Gives each [`Faces`] its own identity, so that an
/// [`Atlas`](crate::atlas::Atlas) can tell when it is handed another.
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// A face and what was found about it when it was added.
#[derive(Debug)]
struct Entry {
    font: Font,
    /// Its glyph 0 has no outline, or one that cannot be read, so a box is
    /// drawn for `.notdef` instead.
    own_notdef: bool,
}

/// Parsed faces, each addressed by a [`FaceId`], and the fallback chain
/// that resolves characters (the [module docs](self)).
///
/// Faces are only added, never removed or replaced, so a [`FaceId`] keeps
/// naming the same face. Each [`Faces`] has an identity of its own (it is
/// not `Clone`), which an [`Atlas`](crate::atlas::Atlas) checks so that
/// it never draws one set's glyphs for another's. `Debug` shows the
/// number of faces and the chain.
pub struct Faces {
    id: u64,
    entries: Vec<Entry>,
    chain: Vec<FaceId>,
}

impl Faces {
    /// A set with no faces and an empty fallback chain.
    pub fn new() -> Self {
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            entries: Vec::new(),
            chain: Vec::new(),
        }
    }

    /// Adds `font`, giving it the next id: the first face added is
    /// `FaceId(0)`. It is not put in the fallback chain.
    ///
    /// This reads the face's glyph 0, to know whether to make a box for
    /// `.notdef` ([`Faces::draws_own_notdef`]).
    ///
    /// # Errors
    ///
    /// [`FontError::Faces`] with [`ErrorKind::TooManyFaces`] when the set
    /// already holds [`MAX_FACES`].
    pub fn add(&mut self, font: Font) -> Result<FaceId, FontError> {
        if self.entries.len() >= MAX_FACES {
            return Err(error(ErrorKind::TooManyFaces { limit: MAX_FACES }));
        }
        let id = FaceId(
            u16::try_from(self.entries.len())
                .map_err(|_| error(ErrorKind::TooManyFaces { limit: MAX_FACES }))?,
        );
        let own_notdef = font
            .outline(GlyphId(0))
            .map_or(true, |outline| !drawable(&outline, font.units_per_em()));
        self.entries.push(Entry { font, own_notdef });
        Ok(id)
    }

    /// The face `face`, or `None` if this set has none of that id.
    pub fn font(&self, face: FaceId) -> Option<&Font> {
        self.entry(face).map(|entry| &entry.font)
    }

    /// How many faces the set holds.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the set holds no face.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The fallback chain, in the order it is tried.
    pub fn fallback(&self) -> &[FaceId] {
        &self.chain
    }

    /// Makes `chain` the fallback chain: the faces tried, in order, for a
    /// character the face asked for does not map.
    ///
    /// # Errors
    ///
    /// [`FontError::Faces`], leaving the chain as it was, with
    /// [`ErrorKind::UnknownFace`] for a face this set does not hold, or
    /// [`ErrorKind::RepeatedFallback`] for one given twice.
    pub fn set_fallback(&mut self, chain: &[FaceId]) -> Result<(), FontError> {
        for (index, &face) in chain.iter().enumerate() {
            if self.entry(face).is_none() {
                return Err(error(ErrorKind::UnknownFace { face }));
            }
            if chain
                .get(..index)
                .is_some_and(|before| before.contains(&face))
            {
                return Err(error(ErrorKind::RepeatedFallback { face }));
            }
        }
        self.chain = chain.to_vec();
        Ok(())
    }

    /// The face and glyph that draw `c` when `primary` is asked for: the
    /// first of `primary` and the fallback chain that maps it, else
    /// `primary`'s `.notdef` (the [module docs](self)).
    ///
    /// # Errors
    ///
    /// [`FontError::Faces`] with [`ErrorKind::UnknownFace`] when the set
    /// does not hold `primary`.
    pub fn resolve(&self, primary: FaceId, c: char) -> Result<Resolved, FontError> {
        let font = self
            .font(primary)
            .ok_or_else(|| error(ErrorKind::UnknownFace { face: primary }))?;
        Ok(self.resolve_in(primary, font, c))
    }

    /// [`Faces::resolve`] for a `primary` known to be `font`.
    pub(crate) fn resolve_in(&self, primary: FaceId, font: &Font, c: char) -> Resolved {
        if let Some(glyph) = font.glyph_index(c) {
            return Resolved {
                face: primary,
                glyph,
            };
        }
        for &face in &self.chain {
            if face == primary {
                continue;
            }
            if let Some(glyph) = self.font(face).and_then(|font| font.glyph_index(c)) {
                return Resolved { face, glyph };
            }
        }
        Resolved {
            face: primary,
            glyph: GlyphId(0),
        }
    }

    /// Whether `.notdef` in `face` is drawn as a box this crate makes,
    /// because the face's glyph 0 has no outline, one that cannot be read,
    /// one that is flat (no width or no height), or one whose control box
    /// reaches more than an em beyond the em square: `false` for every
    /// bundled face, whose glyph 0 is a box. The box is [`NOTDEF_ADVANCE`]
    /// of an em wide, with the outline
    /// [`atlas::rasterize`](crate::atlas::rasterize) draws.
    pub fn draws_own_notdef(&self, face: FaceId) -> bool {
        self.entry(face).is_some_and(|entry| entry.own_notdef)
    }

    /// The identity [`Atlas`](crate::atlas::Atlas) checks.
    pub(crate) fn identity(&self) -> u64 {
        self.id
    }

    fn entry(&self, face: FaceId) -> Option<&Entry> {
        self.entries.get(usize::from(face.0))
    }
}

/// The advance of the `.notdef` box this crate makes, in ems
/// ([`Faces::draws_own_notdef`]).
pub const NOTDEF_ADVANCE: f32 = 0.6;

/// Whether a face's glyph 0, `outline`, draws a visible `.notdef` at a
/// size: it has width and height, and its control box lies within an em of
/// the em square, `em` units to the em.
fn drawable(outline: &Outline, em: u16) -> bool {
    let Some(bounds) = outline.control_box() else {
        return false;
    };
    let em = f32::from(em);
    bounds.x_max > bounds.x_min
        && bounds.y_max > bounds.y_min
        && bounds.x_min >= -em
        && bounds.y_min >= -em
        && bounds.x_max <= 2.0 * em
        && bounds.y_max <= 2.0 * em
}

impl Default for Faces {
    /// [`Faces::new`].
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Faces {
    /// The number of faces and the chain, not the fonts.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Faces")
            .field("faces", &self.entries.len())
            .field("fallback", &self.chain)
            .finish_non_exhaustive()
    }
}
