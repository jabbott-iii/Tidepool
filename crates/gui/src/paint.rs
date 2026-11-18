// Copyright 2026 Joseph Anthony Abbott III
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! The draw-list painter: [`Painter`] paints `ui`'s [`Frame`]s into a
//! framebuffer with `gfx` and `font`, the damage only (M3.5, ADR-0048).
//!
//! New in Tidepool.
//!
//! ```
//! use tidepool_core::color::Color;
//! use tidepool_core::geom::{DevicePoint, DeviceSize, Rect, Scale, Size};
//! use tidepool_gfx::framebuffer::Framebuffer;
//! use tidepool_gui::Painter;
//! use tidepool_ui::draw::{DrawList, PaintContext};
//! use tidepool_ui::layout::Style;
//! use tidepool_ui::{Node, Tree, Widget};
//!
//! /// Fills its bounds in teal.
//! struct Fill;
//!
//! impl Widget for Fill {
//!     fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
//!         list.rect(cx.bounds(), Color::rgb(0x5c, 0xcb, 0xcb));
//!     }
//! }
//!
//! let mut tree = Tree::new();
//! let root = tree.root();
//! tree.insert(root, Node::new(Style::default().fixed(10.0, 10.0)).with_widget(Fill))
//!     .expect("a node");
//!
//! let mut painter = Painter::new().expect("the bundled faces load");
//! let scale = Scale::new(2.0).expect("a scale");
//! let frame = tree
//!     .frame(Size::new(40.0, 20.0), &painter.metrics(scale))
//!     .expect("the first frame damages the window");
//!
//! // A window of 80 × 40 device pixels over a black base.
//! let size = DeviceSize::new(80, 40);
//! let mut base = Framebuffer::new(size).expect("a base");
//! base.clear(Color::BLACK.premultiply());
//! let mut window = Framebuffer::new(size).expect("a window");
//! let damage = painter.paint(&mut window, &base.view(), &frame, scale).expect("painted");
//!
//! assert_eq!(damage.rects().len(), 1, "the whole window");
//! let teal = Color::rgb(0x5c, 0xcb, 0xcb).premultiply();
//! assert_eq!(window.pixel(DevicePoint::new(19, 19)), Some(teal));
//! assert_eq!(window.pixel(DevicePoint::new(20, 20)), Some(Color::BLACK.premultiply()));
//! // Idle: no frame, nothing to paint.
//! assert!(tree.frame(Size::new(40.0, 20.0), &painter.metrics(scale)).is_none());
//! ```
//!
//! # Damage
//!
//! [`Painter::paint`] takes each of the frame's damage rects into device
//! pixels, its edges rounded out to whole pixels; merges them as
//! [`Damage`] merges, into at most 8 rects that do not overlap; and cuts
//! them to the target and the base, which lies at the target's top-left
//! corner. In each it copies the base, then draws the whole list, clipped
//! to the rect. It returns those rects: they are the only pixels it
//! touched, and what to present.
//!
//! Each command's ink is also clipped to its own bounds
//! ([`Command::bounds`]), cut to the clips around it and rounded out to
//! whole device pixels: the area `ui` damages when the command changes,
//! old or new. So no anti-aliasing reaches past the damage, which is
//! therefore not grown, and painting the damage alone gives the same pixels
//! as painting the whole frame anew, provided that:
//!
//! - `Tree::frame` measured text with [`Painter::metrics`] at the scale
//!   painted; and
//! - the caller damages what draws an icon or image whose registration
//!   changed ([`Painter::set_icon`], [`Painter::set_image`] and their
//!   removals): `ui` cannot know that the same command now draws other
//!   pixels. `Tree::set_style` on the nodes that draw it, or
//!   `Tree::invalidate_all`, repaints them.
//!
//! # Geometry
//!
//! Commands are in logical units, times the scale in device pixels:
//!
//! - **Rects** (of a fill, a cut-corner rect, a pill, a border, brackets,
//!   an icon, an image and a clip): each edge rounded to the nearest device
//!   pixel, half-way rounding up. The right edge is `x + width` and the
//!   bottom `y + height` summed in `f32`, as `ui`'s layout places the next
//!   sibling, so rects that share an edge share it at any scale.
//! - **Widths:** a border's width and a bracket's thickness are snapped as
//!   CSS snaps a border width: none for 0 or less, one device pixel for
//!   less than one, and floored otherwise. A bracket's arm is rounded to
//!   the nearest pixel.
//! - **Anti-aliased:** a line's ends and width, and a cut corner's cut, in
//!   device pixels as they are; a pill is anti-aliased round its rounded
//!   rect.
//! - **Text:** set in [`metrics::face`](crate::metrics::face) at its size
//!   times the scale, from its origin's x, split into a whole pixel and a
//!   quarter for each glyph, on its origin's y rounded to a whole pixel. A
//!   glyph the atlas cannot hold (over half its side) is rasterized on its
//!   own each time it is drawn; one that cannot be rasterized is left out.
//! - **Icons:** the [`Icon`] registered for the id, scaled uniformly to
//!   the largest square the rounded rect holds, centred in it on whole
//!   pixels (half a pixel left over goes right and down), and clipped to
//!   it; an id with no icon draws a hollow box one device pixel wide, as a
//!   `.notdef`; an icon `font` cannot rasterize at that size draws nothing.
//! - **Images:** the image registered for the id, scaled to the rounded
//!   rect with `gfx`'s bilinear filter; an id with no image draws nothing.
//!
//! Geometry with a NaN draws nothing, a cut corner's cut too; geometry
//! anywhere else is clipped, and none of it panics.
//!
//! # Clips and layers
//!
//! A [`Command::PushClip`] narrows the clip until its [`Command::PopClip`].
//! A [`Command::PushLayer`] draws what follows, up to its
//! [`Command::PopLayer`], into a layer, and composites that at
//! `round(opacity × 255)`: an opacity of 1 (or NaN, as `ui` takes it)
//! draws straight through, and 0 draws nothing. A layer is a framebuffer
//! made when a command first inks inside it, covering that ink, and grown
//! to hold what inks after, by at least its width or height each way it
//! must, never past the clip it was pushed in; a layer nothing inks costs
//! nothing. At most [`MAX_LAYERS`] layers nest; a deeper one draws into the
//! layer around it, at that layer's opacity alone. A pop that ends no push
//! of its kind is ignored, and pushes left open are closed at the end, so a
//! list made by hand paints as one from `Tree::frame` does.
//!
//! # Cost
//!
//! Each damage rect walks the whole list. A command whose bounds miss the
//! rect costs a test of them, except a text: until it is laid out its
//! width is not known, so a text whose line reaches the rect's rows and
//! that starts left of its right edge is laid out, once in each such rect,
//! and its glyphs taken from the atlas when it inks the rect. An icon is
//! rasterized each time it is drawn. A line, or a cut-corner rect with a
//! cut, is filled one 64 px tile of a grid fixed in device space at a
//! time, so that where a damage rect's edges fall cannot change its
//! coverage: a tile it may cover in part is a framebuffer of its own,
//! composited; one a line does not come near is skipped, and one a
//! cut-corner rect covers whole is filled. Each layer is a framebuffer at
//! least as large as the bounding box of what it inks, at most
//! [`MAX_LAYERS`] of them, made when it first inks and grown, with a copy
//! of what it holds, by at least its width or height each time it must.

