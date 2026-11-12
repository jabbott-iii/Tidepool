# tidepool-sys

Hand-written raw FFI declarations for the OS APIs Tidepool calls: libc, X11,
the Objective-C runtime and AppKit, Win32 and ConPTY. It declares and does
nothing else.

It is part of [Tidepool](https://github.com/jabbott-iii/Tidepool), where
`tidepool-platform` is to wrap what it calls in a safe API, from M2.2, and
no other crate may use it.

Layer 0, foundations. It may use no workspace crate, as its row of the
dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Version 0.1, not published (`publish = false`): the API changes as the
roadmap needs.

Built, with the roadmap task that built it:

- the crate root, the one place besides `platform`'s that allows
  `unsafe_code` — M0.1.
- `dynlib`, ported from Nadir: the `Library` trait, through which a
  symbol table finds its functions in a library loaded while the program
  runs, and `MissingSymbols`, the error that names every function the
  library lacks; the `dynlib!` macro that makes the tables is private to
  the crate — M2.1.
- `linux::libc`: `dlopen`, `dlsym` and `dlerror` with `RTLD_NOW` and
  `RTLD_LOCAL` (no `dlclose`: a loaded library stays until the process
  exits, so a table never dangles); `poll` with `pollfd`, `nfds_t` and
  the `POLL*` bits it reports; and `pipe2` with `O_CLOEXEC` and
  `O_NONBLOCK`, for the waker's self-pipe, whose two ends are to go into
  `OwnedFd`: the event loop is to read its end through `File`, and
  `Waker::wake` to write with `write(2)` (M2.2) — M2.1.
- `linux::x11`, ported in part from Nadir: the Xlib types, constants and
  events (key, button, motion, focus, expose, configure, client message,
  in the 192-byte `XEvent`), the window's attributes and visual, `XImage`
  with its functions, `XErrorEvent` and the error handlers' types, the
  core font cursors, and `Xlib`, the table of the 31 functions in
  `libX11.so.6` that the window, its events and its presentation are to
  call (M2.2). Every struct and union with fields has its size,
  alignment and field offsets asserted (the opaque `Display`, `Visual`,
  `_XGC` and `XGCValues` have none), with numbers measured from the headers on x86_64
  Linux; they hold on aarch64 Linux too, and a Linux target other than
  x86_64 or aarch64 fails the build — M2.1.

Not yet, with the roadmap task that adds it:

- the key lookup (`docs/03-platforms.md` names `XLookupKeysym`;
  `tidepool_core::input::Key` is a physical key, which M2.2 reconciles)
  and typed text (an X input context for `Xutf8LookupString`) — M2.2.
- `write(2)`, for code that must be async-signal-safe, which `std`
  does not promise for `File`'s `write`: `Waker::wake` — M2.2; the
  terminal's signal handler — M7.1.
- the selections and window properties the clipboard reads — M2.7.
- file watching (inotify) — M4.5.
- the declarations for processes and the PTY — M5.7, M5.9.
- the terminal's raw mode and its signals — M7.1.
- macOS — M11.7, M11.8.
- Windows — M12.1, M12.2.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Usage

Only `tidepool-platform` is to depend on it
([`docs/01-architecture.md`](../../docs/01-architecture.md), principle 2:
nothing else touches an OS API). It adds the dependency, by path, in M2.2, and wraps what it calls in a
safe API:

```toml
[dependencies]
tidepool-sys = { path = "../sys" }
```

A symbol table that is loaded from a library lacking some of its
functions fails with one error that names them all:

```rust
use tidepool_sys::dynlib::MissingSymbols;

let error = MissingSymbols {
    names: vec!["XOpenDisplay", "XPending"],
};
assert_eq!(error.to_string(), "missing symbols: XOpenDisplay, XPending");
```

On Linux, Xlib is loaded through a `Library` of the caller's own, as
`tidepool-platform` is to load it from M2.2. Loading it opens no display:

```rust,no_run
# #[cfg(target_os = "linux")]
# fn main() -> Result<(), Box<dyn std::error::Error>> {
use core::ffi::{CStr, c_void};

use tidepool_sys::dynlib::Library;
use tidepool_sys::linux::libc::{RTLD_LOCAL, RTLD_NOW, dlopen, dlsym};
use tidepool_sys::linux::x11::Xlib;

/// A library from `dlopen`, which is never closed.
struct Loaded(*mut c_void);

impl Library for Loaded {
    fn symbol(&self, name: &CStr) -> *const c_void {
        // SAFETY: the handle came from `dlopen` and is never closed, and
        // `name` ends with a NUL.
        unsafe { dlsym(self.0, name.as_ptr()) }
    }
}

// SAFETY: the name ends with a NUL, and Xlib is sound to load.
let handle = unsafe { dlopen(c"libX11.so.6".as_ptr(), RTLD_NOW | RTLD_LOCAL) };
if handle.is_null() {
    return Err("could not load libX11.so.6".into());
}
// SAFETY: the library is Xlib, which the table was written for, and it
// stays loaded until the process exits.
let xlib = unsafe { Xlib::load(&Loaded(handle)) }?;
# let _ = xlib;
# Ok(())
# }
# #[cfg(not(target_os = "linux"))]
# fn main() {}
```

## Documentation

- Design:
  [`docs/subsystems/sys-and-platform.md`](../../docs/subsystems/sys-and-platform.md)
- Platforms: [`docs/03-platforms.md`](../../docs/03-platforms.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0001 (no
  binding crates); ADR-0010 (X11 through `dlopen`).
- API reference: `cargo doc -p tidepool-sys --open`

## Rules

- `unsafe` is allowed here: this is one of the two crates that may hold it
  (`AGENTS.md` rule 4). Each block carries a `// SAFETY:` comment, and each
  `pub unsafe fn` a `# Safety` section.
- Path-scoped rules in `.claude/rules/`:
  [`unsafe-and-ffi.md`](../../.claude/rules/unsafe-and-ffi.md),
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.
- Declare only what is called, cite the header for each item, and assert
  every struct's layout with numbers measured from the headers, never
  read off the Rust types: follow `/add-ffi-binding`.

## Checks

```bash
cargo build -p tidepool-sys
cargo test -p tidepool-sys
cargo clippy -p tidepool-sys --all-targets --target aarch64-unknown-linux-gnu -- -D warnings
cargo xtask unsafe-audit
```

Checking a target evaluates its layout assertions, so the third line
checks aarch64 Linux's (after `rustup target add
aarch64-unknown-linux-gnu`). The test that loads all of `Xlib` from
`libX11.so.6`, opening no display, prints `SKIPPED:` on a machine
without it. `cargo xtask ci` runs the whole gate.

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
