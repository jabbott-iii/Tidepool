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

- `libX11.so.6` is loaded once, with `dlopen`, the first time a window
  opens, and never unloaded; on a machine without it, opening a window
  fails with an error that names it (the TUI, M7.1, is to need no X).
- One `Display` connection for each window, over a local Unix socket
  only, the one exception to `AGENTS.md` rule 8 (ADR-0039).
  `platform` reads `DISPLAY` and passes `:N`, `:N.S`, `unix:N` and
  `unix:N.S` to `XOpenDisplay` as `unix/:N` and `unix/:N.S`, the `unix`
  protocol with no host, so that libxcb never falls back to TCP; libxcb
  1.16 and later read a name that starts with `unix:` as the path of a
  socket file (ADR-0050). It refuses any
  other value, such as a host name (`ssh -X`'s `localhost:10.0`), a
  transport (`tcp/…`), a socket path, a number that is not decimal or an
  unset `DISPLAY`, with an error that says why and names the value,
  escaped, before it opens any socket. It never passes `XOpenDisplay`
  null. Xlib reads the Xauthority file itself; Tidepool never does.
- Every window of the process lives on one thread: the first window to
  open makes its thread the Xlib thread, for as long as any window is
  open, and `Window::open` on another thread meanwhile fails. Once all are
  closed, any thread may open the next. `Window` is `!Send` and `!Sync`.
- Tidepool does not call `XInitThreads`; only that thread calls Xlib, any
  Xlib function, so that
  use is sound whether or not Xlib locks. From 1.8 on, libX11 as
  normally built calls `XInitThreads` itself when the library is loaded;
  a build without that option, like any earlier version, does no
  locking.
- X errors: `platform` replaces Xlib's default handler, which prints the
  error and ends the process, with one that counts the error and keeps
  the last one's codes, without allocating or calling Xlib. A window that
  is opening fails if the server refused one of its requests (it waits
  for the server with `XSync` before it returns); later errors are logged
  by `wait_events`. The handler of a lost connection stays Xlib's own,
  which ends the process: a lost X server is not recovered from (crash
  reports are M4.9's).
- `wait_events(deadline)` uses `poll` on two fds: the X connection
  (`ConnectionNumber`, borrowed, never closed) and a self-pipe the
  `Waker` writes to. It flushes Xlib's queued requests and drains
  `XPending` before and after polling, returns as soon as it has events
  or was woken, and otherwise when the deadline passes; a signal
  (`EINTR`) does not end the wait. The self-pipe comes from `pipe2` with
  `O_CLOEXEC | O_NONBLOCK`; `Waker::wake` is one `write(2)` of a byte, and
  a full pipe means a wake is already pending. Both ends live as long as
  the window or any waker, so a wake never meets `EPIPE`.
- Window: `XCreateWindow` with a 24-bit TrueColor visual from
  `XMatchVisualInfo`, whose red, green and blue masks must be BGRA8's
  (`0xff0000`, `0xff00`, `0xff`), and a black background;
  `WM_DELETE_WINDOW`; the title in `_NET_WM_NAME` (UTF-8) and `WM_NAME`
  (`STRING` when it is ASCII, else `UTF8_STRING`); `_NET_WM_PID` with
  `WM_CLIENT_MACHINE`, from `/proc/sys/kernel/hostname` (neither if it
  cannot be read); and `WM_CLASS` with the instance `tidepool` and the
  class `Tidepool`. There is no `_NET_WM_ICON`: Tidepool has no icon yet
  (`assets/icons` does not exist), so the window manager shows its own.
  The window is at least 1 by 1 and at most 32,767 by 32,767 device
  pixels.
- Client-side title bar is **not** drawn in v1; the window manager decorates.
- Present: `XPutImage` of each damaged rectangle of the caller's `Pixels`
  view (ADR-0040), clipped to the view and the window, through an
  `XImage` that `XInitImage` sets up over the caller's bytes (`ZPixmap`,
  depth 24, 32 bits a pixel, `LSBFirst`, `bytes_per_line` the stride).
  The image is whole rows, from the start of the rectangle's first row
  and as wide as the rectangle's right edge, so that whatever Xlib reads
  for any server's scanline pad is in the view; the rectangle is split
  into bands of rows so that Xlib's `int` offsets cannot overflow. Xlib
  only reads the bytes, and the image is never given to `XDestroyImage`,
  which would free them. The window is opaque: the alpha byte is not shown. MIT-SHM
  (`XShmPutImage`) is a later optimization behind a measured need.
- HiDPI: the scale is `Xft.dpi` in the resource database over 96, read
  once when the window opens (1 when it is not set, or not a density
  from 24 to 1536). A change while Tidepool runs is not seen. A setting
  to override it has no roadmap task yet.

### Input

- Keys: the event's key code minus 8 is the kernel's evdev code, which a
  table maps to Tidepool's `Key`, a physical key on any layout
  (ADR-0039). That holds with the `evdev` and `libinput` drivers, under
  XWayland and in Xvfb. Every `Key` can be reported; a code with no `Key`
  (Japanese and Korean keys, media keys) is dropped. Modifiers come from
  the event's `state`: Shift, Control, `Mod1` as Alt and `Mod4` as Super;
  a modifier key's own press includes it, and its release leaves it out
  unless the same modifier's other key is still down. A held key repeats
  as presses with `repeat` set, through `XkbSetDetectableAutoRepeat`
  (with a warning in the log on a server without it). When the window
  loses the focus, every key still down is reported going up; focus
  events of a keyboard grab are not focus changes.
- Text via `Xutf8LookupString`, through an input context, after
  `XFilterEvent`: none while Control, Alt or Super is held (a shortcut),
  and no control characters (Enter, Tab, Backspace and the like are
  keys). The first window takes the C library's `LC_CTYPE` from the
  environment if it is still "C" and Xlib supports the locale, so that
  text is not Latin-1 only.
- IME: XIM is backlog; dead keys work through `Xutf8LookupString`, with
  Xlib's own input method (`@im=none`), which connects to no server
  (ADR-0039). Nothing of a sequence being composed is shown. Opening it,
  in `Window::open`, reads the locale's Compose file and `~/.XCompose`
  (or `$XCOMPOSEFILE`).
- Mouse: buttons 1–3, wheel as buttons 4–7 (one line a step; positive
  `y` is down), buttons 8 and 9 as Back and Forward, `MotionNotify` with
  compression of consecutive motion events. Positions are logical: device
  pixels over the scale.
- Cursors: the core font's (the arrow, the text beam, the hand, and the
  column and row resize arrows), made with `XCreateFontCursor`, all five
  in `Window::open`; `set_cursor` only shows one (`XDefineCursor`), so it
  reads no file. Where `libXcursor.so.1` is installed,
  libX11 itself loads it with `dlopen` on the first `XCreateFontCursor`,
  with `libXrender.so.1` and `libXfixes.so.3`, and the shapes come from
  the user's cursor theme. Tidepool neither loads nor configures it.
  libXcursor reads its settings with `XGetDefault`, which runs Xlib's
  resource parser over the display's `RESOURCE_MANAGER` property (or
  `~/.Xdefaults` when it is not set), merged with `$XENVIRONMENT` or
  else `~/.Xdefaults-<host>`, and that parser follows `#include "path"`
  lines. It takes the theme from
  `XCURSOR_THEME` or the `Xcursor.theme` resource, and joins it onto its
  icon directories (`$XCURSOR_PATH`, or by default `~/.icons`,
  `/usr/share/icons` and `/usr/share/pixmaps`) without removing `..`; for
  each shape it opens `<theme>/index.theme`, following `Inherits=`, and
  `<theme>/cursors/<name>`, and parses what it finds with its cursor-image
  parser. So any client of the display can make `Window::open` open and
  read a file it names, and open and parse cursor files at a path of its
  choosing; if one of them is a FIFO, `Window::open` blocks until
  something writes to it. Each `Window::open` does this again for its
  own connection, so opening a window while another runs can block the
  window thread, and the open windows with it.

