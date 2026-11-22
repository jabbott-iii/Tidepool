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

//! The toast: a short message in the window's bottom-right corner, with an
//! optional action and a close control (ADR-0052, ADR-0055).

use std::any::Any;
use std::fmt;

use tidepool_core::geom::{Point, Rect, Size};
use tidepool_core::input::{InputEvent, Key, Modifiers, TextLength};

use crate::a11y::{Role, Semantics};
use crate::draw::{DrawList, PaintContext};
use crate::error::UiError;
use crate::input::{ActionKind, EventContext, Handled};
use crate::layout::{Align, Insets, Length, Style};
use crate::metrics::TextMetrics;
use crate::overlay::{Anchor, Layer, MAX_OVERLAYS, OverlayKind};
use crate::theme::Token;
use crate::tree::{MAX_NODES, Node, NodeId, Tree, Widget};

use super::button::Activator;
use super::modal::kept_lines;
use super::tabs::{CROSS, CROSS_STROKE};
use super::{
    Button, ButtonKind, ButtonSize, Control, Look, MAX_TOAST_LINES, MAX_TOASTS, TOAST_CLOSE,
    TOAST_GAP, TOAST_MARGIN, TOAST_PADDING, TOAST_STRIPE, TOAST_TEXT, TOAST_WIDTH, baseline,
    dimmed, focus_outline, line_height,
};

/// What a toast says about what happened, and so its stripe's color and
/// its role in the accessibility tree.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ToastKind {
    /// Something finished or changed: a `teal` stripe, a status.
    #[default]
    Info,
    /// Something needs a look: an `amber` stripe, a status.
    Warning,
    /// Something failed: an `error` stripe, an alert.
    Error,
}

impl ToastKind {
    /// Its stripe's color and its role: a status is a polite live region,
    /// read when the screen reader is idle; an alert an assertive one,
    /// read at once (WAI-ARIA).
    const fn look(self) -> (Token, Role) {
        match self {
            Self::Info => (Token::Teal, Role::Status),
            Self::Warning => (Token::Amber, Role::Status),
            Self::Error => (Token::Error, Role::Alert),
        }
    }
}

