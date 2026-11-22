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

//! The golden scenes of `ui`'s overlays (M3.3, ADR-0052): the modal
//! dialog, the tooltip, the toast and the command palette, each at scale 1
//! and 2. The modal dialog's is a grid of a row for each dialog and a column
//! for each state: rest, with the pointer over a button of the window
//! under the dialog, which is inert and so shows no hover (the same pixels
//! as no pointer at all, since the dialog has no hover look of its own);
//! hover, the pointer over its first button; focus, on the dialog itself,
//! where it lands when the dialog opens (ADR-0053), shown by a key, so
//! that the dialog draws the focus outline; pressed, its first button
//! pressed; and disabled, with the pointer over its first button and
//! pressing.
//!
//! The tooltip's, the toast's and the palette's are grids of a row for
//! each variant and a column for each state they have; each says which
//! below.
//!
//! Each cell is a tree of its own: a window with a section title and
//! buttons, over which the overlay opens, put in its state as a user or
//! its owner would put it: a pointer move, a key, a press without its
//! release, or `Tree::set_enabled`. Each cell is painted by
//! [`Painter`] over the theme's `bg` and copied into the scene, which is
//! written as the other scenes are.

use tidepool_core::geom::{DevicePoint, DeviceSize, Point, Rect, Scale, Size};
use tidepool_core::input::{InputEvent, Key, Modifiers, MouseButton};
use tidepool_gfx::framebuffer::Framebuffer;
use tidepool_gui::Painter;
use tidepool_ui::layout::{Insets, Style};
use tidepool_ui::theme::Token;
use tidepool_ui::widgets::{
    Button, ButtonKind, Control, Modal, Palette, Row, SectionTitle, Toast, ToastKind, Tooltip,
};
use tidepool_ui::{Node, NodeId, Tree};

use super::{painter, save, token};

/// The size of a cell, in logical units: room for the taller dialog and
/// its margins.
const CELL: Size = Size::new(360.0, 180.0);

/// A cell being put in its state: its tree, the dialog, its first button,
/// a button of the window under it, and what lays it out.
struct Cell {
    tree: Tree,
    dialog: NodeId,
    first: NodeId,
    behind: NodeId,
    painter: Painter,
    scale: Scale,
}

impl Cell {
    fn layout(&mut self) {
        self.tree.layout(CELL, &self.painter.metrics(self.scale));
    }

    fn bounds(&self, node: NodeId) -> Rect {
        self.tree.bounds(node).expect("laid out")
    }

    fn center_of(&self, node: NodeId) -> Point {
        let bounds = self.bounds(node);
        Point::new(
            bounds.x + bounds.width / 2.0,
            bounds.y + bounds.height / 2.0,
        )
    }

    /// The pointer comes to `at` from 20 units to its left, as a user's
    /// does: past the slop a press waits for after a modal opens
    /// (ADR-0056).
    fn hover(&mut self, at: Point) {
        for position in [Point::new(at.x - 20.0, at.y), at] {
            self.tree.handle(&InputEvent::MouseMove {
                position,
                modifiers: Modifiers::NONE,
            });
        }
    }

    fn press(&mut self, at: Point) {
        self.tree.handle(&InputEvent::MouseDown {
            button: MouseButton::Left,
            position: at,
            modifiers: Modifiers::NONE,
        });
    }
}

