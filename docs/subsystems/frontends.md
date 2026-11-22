# Subsystem: `tidepool-gui` and `tidepool-tui`

Two thin frontends over the same `ui` draw lists (ADR-0003). Neither knows
about editors, languages or tools.

## Common interface

```rust
pub trait Frontend {
    fn wait_events(&mut self, deadline: Option<Instant>, out: &mut Vec<InputEvent>) -> Result<(), Error>;
    fn waker(&self) -> Waker;
    fn metrics(&self) -> &dyn TextMetrics;   // GUI: font metrics in logical px; TUI: 1 unit = 1 cell
    fn present(&mut self, frame: &Frame) -> Result<(), Error>;   // draw list + damage
    fn clipboard(&mut self) -> &mut dyn Clipboard;
}
```

`app` picks the frontend: GUI by default, TUI with `tidepool --tui` (or
automatically when no display is available, with a message).

## `tidepool-gui`

- Opens a `platform::Window`, keeps a `gfx::Framebuffer` at device
  resolution, and rasterizes the damaged parts of each draw list with `gfx`
  and `font`.
- **The painter** (M3.5, ADR-0048): `paint::Painter::paint` paints a
  `ui::draw::Frame` into a framebuffer, its damage only. Each damage rect
  is rounded out to whole device pixels, at most 8 kept, and cut to the
  target and a base image (the backdrop, or a flat `bg`), which is copied
  in first; then the whole list is drawn, clipped to the rect. Each
  command's ink is clipped to its own bounds, cut to its clips and rounded
  out, so a frame touches only pixels in its damage and painting the
  damage gives what painting the whole frame gives. Rect edges round to
  the nearest device pixel (the right and bottom summed in `f32`, as
  `ui` places a neighbour); border widths and bracket thickness snap as
  CSS snaps a border width; lines, cuts and pills are anti-aliased; text
  is set on a whole-pixel baseline from a quarter-pixel pen. A layer is a
  framebuffer covering what inks in it, at most 8 nested. Icons are
  outlines in a square viewbox (`icon::Icon`), registered with the
  painter and filled by `font::outline::rasterize` at their device size;
  images are premultiplied framebuffers registered with it and scaled
  bilinearly; the caller damages what draws an id whose registration
  changed. The rustdoc of `gui`'s modules is authoritative.
- **Text metrics** (M3.5): `metrics::FontMetrics` implements
  `TextMetrics` with `font::layout`, in logical units at the scale it is
  made for, as the painter draws: a style takes the bundled face CSS's
  font matching would (italic before weight; no synthesized slant).
- **The window** still shows the first frame (M2.6, ADR-0045) until the
  shell paints `ui`'s frames in it (M3.4): `Gui::turn` waits for the
  window's events and presents what an expose asks for, from a frame that
  `first_frame::FirstFrame` draws anew, on the calling thread, whenever
  the window's size or scale has changed: jabbott.dev's backdrop
  (`gfx::backdrop`) and a line in each bundled face, its name in `text-3`
  and a sample in `text`, from the typography table
  (`docs/subsystems/ui-and-theme.md`), the two text colors from `ui`'s
  bundled `tidepool-dark` theme (M3.2, ADR-0047); the backdrop's are
  `gfx`'s `BackdropStyle::jabbott_dev()`, whose `bg` a test holds to the
  theme's.
- Draws the jabbott.dev look in full (`docs/subsystems/ui-and-theme.md`).
- HiDPI: logical units × scale factor; integer and fractional scales.
- Clipboard: the window's, from `platform::clipboard`. A paste is asked
  for and answered by a later `Event::Pasted`, so the GUI's `Clipboard`
  hands the text over when it comes, not when it is asked (ADR-0042).

## `tidepool-tui`

- Uses `platform::terminal`: raw mode, alternate screen, resize events,
  stdin reader. The terminal is restored on exit, panic and signals.
- **Rendering:** maps draw-list primitives onto a cell grid: fills → cell
  backgrounds, text runs → cells (East Asian Wide = 2 cells), borders and
  bracket corners → box-drawing characters, icons → single-cell symbols.
  Diffs against the previous grid and writes the minimum escape sequences.
- **Color:** truecolor when `COLORTERM` says so, else 256 colors with the
  theme's fallback indices.
- **Input decoding:** UTF-8 text; CSI and SS3 keys with modifiers; kitty
  keyboard protocol when the terminal answers the query (distinguishes
  `Ctrl-i` from `Tab`, `Esc` without delay); SGR mouse (1006); bracketed
  paste; focus events. Plain `Esc` uses a short configurable timeout (default
  25 ms) when the kitty protocol is not available, so vim mode feels right.
- **Clipboard:** OSC 52 write (and read where the terminal allows it); falls
  back to an internal register.
- **Cursor shape:** DECSCUSR block/bar to match vim mode.
- Works over SSH and inside tmux (passthrough rules for OSC 52 documented).

## Tests

- GUI: golden frames of each draw command, clips and layers included,
  at scale 1 and 2, and a mix at 1.5 (`crates/gui/tests/golden.rs`,
  `ui_*`, compared exactly, M3.5), of each of `ui`'s controls in each
  state, rest, hover, focus, pressed and disabled, at scale 1 and 2
  (`crates/gui/tests/controls/mod.rs`, M3.6, ADR-0049), of each of M3.7's
  components, the list, tree view, table, tab strip, splitter, card, and
  code block, in each state at scale 1 and 2
  (`crates/gui/tests/components/mod.rs`, M3.7, ADR-0052), of the modal
  dialog over a window it makes inert, the tooltip, the toast, and the
  command palette, in each state at scale 1 and 2
  (`crates/gui/tests/overlays/mod.rs`, M3.3, ADR-0052), and of the shell
  (M3.4); property tests
  that a painted frame changes only pixels inside its damage, and that
  painting the damage gives what painting the whole frame gives, idle
  frames included (`crates/gui/tests/paint.rs`, M3.5); an Xvfb smoke test,
  `crates/gui/tests/window.rs`, that opens the window, sees the first
  frame presented and an idle turn present nothing, gated by
  `TIDEPOOL_IT_X11` and run by Linux CI's gate (M2.6, ADR-0041).
- TUI: cell-grid snapshots (`tests/golden/tui/`) for the shell, vim mode
  chips, popups and resize; input decoder tables for xterm, kitty, tmux and
  the Linux console sequences; a pseudo-terminal integration test runs
  `tidepool --tui` inside a PTY, types, saves and exits.
