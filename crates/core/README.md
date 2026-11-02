# tidepool-core

Tidepool's foundations, which every other crate uses: errors with context,
logging with secret redaction, the property-test harness, a seeded RNG,
hashing (SHA-256 for download integrity only), argument parsing, UTC time,
and the geometry, color and input types that `platform`, `ui`, `editor` and
both frontends share. It has no dependencies, no `unsafe` and no OS-specific
code.

Layer 0, foundations. It may use no workspace crate, as its row of the
dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Built, with the roadmap task that built it:

- `error`, `log`, `testing`, `rng`, `hash`, `cli`, `civil`, `input`, `geom`,
  `color` and `cancel`, as `docs/subsystems/core.md` → "Public API names"
  lists them — M0.2.

Not yet, with the roadmap task that adds it:

- `path`: UTF-8 path helpers, canonicalization, and the display of paths
  that are not UTF-8 — M1.8.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Design

- [`docs/subsystems/core.md`](../../docs/subsystems/core.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0016
  (SHA-256 for downloads only).

## Rules

- `unsafe` is denied by the workspace lint, and `cargo xtask unsafe-audit`
  checks.
- Path-scoped rules in `.claude/rules/`:
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.

## Checks

```bash
cargo test -p tidepool-core
```

`cargo xtask ci` runs the whole gate.

## License

Apache-2.0, like the rest of Tidepool: see [`LICENSE`](../../LICENSE).
