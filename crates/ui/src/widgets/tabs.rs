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

//! The tab strip: a row of tabs, one selected, for the shell's panels
//! (PROBLEMS, TERMINAL) and the editor's files (ADR-0052). jabbott.dev has
//! no tab style; its looks are derived from the tokens and the
//! collections'.

use std::any::Any;
use std::fmt;

use tidepool_core::geom::{Point, Rect, Size};
use tidepool_core::input::{InputEvent, Key, MouseButton, TextLength};

use crate::a11y::{Role, Semantics};
use crate::draw::{DrawList, PaintContext};
use crate::input::{ActionKind, EventContext, Handled};
use crate::layout::{Align, Insets, Length, Style};
use crate::metrics::TextMetrics;
use crate::theme::Token;
use crate::tree::{Node, NodeId, Tree, Widget};

use super::{
    Control, FOCUS_WIDTH, Look, MAX_TAB_LABEL_BYTES, MAX_TABS, ROW_TEXT, TAB_CLOSE, TAB_CLOSE_GAP,
    TAB_HEIGHT, TAB_LABEL, TAB_PADDING, TAB_RULE, baseline, cut_to, dimmed,
};

/// How far a close control's cross reaches from its centre.
pub(super) const CROSS: f32 = 3.5;

/// The width of a close control's strokes.
pub(super) const CROSS_STROKE: f32 = 1.5;

/// What a tab strip's labels are, which sets how they are painted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TabKind {
    /// The shell's chrome, a panel's name (PROBLEMS, TERMINAL): painted
    /// uppercase in [`TAB_LABEL`].
    #[default]
    Chrome,
    /// Content, a file's name: painted as written in
    /// [`ROW_TEXT`](super::ROW_TEXT).
    Content,
}

/// One tab of a [`TabStrip`]: its label, cut to
/// [`MAX_TAB_LABEL_BYTES`] at a grapheme cluster boundary, and whether it
/// can be closed. Its `Debug` shows the label's length only, as it may
/// be the user's (ADR-0043).
///
/// ```
/// use tidepool_ui::widgets::Tab;
///
/// let tab = Tab::new("main.rs").closable();
/// assert_eq!((tab.label(), tab.is_closable()), ("main.rs", true));
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct Tab {
    label: String,
    closable: bool,
}

impl Tab {
    /// A tab labelled `label`, which cannot be closed.
    pub fn new(label: impl Into<String>) -> Self {
        let label: String = label.into();
        Self {
            label: cut_to(&label, MAX_TAB_LABEL_BYTES).to_string(),
            closable: false,
        }
    }

    /// The same, with a close control.
    #[must_use]
    pub fn closable(self) -> Self {
        Self {
            closable: true,
            ..self
        }
    }

    /// Its label, as written.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Whether it can be closed.
    pub const fn is_closable(&self) -> bool {
        self.closable
    }
}

/// Shows the label as its length only.
impl fmt::Debug for Tab {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tab")
            .field("label", &TextLength(&self.label))
            .field("closable", &self.closable)
            .finish()
    }
}

/// Where a tab strip's tabs are, from the last time it bound them.
#[derive(Clone, Debug, Default, PartialEq)]
struct TabsView {
    /// Whether this widget's track was built.
    built: bool,
    /// What it bound: the revision and the selection.
    bound: Option<(u64, Option<usize>)>,
    /// The track's bounds, from the strip's origin: where tabs show.
    track: Rect,
    /// Each tab's bounds and its close control's, from the strip's origin.
    rects: Vec<(Rect, Option<Rect>)>,
    /// The strip's [`edits`](TabStrip::edits) when it found the rects: a
    /// press finds a tab in them only while they are the same.
    stamp: u64,
    /// Where the selected tab was in the track the last time it was
    /// scrolled into view.
    revealed: Option<Revealed>,
}

/// Where a strip's selected tab is in its track, which scrolling does not
/// change: when it changes, the tab is scrolled into view again.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Revealed {
    /// The selected tab.
    index: usize,
    /// Where it starts, from the start of the track's content.
    at: f32,
    /// How wide it is.
    width: f32,
    /// How wide the track shows.
    view: f32,
}

