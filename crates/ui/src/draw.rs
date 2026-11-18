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

//! Draw lists and damage: the [`Command`]s widgets paint into a
//! [`DrawList`], and [`Tree::frame`], which repaints what changed and
//! returns the whole tree's list with the rects it damaged, as a
//! [`Frame`].
//!
//! New in Tidepool; not ported.
//!
//! Commands are in logical units and window coordinates, and carry
//! resolved colors, straight-alpha [`Color`]s (ADR-0046): a widget paints
//! a token's value from the tree's theme, which [`PaintContext::theme`]
//! gives it (ADR-0049). `gui` paints a list
//! with `gfx` and `font` (M3.5); `tui` maps it onto cells (M7.1).
//!
//! # Fragments and damage
//!
//! Each node keeps the commands its widget painted last, its *fragment*,
//! and their *painted bounds*: the union of the commands'
//! [`bounds`](Command::bounds), each cut to the node's clip (its clipping
//! ancestors' bounds, not its own), to the clips the fragment pushed
//! around it, and to the window. A frame repaints only
//! the nodes marked for it, and damages a node's old and new painted
//! bounds when its fragment or those bounds changed. Nodes are marked by
//! being inserted, a [`Tree::widget_mut`], a handler's request, a change
//! of their bounds, clip, content size or scroll offset in layout, a
//! change of their [`NodeState`], and [`Tree::set_enabled`];
//! [`Tree::set_style`] damages its subtree's painted bounds whether they
//! changed or not. Removing a node damages what it painted; however many
//! nodes come and go between two frames, what waits for the next stays
//! bounded (see [`Tree::remove`]). The first
//! frame, a new window size and [`Tree::invalidate_all`] damage the whole
//! window. An idle tree damages nothing, and its frame is `None`.
//!
//! The bounds of a command are geometric. A frontend that anti-aliases
//! keeps each command's ink inside its bounds cut to the clips around it:
//! growing the damage alone would miss the ink of a command a clip cuts
//! off, which is damaged for nothing. `gui` clips each command's ink to
//! its bounds, cut to the clips around it and rounded out to whole device
//! pixels (M3.5, ADR-0048).
//!
//! # Sanitizing
//!
//! A fragment is made sane before it is kept, so that a frame's list and
//! its damage agree. First its pushes and pops are paired: a pop that ends
//! no push is dropped, and pushes still open are closed. Then a command
//! with a NaN in its geometry (a rect, a cut, a width, an arm or
//! thickness, a point, a text's origin or size) is dropped, and a
//! [`Command::PushClip`] with one is dropped together with the
//! [`Command::PopClip`] that ends it, so that what it held stays inside
//! every clip around it; and a [`Command::PushLayer`]'s opacity is clamped
//! to 0 to 1, NaN being 1, as a [`Style`]'s is.
//!
//! # Text in `Debug`
//!
//! A [`Command::Text`] holds what a widget shows, which can be what the
//! user typed or pasted, so its `Debug` shows the text as its length only,
//! as [`TextLength`] does (ADR-0043); a [`DrawList`]'s and a [`Frame`]'s
//! show it so too.
//!
//! [`Style`]: crate::layout::Style

use std::fmt;

use tidepool_core::color::Color;
use tidepool_core::geom::{Point, Rect, Size};
use tidepool_core::input::TextLength;

use crate::layout::{intersect, sane_size};
use crate::metrics::{TextMetrics, TextStyle};
use crate::theme::Theme;
use crate::tree::{NodeId, NodeState, Tree};

/// The most damage rects a [`Frame`] has; more become their bounding rect.
pub const MAX_DAMAGE_RECTS: usize = 32;

/// A choice of a rect's corners, for [`Command::CutCornerRect`] and
/// [`Command::Brackets`]; the same as `gfx`'s.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Corners {
    /// The top-left corner.
    pub top_left: bool,
    /// The top-right corner.
    pub top_right: bool,
    /// The bottom-right corner.
    pub bottom_right: bool,
    /// The bottom-left corner.
    pub bottom_left: bool,
}

impl Corners {
    /// Every corner.
    pub const ALL: Self = Self {
        top_left: true,
        top_right: true,
        bottom_right: true,
        bottom_left: true,
    };

    /// No corner.
    pub const NONE: Self = Self {
        top_left: false,
        top_right: false,
        bottom_right: false,
        bottom_left: false,
    };

    /// Top-left and bottom-right: the bracket corners of a focused panel
    /// (`.panel--brackets`).
    pub const TOP_LEFT_AND_BOTTOM_RIGHT: Self = Self {
        top_left: true,
        top_right: false,
        bottom_right: true,
        bottom_left: false,
    };

    /// Top-right and bottom-left: the cut corners of the amber primary
    /// button (`.btn--amber`).
    pub const TOP_RIGHT_AND_BOTTOM_LEFT: Self = Self {
        top_left: false,
        top_right: true,
        bottom_right: false,
        bottom_left: true,
    };
}

