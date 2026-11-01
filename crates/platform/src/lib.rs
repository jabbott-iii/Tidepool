//! Safe OS behavior: windows, events, the waker, present, clipboard,
//! terminal raw mode, PTY, file watching and paths.
//!
//! Specification: `docs/subsystems/sys-and-platform.md`.

// `tidepool-sys` and `tidepool-platform` are the only two crates allowed to
// contain `unsafe` (AGENTS.md rule 4); every other crate inherits the
// workspace `unsafe_code = "deny"`. Calling the raw declarations in
// `tidepool-sys` and wrapping them safely needs `unsafe` blocks, so the lint
// is wrong here. `cargo xtask unsafe-audit` (M0.3) will enforce that this
// allow appears in no other crate.
#![allow(unsafe_code)]
