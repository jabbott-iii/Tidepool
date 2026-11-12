# tidepool-tui

The terminal frontend: it maps the same `ui` draw lists onto terminal cells,
decodes keys and mouse input (the kitty protocol when offered), and restores
the terminal on every exit path. It holds no feature logic.

It is part of [Tidepool](https://github.com/jabbott-iii/Tidepool), where
the `tidepool` binary connects it to `editor`, which it never depends on; it
reaches the terminal through `tidepool-platform`.

Layer 4, views and frontends. It may use `core`, `platform` and `ui`, as its
row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Version 0.1, not published (`publish = false`): the API changes as the
roadmap needs.

Nothing yet: besides what every library carries (ADR-0028, ADR-0036),
the crate is only its doc comment, which names its spec.

Not yet, with the roadmap task that adds it:

- the renderer and the input decoder — M7.1.
- end-to-end sessions — M7.5.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Usage

Nothing to use yet: the renderer and the input decoder land in M7.1.

## Documentation

- Design: [`docs/subsystems/frontends.md`](../../docs/subsystems/frontends.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0003 (the
  TUI follows the GUI).
- API reference: `cargo doc -p tidepool-tui --open`

## Rules

- `unsafe` is denied by the workspace lint, and in doc tests by the crate
  root's `#![doc(test(attr(forbid(unsafe_code))))]` (ADR-0036);
  `cargo xtask unsafe-audit` checks both.
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

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