/// A toast: a short message, with an optional action and a close control,
/// shown in the window's bottom-right corner over everything else
/// (ADR-0052).
///
/// # Where toasts go
///
/// Each toast is its own modeless overlay ([`OverlayKind::Modeless`])
/// anchored to the window's bottom-right corner, [`TOAST_MARGIN`] from its
/// edges, which it keeps: [`Tree::set_overlay_anchor`] refuses to move it
/// ([`UiError::FixedAnchor`]). They stand in one column, [`TOAST_GAP`]
/// apart: the newest at the bottom, nearest the corner, the older above
/// it, as each layout lifts every toast above the newer ones, from the
/// sizes that layout measures. The bottom-right is clear of where text
/// starts and of the command palette at the top, and is where other
/// editors put them. At most [`MAX_TOASTS`] show: showing one more closes
/// the oldest that is not held (see below). No two toasts overlap: a toast
/// with no room left in the column, in a window too short for all of
/// them, *waits* out of view, the older ones first, until there is room.
/// A toast shown waits so until the first layout gives it its place.
///
/// While a toast is *held* by the user (the pointer is on it, a press went
/// down on it, or keyboard focus is in it), or the pointer is over where a
/// toast shows, even one inert under a modal, the column keeps still: the
/// held toasts keep their places, and the others theirs where they meet
/// nothing (a toast that grows pushes those above it up, or out of view if
/// there is no room); one shown meanwhile goes above the others, if there
/// is room, and one closed meanwhile leaves its gap. No toast overlaps
/// another even then: a held one with no room where it stands takes the
/// lowest place clear of the others, or waits out of view. A window of
/// another height moves them all, so they settle anew. The column keeps still
/// so too after a toast closes under the pointer, such as by a click on its
/// close control, until the pointer moves more than
/// [`PRESS_SLOP`](crate::input::PRESS_SLOP) device pixels: the second click
/// of a double click lands where the toast was, not on another toast's
/// button slid under it. Once none of that holds, they settle into their
/// order again. So a toast never slides out from under the pointer, or
/// from where the user's focus is, to put another one's button there.
///
/// # Over a modal
///
/// A toast stands in the order of the overlays just above the last modal
/// opened before it was shown, shown or hidden (ADR-0055). So a toast
/// shown while a modal is active, or hidden, is above it and live while
/// that modal is active: Tab reaches its controls, as it reaches any
/// overlay opened above the modal (ADR-0053). A toast shown before a modal
/// opened stays under it, inert and dimmed with the rest of the window,
/// until that modal closes.
///
/// A live toast keeps clear of the active modal: if it, or an overlay its
/// nodes host (a tooltip on its button), overlaps the modal or an overlay
/// the modal's nodes host, it *waits* just under the modal, inert and
/// dimmed, until it would stand clear (the window grows, or the toasts
/// below it close) or the modal closes or is hidden, and then goes back
/// above it (ADR-0055). So text from a language server or an agent never
/// covers a dialog's controls. Each layout, and each change of the
/// overlays, works the order out anew from when each overlay opened and
/// each toast showed, so it does not depend on what happened between.
/// [`Toast::waiting`] says when a toast waits, here or for room.
///
/// # Its owner's timer
///
/// `ui` has no clock, so how long a toast stays is its owner's timer, in
/// the main loop's `next_deadline`. [`Toast::held`] says when the timer
/// should wait: while the user holds the toast, as WCAG 2.2.1 suggests, so
/// that no one loses a toast they are reading or about to act on; while it
/// waits; and while it is inert under a modal, shown before it opened, as
/// no one can act on it there. When the timer runs out, the owner closes
/// it with [`Toast::close`].
///
/// # Keys and actions
///
/// A toast takes no focus when it shows; Tab reaches its action and close
/// control as it reaches any. Its action, a small teal button, emits
/// [`ActionKind::Activate`] from its own node ([`Toast::action_button`]).
/// Its close control ([`Toast::close_button`]), and Escape while focus is
/// in the toast, emit [`ActionKind::Dismiss`]; the owner then closes it.
/// [`Toast::of`] finds the toast that holds a node.
///
/// # Its look and its place in the accessibility tree
///
/// [`TOAST_WIDTH`] wide, `panel`'s solid value, opaque as a modal
/// dialog's is (ADR-0053), with a 1-unit `line` border and a
/// [`TOAST_STRIPE`]-wide stripe down its left edge in its kind's color;
/// its text in [`TOAST_TEXT`] (`text`), as written, at most
/// [`MAX_TOAST_LINES`] lines, read and cleaned as a
/// [`Modal`](super::Modal)'s are; its close control a cross, as a tab's.
/// The site has no toast style; this is derived from its panels and the
/// status dot's colors. It is a status, or an alert for an error, named by
/// its text.
///
/// ```
/// use tidepool_core::geom::Size;
/// use tidepool_ui::metrics::FixedMetrics;
/// use tidepool_ui::widgets::{Toast, ToastKind};
/// use tidepool_ui::Tree;
///
/// let mut tree = Tree::new();
/// let toast = Toast::new("rust-analyzer is up to date.").action("Release notes").show(&mut tree)?;
/// assert_eq!(tree.focused(), None, "it takes no focus");
/// tree.layout(Size::new(800.0, 600.0), &FixedMetrics::DEFAULT);
/// assert_eq!(Toast::held(&tree, toast), Some(false));
/// assert_eq!(Toast::toasts(&tree), [toast]);
/// Toast::close(&mut tree, toast)?;
/// assert_eq!(tree.overlays(), []);
/// # Ok::<(), tidepool_ui::UiError>(())
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct Toast {
    kind: ToastKind,
    lines: Vec<String>,
    action: Option<String>,
}

impl Toast {
    /// A toast of [`ToastKind::Info`] saying `text`, read and cleaned as
    /// the type's documentation says.
    pub fn new(text: impl AsRef<str>) -> Self {
        Self {
            kind: ToastKind::Info,
            lines: kept_lines(text.as_ref(), MAX_TOAST_LINES),
            action: None,
        }
    }

    /// The same, of `kind`.
    #[must_use]
    pub fn kind(self, kind: ToastKind) -> Self {
        Self { kind, ..self }
    }

    /// The same, with an action labeled `label`, or none if it is blank
    /// once kept and cleaned.
    #[must_use]
    pub fn action(self, label: impl AsRef<str>) -> Self {
        let label = kept_lines(label.as_ref(), 1).concat();
        Self {
            action: (!label.trim().is_empty()).then_some(label),
            ..self
        }
    }

