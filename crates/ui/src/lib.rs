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

//! The UI toolkit: widget tree, layout, focus, the jabbott.dev theme, draw
//! lists and the accessibility tree.
//!
//! Specification: `docs/subsystems/ui-and-theme.md`.
//!
//! A [`Tree`] is retained: the owner (`editor`, `app`) builds it once and
//! changes it as the program runs, naming nodes by [`NodeId`]s that a
//! removal never lets name another node. Each node has a [`Style`],
//! [`Semantics`] and at most one [`Widget`], which measures the node's
//! content, paints it and handles the events routed to it; the tree tracks
//! each node's interaction state, hovered, pressed, focused, focus showing
//! and disabled, which a widget paints from (ADR-0046).
//!
//! [`layout`] lays the tree out in logical units, the same for every
//! frontend, measuring text through the [`TextMetrics`] the frontend gives
//! it: rows, columns, stacks, grids and splits, with fixed, natural and
//! filling lengths, padding, gaps, alignment and scrolling. Children never
//! overflow a parent that does not scroll.
//!
//! [`input`] routes each `core::input` event to the focused node, or to the
//! node under the pointer (or the one a press captured), and up through its
//! ancestors until a widget handles it; what none handles, after the
//! tree's defaults (Tab moves [`focus`], the wheel scrolls), comes back to
//! the owner, with the actions widgets emitted.
//!
//! [`draw`]: each frame repaints only the nodes whose data, state or layout
//! changed, damages what their paint changed, and returns the whole tree's
//! draw list, in resolved colors, with that damage; an idle tree returns
//! nothing. [`a11y`] snapshots the accessibility tree, in which every
//! focusable node has a role and a name.
//!
//! [`theme`] reads a theme file's tokens into a [`Theme`], and bundles
//! `tidepool-dark`, jabbott.dev's tokens (ADR-0047).
//!
//! `ui` depends on `tidepool-core`, whose geometry, colors and input types
//! it uses, and on `tidepool-codec`, whose TOML parser reads theme files.
//! It has no `unsafe`, no OS calls and no fonts.

// rustdoc does not apply the workspace `unsafe_code = "deny"` to doc tests,
// so this forbids `unsafe` in them, README examples included (ADR-0036).
// `cargo xtask unsafe-audit` enforces that this line is here.
#![doc(test(attr(forbid(unsafe_code))))]

pub mod a11y;
pub mod draw;
mod error;
pub mod focus;
pub mod input;
pub mod layout;
pub mod metrics;
pub mod theme;
pub mod tree;

// The README's examples are compiled as doc tests too.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub use a11y::{Role, Semantics};
pub use draw::{Command, DrawList, Frame};
pub use error::UiError;
pub use input::{Handled, Response};
pub use layout::Style;
pub use metrics::TextMetrics;
pub use theme::{Theme, Token};
pub use tree::{Node, NodeId, NodeState, Tree, Widget};
