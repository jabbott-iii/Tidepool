# tidepool-testkit

Dev-only test support: a fake LSP server that replays transcripts, fake
binaries named like the real tools, and protocol transcripts. It never
ships.

Layer 6, tools (dev-only). It may use `core`, `codec` and `rpc`; another
crate may use it only as a dev-dependency, as its row of the dependency
matrix in [`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Nothing yet: the crate is only its doc comment, which names its spec.

Not yet, with the roadmap task that adds it:

- fake tools — M5.1.
- the fake LSP server — M6.1.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Design

- [`docs/04-quality.md`](../../docs/04-quality.md) ("Fakes")
- Decisions: none yet; M5.1 asks which crates may use it.

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

Apache-2.0, like the rest of Tidepool: see [`LICENSE`](../../LICENSE).
