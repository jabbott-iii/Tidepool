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
//! - [`Painter`] paints `ui`'s frames, their damage only, into a
//!   framebuffer with `gfx` and `font`: every draw-list command, the icons
//!   ([`Icon`]) and images registered with it, clips and layers; and
//!   [`FontMetrics`] measures text for `ui` as the painter draws it (M3.5,
//!   ADR-0048).
//! - [`Gui`] opens a window through `tidepool-platform` and shows `ui`'s
//!   frames in it, for the `tidepool` binary's event-driven main loop
//!   (M3.8, ADR-0012, ADR-0059): [`Gui::wait_events`] waits on the window,
//!   and [`Gui::render`] lays a tree out at the window's size and scale and
//!   paints its damage with the [`Painter`] over jabbott.dev's backdrop,
//!   which is built on threads of its own, over the theme's flat `bg`
//!   until it arrives. The first present waits for the backdrop, up to
//!   [`FIRST_FRAME_WAIT`].
//! - [`icons`] is Tidepool's icon set, its first six the shell's activity
//!   bar's (M3.4, ADR-0060): [`icons::files`], [`icons::search`],
//!   [`icons::branch`], [`icons::wrench`], [`icons::device`] and
//!   [`icons::spark`], line art in a 24-unit viewbox, crisp at scales 1,
//!   1.5 and 2. [`Gui::set_icon`] registers an icon for a `ui` icon id
//!   with the window's [`Painter`], and repaints the whole window.
//! - [`first_frame::tree`] is the first frame's content, a line in each of
//!   the seven bundled faces (M2.6, ADR-0045), as a `ui` tree: what the
//!   window showed until the `tidepool` binary showed `editor`'s shell
//!   (M3.4), and what `gui`'s window test still shows.

// rustdoc does not apply the workspace `unsafe_code = "deny"` to doc tests,
// so this forbids `unsafe` in them, README examples included (ADR-0036).
// `cargo xtask unsafe-audit` enforces that this line is here.
#![doc(test(attr(forbid(unsafe_code))))]

mod builds;
mod error;
pub mod first_frame;
mod frontend;
pub mod icon;
pub mod icons;
pub mod metrics;
pub mod paint;
mod screen;

// The README's examples are compiled as doc tests too.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub use error::GuiError;
pub use frontend::{FIRST_FRAME_WAIT, Gui};
pub use icon::Icon;
pub use metrics::FontMetrics;
pub use paint::Painter;
