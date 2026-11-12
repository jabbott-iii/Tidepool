# tidepool-markdown

A CommonMark and GFM parser producing a document tree with source ranges,
for the preview, outline, folding and link checks.

It is part of [Tidepool](https://github.com/jabbott-iii/Tidepool), where
`lang`'s Markdown services and `editor`'s preview use it; it needs only
`tidepool-core`.

Layer 1, OS behavior, formats and pure engines. It may use `core`, as its
row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Version 0.1, not published (`publish = false`): the API changes as the
roadmap needs.

Nothing yet: besides what every library carries (ADR-0028, ADR-0036),
the crate is only its doc comment, which names its spec.

Not yet, with the roadmap task that adds it:

- the parser — M6.5.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Usage

Nothing to use yet: the parser lands in M6.5.

## Documentation

- Design: [`docs/subsystems/languages.md`](../../docs/subsystems/languages.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0005
  (Rust and Markdown first).
- API reference: `cargo doc -p tidepool-markdown --open`

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
cargo test -p tidepool-markdown
```

`cargo xtask ci` runs the whole gate.

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