    /// Its lines, as kept and cleaned.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Shows it at the bottom of the toasts, in an overlay of its own above
    /// the last modal opened, so above an active modal; closes the oldest
    /// that is not held if [`MAX_TOASTS`] show. It waits out of view until
    /// the next layout places it. Returns its node, the root of its
    /// overlay. Focus does not move.
    ///
    /// # Errors
    ///
    /// - [`UiError::Unnamed`] if its text is blank once kept and cleaned:
    ///   a toast is named by its text;
    /// - [`UiError::TooManyToasts`] if [`MAX_TOASTS`] show and every one is
    ///   held, which cannot happen with four: at most three are held at
    ///   once, one under the pointer, one pressed and one with focus;
    /// - [`UiError::TooManyNodes`] if the tree has no room for its nodes;
    /// - the errors of [`Tree::open_overlay`].
    ///
    /// Nothing changes then: each is found before the tree changes.
    pub fn show(self, tree: &mut Tree) -> Result<NodeId, UiError> {
        let (stripe, role) = self.kind.look();
        if self.lines.iter().all(|line| line.trim().is_empty()) {
            return Err(UiError::Unnamed { role });
        }
        // The toast, its row, text and close, its action's row and button.
        if tree.node_count().saturating_add(6) > MAX_NODES {
            return Err(UiError::TooManyNodes { max: MAX_NODES });
        }
        let toasts = Self::toasts(tree);
        let oldest = if toasts.len() >= MAX_TOASTS {
            let free = toasts
                .iter()
                .copied()
                .find(|&toast| !user_holds(tree, toast));
            Some(free.ok_or(UiError::TooManyToasts { max: MAX_TOASTS })?)
        } else {
            None
        };
        if oldest.is_none() && tree.overlays().len() >= MAX_OVERLAYS {
            return Err(UiError::TooManyOverlays { max: MAX_OVERLAYS });
        }
        if let Some(oldest) = oldest {
            // A toast, so an overlay's root.
            let _ = tree.close_overlay(oldest);
        }
        let style = Style::column()
            .width(Length::Fixed(TOAST_WIDTH))
            .padding(Insets::new(
                TOAST_PADDING,
                TOAST_PADDING,
                TOAST_PADDING,
                TOAST_PADDING + TOAST_STRIPE + 4.0,
            ))
            .gap(TOAST_GAP)
            .clip();
        let view = ToastView {
            stripe,
            placed: false,
            waiting: false,
            seen: (false, false),
            room: 0.0,
        };
        let node = Node::new(style)
            .with_semantics(Semantics::new(role, self.lines.join(" ")))
            .with_widget(view);
        let anchor = Anchor::window(Align::End, Align::End, TOAST_MARGIN);
        let toast = tree.open_overlay(node, OverlayKind::Modeless, anchor)?;
        // Out of view until a layout gives it its place in the column.
        tree.park(toast, true);
        let head = tree.insert(
            toast,
            Node::new(Style::row().width(Length::Fill(1.0)).gap(TOAST_GAP)),
        )?;
        tree.insert(
            head,
            Node::new(Style::default().width(Length::Fill(1.0)))
                .with_widget(ToastText { lines: self.lines }),
        )?;
        tree.insert(head, ToastClose::default().node())?;
        if let Some(label) = self.action {
            let row = tree.insert(
                toast,
                Node::new(
                    Style::row()
                        .width(Length::Fill(1.0))
                        .align(Align::End, Align::Start),
                ),
            )?;
            let button = Button::new(label, ButtonKind::Teal).size(ButtonSize::Small);
            tree.insert(row, button.node())?;
        }
        Ok(toast)
    }

    /// Closes the toast `toast`, its overlay.
    ///
    /// # Errors
    ///
    /// [`UiError::UnknownNode`] if it is not in the tree, and
    /// [`UiError::WrongWidget`] if it is not a toast.
    pub fn close(tree: &mut Tree, toast: NodeId) -> Result<(), UiError> {
        if !tree.contains(toast) {
            return Err(UiError::UnknownNode { id: toast });
        }
        if tree.widget::<ToastView>(toast).is_none() {
            return Err(UiError::WrongWidget { id: toast });
        }
        tree.close_overlay(toast).map(|_| ())
    }

