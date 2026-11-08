# xtask

Project automation, run as `cargo xtask <command>`: the checks behind the
pull request gate and the fuzz smoke, and later goldens, benchmarks, theme
contrast and packaging. It never ships, never uploads, publishes or signs,
reads no secrets, and opens no sockets; the one program it starts is
`cargo`.

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
- `fuzz-smoke [--secs N] [--target T] [--seed S] [--replay]`, which runs
  `cargo test` of each fuzz target (its table is `TARGETS` in
  `src/fuzz.rs`) with mutated inputs, reads each one's result line and
  checks it shows the seed and seconds it was given, tells a death, an
  exit without a result and a cargo failure apart, prints the replay
  command for a death or a range of inputs, notes the size and hash of a
  `.last-input` an earlier replay left, and keeps the input a replay died
  or exited without a result on as `died-<hash>` when the replay wrote it
  (not after an interrupt, and not a second copy of a saved crash);
  ported in part from Nadir — M1.6.
- `crates`, which checks that every member inherits `edition`, `version`,
  `license` and lints from the workspace and sets `publish = false`, that
  no target sets an edition of its own, that the workspace's edition is
  2024, and that each member's `README.md` starts with its package name,
  has `## Status` and `## Design`, and names only task IDs the roadmap
  has — M0.9.

Not yet, with the roadmap task that adds it:

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
  what each workflow may use, `WORKFLOWS` in `src/workflows.rs`); ADR-0023
  (the members' manifests and READMEs, which `crates` checks); ADR-0027
  (the fuzz targets live in the parser crates, and `fuzz-smoke`'s only
  child is `cargo test`).

## Rules

- `unsafe` is denied by the workspace lint, and `cargo xtask unsafe-audit`
  checks.
- Path-scoped rules in `.claude/rules/`:
  [`dependencies.md`](../.claude/rules/dependencies.md) for its
  `Cargo.toml`.
- A check that reads a document reads it as the source of truth: `layers`
  parses the dependency matrix in `docs/01-architecture.md`, and `crates`
  takes the task IDs a README may name from `docs/roadmap.md`.
- It links only `core` and `codec`: the parsers that `fuzz-smoke` fuzzes
  run in their own crates' test processes, never in `xtask`. A new target
  adds its row to `TARGETS` in `src/fuzz.rs`, which must agree with
  `fuzz/corpus/` and `fuzz/crashes/`; none of `fuzz/`, `fuzz/corpus/` and
  `fuzz/crashes/` may be a link.

## Checks

```bash
cargo test -p xtask
cargo xtask ci
cargo xtask fuzz-smoke --secs 60
```

`cargo xtask ci` runs the whole gate. It does not run `fuzz-smoke`, which
CI runs in a step of its own on Linux.

## License

Apache-2.0, like the rest of Tidepool: see [`LICENSE`](../LICENSE).