/// An icon, by its id in Tidepool's icon set. The ids are assigned by the
/// icon set in `assets/icons` (`docs/01-architecture.md` → "Workspace
/// layout"), which no roadmap task builds yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IconId(pub u16);

/// An image, by a handle the frontend resolves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ImageId(pub u32);

/// One drawing command, in logical units and window coordinates.
///
/// Not non-exhaustive: every frontend handles each command. A command with
/// a NaN in its geometry never reaches a [`Frame`]: the tree drops it from
/// the widget's fragment (see [Sanitizing](self#sanitizing)). Its `Debug`
/// shows a [`Command::Text`]'s text as its length only (ADR-0043).
#[derive(Clone, PartialEq)]
pub enum Command {
    /// A filled rect.
    Rect {
        /// Where.
        rect: Rect,
        /// Its color.
        color: Color,
    },
    /// A filled rect with some corners cut at 45°.
    CutCornerRect {
        /// Where.
        rect: Rect,
        /// How far along each edge a cut goes.
        cut: f32,
        /// Which corners are cut.
        corners: Corners,
        /// Its color.
        color: Color,
    },
    /// A fully rounded rect: a chip, the search field, a status dot.
    Pill {
        /// Where.
        rect: Rect,
        /// Its color.
        color: Color,
    },
    /// A border inside `rect`.
    Border {
        /// Its outer edge.
        rect: Rect,
        /// Its width.
        width: f32,
        /// Its color.
        color: Color,
    },
    /// Bracket corners inside `rect`.
    Brackets {
        /// Its outer edge.
        rect: Rect,
        /// How far each arm reaches along an edge.
        arm: f32,
        /// How thick an arm is.
        thickness: f32,
        /// Which corners have one.
        corners: Corners,
        /// Their color.
        color: Color,
    },
    /// A straight line.
    Line {
        /// One end.
        from: Point,
        /// The other end.
        to: Point,
        /// Its width.
        width: f32,
        /// Its color.
        color: Color,
    },
    /// A run of text on one line.
    Text {
        /// The pen's start, on the baseline.
        origin: Point,
        /// The text.
        text: String,
        /// How it is set.
        style: TextStyle,
        /// Its color.
        color: Color,
    },
    /// An icon in a color.
    Icon {
        /// Where.
        rect: Rect,
        /// Which.
        icon: IconId,
        /// Its color.
        color: Color,
    },
    /// An image, scaled to `rect`.
    Image {
        /// Where.
        rect: Rect,
        /// Which.
        image: ImageId,
    },
    /// Clips what follows, up to the matching [`Command::PopClip`], to
    /// `rect` and the clips around it.
    PushClip {
        /// The clip.
        rect: Rect,
    },
    /// Ends the last [`Command::PushClip`].
    PopClip,
    /// Draws what follows, up to the matching [`Command::PopLayer`], as a
    /// layer, composited at `opacity`.
    PushLayer {
        /// The layer's opacity, 0 to 1.
        opacity: f32,
    },
    /// Ends the last [`Command::PushLayer`].
    PopLayer,
}

/// As a derive would write it, but for a text's text, which shows as its
/// length.
impl fmt::Debug for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rect { rect, color } => f
                .debug_struct("Rect")
                .field("rect", rect)
                .field("color", color)
                .finish(),
            Self::CutCornerRect {
                rect,
                cut,
                corners,
                color,
            } => f
                .debug_struct("CutCornerRect")
                .field("rect", rect)
                .field("cut", cut)
                .field("corners", corners)
                .field("color", color)
                .finish(),
            Self::Pill { rect, color } => f
                .debug_struct("Pill")
                .field("rect", rect)
                .field("color", color)
                .finish(),
            Self::Border { rect, width, color } => f
                .debug_struct("Border")
                .field("rect", rect)
                .field("width", width)
                .field("color", color)
                .finish(),
            Self::Brackets {
                rect,
                arm,
                thickness,
                corners,
                color,
            } => f
                .debug_struct("Brackets")
                .field("rect", rect)
                .field("arm", arm)
                .field("thickness", thickness)
                .field("corners", corners)
                .field("color", color)
                .finish(),
            Self::Line {
                from,
                to,
                width,
                color,
            } => f
                .debug_struct("Line")
                .field("from", from)
                .field("to", to)
                .field("width", width)
                .field("color", color)
                .finish(),
            Self::Text {
                origin,
                text,
                style,
                color,
            } => f
                .debug_struct("Text")
                .field("origin", origin)
                .field("text", &TextLength(text))
                .field("style", style)
                .field("color", color)
                .finish(),
            Self::Icon { rect, icon, color } => f
                .debug_struct("Icon")
                .field("rect", rect)
                .field("icon", icon)
                .field("color", color)
                .finish(),
            Self::Image { rect, image } => f
                .debug_struct("Image")
                .field("rect", rect)
                .field("image", image)
                .finish(),
            Self::PushClip { rect } => f.debug_struct("PushClip").field("rect", rect).finish(),
            Self::PopClip => f.write_str("PopClip"),
            Self::PushLayer { opacity } => f
                .debug_struct("PushLayer")
                .field("opacity", opacity)
                .finish(),
            Self::PopLayer => f.write_str("PopLayer"),
        }
    }
}