    /// The toasts shown, the oldest first.
    pub fn toasts(tree: &Tree) -> Vec<NodeId> {
        let mut toasts: Vec<(u64, NodeId)> = tree
            .overlays()
            .into_iter()
            .filter(|&root| tree.widget::<ToastView>(root).is_some())
            .filter_map(|root| tree.overlay_seq_of(root).map(|seq| (seq, root)))
            .collect();
        toasts.sort_unstable();
        toasts.into_iter().map(|(_, root)| root).collect()
    }

    /// The toast that holds `node`, or is it, if any.
    pub fn of(tree: &Tree, node: NodeId) -> Option<NodeId> {
        let mut at = Some(node);
        while let Some(id) = at {
            if tree.widget::<ToastView>(id).is_some() {
                return Some(id);
            }
            at = tree.parent(id);
        }
        None
    }

    /// Whether the toast's timeout should wait: the pointer is on it, a
    /// press went down on it, or keyboard focus is in it; it
    /// [waits](Self::waiting); or it is inert under a modal. `None` if it
    /// is not a toast.
    pub fn held(tree: &Tree, toast: NodeId) -> Option<bool> {
        let waiting = Self::waiting(tree, toast)?;
        Some(waiting || user_holds(tree, toast) || tree.inert(toast) == Some(true))
    }

    /// Whether the toast waits: under the active modal, inert, for want of
    /// room clear of it (see [Over a modal](Self#over-a-modal)), or out of
    /// view, for want of room in the column (see
    /// [Where toasts go](Self#where-toasts-go)). `None` if it is not a
    /// toast.
    pub fn waiting(tree: &Tree, toast: NodeId) -> Option<bool> {
        let view = tree.widget::<ToastView>(toast)?;
        Some(view.waiting || tree.get(toast).is_some_and(|node| node.parked))
    }

    /// The toast's action button, if it has one.
    pub fn action_button(tree: &Tree, toast: NodeId) -> Option<NodeId> {
        tree.widget::<ToastView>(toast)?;
        let row = *tree.children(toast)?.get(1)?;
        tree.children(row)?.first().copied()
    }

    /// The toast's close control.
    pub fn close_button(tree: &Tree, toast: NodeId) -> Option<NodeId> {
        tree.widget::<ToastView>(toast)?;
        let head = *tree.children(toast)?.first()?;
        tree.children(head)?
            .iter()
            .copied()
            .find(|&node| tree.widget::<ToastClose>(node).is_some())
    }
}

/// Shows its lines as their lengths only (ADR-0043).
impl fmt::Debug for Toast {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let lines: Vec<TextLength<'_>> = self.lines.iter().map(|line| TextLength(line)).collect();
        f.debug_struct("Toast")
            .field("kind", &self.kind)
            .field("lines", &lines)
            .field("action", &self.action.as_deref().map(TextLength))
            .finish()
    }
}

/// Whether the user holds the toast: the pointer is on it, a press went
/// down on it, or focus is in it.
fn user_holds(tree: &Tree, toast: NodeId) -> bool {
    tree.state(toast)
        .is_some_and(|state| state.hovered || state.pressed || state.focus_within)
}

/// Whether the column keeps still for the toast: the user holds it, or the
/// pointer is over where it shows, whether or not it is inert. Where the
/// pointer is, not what it hovers, so that a toast that starts or stops
/// waiting under a modal, which changes what is hovered, does not move the
/// column, which could move the modal, if it is anchored in a toast, and
/// so whether the toast waits.
fn keeps_still(tree: &Tree, toast: NodeId) -> bool {
    user_holds(tree, toast)
        || (tree.is_visible(toast)
            && tree
                .pointer
                .zip(tree.bounds(toast))
                .is_some_and(|(pointer, bounds)| bounds.contains(pointer)))
}

/// Whether `a` and `b` share any area.
fn overlap(a: Rect, b: Rect) -> bool {
    a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
}

/// How much a sum of `f32` heights may pass the room it is measured
/// against and still count as fitting.
const SLACK: f32 = 0.01;

