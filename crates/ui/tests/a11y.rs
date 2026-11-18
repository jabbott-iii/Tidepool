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

//! M3.1's accessibility tree, through the public API only:
//!
//! 1. Every focusable node has a role and a name: after each operation of
//!    random sequences of inserts, removes and changes of semantics,
//!    focusability, visibility and enablement, every exposed focusable
//!    node is named, and every focusable node is in the snapshot unless it
//!    is hidden; an operation refused with `Unnamed` changes nothing.
//! 2. The snapshot's text, with nodes of `Role::None` transparent and
//!    hidden ones left out.

use tidepool_core::geom::Size;
use tidepool_core::rng::Pcg32;
use tidepool_core::testing::check_sized;
use tidepool_ui::a11y::{Role, Semantics};
use tidepool_ui::layout::Style;
use tidepool_ui::metrics::FixedMetrics;
use tidepool_ui::{Node, NodeId, Tree, UiError};

const ROLES: [Role; 6] = [
    Role::None,
    Role::Button,
    Role::Group,
    Role::TreeItem,
    Role::TextInput,
    Role::Window,
];

const NAMES: [&str; 6] = ["", " ", "\t\n", "Save", "src", "Run \"cargo test\""];

/// One change to the tree. Nodes are named by an index into every id the
/// sequence has made, removed ones too, the root's first.
#[derive(Clone, Debug)]
enum Op {
    Insert {
        parent: usize,
        node: Spec,
    },
    InsertAt {
        parent: usize,
        index: usize,
        node: Spec,
    },
    Remove(usize),
    SetSemantics(usize, Semantics),
    SetFocusable(usize, bool),
    SetHidden(usize, bool),
    SetEnabled(usize, bool),
}

#[derive(Clone, Debug)]
struct Spec {
    semantics: Semantics,
    focusable: bool,
    enabled: bool,
    hidden: bool,
}

fn semantics(rng: &mut Pcg32) -> Semantics {
    let role = *rng.choose(&ROLES).expect("not empty");
    let name = *rng.choose(&NAMES).expect("not empty");
    let mut semantics = Semantics::new(role, name);
    if rng.range(0..4u32) == 0 {
        semantics = semantics.with_value("v");
    }
    semantics
}

fn spec(rng: &mut Pcg32) -> Spec {
    Spec {
        semantics: semantics(rng),
        focusable: rng.range(0..2u32) == 0,
        enabled: rng.range(0..5u32) != 0,
        hidden: rng.range(0..6u32) == 0,
    }
}

fn op(rng: &mut Pcg32) -> Op {
    let target = rng.range(0..64u32) as usize;
    match rng.range(0..10u32) {
        0..=2 => Op::Insert {
            parent: target,
            node: spec(rng),
        },
        3 => Op::InsertAt {
            parent: target,
            index: rng.range(0..4u32) as usize,
            node: spec(rng),
        },
        4 => Op::Remove(target),
        5 => Op::SetSemantics(target, semantics(rng)),
        6 => Op::SetFocusable(target, rng.range(0..2u32) == 0),
        7 => Op::SetHidden(target, rng.range(0..2u32) == 0),
        _ => Op::SetEnabled(target, rng.range(0..2u32) == 0),
    }
}

fn node(spec: &Spec) -> Node {
    let style = if spec.hidden {
        Style::default().hidden()
    } else {
        Style::default()
    };
    Node {
        style,
        semantics: spec.semantics.clone(),
        focusable: spec.focusable,
        enabled: spec.enabled,
        widget: None,
    }
}

/// Everything about the tree the operations can change, in pre-order.
fn dump(tree: &Tree) -> String {
    let mut out = format!("{} nodes\n", tree.node_count());
    let mut stack = vec![tree.root()];
    while let Some(id) = stack.pop() {
        out.push_str(&format!(
            "{id} parent {:?} focusable {:?} enabled {:?} hidden {:?} {:?}\n",
            tree.parent(id),
            tree.focusable(id),
            tree.enabled(id),
            tree.style(id).map(|style| style.hidden),
            tree.semantics(id).map(|s| (s.role, &s.name, &s.value)),
        ));
        if let Some(children) = tree.children(id) {
            stack.extend(children.iter().rev());
        }
    }
    out.push_str(&tree.accessibility().to_string());
    out
}