/// A dialog's text and its two buttons: the first's label and kind, then
/// the second's label.
struct Dialog {
    title: &'static str,
    text: &'static str,
    first: (&'static str, ButtonKind),
    second: &'static str,
}

const DISCARD: Dialog = Dialog {
    title: "Discard changes?",
    text: "main.rs has changes that are not saved.",
    first: ("Discard", ButtonKind::Amber),
    second: "Cancel",
};

const INSTALL: Dialog = Dialog {
    title: "Install rust-analyzer",
    text: "rust-analyzer 2026-11-17, from GitHub.\nIts license shows before it installs.",
    first: ("Install", ButtonKind::Teal),
    second: "Not now",
};

/// Puts a cell in a column's state.
type Act = fn(&mut Cell);

/// A cell of [`CELL`] showing `dialog` over a window, put in its state by
/// `act`, painted at `factor`.
fn cell(dialog: &Dialog, factor: f32, act: Act) -> Framebuffer {
    let mut tree = Tree::new();
    let root = tree.root();
    tree.set_style(root, Style::column().padding(Insets::all(8.0)).gap(8.0))
        .expect("a style");
    tree.insert(root, SectionTitle::new("Workspace").node())
        .expect("a title");
    let row = tree
        .insert(root, Node::new(Style::row().gap(8.0)))
        .expect("a row");
    let behind = tree
        .insert(row, Button::new("Open folder", ButtonKind::Ghost).node())
        .expect("a button");
    tree.insert(row, Button::new("Run", ButtonKind::Slate).node())
        .expect("a button");
    let id = Modal::new(dialog.title)
        .text(dialog.text)
        .open(&mut tree)
        .expect("a dialog");
    let actions = Modal::actions(&tree, id).expect("its row of buttons");
    let (label, kind) = dialog.first;
    let first = tree
        .insert(actions, Button::new(label, kind).node())
        .expect("a button");
    tree.insert(
        actions,
        Button::new(dialog.second, ButtonKind::Ghost).node(),
    )
    .expect("a button");
    let scale = Scale::new(factor).expect("a scale");
    let mut cell = Cell {
        tree,
        dialog: id,
        first,
        behind,
        painter: painter(),
        scale,
    };
    cell.layout();
    act(&mut cell);
    let Cell {
        mut tree,
        mut painter,
        ..
    } = cell;
    let frame = tree
        .frame(CELL, &painter.metrics(scale))
        .expect("the first frame damages the window");
    let device = DeviceSize::new((CELL.width * factor) as i32, (CELL.height * factor) as i32);
    let mut base = Framebuffer::new(device).expect("a base");
    base.clear(token(Token::Bg).premultiply());
    let mut target = Framebuffer::new(device).expect("a cell");
    painter
        .paint(&mut target, &base.view(), &frame, scale)
        .expect("painted");
    assert_ne!(target, base, "the cell drew something");
    assert!(
        tree.frame(CELL, &painter.metrics(scale)).is_none(),
        "the cell is idle after it"
    );
    target
}

/// Paints a grid of a row for each of `dialogs` and a column for each of
/// `columns`, at `factor`, and writes it as `name`.
fn grid(name: &str, factor: f32, dialogs: &[Dialog], columns: &[Act]) {
    let (cell_width, cell_height) = ((CELL.width * factor) as i32, (CELL.height * factor) as i32);
    let device = DeviceSize::new(
        cell_width * columns.len() as i32,
        cell_height * dialogs.len() as i32,
    );
    let mut scene = Framebuffer::new(device).expect("a scene");
    let bounds = scene.bounds();
    for (row, dialog) in dialogs.iter().enumerate() {
        for (column, &act) in columns.iter().enumerate() {
            let painted = cell(dialog, factor, act);
            let at = DevicePoint::new(cell_width * column as i32, cell_height * row as i32);
            tidepool_gfx::image::copy(&mut scene, bounds, &painted.view(), at);
        }
    }
    save(name, &scene);
}

/// The pointer over the dialog's first button.
fn hover(cell: &mut Cell) {
    let at = cell.center_of(cell.first);
    cell.hover(at);
}

/// The rest state, with the pointer over a button of the window under the
/// dialog, clear of it: the button is inert, so it shows no hover.
fn rest(cell: &mut Cell) {
    let behind = cell.bounds(cell.behind);
    let at = Point::new(behind.x + 4.0, behind.y + behind.height / 2.0);
    assert!(
        !cell.bounds(cell.dialog).contains(at),
        "the point is clear of the dialog"
    );
    cell.hover(at);
    assert!(!cell.tree.state(cell.behind).expect("a node").hovered);
}

/// A key no widget takes, F5: focus, on the dialog since it opened, shows.
fn focus(cell: &mut Cell) {
    let response = cell.tree.handle(&InputEvent::KeyDown {
        key: Key::F5,
        modifiers: Modifiers::NONE,
        repeat: false,
    });
    assert!(response.blocked, "the modal keeps it from the window");
    assert_eq!(cell.tree.focused(), Some(cell.dialog));
    assert!(cell.tree.focus_visible());
    cell.layout();
}

/// A press on the dialog's first button, not yet released.
fn pressed(cell: &mut Cell) {
    let at = cell.center_of(cell.first);
    cell.hover(at);
    cell.press(at);
}

/// The dialog disabled, with the pointer over its first button and
/// pressing: it shows neither (ADR-0049).
fn disabled(cell: &mut Cell) {
    cell.tree.set_enabled(cell.dialog, false).expect("disabled");
    cell.layout();
    let at = cell.center_of(cell.first);
    cell.hover(at);
    cell.press(at);
}

const STATES: [Act; 5] = [rest, hover, focus, pressed, disabled];

#[test]
fn ui_modal() {
    grid("ui_modal", 1.0, &[DISCARD, INSTALL], &STATES);
}

#[test]
fn ui_modal_scale_2() {
    grid("ui_modal_scale_2", 2.0, &[DISCARD, INSTALL], &STATES);
}

// ---------- The tooltip, the toast and the command palette ----------

/// A cell of one of the other overlays' scenes being put in its state: its
/// tree, the nodes its row built (by role: the anchor or toast, then the
/// rest), its size, and what lays it out.
struct Shot {
    tree: Tree,
    nodes: Vec<NodeId>,
    size: Size,
    painter: Painter,
    scale: Scale,
}

impl Shot {
    fn layout(&mut self) {
        self.tree
            .layout(self.size, &self.painter.metrics(self.scale));
    }

