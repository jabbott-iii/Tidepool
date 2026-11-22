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

//! The accessibility tree: each node's [`Role`] and [`Semantics`], and
//! [`Tree::accessibility`], which takes a [`Snapshot`] of the nodes a
//! screen reader would see.
//!
//! New in Tidepool; not ported.
//!
//! The snapshot holds every visible node whose role is not [`Role::None`],
//! in pre-order. A node with [`Role::None`] is not exposed; its children
//! attach to its nearest exposed ancestor. Hidden nodes and their subtrees
//! are left out. The operating systems' accessibility bridges read it from
//! M14.2.
//!
//! Overlays ([`overlay`](crate::overlay)) follow the main root's layer, in
//! the order they were opened, each layer's outermost exposed nodes at
//! depth 0. While a modal overlay is active, the snapshot holds only the
//! live layers, the modal's and those above it, as HTML's modal dialogs
//! and `inert` leave the rest out of the accessibility tree; the modal's
//! root is marked [`modal`](AccessNode::modal), as `aria-modal` would mark
//! it, if it has a role.
//!
//! A node with a tooltip ([`Tree::set_tooltip`]) has its text as its
//! [`description`](AccessNode::description), as `aria-describedby` points
//! a node at its tooltip. A node that keeps focus while its keys act on
//! the selected item of a list, as a command palette's field does, names
//! that item as its [`active`](AccessNode::active) option, as
//! `aria-activedescendant` does.
//!
//! The tree keeps an invariant by construction: **every focusable node has
//! a role and a name** ([`Semantics::is_named`]). [`Tree::insert`],
//! [`Tree::set_semantics`] and [`Tree::set_focusable`] refuse what would
//! break it with [`UiError::Unnamed`](crate::UiError::Unnamed).
//!
//! A secret never goes in the tree: a field holding one (a password, a
//! key) leaves its [`Semantics::value`] `None`, so neither the tree nor its
//! snapshot holds it (`AGENTS.md` rule 7). A value is still text the user
//! typed, so the `Debug` of [`Semantics`] and of [`AccessNode`] shows a
//! value's length only, as [`TextLength`] does (ADR-0043). [`Snapshot`]'s
//! `Display`, which is for tests and diagnostics of the UI's structure,
//! prints values as they are.

use std::fmt;

use tidepool_core::geom::Rect;
use tidepool_core::input::TextLength;

use crate::overlay::OverlayKind;
use crate::tree::{NodeId, Tree};

/// What a node is, for assistive technology.
///
/// Non-exhaustive: later versions may add roles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Role {
    /// Not exposed: a node only for layout. Its children attach to its
    /// nearest exposed ancestor.
    #[default]
    None,
    /// The window.
    Window,
    /// A pane of the shell: a side panel, the editor area.
    Pane,
    /// A group of related nodes.
    Group,
    /// A row of buttons or other controls.
    Toolbar,
    /// The status bar.
    StatusBar,
    /// A heading or section title.
    Heading,
    /// Static text.
    Label,
    /// A link.
    Link,
    /// Navigation, such as breadcrumbs.
    Navigation,
    /// A button.
    Button,
    /// A button that stays pressed or not.
    ToggleButton,
    /// A check box.
    CheckBox,
    /// A field of editable text.
    TextInput,
    /// A search field.
    SearchInput,
    /// A field with a list of choices.
    ComboBox,
    /// A list.
    List,
    /// An item of a list.
    ListItem,
    /// A tree, such as the file explorer.
    TreeView,
    /// An item of a tree.
    TreeItem,
    /// A table.
    Table,
    /// A row of a table.
    Row,
    /// A cell of a table.
    Cell,
    /// A column's header in a table.
    ColumnHeader,
    /// A strip of tabs.
    TabList,
    /// A tab.
    Tab,
    /// The panel a tab shows.
    TabPanel,
    /// A scroll bar.
    ScrollBar,
    /// A splitter between two panes.
    Splitter,
    /// A dialog.
    Dialog,
    /// An urgent message.
    Alert,
    /// A tooltip.
    Tooltip,
    /// A menu.
    Menu,
    /// An item of a menu.
    MenuItem,
    /// A progress bar.
    ProgressBar,
    /// A status message, such as a toast.
    Status,
    /// An image.
    Image,
    /// A document, such as a Markdown preview.
    Document,
    /// A terminal.
    Terminal,
}

