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

//! The scrollbar: a thin track with a pill-shaped thumb. jabbott.dev has
//! no scrollbar style; this one is ADR-0049's.

use tidepool_core::geom::{Point, Rect, Size};
use tidepool_core::input::{InputEvent, MouseButton};

use crate::a11y::{Role, Semantics};
use crate::draw::{DrawList, PaintContext};
use crate::input::{ActionKind, EventContext, Handled};
use crate::layout::{Axis, Length, Style};
use crate::metrics::TextMetrics;
use crate::theme::Token;
use crate::tree::Widget;

use super::{Control, Look, MIN_THUMB, SCROLLBAR_THICKNESS, THUMB_THICKNESS, dimmed};

/// A scrollbar for content that its owner scrolls: it shows where a view
/// of `viewport` units is in `content` units, at an offset, and lets the
/// pointer move it.
///
/// Dragging its thumb moves the offset with the pointer; a press on the
/// track beside the thumb moves it a viewport toward the pointer. Each
/// move emits [`ActionKind::Change`] and sets the node's value, the offset
/// as a percentage; the owner reads [`offset`](Self::offset), scrolls what
/// it shows (`Tree::set_scroll_offset`), and keeps the scrollbar in step
/// with [`set_range`](Self::set_range) and
/// [`set_offset`](Self::set_offset). The wheel goes on to the owner.
///
/// It paints nothing while everything fits. At rest its thumb is
/// `line-2`; hovered, its track is `line` and its thumb `teal-glow`; while
/// pressed, its thumb is `teal`. It is a scroll bar in the accessibility
/// tree, and does not take focus: the keyboard scrolls the content.
///
/// ```
/// use tidepool_ui::layout::Axis;
/// use tidepool_ui::widgets::{Control, ScrollBar};
///
/// let mut bar = ScrollBar::new(Axis::Vertical, "Explorer");
/// bar.set_range(1000.0, 250.0);
/// bar.set_offset(375.0);
/// assert_eq!(bar.max_offset(), 750.0);
/// assert_eq!(bar.semantics().value.as_deref(), Some("50%"));
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct ScrollBar {
    axis: Axis,
    name: String,
    content: f32,
    viewport: f32,
    offset: f32,
    /// While the thumb is dragged: where the pointer took it, from the
    /// thumb's start along the axis.
    grab: Option<f32>,
}

impl ScrollBar {
    /// A scrollbar on `axis` for the content named `name`, with nothing to
    /// scroll.
    pub fn new(axis: Axis, name: impl Into<String>) -> Self {
        Self {
            axis,
            name: name.into(),
            content: 0.0,
            viewport: 0.0,
            offset: 0.0,
            grab: None,
        }
    }

    /// Its axis.
    pub const fn axis(&self) -> Axis {
        self.axis
    }

    /// Sets the content's length and the view's, in units; a value that
    /// is not finite or is negative counts as 0. The offset is kept within
    /// the new range.
    pub fn set_range(&mut self, content: f32, viewport: f32) {
        self.content = sane(content);
        self.viewport = sane(viewport);
        self.offset = self.offset.clamp(0.0, self.max_offset());
    }

    /// The content's length.
    pub const fn content(&self) -> f32 {
        self.content
    }

    /// The view's length.
    pub const fn viewport(&self) -> f32 {
        self.viewport
    }

    /// How far the view is from the content's start.
    pub const fn offset(&self) -> f32 {
        self.offset
    }

    /// The largest offset: the content less the view, or 0.
    pub fn max_offset(&self) -> f32 {
        (self.content - self.viewport).max(0.0)
    }

    /// Moves the view to `offset`, kept from 0 to the
    /// [largest](Self::max_offset); NaN counts as 0.
    pub fn set_offset(&mut self, offset: f32) {
        let offset = if offset.is_nan() { 0.0 } else { offset };
        self.offset = offset.clamp(0.0, self.max_offset());
    }

    /// The offset as a whole percentage of the largest, 0 when nothing
    /// scrolls.
    fn percent(&self) -> u32 {
        let max = self.max_offset();
        if max <= 0.0 {
            0
        } else {
            // From 0 to 100, so the cast is exact after rounding.
            (self.offset / max * 100.0).round().clamp(0.0, 100.0) as u32
        }
    }

    /// The start and length of the track along the axis, in window
    /// coordinates.
    fn track(&self, bounds: Rect) -> (f32, f32) {
        match self.axis {
            Axis::Vertical => (bounds.y, bounds.height),
            Axis::Horizontal => (bounds.x, bounds.width),
        }
    }

