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

//! Workspaces: folders, trust, settings, files, watching, search, buffers and
//! session state.
//!
//! Specification: `docs/subsystems/editor.md`.
//!
//! What is built:
//!
//! - [`folder`]: a workspace root by its canonical path, its `.tidepool/`
//!   folder, and its `.tidepoolignore`, `.gitignore` and
//!   `.git/info/exclude` files, read as Git reads them (M4.7, ADR-0067);
//! - [`trust`]: the folders the user trusts (ADR-0014), in `trust.toml` in
//!   the user data folder, each grant covering the folders under it
//!   (M4.7);
//! - [`settings`]: typed settings in layers (the defaults, the user's
//!   `settings.toml`, a trusted folder's `.tidepool/settings.toml`), an
//!   invalid value falling back with a warning, and values written back
//!   with the file's comments and unknown keys kept (M4.7);
//! - [`buffers`]: open buffers keyed by canonical path, each a rope and
//!   its history, loaded and saved by jobs that workers run, a large file
//!   read-only until made editable, and a save that asks before its rename
//!   would split a hard link or change the file's owner or group (M4.11,
//!   ADR-0068, ADR-0069);
//! - [`files`]: a folder's file tree, read a folder at a time by jobs,
//!   each entry with its ignore state (M4.11);
//! - [`session`]: the editors, splits, selections and scroll a folder had,
//!   one file a folder in the user data folder (M4.11, ADR-0068).
//!
//! Everything here that reads or writes a file blocks, so call it on a
//! worker, never on the main thread (`AGENTS.md` rule 6); `buffers` and
//! `files` hand out jobs for that. The rest of the crate lands with M4.4
//! (`search`, and the file finder's index in `files`) and M4.5 (`watch`).

// rustdoc does not apply the workspace `unsafe_code = "deny"` to doc tests,
// so this forbids `unsafe` in them, README examples included (ADR-0036).
// `cargo xtask unsafe-audit` enforces that this line is here.
#![doc(test(attr(forbid(unsafe_code))))]

// The README's examples are compiled as doc tests too.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub mod buffers;
pub mod files;
pub mod folder;
pub mod session;
pub mod settings;
pub mod trust;

mod error;
mod read;

pub use error::WorkspaceError;
