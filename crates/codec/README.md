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

Not yet, with the roadmap task that adds it:

- `toml`: the TOML 1.0 parser — M1.7.
- `toml`: the document model that preserves comments and order — M1.5.
- `qoi` — M2.4.
- `inflate`, `deflate` and `png` — M10.7.
- `zip` and `xml` — M10.2.
- `plist` — M11.3.
- the YAML subset, CSV and INI: in the backlog.
- fuzz targets for every parser — M1.6.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Design

- [`docs/subsystems/text-and-formats.md`](../../docs/subsystems/text-and-formats.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0013
  (TOML for Tidepool's own files); ADR-0021 (the RFC 8259 examples in
  `tests/json/` keep the IETF Trust's notice, from M0.7).

## Rules

- `unsafe` is denied by the workspace lint, and `cargo xtask unsafe-audit`
  checks.
- Path-scoped rules in `.claude/rules/`:
  [`parsers.md`](../../.claude/rules/parsers.md),
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.
- For `json`, the module docs in `src/json.rs` are authoritative, and the
  spec summarizes them: change both together.

## Checks

```bash
cargo test -p tidepool-codec
```

`cargo xtask ci` runs the whole gate.

## License

Apache-2.0, like the rest of Tidepool: see [`LICENSE`](../../LICENSE).
