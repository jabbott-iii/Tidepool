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

//! The error type for `tidepool-ui`.
//!
//! New in Tidepool; not ported.
//!
//! Changing the tree can fail: a stale or unknown id, the root where it
//! cannot go, a focusable node without a role and a name, an index past
//! the children, a node too deep or one too many. So can reading a theme
//! file that is not TOML or not a theme (M3.2, ADR-0047). Layout, input
//! and frames never fail and never panic.

use std::fmt;

use tidepool_codec::CodecError;

use crate::a11y::Role;
use crate::theme::ThemeError;
use crate::tree::NodeId;

/// Errors produced by `tidepool-ui`.
///
/// Non-exhaustive: later versions may add variants.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum UiError {
    /// The id names no node in the tree: it was removed, or never named
    /// one. An id is meaningful only in the tree that made it: one from
    /// another tree is not detected, and may name a node of this one (see
    /// [the tree's ids](crate::tree#ids)).
    UnknownNode {
        /// The id given.
        id: NodeId,
    },
    /// The root node cannot be removed.
    RootNode,
    /// A focusable node needs a role and a name (see
    /// [`Semantics::is_named`](crate::a11y::Semantics::is_named)), and
    /// this change would leave one without.
    Unnamed {
        /// The role it would have.
        role: Role,
    },
    /// An index past the end of a node's children.
    BadIndex {
        /// The index given.
        index: usize,
        /// How many children the node has.
        len: usize,
    },
    /// The node cannot take focus: it is not focusable, or it or an
    /// ancestor is disabled or hidden.
    NotFocusable {
        /// The id given.
        id: NodeId,
    },
    /// The node holds no widget of the type asked for
    /// ([`widgets::update`](crate::widgets::update)).
    WrongWidget {
        /// The id given.
        id: NodeId,
    },
    /// The node would be more than [`MAX_DEPTH`](crate::tree::MAX_DEPTH)
    /// levels below the root.
    TooDeep {
        /// The most levels there may be.
        max: usize,
    },
    /// The tree already holds [`MAX_NODES`](crate::tree::MAX_NODES) nodes.
    TooManyNodes {
        /// The most nodes there may be, the root included.
        max: usize,
    },
    /// A theme file is not TOML, or breaks the
    /// [`Limits`](tidepool_codec::Limits) it was read with
    /// ([`Theme::parse`](crate::theme::Theme::parse)).
    ThemeSyntax(CodecError),
    /// A theme file is TOML but not a theme of the format
    /// [`theme`](crate::theme) reads.
    Theme(ThemeError),
}

impl fmt::Display for UiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::ThemeSyntax(ref error) => write!(f, "theme: {error}"),
            Self::Theme(ref error) => error.fmt(f),
            Self::UnknownNode { id } => write!(f, "node {id} is not in the tree"),
            Self::RootNode => f.write_str("the root node cannot be removed"),
            Self::Unnamed { role } => {
                f.write_str("a focusable node needs a role and a name; ")?;
                if role == Role::None {
                    f.write_str("this one has no role")
                } else {
                    let name = role.name();
                    let article = if name.starts_with(['a', 'e', 'i', 'o', 'u']) {
                        "an"
                    } else {
                        "a"
                    };
                    write!(f, "this one is {article} {name} with no name")
                }
            }
            Self::BadIndex { index, len } => {
                let children = if len == 1 { "child" } else { "children" };
                write!(f, "index {index} is past the {len} {children}")
            }
            Self::NotFocusable { id } => write!(
                f,
                "node {id} cannot take focus: it is not focusable, is disabled or is hidden"
            ),
            Self::WrongWidget { id } => {
                write!(f, "node {id} holds no widget of the type asked for")
            }
            Self::TooDeep { max } => {
                write!(f, "a node may be at most {max} levels below the root")
            }
            Self::TooManyNodes { max } => write!(f, "the tree holds at most {max} nodes"),
        }
    }
}

impl std::error::Error for UiError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ThemeSyntax(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{MAX_DEPTH, MAX_NODES, Tree};

    #[test]
    fn messages_name_the_values() {
        let root = Tree::new().root();
        let cases = [
            (
                UiError::UnknownNode { id: root },
                "node 0v0 is not in the tree",
            ),
            (UiError::RootNode, "the root node cannot be removed"),
            (
                UiError::Unnamed { role: Role::Button },
                "a focusable node needs a role and a name; this one is a button with no name",
            ),
            (
                UiError::Unnamed { role: Role::Image },
                "a focusable node needs a role and a name; this one is an image with no name",
            ),
            (
                UiError::Unnamed {
                    role: Role::TreeItem,
                },
                "a focusable node needs a role and a name; this one is a tree item with no name",
            ),
            (
                UiError::Unnamed { role: Role::None },
                "a focusable node needs a role and a name; this one has no role",
            ),
            (
                UiError::BadIndex { index: 5, len: 3 },
                "index 5 is past the 3 children",
            ),
            (
                UiError::BadIndex { index: 2, len: 1 },
                "index 2 is past the 1 child",
            ),
            (
                UiError::NotFocusable { id: root },
                "node 0v0 cannot take focus: it is not focusable, is disabled or is hidden",
            ),
            (
                UiError::WrongWidget { id: root },
                "node 0v0 holds no widget of the type asked for",
            ),
            (
                UiError::TooDeep { max: MAX_DEPTH },
                "a node may be at most 256 levels below the root",
            ),
            (
                UiError::TooManyNodes { max: MAX_NODES },
                "the tree holds at most 1048576 nodes",
            ),
        ];
        for (error, message) in cases {
            assert_eq!(error.to_string(), message);
        }
    }
}
