# tidepool-tui

The terminal frontend: it maps the same `ui` draw lists onto terminal cells,
decodes keys and mouse input (the kitty protocol when offered), and restores
the terminal on every exit path. It holds no feature logic.

Layer 4, views and frontends. It may use `core`, `platform` and `ui`, as its
row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Nothing yet: the crate is only its doc comment, which names its spec.

Not yet, with the roadmap task that adds it:

- the renderer and the input decoder — M7.1.
- end-to-end sessions — M7.5.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Design

- [`docs/subsystems/frontends.md`](../../docs/subsystems/frontends.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0003 (the
  TUI follows the GUI).

## Rules

- `unsafe` is denied by the workspace lint, and `cargo xtask unsafe-audit`
  checks.
- Path-scoped rules in `.claude/rules/`:
  [`ui-style.md`](../../.claude/rules/ui-style.md),
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.

## Checks

```bash
cargo test -p tidepool-tui
```

`cargo xtask ci` runs the whole gate.

## License

Apache-2.0, like the rest of Tidepool: see [`LICENSE`](../../LICENSE).
