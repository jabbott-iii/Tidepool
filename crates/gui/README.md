# tidepool-gui

The GUI frontend: it opens a window, keeps a framebuffer at device
resolution, and rasterizes the damaged parts of each `ui` draw list with
`gfx` and `font`, in the full jabbott.dev look. It holds no feature logic.

It is part of [Tidepool](https://github.com/jabbott-iii/Tidepool), where
the `tidepool` binary connects it to `editor`, which it never depends on; it
opens its window through `tidepool-platform`.

Layer 4, views and frontends. It may use `core`, `platform`, `codec`, `gfx`,
`font` and `ui`, as its row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Version 0.1, not published (`publish = false`): the API changes as the
roadmap needs.

Built, with the roadmap task that built it:

- the first frame: `Gui` (`open`, `size` and `turn`, with its `Turn`),
  which opens a window through `tidepool-platform` and presents what each
  expose asks for, drawing anew on the calling thread when the window's
  size or scale changed; `first_frame::FirstFrame`, which draws
  jabbott.dev's backdrop and a line in each of the seven bundled faces,
  its name in `text-3` and a sample in `text`; and `GuiError` — M2.6
  (ADR-0045).
- the first frame's two colors from `tidepool-ui`'s bundled
  `tidepool-dark` theme, in place of M2.6's stand-ins — M3.2 (ADR-0047).

Not yet, with the roadmap task that adds it:

- the draw-list painter and ui goldens — M3.5.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Usage

A workspace crate whose row of the matrix allows it depends on it by path:

```toml
[dependencies]
tidepool-gui = { path = "../gui" }
```

Open the window and answer it until it is asked to close, as the
`tidepool` binary does (this needs a display, so the example is only
compiled):

```rust,no_run
use tidepool_gui::{Gui, GuiError};
use tidepool_platform::window::WindowConfig;

fn main() -> Result<(), GuiError> {
    let mut gui = Gui::open(&WindowConfig::default())?;
    loop {
        let turn = gui.turn(None)?;
        if turn.close_requested {
            return Ok(());
        }
    }
}
```

Draw the first frame without a window, for a size and scale of your
choosing; the backdrop is most of its cost, about 100 ms for 1280 × 800 in
a release build:

```rust
use tidepool_core::geom::{DevicePoint, DeviceSize, Scale};
use tidepool_gui::first_frame::FirstFrame;

let mut first = FirstFrame::new().expect("the bundled faces load");
let frame = first.draw(DeviceSize::new(320, 120), Scale::ONE).expect("a frame");
assert_eq!(frame.size(), DeviceSize::new(320, 120));
// The backdrop is opaque, so every pixel is.
assert!(frame.pixel(DevicePoint::new(300, 100)).is_some_and(|pixel| pixel.a == 255));
```

## Documentation

- Design: [`docs/subsystems/frontends.md`](../../docs/subsystems/frontends.md),
  [`docs/subsystems/rendering.md`](../../docs/subsystems/rendering.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0003 (GUI
  first); ADR-0011 (CPU rendering); ADR-0045 (the first frame: the
  backdrop on the main thread, `gfx` and `font` optimized in dev builds,
  the token stand-ins, a line in each face); ADR-0047 (the theme's
  colors in their place).
- API reference: `cargo doc -p tidepool-gui --open`

## Rules

- `unsafe` is denied by the workspace lint, and in doc tests by the crate
  root's `#![doc(test(attr(forbid(unsafe_code))))]` (ADR-0036);
  `cargo xtask unsafe-audit` checks both.
- No literal colors outside tests: the text colors come from
  `tidepool-ui`'s theme (ADR-0047), and the backdrop's from `gfx`'s
  `BackdropStyle::jabbott_dev`, whose `bg` a test holds to the theme's.
- Path-scoped rules in `.claude/rules/`:
  [`ui-style.md`](../../.claude/rules/ui-style.md),
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.

## Checks

```bash
cargo test -p tidepool-gui
TIDEPOOL_IT_X11=1 xvfb-run -a cargo test -p tidepool-gui
```

The unit tests draw the first frame without a window: every face has a
line, the text lands where the lines go in the tokens' colors, at scale 1
and 2, on its quarter pixel, the same bytes twice, and small and empty
windows; and they check a turn's handling of events and the frame's
limit of 16,384 px a side. The window test,
`tests/window.rs`, opens the window and sees its first frame presented and
an idle turn present nothing; it runs only with `TIDEPOOL_IT_X11=1`, as
`platform`'s do, and prints `SKIPPED:` otherwise (ADR-0039). `cargo xtask
ci` runs the whole gate, and Linux CI's runs the window test too
(ADR-0041).

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