impl Revealed {
    /// Whether it is where `other` is, give or take a hundredth of a unit
    /// of layout's rounding.
    fn near(self, other: Self) -> bool {
        let near = |a: f32, b: f32| (a - b).abs() <= 0.01;
        self.index == other.index
            && near(self.at, other.at)
            && near(self.width, other.width)
            && near(self.view, other.view)
    }
}

/// A tab strip (ADR-0052): a row of [`Tab`]s, of which one is selected
/// whenever there are any.
///
/// # Keys and the pointer
///
/// It is one tab stop, as the WAI-ARIA tabs pattern has a tab list: it
/// takes focus itself and exposes its selected tab as selected, so that
/// focus never has to move between its tabs' nodes. While it has focus,
/// with no modifier: Left and Right select the tab before or after,
/// wrapping at the ends, Home and End the first and last, and Delete asks
/// to close the selected tab if it can be closed; the selection follows
/// the keys, as the pattern's automatic activation does. A press on a
/// tab selects it; a press on its close control, or a middle press on a
/// closable tab, asks to close it, and selects nothing. Other keys, and
/// keys with a modifier, go on to the owner.
///
/// Every action is of the strip's node, about the tab
/// [`target`](Self::target) names: [`ActionKind::Select`] when the
/// selection moves, and [`ActionKind::Dismiss`] when a tab is to be
/// closed, which the owner does with [`remove`](Self::remove), through
/// [`widgets::update`](super::update).
///
/// A press finds its tab where the last frame showed the tabs: after the
/// owner inserts, removes or relabels a tab, and before the next frame
/// shows that, a press on a tab does nothing, so that it never selects or
/// closes a tab other than the one the user saw.
///
/// # Overflow
///
/// Its tabs are as wide as their labels, in a track that scrolls
/// sideways in the tree's own terms and clips them: when they do not fit,
/// the track scrolls the least that shows the selected tab, as
/// [`Tree::scroll_into_view`] does within the track, at the layout after
/// the tab is selected or moves in the track (a tab inserted, removed or
/// relabelled before it) or the track's width changes. A sideways wheel
/// scrolls them, and they stay where it leaves them until then. It holds
/// at most [`MAX_TABS`] tabs.
///
/// # Accessibility
///
/// It is a tab list, named as given, cut to [`MAX_TAB_LABEL_BYTES`], its
/// value the selected tab's place
/// (`"tab 2 of 3"`); its tabs are tabs named by their labels as written,
/// the selected one selected, and a closable tab holds a button named
/// "Close" and its label. The panel a tab shows is the owner's, which
/// gives it [`Role::TabPanel`].
///
/// # Looks
///
/// A tab's label is `text-2`, the selected tab's `text` over a
/// [`TAB_RULE`]-thick `teal` rule; a hovered tab has the `row-hover` wash
/// and a `teal-hi` label, and a pressed one `teal-wash`. A close control
/// is a `text-2` cross, `teal-hi` on a `teal-wash` circle when hovered and
/// on `teal-fill` when pressed. A `line` rule runs under the tabs. Its
/// focus outline is drawn inside its bounds, in the [`FOCUS_WIDTH`] of
/// padding its default style leaves. Disabled, it has its rest look at
/// [`DISABLED_OPACITY`](super::DISABLED_OPACITY).
///
/// ```
/// use tidepool_core::geom::Size;
/// use tidepool_core::input::{InputEvent, Key, Modifiers};
/// use tidepool_ui::input::ActionKind;
/// use tidepool_ui::metrics::FixedMetrics;
/// use tidepool_ui::widgets::{Control, Tab, TabKind, TabStrip};
/// use tidepool_ui::Tree;
///
/// let tabs = [Tab::new("Problems"), Tab::new("Output"), Tab::new("Terminal")];
/// let mut tree = Tree::new();
/// let strip = tree.insert(tree.root(), TabStrip::new("Panel", TabKind::Chrome, tabs).node())?;
/// tree.frame(Size::new(400.0, 40.0), &FixedMetrics::DEFAULT);
///
/// // Left from the first tab wraps to the last.
/// tree.set_focus(strip, true)?;
/// let left = InputEvent::KeyDown { key: Key::Left, modifiers: Modifiers::NONE, repeat: false };
/// let response = tree.handle(&left);
/// assert_eq!(response.actions[0].kind, ActionKind::Select);
/// assert_eq!(tree.widget::<TabStrip>(strip).and_then(TabStrip::selected), Some(2));
///
/// tree.frame(Size::new(400.0, 40.0), &FixedMetrics::DEFAULT);
/// assert_eq!(
///     tree.accessibility().to_string(),
///     "tab list \"Panel\" = \"tab 3 of 3\" focusable focused\n  tab \"Problems\"\n  \
///      tab \"Output\"\n  tab \"Terminal\" selected\n"
/// );
/// # Ok::<(), tidepool_ui::UiError>(())
/// ```
pub struct TabStrip {
    kind: TabKind,
    name: String,
    tabs: Vec<Tab>,
    selected: Option<usize>,
    target: Option<usize>,
    /// Whether the next binding after a pass scrolls the selected tab
    /// into view.
    reveal: bool,
    /// Counts the changes to the tabs and the selection.
    revision: u64,
    /// Counts the changes that move tabs: inserts, removes and labels.
    edits: u64,
    view: TabsView,
}

