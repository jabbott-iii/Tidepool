# tidepool-codec

The data formats Tidepool reads and writes, each treating its input as
untrusted: sizes checked against `Limits` before allocating, nesting
bounded, errors that say where and what was expected, and no panic on any
input.

Layer 1, OS behavior, formats and pure engines. It may use `core`, as its
row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Built, with the roadmap task that built it:

- `json`: JSON (RFC 8259) and JSONC, with compact and pretty writers, and
  `Limits` and `CodecError` — M1.4.
- `toml`: the TOML 1.1 parser, into a value tree whose keys and values carry
  their spans, checked against toml-test's TOML 1.1 cases — M1.7.
- Fuzz targets `json` and `toml` in `tests/fuzz.rs`, with small `Limits`
  and seeds at each limit, checking error places, JSON's dialects and
  write-back, and TOML's spans; seeded from `fuzz/corpus/<target>/` and,
  read in place, `tests/json/` and `tests/toml-test/` — M1.6.

Not yet, with the roadmap task that adds it:

- `toml`: the document model that preserves comments and order, and a
  writer — M1.5.
- `qoi` — M2.4.
- `inflate`, `deflate` and `png` — M10.7.
- `zip` and `xml` — M10.2.
- `plist` — M11.3.
- the YAML subset, CSV and INI: in the backlog.
- a fuzz target for each parser still to come, with that parser's task.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Design

- [`docs/subsystems/text-and-formats.md`](../../docs/subsystems/text-and-formats.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0013
  (TOML for Tidepool's own files); ADR-0021 (the RFC 8259 examples in
  `tests/json/` keep the IETF Trust's notice, in `tests/json.rs`); ADR-0026
  (`toml` parses TOML 1.1, and its compliance cases are toml-test's,
  vendored in `tests/toml-test/`); ADR-0027 (the fuzz targets, in
  `tests/fuzz.rs`).
- [`docs/04-quality.md`](../../docs/04-quality.md) ("Fuzz smoke"): what
  each target runs and checks.

## Rules

- `unsafe` is denied by the workspace lint, and `cargo xtask unsafe-audit`
  checks.
- Path-scoped rules in `.claude/rules/`:
  [`parsers.md`](../../.claude/rules/parsers.md),
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.
- For `json` and `toml`, the module docs in `src/json.rs` and
  `src/toml.rs` are authoritative, and the spec summarizes them: change both
  together.
- `tests/toml-test/` is third-party data (MIT, toml-test v2.2.0), copied
  byte for byte with its notice in `tests/toml-test/NOTICE.md`. Never edit,
  rename or add files there; a newer release replaces the directory and its
  notice in one change (ADR-0026). `tests/toml.rs` checks that the list and
  the files agree. The `toml` fuzz target reads them where they are.
- `tests/json/` and `tests/toml-test/` are `-text` in `.gitattributes`:
  the fuzz targets read them as seeds, which must be the same bytes on
  every OS.
- A new parser adds its fuzz target to `tests/fuzz.rs`, its seeds to
  `fuzz/corpus/<target>/`, and its row to `fuzz-smoke`'s table
  (`.claude/rules/parsers.md`).

## Checks

```bash
cargo test -p tidepool-codec
cargo xtask fuzz-smoke --target json
cargo xtask fuzz-smoke --target toml
```

`cargo test` replays the fuzz targets' seeds and saved crashes;
`fuzz-smoke` also mutates them, for 60 s a target unless `--secs` says
otherwise.

`cargo xtask ci` runs the whole gate.

## License

Apache-2.0, like the rest of Tidepool: see [`LICENSE`](../../LICENSE).
