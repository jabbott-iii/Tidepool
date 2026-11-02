# tidepool-vcs

git features over the managed `git` command line: status, diff hunks for the
gutter, blame and history. Writes happen only on an explicit user action.

Layer 3, language and tool features. It may use `core`, `text` and
`toolchain`, as its row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Nothing yet: the crate is only its doc comment, which names its spec.

Not yet, with the roadmap task that adds it:

- status, diff hunks and the gutter — M8.1.
- blame and history — M8.3.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Design

- [`docs/subsystems/toolchains.md`](../../docs/subsystems/toolchains.md)
  ("`tidepool-vcs`")
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0002 (git
  is a managed tool).

## Rules

- `unsafe` is denied by the workspace lint, and `cargo xtask unsafe-audit`
  checks.
- Path-scoped rules in `.claude/rules/`:
  [`parsers.md`](../../.claude/rules/parsers.md),
  [`toolchains-and-secrets.md`](../../.claude/rules/toolchains-and-secrets.md),
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.

## Checks

```bash
cargo test -p tidepool-vcs
TIDEPOOL_IT_GIT=1 cargo test -p tidepool-vcs    # from M8.1
```

`cargo xtask ci` runs the whole gate.

## License

Apache-2.0, like the rest of Tidepool: see [`LICENSE`](../../LICENSE).
