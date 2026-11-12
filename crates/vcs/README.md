# tidepool-vcs

git features over the managed `git` command line: status, diff hunks for the
gutter, blame and history. Writes happen only on an explicit user action.

It is part of [Tidepool](https://github.com/jabbott-iii/Tidepool), where
`editor`'s gutter and git panels use it; it runs `git` through
`tidepool-toolchain`.

Layer 3, language and tool features. It may use `core`, `text` and
`toolchain`, as its row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Version 0.1, not published (`publish = false`): the API changes as the
roadmap needs.

Nothing yet: besides what every library carries (ADR-0028, ADR-0036),
the crate is only its doc comment, which names its spec.

Not yet, with the roadmap task that adds it:

- status, diff hunks and the gutter — M8.1.
- blame and history — M8.3.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Usage

Nothing to use yet: status and diff hunks land in M8.1.

## Documentation

- Design:
  [`docs/subsystems/toolchains.md`](../../docs/subsystems/toolchains.md)
  ("`tidepool-vcs`")
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0002 (git
  is a managed tool).
- API reference: `cargo doc -p tidepool-vcs --open`

## Rules

- `unsafe` is denied by the workspace lint, and in doc tests by the crate
  root's `#![doc(test(attr(forbid(unsafe_code))))]` (ADR-0036);
  `cargo xtask unsafe-audit` checks both.
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

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
