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

//! The CPU rasterizer, backdrop cache and damage tracking.
//!
//! Specification: `docs/subsystems/rendering.md`.
//!
//! `gfx` draws into a [`Framebuffer`](framebuffer::Framebuffer) of
//! premultiplied BGRA8 pixels in device space: rects, hairlines, borders,
//! bracket corners, anti-aliased polygons, lines, cut corners and pills
//! ([`raster`]); images ([`image`]); coverage masks such as glyphs
//! ([`glyph`]); and layers with an opacity ([`blend`]). Every draw call
//! takes a clip rect ([`clip`]) and touches nothing outside it.
//! [`damage`] tracks what changed, and [`backdrop`] renders and caches
//! jabbott.dev's deep-space backdrop and its blurred copy for translucent
//! panels.
//!
//! The crate is pure and deterministic: no `unsafe`, no OS calls, and only
//! integer arithmetic or IEEE 754 operations that every CPU computes
//! exactly the same, so the same calls give the same bytes on every OS,
//! and golden frames are shared (M2.4). No draw call panics, whatever its
//! geometry.

// rustdoc does not apply the workspace `unsafe_code = "deny"` to doc tests,
// so this forbids `unsafe` in them, README examples included (ADR-0036).
// `cargo xtask unsafe-audit` enforces that this line is here.
#![doc(test(attr(forbid(unsafe_code))))]

pub mod backdrop;
pub mod blend;
pub mod clip;
pub mod damage;
mod error;
pub mod framebuffer;
pub mod glyph;
pub mod image;
pub mod raster;

// The README's examples are compiled as doc tests too.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub use error::GfxError;
