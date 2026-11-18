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
  window: about 190 ms for 1920×1080 at scale 1, and more for larger
  windows, in a release build (M2.3's note in `docs/roadmap.md`). `gui` rebuilds it on the main
  thread when the window is first exposed and when its size changes, and
  `gfx` and `font` are optimized in the dev profile, so that `cargo tp`
  meets the first frame's budget too (ADR-0045).

## `tidepool-font`

`ttf` and `outline` exist (M2.8), and `atlas`, `layout`, `fallback` and
`bundled` (M2.5). The rustdoc of each module is authoritative; the table,
the subset and the text rules below summarize it.

| Module | Provides |
| :- | :- |
| `ttf` | TrueType parsing: `head`, `hhea`, `maxp`, `hmtx`, `cmap` (formats 4 and 12), `loca`, `glyf` (simple and composite), `kern` and the `kern` feature of `GPOS` (pairs only), `OS/2`, `name`. `Font` owns its bytes or borrows `'static` ones, checks against `Limits`, and gives glyph ids, metrics, names, kerning and outlines; errors are `FontError`s naming the table or glyph and the byte offset |
| `outline` | `Outline`, quadratic contours in font units, and `rasterize`: flattening to 1/32 px, then exact-area coverage accumulation (the algorithm of `gfx::raster`'s polygons), grayscale, no hinting, into a `Bitmap` laid out as `gfx::glyph::Mask` with its place relative to the pen on the baseline |
| `atlas` | `Atlas`: rasterized glyphs in one 8-bit coverage image, laid out as `gfx::glyph::Mask`, keyed by (face, glyph, size in 1/64 px, subpixel x offset in quarters); shelf packing, LRU eviction that repacks what it keeps; `rasterize` for a glyph too large for it |
| `layout` | `line`, `truncate` and `wrap`: text in a `Style` (face, size, letter-spacing in ems, tab size) into `Line`s of positioned glyphs, with advance, kerning, tab stops, letter-spacing (for uppercase chrome), an ellipsis, and line breaking for UI text; `metrics`, from `hhea` |
| `fallback` | `Faces`, the parsed faces by `FaceId`: a missing glyph falls back to JetBrains Mono, then a system font when one is configured, then a visible `.notdef` box, made by this crate when a face's glyph 0 is empty |
| `bundled` | Saira (400/500/600/700) and JetBrains Mono (400, 400 italic, 600), compiled in from `assets/fonts/` and parsed at startup by `bundled::load` |

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

Setting text (the `layout`, `fallback` and `atlas` module docs give it in
full):

- **Faces and fallback:** a character is drawn by the face asked for if
  its `cmap` maps it, else by the first face of the fallback chain that
  does (the bundled JetBrains Mono, then a system face the caller parses
  and adds), else by the asked face's `.notdef`. Only the character map
  decides: no shaping, ligatures or mark placement. A system face is
  untrusted: the fuzz target `ttf` sets text in each font it parses, as
  the face asked for and as a fallback, through layout and the atlas.
- **Units:** device pixels, f32, the size in steps of 1/64 px up to
  2,048 px; a glyph's pen x is split into a whole pixel and a quarter,
  and the baseline is on a whole pixel.
- **A line:** advances from `hmtx`; kerning between glyphs of one face,
  never across faces or with `.notdef`; letter-spacing between glyphs,
  not after the last; a tab to the next stop every `tab_size` spaces,
  with no kerning or spacing around it. Case is the caller's. A line's
  width leaves out trailing spaces and tabs.
- **Truncating:** as many characters as fit with `…` after them, the
  blanks before the cut dropped; the ellipsis alone when nothing fits.
- **Wrapping:** at LF, CR LF and a lone CR (ADR-0019), then after runs of
  spaces and tabs, which hang; a word wider than the line breaks before
  the character that overflows. UAX #14 is not followed.
- **The atlas:** shelves as tall as their glyphs, rounded up to 4 px; when
  full, it keeps the most recently used glyphs that fill half its area,
  and less again until the new glyph fits, and packs them anew. A glyph
  is at most half the atlas's shorter side a side, refused from its
  control box before it is rasterized, and a key that fails is remembered
  (up to 1,024), so a hostile face's glyph costs its work once, not every
  frame. An `AtlasGlyph` borrows the atlas, so it cannot outlive the next
  call that may move it. An atlas holds one `Faces`' glyphs, and empties
  itself when handed another.
- **`.notdef`:** a face's glyph 0, unless it is empty, unreadable, flat or
  reaches more than an em beyond the em square, in which case this crate
  draws a box, as it does when glyph 0 cannot be rasterized.
- **Bounds:** letter-spacing beyond ±64 em is clamped, so positions stay
  finite, and a glyph's pixel is within ±2^30, so its offset adds without
  overflow.

Invariants:

- Malformed font files return errors; table offsets and lengths are checked
  before reading (fuzz target `ttf`, M2.8).
- Coverage for a glyph is identical across runs and OSes: `outline` uses
  only the IEEE 754 operations `gfx` does (above), and `tests/bundled.rs`
  pins an FNV-1a hash of each bundled face's ASCII coverage, which CI checks
  on every OS.
- A layout is identical across runs and OSes too: its arithmetic is
  IEEE 754 `+ − × ÷` and `floor` on f32 in a fixed order, so `font`'s
  golden scenes, laid out and drawn through the atlas, are shared by every
  OS as `gfx`'s are.
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
  scale 1 and 2, compared exactly (M2.4); `font`'s in
  `crates/font/tests/golden.rs` draw glyph runs in both bundled faces at
  11, 12, 13, 14, 16 and 24 px, the uppercase letter-spaced labels of
  `docs/subsystems/ui-and-theme.md` → Typography with Saira's four
  weights, and fallback, truncation, wrapping and tabs, each at scale 1
  and 2, through the atlas, their coverage blended by the scenes
  themselves, compared within 1 (M2.5).
- Unit tests: exact pixels for every `gfx` primitive; for `font` (M2.8),
  hand-made fonts that reach every error kind and each table's lookups
  (`tests/ttf.rs`), exact coverage of hand-made outlines
  (`tests/outline.rs`), and the bundled faces (`tests/bundled.rs`); and
  (M2.5) fallback, layout's positions checked against the fonts' own
  metrics and kerning (`tests/layout.rs`), and the atlas, whose glyphs are
  `rasterize`'s through eviction, a tiny atlas drawing what a large one
  does (`tests/atlas.rs`).
- Property tests: no `gfx` draw call writes outside its clip, whatever its
  geometry; merged damage and frame diffs cover every changed pixel; the
  same draw calls give the same bytes twice.
- Benchmarks: atlas fill for the ASCII range at 14 px,
  `font/atlas/fill_ascii/14px` in `tidepool-bench`, with its baseline on
  the reference machine (M2.5); full-frame raster at 1920×1080 and
  2560×1440 has no suite yet, and no task's `Do` names it.
