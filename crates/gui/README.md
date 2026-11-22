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
- the draw-list painter: `paint::Painter` paints `ui`'s frames into a
  framebuffer with `gfx` and `font`, their damage only, over a base it
  copies in first: every draw command, clips, layers (each a framebuffer
  as large as what it inks), the icons (`icon::Icon`, outlines it fills
  with `font`'s rasterizer) and images registered with it; each
  command's ink is clipped to its bounds, and lines and cut corners are
  filled in fixed tiles, so painting the damage gives what painting the
  whole frame gives. `metrics::FontMetrics` is `ui`'s `TextMetrics` with
  `font::layout`, and `metrics::face` picks a style's bundled face as
  CSS would. 23 golden scenes of `ui` trees, compared exactly — M3.5
  (ADR-0048).
- 16 golden scenes of `ui`'s controls, each in each state at scale 1 and
  2, every cell a tree of its own put in its state by real events
  (`tests/controls/mod.rs`, a module of the `golden` target) — M3.6
  (ADR-0049).
- 14 golden scenes of `ui`'s M3.7 components, the list, tree view, table,
  tab strip, splitter, card and code block, each in each state, theirs
  included, at scale 1 and 2, every cell a tree of its own put in its
  state by real events (`tests/components/mod.rs`, a module of the
  `golden` target); their references are blessed once the owner has
  looked at them — M3.7 (ADR-0052).

Not yet, with the roadmap task that adds it:

- painting `ui`'s frames in the window, with the shell, and the
  backdrop's rebuild off the main thread — M3.4.

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

Paint a `ui` tree's frame without a window, at scale 2, over a flat base:
only its damage is painted, and an idle tree gives no frame:

```rust
use tidepool_core::color::Color;
use tidepool_core::geom::{DevicePoint, DeviceSize, Scale, Size};
use tidepool_gfx::framebuffer::Framebuffer;
use tidepool_gui::Painter;
use tidepool_ui::draw::{DrawList, PaintContext};
use tidepool_ui::layout::Style;
use tidepool_ui::{Node, Tree, Widget};

/// Fills its bounds.
struct Fill(Color);

impl Widget for Fill {
    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        list.rect(cx.bounds(), self.0);
    }
}

let teal = Color::rgb(0x5c, 0xcb, 0xcb);
let mut tree = Tree::new();
let root = tree.root();
tree.insert(root, Node::new(Style::default().fixed(10.0, 10.0)).with_widget(Fill(teal)))
    .expect("a node");

let mut painter = Painter::new().expect("the bundled faces load");
let scale = Scale::new(2.0).expect("a scale");
let window = Size::new(40.0, 20.0);
let frame = tree.frame(window, &painter.metrics(scale)).expect("the first frame");

let size = DeviceSize::new(80, 40);
let mut base = Framebuffer::new(size).expect("a base");
base.clear(Color::BLACK.premultiply());
let mut target = Framebuffer::new(size).expect("a target");
painter.paint(&mut target, &base.view(), &frame, scale).expect("painted");
assert_eq!(target.pixel(DevicePoint::new(19, 19)), Some(teal.premultiply()));
assert!(tree.frame(window, &painter.metrics(scale)).is_none());
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
  colors in their place); ADR-0048 (the painter: the damage only, its
  rounding, layers, tiles, icons as outlines, images, the metrics and
  the golden scenes).
- API reference: `cargo doc -p tidepool-gui --open`

## Rules

- `unsafe` is denied by the workspace lint, and in doc tests by the crate
  root's `#![doc(test(attr(forbid(unsafe_code))))]` (ADR-0036);
  `cargo xtask unsafe-audit` checks both.
- No literal colors outside tests: the text colors come from
  `tidepool-ui`'s theme (ADR-0047), and the backdrop's from `gfx`'s
  `BackdropStyle::jabbott_dev`, whose `bg` a test holds to the theme's.
  The painter draws the colors a draw list carries.
- Painting the damage must give what painting the whole frame gives:
  a command never inks past its bounds, cut to its clips, and nothing it
  draws may depend on where a damage rect's edges fall (ADR-0048). The
  caller damages what draws an icon or image whose registration changed.
- Path-scoped rules in `.claude/rules/`:
  [`ui-style.md`](../../.claude/rules/ui-style.md),
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.

## Checks

```bash
cargo test -p tidepool-gui
cargo xtask golden
TIDEPOOL_IT_X11=1 xvfb-run -a cargo test -p tidepool-gui
```

The unit tests draw the first frame without a window: every face has a
line, the text lands where the lines go in the tokens' colors, at scale 1
and 2, on its quarter pixel, the same bytes twice, and small and empty
windows; and they check a turn's handling of events and the frame's
limit of 16,384 px a side. The painter's check each command against what
`gfx` draws for it, the rounding at fractional scales, neighbours `ui`
lays out sharing their edges, clips, layers and their growth, text
against `font`'s own glyphs, icons, images, the tiles, and geometry that
is NaN or huge. `tests/paint.rs` paints random `ui` trees, changed frame
by frame at scales 1 to 2, and checks that each frame changes only its
damage and that painting the damage gives what painting the whole frame
gives, idle frames included; a second property aims slivers of
anti-aliased ink past a clipping node's edge. `tests/golden.rs` paints
the 23 `ui_*` scenes, and its modules `controls` and `components` the
controls' 16 and M3.7's components' 14, which `cargo xtask golden`
compares exactly with `tests/golden/` (ADR-0044, ADR-0048). The window test,
`tests/window.rs`, opens the window and sees its first frame presented and
an idle turn present nothing; it runs only with `TIDEPOOL_IT_X11=1`, as
`platform`'s do, and prints `SKIPPED:` otherwise (ADR-0039). `cargo xtask
ci` runs the whole gate, and Linux CI's runs the window test too
(ADR-0041).

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
