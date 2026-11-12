# tidepool-ui

The UI toolkit: a retained widget tree, layout in logical units, focus and
input routing, the `tidepool-dark` theme in jabbott.dev's design language,
draw lists that both frontends render, and the accessibility tree. It knows
nothing about pixels, fonts or terminals.

It is part of [Tidepool](https://github.com/jabbott-iii/Tidepool), where
`editor` builds its views with it, and `tidepool-gui` and `tidepool-tui`
render its draw lists; it needs only `tidepool-core`.

Layer 2, services. It may use `core`, as its row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Version 0.1, not published (`publish = false`): the API changes as the
roadmap needs.

Nothing yet: besides what every library carries (ADR-0028, ADR-0036),
the crate is only its doc comment, which names its spec.

Not yet, with the roadmap task that adds it:

- `tree`, `layout`, `focus`, `input`, `metrics`, `draw` and `a11y` — M3.1.
- `theme` and `cargo xtask theme` — M3.2.
- the controls — M3.6.
- collections and overlays — M3.3.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Usage

Nothing to use yet: `tree`, `layout`, `focus`, `input`, `metrics`, `draw`
and `a11y` land in M3.1.

## Documentation

- Design:
  [`docs/subsystems/ui-and-theme.md`](../../docs/subsystems/ui-and-theme.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0004 (the
  jabbott.dev design language).
- API reference: `cargo doc -p tidepool-ui --open`

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
cargo test -p tidepool-ui
cargo xtask theme    # from M3.2
cargo xtask golden    # from M2.4
```

`cargo xtask ci` runs the whole gate.

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
