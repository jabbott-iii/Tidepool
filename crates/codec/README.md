# tidepool-codec

The data formats Tidepool reads and writes, each treating its input as
untrusted: sizes checked against `Limits` before allocating, nesting
bounded, errors that say where and what was expected, and no panic on any
input.

It is part of [Tidepool](https://github.com/jabbott-iii/Tidepool), an IDE
for Linux, macOS and Windows, written in Rust with no third-party crates.
Its library uses no other crate (its tests use `core`'s harness), and today
`xtask` reads `cargo metadata` with its `json` and the members' manifests
with its `toml`, and `tidepool-bench` reads and writes its baseline with its
`json`; `rpc`, `toolchain`, `syntax`, `workspace` and every crate
above them but `vcs` and `tui` may use it, as their rows of the matrix
allow.

Layer 1, OS behavior, formats and pure engines. It may use `core`, as its
row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Version 0.1, not published (`publish = false`): the API changes as the
roadmap needs.

Built, with the roadmap task that built it:

- `json`: JSON (RFC 8259) and JSONC, with compact and pretty writers, and
  `Limits` and `CodecError` — M1.4.
- `toml`: the TOML 1.1 parser, into a value tree whose keys and values carry
  their spans, checked against toml-test's TOML 1.1 cases — M1.7.
- `toml::Document`: the document model that edits a TOML document and keeps
  the rest of its text byte for byte, comments and order included. It
  replaces and adds values (making any missing tables), writes what it
  creates as TOML 1.0, and checks every edit by parsing the new text
  before keeping it — M1.5.
- Fuzz targets `json` and `toml` in `tests/fuzz.rs`, with small `Limits`
  and seeds at each limit, checking error places, JSON's dialects and
  write-back, and TOML's spans and document edits; seeded from
  `fuzz/corpus/<target>/` and, read in place, `tests/json/` and
  `tests/toml-test/` — M1.6, M1.5.

Not yet, with the roadmap task that adds it:

- `qoi` — M2.4.
- `inflate`, `deflate` and `png` — M10.7.
- `zip` and `xml` — M10.2.
- `plist` — M11.3.
- the YAML subset, CSV and INI: in the backlog.
- a fuzz target for each parser still to come, with that parser's task.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Usage

A workspace crate whose row of the matrix allows it depends on it by path:

```toml
[dependencies]
tidepool-codec = { path = "../codec" }
```

Read JSONC, JSON with comments and trailing commas, which the caller asks
for; strict RFC 8259 is the default. A malformed document is an error with
its line, column and byte offset, and what was expected there:

```rust
use tidepool_codec::json::{self, Dialect, Value};
use tidepool_codec::{CodecError, Limits};

fn main() -> Result<(), CodecError> {
    let text = r#"{
        // Editor settings.
        "theme": "tidepool-dark",
        "tab_size": 4,
        "rulers": [80, 100,],
    }"#;
    let settings = json::parse(text, Dialect::Jsonc, Limits::DEFAULT)?;
    assert_eq!(settings.get("theme").and_then(Value::as_str), Some("tidepool-dark"));
    assert_eq!(settings.get("tab_size").and_then(Value::as_u64), Some(4));

    let error = json::parse(text, Dialect::Json, Limits::DEFAULT)
        .expect_err("a comment is not strict JSON");
    assert!(error.to_string().starts_with("JSON error at line 2, column 9"));
    Ok(())
}
```

Write a value as compact or pretty JSON. Members keep their order, and the
output reads back as the same value:

```rust
use tidepool_codec::json::{self, Dialect, Value};
use tidepool_codec::{CodecError, Limits};

fn main() -> Result<(), CodecError> {
    let value = Value::Object(vec![
        ("name".to_string(), Value::from("tidepool")),
        ("rulers".to_string(), Value::from(vec![Value::from(80u64), Value::from(100u64)])),
    ]);
    assert_eq!(json::to_string_compact(&value), r#"{"name":"tidepool","rulers":[80,100]}"#);

    let pretty = json::to_string_pretty(&value);
    assert_eq!(pretty, "{\n  \"name\": \"tidepool\",\n  \"rulers\": [\n    80,\n    100\n  ]\n}");
    assert_eq!(json::parse(&pretty, Dialect::Json, Limits::DEFAULT)?, value);
    Ok(())
}
```

Read TOML 1.1. Every key and value carries the span of its text, so a
caller can point at the place a setting came from, and every rule on
defining keys and tables is enforced:

```rust
use tidepool_codec::toml::{self, Value};
use tidepool_codec::{CodecError, Limits};

fn main() -> Result<(), CodecError> {
    let text = "[package]\nname = \"tidepool\"\n\n[lints.clippy]\nunwrap_used = \"deny\"\n";
    let root = toml::parse(text, Limits::DEFAULT)?;

    let name = root.get("package").and_then(|package| package.get("name"));
    assert_eq!(name.and_then(Value::as_str), Some("tidepool"));
    assert_eq!(name.map(|name| &text[name.span.range()]), Some("\"tidepool\""));

    let lint = root
        .get("lints")
        .and_then(|lints| lints.get("clippy"))
        .and_then(|clippy| clippy.get("unwrap_used"));
    assert_eq!(lint.and_then(Value::as_str), Some("deny"));

    let error = toml::parse("name = 1\nname = 2\n", Limits::DEFAULT)
        .expect_err("a key defined twice");
    assert!(error.to_string().starts_with("TOML error at line 2, column 1"));
    Ok(())
}
```

Edit a TOML document and write it back. A `Document` changes only the
bytes of each value it sets, or of each entry or table it adds, so comments,
order and layout elsewhere stay as they were, and what it creates is TOML
1.0. A path follows the structure, not the spelling: `["editor",
"tab-size"]` names `tab-size` under `[editor]`, in `editor.tab-size = 4`
and in `editor = { tab-size = 4 }` alike:

```rust
use tidepool_codec::toml::{Document, ValueKind};
use tidepool_codec::{CodecError, Limits};

fn main() -> Result<(), CodecError> {
    // An edit briefly holds two trees: read settings with a lower limit.
    let limits = Limits { max_input_bytes: 1024 * 1024, ..Limits::DEFAULT };
    let mut doc = Document::parse("# Mine\n[editor]\nfont-size = 14 # big\n", limits)?;
    doc.set(&["editor".into(), "tab-size".into()], &ValueKind::Integer(4))?;
    doc.set(&["editor".into(), "font-size".into()], &ValueKind::Integer(16))?;
    doc.set(&["theme".into(), "name".into()], &ValueKind::String("tidepool-dark".into()))?;
    assert_eq!(
        doc.as_str(),
        "# Mine\n[editor]\nfont-size = 16 # big\ntab-size = 4\n\n[theme]\nname = \"tidepool-dark\"\n"
    );

    // A table defined by a header is not replaced whole: set its entries.
    let error = doc
        .set(&["editor".into()], &ValueKind::Integer(1))
        .expect_err("a header's table");
    assert!(error.to_string().starts_with("TOML edit refused at step 0"));
    Ok(())
}
```

`cargo run -p tidepool-codec --example formats` runs the example whose
output Tidepool's demo page shows: JSONC, TOML 1.1, errors and `Limits`.

## Documentation

- Design: [`docs/subsystems/text-and-formats.md`](../../docs/subsystems/text-and-formats.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0013
  (TOML for Tidepool's own files); ADR-0021 (the RFC 8259 examples in
  `tests/json/` keep the IETF Trust's notice, in `tests/json.rs`); ADR-0026
  (`toml` parses TOML 1.1, and its compliance cases are toml-test's,
  vendored in `tests/toml-test/`); ADR-0027 (the fuzz targets, in
  `tests/fuzz.rs`); ADR-0031 (`toml::Document` writes TOML 1.0 for the
  text it creates, and keeps the text an edit does not change byte for
  byte).
- [`docs/04-quality.md`](../../docs/04-quality.md) ("Fuzz smoke"): what
  each target runs and checks.
- API reference: `cargo doc -p tidepool-codec --open`

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

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).

Two sets of test data are not the project's: the RFC 8259 examples in
`tests/json/` (`rfc8259_image.json` and `rfc8259_locations.json`) are the
IETF Trust's, under the Revised BSD License, with their notice in
`tests/json.rs`; and toml-test's cases in `tests/toml-test/` are MIT, with
their notice in `tests/toml-test/NOTICE.md`. Both are test-only.
