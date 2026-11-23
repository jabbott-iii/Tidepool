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

- the first frame: `Gui::open` and `Gui::size`, which open a window
  through `tidepool-platform`; the first frame's content, jabbott.dev's
  backdrop and over it a line in each of the seven bundled faces, its
  name in `text-3` and a sample in `text`; and `GuiError` — M2.6
  (ADR-0045). M3.8 made the lines a `ui` tree and removed `Gui::turn`,
  `Turn` and `first_frame::FirstFrame` (ADR-0059).
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
  `font::layout`, whose `scale` is its scale's, so that `ui` measures the
  press guard's slop in device pixels (ADR-0056), and `metrics::face`
  picks a style's bundled face as CSS would. 23 golden scenes of `ui`
  trees, compared exactly — M3.5 (ADR-0048).
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
- 8 golden scenes of `ui`'s overlays, the modal dialog over a window it
  makes inert, the tooltip, the toast and the command palette (its
  matched characters in Saira Bold), each in each state at scale 1 and
  2, every cell a tree of its own put in its state by real events
  (`tests/overlays/mod.rs`, a module of the `golden` target); their
  references are blessed once the owner has looked at them — M3.3
  (ADR-0052).
- the GUI's half of the event-driven main loop (ADR-0012):
  `Gui::wait_events` waits on the window, notes what it asks for (exposes
  and resizes), takes the backdrop's finished builds, and leaves every
  event for the caller; `Gui::render` lays a `ui` tree out at the
  window's size in logical units (its device size over its scale, read
  from `Xft.dpi` on Linux, fractional scales included), paints the frame's damage with
  `Painter` over the base, and presents that with the window's exposes, a
  resize or a new base repainting the whole window, and an idle tree
  nothing. The backdrop is built on threads of its own
  (`tidepool-backdrop`), never on the caller's: asked for when the window
  opens and on each resize, at most two at once, a waiting request
  replaced by a newer one, a stale result dropped, and a failed build or
  a thread that cannot start logged as a warning; until it arrives, the
  base is the tree's theme's flat `bg`. The first present waits for it,
  up to `FIRST_FRAME_WAIT` (250 ms) after the window opened, and
  `Gui::next_deadline` gives the loop that deadline. `Gui::scale`,
  `Gui::backdrop_ready`, and `first_frame::tree`, the first frame's lines
  as a `ui` tree, idle after its first frame — M3.8 (ADR-0059).

Not yet, with the roadmap task that adds it:

- the shell, which replaces the first frame's lines in the window — M3.4.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Usage

A workspace crate whose row of the matrix allows it depends on it by path:

```toml
[dependencies]
tidepool-gui = { path = "../gui" }
```

Open the window and show the first frame until the window is asked to
close, in an event-driven loop like the `tidepool` binary's: wait until
the window has events or `Gui::next_deadline` passes, route the input to
the tree, then render (this needs a display, so the example is only
compiled):

```rust,no_run
use tidepool_gui::{Gui, GuiError, first_frame};
use tidepool_platform::events::Event;
use tidepool_platform::window::WindowConfig;

fn main() -> Result<(), GuiError> {
    let mut gui = Gui::open(&WindowConfig::default())?;
    let mut tree = first_frame::tree();
    let mut events = Vec::new();
    loop {
        events.clear();
        gui.wait_events(gui.next_deadline(), &mut events)?;
        for event in &events {
            match event {
                Event::Input(input) => {
                    tree.handle(input);
                }
                Event::CloseRequested => return Ok(()),
                _ => {}
            }
        }
        gui.render(&mut tree)?;
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

Paint the first frame's lines without a window, at scale 1.5 over the
theme's flat `bg`, as the window shows them until the backdrop arrives;
the tree is idle after its first frame:

```rust
use tidepool_core::geom::{DevicePoint, DeviceSize, Scale};
use tidepool_gfx::framebuffer::Framebuffer;
use tidepool_gui::{Painter, first_frame};
use tidepool_ui::theme::Token;

let mut tree = first_frame::tree();
let mut painter = Painter::new().expect("the bundled faces load");
let (size, scale) = (DeviceSize::new(480, 300), Scale::new(1.5).expect("a scale"));
let window = scale.to_logical(size);
let frame = tree.frame(window, &painter.metrics(scale)).expect("the first frame");