impl TabStrip {
    /// A strip named `name`, cut to [`MAX_TAB_LABEL_BYTES`] at a
    /// grapheme cluster boundary, of `tabs` of `kind`, the first selected;
    /// at most [`MAX_TABS`] are kept.
    pub fn new(
        name: impl Into<String>,
        kind: TabKind,
        tabs: impl IntoIterator<Item = Tab>,
    ) -> Self {
        let tabs: Vec<Tab> = tabs.into_iter().take(MAX_TABS).collect();
        let name: String = name.into();
        Self {
            kind,
            name: cut_to(&name, MAX_TAB_LABEL_BYTES).to_string(),
            selected: (!tabs.is_empty()).then_some(0),
            tabs,
            target: None,
            reveal: true,
            revision: 0,
            edits: 0,
            view: TabsView::default(),
        }
    }

    /// What its labels are.
    pub const fn kind(&self) -> TabKind {
        self.kind
    }

    /// Its name, as written, cut to [`MAX_TAB_LABEL_BYTES`].
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Its tabs, in order.
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    /// How many tabs it has.
    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    /// Whether it has no tabs.
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// The selected tab: always one of its tabs, and `None` only when it
    /// has none.
    pub const fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// The tab its last action was about: the selected tab for
    /// [`ActionKind::Select`], the tab to close for
    /// [`ActionKind::Dismiss`]. It moves with its tab as the owner inserts
    /// and removes others, and is `None` once its tab is removed, so it is
    /// always one of its tabs.
    pub const fn target(&self) -> Option<usize> {
        self.target
    }

    /// Selects tab `index`, cut to the last, and scrolls it into view at
    /// the next layout. It emits nothing: actions come from the user.
    pub fn select(&mut self, index: usize) {
        self.selected = self.tabs.len().checked_sub(1).map(|last| index.min(last));
        self.reveal = true;
        self.revision = self.revision.wrapping_add(1);
    }

    /// Inserts `tab` at `at`, cut to the end, keeping the selection and
    /// the [`target`](Self::target) on their tabs, or selecting it if it is
    /// the only one; returns `false`, and changes nothing, if the strip
    /// holds [`MAX_TABS`].
    pub fn insert(&mut self, at: usize, tab: Tab) -> bool {
        if self.tabs.len() >= MAX_TABS {
            return false;
        }
        let at = at.min(self.tabs.len());
        self.tabs.insert(at, tab);
        let after = |index: usize| if index >= at { index + 1 } else { index };
        self.selected = Some(self.selected.map_or(at, after));
        self.target = self.target.map(after);
        self.changed();
        true
    }

    /// Removes tab `index`, if there is one, and returns it: a selection
    /// after it moves with its tab, and a selection of it moves to the
    /// tab now in its place, or the last, or none if none is left. The
    /// [`target`](Self::target) moves with its tab too, and is `None` if
    /// it was this one.
    pub fn remove(&mut self, index: usize) -> Option<Tab> {
        if index >= self.tabs.len() {
            return None;
        }
        let tab = self.tabs.remove(index);
        let last = self.tabs.len().checked_sub(1);
        self.selected = match self.selected {
            Some(selected) if selected > index => Some(selected - 1),
            Some(selected) if selected == index => last.map(|last| index.min(last)),
            other => other,
        };
        self.target = match self.target {
            Some(target) if target > index => Some(target - 1),
            Some(target) if target == index => None,
            other => other,
        };
        self.reveal = true;
        self.changed();
        Some(tab)
    }

