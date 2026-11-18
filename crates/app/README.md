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

Not yet, with the roadmap task that adds it:

- the main loop and the shell — M3.4.
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
shows the first frame until you close it. With `--print-startup-time` it
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
  main loop, Proposed); ADR-0045 (the first frame, its budget and
  `--print-startup-time`).
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
cargo tp -- --print-startup-time
```

The tests check the command line and the startup line, and run the
binary with arguments it refuses, which opens no window. The second
command opens the window on your display: its line must stay within the
budget, and the window must close with its close button. `cargo xtask ci`
runs the whole gate.

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
