# tidepool-lang

Language features: language sessions over LSP (rust-analyzer first), cargo
tasks and their problems, Markdown services such as link checks, and
debugging over DAP.

It is part of [Tidepool](https://github.com/jabbott-iii/Tidepool), where
`editor` shows what it finds; it speaks to language servers through
`tidepool-rpc` and starts them through `tidepool-toolchain`.

Layer 3, language and tool features. It may use `core`, `codec`, `text`,
`markdown`, `rpc`, `toolchain`, `syntax` and `workspace`, as its row of the
dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Version 0.1, not published (`publish = false`): the API changes as the
roadmap needs.

Nothing yet: besides what every library carries (ADR-0028, ADR-0036),
the crate is only its doc comment, which names its spec.

Not yet, with the roadmap task that adds it:

- sessions and diagnostics — M6.7.
- navigation and hover — M6.8.
- completion, rename, formatting, code actions and the rest — M6.3.
- cargo tasks and test discovery — M6.4.
- the Markdown outline and link checks — M6.6.
- `dap` — M13.1.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Usage

Nothing to use yet: sessions and diagnostics land in M6.7.

## Documentation

- Design: [`docs/subsystems/languages.md`](../../docs/subsystems/languages.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0005
  (Rust and Markdown first); ADR-0014 (trust, Proposed); ADR-0019
  (positions, and the lone CR).
- API reference: `cargo doc -p tidepool-lang --open`

## Rules

- `unsafe` is denied by the workspace lint, and in doc tests by the crate
  root's `#![doc(test(attr(forbid(unsafe_code))))]` (ADR-0036);
  `cargo xtask unsafe-audit` checks both.
- Path-scoped rules in `.claude/rules/`:
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.

## Checks

```bash
cargo test -p tidepool-lang
TIDEPOOL_IT_RA=1 cargo test -p tidepool-lang    # real rust-analyzer, from M6.7
```

`cargo xtask ci` runs the whole gate.

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