### Clipboard

The `CLIPBOARD` and `PRIMARY` selections, per ICCCM 2.0 but for the two
departures ADR-0042 records. Each window has its own. Nothing waits for
another client's answer (`AGENTS.md` rule 6); the round trips to the
server, in `set_selection` and when `wait_events` reads a paste, wait
while another client holds a server grab, as every Xlib call does.
`wait_events` takes events in turns of at most 10 ms, between which it
sees a wake and its deadline, so a flood of events another client causes
cannot hold it for longer.

- **Copy:** `set_selection` keeps a copy of the text, writes nothing to
  the window's `TIDEPOOL_TIMESTAMP` property (a replace, which no type
  another client gave the property can refuse) and waits for the server
  (`XSync`); the `PropertyNotify` of that write, told from another
  client's earlier one by its request number (`XNextRequest`), carries
  the time it claims the selection at with `XSetSelectionOwner`. It takes
  events in turns until that one is handled, however many were queued
  before it, so it holds the selection when it returns.
  `XGetSelectionOwner` says whether the claim took: one by another client
  at a later time wins, and the log says so. Other events read meanwhile
  are reported by the next `wait_events`. A `SelectionClear` with the
  very time of the window's claim is checked with `XGetSelectionOwner`
  before the window gives the selection up.