    /// Labels tab `index` `label`, if there is one; returns whether there
    /// was.
    pub fn set_label(&mut self, index: usize, label: impl Into<String>) -> bool {
        let Some(tab) = self.tabs.get_mut(index) else {
            return false;
        };
        let closable = tab.closable;
        *tab = Tab {
            closable,
            ..Tab::new(label)
        };
        self.changed();
        true
    }

    /// Counts a change that may move tabs.
    fn changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.edits = self.edits.wrapping_add(1);
    }

    /// The label of tab `index` as it is painted: uppercase for chrome.
    fn shown(&self, tab: &Tab) -> String {
        match self.kind {
            TabKind::Chrome => tab.label.to_uppercase(),
            TabKind::Content => tab.label.clone(),
        }
    }

    /// Selects `index`, scrolling it into view, and tells the owner if
    /// the selection moved; does nothing if there is no tab `index`.
    fn choose(&mut self, cx: &mut EventContext<'_>, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        let moved = self.selected != Some(index);
        self.selected = Some(index);
        self.target = Some(index);
        self.reveal = true;
        self.revision = self.revision.wrapping_add(1);
        if moved {
            cx.emit(ActionKind::Select);
            cx.set_semantics(self.semantics());
        }
        cx.request_layout();
    }

    /// Asks the owner to close tab `index`, if it can be closed.
    fn dismiss(&mut self, cx: &mut EventContext<'_>, index: usize) -> Handled {
        if !self.tabs.get(index).is_some_and(Tab::is_closable) {
            return Handled::No;
        }
        self.target = Some(index);
        cx.emit(ActionKind::Dismiss);
        Handled::Yes
    }

    /// The tab under `point`, and whether it is on its close control,
    /// where the last binding put them; none if the owner moved tabs since.
    fn hit(&self, origin: Rect, point: Point) -> Option<(usize, bool)> {
        if self.view.stamp != self.edits {
            return None;
        }
        let at = |rect: Rect| {
            Rect::new(
                origin.x + rect.x,
                origin.y + rect.y,
                rect.width,
                rect.height,
            )
        };
        if !at(self.view.track).contains(point) {
            return None;
        }
        self.view
            .rects
            .iter()
            .enumerate()
            .find(|(_, (tab, _))| at(*tab).contains(point))
            .map(|(index, (_, close))| {
                (index, close.is_some_and(|close| at(close).contains(point)))
            })
            .filter(|&(index, _)| index < self.tabs.len())
    }

    /// Handles a key going down while it has focus, with no modifier.
    fn key(&mut self, cx: &mut EventContext<'_>, key: Key) -> Handled {
        let Some(last) = self.tabs.len().checked_sub(1) else {
            return if matches!(key, Key::Left | Key::Right | Key::Home | Key::End) {
                Handled::Yes
            } else {
                Handled::No
            };
        };
        let current = self.selected.unwrap_or(0);
        let to = match key {
            Key::Left => current.checked_sub(1).unwrap_or(last),
            Key::Right => {
                if current >= last {
                    0
                } else {
                    current + 1
                }
            }
            Key::Home => 0,
            Key::End => last,
            Key::Delete => return self.dismiss(cx, current),
            _ => return Handled::No,
        };
        self.choose(cx, to);
        Handled::Yes
    }
}

/// Shows what it is, never its labels.
impl fmt::Debug for TabStrip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TabStrip")
            .field("kind", &self.kind)
            .field("name", &self.name)
            .field("tabs", &self.tabs.len())
            .field("selected", &self.selected)
            .field("target", &self.target)
            .finish_non_exhaustive()
    }
}