impl Role {
    /// The role's name in lowercase words: `"button"`, `"tree item"`,
    /// `"none"`. `Display` writes it.
    pub const fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Window => "window",
            Self::Pane => "pane",
            Self::Group => "group",
            Self::Toolbar => "toolbar",
            Self::StatusBar => "status bar",
            Self::Heading => "heading",
            Self::Label => "label",
            Self::Link => "link",
            Self::Navigation => "navigation",
            Self::Button => "button",
            Self::ToggleButton => "toggle button",
            Self::CheckBox => "check box",
            Self::TextInput => "text input",
            Self::SearchInput => "search input",
            Self::ComboBox => "combo box",
            Self::List => "list",
            Self::ListItem => "list item",
            Self::TreeView => "tree view",
            Self::TreeItem => "tree item",
            Self::Table => "table",
            Self::Row => "row",
            Self::Cell => "cell",
            Self::ColumnHeader => "column header",
            Self::TabList => "tab list",
            Self::Tab => "tab",
            Self::TabPanel => "tab panel",
            Self::ScrollBar => "scroll bar",
            Self::Splitter => "splitter",
            Self::Dialog => "dialog",
            Self::Alert => "alert",
            Self::Tooltip => "tooltip",
            Self::Menu => "menu",
            Self::MenuItem => "menu item",
            Self::ProgressBar => "progress bar",
            Self::Status => "status",
            Self::Image => "image",
            Self::Document => "document",
            Self::Terminal => "terminal",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// What assistive technology is told about a node: its role, name, value
/// and state.
///
/// The default is [`Role::None`] with an empty name. Its `Debug` shows the
/// value's length only, never the value (see the
/// [module documentation](self)).
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Semantics {
    /// The role.
    pub role: Role,
    /// The accessible name: a button's label, a field's label.
    pub name: String,
    /// The current value: a field's text, a progress bar's percentage.
    ///
    /// A field holding a secret (a password, a key) leaves it `None`, so
    /// the tree and its snapshot never hold one (`AGENTS.md` rule 7):
    /// [`Snapshot`]'s `Display` prints values as they are.
    pub value: Option<String>,
    /// Whether the node is the selected item of a list, tree or tab strip.
    pub selected: bool,
    /// Whether a check box or toggle is checked, if it can be.
    pub checked: Option<bool>,
    /// Whether a tree item or combo box is expanded, if it can be.
    pub expanded: Option<bool>,
}

impl Semantics {
    /// Semantics with a role and a name, and no value or state.
    pub fn new(role: Role, name: impl Into<String>) -> Self {
        Self {
            role,
            name: name.into(),
            ..Self::default()
        }
    }

    /// The same, with a value.
    pub fn with_value(self, value: impl Into<String>) -> Self {
        Self {
            value: Some(value.into()),
            ..self
        }
    }

    /// The same, selected.
    pub fn selected(self) -> Self {
        Self {
            selected: true,
            ..self
        }
    }

    /// The same, checked or unchecked.
    pub fn checked(self, checked: bool) -> Self {
        Self {
            checked: Some(checked),
            ..self
        }
    }

    /// The same, expanded or collapsed.
    pub fn expanded(self, expanded: bool) -> Self {
        Self {
            expanded: Some(expanded),
            ..self
        }
    }

    /// Returns `true` if the role is not [`Role::None`] and the name has a
    /// character that is not whitespace: what a focusable node needs.
    pub fn is_named(&self) -> bool {
        self.role != Role::None && self.name.chars().any(|c| !c.is_whitespace())
    }
}

/// Shows the value as its length only.
impl fmt::Debug for Semantics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Semantics")
            .field("role", &self.role)
            .field("name", &self.name)
            .field("value", &self.value.as_deref().map(TextLength))
            .field("selected", &self.selected)
            .field("checked", &self.checked)
            .field("expanded", &self.expanded)
            .finish()
    }
}