use std::collections::BTreeMap;

use tidepool_core::color::{Color, PremultipliedColor};
use tidepool_core::geom::{DevicePoint, DeviceRect, DeviceSize, Point, Rect, Scale};
use tidepool_font::atlas::{self, Atlas, GlyphKey, Subpixel};
use tidepool_font::bundled;
use tidepool_font::fallback::Faces;
use tidepool_font::layout::{Line, Style};
use tidepool_font::outline::Bitmap;
use tidepool_gfx::blend;
use tidepool_gfx::clip::{self, ClipStack, SPACE};
use tidepool_gfx::damage::Damage;
use tidepool_gfx::framebuffer::{Framebuffer, View};
use tidepool_gfx::glyph::{self, Mask};
use tidepool_gfx::image::{self, Filter};
use tidepool_gfx::raster::{self, Vertex};
use tidepool_ui::draw::{Command, Corners, Frame, IconId, ImageId};
use tidepool_ui::layout::MAX_EXTENT;
use tidepool_ui::metrics::{LineMetrics, TextMetrics, TextStyle};

use crate::GuiError;
use crate::icon::Icon;
use crate::metrics::FontMetrics;

/// The most layers that nest, each its own framebuffer.
pub const MAX_LAYERS: usize = 8;

/// The glyph atlas's side, in pixels: glyphs up to half that, 512 px, are
/// cached in it.
pub const ATLAS_SIDE: u32 = 1024;

/// Paints `ui`'s frames (the [module documentation](self)): the bundled
/// faces, a glyph atlas, and the icons and images registered with it.
#[derive(Debug)]
pub struct Painter {
    faces: Faces,
    atlas: Atlas,
    icons: BTreeMap<IconId, Icon>,
    images: BTreeMap<ImageId, Framebuffer>,
}

/// What a push opened, while it is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Open {
    /// A clip.
    Clip,
    /// A layer of [`Walk::layers`], with a framebuffer of its own.
    Layer,
    /// A layer drawn straight into the one around it: opaque, or nested
    /// past [`MAX_LAYERS`].
    Through,
    /// A layer at opacity 0, whose commands draw nothing.
    Hidden,
}

/// Where a damage rect's walk of the list is.
struct Walk {
    /// The clips in device pixels, rounded, from the damage rect.
    clips: ClipStack,
    /// The clips in logical units, each the intersection of the rect
    /// pushed and the one below; the bottom one holds everything.
    logical: Vec<Rect>,
    /// What each open push opened.
    open: Vec<Open>,
    /// The layers with framebuffers of their own, innermost last.
    layers: Vec<Layer>,
    /// How many hidden layers are open.
    hidden: usize,
}

/// A layer with a framebuffer of its own: made when a command first inks
/// in it, and grown to cover what inks after.
struct Layer {
    /// What it holds; `None` until something inks in it.
    fb: Option<Framebuffer>,
    /// The alpha it is composited at.
    alpha: u8,
    /// The clip it was pushed in, which holds everything that inks in it.
    limit: DeviceRect,
}

/// What a command needs to be drawn: where it may ink, and a text's
/// layout.
struct Prepared {
    /// The device pixels it may ink: never empty.
    ink: DeviceRect,
    /// Its bounds cut to the clips in logical units and rounded out:
    /// [`Prepared::ink`] before the device clips cut it, and so the same
    /// in every damage rect.
    reach: DeviceRect,
    /// A text's line and `font` style; `None` for other commands, and for
    /// a text `font` cannot set.
    set: Option<(Line, Style)>,
}