impl Command {
    /// The area the command draws in, geometrically; `None` for the
    /// pushes and pops.
    ///
    /// A shape's is its rect. A line's is the box of its ends grown by half
    /// its width. A text's is its line box, from the origin to its width and
    /// from its ascent above the baseline to its descent below, as
    /// `metrics` gives them, grown by half an em on every side for ink
    /// beyond the advances (italics, bearings). A width or size that is not
    /// finite or is negative counts as 0.
    pub fn bounds(&self, metrics: &dyn TextMetrics) -> Option<Rect> {
        match self {
            Self::Rect { rect, .. }
            | Self::CutCornerRect { rect, .. }
            | Self::Pill { rect, .. }
            | Self::Border { rect, .. }
            | Self::Brackets { rect, .. }
            | Self::Icon { rect, .. }
            | Self::Image { rect, .. } => Some(*rect),
            Self::Line {
                from, to, width, ..
            } => {
                let half = non_negative(*width) / 2.0;
                let left = from.x.min(to.x);
                let top = from.y.min(to.y);
                Some(Rect::new(
                    left - half,
                    top - half,
                    (from.x - to.x).abs() + 2.0 * half,
                    (from.y - to.y).abs() + 2.0 * half,
                ))
            }
            Self::Text {
                origin,
                text,
                style,
                ..
            } => {
                let line = metrics.line(style);
                let width = metrics.width(text, style);
                let half = non_negative(style.size) / 2.0;
                Some(Rect::new(
                    origin.x - half,
                    origin.y - line.ascent - half,
                    width + 2.0 * half,
                    line.ascent + line.descent + 2.0 * half,
                ))
            }
            Self::PushClip { .. } | Self::PopClip | Self::PushLayer { .. } | Self::PopLayer => None,
        }
    }
}

/// `value` if it is finite and positive, else 0.
fn non_negative(value: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        0.0
    }
}

/// A list of [`Command`]s, drawn in order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawList {
    commands: Vec<Command>,
}

impl DrawList {
    /// An empty list.
    pub const fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    /// The commands, in order.
    pub fn commands(&self) -> &[Command] {
        &self.commands
    }

    /// How many commands there are.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Returns `true` if there are none.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Removes every command.
    pub fn clear(&mut self) {
        self.commands.clear();
    }

    /// Adds a command at the end.
    pub fn push(&mut self, command: Command) {
        self.commands.push(command);
    }

    /// Adds a [`Command::Rect`].
    pub fn rect(&mut self, rect: Rect, color: Color) {
        self.push(Command::Rect { rect, color });
    }

    /// Adds a [`Command::CutCornerRect`].
    pub fn cut_corner_rect(&mut self, rect: Rect, cut: f32, corners: Corners, color: Color) {
        self.push(Command::CutCornerRect {
            rect,
            cut,
            corners,
            color,
        });
    }

    /// Adds a [`Command::Pill`].
    pub fn pill(&mut self, rect: Rect, color: Color) {
        self.push(Command::Pill { rect, color });
    }

    /// Adds a [`Command::Border`].
    pub fn border(&mut self, rect: Rect, width: f32, color: Color) {
        self.push(Command::Border { rect, width, color });
    }

    /// Adds a [`Command::Brackets`].
    pub fn brackets(
        &mut self,
        rect: Rect,
        arm: f32,
        thickness: f32,
        corners: Corners,
        color: Color,
    ) {
        self.push(Command::Brackets {
            rect,
            arm,
            thickness,
            corners,
            color,
        });
    }

    /// Adds a [`Command::Line`].
    pub fn line(&mut self, from: Point, to: Point, width: f32, color: Color) {
        self.push(Command::Line {
            from,
            to,
            width,
            color,
        });
    }

    /// Adds a [`Command::Text`].
    pub fn text(&mut self, origin: Point, text: impl Into<String>, style: TextStyle, color: Color) {
        self.push(Command::Text {
            origin,
            text: text.into(),
            style,
            color,
        });
    }

    /// Adds a [`Command::Icon`].
    pub fn icon(&mut self, rect: Rect, icon: IconId, color: Color) {
        self.push(Command::Icon { rect, icon, color });
    }

    /// Adds a [`Command::Image`].
    pub fn image(&mut self, rect: Rect, image: ImageId) {
        self.push(Command::Image { rect, image });
    }

    /// Adds a [`Command::PushClip`].
    pub fn push_clip(&mut self, rect: Rect) {
        self.push(Command::PushClip { rect });
    }

