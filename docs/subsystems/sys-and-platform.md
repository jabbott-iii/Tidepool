# Subsystem: `tidepool-sys` and `tidepool-platform`

Read `docs/03-platforms.md` for per-OS details. This file defines the crate
structure and the API every other crate sees. The approach follows Nadir's
`nadir-sys`/`nadir-platform`, which already has working X11 and Objective-C
patterns to port.

## `tidepool-sys`: declarations only

- **Contains only:** `#[repr(C)]` types, constants, `unsafe extern` blocks,
  function-pointer type aliases, `dlopen` symbol tables, and layout tests.
- **No behavior:** no wrappers, loops or allocation. The only functions are
  the generated symbol-table loaders.

```
crates/sys/src/
├── lib.rs                # #![allow(unsafe_code)], cfg-gated modules
├── dynlib.rs             # dynlib! macro: struct of fn pointers + load(&dyn Library)
├── linux/
│   ├── libc.rs           # poll, pipe2, read/write, ioctl (TIOCGWINSZ, TIOCSCTTY), termios,
│   │                     # posix_openpt/grantpt/unlockpt/ptsname_r, fork/execve/setsid/waitpid,
│   │                     # kill, sigaction, inotify_*, dlopen/dlsym/dlerror, errno
│   └── x11.rs            # Display, Window, XEvent (192-byte union), XImage, atoms… (dynlib!)
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
- Every struct gets `const _: () = assert!(size_of::<T>() == N && align_of::<T>() == A);`
  per target.
- `extern "system"` for Win32, `extern "C"` elsewhere; edition 2024
  `unsafe extern` blocks.
- `msg_send!` transmutes to the exact signature; never call `objc_msgSend`
  through a varargs prototype.
- Prefer `dlopen`/`LoadLibraryW` for anything not guaranteed present
  (libX11), so Tidepool reports a clear error.

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
