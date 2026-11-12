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

//! Data formats: JSON (with comments), TOML, XML, plist, a YAML subset, CSV,
//! INI, DEFLATE, ZIP, PNG and QOI.
//!
//! Specification: `docs/subsystems/text-and-formats.md`.
//!
//! Every parser treats its input as untrusted: it checks sizes against
//! [`Limits`] before it allocates, bounds nesting by an explicit depth limit,
//! returns a [`CodecError`] that says where the input went wrong and what was
//! expected there, and never panics, whatever the input.
//!
//! Formats so far: [`json`] (JSON and JSONC) and [`toml`] (TOML 1.1, with a
//! document model that edits a document and keeps the rest of its text).
//! The others arrive with their roadmap tasks.
//!
//! Parts of this crate are ported from Nadir's `nadir-codec` (same owner);
//! each ported file says so in its header. Tidepool does not depend on Nadir.

// rustdoc does not apply the workspace `unsafe_code = "deny"` to doc tests,
// so this forbids `unsafe` in them, README examples included (ADR-0036).
// `cargo xtask unsafe-audit` enforces that this line is here.
#![doc(test(attr(forbid(unsafe_code))))]

mod error;
pub mod json;
mod limits;
pub mod toml;

// The README's examples are compiled as doc tests too.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub use error::CodecError;
pub use limits::Limits;
