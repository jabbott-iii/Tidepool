# Subsystem: `tidepool-sys` and `tidepool-platform`

Read `docs/03-platforms.md` for per-OS details. This file defines the crate
structure and the API every other crate sees. The approach follows Nadir's
`nadir-sys`/`nadir-platform`, which already has working X11 and Objective-C
patterns to port.

## `tidepool-sys`: declarations only

- **Contains only:** `#[repr(C)]` types, constants, `unsafe extern` blocks,
  function-pointer type aliases, `dlopen` symbol tables, and layout tests.
- **No behavior:** no wrappers or loops. The only code is the generated
  symbol-table loaders and the message of the error they return; a loader
  allocates only to list the symbols a library lacks.

```
crates/sys/src/
├── lib.rs                # #![allow(unsafe_code)], cfg-gated modules
├── dynlib.rs             # dynlib! macro (crate-private): struct of fn pointers + load(&dyn Library),
│                         # MissingSymbols naming every symbol a library lacks
├── linux.rs              # the Linux modules; fails the build on a Linux target but x86_64 or aarch64
├── linux/
│   ├── libc.rs           # dlopen/dlsym/dlerror (no dlclose), poll, pipe2 (O_CLOEXEC, O_NONBLOCK);
│   │                     # later write (M2.2, M7.1), inotify_* (M4.5), fork/execve/setsid/
│   │                     # waitpid, kill (M5.7),
│   │                     # posix_openpt/grantpt/unlockpt/ptsname_r, ioctl TIOCSCTTY (M5.9),
│   │                     # termios, ioctl TIOCGWINSZ, sigaction (M7.1)
│   └── x11.rs            # Display, Window, XEvent (192-byte union), XImage, error handlers,
│                         # font cursors, atoms… and the Xlib table (dynlib!)
├── macos/
│   ├── objc.rs           # objc_* runtime, Class/Sel/Id/BOOL, msg_send! macro
│   ├── foundation.rs appkit.rs coregraphics.rs quartzcore.rs coreservices.rs
└── windows/
    ├── types.rs          # HWND, MSG, RECT, BITMAPINFO, HANDLE…
    ├── kernel32.rs user32.rs gdi32.rs shell32.rs ole32.rs
    └── conpty.rs         # CreatePseudoConsole, ResizePseudoConsole, ClosePseudoConsole
```

### Rules

- Declare **only what is called**, each citing a header or doc URL.
- Declare nothing `std` already gives: `errno` is read with
  `io::Error::last_os_error`, each descriptor Tidepool owns is wrapped
  in `OwnedFd` (one a library owns, such as Xlib's connection, is only
  borrowed, as a `BorrowedFd`, and never closed), and `read` and `write`
  go through `File`. The exception is code that
  must be async-signal-safe, which `std` does not promise for
  `<&File as Write>::write`: for a signal handler and `Waker::wake`,
  `sys` declares `write(2)` in the task that calls it (M2.2 for the
  waker, M7.1 for the handler). A self-pipe comes from `pipe2` with
  `O_CLOEXEC | O_NONBLOCK`: `std::io::pipe` cannot make its ends
  non-blocking without `fcntl`, which is variadic, and a socket pair
  would be a socket of Tidepool's own, against `AGENTS.md` rule 8.
- Every struct gets `const _: () = assert!(size_of::<T>() == N && align_of::<T>() == A);`
  per target, and `offset_of!` for its fields. An opaque type, which only
  the OS looks inside or which is only passed by pointer (`Display`, `_XGC`),
  has no fields and nothing to assert. The Linux numbers are measured on
  x86_64 and hold on aarch64; `linux.rs` fails the build on any other
  Linux target.
- `extern "system"` for Win32, `extern "C"` elsewhere; edition 2024
  `unsafe extern` blocks.
- `msg_send!` transmutes to the exact signature; never call `objc_msgSend`
  through a varargs prototype.
- Prefer `dlopen`/`LoadLibraryW` for anything not guaranteed present
  (libX11), so Tidepool reports a clear error. A loaded library is never
  closed (`dlclose` is not declared), so a table's function pointers never
  dangle.

## `tidepool-platform`: safe behavior

| Module | Provides |
| :- | :- |
| `window` | `Window::open(config)`, title, size, scale factor, cursor shape, focus, close request |
| `events` | `wait_events(deadline, &mut Vec<Event>)`; `Waker` (Send + Clone) that interrupts the wait from any thread |
| `surface` | `present(&Framebuffer, &[DamageRect])`: X11 `XPutImage`, macOS layer contents, Win32 `SetDIBitsToDevice` |
| `clipboard` | Get/set UTF-8 text; X11 `CLIPBOARD` and `PRIMARY` |
| `terminal` | For the TUI: raw mode guard (restores on drop, panic and signals), size, resize events, stdin reader |
| `pty` | `Pty::spawn(argv, env, cwd, size)` → master reader/writer, resize, child handle |
| `process` | Low-level spawn helpers `toolchain` builds on: pipes, process groups, kill-tree |
| `watch` | Directory watching (inotify / FSEvents / ReadDirectoryChangesW) |
| `paths` | Config, data, cache and log directories per OS |
| `dirs` | Standard tool locations (Android SDK default, rustup home) |

### Invariants

- All `Window` methods are main-thread only and the type is `!Send`.
- `Waker::wake` is async-signal-safe on Unix (one byte to a self-pipe).
- Dropping a `Pty` or a terminal guard restores state and reaps children.
- No `target_os` cfg appears in the public API; behavior differences are
  documented here and in `docs/03-platforms.md`.

## Tests

- Layout asserts compile on each target.
- Linux CI runs window tests under Xvfb: open, resize, damage present, clipboard round-trip
  between two windows, `Waker` from another thread.
- PTY tests spawn `/bin/sh -c 'printf …'` through the `Pty` API (argv, not
  a shell string from user input) and check output and exit status.
- Terminal guard test: a child process panics in raw mode; the terminal
  state is restored (checked with `stty -g` before and after).