impl Widget for TabStrip {
    /// The `line` rule under its tabs, and its focus outline inside its
    /// bounds when focus shows.
    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let bounds = cx.bounds();
        let theme = cx.theme();
        dimmed(cx, list, |list| {
            let track = self.view.track;
            list.rect(
                Rect::new(
                    bounds.x + track.x,
                    bounds.y + track.y + track.height - 1.0,
                    track.width,
                    1.0,
                ),
                theme.color(Token::Line),
            );
        });
        if cx.state().focus_visible {
            list.border(bounds, FOCUS_WIDTH, theme.color(Token::Teal));
        }
    }

    fn event(&mut self, cx: &mut EventContext<'_>, event: &InputEvent) -> Handled {
        match *event {
            InputEvent::KeyDown { key, modifiers, .. }
                if cx.state().focused && modifiers.is_empty() =>
            {
                self.key(cx, key)
            }
            InputEvent::MouseDown {
                button, position, ..
            } => {
                let Some((index, on_close)) = self.hit(cx.bounds(), position) else {
                    return Handled::No;
                };
                match button {
                    MouseButton::Left if on_close => self.dismiss(cx, index),
                    MouseButton::Left => {
                        self.choose(cx, index);
                        Handled::Yes
                    }
                    MouseButton::Middle => self.dismiss(cx, index),
                    _ => Handled::No,
                }
            }
            _ => Handled::No,
        }
    }
}

impl Control for TabStrip {
    fn semantics(&self) -> Semantics {
        let value = match self.selected {
            Some(selected) => format!("tab {} of {}", selected + 1, self.tabs.len()),
            None => "no tabs".to_string(),
        };
        Semantics::new(Role::TabList, self.name.clone()).with_value(value)
    }

    /// As wide as its parent, as tall as its tabs, with [`FOCUS_WIDTH`]
    /// of padding on each side, where its focus outline is drawn.
    fn style(&self) -> Style {
        Style::column()
            .width(Length::Fill(1.0))
            .padding(Insets::all(FOCUS_WIDTH))
    }

    fn focusable(&self) -> bool {
        true
    }
}

impl Tree {
    /// Binds a tab strip's tab nodes to its tabs, if it is shown; returns
    /// whether that added, removed or resized nodes, or scrolled them, so
    /// that layout must pass again. After a pass (`after_pass`) that placed
    /// its nodes as they are, the track scrolls the selected tab into view
    /// if it was selected or moved, or the track's width changed; the
    /// tree's [`scroll_into_view`](Tree::scroll_into_view) is left to the
    /// owner and the collections.
    pub(crate) fn bind_tabs(&mut self, id: NodeId, after_pass: bool) -> bool {
        if !self.is_visible(id) {
            return false;
        }
        let Some(strip) = self.widget::<TabStrip>(id) else {
            return false;
        };
        let built = strip.view.built;
        let track = if built {
            self.children(id)
                .and_then(|children| children.first().copied())
                .filter(|&child| self.widget::<TabTrack>(child).is_some())
        } else {
            None
        };
        let (track, rebuilt) = match track {
            Some(track) => (track, false),
            None => {
                for child in self.children(id).map_or_else(Vec::new, <[_]>::to_vec) {
                    // A child of the strip, so not the root.
                    let _ = self.remove(child);
                }
                let Ok(track) = self.insert(id, track_node()) else {
                    return false;
                };
                if let Some(strip) = self.strip_mut(id) {
                    strip.view.built = true;
                    strip.view.bound = None;
                }
                (track, true)
            }
        };
        let Some(strip) = self.widget::<TabStrip>(id) else {
            return rebuilt;
        };
        let key = (strip.revision, strip.selected);
        let mut again = rebuilt;
        if strip.view.bound != Some(key) {
            let kind = strip.kind;
            let selected = strip.selected;
            let tabs: Vec<(Tab, String)> = strip
                .tabs
                .iter()
                .map(|tab| (tab.clone(), strip.shown(tab)))
                .collect();
            again |= self.fit_tabs(track, kind, &tabs, selected);
            if let Some(strip) = self.strip_mut(id) {
                strip.view.bound = Some(key);
            }
        }
        let nodes = self.children(track).map_or_else(Vec::new, <[_]>::to_vec);
        // Not when this binding changed the nodes: the next pass places
        // them, and the binding after it reveals.
        let shift = if after_pass && !again {
            self.reveal_tab(id, track, &nodes)
        } else {
            0.0
        };
        again |= shift != 0.0;
        // Where the tabs are now, for a press to find them: where the pass
        // a scroll makes will put them.
        let origin = self.bounds(id).unwrap_or_default();
        let relative = |rect: Rect, by: f32| {
            Rect::new(
                rect.x - origin.x - by,
                rect.y - origin.y,
                rect.width,
                rect.height,
            )
        };
        let track_rect = self
            .get(track)
            .map(|node| relative(node.placement.rect, 0.0))
            .unwrap_or_default();
        let rects: Vec<(Rect, Option<Rect>)> = nodes
            .iter()
            .map(|&node| {
                let rect = self
                    .bounds(node)
                    .map(|rect| relative(rect, shift))
                    .unwrap_or_default();
                let close = self
                    .children(node)
                    .and_then(|children| children.first().copied())
                    .and_then(|close| self.bounds(close))
                    .map(|rect| relative(rect, shift));
                (rect, close)
            })
            .collect();
        let semantics = self.widget::<TabStrip>(id).map(Control::semantics);
        if let Some(strip) = self.strip_mut(id) {
            strip.view.rects = rects;
            strip.view.stamp = strip.edits;
            // It paints the rule under the track, and nothing of its tabs.
            if strip.view.track != track_rect {
                strip.view.track = track_rect;
                self.mark(id);
            }
        }
        if let Some(semantics) = semantics
            && self.semantics(id) != Some(&semantics)
        {
            // Refused only for a blank name, which the tree refused at the
            // insert already.
            let _ = self.set_semantics(id, semantics);
        }
        again
    }