/// The lowest lift, from `from` up, at which a toast `height` high stands
/// [`TOAST_GAP`] clear of each span of `taken` (from a lift to its top),
/// with its top at most `limit`; `None` if there is none.
fn lowest_fit(taken: &[(f32, f32)], from: f32, height: f32, limit: f32) -> Option<f32> {
    let clear = |lift: f32| {
        taken.iter().all(|&(start, end)| {
            lift + height + TOAST_GAP <= start + SLACK || lift + SLACK >= end + TOAST_GAP
        })
    };
    let mut tries: Vec<f32> = std::iter::once(from)
        .chain(
            taken
                .iter()
                .map(|&(_, end)| end + TOAST_GAP)
                .filter(|&lift| lift >= from),
        )
        .collect();
    tries.sort_by(f32::total_cmp);
    tries
        .into_iter()
        .find(|&lift| clear(lift) && lift + height <= limit + SLACK)
}

impl Tree {
    /// Gives each toast its place in the column, as [`Toast`]'s
    /// documentation says, from the sizes this layout pass measured with
    /// `metrics`: its lift, or out of view for want of room. Before a pass
    /// (`after_pass` false), it only asks for one if the toasts' hold
    /// began or ended since. Returns whether that changed a lift or what
    /// shows, so that layout must pass again.
    pub(crate) fn bind_toasts(&mut self, after_pass: bool, metrics: &dyn TextMetrics) -> bool {
        let toasts = Toast::toasts(self);
        if toasts.is_empty() {
            return false;
        }
        let placed = |tree: &Self, toast: NodeId| {
            tree.widget::<ToastView>(toast)
                .is_some_and(|view| view.placed)
        };
        // Held as of this pass's placements: a window that changed may
        // have moved a toast out from under the pointer. A toast not yet
        // placed holds nothing: it is out of view.
        if after_pass {
            self.refresh_hover();
        }
        let held = self.toast_pin.is_some()
            || toasts
                .iter()
                .any(|&toast| placed(self, toast) && keeps_still(self, toast));
        if !after_pass {
            // Which toasts are held, and so where the others stand, changed
            // since the last pass: pass again.
            let pinned = self.toast_pin.is_some();
            return toasts.iter().any(|&toast| {
                let holds = placed(self, toast) && keeps_still(self, toast);
                self.widget::<ToastView>(toast)
                    .is_some_and(|view| view.seen != (holds, pinned))
            });
        }
        let window = self.bounds(self.root()).unwrap_or_default();
        let room_was = toasts
            .iter()
            .filter_map(|&toast| self.widget::<ToastView>(toast))
            .find(|view| view.placed)
            .map(|view| view.room);
        // Each toast's height, or `None` if its owner hid it.
        let mut sizes: Vec<(NodeId, Option<f32>)> = Vec::with_capacity(toasts.len());
        let mut room = 0.0;
        for &toast in &toasts {
            let hidden = self.style(toast).is_none_or(|style| style.hidden);
            let (size, area) = self.window_overlay_fit(toast, window, TOAST_MARGIN, metrics);
            room = area;
            sizes.push((toast, (!hidden).then_some(size.height)));
        }
        // (toast, its lift if it shows, whether it is placed).
        let mut places: Vec<(NodeId, Option<f32>)> = Vec::with_capacity(toasts.len());
        // A window of another height moves every toast: they settle anew.
        if held && room_was.is_none_or(|was| was == room) {
            // The held toasts keep their places, and the others theirs where
            // they meet nothing: a toast that grew pushes the ones above it
            // up, or out of view if there is no room. A toast not placed
            // goes above them all, if there is room, and waits out of view
            // if not.
            let shown = |tree: &Self, toast: NodeId| {
                placed(tree, toast) && !tree.get(toast).is_some_and(|node| node.parked)
            };
            let mut kept: Vec<(NodeId, f32, f32, bool)> = sizes
                .iter()
                .filter(|&&(toast, _)| shown(self, toast))
                .filter_map(|&(toast, height)| {
                    let lift = self.overlay_lift(toast).unwrap_or(0.0);
                    height.map(|height| (toast, lift, height, keeps_still(self, toast)))
                })
                .collect();
            // The held first, then the rest, each from the corner up.
            kept.sort_by(|a, b| b.3.cmp(&a.3).then(a.1.total_cmp(&b.1)));
            let mut taken: Vec<(f32, f32)> = Vec::new();
            let mut lifts: Vec<(NodeId, Option<f32>)> = Vec::new();
            for &(toast, lift, height, held) in &kept {
                // A held toast stays where it is if it can; else the lowest
                // place clear of the others; else, as any other, out of
                // view: no two overlap, ever.
                let lift = lowest_fit(&taken, lift, height, room).or_else(|| {
                    held.then(|| lowest_fit(&taken, 0.0, height, room))
                        .flatten()
                });
                if let Some(lift) = lift {
                    taken.push((lift, lift + height));
                }
                lifts.push((toast, lift));
            }
            let mut top = taken
                .iter()
                .map(|&(_, end)| end + TOAST_GAP)
                .fold(0.0, f32::max);
            for &(toast, height) in &sizes {
                if let Some(&(_, lift)) = lifts.iter().find(|(kept, _)| *kept == toast) {
                    places.push((toast, lift));
                    continue;
                }
                match height {
                    Some(height) if top + height <= room + SLACK => {
                        places.push((toast, Some(top)));
                        top += height + TOAST_GAP;
                    }
                    _ => places.push((toast, None)),
                }
            }
        } else {
            // From the corner up, the newest first; once one has no room,
            // it and every older one wait.
            let mut lift = 0.0;
            let mut fits = true;
            for &(toast, height) in sizes.iter().rev() {
                match height {
                    Some(height) if fits && lift + height <= room + SLACK => {
                        places.push((toast, Some(lift)));
                        lift += height + TOAST_GAP;
                    }
                    Some(_) => {
                        fits = false;
                        places.push((toast, None));
                    }
                    None => places.push((toast, None)),
                }
            }
        }
        let mut again = false;
        let pinned = self.toast_pin.is_some();
        for (toast, lift) in places {
            let hidden = sizes
                .iter()
                .any(|&(id, height)| id == toast && height.is_none());
            if let Some(lift) = lift {
                again |= self.set_overlay_lift(toast, lift);
            }
            // Waiting for room is out of view; hidden by its owner, it is
            // placed anew when shown again.
            again |= self.park(toast, lift.is_none() && !hidden);
            let holds = lift.is_some() && keeps_still(self, toast);
            if let Some(view) = view_mut(self, toast) {
                view.placed = lift.is_some();
                view.seen = (holds, pinned);
                view.room = room;
            }
        }
        again
    }