/// A logical rect that holds every rect a list can usefully reach.
const EVERYTHING: Rect = Rect::new(-1e30, -1e30, 2e30, 2e30);

impl Painter {
    /// Loads the bundled faces, under 1 ms (M2.5), and makes an empty atlas
    /// [`ATLAS_SIDE`] pixels a side, with no icons or images.
    ///
    /// # Errors
    ///
    /// [`GuiError::Font`] if a bundled face does not parse or the atlas
    /// cannot be made, which would be a bug.
    pub fn new() -> Result<Self, GuiError> {
        Ok(Self {
            faces: bundled::load()?,
            atlas: Atlas::new(ATLAS_SIDE, ATLAS_SIDE)?,
            icons: BTreeMap::new(),
            images: BTreeMap::new(),
        })
    }

    /// The metrics to measure text with for frames painted at `scale`:
    /// what `Tree::frame` is given.
    pub const fn metrics(&self, scale: Scale) -> FontMetrics<'_> {
        FontMetrics::new(&self.faces, scale)
    }

    /// The faces text is set in.
    pub const fn faces(&self) -> &Faces {
        &self.faces
    }

    /// Registers `icon` for `id`, and returns the icon it replaces.
    ///
    /// `ui` does not know that a command drawing `id` now draws other
    /// pixels: the caller damages it (the [module
    /// documentation](self#damage)).
    pub fn set_icon(&mut self, id: IconId, icon: Icon) -> Option<Icon> {
        self.icons.insert(id, icon)
    }

    /// Removes the icon of `id`, and returns it. The caller damages what
    /// draws `id`, as for [`Painter::set_icon`].
    pub fn remove_icon(&mut self, id: IconId) -> Option<Icon> {
        self.icons.remove(&id)
    }

    /// The icon registered for `id`.
    pub fn icon(&self, id: IconId) -> Option<&Icon> {
        self.icons.get(&id)
    }

    /// Registers `image`, premultiplied, for `id`, and returns the image it
    /// replaces. Its pixels are drawn from its top-left corner, wherever
    /// its bounds are. The caller damages what draws `id`, as for
    /// [`Painter::set_icon`].
    pub fn set_image(&mut self, id: ImageId, image: Framebuffer) -> Option<Framebuffer> {
        self.images.insert(id, image)
    }

    /// Removes the image of `id`, and returns it. The caller damages what
    /// draws `id`, as for [`Painter::set_icon`].
    pub fn remove_image(&mut self, id: ImageId) -> Option<Framebuffer> {
        self.images.remove(&id)
    }

    /// The image registered for `id`.
    pub fn image(&self, id: ImageId) -> Option<&Framebuffer> {
        self.images.get(&id)
    }

    /// Paints `frame`, laid out at `scale`, into `target`: in each of its
    /// damage rects, made device pixels as the [module
    /// documentation](self#damage) says, `base` (at `target`'s top-left
    /// corner) and over it the frame's list. Returns the rects painted,
    /// the only pixels changed.
    ///
    /// # Errors
    ///
    /// [`GuiError::Gfx`] if a layer's framebuffer, or a tile a line or cut
    /// corner is filled in, cannot be allocated. The
    /// rects are then partly painted, and should be painted again whole:
    /// after `Tree::invalidate_all`, the next frame damages the window.
    pub fn paint(
        &mut self,
        target: &mut Framebuffer,
        base: &View<'_>,
        frame: &Frame,
        scale: Scale,
    ) -> Result<Damage, GuiError> {
        let bounds = target.bounds();
        let origin = DevicePoint::new(bounds.x, bounds.y);
        let size = base.size();
        let area = clip::intersect(
            bounds,
            DeviceRect::new(origin.x, origin.y, size.width, size.height),
        );
        let mut damage = Damage::new();
        for rect in &frame.damage {
            if let Some(rect) = damage_rect(*rect, scale) {
                damage.add(rect);
            }
        }
        damage.clip_to(area);
        for &rect in damage.rects() {
            image::copy(target, rect, base, origin);
            self.draw_list(target, rect, frame.list.commands(), scale)?;
        }
        Ok(damage)
    }

    /// Draws `commands` into `target`, clipped to `rect`.
    fn draw_list(
        &mut self,
        target: &mut Framebuffer,
        rect: DeviceRect,
        commands: &[Command],
        scale: Scale,
    ) -> Result<(), GuiError> {
        let mut walk = Walk {
            clips: ClipStack::new(rect),
            logical: vec![EVERYTHING],
            open: Vec::new(),
            layers: Vec::new(),
            hidden: 0,
        };
        for command in commands {
            match command {
                Command::PushClip { rect } => {
                    walk.clips.push(device_rect(*rect, scale));
                    let narrowed = intersect(walk.logical_clip(), *rect);
                    walk.logical.push(narrowed);
                    walk.open.push(Open::Clip);
                }
                Command::PushLayer { opacity } => walk.push_layer(*opacity),
                Command::PopClip => {
                    if walk.open.last() == Some(&Open::Clip) {
                        walk.close(target)?;
                    }
                }
                Command::PopLayer => {
                    if walk.open.last().is_some_and(|open| *open != Open::Clip) {
                        walk.close(target)?;
                    }
                }
                draw => {
                    if walk.hidden == 0 {
                        let (clip, logical) = (walk.clips.current(), walk.logical_clip());
                        if let Some(prepared) = self.prepare(draw, clip, logical, scale) {
                            let into = walk.target(prepared.ink, target)?;
                            self.draw(into, &prepared, draw, scale)?;
                        }
                    }
                }
            }
        }
        while !walk.open.is_empty() {
            walk.close(target)?;
        }
        Ok(())
    }

    /// Where `command`, neither a push nor a pop, may ink: inside `clip`
    /// (device pixels) and inside its bounds cut to `logical`, the clip in
    /// logical units; with a text's layout. `None` if that is nowhere, or
    /// the command has a NaN that its bounds do not show, a cut's.
    fn prepare(
        &self,
        command: &Command,
        clip: DeviceRect,
        logical: Rect,
        scale: Scale,
    ) -> Option<Prepared> {
        let metrics = FontMetrics::new(&self.faces, scale);
        let (set, measured) = match command {
            Command::CutCornerRect { cut, .. } if cut.is_nan() => return None,
            // A text is laid out only if its bounds, as wide as can be,
            // reach the clip.
            Command::Text { text, style, .. } => {
                let widest = Measured {
                    line: metrics.line(style),
                    width: MAX_EXTENT,
                };
                if clip::intersect(clip, reach(command, &widest, logical, scale)).is_empty() {
                    return None;
                }
                let set = metrics.set(text, style);
                let width = set.as_ref().map_or(0.0, |(line, _)| metrics.advance(line));
                (set, widest.with_width(width))
            }
            _ => (None, Measured::NONE),
        };
        let reach = reach(command, &measured, logical, scale);
        let ink = clip::intersect(clip, reach);
        (!ink.is_empty()).then_some(Prepared { ink, reach, set })
    }

    /// Draws `command`, neither a push nor a pop, into `fb`, inside
    /// `prepared.ink`.
    ///
    /// # Errors
    ///
    /// [`GuiError::Gfx`] if a tile of a path cannot be allocated.
    fn draw(
        &mut self,
        fb: &mut Framebuffer,
        prepared: &Prepared,
        command: &Command,
        scale: Scale,
    ) -> Result<(), GuiError> {
        let clip = prepared.ink;
        match command {
            Command::Rect { rect, color } => {
                raster::fill_rect(fb, clip, device_rect(*rect, scale), color.premultiply());
            }
            Command::CutCornerRect {
                rect,
                cut,
                corners,
                color,
            } => {
                let (rect, cut) = (device_rect(*rect, scale), device_f32(*cut, scale));
                let (corners, color) = (gfx_corners(*corners), color.premultiply());
                if cut > 0.0 && corners != raster::Corners::NONE {
                    // Filled as a path: in tiles, those clear of the cuts
                    // solid.
                    let kind = |tile| cut_corner_tile(tile, rect, cut, corners);
                    fill_in_tiles(fb, (clip, prepared.reach), kind, color, |tile, window| {
                        raster::cut_corner_rect(tile, window, rect, cut, corners, color);
                    })?;
                } else {
                    raster::cut_corner_rect(fb, clip, rect, cut, corners, color);
                }
            }
            Command::Pill { rect, color } => {
                raster::pill(fb, clip, device_rect(*rect, scale), color.premultiply());
            }
            Command::Border { rect, width, color } => raster::border(
                fb,
                clip,
                device_rect(*rect, scale),
                border_width(*width, scale),
                color.premultiply(),
            ),
            Command::Brackets {
                rect,
                arm,
                thickness,
                corners,
                color,
            } => raster::brackets(
                fb,
                clip,
                device_rect(*rect, scale),
                device_length(*arm, scale),
                border_width(*thickness, scale),
                gfx_corners(*corners),
                color.premultiply(),
            ),
            Command::Line {
                from,
                to,
                width,
                color,
            } => {
                let (from, to) = (device_point(*from, scale), device_point(*to, scale));
                let (width, color) = (device_f32(*width, scale), color.premultiply());
                // Only the tiles the line comes near.
                let kind = |tile| {
                    let margin = f64::from(width) / 2.0 + 1.0;
                    if segment_reaches(from, to, tile, margin) {
                        Tile::Path
                    } else {
                        Tile::Empty
                    }
                };
                fill_in_tiles(fb, (clip, prepared.reach), kind, color, |tile, window| {
                    raster::line(tile, window, from, to, width, color);
                })?;
            }
            Command::Text { origin, color, .. } => {
                if let Some((line, style)) = &prepared.set {
                    self.draw_text(fb, clip, (line, style), *origin, scale, *color);
                }
            }
            Command::Icon { rect, icon, color } => {
                let rect = device_rect(*rect, scale);
                draw_icon(fb, clip, rect, self.icons.get(icon), color.premultiply());
            }
            Command::Image { rect, image } => {
                if let Some(image) = self.images.get(image) {
                    let rect = device_rect(*rect, scale);
                    image::blit_scaled(fb, clip, &image.view(), rect, Filter::Bilinear);
                }
            }
            Command::PushClip { .. }
            | Command::PopClip
            | Command::PushLayer { .. }
            | Command::PopLayer => {}
        }
        Ok(())
    }

    /// Draws `line`, laid out in `style`, from the pen at `origin` (logical
    /// units), in `color`, inside `clip`.
    fn draw_text(
        &mut self,
        fb: &mut Framebuffer,
        clip: DeviceRect,
        (line, style): (&Line, &Style),
        origin: Point,
        scale: Scale,
        color: Color,
    ) {
        let factor = f64::from(scale.factor());
        // The pen's start in device pixels; `as` saturates.
        let x = (f64::from(origin.x) * factor) as f32;
        let baseline = snap(f64::from(origin.y) * factor);
        let color = color.premultiply();
        for placed in &line.glyphs {
            // A tab draws nothing.
            let Some(id) = placed.glyph else {
                continue;
            };
            let (pixel, subpixel) = Subpixel::split(x + placed.x);
            let key = GlyphKey {
                face: placed.face,
                glyph: id,
                size: style.size,
                subpixel,
            };
            if let Ok(image) = self.atlas.glyph(&self.faces, key) {
                let atlas = Mask::new(
                    image.pixels(),
                    DeviceSize::new(side(image.atlas_width()), side(image.atlas_height())),
                    image.atlas_width() as usize,
                );
                let region = DeviceRect::new(
                    side(image.x),
                    side(image.y),
                    side(image.width),
                    side(image.height),
                );
                if let Ok(mask) = atlas.and_then(|atlas| atlas.region(region)) {
                    let at = DevicePoint::new(
                        pixel.saturating_add(image.left),
                        baseline.saturating_add(image.top),
                    );
                    glyph::draw(fb, clip, &mask, at, color);
                }
            } else if let Ok(bitmap) = atlas::rasterize(&self.faces, key) {
                // Too large for the atlas: drawn on its own.
                let at = DevicePoint::new(pixel, baseline);
                draw_bitmap(fb, clip, &bitmap, at, color);
            }
        }
    }
}

