# tidepool-workspace

The project model under the editor: workspace folders and ignore rules,
workspace trust, layered settings, the file tree, project search, open
buffers and session state.

Layer 2, services. It may use `core`, `platform`, `codec`, `text` and
`regex`, as its row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Nothing yet: the crate is only its doc comment, which names its spec.

Not yet, with the roadmap task that adds it:

- `folder`, `trust` and `settings` — M4.7.
- `buffers`, `session`, and the file tree model in `files` — M4.3.
- `search`, and the file finder's index in `files` — M4.4.
- `watch` — M4.5.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Design

- [`docs/subsystems/editor.md`](../../docs/subsystems/editor.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0014
  (untrusted until trusted, Proposed); ADR-0013 (settings are TOML).

## Rules

- `unsafe` is denied by the workspace lint, and `cargo xtask unsafe-audit`
  checks.
- Path-scoped rules in `.claude/rules/`:
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.

## Checks

```bash
cargo test -p tidepool-workspace
```

`cargo xtask ci` runs the whole gate.

## License

Apache-2.0, like the rest of Tidepool: see [`LICENSE`](../../LICENSE).
