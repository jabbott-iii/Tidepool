# 03 — Platforms

Per-OS details for `tidepool-sys` and `tidepool-platform`, and for the
managed tools that differ by OS. The API every other crate sees is in
`docs/subsystems/sys-and-platform.md`.

## Targets

| OS | Versions | Architectures | Display |
| :- | :- | :- | :- |
| Linux | glibc 2.31+ distributions | x86_64 (aarch64 in backlog) | X11; Wayland sessions through XWayland (ADR-0010) |
| macOS | 13+ | Apple Silicon and Intel (universal binary) | AppKit |
| Windows | 10 (1809+) and 11 | x86_64 | Win32 |

The TUI runs anywhere the GUI runs, in any terminal that supports
ECMA-48 / xterm sequences, including over SSH.

## Development order

**Linux → macOS → Windows** (ADR-0015).

- Linux first: it is where Claude Code sessions and CI run, and the
  owner's machine.
- macOS second, because iOS work needs it (M11).
- Windows third (M12).

Every milestone before M11 is Linux-only unless a task says otherwise. A
task that needs another OS leaves its box unchecked with a
`⏳ needs <OS>: <check>` note (see `docs/roadmap.md`).

## Linux

### Window and event loop (X11 via `dlopen("libX11.so.6")`)

- One `Display` connection, opened on the main thread. Tidepool does not
  call `XInitThreads`; only main calls Xlib, any Xlib function, so that
  use is sound whether or not Xlib locks. From 1.8 on, libX11 as
  normally built calls `XInitThreads` itself when the library is loaded;
  a build without that option, like any earlier version, does no
  locking.
- `wait_events(deadline)` uses `poll` on two fds: the X connection
  (`ConnectionNumber`) and a self-pipe the `Waker` writes to. It drains
  `XPending` before and after polling.
- Window: `XCreateWindow` with a 24-bit TrueColor visual, `WM_DELETE_WINDOW`,
  `_NET_WM_NAME` (UTF-8), `_NET_WM_PID`, `WM_CLASS = "tidepool"`, and
  `_NET_WM_ICON` from Tidepool's icon.
- Client-side title bar is **not** drawn in v1; the window manager decorates.
- Present: `XPutImage` of damaged rectangles from the framebuffer. MIT-SHM
  (`XShmPutImage`) is a later optimization behind a measured need.
- HiDPI: scale from `Xft.dpi` in the resource database (default 96), with a
  setting to override.

### Input

- Keys: `XLookupKeysym` → Tidepool `Key`; text via `Xutf8LookupString`.
- IME: XIM is backlog; dead keys work through `Xutf8LookupString`.
- Mouse: buttons 1–3, wheel as buttons 4–7, `MotionNotify` with
  compression of consecutive motion events.
- Cursors: core font cursors in v1; `libXcursor` themes later.

### Clipboard

- `CLIPBOARD` and `PRIMARY` selections with `UTF8_STRING` and `TARGETS`.
  `INCR` transfers for large pastes. The owner keeps serving after copy
  until another client takes the selection.

### Terminal (integrated) and TUI

- PTY: `posix_openpt`, `grantpt`, `unlockpt`, `ptsname_r`, then
  `fork`/`execve` with `setsid` and `TIOCSCTTY` in the child.
- TUI: `tcgetattr`/`tcsetattr` raw mode, `TIOCGWINSZ`, `SIGWINCH` via a
  self-pipe; the terminal state is restored on exit, panic, `SIGINT`,
  `SIGTERM` and `SIGHUP`.

### File watching

- inotify with `IN_CLOEXEC | IN_NONBLOCK`, one watch per directory, raised
  `max_user_watches` errors reported with a fix-it message; falls back to
  polling for the affected subtree.

### Paths

