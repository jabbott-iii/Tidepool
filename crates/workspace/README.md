# tidepool-workspace

The project model under the editor: workspace folders and ignore rules,
workspace trust, layered settings, the file tree, project search, open
buffers and session state.

It is part of [Tidepool](https://github.com/jabbott-iii/Tidepool), where
`editor`, `lang` and `agent` use it; its buffers are `tidepool-text`
documents, and its settings are TOML read with `tidepool-codec`.

Layer 2, services. It may use `core`, `platform`, `codec`, `text` and
`regex`, as its row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Version 0.1, not published (`publish = false`): the API changes as the
roadmap needs.

Nothing yet: besides what every library carries (ADR-0028, ADR-0036),
the crate is only its doc comment, which names its spec.

Not yet, with the roadmap task that adds it:

- `folder`, `trust` and `settings` — M4.7.
- `buffers`, `session`, and the file tree model in `files` — M4.3.
- `search`, and the file finder's index in `files` — M4.4.
- `watch` — M4.5.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Usage

Nothing to use yet: `folder`, `trust` and `settings` land in M4.7.

## Documentation

- Design: [`docs/subsystems/editor.md`](../../docs/subsystems/editor.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0014
  (untrusted until trusted, Proposed); ADR-0013 (settings are TOML).
- API reference: `cargo doc -p tidepool-workspace --open`

## Rules

- `unsafe` is denied by the workspace lint, and in doc tests by the crate
  root's `#![doc(test(attr(forbid(unsafe_code))))]` (ADR-0036);
  `cargo xtask unsafe-audit` checks both.
- Path-scoped rules in `.claude/rules/`:
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.

## Checks

```bash
cargo test -p tidepool-workspace
```

`cargo xtask ci` runs the whole gate.

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