impl Walk {
    /// The clip in logical units.
    fn logical_clip(&self) -> Rect {
        self.logical.last().copied().unwrap_or(EVERYTHING)
    }

    /// Opens a layer at `opacity`, as the [module
    /// documentation](self#clips-and-layers) says. Its framebuffer is made
    /// when something first inks in it.
    fn push_layer(&mut self, opacity: f32) {
        let alpha = opacity_byte(opacity);
        let open = if alpha == 0 {
            self.hidden += 1;
            Open::Hidden
        } else if self.hidden > 0 || alpha == u8::MAX || self.layers.len() >= MAX_LAYERS {
            // Inside a hidden layer nothing draws, so no framebuffer is
            // needed.
            Open::Through
        } else {
            self.layers.push(Layer {
                fb: None,
                alpha,
                limit: self.clips.current(),
            });
            Open::Layer
        };
        self.open.push(open);
    }

    /// What a command inking `ink` draws into: the innermost layer, made or
    /// grown to cover it, or `target`.
    fn target<'a>(
        &'a mut self,
        ink: DeviceRect,
        target: &'a mut Framebuffer,
    ) -> Result<&'a mut Framebuffer, GuiError> {
        match self.layers.last_mut() {
            Some(layer) => layer.cover(ink),
            None => Ok(target),
        }
    }

    /// Closes the innermost push: pops a clip, or composites a layer into
    /// the one around it, or into `target`.
    fn close(&mut self, target: &mut Framebuffer) -> Result<(), GuiError> {
        match self.open.pop() {
            Some(Open::Clip) => {
                self.clips.pop();
                self.logical.pop();
            }
            Some(Open::Layer) => {
                if let Some(Layer {
                    fb: Some(layer),
                    alpha,
                    ..
                }) = self.layers.pop()
                {
                    // Inked inside the clips around it, and so inside the
                    // clip here, which is the one it was pushed in.
                    let region = clip::intersect(layer.bounds(), self.clips.current());
                    if !region.is_empty() {
                        let into = self.target(region, target)?;
                        blend::composite(into, region, &layer, alpha);
                    }
                }
            }
            Some(Open::Hidden) => self.hidden = self.hidden.saturating_sub(1),
            Some(Open::Through) | None => {}
        }
        Ok(())
    }
}