    /// Adds a [`Command::PopClip`].
    pub fn pop_clip(&mut self) {
        self.push(Command::PopClip);
    }

    /// Adds a [`Command::PushLayer`].
    pub fn push_layer(&mut self, opacity: f32) {
        self.push(Command::PushLayer { opacity });
    }

    /// Adds a [`Command::PopLayer`].
    pub fn pop_layer(&mut self) {
        self.push(Command::PopLayer);
    }
}

/// What a frame draws: the whole tree's list, and the rects it damaged,
/// outside which the frontend need not redraw.
///
/// The list holds no command with a NaN in its geometry, and every layer's
/// opacity is 0 to 1 (see [Sanitizing](self#sanitizing)), so every command
/// it holds is inside the damage of the frame that changed it. Its `Debug`
/// shows text as its length only.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// Every shown node's fragment, in pre-order, each subtree inside its
    /// node's layer and clip.
    pub list: DrawList,
    /// The damaged rects, inside the window, with no duplicates; at most
    /// [`MAX_DAMAGE_RECTS`].
    pub damage: Vec<Rect>,
}

/// What a widget's [`paint`](crate::tree::Widget::paint) gets: its node,
/// bounds and state, the metrics to measure text with, and the theme to
/// take colors from.
pub struct PaintContext<'a> {
    node: NodeId,
    bounds: Rect,
    state: NodeState,
    metrics: &'a dyn TextMetrics,
    theme: &'a Theme,
}

impl<'a> PaintContext<'a> {
    /// The widget's node.
    pub const fn node(&self) -> NodeId {
        self.node
    }

    /// The node's bounds in this frame's layout.
    pub const fn bounds(&self) -> Rect {
        self.bounds
    }

    /// The node's interaction state.
    pub const fn state(&self) -> NodeState {
        self.state
    }

    /// The metrics to measure text with.
    pub const fn metrics(&self) -> &'a dyn TextMetrics {
        self.metrics
    }

    /// The tree's theme, to take every color from ([`Tree::theme`],
    /// ADR-0049).
    pub const fn theme(&self) -> &'a Theme {
        self.theme
    }
}

/// An open push in a fragment.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Push {
    Clip,
    Layer,
}

impl Tree {
    /// Lays the tree out in a window of `size`, repaints the nodes marked
    /// for it, and returns the frame if anything was damaged; `None` if
    /// nothing was, as for an idle tree. See the
    /// [module documentation](self).
    ///
    /// Damage rects are cut to the window, empty ones and duplicates are
    /// dropped, and more than [`MAX_DAMAGE_RECTS`] become their bounding
    /// rect.
    pub fn frame(&mut self, size: Size, metrics: &dyn TextMetrics) -> Option<Frame> {
        let size = sane_size(size);
        self.layout(size, metrics);
        let window = Rect::new(0.0, 0.0, size.width, size.height);
        let all = self.repaint_all || self.frame_size != Some(size);
        self.repaint_all = false;
        self.frame_size = Some(size);
        let queued = std::mem::take(&mut self.paint_queue);
        let ids = if all { self.ids() } else { queued };
        let mut damage = std::mem::take(&mut self.damage);
        for id in ids {
            self.repaint(id, metrics, &mut damage);
        }
        let damage = if all {
            merge(vec![window], window)
        } else {
            merge(damage, window)
        };
        if damage.is_empty() {
            return None;
        }
        let mut list = DrawList::new();
        self.emit(self.root(), &mut list.commands);
        Some(Frame { list, damage })
    }

    /// Paints the node again, and damages its old and new painted bounds if
    /// they or its fragment changed, or its damage was forced.
    fn repaint(&mut self, id: NodeId, metrics: &dyn TextMetrics, damage: &mut Vec<Rect>) {
        let visible = self.is_visible(id);
        let Some(state) = self.state(id) else {
            return;
        };
        let Some(node) = self.get(id) else {
            return;
        };
        let mut list = DrawList::new();
        if visible && let Some(widget) = &node.widget {
            let cx = PaintContext {
                node: id,
                bounds: node.placement.rect,
                state,
                metrics,
                theme: &self.theme,
            };
            widget.paint(&cx, &mut list);
        }
        let fragment = sanitize(balance(list.commands));
        let painted = painted_bounds(&fragment, node.placement.clip, metrics);
        let Some(node) = self.get_mut(id) else {
            return;
        };
        node.queued = false;
        let forced = std::mem::take(&mut node.forced);
        if forced || node.painted != painted || node.fragment != fragment {
            damage.extend(node.painted);
            damage.extend(painted);
        }
        node.fragment = fragment;
        node.painted = painted;
    }

