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

//! Hand-written raw FFI declarations for the OS APIs Tidepool calls: libc,
//! X11, objc/AppKit, Win32 and ConPTY.
//!
//! Specification: `docs/subsystems/sys-and-platform.md`.
//!
//! This crate declares and does nothing else: types, constants, `extern`
//! blocks, function types and symbol tables, each with the header it comes
//! from, and layout assertions. Its only code is the tables' loaders and
//! the message of the error they return.
//! Only `tidepool-platform` may use it, and from M2.2 it wraps what it
//! calls in a safe API.
//!
//! On every system, [`dynlib`]: the [`Library`](dynlib::Library) trait,
//! through which a symbol table looks its functions up in a library loaded
//! while the program runs, and [`MissingSymbols`](dynlib::MissingSymbols),
//! the error that names every function the library lacks. The `dynlib!`
//! macro that makes the tables is private to this crate.
//!
//! On Linux, the module `linux`: `linux::libc` (the dynamic loader, `poll`
//! and `pipe2`) and `linux::x11` (Xlib's types, constants and events, and
//! `Xlib`, the table of its functions in `libX11.so.6`). A system's module
//! exists only on that system. macOS and Windows are declared from M11.7
//! and M12.1.

// `tidepool-sys` and `tidepool-platform` are the only two crates allowed to
// contain `unsafe` (AGENTS.md rule 4); every other crate inherits the
// workspace `unsafe_code = "deny"`. Raw OS declarations need `unsafe extern`
// blocks, so the lint is wrong here. `cargo xtask unsafe-audit` enforces
// that this allow appears in no other crate.
#![allow(unsafe_code)]

// The README's examples are compiled as doc tests too.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub mod dynlib;

#[cfg(target_os = "linux")]
pub mod linux;