    /// Puts each toast in its place in the order of the overlays, worked
    /// out anew from when each overlay opened and each toast showed (see
    /// [Over a modal](Toast#over-a-modal)):
    ///
    /// - a toast's home is just above the block (the overlay and those
    ///   its nodes host) of the last modal opened before it showed, shown
    ///   or hidden, or just above the overlays the main root's nodes host if
    ///   there is none; toasts with one home stand in the order they
    ///   showed;
    /// - a toast whose home is above the active modal *waits* just under
    ///   it while its block overlaps the modal's block, from the bounds of
    ///   the last layout pass, or while it waits out of view for room in
    ///   the column.
    ///
    /// The other overlays keep their order, and every block stays whole.
    /// The result depends only on that order, the toasts, and the bounds,
    /// so doing it again changes nothing. Returns whether the order
    /// changed; what it moved is damaged at the next frame. Focus, the
    /// hovered path and a press are the caller's to settle.
    pub(crate) fn order_toasts(&mut self) -> bool {
        let starts: Vec<usize> = (0..self.overlays.len())
            .filter(|&at| {
                self.overlays.get(at).is_some_and(|layer| {
                    self.get(layer.root)
                        .and_then(|node| node.widget.as_deref())
                        .is_some_and(is_toast)
                })
            })
            .collect();
        if starts.is_empty() {
            return false;
        }
        let before: Vec<NodeId> = self.overlays.iter().map(|layer| layer.root).collect();
        // The rects each layer of a block shows, from the last layout.
        let shown = |tree: &Self, roots: &[NodeId]| -> Vec<Rect> {
            roots
                .iter()
                .filter(|&&root| tree.is_visible(root))
                .filter_map(|&root| tree.bounds(root))
                .collect()
        };
        // Take the toasts' blocks out; each is whole, and holds no other
        // toast, as a toast is hosted by nothing.
        let mut blocks: Vec<(u64, Vec<NodeId>, Vec<Layer>)> = Vec::new();
        let mut taken = vec![false; self.overlays.len()];
        for &start in &starts {
            let end = self.block_end(start);
            let roots: Vec<NodeId> = self.overlays[start..end]
                .iter()
                .map(|layer| layer.root)
                .collect();
            let seq = self.overlays.get(start).map_or(0, |layer| layer.seq);
            blocks.push((seq, roots, Vec::new()));
            for flag in taken.iter_mut().take(end).skip(start) {
                *flag = true;
            }
        }
        let block_roots: Vec<NodeId> = blocks
            .iter()
            .flat_map(|(_, roots, _)| roots.iter().copied())
            .collect();
        let all = std::mem::take(&mut self.overlays);
        let mut moved: Vec<Layer> = Vec::new();
        for (at, layer) in all.into_iter().enumerate() {
            if taken.get(at).copied().unwrap_or(false) {
                moved.push(layer);
            } else {
                self.overlays.push(layer);
            }
        }
        for layer in moved {
            if let Some(block) = blocks
                .iter_mut()
                .find(|(_, roots, _)| roots.contains(&layer.root))
            {
                block.2.push(layer);
            }
        }
        // Now `self.overlays` holds the rest, whose order and blocks stand.
        let active = self.active_modal_index();
        let active_rects = active.map_or_else(Vec::new, |at| {
            let roots: Vec<NodeId> = self.overlays[at..self.block_end(at)]
                .iter()
                .map(|layer| layer.root)
                .collect();
            shown(self, &roots)
        });
        let mut slots: Vec<(usize, u64, bool, Vec<Layer>)> = Vec::new();
        for (seq, roots, layers) in blocks {
            let after = self
                .overlays
                .iter()
                .enumerate()
                .filter(|(_, layer)| layer.kind == OverlayKind::Modal && layer.seq < seq)
                .max_by_key(|(_, layer)| layer.seq)
                .map(|(at, _)| at);
            let home = match after {
                Some(at) => self.block_end(at),
                None => self.group_end(self.root()),
            };
            // Out of view for want of room, it waits under the modal too, so
            // that every waiting toast stands there.
            let parked = roots
                .first()
                .and_then(|&root| self.get(root))
                .is_some_and(|node| node.parked);
            let waits = active.is_some_and(|at| {
                home > at
                    && (parked || {
                        let rects = shown(self, &roots);
                        rects
                            .iter()
                            .any(|&rect| active_rects.iter().any(|&modal| overlap(rect, modal)))
                    })
            });
            let slot = match (waits, active) {
                (true, Some(at)) => at,
                _ => home,
            };
            slots.push((slot, seq, waits, layers));
        }
        slots.sort_by_key(|&(slot, seq, _, _)| (slot, seq));
        let rest = std::mem::take(&mut self.overlays);
        let mut waits_of = Vec::new();
        let mut slots = slots.into_iter().peekable();
        for (at, layer) in rest.into_iter().enumerate() {
            while let Some((_, _, waits, layers)) = slots.next_if(|(slot, ..)| *slot <= at) {
                waits_of.extend(layers.first().map(|first| (first.root, waits)));
                self.overlays.extend(layers);
            }
            self.overlays.push(layer);
        }
        for (_, _, waits, layers) in slots {
            waits_of.extend(layers.first().map(|first| (first.root, waits)));
            self.overlays.extend(layers);
        }
        for (toast, waits) in waits_of {
            if let Some(view) = view_mut(self, toast) {
                view.waiting = waits;
            }
        }
        let after: Vec<NodeId> = self.overlays.iter().map(|layer| layer.root).collect();
        if after == before {
            return false;
        }
        self.restacked(&block_roots);
        self.layout_dirty = true;
        true
    }
}

