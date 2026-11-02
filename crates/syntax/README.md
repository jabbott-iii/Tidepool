# tidepool-syntax

Incremental syntax highlighting: hand-written lexers for Rust, Markdown and
TOML, the language-pack format, bracket matching and indentation rules.

Layer 2, services. It may use `core`, `codec`, `text` and `regex`, as its
row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Nothing yet: the crate is only its doc comment, which names its spec.

Not yet, with the roadmap task that adds it:

- the Rust, Markdown and TOML lexers and the pack format — M6.2.
- declarative pack lexers — M13.2.
- packs for common languages — M13.3.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Design

- [`docs/subsystems/languages.md`](../../docs/subsystems/languages.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0005
  (Rust and Markdown first); ADR-0013 (packs are TOML).

## Rules

- `unsafe` is denied by the workspace lint, and `cargo xtask unsafe-audit`
  checks.
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

Apache-2.0, like the rest of Tidepool: see [`LICENSE`](../../LICENSE).
