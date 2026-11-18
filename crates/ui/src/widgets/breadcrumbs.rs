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

//! Breadcrumbs (`.breadcrumbs`): a row of path segments as written, with
//! `teal` separators, the last one the current page.

use tidepool_core::geom::{Point, Size};
use tidepool_core::input::InputEvent;

use crate::a11y::{Role, Semantics};
use crate::draw::{DrawList, PaintContext};
use crate::error::UiError;
use crate::input::{EventContext, Handled};
use crate::layout::{Align, Style};
use crate::metrics::TextMetrics;
use crate::theme::Token;
use crate::tree::{Node, NodeId, Tree, Widget};

use super::button::Activator;
use super::{CRUMB_GAP, Control, Look, PATH, baseline, dimmed, focus_outline, line_height};

/// What separates two crumbs.
const SLASH: &str = "/";

/// Breadcrumbs: a navigation row of [`Crumb`]s, one for each segment of a
/// path, with a `/` in `teal` between each two, [`CRUMB_GAP`] either side.
///
/// It is a builder, not a widget: [`insert`](Self::insert) adds the row,
/// a navigation node named for it, with a crumb and a separator node for
/// each segment; [`set_segments`](Self::set_segments) rebuilds them. Each
/// segment but the last is a link, which takes focus and emits
/// [`ActionKind::Activate`](crate::input::ActionKind::Activate); the last
/// is the current page, a label. Separators are not exposed.
///
/// ```
/// use tidepool_ui::Tree;
/// use tidepool_ui::widgets::Breadcrumbs;
///
/// let mut tree = Tree::new();
/// let crumbs = Breadcrumbs::new("Breadcrumbs", ["crates", "ui", "widgets.rs"]);
/// let root = tree.root();
/// let nav = crumbs.insert(&mut tree, root)?;
/// assert_eq!(
///     tree.accessibility().to_string(),
///     "navigation \"Breadcrumbs\"\n  link \"crates\" focusable\n  link \"ui\" focusable\n  \
///      label \"widgets.rs\"\n"
/// );
/// assert_eq!(tree.children(nav).map(<[_]>::len), Some(5));
/// # Ok::<(), tidepool_ui::UiError>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Breadcrumbs {
    name: String,
    segments: Vec<String>,
}

impl Breadcrumbs {
    /// Breadcrumbs named `name`, of `segments`, from the outermost.
    pub fn new<S: Into<String>>(
        name: impl Into<String>,
        segments: impl IntoIterator<Item = S>,
    ) -> Self {
        Self {
            name: name.into(),
            segments: segments.into_iter().map(Into::into).collect(),
        }
    }

    /// Adds the row as the last child of `parent`, with its crumbs, and
    /// returns its id.
    ///
    /// # Errors
    ///
    /// [`UiError::Unnamed`] for a blank segment but the last, which would
    /// be a link without a name, before the tree changes; and the errors
    /// of [`Tree::insert`], after which the row is removed again. Either
    /// way the tree is as it was.
    pub fn insert(self, tree: &mut Tree, parent: NodeId) -> Result<NodeId, UiError> {
        check(&self.segments)?;
        let row = Node::new(
            Style::row()
                .gap(CRUMB_GAP)
                .align(Align::Start, Align::Center),
        )
        .with_semantics(Semantics::new(Role::Navigation, self.name));
        let nav = tree.insert(parent, row)?;
        if let Err(error) = fill(tree, nav, &self.segments) {
            // The row was just inserted, and is not the root, so it can
            // be removed; were it not, the original error is still the one
            // to report.
            let _ = tree.remove(nav);
            return Err(error);
        }
        Ok(nav)
    }