/// Applies the operation; the id it made, if any, is added to `ids`.
fn apply(tree: &mut Tree, ids: &mut Vec<NodeId>, op: &Op) -> Result<(), UiError> {
    let pick = |ids: &[NodeId], at: usize| ids[at % ids.len()];
    match op {
        Op::Insert { parent, node: spec } => {
            let id = tree.insert(pick(ids, *parent), node(spec))?;
            ids.push(id);
        }
        Op::InsertAt {
            parent,
            index,
            node: spec,
        } => {
            let id = tree.insert_at(pick(ids, *parent), *index, node(spec))?;
            ids.push(id);
        }
        Op::Remove(at) => tree.remove(pick(ids, *at))?,
        Op::SetSemantics(at, semantics) => tree.set_semantics(pick(ids, *at), semantics.clone())?,
        Op::SetFocusable(at, focusable) => tree.set_focusable(pick(ids, *at), *focusable)?,
        Op::SetHidden(at, hidden) => {
            let id = pick(ids, *at);
            if let Some(&style) = tree.style(id) {
                tree.set_style(
                    id,
                    Style {
                        hidden: *hidden,
                        ..style
                    },
                )?;
            }
        }
        Op::SetEnabled(at, enabled) => tree.set_enabled(pick(ids, *at), *enabled)?,
    }
    Ok(())
}

/// Whether the node or an ancestor is hidden.
fn hidden(tree: &Tree, id: NodeId) -> bool {
    let mut next = Some(id);
    while let Some(id) = next {
        if tree.style(id).is_some_and(|style| style.hidden) {
            return true;
        }
        next = tree.parent(id);
    }
    false
}

fn named(role: Role, name: &str) -> bool {
    role != Role::None && name.chars().any(|c| !c.is_whitespace())
}

fn every_focusable_node_is_named(ops: &[Op]) -> Result<(), String> {
    let mut tree = Tree::new();
    let mut ids = vec![tree.root()];
    let metrics = FixedMetrics::DEFAULT;
    for (step, op) in ops.iter().enumerate() {
        let before = dump(&tree);
        let result = apply(&mut tree, &mut ids, op);
        if let Err(UiError::Unnamed { .. }) = result
            && dump(&tree) != before
        {
            return Err(format!("step {step}: a refused {op:?} changed the tree"));
        }
        if step % 8 == 0 {
            tree.layout(Size::new(200.0, 200.0), &metrics);
        }
        let snapshot = tree.accessibility();
        for node in snapshot.nodes() {
            if node.focusable && !named(node.role, &node.name) {
                return Err(format!("step {step}: {node:?} is focusable without a name"));
            }
            if hidden(&tree, node.id) {
                return Err(format!(
                    "step {step}: hidden {} is in the snapshot",
                    node.id
                ));
            }
        }
        let mut stack = vec![tree.root()];
        while let Some(id) = stack.pop() {
            if tree.focusable(id) == Some(true) {
                let semantics = tree.semantics(id).ok_or("a node is gone")?;
                if !semantics.is_named() {
                    return Err(format!("step {step}: {id} is focusable without a name"));
                }
                if !hidden(&tree, id) && snapshot.get(id).is_none() {
                    return Err(format!(
                        "step {step}: focusable {id} is not in the snapshot"
                    ));
                }
            }
            if let Some(children) = tree.children(id) {
                stack.extend(children.iter().rev());
            }
        }
        if let Some(focus) = tree.focused()
            && !tree.tab_order().contains(&focus)
        {
            return Err(format!("step {step}: {focus} has focus but cannot take it"));
        }
        if step % 5 == 0 {
            tree.focus_next();
        }
    }
    Ok(())
}

