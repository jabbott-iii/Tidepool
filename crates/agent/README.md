# tidepool-agent

The host for language agents speaking the Tidepool Agent Protocol:
manifests, supervision, consent, the context Tidepool assembles for them
(with the secret deny-list), offline predictive text, and the merging of
suggestions.

It is part of [Tidepool](https://github.com/jabbott-iii/Tidepool), where
`editor`'s AI completion, chat and proposals use it; it speaks TAP through
`tidepool-rpc` and starts agents through `tidepool-toolchain`.

Layer 3, language and tool features. It may use `core`, `codec`, `text`,
`rpc`, `toolchain` and `workspace`, as its row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Version 0.1, not published (`publish = false`): the API changes as the
roadmap needs.

Nothing yet: besides what every library carries (ADR-0028, ADR-0036),
the crate is only its doc comment, which names its spec.

Not yet, with the roadmap task that adds it:

- `manifest`, `host`, `tap`, `context` and `consent` — M9.3.
- `predict` and `merge` — M9.4.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Usage

Nothing to use yet: `manifest`, `host`, `tap`, `context` and `consent` land
in M9.3.

## Documentation

- Design: [`docs/subsystems/agents.md`](../../docs/subsystems/agents.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0009
  (agents are processes speaking TAP, Proposed); ADR-0014 (trust, Proposed).
- API reference: `cargo doc -p tidepool-agent --open`

## Rules

- `unsafe` is denied by the workspace lint, and in doc tests by the crate
  root's `#![doc(test(attr(forbid(unsafe_code))))]` (ADR-0036);
  `cargo xtask unsafe-audit` checks both.
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

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
