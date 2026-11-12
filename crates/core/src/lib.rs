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

//! Tidepool foundations: errors, logging, the test harness, RNG, hashing
//! (including SHA-256 for downloads), CLI parsing, time, geometry, color and
//! input types, and path helpers.
//!
//! Specification: `docs/subsystems/core.md`.
//!
//! Several modules are ported from Nadir's `nadir-core` (same owner); each
//! says so in its header. Tidepool does not depend on Nadir. This crate has
//! no dependencies, no `unsafe` and no OS-specific code.

// rustdoc does not apply the workspace `unsafe_code = "deny"` to doc tests,
// so this forbids `unsafe` in them, README examples included (ADR-0036).
// `cargo xtask unsafe-audit` enforces that this line is here.
#![doc(test(attr(forbid(unsafe_code))))]

pub mod cancel;
pub mod civil;
pub mod cli;
pub mod color;
pub mod error;
pub mod geom;
pub mod hash;
pub mod input;
pub mod log;
pub mod path;
pub mod rng;
pub mod testing;

// The README's examples are compiled as doc tests too.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub use cancel::{CancelToken, Deadline};
pub use color::{Color, PremultipliedColor};
pub use error::{CoreError, ErrorContext, WithContext};
pub use hash::{FastHashMap, FastHashSet};
pub use input::{InputEvent, Key, Modifiers, MouseButton};
pub use rng::Pcg32;
