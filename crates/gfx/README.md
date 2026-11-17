# tidepool-gfx

The CPU rasterizer for the GUI: a premultiplied framebuffer, the primitives
the jabbott.dev look needs (cut corners, hairlines, bracket corners),
clipping, glyph blits, the cached backdrop and damage tracking. Pure and
deterministic, so golden frames are shared across OSes.

It is part of [Tidepool](https://github.com/jabbott-iii/Tidepool), where
`tidepool-gui` paints `ui`'s draw lists with it, beside `font`; it needs
only `tidepool-core`, and no OS.

Layer 1, OS behavior, formats and pure engines. It may use `core`, as its
row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Version 0.1, not published (`publish = false`): the API changes as the
roadmap needs.

Built, with the roadmap task that built it:

- `framebuffer`: `Framebuffer`, premultiplied BGRA8 pixels covering a
  rect anywhere in device space, up to 16,384 px a side, whose bytes,
  size and stride `platform::surface::Pixels` takes as they are
  (ADR-0040); and `View`, a borrowed view of such pixels whose shape is
  checked, and its regions — M2.3.
- `raster`: filled rects, 1 px hairlines, borders, bracket corners, and
  anti-aliased polygons (exact-area coverage; the non-zero rule for paths
  that do not overlap themselves, with the difference for those that do
  in the module's documentation), lines, cut-corner rects and pills —
  M2.3.
- `blend`: integer source-over, scaling by a coverage or an opacity, and
  compositing a layer with an opacity — M2.3.
- `clip`: rect intersection and the clip stack. Every draw call takes a
  clip rect and writes nothing outside it — M2.3.
- `image`: copy, blit, and scaled blit with nearest or bilinear filtering
  in fixed-point integers — M2.3.
- `glyph`: 8-bit coverage masks and their regions, drawn in a color —
  M2.3.
- `damage`: at most eight merged damage rects, and the damage between two
  frames, by 16 px tiles — M2.3.
- `backdrop`: jabbott.dev's `.space` backdrop (its glows, planet, stars
  and grid, with the stylesheet's values), cached per size, scale and
  style, with a box-blurred copy for translucent panels. Its
  approximations are listed in the module's documentation — M2.3.

- golden frames: 14 scenes in `tests/golden.rs`, one for each primitive,
  clipping, a layer, images, glyph masks, and the backdrop at scale 1 and
  2, which `cargo xtask golden` compares, exactly, with
  `tests/golden/*.qoi` at the repository root (ADR-0044) — M2.4.

The crate is written so that the same calls give the same bytes on every
OS and CPU: it uses integer arithmetic, or floating-point operations that
IEEE 754 rounds the same everywhere. The golden frames check that on every
CI runner, since `golden` is a step of `ci`. No draw call panics, whatever
its geometry.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Usage

A workspace crate whose row of the matrix allows it depends on it by path:

```toml
[dependencies]
tidepool-gfx = { path = "../gfx" }
```

Draw a focused panel over the backdrop, then an amber button, and find
what the button changed:

```rust
use tidepool_core::color::Color;
use tidepool_core::geom::{DevicePoint, DeviceRect, DeviceSize, Scale};
use tidepool_gfx::GfxError;
use tidepool_gfx::backdrop::{Backdrop, BackdropStyle};
use tidepool_gfx::clip::{ClipStack, intersect};
use tidepool_gfx::framebuffer::Framebuffer;
use tidepool_gfx::raster::{self, Corners};
use tidepool_gfx::{damage, image};

fn main() -> Result<(), GfxError> {
    let size = DeviceSize::new(320, 200);
    let mut frame = Framebuffer::new(size)?;
    let all = frame.bounds();

    // The backdrop is rendered once per size, scale and style.
    let mut backdrop = Backdrop::new();
    assert!(backdrop.ensure(size, Scale::ONE, &BackdropStyle::jabbott_dev())?);
    assert!(!backdrop.ensure(size, Scale::ONE, &BackdropStyle::jabbott_dev())?);
    image::copy(&mut frame, all, &backdrop.base().view(), DevicePoint::ZERO);

    // A translucent panel shows the blurred backdrop, under a 1 px border
    // and teal bracket corners.
    let panel = DeviceRect::new(20, 20, 200, 120);
    let mut clips = ClipStack::new(all);
    clips.push(panel);
    let clip = clips.current();
    image::copy(&mut frame, clip, &backdrop.blurred().view(), DevicePoint::ZERO);
    raster::fill_rect(&mut frame, clip, panel, Color::rgba(16, 19, 21, 199).premultiply());
    raster::border(&mut frame, clip, panel, 1, Color::rgba(255, 255, 255, 20).premultiply());
    let teal = Color::rgb(0x5c, 0xcb, 0xcb).premultiply();
    raster::brackets(&mut frame, clip, panel, 16, 1, Corners::TOP_LEFT_AND_BOTTOM_RIGHT, teal);
    clips.pop();

    // The primary button cuts its top-right and bottom-left corners.
    let before = frame.clone();
    let button = DeviceRect::new(40, 100, 96, 30);
    let amber = Color::rgb(0xf2, 0xc2, 0x00).premultiply();
    raster::cut_corner_rect(&mut frame, all, button, 10.0, Corners::TOP_RIGHT_AND_BOTTOM_LEFT, amber);

    // Only the tiles under the button changed, so only they are presented,
    // from `frame.bytes()`, `frame.size()` and `frame.stride()`.
    let changed = damage::diff(&before, &frame);
    assert_eq!(intersect(changed.bounds(), button), button);
    assert!(changed.rects().iter().all(|&rect| intersect(rect, DeviceRect::new(32, 96, 112, 48)) == rect));
    Ok(())
}
```

## Documentation

- Design: [`docs/subsystems/rendering.md`](../../docs/subsystems/rendering.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0011 (CPU
  rendering); ADR-0004 (the jabbott.dev look); ADR-0044 (the golden
  scenes).
- API reference: `cargo doc -p tidepool-gfx --open`

## Rules

- `unsafe` is denied by the workspace lint, and in doc tests by the crate
  root's `#![doc(test(attr(forbid(unsafe_code))))]` (ADR-0036);
  `cargo xtask unsafe-audit` checks both.
- Path-scoped rules in `.claude/rules/`:
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.

## Checks

```bash
cargo test -p tidepool-gfx
cargo xtask golden
```

`cargo xtask ci` runs the whole gate.

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