let bg = tree.theme().color(Token::Bg).premultiply();
let mut base = Framebuffer::new(size).expect("a base");
base.clear(bg);
let mut target = Framebuffer::new(size).expect("a target");
painter.paint(&mut target, &base.view(), &frame, scale).expect("painted");
// The lines ink the frame; above the first, it is the flat `bg`.
assert_ne!(target.bytes(), base.bytes());
assert_eq!(target.pixel(DevicePoint::new(479, 0)), Some(bg));
assert!(tree.frame(window, &painter.metrics(scale)).is_none());
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
  the golden scenes); ADR-0012 (the event-driven main loop, on plain
  threads); ADR-0059 (`gui` presents `ui` frames, the backdrop built on
  threads of its own over the flat `bg` meanwhile, and the first
  present's wait of up to 250 ms).
- API reference: `cargo doc -p tidepool-gui --open`

## Rules

- `unsafe` is denied by the workspace lint, and in doc tests by the crate
  root's `#![doc(test(attr(forbid(unsafe_code))))]` (ADR-0036);
  `cargo xtask unsafe-audit` checks both.
- No literal colors outside tests: the text colors and the flat base's
  `bg` come from `tidepool-ui`'s theme (ADR-0047), and the backdrop's
  from `gfx`'s `BackdropStyle::jabbott_dev`, whose `bg` a test holds to
  the theme's. The painter draws the colors a draw list carries.
- The thread that calls `Gui` never builds a backdrop and never waits on
  a build: builds run on threads of their own, and their results are
  taken without waiting (ADR-0012, ADR-0059).
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

The unit tests run without a window. The first frame's: every face has a
line, in order, chrome in uppercase; its node fills the window; the
labels and samples go in their two columns on baselines 32 apart, and
their ink lands where the lines go at scale 1 and 2, in the tokens'
colors, the same bytes twice; and it is idle after its first frame. The
screen's, the part of `Gui` that needs no window, at scales 1, 1.5 and
2: the tree is laid out at the device size over the scale and painted
at the device size; a new size, and a new base (the backdrop arriving),
repaint the whole window though the tree did not change; an idle tree
paints and presents nothing; exposes are kept until a present shows
them, before the first paint too; the first present waits for the
backdrop until its deadline; a failed present is presented again; the
flat base is the theme's `bg`, which is the backdrop's; and small, empty
and impossible sizes. The builds': at most two run, a newer request
replaces the waiting one, a stale result is dropped and the waiting
request then starts, a request already current or being built starts
nothing, and a failed build or a thread that cannot start logs a
warning, all with a stand-in builder and no threads; and, on real
threads, a build sends its backdrop and then wakes, and a build `gfx`
refuses or that panics is reported. The frame's limit is 16,384 px a
side. The painter's check each command against what
`gfx` draws for it, the rounding at fractional scales, neighbours `ui`
lays out sharing their edges, clips, layers and their growth, text
against `font`'s own glyphs, icons, images, the tiles, and geometry that
is NaN or huge. `tests/paint.rs` paints random `ui` trees, changed frame
by frame at scales 1 to 2, and checks that each frame changes only its
damage and that painting the damage gives what painting the whole frame
gives, idle frames included; a second property aims slivers of
anti-aliased ink past a clipping node's edge. `tests/golden.rs` paints
the 23 `ui_*` scenes, and its modules `controls`, `components`, and
`overlays` the controls' 16, M3.7's components' 14, and the overlays'
8, which `cargo xtask golden` compares exactly with
`tests/golden/` (ADR-0044, ADR-0048). The window test,
`tests/window.rs`, opens the window, renders the first frame's tree until
it is presented and then until the backdrop arrives from its thread, and
sees an idle wait run out its deadline and a render present nothing; it
runs only with `TIDEPOOL_IT_X11=1`, as `platform`'s do, and prints
`SKIPPED:` otherwise (ADR-0039). `cargo xtask
ci` runs the whole gate, and Linux CI's runs the window test too
(ADR-0041).

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
