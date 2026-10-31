# Subsystem: `tidepool-gfx` and `tidepool-font`

CPU rendering for the GUI (ADR-0011). Both crates are pure: no `unsafe`, no
OS calls. `gui` connects them to `platform::surface`.

## `tidepool-gfx`

| Module | Provides |
| :- | :- |
| `framebuffer` | Premultiplied BGRA8 framebuffer with stride; sub-rect views |
| `raster` | Filled rects, cut-corner rects (polygon), 1 px hairlines, borders, bracket corners, anti-aliased lines and polygons (coverage accumulation), rounded pills |
| `blend` | Source-over with premultiplied alpha; layer opacity |
| `clip` | Clip-rect stack |
| `image` | Blit with nearest and bilinear scaling |
| `glyph` | Coverage-mask blit with color (from `font`'s atlas) |
| `backdrop` | Cached background layers: radial gradients, the planet and star field, and the faint grid from jabbott.dev; rebuilt only on resize or theme change; a box-blur approximation for translucent panels |
| `damage` | Damage rectangle merging and frame diffing |

Invariants:

- Output is bit-identical on every OS for the same draw list and size
  (goldens are shared).
- A frame touches only pixels inside its damage rectangles.

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

- Golden frames: each `raster` primitive, the backdrop, glyph runs in both
  bundled faces at 11, 12, 13, 14, 16 and 24 px at scale 1 and 2, uppercase
  letter-spaced labels.
- Property tests: rects fully inside the clip never write outside; damage
  merging covers every changed pixel.
- Benchmarks: full-frame raster at 1920×1080 and 2560×1440; atlas fill for
  the ASCII range at 14 px.
