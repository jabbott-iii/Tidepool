# tidepool-testkit

Dev-only test support: a fake LSP server that replays transcripts, fake
binaries named like the real tools, and protocol transcripts. It never
ships.

It is part of [Tidepool](https://github.com/jabbott-iii/Tidepool), where
the test plan has other crates' tests use it as a dev-dependency (today the
matrix lets only `tidepool-bench` use it, and M5.1 decides the rest); its
fake server speaks `tidepool-rpc`'s messages.

Layer 6, tools (dev-only). It may use `core`, `codec` and `rpc`; another
crate may use it only as a dev-dependency, as its row of the dependency
matrix in [`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Version 0.1, not published (`publish = false`): the API changes as the
roadmap needs.

Nothing yet: the crate is only its doc comment, which names its spec.

Not yet, with the roadmap task that adds it:

- fake tools — M5.1.
- the fake LSP server — M6.1.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Usage

Nothing to use yet: fake tools land in M5.1.

## Documentation

- Design: [`docs/04-quality.md`](../../docs/04-quality.md) ("Fakes")
- Decisions: none yet; M5.1 asks which crates may use it.
- API reference: `cargo doc -p tidepool-testkit --open`

## Rules

- `unsafe` is denied by the workspace lint, and `cargo xtask unsafe-audit`
  checks.
- Path-scoped rules in `.claude/rules/`:
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.

## Checks

```bash
cargo test -p tidepool-testkit
```

`cargo xtask ci` runs the whole gate.

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