    /// Writes the node's subtree, if it is shown, into `out`.
    fn emit(&self, id: NodeId, out: &mut Vec<Command>) {
        let Some(node) = self.get(id) else {
            return;
        };
        let style = &node.sane;
        if style.hidden {
            return;
        }
        let layer = style.opacity < 1.0;
        if layer {
            out.push(Command::PushLayer {
                opacity: style.opacity,
            });
        }
        out.extend(node.fragment.iter().cloned());
        let clips = style.clips();
        if clips {
            out.push(Command::PushClip {
                rect: node.placement.rect,
            });
        }
        for &child in &node.children {
            self.emit(child, out);
        }
        if clips {
            out.push(Command::PopClip);
        }
        if layer {
            out.push(Command::PopLayer);
        }
    }
}

/// The commands, which [`balance`] has paired, less those with a NaN in
/// their geometry, and with every layer's opacity 0 to 1, NaN being 1. A
/// clip with a NaN goes with the pop that ends it, so the pushes and pops
/// stay paired; a pop that ends no push of its kind is dropped.
fn sanitize(commands: Vec<Command>) -> Vec<Command> {
    // For each open push, its kind and whether it was kept.
    let mut open: Vec<(Push, bool)> = Vec::new();
    let mut out = Vec::with_capacity(commands.len());
    for command in commands {
        let keep = match command {
            Command::PushClip { .. } => {
                let keep = !has_nan_geometry(&command);
                open.push((Push::Clip, keep));
                keep
            }
            Command::PushLayer { .. } => {
                open.push((Push::Layer, true));
                true
            }
            Command::PopClip => pop_kept(&mut open, Push::Clip),
            Command::PopLayer => pop_kept(&mut open, Push::Layer),
            _ => !has_nan_geometry(&command),
        };
        if !keep {
            continue;
        }
        out.push(match command {
            Command::PushLayer { opacity } => Command::PushLayer {
                opacity: if opacity.is_nan() {
                    1.0
                } else {
                    opacity.clamp(0.0, 1.0)
                },
            },
            other => other,
        });
    }
    out
}

/// Ends the innermost open push if it is of `kind`, and returns whether it
/// was kept; a pop that ends no push of its kind is not kept.
fn pop_kept(open: &mut Vec<(Push, bool)>, kind: Push) -> bool {
    match open.last() {
        Some(&(push, keep)) if push == kind => {
            open.pop();
            keep
        }
        _ => false,
    }
}

/// Returns `true` if a number of the command's geometry is NaN.
fn has_nan_geometry(command: &Command) -> bool {
    let point = |point: &Point| point.x.is_nan() || point.y.is_nan();
    match command {
        Command::Rect { rect, .. }
        | Command::Pill { rect, .. }
        | Command::Icon { rect, .. }
        | Command::Image { rect, .. }
        | Command::PushClip { rect } => has_nan(*rect),
        Command::CutCornerRect { rect, cut, .. } => has_nan(*rect) || cut.is_nan(),
        Command::Border { rect, width, .. } => has_nan(*rect) || width.is_nan(),
        Command::Brackets {
            rect,
            arm,
            thickness,
            ..
        } => has_nan(*rect) || arm.is_nan() || thickness.is_nan(),
        Command::Line {
            from, to, width, ..
        } => point(from) || point(to) || width.is_nan(),
        Command::Text { origin, style, .. } => point(origin) || style.size.is_nan(),
        Command::PopClip | Command::PushLayer { .. } | Command::PopLayer => false,
    }
}

/// The commands with every pop matched to a push: a pop that does not end
/// the innermost open push of its kind is dropped, and the pushes still
/// open at the end are closed, innermost first.
fn balance(commands: Vec<Command>) -> Vec<Command> {
    let mut open = Vec::new();
    let mut out = Vec::with_capacity(commands.len());
    for command in commands {
        let keep = match command {
            Command::PushClip { .. } => {
                open.push(Push::Clip);
                true
            }
            Command::PushLayer { .. } => {
                open.push(Push::Layer);
                true
            }
            Command::PopClip => open.last() == Some(&Push::Clip) && open.pop().is_some(),
            Command::PopLayer => open.last() == Some(&Push::Layer) && open.pop().is_some(),
            _ => true,
        };
        if keep {
            out.push(command);
        }
    }
    while let Some(push) = open.pop() {
        out.push(match push {
            Push::Clip => Command::PopClip,
            Push::Layer => Command::PopLayer,
        });
    }
    out
}

/// The union of the commands' bounds, each cut to `clip` and to the clips
/// the commands themselves pushed around it, which a frontend clips its ink
/// to as well; `None` if none covers any area. A rect with a NaN in it
/// covers none. The commands are a fragment, whose pushes and pops
/// [`balance`] has paired.
fn painted_bounds(commands: &[Command], clip: Rect, metrics: &dyn TextMetrics) -> Option<Rect> {
    let mut clips = vec![clip];
    let mut painted: Option<Rect> = None;
    for command in commands {
        let current = clips.last().copied().unwrap_or(clip);
        match command {
            Command::PushClip { rect } => clips.push(intersect(*rect, current)),
            Command::PopClip => {
                if clips.len() > 1 {
                    clips.pop();
                }
            }
            _ => {
                let Some(rect) = command.bounds(metrics) else {
                    continue;
                };
                if has_nan(rect) {
                    continue;
                }
                let rect = intersect(rect, current);
                if !rect.is_empty() {
                    painted = Some(painted.map_or(rect, |painted| union(painted, rect)));
                }
            }
        }
    }
    painted
}

