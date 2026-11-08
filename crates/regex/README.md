# tidepool-regex

A regular-expression engine that runs in linear time for every pattern (a
Pike VM). No backreferences or lookaround.

It is part of [Tidepool](https://github.com/jabbott-iii/Tidepool), where
search (`workspace` and `editor`), vim mode (`editor`) and the language
packs' lexers (`syntax`) use it; it needs only `tidepool-core`.

Layer 1, OS behavior, formats and pure engines. It may use `core`, as its
row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Version 0.1, not published (`publish = false`): the API changes as the
roadmap needs.

Nothing yet: the crate is only its doc comment, which names its spec.

Not yet, with the roadmap task that adds it:

- the engine, Unicode classes and case folding — M4.8.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Usage

Nothing to use yet: the engine lands in M4.8.

## Documentation

- Design:
  [`docs/subsystems/text-and-formats.md`](../../docs/subsystems/text-and-formats.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0018
  (Unicode data; M4.8 asks to extend it).
- API reference: `cargo doc -p tidepool-regex --open`

## Rules

- `unsafe` is denied by the workspace lint, and `cargo xtask unsafe-audit`
  checks.
- Path-scoped rules in `.claude/rules/`:
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.

## Checks

```bash
cargo test -p tidepool-regex
```

`cargo xtask ci` runs the whole gate.

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
