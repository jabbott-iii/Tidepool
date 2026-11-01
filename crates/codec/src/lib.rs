//! Data formats: JSON (with comments), TOML, XML, plist, a YAML subset, CSV,
//! INI, DEFLATE, ZIP, PNG and QOI.
//!
//! Specification: `docs/subsystems/text-and-formats.md`.
//!
//! Every parser treats its input as untrusted: it checks sizes against
//! [`Limits`] before it allocates, bounds nesting by an explicit depth limit,
//! returns a [`CodecError`] that says where the input went wrong and what was
//! expected there, and never panics, whatever the input.
//!
//! Formats so far: [`json`] (JSON and JSONC). The others arrive with their
//! roadmap tasks.
//!
//! Parts of this crate are ported from Nadir's `nadir-codec` (same owner);
//! each ported file says so in its header. Tidepool does not depend on Nadir.

mod error;
pub mod json;
mod limits;

pub use error::CodecError;
pub use limits::Limits;
