# tidepool-text

The text engine every buffer is built on: a persistent rope, positions and
their conversions (UTF-16 included), extended grapheme clusters,
multi-cursor selections, canonical change sets, the undo tree, lossless
decoding of UTF-8 and UTF-16, and loading and atomic saving of text files.
Positions are byte offsets, and a bad position is an error, never a panic.

Layer 1, OS behavior, formats and pure engines. It may use `core` (its tests
use `core`'s property harness), as its row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Built, with the roadmap task that built it:

- `rope` and `pos`, with grapheme boundaries (UAX #29) from tables that
  `cargo xtask ucd` generates from `ucd/17.0.0/` — M1.1.
- `selection`, `transaction` and `history` — M1.2.
- `encoding` (UTF-8 and UTF-16 LE/BE by BOM, decoded losslessly, invalid
  input read-only) and `file` (loading with the 16 MiB large-file
  threshold, and the atomic save, which follows symbolic links) — M1.3.

All of the crate's planned modules exist.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Design

- [`docs/subsystems/text-and-formats.md`](../../docs/subsystems/text-and-formats.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0018 (the
  Unicode data); ADR-0019 (positions); ADR-0020 (edits and undo); ADR-0025
  (saving: links followed, the temporary file's mode, what a rename
  changes).

## Rules

- `unsafe` is denied by the workspace lint, and `cargo xtask unsafe-audit`
  checks.
- Path-scoped rules in `.claude/rules/`:
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.
- Never edit `src/pos/grapheme/tables.rs` by hand: `cargo xtask ucd` writes
  it.
- One platform `cfg` in the library, which ADR-0025 allows: the
  `#[cfg(unix)]` in `src/file.rs` that creates the temporary file of an
  existing file with mode 0600 and clears setuid and setgid from the mode
  it copies. Tests that check modes and links may use `cfg(unix)` too.

## Checks

```bash
cargo test -p tidepool-text
cargo xtask ucd --check
```

`cargo xtask ci` runs the whole gate.

## License

Apache-2.0, like the rest of Tidepool: see [`LICENSE`](../../LICENSE).
