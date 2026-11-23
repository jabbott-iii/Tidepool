# tidepool

The Tidepool binary: the event-driven main loop, the wiring between `editor`
and the frontends, startup, settings loading and crash handling.

It is part of [Tidepool](https://github.com/jabbott-iii/Tidepool) as the
program itself: it may use every crate above it, and no crate uses it.

Layer 5, the application. It may use all crates above, as its row of the
dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Version 0.1, not published (`publish = false`): the API changes as the
roadmap needs.

Built, with the roadmap task that built it:

- a `main`, so the workspace builds a binary — M0.1.
- the first frame: on Linux it opens the GUI's window, shows jabbott.dev's
  backdrop and a line in each bundled face until the window is closed,
  and with `--print-startup-time` prints the time from the start of
  `main` to that frame; it refuses any other argument, exiting 2 (a
  `--` only ends the options) — M2.6 (ADR-0045).
- the event-driven main loop: it waits on the window until something
  happens or the earliest deadline passes, hands each input event to the
  `ui` tree first, stops at a close request, and presents only what
  changed, so an idle Tidepool uses no processor; the first frame's lines
  are a `ui` tree that `tidepool-gui` paints, and the backdrop is built
  off the main thread — M3.8 (ADR-0012, ADR-0059).
- the shell: the window shows `tidepool-editor`'s shell, its activity
  bar's six icons registered from `tidepool-gui`'s, with the shell's
  tooltip timer among the loop's deadlines; and the settings, read once at
  startup, read-only: `[ui] side_panel` and `bottom_panel` in
  `settings.toml` in the user's config folder say which panels show, a
  bad value falling back to its default with a warning in the log; and
  the shell's golden scenes, `tests/golden.rs` — M3.4 (ADR-0060).

Not yet, with the roadmap task that adds it:

- settings written back, and the workspace's own — M4.7.
- the channel of messages from workers to the loop — M4.3.
- crash reports and buffer recovery — M4.9.
- `--tui` — M7.1, M7.5.
- `--smoke-test` — M14.4.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Usage

```bash
cargo tp
cargo tp -- --print-startup-time
```

`cargo tp` builds and runs the binary, which opens a window on Linux and
shows the shell until you close it: Ctrl+Shift+P or F1 opens the command
palette, and Ctrl+B and Ctrl+J toggle the side and bottom panels. Two
settings, in `~/.config/tidepool/settings.toml` on Linux (or
`$XDG_CONFIG_HOME/tidepool/settings.toml`), say which panels show at
start:

```toml
[ui]
side_panel = true
bottom_panel = false
```

 With `--print-startup-time` it
also prints a line such as `first frame: 131.5 ms`, the time from the
start of `main` to the return of the first present; the budget is
300 ms on the reference machine (`docs/00-overview.md`). `cargo tp` is a
dev build, with `gfx` and `font` optimized (ADR-0045). Elsewhere than
Linux it fails, as there is no window there yet. `cargo tp -- --tui`
starts the terminal UI from M7.1.

## Documentation

- Design: [`docs/01-architecture.md`](../../docs/01-architecture.md) ("The
  main loop")
- Release: [`docs/05-release.md`](../../docs/05-release.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0012 (the
  event-driven main loop on plain threads, no async runtime); ADR-0045
  (the first frame, its budget and `--print-startup-time`); ADR-0059 (the
  loop, built in M3.8, and the backdrop built off the main thread);
  ADR-0060 (the shell, its icons, the settings it reads and its golden
  scenes).
- API reference: `cargo doc -p tidepool --open`

## Rules

- `unsafe` is denied by the workspace lint, and `cargo xtask unsafe-audit`
  checks.
- Path-scoped rules in `.claude/rules/`:
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.

## Checks

```bash
cargo test -p tidepool
TIDEPOOL_IT_X11=1 xvfb-run -a cargo test -p tidepool
cargo xtask golden
cargo tp -- --print-startup-time
```

The tests check the command line, the startup line, that the loop hands
input to the shell in order and stops at a close request, the earliest
deadline, a distinct icon for each view, and the settings: the defaults,
the two keys, unknown keys ignored, a wrong type, a file that is not
TOML, missing or too large, each warning logged; they run the binary with
arguments it refuses, which opens no window. Its `golden` target paints
the shell's scenes, which `cargo xtask golden` compares. The second
command also runs the window test, under Xvfb (ADR-0041): the binary
reaches its first frame, then uses no processor time over an idle second,
read from `/proc`; without `TIDEPOOL_IT_X11=1` it prints `SKIPPED:`. The
third draws and compares the golden scenes, the shell's among them. The
fourth opens the window on your display: its line must stay within the
budget, a resize must repaint at once with the backdrop following, and
the window must close with its close button. `cargo xtask ci` runs the
whole gate.

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
