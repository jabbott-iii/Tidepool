# tidepool-gui

The GUI frontend: it opens a window, keeps a framebuffer at device
resolution, and rasterizes the damaged parts of each `ui` draw list with
`gfx` and `font`, in the full jabbott.dev look. It holds no feature logic.

It is part of [Tidepool](https://github.com/jabbott-iii/Tidepool), where
the `tidepool` binary connects it to `editor`, which it never depends on; it
opens its window through `tidepool-platform`.

Layer 4, views and frontends. It may use `core`, `platform`, `codec`, `gfx`,
`font` and `ui`, as its row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Version 0.1, not published (`publish = false`): the API changes as the
roadmap needs.

Nothing yet: the crate is only its doc comment, which names its spec.

Not yet, with the roadmap task that adds it:

- the first frame — M2.6.
- the draw-list painter and ui goldens — M3.5.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Usage

Nothing to use yet: the first frame lands in M2.6.

## Documentation

- Design: [`docs/subsystems/frontends.md`](../../docs/subsystems/frontends.md),
  [`docs/subsystems/rendering.md`](../../docs/subsystems/rendering.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0003 (GUI
  first); ADR-0011 (CPU rendering, Proposed).
- API reference: `cargo doc -p tidepool-gui --open`

## Rules

- `unsafe` is denied by the workspace lint, and `cargo xtask unsafe-audit`
  checks.
- Path-scoped rules in `.claude/rules/`:
  [`ui-style.md`](../../.claude/rules/ui-style.md),
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.

## Checks

```bash
cargo test -p tidepool-gui
```

`cargo xtask ci` runs the whole gate.

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
