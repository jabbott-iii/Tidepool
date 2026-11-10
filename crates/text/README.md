# tidepool-text

The text engine every buffer is built on: a persistent rope, positions and
their conversions (UTF-16 included), extended grapheme clusters,
multi-cursor selections, canonical change sets, the undo tree, lossless
decoding of UTF-8 and UTF-16, and loading and atomic saving of text files.
Positions are byte offsets, and a bad position is an error, never a panic.

It is part of [Tidepool](https://github.com/jabbott-iii/Tidepool), an IDE
for Linux, macOS and Windows, written in Rust with no third-party crates.
Its library uses no other crate (its tests use `core`'s harness).
`tidepool-bench` measures it, and saves its baseline with `file::save`;
`syntax`, `workspace`, `lang`, `vcs`, `agent` and `editor` may build their
buffers, edits and saves on it, as their rows of the matrix allow, and none
does yet.

Layer 1, OS behavior, formats and pure engines. It may use `core` (its tests
use `core`'s property harness), as its row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Version 0.1, not published (`publish = false`): the API changes as the
roadmap needs.

Built, with the roadmap task that built it:

- `rope` and `pos`, with grapheme boundaries (UAX #29) from tables that
  `cargo xtask ucd` generates from `ucd/17.0.0/` — M1.1.
- `selection`, `transaction` and `history` — M1.2.
- `encoding` (UTF-8 and UTF-16 LE/BE by BOM, decoded losslessly, invalid
  input read-only) and `file` (loading with the 16 MiB large-file
  threshold, and the atomic save, which follows symbolic links) — M1.3.
- The fuzz target `encoding` in `tests/fuzz.rs`: the decoder, which takes
  no `Limits`, fed in pieces and at once, which must agree, decode as
  `std` does, keep the promises about replaced runs, give back the input,
  and encode it back when nothing was replaced; seeded from
  `fuzz/corpus/encoding/` — M1.6.

All of the crate's planned modules exist.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Usage

A workspace crate whose row of the matrix allows it depends on it by path:

```toml
[dependencies]
tidepool-text = { path = "../text" }
```

Edit through the buffer's `History`, which every edit goes through
(ADR-0020): here one transaction types at two carets, then undo restores
the text and the carets it was made from, and redo brings the edit back:

```rust
use std::time::Instant;

use tidepool_text::{Cursor, Group, History, Place, Rope, Selection, TextError, Transaction};

fn main() -> Result<(), TextError> {
    let mut text = Rope::from("one\ntwo\n");
    let mut history = History::new(&text);

    let carets = Selection::new(Cursor::caret(0), [Cursor::caret(4)]);
    let edit = Transaction::change_each(&text, &carets, |cursor| {
        Some((cursor.head..cursor.head, "// ", Place::End))
    })?;
    history.commit(&mut text, edit, &carets, Instant::now(), Group::New)?;
    assert_eq!(text, "// one\n// two\n");
    assert!(history.is_modified());

    let undone = history.undo(&mut text)?;
    assert_eq!(text, "one\ntwo\n");
    assert_eq!(undone.map(|step| step.selection), Some(carets));
    assert!(!history.is_modified());

    history.redo(&mut text)?;
    assert_eq!(text, "// one\n// two\n");
    Ok(())
}
```

Convert positions. A byte offset becomes a line and a column counted in
bytes, UTF-16 units (as LSP counts them by default) or chars, and back,
exactly; CR LF is one line break; a position inside a character is an
error; and cursor motion goes by grapheme cluster:

```rust
use tidepool_text::{LineCol, Rope, TextError, Unit};

fn main() -> Result<(), TextError> {
    let text = Rope::from("let s = \"é😀\";\r\nok\n");
    let quote = "let s = \"é😀".len();
    assert_eq!(text.byte_to_line_col(quote, Unit::Byte)?, LineCol::new(0, 15));
    assert_eq!(text.byte_to_line_col(quote, Unit::Utf16)?, LineCol::new(0, 12));
    assert_eq!(text.byte_to_line_col(quote, Unit::Char)?, LineCol::new(0, 11));
    assert_eq!(text.line_col_to_byte(LineCol::new(0, 12), Unit::Utf16)?, quote);

    assert_eq!(text.len_lines(), 3);
    assert_eq!(text.line_to_byte(1)?, quote + "\";\r\n".len());

    // The second half of "😀" in UTF-16, and the middle of "é" in UTF-8.
    assert!(matches!(
        text.line_col_to_byte(LineCol::new(0, 11), Unit::Utf16),
        Err(TextError::ColumnInsideChar { .. })
    ));
    assert!(matches!(
        text.byte_to_line(10),
        Err(TextError::NotCharBoundary { byte: 10 })
    ));

    // A flag is two chars and one grapheme cluster.
    let flag = Rope::from("🇺🇸!");
    assert_eq!(flag.next_grapheme_boundary(0), Some(8));
    Ok(())
}
```

Decode bytes. The encoding comes from the byte-order mark alone, and
writing the text back in the format it was read in gives back the same
bytes. Bytes that are not text become U+FFFD and make the text read-only,
but are kept, so nothing is ever rewritten:

```rust
use tidepool_text::encoding::{self, Decoder};
use tidepool_text::{Encoding, Format};

fn main() -> std::io::Result<()> {
    let bytes = [0xFF, 0xFE, b'h', 0, b'i', 0];
    let decoded = encoding::decode(&bytes);
    assert_eq!(decoded.text(), "hi");
    assert_eq!(decoded.format(), Some(Format::new(Encoding::Utf16Le, true)));

    let mut saved = Vec::new();
    encoding::encode(decoded.text(), decoded.detected(), &mut saved)?;
    assert_eq!(saved, bytes);

    // However the bytes arrive, the result is the same.
    let mut decoder = Decoder::new();
    decoder.push(&bytes[..3]);
    decoder.push(&bytes[3..]);
    assert_eq!(decoder.finish(), decoded);

    let broken = encoding::decode(b"ok \xFF\n");
    assert_eq!(broken.text(), "ok \u{FFFD}\n");
    assert!(broken.read_only());
    assert_eq!(broken.format(), None);
    assert_eq!(broken.original_bytes(), b"ok \xFF\n");
    Ok(())
}
```

`cargo run -p tidepool-text --example editing` runs the example whose
output Tidepool's demo page shows: positions, graphemes, undo and the save.

## Documentation

- Design: [`docs/subsystems/text-and-formats.md`](../../docs/subsystems/text-and-formats.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0018 (the
  Unicode data); ADR-0019 (positions); ADR-0020 (edits and undo); ADR-0025
  (saving: links followed, the temporary file's mode, what a rename
  changes); ADR-0027 (the fuzz target, in `tests/fuzz.rs`); ADR-0034 (a
  relative link is followed from its own directory as Windows reaches it;
  Unix keeps the lexical join).
- [`docs/04-quality.md`](../../docs/04-quality.md) ("Fuzz smoke"): what the
  target runs and checks.
- API reference: `cargo doc -p tidepool-text --open`

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
  it copies. Tests that check modes and links may use `cfg(unix)` too,
  and tests that make links to directories `cfg(windows)` (ADR-0034).

## Checks

```bash
cargo test -p tidepool-text
cargo xtask ucd --check
cargo xtask fuzz-smoke --target encoding
```

`cargo test` replays the fuzz target's seeds and saved crashes;
`fuzz-smoke` also mutates them, for 60 s unless `--secs` says otherwise.

`cargo xtask ci` runs the whole gate.

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).

The Unicode data in `ucd/17.0.0/` is Unicode, Inc.'s, under the Unicode
License v3 in `ucd/17.0.0/license.txt`; the grapheme tables generated from
it, `src/pos/grapheme/tables.rs`, keep that notice.