    /// The thumb's start and length along the axis, or `None` when
    /// everything fits.
    fn thumb(&self, bounds: Rect) -> Option<(f32, f32)> {
        let max = self.max_offset();
        if max <= 0.0 || self.content <= 0.0 {
            return None;
        }
        let (start, track) = self.track(bounds);
        // Each ratio first, at most 1, so that a range as large as `f32`
        // holds does not overflow.
        let length = (track * (self.viewport / self.content))
            .max(MIN_THUMB)
            .min(track);
        let travel = track - length;
        Some((start + travel * (self.offset / max), length))
    }

    /// The pointer's position along the axis.
    const fn along(&self, point: Point) -> f32 {
        match self.axis {
            Axis::Vertical => point.y,
            Axis::Horizontal => point.x,
        }
    }

    /// Moves the offset so that the thumb starts at `thumb_start`.
    fn drag_to(&mut self, bounds: Rect, thumb_start: f32) {
        let Some((_, length)) = self.thumb(bounds) else {
            return;
        };
        let (start, track) = self.track(bounds);
        let travel = track - length;
        if travel > 0.0 {
            let ratio = ((thumb_start - start) / travel).clamp(0.0, 1.0);
            self.set_offset(ratio * self.max_offset());
        }
    }

    /// Tells the owner the offset moved, if it did.
    fn moved(&self, cx: &mut EventContext<'_>, before: f32) {
        if self.offset != before {
            cx.emit(ActionKind::Change);
            cx.set_semantics(self.semantics());
        }
        cx.request_paint();
    }
}

impl Widget for ScrollBar {
    fn measure(&self, _available: Size, _metrics: &dyn TextMetrics) -> Size {
        Size::new(SCROLLBAR_THICKNESS, SCROLLBAR_THICKNESS)
    }

    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let bounds = cx.bounds();
        let theme = cx.theme();
        let Some((start, length)) = self.thumb(bounds) else {
            return;
        };
        let look = Look::of(cx.state());
        dimmed(cx, list, |list| {
            let thumb = match look {
                Look::Rest => Token::Line2,
                Look::Hover => Token::TealGlow,
                Look::Pressed => Token::Teal,
            };
            if look != Look::Rest {
                list.rect(bounds, theme.color(Token::Line));
            }
            let side = (SCROLLBAR_THICKNESS - THUMB_THICKNESS) / 2.0;
            let rect = match self.axis {
                Axis::Vertical => Rect::new(bounds.x + side, start, THUMB_THICKNESS, length),
                Axis::Horizontal => Rect::new(start, bounds.y + side, length, THUMB_THICKNESS),
            };
            list.pill(rect, theme.color(thumb));
        });
    }

    fn event(&mut self, cx: &mut EventContext<'_>, event: &InputEvent) -> Handled {
        let bounds = cx.bounds();
        match *event {
            InputEvent::MouseDown {
                button: MouseButton::Left,
                position,
                ..
            } => {
                let Some((start, length)) = self.thumb(bounds) else {
                    return Handled::Yes;
                };
                let at = self.along(position);
                let before = self.offset;
                self.grab = None;
                if at >= start && at <= start + length {
                    self.grab = Some(at - start);
                } else if at < start {
                    self.set_offset(self.offset - self.viewport);
                } else {
                    self.set_offset(self.offset + self.viewport);
                }
                self.moved(cx, before);
                Handled::Yes
            }
            InputEvent::MouseMove { position, .. } => match self.grab {
                Some(grab) if cx.state().pressed => {
                    let before = self.offset;
                    self.drag_to(bounds, self.along(position) - grab);
                    self.moved(cx, before);
                    Handled::Yes
                }
                _ => Handled::No,
            },
            InputEvent::MouseUp {
                button: MouseButton::Left,
                ..
            } => {
                if self.grab.take().is_some() {
                    cx.request_paint();
                }
                Handled::Yes
            }
            _ => Handled::No,
        }
    }
}

impl Control for ScrollBar {
    fn semantics(&self) -> Semantics {
        Semantics::new(Role::ScrollBar, self.name.clone())
            .with_value(format!("{}%", self.percent()))
    }

    /// [`SCROLLBAR_THICKNESS`] across, and filling the parent along its
    /// axis.
    fn style(&self) -> Style {
        match self.axis {
            Axis::Vertical => Style::stack()
                .width(Length::Fixed(SCROLLBAR_THICKNESS))
                .height(Length::Fill(1.0)),
            Axis::Horizontal => Style::stack()
                .width(Length::Fill(1.0))
                .height(Length::Fixed(SCROLLBAR_THICKNESS)),
        }
    }

    fn focusable(&self) -> bool {
        false
    }
}

/// `value` if it is finite and positive, else 0.
fn sane(value: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        0.0
    }
}
