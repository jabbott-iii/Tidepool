# tidepool-rpc

JSON-RPC 2.0 over `Content-Length` framing, shared by LSP, DAP and TAP, with
request IDs, cancellation, timeouts and size limits, and the typed messages
of each protocol.

It is part of [Tidepool](https://github.com/jabbott-iii/Tidepool), where
`lang` talks to language servers and debug adapters through it, `agent` to
language agents, and `testkit`'s fake LSP server answers with it; it reads
and writes JSON with `tidepool-codec`.

Layer 2, services. It may use `core` and `codec`, as its row of the
dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Version 0.1, not published (`publish = false`): the API changes as the
roadmap needs.

Nothing yet: the crate is only its doc comment, which names its spec.

Not yet, with the roadmap task that adds it:

- the transport and the LSP client's messages — M6.1.
- TAP messages — M9.3.
- DAP — M13.1.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Usage

Nothing to use yet: the transport and the LSP client's messages land in
M6.1.

## Documentation

- Design: [`docs/subsystems/languages.md`](../../docs/subsystems/languages.md),
  [`docs/subsystems/agents.md`](../../docs/subsystems/agents.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0009
  (agents speak TAP, Proposed).
- API reference: `cargo doc -p tidepool-rpc --open`

## Rules

- `unsafe` is denied by the workspace lint, and `cargo xtask unsafe-audit`
  checks.
- Path-scoped rules in `.claude/rules/`:
  [`parsers.md`](../../.claude/rules/parsers.md),
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.

## Checks

```bash
cargo test -p tidepool-rpc
```

`cargo xtask ci` runs the whole gate.

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
