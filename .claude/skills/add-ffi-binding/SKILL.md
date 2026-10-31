---
name: add-ffi-binding
description: Use whenever adding or changing OS bindings (libc, X11, Objective-C/AppKit/CoreGraphics, Win32, ConPTY). Declares items in tidepool-sys with layout assertions and source citations, then wraps them safely in tidepool-platform.
argument-hint: "<linux|macos|windows> <API or functions needed>"
---

# Add an FFI binding

Request: `$ARGUMENTS`

We use no binding crates (ADR-0001), so correctness depends on following
this procedure exactly. Nadir (same owner) uses the same procedure; check
whether it already declares what you need and port it with a note.

## 1. Scope it

- List the exact functions, types, constants or selectors the feature
  calls. **Declare nothing else.**
- For each item, find the authoritative source:
  - Linux: glibc headers (`/usr/include/*.h`, `sys/*.h`), X11 headers
    (`/usr/include/X11/*.h`), kernel uapi headers (`linux/inotify.h`)
  - macOS: SDK framework headers or Apple's developer documentation
  - Windows: Windows SDK headers (`um/*.h`) or Microsoft Learn
- If the header isn't available in this session, use the official
  documentation and mark the values "verify on <OS>".

## 2. Declare in `tidepool-sys`

- File: `crates/sys/src/<os>/<module>.rs`. Every item carries a doc comment
  or a `// source:` comment naming the header and declaration.
- Functions: `unsafe extern "system" { ... }` for Win32, `unsafe extern "C"
  { ... }` elsewhere. Libraries we `dlopen` (libX11) use the `dynlib!`
  macro, which reports every missing symbol by name.
- Types: `#[repr(C)]`, fields in header order with exact widths (`long` is
  `i64` on LP64 Linux and macOS, `i32` on Windows; objc `BOOL` is `bool` on
  arm64, `i8` on x86_64).
- Layout assertions for every struct, with numbers from the header or docs
  (never from the Rust struct you just wrote):
  ```rust
  const _: () = assert!(core::mem::size_of::<XEvent>() == 192);
  const _: () = assert!(core::mem::align_of::<XEvent>() == 8);
  const _: () = assert!(core::mem::offset_of!(XKeyEvent, keycode) == 84);
  ```
  Gate per target with `#[cfg(...)]` when values differ.
- Constants: the header's type and spelling (hex stays hex).
- Objective-C: cache selectors once; every `msg_send!` spells the full
  signature; `_stret` for big struct returns on x86_64; runtime-created
  classes list ivars and methods with type encodings in one place.

## 3. Wrap safely in `tidepool-platform`

- Safe API: owned RAII types for handles, `&str`/`&Path` instead of raw
  strings, our error enums instead of codes.
- One unsafe operation per block, each preceded by `// SAFETY:`.
- Document ownership (who frees what, with which call) and main-thread-only
  requirements; make main-thread types `!Send`.
- Convert every failure path into an error with context.

## 4. Test

- Layout assertions compile on the OS they target: say which OS builds you
  actually ran.
- Unit tests for wrapper logic that doesn't need the OS (key mapping
  tables, string conversion).
- Behavior tests under Xvfb on Linux where a display is needed.
- Run `cargo xtask unsafe-audit` (once it exists) and
  `cargo clippy -p <crate> --all-targets -- -D warnings`.
- Ask `security-reviewer` to review.

## 5. Report

- Each declared item and its source.
- Which layout assertions compiled on which OS in this session.
- Which items still need verification on another OS (also add a `⏳` note
  to the roadmap task).
