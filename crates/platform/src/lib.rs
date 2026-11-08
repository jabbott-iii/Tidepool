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
//! Specification: `docs/subsystems/sys-and-platform.md`.

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