    /// Replaces the crumbs of the row `nav`, made by
    /// [`insert`](Self::insert), with crumbs of `segments`.
    ///
    /// # Errors
    ///
    /// [`UiError::Unnamed`] for a blank segment but the last, before the
    /// tree changes; [`UiError::UnknownNode`] if `nav` is not in the tree;
    /// and the errors of [`Tree::remove`] and [`Tree::insert`], which leave
    /// the row with what was done.
    pub fn set_segments<S: AsRef<str>>(
        tree: &mut Tree,
        nav: NodeId,
        segments: &[S],
    ) -> Result<(), UiError> {
        let segments: Vec<String> = segments.iter().map(|s| s.as_ref().to_string()).collect();
        check(&segments)?;
        let children = tree
            .children(nav)
            .ok_or(UiError::UnknownNode { id: nav })?
            .to_vec();
        for child in children {
            tree.remove(child)?;
        }
        fill(tree, nav, &segments)
    }
}

/// Refuses segments of which one but the last is blank: it would be a
/// link, which takes focus, without a name.
fn check(segments: &[String]) -> Result<(), UiError> {
    let last = segments.len().saturating_sub(1);
    if segments[..last]
        .iter()
        .any(|segment| segment.trim().is_empty())
    {
        return Err(UiError::Unnamed { role: Role::Link });
    }
    Ok(())
}

/// Adds a crumb for each segment to `nav`, with a separator before each
/// but the first.
fn fill(tree: &mut Tree, nav: NodeId, segments: &[String]) -> Result<(), UiError> {
    let last = segments.len().saturating_sub(1);
    for (index, segment) in segments.iter().enumerate() {
        if index > 0 {
            tree.insert(nav, Node::default().with_widget(Separator))?;
        }
        tree.insert(nav, Crumb::new(segment.clone(), index == last).node())?;
    }
    Ok(())
}

/// One segment of [`Breadcrumbs`], in [`PATH`], as written: a link in
/// `text-2`, `teal-hi` when hovered and `teal` when pressed, or, for the
/// current page, a label in `text-3`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Crumb {
    text: String,
    current: bool,
    activator: Activator,
}

impl Crumb {
    /// A crumb reading `text`, the current page or a link.
    pub fn new(text: impl Into<String>, current: bool) -> Self {
        Self {
            text: text.into(),
            current,
            activator: Activator::default(),
        }
    }

    /// Its text, as written.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether it is the current page.
    pub const fn is_current(&self) -> bool {
        self.current
    }
}

impl Widget for Crumb {
    fn measure(&self, _available: Size, metrics: &dyn TextMetrics) -> Size {
        Size::new(
            metrics.width(&self.text, &PATH),
            line_height(metrics, &PATH),
        )
    }

    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let bounds = cx.bounds();
        let theme = cx.theme();
        let token = if self.current {
            Token::Text3
        } else {
            match Look::of(cx.state()) {
                Look::Rest => Token::Text2,
                Look::Hover => Token::TealHi,
                Look::Pressed => Token::Teal,
            }
        };
        dimmed(cx, list, |list| {
            let origin = Point::new(bounds.x, baseline(bounds, cx.metrics(), &PATH));
            list.text(origin, self.text.clone(), PATH, theme.color(token));
        });
        if cx.state().focus_visible {
            focus_outline(list, bounds, theme);
        }
    }

    fn event(&mut self, cx: &mut EventContext<'_>, event: &InputEvent) -> Handled {
        if self.current {
            return Handled::No;
        }
        self.activator.activate(cx, event)
    }
}

impl Control for Crumb {
    fn semantics(&self) -> Semantics {
        let role = if self.current {
            Role::Label
        } else {
            Role::Link
        };
        Semantics::new(role, self.text.clone())
    }

    fn focusable(&self) -> bool {
        !self.current
    }
}

/// The `/` between two crumbs, in `teal`; not exposed.
struct Separator;

impl Widget for Separator {
    fn measure(&self, _available: Size, metrics: &dyn TextMetrics) -> Size {
        Size::new(metrics.width(SLASH, &PATH), line_height(metrics, &PATH))
    }

    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let bounds = cx.bounds();
        dimmed(cx, list, |list| {
            let origin = Point::new(bounds.x, baseline(bounds, cx.metrics(), &PATH));
            list.text(origin, SLASH, PATH, cx.theme().color(Token::Teal));
        });
    }
}