| Kind | Path |
| :- | :- |
| Config | `$XDG_CONFIG_HOME/tidepool` (default `~/.config/tidepool`) |
| Data (logs, session, trust, toolchain state) | `$XDG_DATA_HOME/tidepool` (default `~/.local/share/tidepool`) |
| Cache | `$XDG_CACHE_HOME/tidepool` |
| Android SDK (managed) | `$ANDROID_HOME` if set, else `~/Android/Sdk` (Android Studio's default) |

### Managed tools on Linux

- rustup: the official `rustup-init` for the host triple, downloaded with
  `curl` and checked against its published SHA-256, run without `sudo`.
- git: the distro package manager (`apt`, `dnf`, `pacman`, `zypper`).
  It needs admin, so Tidepool shows the exact command and runs it in the
  integrated terminal only when the user presses Run.

## macOS

### Objective-C runtime glue

- `msg_send!` transmutes `objc_msgSend` to the exact signature at every
  call, never through a varargs prototype (arm64 ABI). Struct returns over
  16 bytes use `objc_msgSend_stret` on x86_64. Same pattern as Nadir.
- AppKit only on the main thread. An autorelease pool wraps every loop turn.

### Window and event loop

- `NSApplication` with a custom `NSView` subclass built at runtime
  (`objc_allocateClassPair`), layer-backed. Present sets the layer's
  `contents` to a `CGImage` made from the framebuffer's damaged region.
- `wait_events(deadline)` uses `nextEventMatchingMask:untilDate:` with the
  deadline; the `Waker` posts an application-defined `NSEvent`.
- Live resize keeps drawing from `viewDidEndLiveResize` / `setNeedsDisplay`.
- Retina: `backingScaleFactor`.

### Input, clipboard, IME

- `keyDown:` with `interpretKeyEvents:` and a minimal `NSTextInputClient`
  for dead keys; full IME is backlog.
- `NSPasteboard` general pasteboard, `public.utf8-plain-text`.

### Terminal, file watching, paths

- PTY as on Linux (`posix_openpt`). FSEvents for watching.
- Config and data: `~/Library/Application Support/Tidepool`; logs:
  `~/Library/Logs/Tidepool`; caches: `~/Library/Caches/Tidepool`.

### Managed tools on macOS

- Xcode cannot be installed silently. Tidepool detects it
  (`xcode-select -p`, `xcodebuild -version`), opens the App Store page for
  the user, offers `xcode-select --install` for the Command Line Tools
  (which also provides git), and shows the license status. Accepting the
  Xcode license is the user's action.
- Signing identities are read with `security find-identity -v -p
  codesigning`; private keys stay in the Keychain.

## Windows

### Window and event loop

- `RegisterClassExW` / `CreateWindowExW`, per-monitor DPI awareness v2
  (manifest), `WM_DPICHANGED`.
- `wait_events(deadline)` uses `MsgWaitForMultipleObjectsEx` on an event
  the `Waker` sets. Modal move/size loops keep rendering through a timer.
- Present: `StretchDIBits` (or `SetDIBitsToDevice`) of damaged rectangles.

### Input, clipboard, terminal, watching

- `WM_KEYDOWN`/`WM_CHAR` with surrogate-pair handling; IMM32 is backlog.
- Clipboard: `CF_UNICODETEXT`.
- Terminal: ConPTY (`CreatePseudoConsole`) on Windows 10 1809+.
- File watching: `ReadDirectoryChangesW` with overlapped I/O.
- Paths: `%APPDATA%\Tidepool` (config), `%LOCALAPPDATA%\Tidepool` (data,
  logs, cache).

### Managed tools on Windows

- git and a JDK through `winget` when present (the user confirms), else
  the official installer's download page.
- rustup: `rustup-init.exe`, SHA-256 checked.

## Cross-platform invariants (tested on every OS)

- The same keystroke sequence produces the same buffer on every OS.
- Line endings and encodings round-trip byte-for-byte on save.
- Paths are UTF-8 internally; a path that is not UTF-8 is displayed with
  each byte that is not UTF-8 shown as `\xNN`, and opened by its original
  bytes (ADR-0032).
- A file's canonical path (`core::path::canonicalize`) is `realpath(3)`'s
  on Linux and macOS (on macOS each name that exists as the file system
  stores it, and `/tmp` is `/private/tmp`), and `C:\…` or
  `\\server\share\…` on Windows, keeping `\\?\` only where Win32 would
  read the plain form differently, so forms can alternate down a tree
  (ADR-0033).
- Child processes never outlive Tidepool; shutdown kills stragglers.
- Every managed install asks first, shows its source, and verifies what it
  downloaded.
