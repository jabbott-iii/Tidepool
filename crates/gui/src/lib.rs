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

//! The GUI frontend: a window, drawn by the CPU in jabbott.dev's look.
//!
//! Specification: `docs/subsystems/frontends.md`.
//!
//! Today it shows the first frame (M2.6, ADR-0045): [`Gui`] opens a window
//! through `tidepool-platform` and presents, each time the window is
//! exposed, what [`first_frame::FirstFrame`] draws with `gfx` and `font`:
//! jabbott.dev's backdrop and a line in each of the seven bundled faces.
//! The draw-list painter, which draws `ui`'s frames, comes with M3.5.

// rustdoc does not apply the workspace `unsafe_code = "deny"` to doc tests,
// so this forbids `unsafe` in them, README examples included (ADR-0036).
// `cargo xtask unsafe-audit` enforces that this line is here.
#![doc(test(attr(forbid(unsafe_code))))]

mod error;
pub mod first_frame;
mod frontend;

// The README's examples are compiled as doc tests too.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub use error::GuiError;
pub use frontend::{Gui, Turn};
