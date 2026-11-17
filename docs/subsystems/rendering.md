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

| Module | Provides |
| :- | :- |
| `ttf` | TrueType parsing: `head`, `hhea`, `maxp`, `hmtx`, `cmap` (formats 4 and 12), `loca`, `glyf` (simple and composite), `kern` and the `kern` feature of `GPOS` (pairs only), `OS/2`, `name` |
| `outline` | Quadratic Bézier flattening, non-zero winding, exact-area coverage rasterization (grayscale, no hinting) |
| `atlas` | Glyph atlas keyed by (face, size, subpixel x offset in quarters); LRU eviction |
| `layout` | Runs of text into positioned glyphs: advance, kerning, tab stops, letter-spacing (for uppercase chrome), ellipsis, line breaking for UI text |
| `fallback` | Missing glyphs fall back to JetBrains Mono, then a system font when one is configured, then a visible `.notdef` box |
| `bundled` | Saira (400/500/600/700) and JetBrains Mono (400, 400 italic, 600) loaded from `assets/fonts/` at startup |

Out of scope in v1 (backlog with a trigger): CFF/OpenType outlines,
variable fonts, complex shaping, bidi, color emoji.

Invariants:

- Malformed font files return errors; table offsets and lengths are checked
  before reading (fuzz target).
- Coverage for a glyph is identical across runs and OSes.

## Tests

- Golden frames (`docs/04-quality.md` → "Golden frames", ADR-0044):
  `gfx`'s scenes in `crates/gfx/tests/golden.rs` draw each `raster`
  primitive, clipping, a layer, images, glyph masks and the backdrop at
  scale 1 and 2, compared exactly (M2.4); `font`'s will draw glyph runs in
  both bundled faces at 11, 12, 13, 14, 16 and 24 px at scale 1 and 2, uppercase
  letter-spaced labels.
- Unit tests: exact pixels for every `gfx` primitive.
- Property tests: no `gfx` draw call writes outside its clip, whatever its
  geometry; merged damage and frame diffs cover every changed pixel; the
  same draw calls give the same bytes twice.
- Benchmarks: full-frame raster at 1920×1080 and 2560×1440; atlas fill for
  the ASCII range at 14 px.
