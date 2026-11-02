# tidepool-editor

Where Tidepool's features live: the editor view, named commands and the
modal keymap engine (vim mode included), and every panel. It renders through
`ui` and never knows which frontend draws it.

Layer 4, views and frontends. It may use `core`, `codec`, `text`, `regex`,
`markdown`, `syntax`, `term`, `workspace`, `ui`, `lang`, `vcs`, `agent` and
`mobile`, as its row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Nothing yet: the crate is only its doc comment, which names its spec.

Not yet, with the roadmap task that adds it:

- the editor view — M4.1.
- commands and the keymap engine — M4.6.
- editing commands, multi-cursor, undo and clipboard — M4.2.
- explorer, tabs, splits, the trust prompt — M4.3.
- search — M4.4.
- the terminal panel — M5.4.
- the Toolchains panel — M5.6.
- highlighting — M6.2.
- language features — M6.7, M6.8, M6.3.
- the Markdown preview — M6.6.
- vim mode — M7.2, M7.3, M7.4.
- git panels — M8.2, M8.3.
- AI completion, chat and proposals — M9.4, M9.5.
- mobile panels — M10.6, M11.6.
- image and hex viewers — M13.4.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Design

- [`docs/subsystems/editor.md`](../../docs/subsystems/editor.md)
- [`docs/subsystems/vim-mode.md`](../../docs/subsystems/vim-mode.md)
- [`docs/subsystems/ui-and-theme.md`](../../docs/subsystems/ui-and-theme.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0003 (GUI
  first, features in `editor`); ADR-0006 (vim mode, Proposed).

## Rules

- `unsafe` is denied by the workspace lint, and `cargo xtask unsafe-audit`
  checks.
- Path-scoped rules in `.claude/rules/`:
  [`ui-style.md`](../../.claude/rules/ui-style.md),
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.

## Checks

```bash
cargo test -p tidepool-editor
```

`cargo xtask ci` runs the whole gate.

## License

Apache-2.0, like the rest of Tidepool: see [`LICENSE`](../../LICENSE).
