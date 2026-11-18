# Subsystem: `tidepool-gfx` and `tidepool-font`

CPU rendering for the GUI (ADR-0011). Both crates are pure: no `unsafe`, no
OS calls. `gui` connects them to `platform::surface`.

## `tidepool-gfx`

Everything is in device pixels; colors are `core::color::PremultipliedColor`.
The rustdoc of each module is authoritative; this table summarizes it.

| Module | Provides |
| :- | :- |
| `framebuffer` | `Framebuffer`: premultiplied BGRA8, bytes `[b, g, r, a]`, stride four times the width, covering any rect of device space (its bounds), so a layer can cover part of a window; at most `MAX_DIMENSION` (16,384) a side. `View`: a checked, borrowed view of such rows, with sub-rect views; the bytes, size and stride are what `platform::surface::Pixels` takes (ADR-0040) |
| `raster` | Filled rects, 1 px hairlines, borders and bracket corners, crisp and blending no pixel twice; anti-aliased polygons (exact-area coverage accumulation), lines with butt caps, cut-corner rects and pills |
| `blend` | Source-over with premultiplied alpha, scaling by coverage or opacity, and compositing a layer with an opacity |
| `clip` | `ClipStack`, rect intersection, and `SPACE`, the device space a framebuffer may cover |
| `image` | Copy, blit, and scaled blit with nearest and bilinear filtering in fixed-point integers |
| `glyph` | `Mask`, an 8-bit coverage view with regions (from `font`'s atlas, M2.5), and its blit with a color |
| `backdrop` | `BackdropStyle`, jabbott.dev's `.space` layers as data (radial glows, the planet, two star tiles, the masked grid, the blur radius), and `Backdrop`, which renders them and a box-blurred copy for translucent panels, and rebuilds only when the size, scale or style changes |
| `damage` | `Damage`, at most `MAX_RECTS` (8) merged rects that cover everything added, and `diff`, the rects of 16 px tiles that changed between two frames |

Invariants:

- Output is bit-identical on every OS for the same draw calls and size
  (goldens are shared): the code uses integer arithmetic, or IEEE 754
  operations that every CPU rounds the same (`+ - * /`, `sqrt`, `floor`),
  never `mul_add` or a libm function such as `exp` or `sin`.
- Every draw call takes a clip rect and touches only pixels inside it and
  the framebuffer's bounds; the painter clips to the damage rectangles, so
  a frame touches only pixels inside them.
- No draw call panics, whatever its geometry (NaN, infinite or huge
  coordinates, empty or inverted rects): what is invalid draws nothing.
- The backdrop approximates the site's CSS where the browser uses a
  Gaussian or clips to a rounded box; each approximation is listed in
  `backdrop`'s module docs. A rebuild is CPU work that grows with the
  window: a few hundred milliseconds for 1920×1080 in a release build
  (M2.3's note in `docs/roadmap.md`), which M2.6 weighs against the
  first frame's budget.

## `tidepool-font`

`ttf` and `outline` exist (M2.8); `atlas`, `layout`, `fallback` and
`bundled` land in M2.5. The rustdoc of each module is authoritative; the
table and the subset below summarize it.

| Module | Provides |
| :- | :- |
| `ttf` | TrueType parsing: `head`, `hhea`, `maxp`, `hmtx`, `cmap` (formats 4 and 12), `loca`, `glyf` (simple and composite), `kern` and the `kern` feature of `GPOS` (pairs only), `OS/2`, `name`. `Font` owns its bytes or borrows `'static` ones, checks against `Limits`, and gives glyph ids, metrics, names, kerning and outlines; errors are `FontError`s naming the table or glyph and the byte offset |
| `outline` | `Outline`, quadratic contours in font units, and `rasterize`: flattening to 1/32 px, then exact-area coverage accumulation (the algorithm of `gfx::raster`'s polygons), grayscale, no hinting, into a `Bitmap` laid out as `gfx::glyph::Mask` with its place relative to the pen on the baseline |
| `atlas` | Glyph atlas keyed by (face, size, subpixel x offset in quarters); LRU eviction |
| `layout` | Runs of text into positioned glyphs: advance, kerning, tab stops, letter-spacing (for uppercase chrome), ellipsis, line breaking for UI text |
| `fallback` | Missing glyphs fall back to JetBrains Mono, then a system font when one is configured, then a visible `.notdef` box |
| `bundled` | Saira (400/500/600/700) and JetBrains Mono (400, 400 italic, 600) loaded from `assets/fonts/` at startup |

Out of scope in v1 (backlog with a trigger): CFF/OpenType outlines,
variable fonts, complex shaping, bidi, color emoji.

The supported subset (the `ttf` module docs, `Font::kerning` and
`Font::outline` give it in full):

- **Files:** an sfnt of TrueType outlines (`0x00010000` or `true`). CFF
  (`OTTO`) and collections (`ttcf`) are refused. Checksums are not
  verified. A variable font reads as its default instance.
- **Tables read:** `head`, `hhea`, `maxp` 1.0, `hmtx`, `loca`, `glyf`,
  and one `cmap` subtable, required; `OS/2`, `name`, `GPOS` and `kern`
  if present. Every other table is ignored, the hinting tables, `post`,
  `GSUB` and `GDEF` among them, and no instruction is ever run.
- **`cmap`:** format 12 (platform 3 encoding 10, or platform 0), else
  format 4 (platform 3 encoding 1, or platform 0); symbol and format 14
  subtables are not used.
- **`name`:** Windows (encoding 1 or 10, preferring US English) and
  Unicode records, in UTF-16BE; Macintosh records are ignored.
- **`OS/2`:** any version, long enough for its fields; a legacy version 0
  table of 68 bytes reads without its typographic and Windows metrics.
- **Kerning:** from `GPOS` when the default language system of `latn`,
  else `DFLT`, else the first script, has a `kern` feature: pair
  adjustment, formats 1 and 2, directly or through extension lookups,
  taking only the first glyph's `XAdvance`; a subtable that has no value
  for the pair (no record in format 1, a second glyph in class 0 in
  format 2) gives way to the next. Otherwise from a Microsoft
  `kern` table (version 0, format 0, horizontal); Apple's version is
  ignored.
- **Glyphs:** simple and composite glyphs, with offsets, a scale, x and y
  scales or a 2 × 2 matrix, and point matching. `ROUND_XY_TO_GRID`,
  `USE_MY_METRICS` and `OVERLAP_COMPOUND` are ignored.
- **Checked when parsed:** everything a lookup reads (the directory, the
  tables' fields and lengths, every `loca` offset, the `cmap` subtable's
  order, the `name` records, every structure a kerning lookup reads, with
  a budget that refuses `GPOS` structures that overlap). A glyph's data is
  checked when its outline is read. Lookups then cannot fail: an id the
  `cmap` arithmetic puts past the glyph count is an unmapped character.
  The parser is strict about what it reads: a malformed part of a table
  it reads fails the whole font, optional tables too. When `GPOS` gives
  the kerning, a `kern` table's directory record is still checked, but
  not its contents; and of `GPOS`, only the chosen language system, its
  `kern` features and the lookups they name are read, other types of
  lookup no further than their type and subtables' offsets.
- **Limits** (`font::Limits`): the file's size, a glyph's points, the
  nesting and number of components, the kerning subtables kept, a
  bitmap's sides, and the work of rasterizing one glyph, each checked
  before what it bounds is allocated or done.

Invariants:

- Malformed font files return errors; table offsets and lengths are checked
  before reading (fuzz target `ttf`, M2.8).
- Coverage for a glyph is identical across runs and OSes: `outline` uses
  only the IEEE 754 operations `gfx` does (above), and `tests/bundled.rs`
  pins an FNV-1a hash of each bundled face's ASCII coverage, which CI checks
  on every OS.
- Coverage is the non-zero rule exactly for contours that overlap neither
  themselves nor one another and wind the same way, with holes inside
  them winding the other way, as TrueType's do. Elsewhere it differs as
  `gfx::raster`'s polygons do: at the edges of overlaps; where contours
  winding opposite ways share a pixel, as a mirrored component beside its
  original does, their coverage cancels there; and a self-intersecting
  contour's lobes cancel or double (the `outline` module docs).

## Tests

- Golden frames (`docs/04-quality.md` → "Golden frames", ADR-0044):
  `gfx`'s scenes in `crates/gfx/tests/golden.rs` draw each `raster`
  primitive, clipping, a layer, images, glyph masks and the backdrop at
  scale 1 and 2, compared exactly (M2.4); `font`'s will draw glyph runs in
  both bundled faces at 11, 12, 13, 14, 16 and 24 px at scale 1 and 2, uppercase
  letter-spaced labels.
- Unit tests: exact pixels for every `gfx` primitive; for `font` (M2.8),
  hand-made fonts that reach every error kind and each table's lookups
  (`tests/ttf.rs`), exact coverage of hand-made outlines
  (`tests/outline.rs`), and the bundled faces (`tests/bundled.rs`).
- Property tests: no `gfx` draw call writes outside its clip, whatever its
  geometry; merged damage and frame diffs cover every changed pixel; the
  same draw calls give the same bytes twice.
- Benchmarks: full-frame raster at 1920×1080 and 2560×1440; atlas fill for
  the ASCII range at 14 px.