/// A toast's root widget, to change, without marking its node.
fn view_mut(tree: &mut Tree, toast: NodeId) -> Option<&mut ToastView> {
    let widget: &mut dyn Widget = tree.get_mut(toast)?.widget.as_deref_mut()?;
    (widget as &mut dyn Any).downcast_mut()
}

/// Returns `true` if `widget` is a toast's root, whose overlay's anchor and
/// place in the order are the toast's.
pub(crate) fn is_toast(widget: &dyn Widget) -> bool {
    (widget as &dyn Any).is::<ToastView>()
}

/// A toast's box, the root of its overlay.
#[derive(Debug)]
pub(super) struct ToastView {
    /// Its kind's color.
    stripe: Token,
    /// Whether a layout has given it its place in the column, which it
    /// keeps while the toasts are held.
    placed: bool,
    /// Whether it waits under the active modal for want of room clear of
    /// it, as the order was last worked out.
    waiting: bool,
    /// Whether the user held it, and whether the column was pinned, at the
    /// last layout pass, so that the next passes again if that changed.
    seen: (bool, bool),
    /// The height of the room the column had then.
    room: f32,
}

impl Widget for ToastView {
    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let bounds = cx.bounds();
        let theme = cx.theme();
        dimmed(cx, list, |list| {
            list.rect(bounds, theme.solid(Token::Panel));
            list.border(bounds, 1.0, theme.color(Token::Line));
            list.rect(
                Rect::new(bounds.x, bounds.y, TOAST_STRIPE, bounds.height),
                theme.color(self.stripe),
            );
        });
    }

    fn event(&mut self, cx: &mut EventContext<'_>, event: &InputEvent) -> Handled {
        match *event {
            InputEvent::KeyDown {
                key: Key::Escape,
                modifiers,
                repeat,
            } if modifiers.difference(Modifiers::SHIFT).is_empty() => {
                if !repeat {
                    cx.emit(ActionKind::Dismiss);
                }
                Handled::Yes
            }
            _ => Handled::No,
        }
    }
}

