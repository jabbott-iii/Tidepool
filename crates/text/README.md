# tidepool-text

The text engine every buffer is built on: a persistent rope, positions and
their conversions (UTF-16 included), extended grapheme clusters,
multi-cursor selections, canonical change sets, and the undo tree. Positions
are byte offsets, and a bad position is an error, never a panic.

Layer 1, OS behavior, formats and pure engines. It may use `core` (its tests
use `core`'s property harness), as its row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Built, with the roadmap task that built it:

- `rope` and `pos`, with grapheme boundaries (UAX #29) from tables that
  `cargo xtask ucd` generates from `ucd/17.0.0/` — M1.1.
- `selection`, `transaction` and `history` — M1.2.

Not yet, with the roadmap task that adds it:

- `encoding` and `file` (atomic save, large-file threshold) — M1.3.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Design

- [`docs/subsystems/text-and-formats.md`](../../docs/subsystems/text-and-formats.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0018 (the
  Unicode data); ADR-0019 (positions); ADR-0020 (edits and undo).

## Rules

- `unsafe` is denied by the workspace lint, and `cargo xtask unsafe-audit`
  checks.
- Path-scoped rules in `.claude/rules/`:
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.
- Never edit `src/pos/grapheme/tables.rs` by hand: `cargo xtask ucd` writes
  it.

## Checks

```bash
cargo test -p tidepool-text
cargo xtask ucd --check
```

`cargo xtask ci` runs the whole gate.

## License

Apache-2.0, like the rest of Tidepool: see [`LICENSE`](../../LICENSE).
