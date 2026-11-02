# tidepool

The Tidepool binary: the event-driven main loop, the wiring between `editor`
and the frontends, startup, settings loading and crash handling. `cargo tp`
runs it.

Layer 5, the application. It may use all crates above, as its row of the
dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Built, with the roadmap task that built it:

- an empty `main`, so the workspace builds a binary — M0.1.

Not yet, with the roadmap task that adds it:

- the first frame — M2.6.
- the main loop and the shell — M3.4.
- crash reports and buffer recovery — M4.9.
- `--tui` — M7.1, M7.5.
- `--smoke-test` — M14.4.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Design

- [`docs/01-architecture.md`](../../docs/01-architecture.md) ("The main
  loop")
- [`docs/05-release.md`](../../docs/05-release.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0012 (the
  main loop, Proposed).

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

Apache-2.0, like the rest of Tidepool: see [`LICENSE`](../../LICENSE).
