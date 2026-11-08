# tidepool-gfx

The CPU rasterizer for the GUI: a premultiplied framebuffer, the primitives
the jabbott.dev look needs (cut corners, hairlines, bracket corners),
clipping, glyph blits, the cached backdrop and damage tracking. Pure and
deterministic, so golden frames are shared across OSes.

It is part of [Tidepool](https://github.com/jabbott-iii/Tidepool), where
`tidepool-gui` paints `ui`'s draw lists with it, beside `font`; it needs
only `tidepool-core`, and no OS.

Layer 1, OS behavior, formats and pure engines. It may use `core`, as its
row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Version 0.1, not published (`publish = false`): the API changes as the
roadmap needs.

Nothing yet: the crate is only its doc comment, which names its spec.

Not yet, with the roadmap task that adds it:

- the rasterizer, blending, clipping, damage and backdrop — M2.3.
- golden frames for every primitive — M2.4.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Usage

Nothing to use yet: the rasterizer lands in M2.3.

## Documentation

- Design: [`docs/subsystems/rendering.md`](../../docs/subsystems/rendering.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0011 (CPU
  rendering, Proposed); ADR-0004 (the jabbott.dev look).
- API reference: `cargo doc -p tidepool-gfx --open`

## Rules

- `unsafe` is denied by the workspace lint, and `cargo xtask unsafe-audit`
  checks.
- Path-scoped rules in `.claude/rules/`:
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.

## Checks

```bash
cargo test -p tidepool-gfx
cargo xtask golden    # from M2.4
```

`cargo xtask ci` runs the whole gate.

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
