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

- an empty `main`, so the workspace builds a binary — M0.1.

Not yet, with the roadmap task that adds it:

- the first frame — M2.6.
- the main loop and the shell — M3.4.
- crash reports and buffer recovery — M4.9.
- `--tui` — M7.1, M7.5.
- `--smoke-test` — M14.4.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Usage

```bash
cargo tp
```

`cargo tp` builds and runs the binary. It does nothing until M2.6: it
ignores its arguments and exits 0. `cargo tp -- --tui` starts the terminal
UI from M7.1.

## Documentation

- Design: [`docs/01-architecture.md`](../../docs/01-architecture.md) ("The
  main loop")
- Release: [`docs/05-release.md`](../../docs/05-release.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0012 (the
  main loop, Proposed).
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
cargo tp
```

`cargo xtask ci` runs the whole gate.

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