/// One exposed node of a [`Snapshot`].
///
/// Its `Debug` shows the value's length only, never the value.
#[derive(Clone, PartialEq)]
pub struct AccessNode {
    /// The node.
    pub id: NodeId,
    /// How many exposed ancestors it has: 0 for the outermost.
    pub depth: usize,
    /// The role; never [`Role::None`].
    pub role: Role,
    /// The accessible name.
    pub name: String,
    /// The value, if any.
    pub value: Option<String>,
    /// Its bounds in the last layout, in logical units.
    pub bounds: Rect,
    /// Whether the node is focusable (its own flag; it may be disabled),
    /// `false` in a pass-through overlay, whose nodes never take focus.
    pub focusable: bool,
    /// Whether it has keyboard focus.
    pub focused: bool,
    /// Whether it or an ancestor is disabled.
    pub disabled: bool,
    /// Whether it is selected.
    pub selected: bool,
    /// Whether it is checked, if it can be.
    pub checked: Option<bool>,
    /// Whether it is expanded, if it can be.
    pub expanded: Option<bool>,
    /// Whether it is the root of the active modal overlay, so that the
    /// rest of the window is inert while it is open (`aria-modal`).
    pub modal: bool,
    /// What a tooltip on it says, if it has one ([`Tree::set_tooltip`]):
    /// its accessible description, which a screen reader reads after its
    /// name whether the tooltip shows or not (`aria-describedby`).
    pub description: Option<String>,
    /// The option its keys act on while it keeps focus itself, as a command
    /// palette's field acts on the list it filters: the selected item of
    /// that list, if it is exposed (`aria-activedescendant`).
    pub active: Option<NodeId>,
}

/// Shows the value as its length only.
impl fmt::Debug for AccessNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccessNode")
            .field("id", &self.id)
            .field("depth", &self.depth)
            .field("role", &self.role)
            .field("name", &self.name)
            .field("value", &self.value.as_deref().map(TextLength))
            .field("bounds", &self.bounds)
            .field("focusable", &self.focusable)
            .field("focused", &self.focused)
            .field("disabled", &self.disabled)
            .field("selected", &self.selected)
            .field("checked", &self.checked)
            .field("expanded", &self.expanded)
            .field("modal", &self.modal)
            .field("description", &self.description.as_deref().map(TextLength))
            .field("active", &self.active)
            .finish()
    }
}

/// The exposed nodes of a tree at one moment, in pre-order. See
/// [`Tree::accessibility`].
///
/// Its `Display` writes one line per node: two spaces per level of
/// [`depth`](AccessNode::depth), the role, the name in quotes, ` = ` and
/// the value in quotes if there is one, ` description ` and the
/// description in quotes if there is one, ` active ` and the active
/// option's name in quotes if there is one, then those of `modal`,
/// `focusable`, `focused`, `disabled`, `selected`, `checked` or
/// `unchecked`, and `expanded` or `collapsed` that hold. Names, values and
/// descriptions are escaped as [`str::escape_debug`] does, so each node is
/// one line:
///
/// ```text
/// window "Tidepool"
///   button "Save" focusable focused
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    nodes: Vec<AccessNode>,
}

impl Snapshot {
    /// The exposed nodes, in pre-order.
    pub fn nodes(&self) -> &[AccessNode] {
        &self.nodes
    }

    /// The exposed node with this id, if there is one. Takes time linear
    /// in the number of nodes.
    pub fn get(&self, id: NodeId) -> Option<&AccessNode> {
        self.nodes.iter().find(|node| node.id == id)
    }
}

impl fmt::Display for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for node in &self.nodes {
            for _ in 0..node.depth {
                f.write_str("  ")?;
            }
            write!(f, "{} \"{}\"", node.role, node.name.escape_debug())?;
            if let Some(value) = &node.value {
                write!(f, " = \"{}\"", value.escape_debug())?;
            }
            if let Some(description) = &node.description {
                write!(f, " description \"{}\"", description.escape_debug())?;
            }
            if let Some(active) = node.active.and_then(|active| self.get(active)) {
                write!(f, " active \"{}\"", active.name.escape_debug())?;
            }
            let flags = [
                (node.modal, "modal"),
                (node.focusable, "focusable"),
                (node.focused, "focused"),
                (node.disabled, "disabled"),
                (node.selected, "selected"),
                (node.checked == Some(true), "checked"),
                (node.checked == Some(false), "unchecked"),
                (node.expanded == Some(true), "expanded"),
                (node.expanded == Some(false), "collapsed"),
            ];
            for (holds, flag) in flags {
                if holds {
                    write!(f, " {flag}")?;
                }
            }
            f.write_str("\n")?;
        }
        Ok(())
    }
}

