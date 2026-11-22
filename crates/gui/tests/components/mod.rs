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

//! The golden scenes of `ui`'s M3.7 components (ADR-0052): the list, the
//! tree view, the table, the tab strip, the splitter, the card and the
//! code block, at scale 1 and 2, each a grid of a row for each variant
//! and a column for each state: rest, hover, focus, pressed and disabled,
//! and each component's own (a selected row or tab, a column header
//! hovered and pressed, the splitter dragged, COPY hovered and pressed).
//! The code block has no hover look of its own: its hover column, the
//! pointer over its code, is the same as its rest column, and shows that.
//!
//! Each cell is a tree of its own holding the component, put in its state
//! as a user or its owner would put it: a pointer move, Tab, a press
//! without its release, a press and a move, `widgets::update` or
//! `Tree::set_enabled` with the pointer over it and pressing. Its rows are
//! fixed text. Each cell is painted by [`Painter`] over the theme's `bg`
//! and copied into the scene, which is written as the other scenes are.

use tidepool_core::geom::{DevicePoint, DeviceSize, Point, Rect, Scale, Size};
use tidepool_core::input::{InputEvent, Key, Modifiers, MouseButton};
use tidepool_gfx::framebuffer::Framebuffer;
use tidepool_gui::Painter;
use tidepool_ui::draw::{DrawList, PaintContext};
use tidepool_ui::layout::{Axis, Insets, Length, Style};
use tidepool_ui::theme::Token;
use tidepool_ui::widgets::{
    self, Button, ButtonKind, Card, CodeBlock, CodeSpan, Collection, Column, Control, Row,
    Splitter, Tab, TabKind, TabStrip,
};
use tidepool_ui::{Node, NodeId, Tree, Widget};

use super::{painter, save, token};

/// A cell being put in its state: its tree, the component's node, and
/// what lays it out.
struct Cell {
    tree: Tree,
    main: NodeId,
    size: Size,
    painter: Painter,
    scale: Scale,
}

impl Cell {
    fn layout(&mut self) {
        self.tree
            .layout(self.size, &self.painter.metrics(self.scale));
    }

    fn bounds(&self, node: NodeId) -> Rect {
        self.tree.bounds(node).expect("laid out")
    }

    /// A point `dx` and `dy` from the component's top-left corner.
    fn at(&self, dx: f32, dy: f32) -> Point {
        let bounds = self.bounds(self.main);
        Point::new(bounds.x + dx, bounds.y + dy)
    }

    fn centre_of(&self, node: NodeId) -> Point {
        let bounds = self.bounds(node);
        Point::new(
            bounds.x + bounds.width / 2.0,
            bounds.y + bounds.height / 2.0,
        )
    }

    fn hover(&mut self, at: Point) {
        self.tree.handle(&InputEvent::MouseMove {
            position: at,
            modifiers: Modifiers::NONE,
        });
    }

    fn press(&mut self, at: Point) {
        self.tree.handle(&InputEvent::MouseDown {
            button: MouseButton::Left,
            position: at,
            modifiers: Modifiers::NONE,
        });
    }

    fn key(&mut self, key: Key) {
        self.tree.handle(&InputEvent::KeyDown {
            key,
            modifiers: Modifiers::NONE,
            repeat: false,
        });
        self.layout();
    }

    /// Disables the component, and puts the pointer over `at` and presses
    /// there: a disabled component shows neither (ADR-0049).
    fn disable(&mut self, at: Point) {
        self.tree.set_enabled(self.main, false).expect("disabled");
        self.layout();
        self.hover(at);
        self.press(at);
    }
}

/// Builds a row's component under the cell's root, and returns its node.
type Build = fn(&mut Tree, NodeId) -> NodeId;

/// Puts a cell in a column's state.
type Act = fn(&mut Cell);

