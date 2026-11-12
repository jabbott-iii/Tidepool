# tidepool-term

The integrated terminal's VT parser (ECMA-48 and an xterm subset) and its
grid, with scrollback, reflow and wide characters. Full-screen programs such
as `vim`, `less` and `htop` must run correctly in it.

It is part of [Tidepool](https://github.com/jabbott-iii/Tidepool), where
`editor`'s terminal panel shows its grid; it reads a PTY that
`tidepool-platform` opens.

Layer 2, services. It may use `core` and `platform`, as its row of the
dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Version 0.1, not published (`publish = false`): the API changes as the
roadmap needs.

Nothing yet: besides what every library carries (ADR-0028, ADR-0036),
the crate is only its doc comment, which names its spec.

Not yet, with the roadmap task that adds it:

- the VT parser and grid — M5.8.
- the terminal panel, in `editor` — M5.4.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Usage

Nothing to use yet: the VT parser and grid land in M5.8.

## Documentation

- Design:
  [`docs/subsystems/toolchains.md`](../../docs/subsystems/toolchains.md)
  ("`tidepool-term`")
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0018
  (Unicode data; M5.8 asks to extend it for display width).
- API reference: `cargo doc -p tidepool-term --open`

## Rules

- `unsafe` is denied by the workspace lint, and in doc tests by the crate
  root's `#![doc(test(attr(forbid(unsafe_code))))]` (ADR-0036);
  `cargo xtask unsafe-audit` checks both.
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

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
