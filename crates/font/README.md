# tidepool-font

TrueType parsing, the glyph rasterizer, the glyph atlas and text layout, for
the bundled Saira and JetBrains Mono faces. Pure; malformed fonts return
errors.

It is part of [Tidepool](https://github.com/jabbott-iii/Tidepool), where
`tidepool-gui` lays out and draws text with it, beside `gfx`; it needs only
`tidepool-core`, and its library uses no crate at all (its tests use
`core`'s fuzz engine and FNV-1a hash).

Layer 1, OS behavior, formats and pure engines. It may use `core`, as its
row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Version 0.1, not published (`publish = false`): the API changes as the
roadmap needs.

Built, with the roadmap task that built it:

- `ttf`: the TrueType parser — M2.8. `Font::parse` takes a file's bytes,
  owned or borrowed for the program's life, and checks against `Limits`
  everything its lookups rely on: the table directory, `head`, `hhea`,
  `maxp`, `hmtx`, `loca`, the `cmap` subtable it uses (format 12, else
  format 4), and the optional `OS/2` (a legacy 68-byte version 0 too),
  `name`, `kern` and `GPOS`, each structure once, so that the checks take
  time that grows with the file. CFF outlines and collections are
  refused. A `Font` then maps characters to glyphs, gives the metrics,
  names and style, kerns pairs from the `kern` feature of `GPOS`'s `latn`
  language system, else `DFLT`'s, else the first script's (pair
  adjustment formats 1 and 2, extensions too), or else from the `kern`
  table, keeping at most `Limits::max_kerning_subtables` subtables from
  either (512, so a call tries at most that many), and reads each glyph's
  outline, simple or composite, with nesting, components and points
  bounded and every coordinate finite. Errors are `FontError`s that name
  the table or glyph, the byte offset and what was wrong.
- `outline`: outlines and their rasterizer — M2.8. `outline::rasterize`
  flattens the quadratic curves to within 1/32 of a pixel and computes each
  pixel's exact area by accumulation (the algorithm of `gfx::raster`'s
  polygons), unhinted, with a subpixel shift, into an 8-bit coverage bitmap
  in the layout of `gfx::glyph::Mask`. Before it allocates, it checks the
  bitmap against `Limits::max_bitmap_dimension` (4,096 pixels a side) and
  the work against `Limits::max_raster_work` (2^21 steps, estimated from
  the outline; under 100 ms at most, measured). Its arithmetic gives the
  same bytes on every run and OS; a test pins the FNV-1a 64 hash of every
  bundled face's printable ASCII glyphs at 14 px.
- The bundled font files, in [`assets/fonts/`](../../assets/fonts): Saira
  Regular, Medium, SemiBold and Bold, and JetBrains Mono Regular, Italic and
  SemiBold, with their OFL license files — M2.8. Each parses, every glyph
  reads and rasterizes, and every composite is its components moved by the
  offsets its data gives, in `tests/bundled.rs`.
- The fuzz target `ttf` in `tests/fuzz.rs`, with small `Limits` and seeds
  made by hand in `fuzz/corpus/ttf/`, some exactly at each limit. It checks
  that each error is where its kind and table say, and that what parses
  reads, rasterizes and maps within the limits — M2.8; and sets text in
  each font that parses, as the face asked for and as a fallback, through
  layout and a small atlas — M2.5.

- `bundled`: the seven faces compiled into the binary with
  `include_bytes!` and parsed by `bundled::load` at startup, in 0.04 ms in
  a release build — M2.5.
- `fallback`: `Faces`, the parsed faces by `FaceId`, at most 64, with a
  fallback chain. A character is drawn by the face asked for, else the
  first face of the chain that maps it (the bundled JetBrains Mono, then a
  system face the caller parses and adds), else the asked face's
  `.notdef` box, which this crate makes for a face whose glyph 0 is empty
  — M2.5.
- `layout`: `line`, `truncate` (with an ellipsis) and `wrap` set text in a
  `Style` (face, size in 1/64 px, letter-spacing in ems, tab size) as
  `Line`s of positioned glyphs, in device pixels: advances, kerning within
  a face, letter-spacing between glyphs, tab stops, breaks at LF, CR LF
  and CR and after spaces, which hang, and a word wider than the line
  broken where it overflows; `metrics` gives the ascent and descent — M2.5.
- `atlas`: `Atlas` caches rasterized glyphs, keyed by face, glyph, size
  and quarter-pixel offset, in one coverage image laid out as
  `gfx::glyph::Mask`, on shelves; when full it keeps the most recently
  used glyphs and repacks them. A glyph is at most half its shorter side,
  refused before it is rasterized, and a key that fails is remembered, so
  a hostile face's glyph costs its work once. Its `AtlasGlyph` borrows it,
  so a glyph's place cannot be used after a later call moves it — M2.5.
- The golden scenes in `tests/golden.rs`: both faces at 11 to 24 px, the
  chrome's uppercase letter-spaced labels with Saira's four weights, and
  fallback, truncation, wrapping and tabs, each at scale 1 and 2, drawn
  through the atlas and compared within 1 by `cargo xtask golden` — M2.5.

Not yet:

- CFF outlines, variable fonts beyond their default instance, shaping,
  bidi and color emoji: in the backlog
  ([`docs/subsystems/rendering.md`](../../docs/subsystems/rendering.md)).

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Usage

A workspace crate whose row of the matrix allows it depends on it by path:

```toml
[dependencies]
tidepool-font = { path = "../font" }
```

Parse a face, map a character to its glyph, and rasterize the glyph at
14 pixels per em, a quarter of a pixel to the right. The bitmap is one byte
of coverage a pixel, rows from the top; its `left` and `top` place it from
the pen on the baseline, y growing downwards:

```rust
use tidepool_font::ttf::{Font, NameId};
use tidepool_font::{FontError, Limits, outline};

fn main() -> Result<(), FontError> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/saira/Saira-Regular.ttf"
    );
    let bytes = std::fs::read(path).expect("the bundled face");
    let font = Font::parse(bytes, Limits::DEFAULT)?;
    assert_eq!(font.name(NameId::FAMILY).as_deref(), Some("Saira"));

    let a = font.glyph_index('A').expect("Saira has an A");
    let v = font.glyph_index('V').expect("and a V");
    assert!(font.kerning(a, v) < 0);

    let scale = 14.0 / f32::from(font.units_per_em());
    let bitmap = outline::rasterize(&font.outline(a)?, scale, 0.25, Limits::DEFAULT)?;
    assert!(bitmap.top < 0, "above the baseline");
    assert_eq!(bitmap.pixels.len(), (bitmap.width * bitmap.height) as usize);
    // Partly covered pixels along the strokes, and the empty inside.
    assert!(bitmap.pixels.iter().any(|&coverage| coverage > 0 && coverage < 255));
    assert!(bitmap.pixels.contains(&0));
    Ok(())
}
```

Set a label in the bundled faces, cut to fit, and take its glyphs from an
atlas. Each glyph's pen x splits into a whole pixel and a quarter; its
image goes at that pixel plus `left`, and the baseline plus `top`. `gui`
hands `pixels()` to `gfx::glyph::Mask::new` with the atlas's size as its
stride, and draws each glyph's `Mask::region`:

```rust
use tidepool_font::FontError;
use tidepool_font::atlas::{Atlas, GlyphKey, Subpixel};
use tidepool_font::bundled::{self, Face};
use tidepool_font::layout::{self, PxSize, Style};

fn main() -> Result<(), FontError> {
    let faces = bundled::load()?;
    // An eyebrow: Saira SemiBold, 11 px at scale 2, tracked 0.28em.
    let style = Style {
        letter_spacing: 0.28,
        ..Style::new(Face::SairaSemiBold.id(), PxSize::new(22.0).expect("a size"))
    };
    let line = layout::truncate(&faces, "EXPLORER: OPEN EDITORS", &style, 200.0)?;
    assert!(line.truncated && line.width <= 200.0);

    let mut atlas = Atlas::new(1024, 1024)?;
    let (origin_x, baseline) = (16.0, 40);
    for glyph in &line.glyphs {
        let Some(id) = glyph.glyph else { continue };
        let (pixel, subpixel) = Subpixel::split(origin_x + glyph.x);
        let key = GlyphKey { face: glyph.face, glyph: id, size: style.size, subpixel };
        let image = atlas.glyph(&faces, key)?;
        let (x, y) = (pixel + image.left, baseline + image.top);
        assert!(x >= 16 && y < baseline);
        assert_eq!(image.pixels().len(), 1024 * 1024);
    }
    Ok(())
}
```

A character no bundled face has falls back to a `.notdef` box:

```rust
use tidepool_font::bundled::{self, Face};

let faces = bundled::load().expect("the bundled faces");
let saira = Face::SairaRegular.id();
assert_eq!(faces.resolve(saira, '✓').map(|found| found.face), Ok(Face::MonoRegular.id()));
assert!(faces.resolve(saira, '中').is_ok_and(|found| found.is_notdef()));
```

A malformed font is an error that says where it went wrong:

```rust
use tidepool_font::ttf::Font;
use tidepool_font::{FontError, Limits};

let error = Font::parse(b"OTTO and the rest".to_vec(), Limits::DEFAULT)
    .expect_err("CFF outlines are not read");
assert_eq!(
    error.to_string(),
    "TrueType error at byte 0: the font has CFF outlines (`OTTO`), which are not supported"
);
assert!(matches!(error, FontError::Ttf { offset: 0, .. }));
```

## Documentation

- Design: [`docs/subsystems/rendering.md`](../../docs/subsystems/rendering.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0004 (the
  bundled faces), ADR-0019 (the line breaks `wrap` keeps), ADR-0027 (the
  fuzz target), ADR-0044 (the golden scenes).
- What is read and checked, and how coverage relates to the non-zero
  winding rule: the module docs of `ttf` and `outline`; how text is set,
  falls back and is cached: those of `layout`, `fallback` and `atlas`.
- API reference: `cargo doc -p tidepool-font --open`

## Rules

- `unsafe` is denied by the workspace lint, and in doc tests by the crate
  root's `#![doc(test(attr(forbid(unsafe_code))))]` (ADR-0036);
  `cargo xtask unsafe-audit` checks both.
- Path-scoped rules in `.claude/rules/`:
  [`parsers.md`](../../.claude/rules/parsers.md),
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.
- Coverage and layout must stay the same on every OS: the rasterizer
  uses only IEEE 754 `+ − × ÷`, `sqrt`, `floor` and `ceil`, and layout
  `+ − × ÷` and `floor`, never `mul_add` or a libm function; changing
  either's output changes the pinned hashes in `tests/bundled.rs` or the
  golden references, which are re-blessed only with review.

## Checks

```bash
cargo test -p tidepool-font
cargo xtask golden
cargo xtask fuzz-smoke --target ttf --secs 60
```

`cargo xtask ci` runs the whole gate.

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
