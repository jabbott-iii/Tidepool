# tidepool-bench

Tidepool's own benchmark harness and suites: warm-up, N samples, median and
p95, against a JSON baseline. Dev-only; it never ships.

Layer 6, tools (dev-only). It may use any workspace crate, as its row of the
dependency matrix in [`docs/01-architecture.md`](../docs/01-architecture.md)
says.

## Status

Nothing yet: the crate is only its doc comment, which names its spec.

Not yet, with the roadmap task that adds it:

- the harness, `cargo xtask bench` and the text suites — M1.9.

Tasks and their state are in [`docs/roadmap.md`](../docs/roadmap.md).

## Design

- [`docs/04-quality.md`](../docs/04-quality.md) ("Benchmarks")
- Decisions in [`docs/decisions.md`](../docs/decisions.md): ADR-0017 (M1.9
  asks to extend it).

## Rules

- `unsafe` is denied by the workspace lint, and `cargo xtask unsafe-audit`
  checks.
- Path-scoped rules in `.claude/rules/`:
  [`dependencies.md`](../.claude/rules/dependencies.md) for its
  `Cargo.toml`.

## Checks

```bash
cargo xtask bench    # from M1.9
```

`cargo xtask ci` runs the whole gate.

## License

Apache-2.0, like the rest of Tidepool: see [`LICENSE`](../LICENSE).