    fn center_of(&self, node: NodeId) -> Point {
        let bounds = self.tree.bounds(node).expect("laid out");
        Point::new(
            bounds.x + bounds.width / 2.0,
            bounds.y + bounds.height / 2.0,
        )
    }

    fn event(&mut self, event: InputEvent) {
        self.tree.handle(&event);
        self.layout();
    }

    /// The pointer comes to `at` from 20 units to its left, as a user's
    /// does: past the slop a press waits for after a modal opens
    /// (ADR-0056).
    fn hover(&mut self, at: Point) {
        for position in [Point::new(at.x - 20.0, at.y), at] {
            self.event(InputEvent::MouseMove {
                position,
                modifiers: Modifiers::NONE,
            });
        }
    }

    fn press(&mut self, at: Point) {
        self.event(InputEvent::MouseDown {
            button: MouseButton::Left,
            position: at,
            modifiers: Modifiers::NONE,
        });
    }

    fn key(&mut self, key: Key) {
        self.event(InputEvent::KeyDown {
            key,
            modifiers: Modifiers::NONE,
            repeat: false,
        });
    }
}

/// Builds a row's overlay in a cell's tree, and returns the nodes its
/// columns act on.
type Make = fn(&mut Tree) -> Vec<NodeId>;

/// Puts a cell of the other overlays' scenes in a column's state.
type Put = fn(&mut Shot);

/// A cell of `size` showing what `make` builds over a window, put in its
/// state by `put`, painted at `factor`.
fn shot(size: Size, factor: f32, make: Make, put: Put) -> Framebuffer {
    let mut tree = Tree::new();
    let root = tree.root();
    tree.set_style(root, Style::column().padding(Insets::all(8.0)).gap(8.0))
        .expect("a style");
    let nodes = make(&mut tree);
    let scale = Scale::new(factor).expect("a scale");
    let mut shot = Shot {
        tree,
        nodes,
        size,
        painter: painter(),
        scale,
    };
    shot.layout();
    put(&mut shot);
    let Shot {
        mut tree,
        mut painter,
        ..
    } = shot;
    let frame = tree
        .frame(size, &painter.metrics(scale))
        .expect("the first frame damages the window");
    let device = DeviceSize::new((size.width * factor) as i32, (size.height * factor) as i32);
    let mut base = Framebuffer::new(device).expect("a base");
    base.clear(token(Token::Bg).premultiply());
    let mut target = Framebuffer::new(device).expect("a cell");
    painter
        .paint(&mut target, &base.view(), &frame, scale)
        .expect("painted");
    assert_ne!(target, base, "the cell drew something");
    assert!(
        tree.frame(size, &painter.metrics(scale)).is_none(),
        "the cell is idle after it"
    );
    target
}

/// Paints a grid of a row for each of `makes` and a column for each of
/// `puts`, cells of `size`, at `factor`, and writes it as `name`.
fn shots(name: &str, size: Size, factor: f32, makes: &[Make], puts: &[Put]) {
    let (cell_width, cell_height) = ((size.width * factor) as i32, (size.height * factor) as i32);
    let device = DeviceSize::new(
        cell_width * puts.len() as i32,
        cell_height * makes.len() as i32,
    );
    assert!(device.width <= 4096, "within xtask's scene width");
    let mut scene = Framebuffer::new(device).expect("a scene");
    let bounds = scene.bounds();
    for (row, &make) in makes.iter().enumerate() {
        for (column, &put) in puts.iter().enumerate() {
            let painted = shot(size, factor, make, put);
            let at = DevicePoint::new(cell_width * column as i32, cell_height * row as i32);
            tidepool_gfx::image::copy(&mut scene, bounds, &painted.view(), at);
        }
    }
    save(name, &scene);
}

// The tooltip: a row for each tooltip, a column for each state of its
// node: the pointer on it; keyboard focus on it, showing; the pointer
// resting on the tooltip itself; and the node disabled, with the pointer
// on it. Each opens the tooltip as its owner would once its timer fires.

const TOOLTIP_CELL: Size = Size::new(280.0, 120.0);

/// Two buttons in a row, the first with `text` as its tooltip.
fn tooltip_bar(tree: &mut Tree, text: &str, at_bottom: bool) -> Vec<NodeId> {
    let root = tree.root();
    if at_bottom {
        tree.set_style(
            root,
            Style::column().padding(Insets::all(8.0)).align(
                tidepool_ui::layout::Align::Start,
                tidepool_ui::layout::Align::End,
            ),
        )
        .expect("a style");
    }
    let row = tree
        .insert(root, Node::new(Style::row().gap(8.0)))
        .expect("a row");
    let save = tree
        .insert(row, Button::new("Save", ButtonKind::Ghost).node())
        .expect("a button");
    tree.insert(row, Button::new("Run", ButtonKind::Slate).node())
        .expect("a button");
    tree.set_tooltip(save, text).expect("a tooltip");
    vec![save]
}

fn tooltip_one_line(tree: &mut Tree) -> Vec<NodeId> {
    tooltip_bar(tree, "Write main.rs to disk (Ctrl+S)", false)
}

fn tooltip_two_lines(tree: &mut Tree) -> Vec<NodeId> {
    tooltip_bar(tree, "Save all\nWrites the 3 files that changed", false)
}

/// At the window's bottom, so that it flips above its node.
fn tooltip_above(tree: &mut Tree) -> Vec<NodeId> {
    tooltip_bar(tree, "Write main.rs to disk (Ctrl+S)", true)
}

/// Opens the tooltip of the cell's node, as its owner does once its target
/// is the node and its timer fires.
fn open_tooltip(shot: &mut Shot) -> NodeId {
    let anchor = shot.nodes[0];
    let target = shot.tree.tooltip_target().expect("a target");
    assert_eq!(target.node, anchor);
    let tip = Tooltip::open(&mut shot.tree, anchor).expect("opened");
    shot.layout();
    tip
}

fn tooltip_hover(shot: &mut Shot) {
    let at = shot.center_of(shot.nodes[0]);
    shot.hover(at);
    open_tooltip(shot);
}

fn tooltip_focus(shot: &mut Shot) {
    shot.key(Key::Tab);
    open_tooltip(shot);
}

fn tooltip_resting(shot: &mut Shot) {
    let at = shot.center_of(shot.nodes[0]);
    shot.hover(at);
    let tip = open_tooltip(shot);
    let bounds = shot.tree.bounds(tip).expect("laid out");
    shot.hover(Point::new(bounds.x + 6.0, bounds.y + 6.0));
    assert_eq!(
        shot.tree.tooltip_target().map(|target| target.node),
        Some(shot.nodes[0]),
        "the pointer on the tooltip keeps it"
    );
}

fn tooltip_disabled(shot: &mut Shot) {
    shot.tree
        .set_enabled(shot.nodes[0], false)
        .expect("disabled");
    shot.layout();
    tooltip_hover(shot);
}

const TOOLTIP_STATES: [Put; 4] = [
    tooltip_hover,
    tooltip_focus,
    tooltip_resting,
    tooltip_disabled,
];

const TOOLTIPS: [Make; 3] = [tooltip_one_line, tooltip_two_lines, tooltip_above];

#[test]
fn ui_tooltip() {
    shots("ui_tooltip", TOOLTIP_CELL, 1.0, &TOOLTIPS, &TOOLTIP_STATES);
}

#[test]
fn ui_tooltip_scale_2() {
    shots(
        "ui_tooltip_scale_2",
        TOOLTIP_CELL,
        2.0,
        &TOOLTIPS,
        &TOOLTIP_STATES,
    );
}

// The toast: a row for each toast, a column for each state: rest; hover,
// the pointer on its close control; focus, Tab onto its first control;
// pressed, its action (or its close control) pressed; and disabled.

const TOAST_CELL: Size = Size::new(380.0, 150.0);

/// A section title over the window, and `toast` shown in its corner; the
/// toast, then its close control, then its action if it has one.
fn toast_over_window(tree: &mut Tree, toast: Toast) -> Vec<NodeId> {
    tree.insert(tree.root(), SectionTitle::new("Workspace").node())
        .expect("a title");
    let shown = toast.show(tree).expect("shown");
    let mut nodes = vec![shown, Toast::close_button(tree, shown).expect("a close")];
    nodes.extend(Toast::action_button(tree, shown));
    nodes
}

fn toast_info(tree: &mut Tree) -> Vec<NodeId> {
    toast_over_window(tree, Toast::new("Build finished in 4.2 s."))
}

fn toast_warning(tree: &mut Tree) -> Vec<NodeId> {
    toast_over_window(
        tree,
        Toast::new("rust-analyzer 2026-11-24 is available.")
            .kind(ToastKind::Warning)
            .action("Update"),
    )
}

fn toast_error(tree: &mut Tree) -> Vec<NodeId> {
    toast_over_window(
        tree,
        Toast::new("cargo build failed.\n3 errors in src/main.rs")
            .kind(ToastKind::Error)
            .action("Show problems"),
    )
}

fn toast_rest(_shot: &mut Shot) {}

fn toast_hover(shot: &mut Shot) {
    let at = shot.center_of(shot.nodes[1]);
    shot.hover(at);
}

fn toast_focus(shot: &mut Shot) {
    shot.key(Key::Tab);
    assert_eq!(shot.tree.focused(), Some(shot.nodes[1]));
}

fn toast_pressed(shot: &mut Shot) {
    let target = *shot.nodes.last().expect("a node");
    let at = shot.center_of(target);
    shot.hover(at);
    shot.press(at);
}

fn toast_disabled(shot: &mut Shot) {
    shot.tree
        .set_enabled(shot.nodes[0], false)
        .expect("disabled");
    shot.layout();
    let at = shot.center_of(shot.nodes[1]);
    shot.hover(at);
    shot.press(at);
}

const TOAST_STATES: [Put; 5] = [
    toast_rest,
    toast_hover,
    toast_focus,
    toast_pressed,
    toast_disabled,
];

const TOASTS: [Make; 3] = [toast_info, toast_warning, toast_error];

#[test]
fn ui_toast() {
    shots("ui_toast", TOAST_CELL, 1.0, &TOASTS, &TOAST_STATES);
}

#[test]
fn ui_toast_scale_2() {
    shots("ui_toast_scale_2", TOAST_CELL, 2.0, &TOASTS, &TOAST_STATES);
}

// The command palette: a row for each list, a column for each state:
// rest, as it shows once its owner gave it items for the query typed;
// hover, the pointer over its third item; keys, Down twice; and pressed,
// its second item pressed. It has no disabled state: an owner disables no
// palette it opens.

const PALETTE_CELL: Size = Size::new(504.0, 300.0);

/// A window under the palette, the palette with `query` typed and `items`
/// given; the palette, then its list.
fn palette_over_window(tree: &mut Tree, query: &str, items: Vec<Row>) -> Vec<NodeId> {
    tree.insert(tree.root(), SectionTitle::new("Explorer").node())
        .expect("a title");
    tree.insert(
        tree.root(),
        Button::new("Open folder", ButtonKind::Ghost).node(),
    )
    .expect("a button");
    let palette = Palette::new("Command palette")
        .placeholder("Type a command")
        .open(tree)
        .expect("opened");
    tree.handle(&InputEvent::Text(query.to_string()));
    Palette::set_items(tree, palette, items).expect("items");
    vec![palette, Palette::list(tree, palette).expect("a list")]
}

fn palette_matches(tree: &mut Tree) -> Vec<NodeId> {
    // Each label with the (start, end) bytes the query matched.
    let items: [(&str, &[(usize, usize)]); 6] = [
        ("Open File", &[(0, 2)]),
        ("Open Folder", &[(0, 2)]),
        ("Open Recent", &[(0, 2)]),
        ("Go to Symbol", &[(1, 2), (9, 10)]),
        ("Toggle Problems Panel", &[(2, 3), (9, 10)]),
        ("Copy Path", &[(1, 3)]),
    ];
    let items = items
        .into_iter()
        .map(|(label, matched)| {
            Row::new(label).matched(matched.iter().map(|&(start, end)| start..end))
        })
        .collect();
    palette_over_window(tree, "op", items)
}

fn palette_empty(tree: &mut Tree) -> Vec<NodeId> {
    palette_over_window(tree, "zzz", Vec::new())
}

/// Thirty items, the selection wrapped to the last by Up and scrolled.
fn palette_long(tree: &mut Tree) -> Vec<NodeId> {
    let items = (1..=30)
        .map(|n| Row::new(format!("src/module_{n:02}.rs")).matched(Some(4..10)))
        .collect();
    let nodes = palette_over_window(tree, "module", items);
    tree.handle(&InputEvent::KeyDown {
        key: Key::Up,
        modifiers: Modifiers::NONE,
        repeat: false,
    });
    nodes
}

/// The node of the item at `index` among those on screen.
fn palette_row(shot: &Shot, index: usize) -> Option<NodeId> {
    shot.tree
        .accessibility()
        .nodes()
        .iter()
        .filter(|node| node.role == tidepool_ui::a11y::Role::ListItem)
        .nth(index)
        .map(|node| node.id)
}

fn palette_rest(_shot: &mut Shot) {}

fn palette_hover(shot: &mut Shot) {
    if let Some(row) = palette_row(shot, 2) {
        let at = shot.center_of(row);
        shot.hover(at);
    }
}

fn palette_keys(shot: &mut Shot) {
    shot.key(Key::Down);
    shot.key(Key::Down);
}

fn palette_pressed(shot: &mut Shot) {
    if let Some(row) = palette_row(shot, 1) {
        let at = shot.center_of(row);
        shot.hover(at);
        shot.press(at);
    }
}

const PALETTE_STATES: [Put; 4] = [palette_rest, palette_hover, palette_keys, palette_pressed];

const PALETTES: [Make; 3] = [palette_matches, palette_empty, palette_long];

#[test]
fn ui_palette() {
    shots("ui_palette", PALETTE_CELL, 1.0, &PALETTES, &PALETTE_STATES);
}

#[test]
fn ui_palette_scale_2() {
    shots(
        "ui_palette_scale_2",
        PALETTE_CELL,
        2.0,
        &PALETTES,
        &PALETTE_STATES,
    );
}
