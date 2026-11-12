# tidepool-syntax

Incremental syntax highlighting: hand-written lexers for Rust, Markdown and
TOML, the language-pack format, bracket matching and indentation rules.

It is part of [Tidepool](https://github.com/jabbott-iii/Tidepool), where
`lang` and `editor` use it; it lexes `tidepool-text` buffers, reads the
packs' TOML with `tidepool-codec`, and matches with `tidepool-regex`.

Layer 2, services. It may use `core`, `codec`, `text` and `regex`, as its
row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Version 0.1, not published (`publish = false`): the API changes as the
roadmap needs.

Nothing yet: besides what every library carries (ADR-0028, ADR-0036),
the crate is only its doc comment, which names its spec.

Not yet, with the roadmap task that adds it:

- the Rust, Markdown and TOML lexers and the pack format — M6.2.
- declarative pack lexers — M13.2.
- packs for common languages — M13.3.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Usage

Nothing to use yet: the Rust, Markdown and TOML lexers and the pack format
land in M6.2.

## Documentation

- Design: [`docs/subsystems/languages.md`](../../docs/subsystems/languages.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0005
  (Rust and Markdown first); ADR-0013 (packs are TOML).
- API reference: `cargo doc -p tidepool-syntax --open`

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
cargo test -p tidepool-syntax
```

`cargo xtask ci` runs the whole gate.

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
