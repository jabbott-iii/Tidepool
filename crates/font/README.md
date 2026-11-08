# tidepool-font

TrueType parsing, the glyph rasterizer, the glyph atlas and text layout, for
the bundled Saira and JetBrains Mono faces. Pure; malformed fonts return
errors.

It is part of [Tidepool](https://github.com/jabbott-iii/Tidepool), where
`tidepool-gui` lays out and draws text with it, beside `gfx`; it needs only
`tidepool-core`.

Layer 1, OS behavior, formats and pure engines. It may use `core`, as its
row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Version 0.1, not published (`publish = false`): the API changes as the
roadmap needs.

Nothing yet: the crate is only its doc comment, which names its spec.

Not yet, with the roadmap task that adds it:

- `ttf` and `outline`, and the bundled font files — M2.8.
- `atlas`, `layout`, `fallback` and `bundled` — M2.5.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Usage

Nothing to use yet: `ttf` and `outline` land in M2.8.

## Documentation

- Design: [`docs/subsystems/rendering.md`](../../docs/subsystems/rendering.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0004 (the
  bundled faces).
- API reference: `cargo doc -p tidepool-font --open`

## Rules

- `unsafe` is denied by the workspace lint, and `cargo xtask unsafe-audit`
  checks.
- Path-scoped rules in `.claude/rules/`:
  [`parsers.md`](../../.claude/rules/parsers.md),
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.

## Checks

```bash
cargo test -p tidepool-font
```

`cargo xtask ci` runs the whole gate.

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
