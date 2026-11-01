//! Tidepool foundations: errors, logging, the test harness, RNG, hashing
//! (including SHA-256 for downloads), CLI parsing, time, geometry, color and
//! input types.
//!
//! Specification: `docs/subsystems/core.md`.
//!
//! Several modules are ported from Nadir's `nadir-core` (same owner); each
//! says so in its header. Tidepool does not depend on Nadir. This crate has
//! no dependencies, no `unsafe` and no OS-specific code.

pub mod cancel;
pub mod civil;
pub mod cli;
pub mod color;
pub mod error;
pub mod geom;
pub mod hash;
pub mod input;
pub mod log;
pub mod rng;
pub mod testing;

pub use cancel::{CancelToken, Deadline};
pub use color::{Color, PremultipliedColor};
pub use error::{CoreError, ErrorContext, WithContext};
pub use hash::{FastHashMap, FastHashSet};
pub use input::{InputEvent, Key, Modifiers, MouseButton};
pub use rng::Pcg32;