/// A cell of `size` logical units holding what `build` builds, put in its
/// state by `act`, painted at `factor`.
fn cell(size: Size, factor: f32, build: Build, act: Act) -> Framebuffer {
    let mut tree = Tree::new();
    let root = tree.root();
    tree.set_style(root, Style::column().padding(Insets::all(8.0)))
        .expect("a style");
    let main = build(&mut tree, root);
    let scale = Scale::new(factor).expect("a scale");
    let mut cell = Cell {
        tree,
        main,
        size,
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

/// Paints a grid of `rows`, a cell of `size` for each of `columns`, at
/// `factor`, and writes it as `name`.
fn grid(name: &str, size: Size, factor: f32, rows: &[Build], columns: &[Act]) {
    let (cell_width, cell_height) = ((size.width * factor) as i32, (size.height * factor) as i32);
    let device = DeviceSize::new(
        cell_width * columns.len() as i32,
        cell_height * rows.len() as i32,
    );
    let mut scene = Framebuffer::new(device).expect("a scene");
    let bounds = scene.bounds();
    for (row, &build) in rows.iter().enumerate() {
        for (column, &act) in columns.iter().enumerate() {
            let painted = cell(size, factor, build, act);
            let at = DevicePoint::new(cell_width * column as i32, cell_height * row as i32);
            tidepool_gfx::image::copy(&mut scene, bounds, &painted.view(), at);
        }
    }
    save(name, &scene);
}

/// Nothing: the rest state.
fn rest(_cell: &mut Cell) {}

/// Tab, from the keyboard: focus on the first tab stop, showing.
fn focus(cell: &mut Cell) {
    cell.key(Key::Tab);
}

// ---------- The collections ----------

const LIST_CELL: Size = Size::new(200.0, 96.0);

fn file_rows() -> Vec<Row> {
    ["Cargo.toml", "README.md", "build.rs", "src", "tests"]
        .map(Row::new)
        .to_vec()
}

fn list_plain(tree: &mut Tree, root: NodeId) -> NodeId {
    tree.insert(root, Collection::list("Files", file_rows()).node())
        .expect("a list")
}

/// A list scrolled by half a row, so that its first and last rows are cut.
fn list_scrolled(tree: &mut Tree, root: NodeId) -> NodeId {
    let rows: Vec<Row> = (1..=12)
        .map(|n| Row::new(format!("module_{n}.rs")))
        .collect();
    let mut list = Collection::list("Modules", rows);
    list.scroll_to(10.0);
    tree.insert(root, list.node()).expect("a list")
}

fn explorer_collapsed(tree: &mut Tree, root: NodeId) -> NodeId {
    let rows = vec![
        Row::new("src").expanded(false),
        Row::new("tests").expanded(false),
        Row::new("Cargo.toml"),
        Row::new("README.md"),
    ];
    tree.insert(root, Collection::tree_view("Explorer", rows).node())
        .expect("a tree view")
}

fn explorer_expanded(tree: &mut Tree, root: NodeId) -> NodeId {
    let rows = vec![
        Row::new("src").expanded(true),
        Row::new("widgets").depth(1).expanded(false).parent(0),
        Row::new("lib.rs").depth(1).parent(0),
        Row::new("Cargo.toml"),
    ];
    tree.insert(root, Collection::tree_view("Explorer", rows).node())
        .expect("a tree view")
}

/// The second row's middle, below a table's header if it has one.
fn second_row(cell: &Cell) -> Point {
    let header = cell
        .tree
        .widget::<Collection>(cell.main)
        .is_some_and(|c| c.kind() == widgets::CollectionKind::Table);
    let top = if header { 22.0 } else { 2.0 };
    cell.at(60.0, top + 30.0)
}

fn row_hover(cell: &mut Cell) {
    let at = second_row(cell);
    cell.hover(at);
}

fn row_press(cell: &mut Cell) {
    let at = second_row(cell);
    cell.press(at);
}

/// The owner selects the second row, with focus elsewhere.
fn row_selected(cell: &mut Cell) {
    let main = cell.main;
    widgets::update(&mut cell.tree, main, |c: &mut Collection| c.select(Some(1)))
        .expect("a collection");
    cell.layout();
}

/// The second row selected from the keyboard: Tab, then Down twice.
fn row_keyed(cell: &mut Cell) {
    cell.key(Key::Tab);
    cell.key(Key::Down);
    cell.key(Key::Down);
}

fn row_disabled(cell: &mut Cell) {
    row_selected(cell);
    let at = second_row(cell);
    cell.disable(at);
}

const ROW_STATES: [Act; 6] = [rest, row_hover, focus, row_press, row_keyed, row_disabled];

fn lists() -> Vec<Build> {
    vec![list_plain, list_scrolled]
}

fn tree_views() -> Vec<Build> {
    vec![explorer_collapsed, explorer_expanded]
}

/// 250 wide, so that eight columns of it at scale 2 are no wider than
/// the 4,096 pixels a scene may be.
const TABLE_CELL: Size = Size::new(250.0, 104.0);

fn problems(tree: &mut Tree, root: NodeId) -> NodeId {
    let rows = vec![
        Row::cells(["error", "mismatched types"]),
        Row::cells(["warning", "unused import: `std::fmt`"]),
        Row::cells(["note", "consider borrowing here"]),
    ];
    let columns = [Column::new("Severity", 84.0), Column::new("Message", 140.0)];
    tree.insert(root, Collection::table("Problems", columns, rows).node())
        .expect("a table")
}

/// The first column's header.
fn header_point(cell: &Cell) -> Point {
    cell.at(30.0, 12.0)
}

fn header_hover(cell: &mut Cell) {
    let at = header_point(cell);
    cell.hover(at);
}

fn header_press(cell: &mut Cell) {
    let at = header_point(cell);
    cell.press(at);
}

const TABLE_STATES: [Act; 8] = [
    rest,
    row_hover,
    focus,
    row_press,
    row_keyed,
    row_disabled,
    header_hover,
    header_press,
];

// ---------- The tab strip ----------

const TABS_CELL: Size = Size::new(300.0, 52.0);

fn panel_tabs(tree: &mut Tree, root: NodeId) -> NodeId {
    let tabs = ["Problems", "Output", "Terminal"].map(Tab::new);
    tree.insert(root, TabStrip::new("Panel", TabKind::Chrome, tabs).node())
        .expect("a strip")
}

fn editor_tabs(tree: &mut Tree, root: NodeId) -> NodeId {
    let tabs = ["main.rs", "lib.rs", "README.md"].map(|name| Tab::new(name).closable());
    tree.insert(
        root,
        TabStrip::new("Editors", TabKind::Content, tabs).node(),
    )
    .expect("a strip")
}

/// More tabs than fit, the last selected and scrolled into view.
fn many_tabs(tree: &mut Tree, root: NodeId) -> NodeId {
    let tabs = (1..=9).map(|n| Tab::new(format!("view_{n}.rs")).closable());
    let mut strip = TabStrip::new("Editors", TabKind::Content, tabs);
    strip.select(8);
    tree.insert(root, strip.node()).expect("a strip")
}

/// The strip's tab nodes.
fn tab_nodes(cell: &Cell) -> Vec<NodeId> {
    let track = cell.tree.children(cell.main).expect("a track")[0];
    cell.tree.children(track).expect("tabs").to_vec()
}

/// A point on the label of the tab after the selected one, or before it
/// if it is the last.
fn other_tab(cell: &Cell) -> Point {
    let nodes = tab_nodes(cell);
    let selected = cell
        .tree
        .widget::<TabStrip>(cell.main)
        .and_then(TabStrip::selected)
        .unwrap_or(0);
    let other = if selected + 1 < nodes.len() {
        selected + 1
    } else {
        selected.saturating_sub(1)
    };
    let bounds = cell.bounds(nodes[other]);
    Point::new(bounds.x + 14.0, bounds.y + bounds.height / 2.0)
}

fn tab_hover(cell: &mut Cell) {
    let at = other_tab(cell);
    cell.hover(at);
}

fn tab_press(cell: &mut Cell) {
    let at = other_tab(cell);
    cell.press(at);
}

fn tab_disabled(cell: &mut Cell) {
    let at = other_tab(cell);
    cell.disable(at);
}

/// The pointer over the selected tab's close control, if it has one, or
/// its label.
fn close_hover(cell: &mut Cell) {
    let nodes = tab_nodes(cell);
    let selected = cell
        .tree
        .widget::<TabStrip>(cell.main)
        .and_then(TabStrip::selected)
        .unwrap_or(0);
    let tab = nodes[selected];
    let at = match cell.tree.children(tab).and_then(|c| c.first().copied()) {
        Some(close) => cell.centre_of(close),
        None => cell.centre_of(tab),
    };
    cell.hover(at);
}

const TAB_STATES: [Act; 6] = [rest, tab_hover, focus, tab_press, tab_disabled, close_hover];

fn tab_strips() -> Vec<Build> {
    vec![panel_tabs, editor_tabs, many_tabs]
}

// ---------- The splitter ----------

const SPLIT_CELL: Size = Size::new(160.0, 96.0);

/// A pane: a fill of one of the theme's tokens.
struct Pane(Token);

impl Widget for Pane {
    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        list.rect(cx.bounds(), cx.theme().color(self.0));
    }
}

fn pane(token: Token) -> Node {
    Node::new(Style::stack().fill()).with_widget(Pane(token))
}

fn split_of(tree: &mut Tree, root: NodeId, axis: Axis) -> NodeId {
    let split = Splitter::new(axis, "Side panel", 0.4)
        .insert(tree, root, pane(Token::Slate2), pane(Token::EditorBg))
        .expect("a split");
    Splitter::divider(tree, split).expect("a divider")
}

fn split_across(tree: &mut Tree, root: NodeId) -> NodeId {
    split_of(tree, root, Axis::Horizontal)
}

fn split_down(tree: &mut Tree, root: NodeId) -> NodeId {
    split_of(tree, root, Axis::Vertical)
}

fn divider_hover(cell: &mut Cell) {
    let at = cell.centre_of(cell.main);
    cell.hover(at);
}

fn divider_press(cell: &mut Cell) {
    let at = cell.centre_of(cell.main);
    cell.press(at);
}

/// The divider pressed and dragged 20 units along its axis.
fn divider_drag(cell: &mut Cell) {
    let at = cell.centre_of(cell.main);
    cell.press(at);
    let horizontal = cell
        .tree
        .widget::<Splitter>(cell.main)
        .is_some_and(|s| s.axis() == Axis::Horizontal);
    let to = if horizontal {
        Point::new(at.x + 20.0, at.y)
    } else {
        Point::new(at.x, at.y + 20.0)
    };
    cell.hover(to);
    cell.layout();
}

fn divider_disabled(cell: &mut Cell) {
    let at = cell.centre_of(cell.main);
    cell.disable(at);
}

const SPLIT_STATES: [Act; 6] = [
    rest,
    divider_hover,
    focus,
    divider_press,
    divider_drag,
    divider_disabled,
];

fn splitters() -> Vec<Build> {
    vec![split_across, split_down]
}

// ---------- The card ----------

const CARD_CELL: Size = Size::new(260.0, 140.0);

/// A card 240 wide holding a button: the card.
fn card_with(tree: &mut Tree, root: NodeId, card: Card, button: Button) -> NodeId {
    let node = Node {
        style: Style {
            width: Length::Fixed(240.0),
            ..card.style()
        },
        ..card.node()
    };
    let id = tree.insert(root, node).expect("a card");
    tree.insert(id, button.node()).expect("a button");
    id
}

fn card_ready(tree: &mut Tree, root: NodeId) -> NodeId {
    let card = Card::new("rust-analyzer")
        .kicker("Toolchain")
        .text("2026-11-17, up to date")
        .tab("Ready");
    card_with(tree, root, card, Button::new("Open", ButtonKind::Ghost))
}

fn card_update(tree: &mut Tree, root: NodeId) -> NodeId {
    let card = Card::new("Android SDK")
        .kicker("Toolchain")
        .text("platform-tools 36.0.2 is out")
        .tab("Update");
    card_with(tree, root, card, Button::new("Update", ButtonKind::Teal))
}

/// The card's top right, clear of its button.
fn card_hover(cell: &mut Cell) {
    let at = cell.at(120.0, 40.0);
    cell.hover(at);
}

/// A press on the card's button.
fn card_press(cell: &mut Cell) {
    let button = cell.tree.children(cell.main).expect("a button")[0];
    let at = cell.centre_of(button);
    cell.press(at);
}

fn card_disabled(cell: &mut Cell) {
    let at = cell.at(120.0, 40.0);
    cell.disable(at);
}

const CARD_STATES: [Act; 5] = [rest, card_hover, focus, card_press, card_disabled];

fn cards() -> Vec<Build> {
    vec![card_ready, card_update]
}

// ---------- The code block ----------

const CODE_CELL: Size = Size::new(300.0, 144.0);

fn rust_snippet(tree: &mut Tree, root: NodeId) -> NodeId {
    let lines = [
        vec![
            CodeSpan::new("fn", Token::Teal),
            CodeSpan::new(" main", Token::Text),
            CodeSpan::new("() {", Token::Text2),
        ],
        vec![CodeSpan::new("    // the first frame", Token::Comment)],
        vec![
            CodeSpan::new("    let", Token::Teal),
            CodeSpan::new(" retries = ", Token::Text),
            CodeSpan::new("3", Token::Amber),
            CodeSpan::new(";", Token::Text2),
        ],
        vec![CodeSpan::new("}", Token::Text2)],
    ];
    CodeBlock::new("rust", lines)
        .insert(tree, root)
        .expect("a block")
}

/// A line wider than the block, cut at its right edge.
fn long_line(tree: &mut Tree, root: NodeId) -> NodeId {
    let text = "cargo build --release --target x86_64-unknown-linux-gnu --locked\n\
                cargo test";
    CodeBlock::plain("sh", text)
        .insert(tree, root)
        .expect("a block")
}

fn copy_of(cell: &Cell) -> NodeId {
    CodeBlock::copy_button(&cell.tree, cell.main).expect("COPY")
}

/// The pointer over the code, clear of COPY: the block has no hover look,
/// so this column shows that nothing changes, as the rest column does.
fn code_hover(cell: &mut Cell) {
    let at = cell.at(60.0, 60.0);
    cell.hover(at);
}

fn copy_hover(cell: &mut Cell) {
    let at = cell.centre_of(copy_of(cell));
    cell.hover(at);
}

fn copy_press(cell: &mut Cell) {
    let at = cell.centre_of(copy_of(cell));
    cell.press(at);
}

fn code_disabled(cell: &mut Cell) {
    let at = cell.centre_of(copy_of(cell));
    cell.disable(at);
}

const CODE_STATES: [Act; 6] = [
    rest,
    code_hover,
    copy_hover,
    focus,
    copy_press,
    code_disabled,
];

fn code_blocks() -> Vec<Build> {
    vec![rust_snippet, long_line]
}

#[test]
fn ui_list() {
    grid("ui_list", LIST_CELL, 1.0, &lists(), &ROW_STATES);
}

#[test]
fn ui_list_scale_2() {
    grid("ui_list_scale_2", LIST_CELL, 2.0, &lists(), &ROW_STATES);
}

#[test]
fn ui_tree_view() {
    grid("ui_tree_view", LIST_CELL, 1.0, &tree_views(), &ROW_STATES);
}

#[test]
fn ui_tree_view_scale_2() {
    grid(
        "ui_tree_view_scale_2",
        LIST_CELL,
        2.0,
        &tree_views(),
        &ROW_STATES,
    );
}

#[test]
fn ui_table() {
    grid("ui_table", TABLE_CELL, 1.0, &[problems], &TABLE_STATES);
}

#[test]
fn ui_table_scale_2() {
    grid(
        "ui_table_scale_2",
        TABLE_CELL,
        2.0,
        &[problems],
        &TABLE_STATES,
    );
}

#[test]
fn ui_tab_strip() {
    grid("ui_tab_strip", TABS_CELL, 1.0, &tab_strips(), &TAB_STATES);
}

#[test]
fn ui_tab_strip_scale_2() {
    grid(
        "ui_tab_strip_scale_2",
        TABS_CELL,
        2.0,
        &tab_strips(),
        &TAB_STATES,
    );
}

#[test]
fn ui_splitter() {
    grid("ui_splitter", SPLIT_CELL, 1.0, &splitters(), &SPLIT_STATES);
}

#[test]
fn ui_splitter_scale_2() {
    grid(
        "ui_splitter_scale_2",
        SPLIT_CELL,
        2.0,
        &splitters(),
        &SPLIT_STATES,
    );
}

#[test]
fn ui_card() {
    grid("ui_card", CARD_CELL, 1.0, &cards(), &CARD_STATES);
}

#[test]
fn ui_card_scale_2() {
    grid("ui_card_scale_2", CARD_CELL, 2.0, &cards(), &CARD_STATES);
}

#[test]
fn ui_code_block() {
    grid(
        "ui_code_block",
        CODE_CELL,
        1.0,
        &code_blocks(),
        &CODE_STATES,
    );
}

#[test]
fn ui_code_block_scale_2() {
    grid(
        "ui_code_block_scale_2",
        CODE_CELL,
        2.0,
        &code_blocks(),
        &CODE_STATES,
    );
}