    /// Scrolls the strip's track the least that shows its selected tab, as
    /// placed by the last pass, if the tab was selected since, or moved in
    /// the track, or the track's width changed; returns how far the tabs
    /// move, the scroll's change, which the next pass makes.
    fn reveal_tab(&mut self, id: NodeId, track: NodeId, nodes: &[NodeId]) -> f32 {
        let Some(strip) = self.widget::<TabStrip>(id) else {
            return 0.0;
        };
        let (selected, asked, last) = (strip.selected, strip.reveal, strip.view.revealed);
        let Some((placement, offset)) = self.get(track).map(|node| (node.placement, node.scroll))
        else {
            return 0.0;
        };
        let view = placement.viewport;
        let tab = selected
            .and_then(|index| nodes.get(index))
            .and_then(|&node| self.bounds(node));
        let now = selected.zip(tab).map(|(index, rect)| Revealed {
            index,
            at: rect.x - view.x + offset.x,
            width: rect.width,
            view: view.width,
        });
        if let Some(strip) = self.strip_mut(id) {
            strip.reveal = false;
            strip.view.revealed = now;
        }
        let (Some(now), Some(rect)) = (now, tab) else {
            return 0.0;
        };
        if !asked && last.is_some_and(|last| last.near(now)) {
            return 0.0;
        }
        let moved = crate::layout::reveal(
            offset.x,
            (rect.x, rect.width),
            (view.x, view.width),
            placement.content_size.width - view.width,
        );
        if moved == offset.x {
            return 0.0;
        }
        if let Some(node) = self.get_mut(track) {
            node.scroll.x = moved;
        }
        self.mark(track);
        moved - offset.x
    }

    /// The strip's widget, to change, without marking its node.
    fn strip_mut(&mut self, id: NodeId) -> Option<&mut TabStrip> {
        let widget: &mut dyn Widget = self.get_mut(id)?.widget.as_deref_mut()?;
        (widget as &mut dyn Any).downcast_mut()
    }

