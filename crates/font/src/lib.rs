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
//! Every font is treated as untrusted: [`ttf::Font::parse`] checks each
//! table's offsets and lengths, and everything later lookups rely on,
//! against the file and [`Limits`] before reading or allocating; a
//! malformed font is a [`FontError`] that names the table (or glyph), the
//! byte offset and what was wrong, never a panic. The crate is pure: no OS
//! calls, and the same coverage for a glyph on every run and OS.
//!
//! Built so far: [`ttf`] (the TrueType parser: tables, character map,
//! metrics, names, kerning and glyph outlines) and [`outline`] (outlines
//! and their exact-area, unhinted rasterizer), and the bundled Saira and
//! JetBrains Mono files in `assets/fonts/` (M2.8). The atlas, layout,
//! fallback and `bundled` (loading the faces in `assets/fonts/` at
//! startup) arrive with M2.5.

// rustdoc does not apply the workspace `unsafe_code = "deny"` to doc tests,
// so this forbids `unsafe` in them, README examples included (ADR-0036).
// `cargo xtask unsafe-audit` enforces that this line is here.
#![doc(test(attr(forbid(unsafe_code))))]

mod error;
mod limits;
pub mod outline;
pub mod ttf;

// The README's examples are compiled as doc tests too.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub use error::FontError;
pub use limits::Limits;
