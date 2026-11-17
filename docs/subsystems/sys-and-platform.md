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
│                         # C's `...` included; MissingSymbols naming every symbol a library lacks
├── linux.rs              # the Linux modules; fails the build on a Linux target but x86_64 or aarch64
├── linux/
│   ├── libc.rs           # dlopen/dlsym/dlerror (no dlclose), poll, pipe2 (O_CLOEXEC, O_NONBLOCK),
│   │                     # write (async-signal-safe), setlocale (LC_CTYPE);
│   │                     # later inotify_* (M4.5), fork/execve/setsid/waitpid, kill (M5.7),
│   │                     # posix_openpt/grantpt/unlockpt/ptsname_r, ioctl TIOCSCTTY (M5.9),
│   │                     # termios, ioctl TIOCGWINSZ, sigaction (M7.1)
│   ├── evdev.rs          # the kernel's KEY_* codes that map to a Key; X reports them + 8 (ADR-0039)
│   └── x11.rs            # Display, Window, XEvent (192-byte union), XImage, the error handler,
│                         # font cursors, atoms, the input method and context for typed text…
│                         # and the Xlib table (dynlib!, 41 functions)
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
  `<&File as Write>::write`: for `Waker::wake` and a signal handler,
  `sys` declares `write(2)`, which the waker writes with (M2.2) and the
  terminal's handler is to (M7.1). A self-pipe comes from `pipe2` with
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
| `window` | `Window::open(&WindowConfig)`, `set_title`, `size` (device pixels) and `logical_size`, `scale`, `set_cursor(CursorShape)`, `is_focused`; the close request is an event |
| `events` | `Window::wait_events(deadline, &mut Vec<Event>)`: `Event::Input` (`core::input::InputEvent`, logical positions), `Resized`, `Exposed`, `CloseRequested`, `Woken`; `Waker` (`Send + Sync + Clone`) that interrupts the wait from any thread |
| `surface` | `present(&Pixels, &[DeviceRect])`: a borrowed BGRA8 view with its size and stride, which `gfx`'s framebuffer hands out, and the damage in device pixels (ADR-0040). X11 `XPutImage`, macOS layer contents, Win32 `SetDIBitsToDevice` |
| `clipboard` | Get/set UTF-8 text; X11 `CLIPBOARD` and `PRIMARY` |
| `terminal` | For the TUI: raw mode guard (restores on drop, panic and signals), size, resize events, stdin reader |
| `pty` | `Pty::spawn(argv, env, cwd, size)` → master reader/writer, resize, child handle |
| `process` | Low-level spawn helpers `toolchain` builds on: pipes, process groups, kill-tree |
| `watch` | Directory watching (inotify / FSEvents / ReadDirectoryChangesW) |
| `paths` | Config, data, cache and log directories per OS |
| `dirs` | Standard tool locations (Android SDK default, rustup home) |

### Invariants

- A `Window` is `!Send` and `!Sync`, and every window of the process
  lives on one thread: the first to open one, until all are closed.
  `Window::open` on another thread meanwhile fails with
  `Error::WrongThread`. On Linux this keeps Xlib's calls from
  overlapping.
- `Waker::wake` is async-signal-safe on Unix (one byte to a self-pipe);
  a signal handler that calls it saves and restores `errno`, which a full
  pipe sets.
- `present` cannot read outside its `Pixels` view: `Pixels::new` checks
  that the stride covers a row and the bytes cover every row.
- The crate's error is `platform::Error`, with every variant on every
  system; on a system with no window yet, `Window::open` fails with
  `Error::Unsupported`.
- Dropping a `Pty` or a terminal guard restores state and reaps children.
- No `target_os` cfg appears in the public API; behavior differences are
  documented here and in `docs/03-platforms.md`.

## Tests

- Layout asserts compile on each target.
- Window tests need an X server: M2.2's open a window, present damage
  and read it back with `XGetImage`, wake a wait from another thread
  within 5 ms, end a wait at its deadline, refuse a window on a second
  thread, and record a request the server refused; M2.7 adds the
  clipboard round-trip between two windows. A resize is tested through
  the translation of `ConfigureNotify`, without a server, as are the
  display's rules, the key map, the modifiers and the rest of the
  events. The window tests are `#[cfg(test)]` tests inside
  `platform`'s library, since only its library target may call Xlib,
  and they run only with `TIDEPOOL_IT_X11=1`, printing `SKIPPED:`
  otherwise: `TIDEPOOL_IT_X11=1 xvfb-run cargo test -p
  tidepool-platform` (ADR-0039). `cargo xtask ci` and the Linux CI job
  do not set the variable yet, so they skip them.
- PTY tests spawn `/bin/sh -c 'printf …'` through the `Pty` API (argv, not
  a shell string from user input) and check output and exit status.
- Terminal guard test: a child process panics in raw mode; the terminal
  state is restored (checked with `stty -g` before and after).