impl Layer {
    /// Its framebuffer, made or grown to cover `rect`, with what it holds
    /// kept. It grows by at least its own width or height in each
    /// direction it must, up to its limit, so that a layer inked row after
    /// row is copied a few times, not once a row.
    fn cover(&mut self, rect: DeviceRect) -> Result<&mut Framebuffer, GuiError> {
        let fb = match self.fb.take() {
            None => Framebuffer::new_at(rect)?,
            Some(fb) if clip::intersect(fb.bounds(), rect) == rect => fb,
            Some(old) => {
                let bounds = old.bounds();
                let grown = grow(bounds, bounding(bounds, rect));
                // Inked inside the limit, as `rect` is.
                let grown = bounding(clip::intersect(grown, self.limit), rect);
                let mut grown = Framebuffer::new_at(grown)?;
                image::copy(
                    &mut grown,
                    bounds,
                    &old.view(),
                    DevicePoint::new(bounds.x, bounds.y),
                );
                grown
            }
        };
        Ok(self.fb.insert(fb))
    }
}

/// Line metrics and a width known already, as a [`TextMetrics`], so that a
/// text's [`Command::bounds`] are `ui`'s, from one layout.
#[derive(Clone, Copy)]
struct Measured {
    line: LineMetrics,
    width: f32,
}