/// Damage rects cut to `window`, without empty ones and duplicates, or
/// their bounding rect if there are more than [`MAX_DAMAGE_RECTS`].
fn merge(rects: Vec<Rect>, window: Rect) -> Vec<Rect> {
    let mut kept: Vec<Rect> = Vec::new();
    let mut bounding: Option<Rect> = None;
    for rect in rects {
        if has_nan(rect) {
            continue;
        }
        let rect = intersect(rect, window);
        if rect.is_empty() {
            continue;
        }
        bounding = Some(bounding.map_or(rect, |bounding| union(bounding, rect)));
        // Past the limit only the bounding rect is kept, so the list need
        // not grow further.
        if kept.len() <= MAX_DAMAGE_RECTS && !kept.contains(&rect) {
            kept.push(rect);
        }
    }
    if kept.len() > MAX_DAMAGE_RECTS {
        bounding.into_iter().collect()
    } else {
        kept
    }
}

/// The smallest rect holding every rect of `rects` that has no NaN in it;
/// `None` if there is none.
pub(crate) fn bounding(rects: &[Rect]) -> Option<Rect> {
    rects
        .iter()
        .copied()
        .filter(|rect| !has_nan(*rect))
        .reduce(union)
}

/// The smallest rect holding both.
fn union(a: Rect, b: Rect) -> Rect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let right = a.right().max(b.right());
    let bottom = a.bottom().max(b.bottom());
    Rect::new(x, y, right - x, bottom - y)
}