    /// Gives the track a node for each of `tabs`, bound to it; returns
    /// whether a node was added or removed or a tab's width changed.
    fn fit_tabs(
        &mut self,
        track: NodeId,
        kind: TabKind,
        tabs: &[(Tab, String)],
        selected: Option<usize>,
    ) -> bool {
        let mut nodes = self.children(track).map_or_else(Vec::new, <[_]>::to_vec);
        let mut again = false;
        while nodes.len() < tabs.len() {
            let Ok(node) = self.insert(track, tab_node(kind)) else {
                break;
            };
            nodes.push(node);
            again = true;
        }
        for &node in nodes.iter().skip(tabs.len()).rev() {
            // A node of the track, so not the root.
            let _ = self.remove(node);
            again = true;
        }
        for (index, (&node, (tab, shown))) in nodes.iter().zip(tabs).enumerate() {
            let name = if tab.label.trim().is_empty() {
                format!("tab {}", index + 1)
            } else {
                tab.label.clone()
            };
            let mut semantics = Semantics::new(Role::Tab, name.clone());
            semantics.selected = selected == Some(index);
            let view = TabView {
                kind,
                shown: shown.clone(),
                closable: tab.closable,
                selected: semantics.selected,
            };
            let (changed, resized) = self.get_mut(node).map_or((false, false), |data| {
                data.semantics = semantics;
                let Some(current) = data
                    .widget
                    .as_deref_mut()
                    .and_then(|widget| (widget as &mut dyn Any).downcast_mut::<TabView>())
                else {
                    return (false, false);
                };
                let resized = current.shown != view.shown || current.closable != view.closable;
                let changed = *current != view;
                *current = view;
                (changed, resized)
            });
            if changed {
                self.mark(node);
            }
            if resized {
                self.layout_dirty = true;
                again = true;
            }
            // Its close control, if it can be closed.
            let close = self
                .children(node)
                .and_then(|children| children.first().copied());
            let label = format!("Close {name}");
            match (tab.closable, close) {
                (true, Some(close)) => {
                    if let Some(data) = self.get_mut(close) {
                        data.semantics = Semantics::new(Role::Button, label);
                    }
                }
                (true, None) => {
                    if self.insert(node, close_node(label)).is_ok() {
                        again = true;
                    }
                }
                (false, Some(close)) => {
                    let _ = self.remove(close);
                    again = true;
                }
                (false, None) => {}
            }
        }
        again
    }
}

/// The track a strip's tabs sit in, which scrolls them sideways.
fn track_node() -> Node {
    Node::new(
        Style::row()
            .width(Length::Fill(1.0))
            .height(Length::Fixed(TAB_HEIGHT))
            .scroll(true, false),
    )
    .with_widget(TabTrack)
}

/// A tab's node of `kind`, bound to no tab yet.
fn tab_node(kind: TabKind) -> Node {
    Node::new(
        Style::row()
            .height(Length::Fill(1.0))
            .padding(Insets::symmetric(TAB_PADDING, 0.0))
            .align(Align::End, Align::Center),
    )
    .with_semantics(Semantics::new(Role::Tab, ""))
    .with_widget(TabView {
        kind,
        shown: String::new(),
        closable: false,
        selected: false,
    })
}

/// A tab's close control, named `name`.
fn close_node(name: String) -> Node {
    Node::new(Style::stack().fixed(TAB_CLOSE, TAB_CLOSE))
        .with_semantics(Semantics::new(Role::Button, name))
        .with_widget(TabClose)
}

/// The track of a strip's tabs; it paints nothing.
struct TabTrack;

impl Widget for TabTrack {}

/// A tab: its label, the selected tab's rule, and its hover and pressed
/// fills. Its `Debug` shows its label as its length only.
#[derive(PartialEq, Eq)]
struct TabView {
    kind: TabKind,
    shown: String,
    closable: bool,
    selected: bool,
}

/// Shows its label as its length only.
impl fmt::Debug for TabView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TabView")
            .field("kind", &self.kind)
            .field("shown", &TextLength(&self.shown))
            .field("closable", &self.closable)
            .field("selected", &self.selected)
            .finish()
    }
}

impl TabView {
    /// The style its label is painted in.
    const fn text(&self) -> crate::metrics::TextStyle {
        match self.kind {
            TabKind::Chrome => TAB_LABEL,
            TabKind::Content => ROW_TEXT,
        }
    }

    /// The width of its close control and the gap before it, if it has one.
    fn close_width(&self) -> f32 {
        if self.closable {
            TAB_CLOSE_GAP + TAB_CLOSE
        } else {
            0.0
        }
    }
}