impl Measured {
    /// For commands other than text, which need no metrics.
    const NONE: Self = Self {
        line: LineMetrics {
            ascent: 0.0,
            descent: 0.0,
            height: 0.0,
        },
        width: 0.0,
    };

    const fn with_width(self, width: f32) -> Self {
        Self { width, ..self }
    }
}

impl TextMetrics for Measured {
    fn line(&self, _style: &TextStyle) -> LineMetrics {
        self.line
    }

    fn width(&self, _text: &str, _style: &TextStyle) -> f32 {
        self.width
    }
}

/// Where `command` may draw whatever the damage: its bounds (measured with
/// `metrics`) cut to `logical`, the clip in logical units, and rounded out
/// to whole device pixels; empty for a command with no bounds, or bounds
/// with a NaN, which `ui` damages nothing for.
fn reach(command: &Command, metrics: &dyn TextMetrics, logical: Rect, scale: Scale) -> DeviceRect {
    match command.bounds(metrics) {
        Some(bounds) if !has_nan(bounds) => {
            outward(intersect(bounds, logical), scale).unwrap_or(DeviceRect::ZERO)
        }
        _ => DeviceRect::ZERO,
    }
}

/// Returns `true` if a component of `rect` is NaN.
fn has_nan(rect: Rect) -> bool {
    rect.x.is_nan() || rect.y.is_nan() || rect.width.is_nan() || rect.height.is_nan()
}

/// The side of the tiles, on a grid fixed in device space, that a path is
/// filled in: [`fill_in_tiles`].
const PATH_TILE: i64 = 64;

/// What a path is in one of [`fill_in_tiles`]' tiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tile {
    /// It covers none of the tile's pixels.
    Empty,
    /// It covers every pixel of the tile whole.
    Solid,
    /// It may cover some pixels in part.
    Path,
}

/// Fills a path with `fill`, a `gfx` call given a framebuffer and its
/// clip, into `fb` inside `clip ∩ reach` (`area`): one tile of a grid fixed
/// in device space at a time. A tile `kind` says is [`Tile::Path`] is
/// filled into a framebuffer of its own covering the tile cut to `reach`,
/// then composited into `fb`; a [`Tile::Solid`] one is filled with
/// `color`, and an [`Tile::Empty`] one skipped.
///
/// `gfx` sums a path's coverage across the part of the path inside its
/// clip, from that part's corner, so where the clip's edges fall can move
/// a pixel's coverage by one level. A tile and `reach` are the same in
/// every damage rect, and so then is the coverage: painting the damage
/// gives what painting the whole frame gives. Blending a path's pixels
/// over a transparent tile and the tile over `fb` at full opacity is
/// exactly blending them over `fb`; so is filling a pixel the path covers
/// whole, and skipping one it covers none of.
///
/// # Errors
///
/// [`GuiError::Gfx`] if a tile cannot be allocated.
fn fill_in_tiles(
    fb: &mut Framebuffer,
    (clip, reach): (DeviceRect, DeviceRect),
    kind: impl Fn(DeviceRect) -> Tile,
    color: PremultipliedColor,
    fill: impl Fn(&mut Framebuffer, DeviceRect),
) -> Result<(), GuiError> {
    let area = clip::intersect(clip, reach);
    if area.is_empty() {
        return Ok(());
    }
    let (left, top) = (i64::from(area.x), i64::from(area.y));
    let (right, bottom) = (left + i64::from(area.width), top + i64::from(area.height));
    let mut y = top.div_euclid(PATH_TILE) * PATH_TILE;
    while y < bottom {
        let mut x = left.div_euclid(PATH_TILE) * PATH_TILE;
        while x < right {
            let tile = from_edges(x, y, x + PATH_TILE, y + PATH_TILE);
            let window = clip::intersect(tile, reach);
            if !window.is_empty() {
                match kind(tile) {
                    Tile::Empty => {}
                    Tile::Solid => {
                        raster::fill_rect(fb, clip::intersect(window, clip), window, color);
                    }
                    Tile::Path => {
                        let mut scratch = Framebuffer::new_at(window)?;
                        fill(&mut scratch, window);
                        blend::composite(fb, clip::intersect(window, clip), &scratch, u8::MAX);
                    }
                }
            }
            x += PATH_TILE;
        }
        y += PATH_TILE;
    }
    Ok(())
}

/// Draws `icon` (or, with none, a `.notdef` box) into `rect`, inside
/// `clip`.
fn draw_icon(
    fb: &mut Framebuffer,
    clip: DeviceRect,
    rect: DeviceRect,
    icon: Option<&Icon>,
    color: PremultipliedColor,
) {
    let clip = clip::intersect(clip, rect);
    let Some(icon) = icon else {
        raster::border(fb, clip, rect, 1, color);
        return;
    };
    let side = rect.width.min(rect.height);
    let Some(bitmap) = icon.rasterize(side) else {
        return;
    };
    // At most `i32::MAX`, and halved: the sums stay in range.
    let left = rect.x.saturating_add((rect.width - side) / 2);
    let top = rect.y.saturating_add((rect.height - side) / 2);
    draw_bitmap(fb, clip, &bitmap, DevicePoint::new(left, top), color);
}