- **Serving:** inside `wait_events` the owner answers `UTF8_STRING` with
  the text, `TARGETS` with `TARGETS`, `TIMESTAMP` and `UTF8_STRING`, and
  `TIMESTAMP` with its claim's time, and refuses every other target
  (`MULTIPLE`, `STRING`, `TEXT`, `COMPOUND_TEXT` included), any request
  timed before its claim, and any that would have it write into one of
  its own properties but its answer properties. A request into a
  property, answered or not, ends a transfer into that property. A
  request with no property, from a client older than ICCCM 2.0, is
  answered in the property its target names. The owner begins at most
  64 MiB of text a second, and refuses requests beyond that but the
  first of each second; a request refused for the cap on transfers
  spends none of it. A text
  longer than a part, at most 256 KiB or the server's request limit less
  24 bytes, goes by `INCR`: the owner watches the requestor's window
  (`PropertyChangeMask`) and writes the next part each time the property
  is deleted, then an empty one. At most 16 transfers run at once; one
  whose part is not taken within 5 s ends. The owner gives the text up on
  `SelectionClear`, and when the window closes; no clipboard manager
  protocol (`SAVE_TARGETS`) keeps it after that.
- **Paste:** `request_selection` asks for `UTF8_STRING` at `CurrentTime`
  into the next of the window's four properties for the selection
  (`TIDEPOOL_CLIPBOARD_0` to `_3`, `TIDEPOOL_PRIMARY_0` to `_3`; one whose
  paste ended without its answer rests for a minute), and takes only the
  answer named there; a later `wait_events` reports `Event::Pasted`: the text, whole or by
  `INCR`, with U+FFFD for bytes that are not UTF-8, at most 64 MiB once
  they are replaced; or `None` if the selection has no owner, the owner
  refused, the answer was of another type or too large, or 5 s passed
  with no answer, or between two parts, or a minute in all by `INCR`. A
  second request while one is pending joins it. However often another
  client writes a part, the property is read once a turn. A late refusal
  of an earlier request can end the next paste with `None`.
- **Trust:** any client of the display can set the window's properties and
  send it events, so a paste is bounded in size and time, not trusted, and
  an event with `send_event` set never makes the window claim, give up or
  answer for a selection. Clipboard text is never logged, and `Event`'s
  `Debug` shows its length only (ADR-0043).

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
| Data (session, trust, toolchain state) | `$XDG_DATA_HOME/tidepool` (default `~/.local/share/tidepool`) |
| Cache | `$XDG_CACHE_HOME/tidepool` (default `~/.cache/tidepool`) |
| Logs | `$XDG_DATA_HOME/tidepool/logs` (ADR-0042) |
| Android SDK (managed) | `$ANDROID_HOME` if set, else `~/Android/Sdk` (Android Studio's default) |
| rustup | `$RUSTUP_HOME` if set, else `~/.rustup` |

`platform::paths::user_dirs` gives the first four, and `platform::dirs`
the last two (M2.7). A variable that is unset, empty or not an absolute
path counts as unset (XDG Base Directory 0.8); the home directory is
`HOME`, or the user database's when that is unset or empty. Nothing is
made: a caller makes a folder before writing into it.

### File metadata for saves

A save renames a new file over the old one (ADR-0025), so before it
`workspace::buffers` (M4.11) asks `platform::file` (M4.10, ADR-0068) two
things:

- the file's link count, owner, group and permission bits, from
  `stat(2)` through `std`, following links;
- the owner and group of a file Tidepool would make in its folder: the
  effective user (`geteuid`), and the folder's group if the folder has
  the setgid bit, else the effective group (`getegid`), as open(2) says.
  A file system mounted with `bsdgroups` (`grpid`, ext4(5)) gives every
  new file its folder's group; one without Unix owners (FAT, a share
  mapped to one user) fixes the owner by its mount options; NFS's root
  squashing, an export's default, gives root's new files the anonymous
  user and group, 65534 by default (exports(5)); and in a user namespace
  (a rootless container, a Flatpak sandbox) unmapped ids read as the
  overflow id, 65534 by default (user_namespaces(7)). None is seen, so
  an error or an overflow id means not knowing, and the save asks
  (ADR-0068).

The folder asked about is the one holding the file the save reaches after
its links, not the folder of a link to it.

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