impl Widget for TabView {
    fn measure(&self, _available: Size, metrics: &dyn TextMetrics) -> Size {
        let style = self.text();
        Size::new(metrics.width(&self.shown, &style) + self.close_width(), 0.0)
    }

    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let bounds = cx.bounds();
        let theme = cx.theme();
        let look = Look::of(cx.state());
        let style = self.text();
        dimmed(cx, list, |list| {
            match look {
                Look::Pressed => list.rect(bounds, theme.color(Token::TealWash)),
                Look::Hover => list.rect(bounds, theme.color(Token::RowHover)),
                Look::Rest => {}
            }
            if self.selected {
                list.rect(
                    Rect::new(
                        bounds.x,
                        bounds.y + bounds.height - TAB_RULE,
                        bounds.width,
                        TAB_RULE,
                    ),
                    theme.color(Token::Teal),
                );
            }
            let token = if look != Look::Rest {
                Token::TealHi
            } else if self.selected {
                Token::Text
            } else {
                Token::Text2
            };
            let text = Rect::new(
                bounds.x + TAB_PADDING,
                bounds.y,
                (bounds.width - 2.0 * TAB_PADDING - self.close_width()).max(0.0),
                bounds.height,
            );
            list.push_clip(text);
            list.text(
                Point::new(text.x, baseline(bounds, cx.metrics(), &style)),
                self.shown.clone(),
                style,
                theme.color(token),
            );
            list.pop_clip();
        });
    }
}

/// A tab's close control: a cross, on a circle when hovered or pressed.
struct TabClose;

impl Widget for TabClose {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(count: usize) -> TabStrip {
        TabStrip::new(
            "Tabs",
            TabKind::Content,
            (0..count).map(|n| Tab::new(format!("t{n}")).closable()),
        )
    }

    #[test]
    fn the_selection_follows_inserts_and_removes_and_is_one_tab_while_there_are_any() {
        let mut tabs = strip(4);
        assert_eq!(tabs.selected(), Some(0));
        tabs.select(2);
        assert!(tabs.insert(0, Tab::new("new")));
        assert_eq!(tabs.selected(), Some(3));
        assert!(tabs.insert(9, Tab::new("end")));
        assert_eq!((tabs.len(), tabs.selected()), (6, Some(3)));
        // Removing one before it moves it; removing it, the tab now in its
        // place; the last, the one before.
        tabs.remove(0);
        assert_eq!(tabs.selected(), Some(2));
        tabs.remove(2);
        assert_eq!(tabs.selected(), Some(2));
        tabs.select(9);
        assert_eq!(tabs.selected(), Some(3));
        tabs.remove(3);
        assert_eq!(tabs.selected(), Some(2));
        assert_eq!(tabs.remove(7), None);
        while !tabs.is_empty() {
            tabs.remove(0);
        }
        assert_eq!(tabs.selected(), None);
        // The first tab of an empty strip is selected.
        tabs.insert(0, Tab::new("only"));
        assert_eq!(tabs.selected(), Some(0));
    }

    #[test]
    fn a_press_finds_only_tabs_the_strip_has_and_only_where_they_were_shown() {
        let mut tabs = strip(2);
        // Rects for three tabs, as a strip too full to drop a node might
        // keep: a press on the third finds none.
        tabs.view.track = Rect::new(0.0, 0.0, 300.0, 30.0);
        tabs.view.rects = (0..3)
            .map(|k| (Rect::new(100.0 * k as f32, 0.0, 100.0, 30.0), None))
            .collect();
        tabs.view.stamp = tabs.edits;
        let at = |x: f32| Point::new(x, 15.0);
        assert_eq!(tabs.hit(Rect::ZERO, at(150.0)), Some((1, false)));
        assert_eq!(tabs.hit(Rect::ZERO, at(250.0)), None);
        // After a change that moves tabs, none until they are bound again.
        tabs.set_label(0, "longer");
        assert_eq!(tabs.hit(Rect::ZERO, at(150.0)), None);
        tabs.view.stamp = tabs.edits;
        assert_eq!(tabs.hit(Rect::ZERO, at(150.0)), Some((1, false)));
        // A selection moves none.
        tabs.select(1);
        assert_eq!(tabs.hit(Rect::ZERO, at(50.0)), Some((0, false)));
    }

    #[test]
    fn a_strip_holds_at_most_max_tabs_and_labels_are_capped() {
        let mut tabs = strip(MAX_TABS + 10);
        assert_eq!(tabs.len(), MAX_TABS);
        assert!(!tabs.insert(0, Tab::new("one more")));
        assert_eq!(tabs.len(), MAX_TABS);
        let long = Tab::new("é".repeat(MAX_TAB_LABEL_BYTES));
        assert_eq!(long.label().len(), MAX_TAB_LABEL_BYTES);
        let shown = format!("{long:?}");
        assert!(shown.contains("bytes>") && !shown.contains('é'), "{shown}");
    }
}
