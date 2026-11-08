---
paths:
  - "crates/codec/**"
  - "crates/font/**"
  - "crates/markdown/**"
  - "crates/syntax/**"
  - "crates/rpc/**"
  - "crates/term/**"
  - "crates/vcs/**"
  - "fuzz/**"
---

# Rules for parsers, decoders and protocols

Every byte we read may be malformed or hostile: files a user opens, archives
(APK/AAB/IPA), fonts, images, terminal output, and messages from LSP
servers, agents and tools.

- **Never panic on input.**
  - No indexing that can go out of bounds on input-derived offsets.
  - No `unwrap`/`expect` on input-derived values.
  - No unchecked arithmetic on input lengths: `checked_add`/`checked_mul`,
    error on overflow.
- **Check before allocating.** Compare every size against `Limits`, and the
  claimed size against the remaining input, before `Vec::with_capacity` or
  `resize`. Cap decompression output and expansion ratio (zip bombs).
- **Bound recursion** (JSON, TOML tables, XML, Markdown nesting) by an
  explicit depth limit.
- **Archives:** reject `..`, absolute paths, drive letters and symlinks on
  extraction.
- **XML:** no DTDs, no external entities, no entity expansion.
- **Errors carry context:** the format, the byte offset (or line and
  column), and what was expected.
- **Every new parser:**
  1. has a fuzz target (ADR-0027): a test named after the target in its
     crate's `tests/fuzz.rs`, built on `tidepool_core::testing::fuzz`,
     running the parser with small `Limits`; seeds in
     `fuzz/corpus/<target>/`; and a row in `fuzz-smoke`'s `TARGETS`
     (`xtask/src/fuzz.rs`). Run it with
     `cargo xtask fuzz-smoke --target <target>` (`docs/04-quality.md` →
     "Fuzz smoke")
  2. has hand-made test vectors committed beside its tests
  3. documents its supported subset in its subsystem doc
- **Formats Tidepool writes** (settings, keymaps, themes, packs,
  descriptors, session files) carry a version. A change bumps it and needs a
  migration or a clear "not supported" error, plus a fixture test.
