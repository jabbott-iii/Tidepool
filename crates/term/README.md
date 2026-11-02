# tidepool-term

The integrated terminal's VT parser (ECMA-48 and an xterm subset) and its
grid, with scrollback, reflow and wide characters. Full-screen programs such
as `vim`, `less` and `htop` must run correctly in it.

Layer 2, services. It may use `core` and `platform`, as its row of the
dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Nothing yet: the crate is only its doc comment, which names its spec.

Not yet, with the roadmap task that adds it:

- the VT parser and grid — M5.8.
- the terminal panel, in `editor` — M5.4.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Design

- [`docs/subsystems/toolchains.md`](../../docs/subsystems/toolchains.md)
  ("`tidepool-term`")
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0018
  (Unicode data; M5.8 asks to extend it for display width).

## Rules

- `unsafe` is denied by the workspace lint, and `cargo xtask unsafe-audit`
  checks.
- Path-scoped rules in `.claude/rules/`:
  [`parsers.md`](../../.claude/rules/parsers.md),
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.

## Checks

```bash
cargo test -p tidepool-term
```

`cargo xtask ci` runs the whole gate.

## License

Apache-2.0, like the rest of Tidepool: see [`LICENSE`](../../LICENSE).
