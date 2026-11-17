# tidepool-platform

Safe OS behavior on top of `tidepool-sys`: windows, the event loop and its
waker, presenting a framebuffer, the clipboard, terminal raw mode, PTYs,
processes, file watching and the per-OS paths. No `target_os` cfg shows in
its public API.

It is part of [Tidepool](https://github.com/jabbott-iii/Tidepool), where it
is the only crate that uses `tidepool-sys`: `toolchain`, `term`,
`workspace`, both frontends and the binary reach the OS through it, and
none does yet.

Layer 1, OS behavior, formats and pure engines. It may use `core` and `sys`,
as its row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says, and uses
both.

## Status

Version 0.1, not published (`publish = false`): the API changes as the
roadmap needs.

Built, with the roadmap task that built it:

- the crate root, which allows `unsafe_code` — M0.1.
- `window` (`Window::open` from a `WindowConfig`, the title, the size in
  device pixels and logical units, the scale, the pointer's
  `CursorShape`, the focus), `events` (`Window::wait_events` with a
  deadline, the `Event`s it reports, and the `Waker`, which ends a wait
  from any thread and is async-signal-safe), `surface`
  (`Window::present` of a `Pixels` view, BGRA8 with its stride, clipped
  to the damage, the frame and the window, ADR-0040) and the crate's
  `Error` — M2.2. On Linux they are an X11 window through `libX11.so.6`,
  loaded once with `dlopen`, over a local Unix socket only (ADR-0039):
  keys from key codes, which give every `Key`; text through Xlib's own
  input method; the scale from `Xft.dpi`; one thread of the process for
  every window; and X errors logged, not fatal. On every other system
  `Window::open` fails with `Error::Unsupported`.

Not yet, with the roadmap task that adds it:

- `clipboard`, `paths` and `dirs` on Linux — M2.7.
- `watch` (inotify) — M4.5.
- `process` on Linux — M5.7.
- `pty` on Linux — M5.9.
- `terminal`, the raw-mode guard for the TUI — M7.1.
- macOS — M11.7, M11.8, M11.1.
- Windows — M12.1, M12.2.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Usage

A workspace crate whose row of the matrix allows it depends on it by path:

```toml
[dependencies]
tidepool-platform = { path = "../platform" }
```

Open a window, wake its wait from another thread, and present a frame
where the system asks for it (this needs a display, so the example is only
compiled):

```rust,no_run
use std::time::{Duration, Instant};

use tidepool_platform::Error;
use tidepool_platform::events::Event;
use tidepool_platform::surface::Pixels;
use tidepool_platform::window::{Window, WindowConfig};

fn main() -> Result<(), Error> {
    let mut window = Window::open(&WindowConfig::default())?;
    let waker = window.waker();
    std::thread::spawn(move || waker.wake());

    let size = window.size();
    let stride = 4 * usize::try_from(size.width).unwrap_or(0);
    let rows = usize::try_from(size.height).unwrap_or(0);
    // A frame of BGRA8 pixels, all a dark blue.
    let frame = [0x30_u8, 0x18, 0x10, 0xff].repeat(stride / 4 * rows);
    let pixels = Pixels::new(&frame, size, stride).expect("the frame covers the window");

    let mut events = Vec::new();
    loop {
        events.clear();
        window.wait_events(Some(Instant::now() + Duration::from_secs(1)), &mut events)?;
        for event in &events {
            match event {
                Event::Exposed(damage) => window.present(&pixels, &[*damage])?,
                Event::CloseRequested => return Ok(()),
                _ => {}
            }
        }
    }
}
```

A frame's view checks its shape, so that `present` can never read past
it:

```rust
use tidepool_core::geom::DeviceSize;
use tidepool_platform::surface::{Pixels, PixelsError};

let bytes = [0_u8; 32];
let short = Pixels::new(&bytes, DeviceSize::new(3, 3), 12);
assert_eq!(short, Err(PixelsError::TooFewBytes { len: 32, needed: 36 }));
```

## Documentation

- Design:
  [`docs/subsystems/sys-and-platform.md`](../../docs/subsystems/sys-and-platform.md)
- Platforms: [`docs/03-platforms.md`](../../docs/03-platforms.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0010 (X11
  through `dlopen`); ADR-0012 (the main loop, Proposed); ADR-0039 (M2.2's
  window: the read-back test, the X connection over a Unix socket only,
  keys from key codes, the `TIDEPOOL_IT_X11` gate); ADR-0040 (`present`
  takes a pixel view of its own).
- API reference: `cargo doc -p tidepool-platform --open`

## Rules

- `unsafe` is allowed here: this is one of the two crates that may hold it
  (`AGENTS.md` rule 4). Each block carries a `// SAFETY:` comment, and each
  `pub unsafe fn` a `# Safety` section.
- Path-scoped rules in `.claude/rules/`:
  [`unsafe-and-ffi.md`](../../.claude/rules/unsafe-and-ffi.md),
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.
- The connection to the X server is the one socket the core binary opens
  (`AGENTS.md` rule 8, ADR-0039), and only over a local Unix socket.
- Every change here goes to the `security-reviewer` subagent.

## Checks

```bash
cargo test -p tidepool-platform
TIDEPOOL_IT_X11=1 xvfb-run -a cargo test -p tidepool-platform
cargo clippy -p tidepool-platform --all-targets --target x86_64-apple-darwin -- -D warnings
cargo clippy -p tidepool-platform --all-targets --target x86_64-pc-windows-msvc -- -D warnings
cargo xtask unsafe-audit
```

The tests that open windows (present and read back with `XGetImage`, the
`Waker` from another thread, the deadline, the one window thread, refused
requests) run only with `TIDEPOOL_IT_X11=1`, and print `SKIPPED:`
otherwise; with it set, a test that cannot reach an X server fails
(ADR-0039). Run them under `xvfb-run`, not on a desktop, where a
compositor can change what reads back. The rest, such as the display's
rules, the key map and the translation of events, always run. The two
cross-target lines check that the crate builds where it has no window yet
(after `rustup target add`). `cargo xtask ci` runs the whole gate, without
the window tests unless it runs as Linux CI runs it,
`TIDEPOOL_IT_X11=1 xvfb-run -a cargo xtask ci` (ADR-0041).

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