/// Returns `true` if a component of `rect` is NaN.
fn has_nan(rect: Rect) -> bool {
    rect.x.is_nan() || rect.y.is_nan() || rect.width.is_nan() || rect.height.is_nan()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::{Family, FixedMetrics};

    #[test]
    fn bounds_of_each_command() {
        let metrics = FixedMetrics::DEFAULT;
        let rect = Rect::new(1.0, 2.0, 3.0, 4.0);
        let white = Color::WHITE;
        for command in [
            Command::Rect { rect, color: white },
            Command::CutCornerRect {
                rect,
                cut: 1.0,
                corners: Corners::ALL,
                color: white,
            },
            Command::Pill { rect, color: white },
            Command::Border {
                rect,
                width: 1.0,
                color: white,
            },
            Command::Brackets {
                rect,
                arm: 2.0,
                thickness: 1.0,
                corners: Corners::TOP_LEFT_AND_BOTTOM_RIGHT,
                color: white,
            },
            Command::Icon {
                rect,
                icon: IconId(7),
                color: white,
            },
            Command::Image {
                rect,
                image: ImageId(9),
            },
        ] {
            assert_eq!(command.bounds(&metrics), Some(rect), "{command:?}");
        }
        let line = Command::Line {
            from: Point::new(10.0, 4.0),
            to: Point::new(2.0, 8.0),
            width: 2.0,
            color: white,
        };
        assert_eq!(line.bounds(&metrics), Some(Rect::new(1.0, 3.0, 10.0, 6.0)));
        let hairline = Command::Line {
            from: Point::new(0.0, 0.0),
            to: Point::new(5.0, 0.0),
            width: f32::NAN,
            color: white,
        };
        assert_eq!(
            hairline.bounds(&metrics),
            Some(Rect::new(0.0, 0.0, 5.0, 0.0))
        );
        // "abc" at 10 units: 18 wide, ascent 8, descent 2, grown by 5.
        let text = Command::Text {
            origin: Point::new(20.0, 30.0),
            text: "abc".to_string(),
            style: TextStyle::new(Family::Ui, 10.0),
            color: white,
        };
        assert_eq!(
            text.bounds(&metrics),
            Some(Rect::new(15.0, 17.0, 28.0, 20.0))
        );
        for command in [
            Command::PushClip { rect },
            Command::PopClip,
            Command::PushLayer { opacity: 0.5 },
            Command::PopLayer,
        ] {
            assert_eq!(command.bounds(&metrics), None, "{command:?}");
        }
    }

    #[test]
    fn list_helpers_push_one_command_each() {
        let rect = Rect::new(0.0, 0.0, 1.0, 1.0);
        let color = Color::BLACK;
        let style = TextStyle::new(Family::Mono, 12.0);
        let mut list = DrawList::new();
        assert!(list.is_empty());
        list.rect(rect, color);
        list.cut_corner_rect(rect, 2.0, Corners::NONE, color);
        list.pill(rect, color);
        list.border(rect, 1.0, color);
        list.brackets(rect, 3.0, 1.0, Corners::ALL, color);
        list.line(Point::ZERO, Point::new(1.0, 1.0), 1.0, color);
        list.text(Point::ZERO, "x", style, color);
        list.icon(rect, IconId(1), color);
        list.image(rect, ImageId(2));
        list.push_clip(rect);
        list.pop_clip();
        list.push_layer(0.5);
        list.pop_layer();
        assert_eq!(list.len(), 13);
        assert_eq!(
            list.commands().get(6),
            Some(&Command::Text {
                origin: Point::ZERO,
                text: "x".to_string(),
                style,
                color,
            })
        );
        list.clear();
        assert_eq!(list, DrawList::default());
    }

    #[test]
    fn balancing_drops_stray_pops_and_closes_open_pushes() {
        let rect = Rect::new(0.0, 0.0, 1.0, 1.0);
        let fill = Command::Rect {
            rect,
            color: Color::WHITE,
        };
        let balanced = balance(vec![
            Command::PopClip,
            Command::PushLayer { opacity: 0.5 },
            fill.clone(),
            Command::PopClip,
            Command::PushClip { rect },
            fill.clone(),
        ]);
        assert_eq!(
            balanced,
            vec![
                Command::PushLayer { opacity: 0.5 },
                fill.clone(),
                Command::PushClip { rect },
                fill,
                Command::PopClip,
                Command::PopLayer,
            ]
        );
    }

    #[test]
    fn debug_shows_a_texts_length_and_never_the_text() {
        let text = Command::Text {
            origin: Point::new(1.0, 2.0),
            text: "hunter2".to_string(),
            style: TextStyle::new(Family::Mono, 12.0),
            color: Color::WHITE,
        };
        let shown = format!("{text:?}");
        assert!(shown.contains("text: <7 bytes>"), "{shown}");
        assert!(shown.starts_with("Text { origin: Point"), "{shown}");
        assert!(!shown.contains("hunter2"), "{shown}");
        let mut list = DrawList::new();
        list.push(text);
        list.rect(Rect::new(0.0, 0.0, 1.0, 1.0), Color::BLACK);
        list.pop_clip();
        let shown = format!("{list:?}");
        assert!(!shown.contains("hunter2"), "{shown}");
        assert!(shown.contains("<7 bytes>"), "{shown}");
        // The other commands show as a derive would show them.
        assert!(shown.contains("Rect { rect: Rect {"), "{shown}");
        assert!(shown.ends_with("PopClip] }"), "{shown}");
        let frame = Frame {
            list,
            damage: vec![Rect::new(0.0, 0.0, 1.0, 1.0)],
        };
        let shown = format!("{frame:?}");
        assert!(!shown.contains("hunter2"), "{shown}");
        assert!(shown.contains("<7 bytes>"), "{shown}");
    }

    #[test]
    fn sanitizing_drops_nan_geometry_and_clamps_opacity() {
        let nan = f32::NAN;
        let rect = Rect::new(0.0, 0.0, 1.0, 1.0);
        let bad = Rect::new(0.0, 0.0, 1.0, nan);
        let at = Point::new(1.0, 1.0);
        let white = Color::WHITE;
        let style = TextStyle::new(Family::Ui, 10.0);
        let kept = vec![
            Command::Rect { rect, color: white },
            Command::CutCornerRect {
                rect,
                cut: 1.0,
                corners: Corners::ALL,
                color: white,
            },
            Command::Pill { rect, color: white },
            Command::Border {
                rect,
                width: 1.0,
                color: white,
            },
            Command::Brackets {
                rect,
                arm: 2.0,
                thickness: 1.0,
                corners: Corners::ALL,
                color: white,
            },
            Command::Line {
                from: at,
                to: Point::ZERO,
                width: 1.0,
                color: white,
            },
            Command::Text {
                origin: at,
                text: "x".to_string(),
                style,
                color: white,
            },
            Command::Icon {
                rect,
                icon: IconId(1),
                color: white,
            },
            Command::Image {
                rect,
                image: ImageId(1),
            },
            Command::PushClip { rect },
            Command::PopClip,
            Command::PushLayer { opacity: 0.5 },
            Command::PopLayer,
        ];
        assert_eq!(sanitize(kept.clone()), kept);
        let dropped = vec![
            Command::Rect {
                rect: bad,
                color: white,
            },
            Command::CutCornerRect {
                rect,
                cut: nan,
                corners: Corners::ALL,
                color: white,
            },
            Command::Pill {
                rect: bad,
                color: white,
            },
            Command::Border {
                rect,
                width: nan,
                color: white,
            },
            Command::Brackets {
                rect,
                arm: nan,
                thickness: 1.0,
                corners: Corners::ALL,
                color: white,
            },
            Command::Brackets {
                rect,
                arm: 1.0,
                thickness: nan,
                corners: Corners::ALL,
                color: white,
            },
            Command::Line {
                from: Point::new(nan, 0.0),
                to: Point::ZERO,
                width: 1.0,
                color: white,
            },
            Command::Line {
                from: at,
                to: Point::new(0.0, nan),
                width: 1.0,
                color: white,
            },
            Command::Line {
                from: at,
                to: Point::ZERO,
                width: nan,
                color: white,
            },
            Command::Text {
                origin: Point::new(0.0, nan),
                text: "x".to_string(),
                style,
                color: white,
            },
            Command::Text {
                origin: at,
                text: "x".to_string(),
                style: TextStyle::new(Family::Ui, nan),
                color: white,
            },
            Command::Icon {
                rect: bad,
                icon: IconId(1),
                color: white,
            },
            Command::Image {
                rect: bad,
                image: ImageId(1),
            },
            Command::PushClip { rect: bad },
        ];
        assert_eq!(sanitize(dropped), vec![]);
        assert_eq!(
            sanitize(vec![
                Command::PushLayer { opacity: nan },
                Command::PushLayer { opacity: 2.0 },
                Command::PushLayer { opacity: -0.5 },
                Command::PushLayer {
                    opacity: f32::NEG_INFINITY
                },
            ]),
            vec![
                Command::PushLayer { opacity: 1.0 },
                Command::PushLayer { opacity: 1.0 },
                Command::PushLayer { opacity: 0.0 },
                Command::PushLayer { opacity: 0.0 },
            ]
        );
    }

    #[test]
    fn a_clip_with_a_nan_goes_with_its_own_pop() {
        let outer = Rect::new(0.0, 0.0, 10.0, 10.0);
        let bad = Rect::new(f32::NAN, 0.0, 5.0, 5.0);
        let x = Command::Rect {
            rect: Rect::new(1.0, 1.0, 2.0, 2.0),
            color: Color::WHITE,
        };
        let y = Command::Rect {
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            color: Color::BLACK,
        };
        // Nested in a clip: what follows the bad clip's pop stays inside
        // the outer clip.
        assert_eq!(
            sanitize(balance(vec![
                Command::PushClip { rect: outer },
                Command::PushClip { rect: bad },
                x.clone(),
                Command::PopClip,
                y.clone(),
                Command::PopClip,
            ])),
            vec![
                Command::PushClip { rect: outer },
                x.clone(),
                y.clone(),
                Command::PopClip,
            ]
        );
        // With a layer between the clips.
        assert_eq!(
            sanitize(balance(vec![
                Command::PushClip { rect: outer },
                Command::PushClip { rect: bad },
                Command::PushLayer { opacity: 0.5 },
                x.clone(),
                Command::PopLayer,
                Command::PopClip,
                y.clone(),
                Command::PopClip,
            ])),
            vec![
                Command::PushClip { rect: outer },
                Command::PushLayer { opacity: 0.5 },
                x.clone(),
                Command::PopLayer,
                y.clone(),
                Command::PopClip,
            ]
        );
        // A bad clip left open, and one whose pop comes inside a layer, are
        // closed by balancing, and that pop goes too.
        assert_eq!(
            sanitize(balance(vec![
                Command::PushClip { rect: outer },
                Command::PushClip { rect: bad },
                Command::PushLayer { opacity: 0.5 },
                x.clone(),
                Command::PopClip,
                Command::PopLayer,
                y.clone(),
            ])),
            vec![
                Command::PushClip { rect: outer },
                Command::PushLayer { opacity: 0.5 },
                x,
                Command::PopLayer,
                y,
                Command::PopClip,
            ]
        );
        // Alone, a pop that ends no push of its kind is dropped.
        assert_eq!(
            sanitize(vec![
                Command::PopClip,
                Command::PushLayer { opacity: 1.0 },
                Command::PopClip,
                Command::PopLayer,
                Command::PopLayer,
            ]),
            vec![Command::PushLayer { opacity: 1.0 }, Command::PopLayer]
        );
    }

    #[test]
    fn merging_cuts_drops_and_bounds() {
        let window = Rect::new(0.0, 0.0, 100.0, 100.0);
        let a = Rect::new(-10.0, 0.0, 20.0, 10.0);
        let b = Rect::new(50.0, 50.0, 10.0, 10.0);
        assert_eq!(
            merge(
                vec![a, b, b, Rect::new(200.0, 0.0, 5.0, 5.0), Rect::ZERO],
                window
            ),
            vec![Rect::new(0.0, 0.0, 10.0, 10.0), b]
        );
        let many: Vec<Rect> = (0..=MAX_DAMAGE_RECTS)
            .map(|i| Rect::new(i as f32 * 2.0, 1.0, 1.0, 1.0))
            .collect();
        assert_eq!(
            merge(many, window),
            vec![Rect::new(
                0.0,
                1.0,
                MAX_DAMAGE_RECTS as f32 * 2.0 + 1.0,
                1.0
            )]
        );
        assert_eq!(
            merge(vec![Rect::new(f32::NAN, 0.0, 5.0, 5.0)], window),
            vec![]
        );
    }
}
