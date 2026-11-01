//! Hand-written raw FFI declarations for the OS APIs Tidepool calls: libc,
//! X11, objc/AppKit, Win32 and ConPTY.
//!
//! Specification: `docs/subsystems/sys-and-platform.md`.

// `tidepool-sys` and `tidepool-platform` are the only two crates allowed to
// contain `unsafe` (AGENTS.md rule 4); every other crate inherits the
// workspace `unsafe_code = "deny"`. Raw OS declarations need `unsafe extern`
// blocks, so the lint is wrong here. `cargo xtask unsafe-audit` (M0.3) will
// enforce that this allow appears in no other crate.
#![allow(unsafe_code)]
