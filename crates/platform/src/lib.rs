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

//! Safe OS behavior: windows, events, the waker, present, clipboard,
//! terminal raw mode, PTY, file watching and paths.
//!
//! Specification: `docs/subsystems/sys-and-platform.md`; what each system
//! does, `docs/03-platforms.md`.
//!
//! Built so far: a [`window`](window::Window), its
//! [`events`](events::Event) and [waker](events::Waker), and
//! [`present`](window::Window::present) in [`surface`] (M2.2), the
//! [`clipboard`] (M2.7), on Linux, through X11; and on Linux, the user's
//! folders in [`paths`] and the tools' in [`dirs`] (M2.7), and in
//! [`file`](mod@file) a file's link count, owner, group and permission
//! bits and the owner a new file gets, which a save checks (M4.10). On
//! any other system [`Window::open`](window::Window::open), the folders
//! and the file metadata fail with [`Error::Unsupported`], until macOS
//! (M11) and Windows (M12).
//!
//! No `target_os` shows in the API: every type, method and error variant
//! exists on every system, and each says where it behaves differently.
//! The backends are private modules.

// `tidepool-sys` and `tidepool-platform` are the only two crates allowed to
// contain `unsafe` (AGENTS.md rule 4); every other crate inherits the
// workspace `unsafe_code = "deny"`. Calling the raw declarations in
// `tidepool-sys` and wrapping them safely needs `unsafe` blocks, so the lint
// is wrong here. `cargo xtask unsafe-audit` enforces that this allow
// appears in no other crate.
#![allow(unsafe_code)]

// The README's examples are compiled as doc tests too.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub mod clipboard;
pub mod dirs;
mod error;
pub mod events;
pub mod file;
pub mod paths;
pub mod surface;
pub mod window;

// The backend of this system: X11 on Linux, and on every other system one
// whose windows fail to open, until M11.7 and M12.1 add theirs.
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as backend;
#[cfg(not(target_os = "linux"))]
mod unsupported;
#[cfg(not(target_os = "linux"))]
use unsupported as backend;

pub use error::Error;