/// Draws `bitmap`'s coverage in `color`, its origin at `at`, inside `clip`.
fn draw_bitmap(
    fb: &mut Framebuffer,
    clip: DeviceRect,
    bitmap: &Bitmap,
    at: DevicePoint,
    color: PremultipliedColor,
) {
    let size = DeviceSize::new(side(bitmap.width), side(bitmap.height));
    let Ok(mask) = Mask::new(&bitmap.pixels, size, bitmap.width as usize) else {
        return;
    };
    let at = DevicePoint::new(
        at.x.saturating_add(bitmap.left),
        at.y.saturating_add(bitmap.top),
    );
    glyph::draw(fb, clip, &mask, at, color);
}

/// A logical damage rect in device pixels, rounded out to whole pixels;
/// `None` if it has a NaN in it.
fn damage_rect(rect: Rect, scale: Scale) -> Option<DeviceRect> {
    outward(rect, scale)
}

/// The edges of `rect` times `scale`, in `f64`: the right edge and the
/// bottom are summed in `f32` first, as `ui` places a rect's neighbour.
fn edges(rect: Rect, scale: Scale) -> [f64; 4] {
    let factor = f64::from(scale.factor());
    [
        f64::from(rect.x) * factor,
        f64::from(rect.y) * factor,
        f64::from(rect.x + rect.width) * factor,
        f64::from(rect.y + rect.height) * factor,
    ]
}

/// `rect` times `scale`, its edges rounded out to whole pixels, within
/// `gfx`'s device space; empty if `rect` is, and `None` if it has a NaN in
/// it.
fn outward(rect: Rect, scale: Scale) -> Option<DeviceRect> {
    let [left, top, right, bottom] = edges(rect, scale);
    if [left, top, right, bottom].iter().any(|edge| edge.is_nan()) {
        return None;
    }
    // A rect with no area covers no pixel, however its edges round.
    if right <= left || bottom <= top {
        return Some(DeviceRect::ZERO);
    }
    // `as` saturates.
    Some(from_edges(
        left.floor() as i64,
        top.floor() as i64,
        right.ceil() as i64,
        bottom.ceil() as i64,
    ))
}

/// `rect` times `scale`, each edge rounded to the nearest device pixel,
/// half-way up; empty if it has a NaN in it.
fn device_rect(rect: Rect, scale: Scale) -> DeviceRect {
    let [left, top, right, bottom] = edges(rect, scale);
    if [left, top, right, bottom].iter().any(|edge| edge.is_nan()) {
        return DeviceRect::ZERO;
    }
    let edge = |value: f64| i64::from(snap(value));
    from_edges(edge(left), edge(top), edge(right), edge(bottom))
}

/// The rect from `left` to `right` and `top` to `bottom`, edges within
/// `gfx`'s device space; empty when `right <= left` or `bottom <= top`.
fn from_edges(left: i64, top: i64, right: i64, bottom: i64) -> DeviceRect {
    let (left, top, right, bottom) = (clamp(left), clamp(top), clamp(right), clamp(bottom));
    // Each edge is within ±2^30, so each side is within 2^31 and fits once
    // clamped to `i32::MAX`.
    let side = |from: i64, to: i64| i32::try_from((to - from).max(0)).unwrap_or(i32::MAX);
    DeviceRect::new(
        i32::try_from(left).unwrap_or(0),
        i32::try_from(top).unwrap_or(0),
        side(left, right),
        side(top, bottom),
    )
}

/// `value` within `gfx`'s device space, from −2³⁰ to 2³⁰.
fn clamp(value: i64) -> i64 {
    let low = i64::from(SPACE.x);
    value.clamp(low, -low)
}

/// `value` rounded to the nearest whole pixel, half-way up, within `gfx`'s
/// device space; NaN is 0.
fn snap(value: f64) -> i32 {
    // `as` saturates, and NaN becomes 0.
    let rounded = (value + 0.5).floor() as i64;
    i32::try_from(clamp(rounded)).unwrap_or(0)
}

/// A border's width or a bracket's thickness in device pixels, snapped as
/// CSS snaps a border width: 0 for none (0 or less, or NaN), 1 for less
/// than one device pixel, else floored.
fn border_width(width: f32, scale: Scale) -> i32 {
    let device = f64::from(width) * f64::from(scale.factor());
    if device.is_nan() || device <= 0.0 {
        0
    } else if device < 1.0 {
        1
    } else {
        // `as` saturates.
        device.floor().min(f64::from(i32::MAX)) as i32
    }
}

/// A length in device pixels, rounded to the nearest, half-way up; 0 for
/// NaN.
fn device_length(length: f32, scale: Scale) -> i32 {
    snap(f64::from(length) * f64::from(scale.factor()))
}

/// A length in device pixels as an `f32`, for what `gfx` anti-aliases.
fn device_f32(length: f32, scale: Scale) -> f32 {
    // `as` rounds to nearest, and gives infinity past `f32`'s range: `gfx`
    // draws a line of infinite width or at an infinite point as nothing,
    // and shortens an infinite cut to half the rect's shorter side.
    (f64::from(length) * f64::from(scale.factor())) as f32
}

/// A point in device pixels, for what `gfx` anti-aliases.
fn device_point(point: Point, scale: Scale) -> Vertex {
    Vertex::new(device_f32(point.x, scale), device_f32(point.y, scale))
}

/// A layer's opacity as an alpha: `round(opacity × 255)`, 0 to 1 clamped,
/// NaN as 1.
fn opacity_byte(opacity: f32) -> u8 {
    if opacity.is_nan() {
        return u8::MAX;
    }
    let clamped = f64::from(opacity).clamp(0.0, 1.0);
    // From 0.5 to 255.5, floored: 0 to 255.
    (clamped * 255.0 + 0.5).floor() as u8
}

