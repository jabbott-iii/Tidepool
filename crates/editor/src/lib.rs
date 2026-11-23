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

//! The editor view, modal keymaps (including vim mode) and all panels.
//!
//! Specification: `docs/subsystems/editor.md` and
//! `docs/subsystems/vim-mode.md`.
//!
//! Today it has the [`shell`]: the window's frame around the editor, in
//! jabbott.dev's look, built in a `ui` tree and answering its events
//! through `ui` alone, so any frontend can show it (M3.4, ADR-0060); and
//! the editor [`view`]: a rope's text with its gutter, cursors,
//! selections, current line, soft wrap and scrolling, which lays out only
//! the lines on screen, in a `ui` tree too (M4.1, ADR-0063). The view's
//! errors are [`EditorError`]s; the shell's are `ui`'s. The panels' content
//! comes with their tasks, and the binary shows a view from M4.3.

// rustdoc does not apply the workspace `unsafe_code = "deny"` to doc tests,
// so this forbids `unsafe` in them, README examples included (ADR-0036).
// `cargo xtask unsafe-audit` enforces that this line is here.
#![doc(test(attr(forbid(unsafe_code))))]

// The README's examples are compiled as doc tests too.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

mod error;
pub mod shell;
pub mod view;

pub use error::EditorError;
