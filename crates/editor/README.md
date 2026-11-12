# tidepool-editor

Where Tidepool's features live: the editor view, named commands and the
modal keymap engine (vim mode included), and every panel. It renders through
`ui` and never knows which frontend draws it.

It is part of [Tidepool](https://github.com/jabbott-iii/Tidepool), where
the `tidepool` binary connects it to the frontends; it uses the services and
features below it, and never `gui` or `tui`.

Layer 4, views and frontends. It may use `core`, `codec`, `text`, `regex`,
`markdown`, `syntax`, `term`, `workspace`, `ui`, `lang`, `vcs`, `agent` and
`mobile`, as its row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Version 0.1, not published (`publish = false`): the API changes as the
roadmap needs.

Nothing yet: besides what every library carries (ADR-0028, ADR-0036),
the crate is only its doc comment, which names its spec.

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

## Usage

Nothing to use yet: the editor view lands in M4.1.

## Documentation

- Design: [`docs/subsystems/editor.md`](../../docs/subsystems/editor.md),
  [`docs/subsystems/vim-mode.md`](../../docs/subsystems/vim-mode.md),
  [`docs/subsystems/ui-and-theme.md`](../../docs/subsystems/ui-and-theme.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0003 (GUI
  first, features in `editor`); ADR-0006 (vim mode, Proposed).
- API reference: `cargo doc -p tidepool-editor --open`

## Rules

- `unsafe` is denied by the workspace lint, and in doc tests by the crate
  root's `#![doc(test(attr(forbid(unsafe_code))))]` (ADR-0036);
  `cargo xtask unsafe-audit` checks both.
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

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