/// `ui`'s corners as `gfx`'s.
const fn gfx_corners(corners: Corners) -> raster::Corners {
    raster::Corners {
        top_left: corners.top_left,
        top_right: corners.top_right,
        bottom_right: corners.bottom_right,
        bottom_left: corners.bottom_left,
    }
}

/// What a cut-corner rect `rect`, cut `cut` device pixels at `corners`, is
/// in `tile`: [`Tile::Solid`] inside the rect and clear of each cut
/// corner's square, whose pixels it covers whole; [`Tile::Empty`] outside
/// the rect; else [`Tile::Path`]. The cut is shortened as `gfx` shortens
/// it, to half the rect's shorter side.
fn cut_corner_tile(tile: DeviceRect, rect: DeviceRect, cut: f32, corners: raster::Corners) -> Tile {
    if clip::intersect(tile, rect).is_empty() {
        return Tile::Empty;
    }
    if clip::intersect(tile, rect) != tile {
        return Tile::Path;
    }
    let most = f64::from(rect.width.min(rect.height)) / 2.0;
    // At most 2^30: an `i32`. NaN cuts are never drawn.
    let side = f64::from(cut).min(most).ceil() as i32;
    let (left, top) = (rect.x, rect.y);
    let (right, bottom) = (
        rect.x.saturating_add(rect.width),
        rect.y.saturating_add(rect.height),
    );
    let squares = [
        (corners.top_left, left, top),
        (corners.top_right, right.saturating_sub(side), top),
        (
            corners.bottom_right,
            right.saturating_sub(side),
            bottom.saturating_sub(side),
        ),
        (corners.bottom_left, left, bottom.saturating_sub(side)),
    ];
    let clear = squares.iter().all(|&(cut, x, y)| {
        !cut || clip::intersect(tile, DeviceRect::new(x, y, side, side)).is_empty()
    });
    if clear { Tile::Solid } else { Tile::Path }
}

/// Returns `true` if the segment from `from` to `to` comes within `margin`
/// device pixels of `rect`, along each axis: false only when every point
/// of it is further than that from the rect, and so a line along it at
/// most `2 × (margin − 1)` wide covers none of the rect's pixels. A
/// segment with a coordinate that is not finite reaches nothing, as `gfx`
/// draws it as nothing.
fn segment_reaches(from: Vertex, to: Vertex, rect: DeviceRect, margin: f64) -> bool {
    let (x0, y0) = (f64::from(from.x), f64::from(from.y));
    let (x1, y1) = (f64::from(to.x), f64::from(to.y));
    if ![x0, y0, x1, y1, margin]
        .iter()
        .all(|value| value.is_finite())
    {
        return false;
    }
    let left = f64::from(rect.x) - margin;
    let top = f64::from(rect.y) - margin;
    let right = f64::from(rect.x) + f64::from(rect.width) + margin;
    let bottom = f64::from(rect.y) + f64::from(rect.height) + margin;
    // Liang–Barsky: the part of the segment, from `t0` to `t1`, inside the
    // grown rect.
    let (dx, dy) = (x1 - x0, y1 - y0);
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    for (p, q) in [
        (-dx, x0 - left),
        (dx, right - x0),
        (-dy, y0 - top),
        (dy, bottom - y0),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return false;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
        }
    }
    t0 <= t1
}

/// `wanted`, which holds `old`, grown on each side that it reaches past
/// `old` by at least `old`'s width or height, within `gfx`'s device space.
fn grow(old: DeviceRect, wanted: DeviceRect) -> DeviceRect {
    let edges = |rect: DeviceRect| {
        let (x, y) = (i64::from(rect.x), i64::from(rect.y));
        (x, y, x + i64::from(rect.width), y + i64::from(rect.height))
    };
    let (ol, ot, or, ob) = edges(old);
    let (wl, wt, wr, wb) = edges(wanted);
    let (width, height) = (or - ol, ob - ot);
    from_edges(
        if wl < ol { wl.min(ol - width) } else { wl },
        if wt < ot { wt.min(ot - height) } else { wt },
        if wr > or { wr.max(or + width) } else { wr },
        if wb > ob { wb.max(ob + height) } else { wb },
    )
}

/// The smallest device rect holding both, which are inside `gfx`'s device
/// space.
fn bounding(a: DeviceRect, b: DeviceRect) -> DeviceRect {
    let edge = |rect: DeviceRect| {
        let (x, y) = (i64::from(rect.x), i64::from(rect.y));
        (x, y, x + i64::from(rect.width), y + i64::from(rect.height))
    };
    let (al, at, ar, ab) = edge(a);
    let (bl, bt, br, bb) = edge(b);
    from_edges(al.min(bl), at.min(bt), ar.max(br), ab.max(bb))
}

/// The intersection of two logical rects; empty (zero-sized at the larger
/// origin) when they do not overlap.
fn intersect(a: Rect, b: Rect) -> Rect {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = a.right().min(b.right());
    let bottom = a.bottom().min(b.bottom());
    Rect::new(x, y, (right - x).max(0.0), (bottom - y).max(0.0))
}

/// A place or a side in the atlas or a bitmap, as `gfx` takes it: at most
/// 4,096 px, so it fits; anything else makes the mask refuse it.
fn side(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests;
