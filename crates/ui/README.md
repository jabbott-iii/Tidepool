# tidepool-ui

The UI toolkit: a retained widget tree, layout in logical units, focus and
input routing, the `tidepool-dark` theme in jabbott.dev's design language,
draw lists that both frontends render, and the accessibility tree. It knows
nothing about pixels, fonts or terminals.

Layer 2, services. It may use `core`, as its row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Nothing yet: the crate is only its doc comment, which names its spec.

Not yet, with the roadmap task that adds it:

- `tree`, `layout`, `focus`, `input`, `metrics`, `draw` and `a11y` — M3.1.
- `theme` and `cargo xtask theme` — M3.2.
- the controls — M3.6.
- collections and overlays — M3.3.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Design

- [`docs/subsystems/ui-and-theme.md`](../../docs/subsystems/ui-and-theme.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0004 (the
  jabbott.dev design language).

## Rules

- `unsafe` is denied by the workspace lint, and `cargo xtask unsafe-audit`
  checks.
- Path-scoped rules in `.claude/rules/`:
  [`ui-style.md`](../../.claude/rules/ui-style.md),
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.

## Checks

```bash
cargo test -p tidepool-ui
cargo xtask theme    # from M3.2
cargo xtask golden    # from M2.4
```

`cargo xtask ci` runs the whole gate.

## License

Apache-2.0, like the rest of Tidepool: see [`LICENSE`](../../LICENSE).
