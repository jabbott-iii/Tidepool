# tidepool-agent

The host for language agents speaking the Tidepool Agent Protocol:
manifests, supervision, consent, the context Tidepool assembles for them
(with the secret deny-list), offline predictive text, and the merging of
suggestions.

Layer 3, language and tool features. It may use `core`, `codec`, `text`,
`rpc`, `toolchain` and `workspace`, as its row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Nothing yet: the crate is only its doc comment, which names its spec.

Not yet, with the roadmap task that adds it:

- `manifest`, `host`, `tap`, `context` and `consent` — M9.3.
- `predict` and `merge` — M9.4.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Design

- [`docs/subsystems/agents.md`](../../docs/subsystems/agents.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0009
  (agents are processes speaking TAP, Proposed); ADR-0014 (trust, Proposed).

## Rules

- `unsafe` is denied by the workspace lint, and `cargo xtask unsafe-audit`
  checks.
- Path-scoped rules in `.claude/rules/`:
  [`ai-context.md`](../../.claude/rules/ai-context.md),
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.

## Checks

```bash
cargo test -p tidepool-agent
```

`cargo xtask ci` runs the whole gate.

## License

Apache-2.0, like the rest of Tidepool: see [`LICENSE`](../../LICENSE).
