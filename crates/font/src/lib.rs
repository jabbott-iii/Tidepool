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

//! TrueType parsing, the glyph rasterizer, the glyph atlas and text layout.
//!
//! Specification: `docs/subsystems/rendering.md`.
//!
//! ```
//! # fn main() -> Result<(), tidepool_font::FontError> {
//! use tidepool_font::atlas::{Atlas, GlyphKey, Subpixel};
//! use tidepool_font::bundled::{self, Face};
//! use tidepool_font::layout::{self, PxSize, Style};
//!
//! // The bundled faces, the fallback chain JetBrains Mono.
//! let faces = bundled::load()?;
//! let style = Style::new(Face::SairaRegular.id(), PxSize::new(13.0).expect("a size"));
//! let line = layout::line(&faces, "Hello, world", &style)?;
//! let mut atlas = Atlas::new(512, 512)?;
//! for glyph in &line.glyphs {
//!     let Some(id) = glyph.glyph else { continue };
//!     let (_pixel, subpixel) = Subpixel::split(glyph.x);
//!     let image = atlas.glyph(&faces, GlyphKey { face: glyph.face, glyph: id, size: style.size, subpixel })?;
//!     assert_eq!(image.row(0).map(<[u8]>::len), (image.height > 0).then_some(image.width as usize));
//! }
//! # Ok(())
//! # }
//! ```
//!
//! Every font is treated as untrusted: [`ttf::Font::parse`] checks each
//! table's offsets and lengths, and everything later lookups rely on,
//! against the file and [`Limits`] before reading or allocating; a
//! malformed font is a [`FontError`] that names the table (or glyph), the
//! byte offset and what was wrong, never a panic. The crate is pure: no OS
//! calls, and the same coverage for a glyph on every run and OS.
//!
//! The modules: [`ttf`] (the TrueType parser: tables, character map,
//! metrics, names, kerning and glyph outlines) and [`outline`] (outlines
//! and their exact-area, unhinted rasterizer), with the bundled Saira and
//! JetBrains Mono files in `assets/fonts/` (M2.8); and [`bundled`] (those
//! faces, compiled in and parsed at startup), [`fallback`] (the faces text
//! is set in, and the chain a missing character falls back through),
//! [`layout`] (lines of positioned glyphs: kerning, letter-spacing, tabs,
//! ellipsis and wrapping) and [`atlas`] (rasterized glyphs cached in one
//! coverage image, evicting the least recently used) (M2.5).

// rustdoc does not apply the workspace `unsafe_code = "deny"` to doc tests,
// so this forbids `unsafe` in them, README examples included (ADR-0036).
// `cargo xtask unsafe-audit` enforces that this line is here.
#![doc(test(attr(forbid(unsafe_code))))]

pub mod atlas;
pub mod bundled;
mod error;
pub mod fallback;
pub mod layout;
mod limits;
pub mod outline;
pub mod ttf;

// The README's examples are compiled as doc tests too.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub use error::FontError;
pub use limits::Limits;
