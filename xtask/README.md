# xtask

Project automation, run as `cargo xtask <command>`: the checks behind the
pull request gate, and later goldens, fuzz smoke, benchmarks, theme contrast
and packaging. It never ships, never uploads, publishes or signs, reads no
secrets, and opens no sockets; the one program it starts is `cargo`.

Layer 6, tools (dev-only). It may use `core` and `codec`, as its row of the
dependency matrix in [`docs/01-architecture.md`](../docs/01-architecture.md)
says.

## Status

Built, with the roadmap task that built it:

- `deps`, `layers`, `unsafe-audit` and `ci`; the Claude Code hook builds and
  runs it — M0.3.
- `ucd [--check]`, which generates `tidepool-text`'s grapheme tables — M1.1.
- `headers`, which checks the license header of the source files — M0.7.
- Paths shown with their control characters escaped, and symbolic links
  reported without being resolved, in every check — M0.10.
- `workflows`, which checks the actions, pins and token permissions of the
  CI workflows, ported from Nadir — M0.8.

Not yet, with the roadmap task that adds it:

- `crates` — M0.9.
- `fuzz-smoke` — M1.6.
- `bench` — M1.9.
- `golden` — M2.4.
- `theme` — M3.2.
- `tools` — M5.1.
- `package` — M11.6, M12.4, M14.4.

Tasks and their state are in [`docs/roadmap.md`](../docs/roadmap.md).

## Design

- [`docs/04-quality.md`](../docs/04-quality.md) ("`xtask` checks")
- Decisions in [`docs/decisions.md`](../docs/decisions.md): ADR-0017 (it
  runs `cargo` itself); ADR-0018 (the Unicode data); ADR-0021 (license
  headers); ADR-0022 (the CI workflows' supply chain, and the table of
  what each workflow may use, `WORKFLOWS` in `src/workflows.rs`).

## Rules

- `unsafe` is denied by the workspace lint, and `cargo xtask unsafe-audit`
  checks.
- Path-scoped rules in `.claude/rules/`:
  [`dependencies.md`](../.claude/rules/dependencies.md) for its
  `Cargo.toml`.
- A check that reads a document reads it as the source of truth: `layers`
  parses the dependency matrix in `docs/01-architecture.md`.

## Checks

```bash
cargo test -p xtask
cargo xtask ci
```

`cargo xtask ci` runs the whole gate.

## License

Apache-2.0, like the rest of Tidepool: see [`LICENSE`](../LICENSE).
