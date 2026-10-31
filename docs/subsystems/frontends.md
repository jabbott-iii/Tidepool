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
- Implements `TextMetrics` with `font::layout`.
- Draws the jabbott.dev look in full (`docs/subsystems/ui-and-theme.md`).
- HiDPI: logical units × scale factor; integer and fractional scales.

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

- GUI: golden frames of the shell; Xvfb smoke test on Linux CI.
- TUI: cell-grid snapshots (`tests/golden/tui/`) for the shell, vim mode
  chips, popups and resize; input decoder tables for xterm, kitty, tmux and
  the Linux console sequences; a pseudo-terminal integration test runs
  `tidepool --tui` inside a PTY, types, saves and exits.