#[test]
fn every_focusable_node_has_a_role_and_a_name() {
    check_sized(
        "every focusable node has a role and a name",
        400,
        |rng, size| (0..size).map(|_| op(rng)).collect::<Vec<Op>>(),
        |ops| every_focusable_node_is_named(ops),
    );
}

#[test]
fn refused_operations_change_nothing() {
    let mut tree = Tree::new();
    let root = tree.root();
    let button = tree
        .insert(
            root,
            Node::default()
                .with_semantics(Semantics::new(Role::Button, "Save"))
                .focusable(),
        )
        .expect("named");
    let label = tree
        .insert(
            root,
            Node::default().with_semantics(Semantics::new(Role::Label, "")),
        )
        .expect("not focusable");
    let before = dump(&tree);
    assert_eq!(
        tree.set_semantics(button, Semantics::new(Role::Button, " ")),
        Err(UiError::Unnamed { role: Role::Button })
    );
    assert_eq!(
        tree.set_semantics(button, Semantics::new(Role::None, "Save")),
        Err(UiError::Unnamed { role: Role::None })
    );
    assert_eq!(
        tree.set_focusable(label, true),
        Err(UiError::Unnamed { role: Role::Label })
    );
    assert_eq!(
        tree.insert(root, Node::default().focusable()),
        Err(UiError::Unnamed { role: Role::None })
    );
    assert_eq!(dump(&tree), before);
}

#[test]
fn the_snapshot_reads_as_text() {
    let mut tree = Tree::new();
    let root = tree.root();
    tree.set_semantics(root, Semantics::new(Role::Window, "Tidepool"))
        .expect("the root");
    let explorer = tree
        .insert(
            root,
            Node::default().with_semantics(Semantics::new(Role::Pane, "Explorer")),
        )
        .expect("insert");
    // A node of no role, only for layout: its children attach to the pane.
    let layout_only = tree.insert(explorer, Node::default()).expect("insert");
    let src = tree
        .insert(
            layout_only,
            Node::default()
                .with_semantics(
                    Semantics::new(Role::TreeItem, "src")
                        .expanded(true)
                        .selected(),
                )
                .focusable(),
        )
        .expect("insert");
    tree.insert(
        src,
        Node::default().with_semantics(Semantics::new(Role::TreeItem, "lib").expanded(false)),
    )
    .expect("insert");
    tree.insert(
        root,
        Node::default()
            .with_semantics(Semantics::new(Role::CheckBox, "Format on save").checked(true))
            .focusable()
            .disabled(),
    )
    .expect("insert");
    tree.insert(
        root,
        Node::default()
            .with_semantics(
                Semantics::new(Role::TextInput, "Run \"cargo\"\nnow").with_value("--release"),
            )
            .focusable(),
    )
    .expect("insert");
    tree.insert(
        root,
        Node::new(Style::default().hidden()).with_semantics(Semantics::new(Role::Button, "Hidden")),
    )
    .expect("insert");
    tree.set_focus(src, true).expect("focusable");

    let snapshot = tree.accessibility();
    assert_eq!(
        snapshot.to_string(),
        "window \"Tidepool\"\n\
         \x20 pane \"Explorer\"\n\
         \x20   tree item \"src\" focusable focused selected expanded\n\
         \x20     tree item \"lib\" collapsed\n\
         \x20 check box \"Format on save\" focusable disabled checked\n\
         \x20 text input \"Run \\\"cargo\\\"\\nnow\" = \"--release\" focusable\n"
    );
    assert_eq!(snapshot.nodes().len(), 6);
    assert!(snapshot.get(layout_only).is_none());
    assert_eq!(snapshot.get(src).map(|node| node.depth), Some(2));
    assert_eq!(snapshot.get(explorer).map(|node| node.depth), Some(1));
}
