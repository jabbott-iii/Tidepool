# Notice for `fuzz/`

The seeds in `fuzz/corpus/` are hand-made for Tidepool, under its license
(`LICENSE`, `NOTICE`).

The inputs saved in `fuzz/crashes/` are made by mutating seeds, and some
seeds are read in place from third-party test data (ADR-0027,
`docs/04-quality.md` → "Fuzz smoke"). A saved input may therefore be
derived from:

- toml-test v2.2.0's cases (MIT), the seeds of the `toml` target: see
  `crates/codec/tests/toml-test/NOTICE.md` for their source, copyright and
  license;
- the examples of RFC 8259 (the IETF Trust's), among the seeds of the
  `json` target: see the legend in `crates/codec/tests/json.rs`, which
  names the files and gives their notice.

Those notices apply to such an input as they do to the data it came from.