/// A toast's text.
struct ToastText {
    lines: Vec<String>,
}

impl Widget for ToastText {
    fn measure(&self, _available: Size, metrics: &dyn TextMetrics) -> Size {
        Size::new(
            0.0,
            self.lines.len() as f32 * line_height(metrics, &TOAST_TEXT),
        )
    }

    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let bounds = cx.bounds();
        let height = line_height(cx.metrics(), &TOAST_TEXT);
        let color = cx.theme().color(Token::Text);
        dimmed(cx, list, |list| {
            list.push_clip(bounds);
            let mut y = bounds.y;
            for line in &self.lines {
                if !line.is_empty() {
                    let rect = Rect::new(bounds.x, y, bounds.width, height);
                    list.text(
                        Point::new(bounds.x, baseline(rect, cx.metrics(), &TOAST_TEXT)),
                        line.clone(),
                        TOAST_TEXT,
                        color,
                    );
                }
                y += height;
            }
            list.pop_clip();
        });
    }
}

/// A toast's close control: a cross, on a circle when hovered or pressed,
/// as a tab's.
#[derive(Debug, Default)]
struct ToastClose {
    activator: Activator,
}

impl Widget for ToastClose {
    fn measure(&self, _available: Size, _metrics: &dyn TextMetrics) -> Size {
        Size::new(TOAST_CLOSE, TOAST_CLOSE)
    }

    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let bounds = cx.bounds();
        let theme = cx.theme();
        let look = Look::of(cx.state());
        dimmed(cx, list, |list| {
            let (fill, cross) = match look {
                Look::Rest => (None, Token::Text2),
                Look::Hover => (Some(Token::TealWash), Token::TealHi),
                Look::Pressed => (Some(Token::TealFill), Token::TealHi),
            };
            if let Some(fill) = fill {
                list.pill(bounds, theme.color(fill));
            }
            let centre = Point::new(
                bounds.x + bounds.width / 2.0,
                bounds.y + bounds.height / 2.0,
            );
            let color = theme.color(cross);
            list.line(
                Point::new(centre.x - CROSS, centre.y - CROSS),
                Point::new(centre.x + CROSS, centre.y + CROSS),
                CROSS_STROKE,
                color,
            );
            list.line(
                Point::new(centre.x - CROSS, centre.y + CROSS),
                Point::new(centre.x + CROSS, centre.y - CROSS),
                CROSS_STROKE,
                color,
            );
        });
        if cx.state().focus_visible {
            focus_outline(list, bounds, theme);
        }
    }

    fn event(&mut self, cx: &mut EventContext<'_>, event: &InputEvent) -> Handled {
        let (activated, handled) = self.activator.event(cx, event);
        if activated {
            cx.emit(ActionKind::Dismiss);
        }
        handled
    }
}

impl Control for ToastClose {
    fn semantics(&self) -> Semantics {
        Semantics::new(Role::Button, "Close")
    }

    fn focusable(&self) -> bool {
        true
    }
}