impl Tree {
    /// A snapshot of the accessibility tree: every visible node whose role
    /// is not [`Role::None`] in the live layers, in pre-order, layer by
    /// layer (see the [module documentation](self)), with its depth among
    /// exposed nodes and its bounds from the last layout.
    pub fn accessibility(&self) -> Snapshot {
        let mut nodes = Vec::new();
        // (node, the list whose selected item is its active option).
        let mut actives: Vec<(usize, NodeId)> = Vec::new();
        let modal = self.modal();
        for (root, kind) in self.live_layers() {
            let passes = kind == Some(OverlayKind::PassThrough);
            // (node, exposed ancestors), in pre-order.
            let mut stack = vec![(root, 0)];
            while let Some((id, depth)) = stack.pop() {
                let Some(node) = self.get(id) else { continue };
                if node.sane.hidden {
                    continue;
                }
                let semantics = &node.semantics;
                let exposed = semantics.role != Role::None;
                if exposed {
                    if let Some(list) = node.active_in {
                        actives.push((nodes.len(), list));
                    }
                    nodes.push(AccessNode {
                        id,
                        depth,
                        role: semantics.role,
                        name: semantics.name.clone(),
                        value: semantics.value.clone(),
                        bounds: node.placement.rect,
                        focusable: node.focusable && !passes,
                        focused: self.focused() == Some(id),
                        disabled: !self.is_enabled(id),
                        selected: semantics.selected,
                        checked: semantics.checked,
                        expanded: semantics.expanded,
                        modal: modal == Some(id),
                        description: node.tooltip.clone(),
                        active: None,
                    });
                }
                let below = if exposed { depth + 1 } else { depth };
                stack.extend(node.children.iter().rev().map(|&child| (child, below)));
            }
        }
        for (at, list) in actives {
            let active = nodes
                .iter()
                .find(|option| {
                    option.selected && option.id != list && self.is_within(option.id, list)
                })
                .map(|option| option.id);
            if let Some(node) = nodes.get_mut(at) {
                node.active = active;
            }
        }
        Snapshot { nodes }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_lowercase_words() {
        assert_eq!(Role::None.name(), "none");
        assert_eq!(Role::TreeItem.to_string(), "tree item");
        assert_eq!(Role::Button.to_string(), "button");
        assert_eq!(Role::default(), Role::None);
    }

    #[test]
    fn named_needs_a_role_and_a_visible_character() {
        assert!(Semantics::new(Role::Button, "Save").is_named());
        assert!(!Semantics::new(Role::Button, "").is_named());
        assert!(!Semantics::new(Role::Button, " \t\u{3000}\n").is_named());
        assert!(!Semantics::new(Role::None, "Save").is_named());
        assert!(!Semantics::default().is_named());
    }

    #[test]
    fn builders_set_value_and_state() {
        let semantics = Semantics::new(Role::TreeItem, "src")
            .with_value("3 files")
            .selected()
            .checked(false)
            .expanded(true);
        assert_eq!(semantics.value.as_deref(), Some("3 files"));
        assert!(semantics.selected);
        assert_eq!(semantics.checked, Some(false));
        assert_eq!(semantics.expanded, Some(true));
    }

    #[test]
    fn debug_shows_a_values_length_and_never_the_value() {
        // A name that the role's `Debug` does not hold, so that only the
        // name can show it.
        let field = Semantics::new(Role::SearchInput, "Find in files").with_value("hunter2");
        let shown = format!("{field:?}");
        assert!(shown.contains("Some(<7 bytes>)"), "{shown}");
        assert!(!shown.contains("hunter2"), "{shown}");
        assert!(shown.contains("name: \"Find in files\""), "{shown}");

        let node = AccessNode {
            id: Tree::new().root(),
            depth: 0,
            role: Role::SearchInput,
            name: "Find in files".to_string(),
            value: Some("hunter2".to_string()),
            bounds: Rect::ZERO,
            focusable: true,
            focused: false,
            disabled: false,
            selected: false,
            checked: None,
            expanded: None,
            modal: false,
            description: None,
            active: None,
        };
        let shown = format!("{node:?}");
        assert!(shown.contains("Some(<7 bytes>)"), "{shown}");
        assert!(!shown.contains("hunter2"), "{shown}");
        assert!(shown.contains("name: \"Find in files\""), "{shown}");
    }
}
