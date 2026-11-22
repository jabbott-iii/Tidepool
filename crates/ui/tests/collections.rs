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

//! M3.7's collections, the list, the tree view and the table, through the
//! public API only (ADR-0052): keyboard navigation for each, at the edges,
//! with no rows and with the selection scrolled away; a million rows kept
//! as nodes only for the rows on screen, which are the only ones asked
//! for, at the root and inside a scroller, and a 64-column table that
//! leaves the tree room; every row on screen with its role and a name,
//! from what it shows only; the selection scrolled into view, the least,
//! within the collection and by the scrollers around it; idle frames
//! empty and passing no layout; a press selecting the row drawn under it;
//! scrolling damaging the collection only; the wheel passing on what the
//! rows cannot take, and over a disabled collection; the model changed
//! under the selection; a collection's widget replaced; rows that survive
//! being hidden; long text capped; the focus outline inside the bounds;
//! and the theme's tokens each paints in each state. A property test
//! drives random keys, presses, scrolls, resizes and model changes, several
//! to a frame, at the root or in a scroller, and checks the rows kept, the
//! nodes, the names, the reveals and the idle frames after each frame.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use tidepool_core::color::Color;
use tidepool_core::geom::{Point, Rect, Size};
use tidepool_core::input::{InputEvent, Key, Modifiers, MouseButton, ScrollDelta};
use tidepool_core::rng::Pcg32;
use tidepool_core::testing::check_sized;
use tidepool_text::pos::grapheme;
use tidepool_ui::a11y::{AccessNode, Role};
use tidepool_ui::draw::Command;
use tidepool_ui::input::{ActionKind, Response};
use tidepool_ui::layout::{Length, Style};
use tidepool_ui::metrics::{FixedMetrics, TextMetrics};
use tidepool_ui::theme::{LIMITS, TIDEPOOL_DARK_TOML, Theme, Token};
use tidepool_ui::widgets::{
    self, Collection, CollectionKind, Column, Control, DISABLED_OPACITY, FOCUS_WIDTH,
    MAX_CELL_BYTES, MAX_VIEW_ROWS, ROW_HEIGHT, ROW_MARGIN, Row, Rows,
};
use tidepool_ui::{Node, NodeId, Tree, Widget};

const METRICS: FixedMetrics = FixedMetrics::DEFAULT;
const WINDOW: Size = Size::new(400.0, 300.0);

fn key(key: Key) -> InputEvent {
    key_with(key, Modifiers::NONE)
}

fn key_with(key: Key, modifiers: Modifiers) -> InputEvent {
    InputEvent::KeyDown {
        key,
        modifiers,
        repeat: false,
    }
}

fn key_up(key: Key) -> InputEvent {
    InputEvent::KeyUp {
        key,
        modifiers: Modifiers::NONE,
    }
}

fn down(at: Point) -> InputEvent {
    InputEvent::MouseDown {
        button: MouseButton::Left,
        position: at,
        modifiers: Modifiers::NONE,
    }
}

fn up(at: Point) -> InputEvent {
    InputEvent::MouseUp {
        button: MouseButton::Left,
        position: at,
        modifiers: Modifiers::NONE,
    }
}

fn click(tree: &mut Tree, at: Point) -> Response {
    let response = tree.handle(&down(at));
    tree.handle(&up(at));
    response
}

fn hover(at: Point) -> InputEvent {
    InputEvent::MouseMove {
        position: at,
        modifiers: Modifiers::NONE,
    }
}

fn wheel(at: Point, lines: f32) -> InputEvent {
    InputEvent::Wheel {
        delta: ScrollDelta::Lines { x: 0.0, y: lines },
        position: at,
        modifiers: Modifiers::NONE,
    }
}

fn color(token: Token) -> Color {
    Theme::bundled().color(token)
}

/// `count` rows of a list or tree view, `name 1` to `name count`.
fn labels(count: usize) -> Vec<Row> {
    (1..=count).map(|n| Row::new(format!("name {n}"))).collect()
}

/// A tree holding `collection` under the root, with its first frame drawn
/// in [`WINDOW`].
fn single(collection: Collection) -> (Tree, NodeId) {
    let mut tree = Tree::new();
    let id = tree
        .insert(tree.root(), collection.node())
        .expect("a collection");
    assert!(tree.frame(WINDOW, &METRICS).is_some());
    (tree, id)
}

/// The same, focused, with focus showing.
fn focused(collection: Collection) -> (Tree, NodeId) {
    let (mut tree, id) = single(collection);
    tree.set_focus(id, true).expect("focus");
    (tree, id)
}

/// A column that scrolls vertically under the root, holding `above` units
/// of other content, then `node`, then 400 more: the column and the node.
fn in_scroller(tree: &mut Tree, above: f32, node: Node) -> (NodeId, NodeId) {
    let column = tree
        .insert(
            tree.root(),
            Node::new(Style::column().fill().scroll(false, true)),
        )
        .expect("a column");
    let filler = |height: f32| {
        Node::new(
            Style::default()
                .width(Length::Fill(1.0))
                .height(Length::Fixed(height)),
        )
    };
    tree.insert(column, filler(above)).expect("above");
    let id = tree.insert(column, node).expect("the collection");
    tree.insert(column, filler(400.0)).expect("below");
    (column, id)
}

/// `node` given `style` in place of its own.
fn styled(node: Node, style: Style) -> Node {
    Node { style, ..node }
}

fn selected(tree: &Tree, id: NodeId) -> Option<usize> {
    tree.widget::<Collection>(id).and_then(Collection::selected)
}

fn offset(tree: &Tree, id: NodeId) -> f64 {
    tree.widget::<Collection>(id)
        .map_or(f64::NAN, Collection::offset)
}

fn value(tree: &Tree, id: NodeId) -> Option<String> {
    tree.semantics(id)
        .and_then(|semantics| semantics.value.clone())
}

/// The kinds of the actions `response` holds of `id`.
fn kinds(response: &Response, id: NodeId) -> Vec<ActionKind> {
    response
        .actions
        .iter()
        .filter(|action| action.node == id)
        .map(|action| action.kind)
        .collect()
}

/// Presses `key` and returns the kinds of what `id` emitted, and whether
/// the key was handled.
fn press_key(tree: &mut Tree, id: NodeId, pressed: Key) -> (Vec<ActionKind>, bool) {
    let response = tree.handle(&key(pressed));
    (kinds(&response, id), response.handled)
}

/// The exposed rows of the last layout: list items, tree items and rows
/// that are not a table's header.
fn rows(tree: &Tree) -> Vec<AccessNode> {
    tree.accessibility()
        .nodes()
        .iter()
        .filter(|node| matches!(node.role, Role::ListItem | Role::TreeItem | Role::Row))
        .filter(|node| {
            // A table's header row holds the column headers.
            !tree.children(node.id).is_some_and(|children| {
                children.iter().any(|&child| {
                    tree.semantics(child)
                        .is_some_and(|semantics| semantics.role == Role::ColumnHeader)
                })
            })
        })
        .cloned()
        .collect()
}

/// The rows' names, in order.
fn names(tree: &Tree) -> Vec<String> {
    rows(tree).into_iter().map(|node| node.name).collect()
}

/// The selected row's node in the accessibility tree.
fn chosen(tree: &Tree) -> Option<AccessNode> {
    rows(tree).into_iter().find(|node| node.selected)
}

/// The collection's body: its last child, where its rows show.
fn body(tree: &Tree, id: NodeId) -> NodeId {
    *tree
        .children(id)
        .and_then(<[NodeId]>::last)
        .expect("a body")
}

/// The area a collection's rows show in: its body's bounds.
fn view(tree: &Tree, id: NodeId) -> Rect {
    tree.bounds(body(tree, id)).expect("bounds")
}

/// The intersection of two rects, empty where they do not meet.
fn meet(a: Rect, b: Rect) -> Rect {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    Rect::new(
        x,
        y,
        (a.right().min(b.right()) - x).max(0.0),
        (a.bottom().min(b.bottom()) - y).max(0.0),
    )
}

/// Whether `inner` lies inside `outer`, give or take [`SLACK`]: rows are
/// placed in `f32`, so an edge can round past by a unit in the last place.
fn inside(inner: Rect, outer: Rect) -> bool {
    outer.x - SLACK <= inner.x
        && inner.right() <= outer.right() + SLACK
        && outer.y - SLACK <= inner.y
        && inner.bottom() <= outer.bottom() + SLACK
}

/// What [`inside`] allows: far below a pixel at any scale.
const SLACK: f32 = 1e-3;

/// How many rows the `damage` covers, each rect inside `area` and in one
/// row's band.
fn bands(damage: &[Rect], area: Rect) -> usize {
    let mut tops: Vec<f32> = Vec::new();
    for rect in damage {
        assert!(inside(*rect, area) && rect.height <= ROW_HEIGHT, "{rect:?}");
        if !tops.contains(&rect.y) {
            tops.push(rect.y);
        }
    }
    tops.len()
}

/// The rows of `len` that meet a view `height` high scrolled to `offset`:
/// `(first, count)`, as many as a whole view on screen keeps.
fn meeting(len: usize, height: f32, offset: f64) -> (usize, usize) {
    let row = f64::from(ROW_HEIGHT);
    let reach = f64::from(height).min(len as f64 * row - offset).max(0.0);
    if len == 0 || reach <= 0.0 {
        return (0, 0);
    }
    let first = (offset / row).floor() as usize;
    let end = (((offset + reach) / row).ceil() as usize).min(len);
    (first, end - first)
}

/// Counts its measures: one for each layout pass.
struct Passes(Rc<Cell<usize>>);

impl Widget for Passes {
    /// Nothing, so that it moves no other node.
    fn measure(&self, _available: Size, _metrics: &dyn TextMetrics) -> Size {
        self.0.set(self.0.get() + 1);
        Size::ZERO
    }
}

/// Puts a node under the root that counts the layout passes.
fn count_passes(tree: &mut Tree) -> Rc<Cell<usize>> {
    let passes = Rc::new(Cell::new(0));
    let root = tree.root();
    tree.insert(
        root,
        Node::default().with_widget(Passes(Rc::clone(&passes))),
    )
    .expect("a counter");
    passes
}

#[test]
fn a_list_moves_its_selection_with_each_key_and_stops_at_its_ends() {
    let (mut tree, list) = focused(Collection::list("Files", labels(40)));
    // From no selection, Down selects the first row.
    assert_eq!(
        press_key(&mut tree, list, Key::Down),
        (vec![ActionKind::Select], true)
    );
    assert_eq!(selected(&tree, list), Some(0));
    assert_eq!(
        press_key(&mut tree, list, Key::Down).0,
        [ActionKind::Select]
    );
    assert_eq!(selected(&tree, list), Some(1));
    assert_eq!(press_key(&mut tree, list, Key::Up).0, [ActionKind::Select]);
    // At the first row Up stays, keeps the key and emits nothing.
    assert_eq!(press_key(&mut tree, list, Key::Up), (vec![], true));
    assert_eq!(selected(&tree, list), Some(0));
    assert_eq!(press_key(&mut tree, list, Key::End).0, [ActionKind::Select]);
    assert_eq!(selected(&tree, list), Some(39));
    assert_eq!(press_key(&mut tree, list, Key::Down), (vec![], true));
    assert_eq!(press_key(&mut tree, list, Key::PageDown), (vec![], true));
    assert_eq!(selected(&tree, list), Some(39));
    assert_eq!(
        press_key(&mut tree, list, Key::Home).0,
        [ActionKind::Select]
    );
    assert_eq!(selected(&tree, list), Some(0));
    // A page is the rows wholly on screen: 296 units of 20, inside the
    // padding its focus outline takes.
    tree.frame(WINDOW, &METRICS);
    assert_eq!(view(&tree, list), Rect::new(2.0, 2.0, 396.0, 296.0));
    press_key(&mut tree, list, Key::PageDown);
    assert_eq!(selected(&tree, list), Some(14));
    press_key(&mut tree, list, Key::PageDown);
    press_key(&mut tree, list, Key::PageDown);
    assert_eq!(selected(&tree, list), Some(39));
    press_key(&mut tree, list, Key::PageUp);
    assert_eq!(selected(&tree, list), Some(25));
    press_key(&mut tree, list, Key::PageUp);
    press_key(&mut tree, list, Key::PageUp);
    assert_eq!(selected(&tree, list), Some(0));
    assert_eq!(press_key(&mut tree, list, Key::PageUp), (vec![], true));
    // Enter activates the selected row; its repeats do not.
    for enter in [Key::Enter, Key::NumpadEnter] {
        assert_eq!(
            press_key(&mut tree, list, enter),
            (vec![ActionKind::Activate], true)
        );
    }
    let repeat = InputEvent::KeyDown {
        key: Key::Enter,
        modifiers: Modifiers::NONE,
        repeat: true,
    };
    let response = tree.handle(&repeat);
    assert!(response.handled && response.actions.is_empty());
    // Left and Right, and keys with a modifier, are the owner's.
    for owner in [Key::Left, Key::Right, Key::Space] {
        assert_eq!(
            press_key(&mut tree, list, owner),
            (vec![], false),
            "{owner:?}"
        );
    }
    for modifiers in [Modifiers::CTRL, Modifiers::SHIFT, Modifiers::ALT] {
        let response = tree.handle(&key_with(Key::Down, modifiers));
        assert!(!response.handled && response.actions.is_empty());
    }
    assert_eq!(selected(&tree, list), Some(0));
    // Its value follows the selection.
    tree.frame(WINDOW, &METRICS);
    assert_eq!(value(&tree, list).as_deref(), Some("row 1 of 40"));
}

#[test]
fn from_no_selection_every_key_but_end_selects_the_first_row() {
    for (pressed, row) in [
        (Key::Down, 0),
        (Key::Up, 0),
        (Key::PageDown, 0),
        (Key::PageUp, 0),
        (Key::Home, 0),
        (Key::End, 9),
    ] {
        let (mut tree, list) = focused(Collection::list("Files", labels(10)));
        assert_eq!(
            press_key(&mut tree, list, pressed),
            (vec![ActionKind::Select], true)
        );
        assert_eq!(selected(&tree, list), Some(row), "{pressed:?}");
    }
    // Enter with nothing selected is the owner's.
    let (mut tree, list) = focused(Collection::list("Files", labels(10)));
    assert_eq!(press_key(&mut tree, list, Key::Enter), (vec![], false));
}

#[test]
fn with_no_rows_the_keys_select_nothing() {
    let columns = [Column::new("Name", 100.0)];
    for collection in [
        Collection::list("Empty", Vec::<Row>::new()),
        Collection::tree_view("Empty", Vec::<Row>::new()),
        Collection::table("Empty", columns.clone(), Vec::<Row>::new()),
    ] {
        let kind = collection.kind();
        let (mut tree, id) = focused(collection);
        for pressed in [
            Key::Down,
            Key::Up,
            Key::PageDown,
            Key::PageUp,
            Key::Home,
            Key::End,
        ] {
            assert_eq!(
                press_key(&mut tree, id, pressed),
                (vec![], true),
                "{kind:?} {pressed:?}"
            );
        }
        assert_eq!(press_key(&mut tree, id, Key::Enter), (vec![], false));
        if kind == CollectionKind::TreeView {
            for pressed in [Key::Left, Key::Right] {
                assert_eq!(press_key(&mut tree, id, pressed), (vec![], true));
            }
        }
        // A press where rows would be selects nothing either.
        let at = Point::new(50.0, view(&tree, id).y + 5.0);
        click(&mut tree, at);
        assert_eq!(selected(&tree, id), None, "{kind:?}");
        tree.frame(WINDOW, &METRICS);
        assert!(rows(&tree).is_empty(), "{kind:?}");
        assert_eq!(value(&tree, id).as_deref(), Some("0 rows"));
    }
}

/// An explorer's model: `src`, collapsed, and `Cargo.toml`.
fn explorer() -> Vec<Row> {
    vec![Row::new("src").expanded(false), Row::new("Cargo.toml")]
}

/// Expands `src` in the explorer's model, as its owner does on `Expand`.
fn expand_src(tree: &mut Tree, id: NodeId) {
    widgets::update(tree, id, |view: &mut Collection| {
        if let Some(rows) = view.model_mut::<Vec<Row>>() {
            rows[0] = Row::new("src").expanded(true);
            rows.insert(1, Row::new("main.rs").depth(1).parent(0));
            rows.insert(2, Row::new("lib.rs").depth(1).parent(0));
        }
        view.rows_inserted(1, 2);
    })
    .expect("the explorer");
}

/// Collapses `src` again, as its owner does on `Collapse`.
fn collapse_src(tree: &mut Tree, id: NodeId) {
    widgets::update(tree, id, |view: &mut Collection| {
        if let Some(rows) = view.model_mut::<Vec<Row>>() {
            rows[0] = Row::new("src").expanded(false);
            rows.drain(1..3);
        }
        view.rows_removed(1..3);
    })
    .expect("the explorer");
}

#[test]
fn a_tree_view_expands_collapses_and_moves_between_levels() {
    let (mut tree, id) = focused(Collection::tree_view("Explorer", explorer()));
    // Left and Right with nothing selected do nothing.
    assert_eq!(press_key(&mut tree, id, Key::Right), (vec![], true));
    press_key(&mut tree, id, Key::Down);
    assert_eq!(selected(&tree, id), Some(0));
    // Right on a collapsed row asks to expand it; the owner does.
    assert_eq!(
        press_key(&mut tree, id, Key::Right),
        (vec![ActionKind::Expand], true)
    );
    assert_eq!(selected(&tree, id), Some(0));
    expand_src(&mut tree, id);
    tree.frame(WINDOW, &METRICS);
    assert_eq!(
        tree.accessibility().to_string(),
        "tree view \"Explorer\" = \"row 1 of 4\" focusable focused\n\
         \x20 tree item \"src\" selected expanded\n\
         \x20 tree item \"main.rs\"\n\
         \x20 tree item \"lib.rs\"\n\
         \x20 tree item \"Cargo.toml\"\n"
    );
    // Right on an expanded row moves to its first child.
    assert_eq!(press_key(&mut tree, id, Key::Right).0, [ActionKind::Select]);
    assert_eq!(selected(&tree, id), Some(1));
    // Right on a leaf does nothing; Left moves to its parent.
    assert_eq!(press_key(&mut tree, id, Key::Right), (vec![], true));
    assert_eq!(press_key(&mut tree, id, Key::Down).0, [ActionKind::Select]);
    assert_eq!(press_key(&mut tree, id, Key::Left).0, [ActionKind::Select]);
    assert_eq!(selected(&tree, id), Some(0));
    // Left on an expanded row asks to collapse it.
    assert_eq!(
        press_key(&mut tree, id, Key::Left),
        (vec![ActionKind::Collapse], true)
    );
    collapse_src(&mut tree, id);
    tree.frame(WINDOW, &METRICS);
    assert_eq!(names(&tree), ["src", "Cargo.toml"]);
    // Left on a collapsed row at the top, or a leaf without a parent, does
    // nothing.
    assert_eq!(press_key(&mut tree, id, Key::Left), (vec![], true));
    press_key(&mut tree, id, Key::Down);
    assert_eq!(press_key(&mut tree, id, Key::Left), (vec![], true));
    assert_eq!(selected(&tree, id), Some(1));
    // A row's parent that is not a row is not followed.
    widgets::update(&mut tree, id, |view: &mut Collection| {
        if let Some(rows) = view.model_mut::<Vec<Row>>() {
            rows[1] = Row::new("Cargo.toml").parent(99);
        }
    })
    .expect("the explorer");
    assert_eq!(press_key(&mut tree, id, Key::Left), (vec![], true));
    assert_eq!(selected(&tree, id), Some(1));
}

/// A tree view's model: a root and its children, which records the rows
/// it is asked for.
struct Asked {
    len: usize,
    asked: RefCell<Vec<usize>>,
}

impl Rows for Asked {
    fn len(&self) -> usize {
        self.len
    }

    fn row(&self, index: usize) -> Row {
        assert!(index < self.len, "asked for row {index}");
        self.asked.borrow_mut().push(index);
        if index == 0 {
            Row::new("root").expanded(true)
        } else {
            Row::new(format!("child {index}")).depth(1).parent(0)
        }
    }
}

/// The rows the collection's model of [`Asked`] was asked for since the
/// last call, in order.
fn asked_rows(tree: &mut Tree, id: NodeId) -> Vec<usize> {
    widgets::update(tree, id, |view: &mut Collection| {
        view.model_as::<Asked>()
            .map_or_else(Vec::new, |model| model.asked.take())
    })
    .expect("the collection")
}

#[test]
fn a_tree_views_keys_act_on_the_selection_scrolled_away() {
    let model = Asked {
        len: 100_000,
        asked: RefCell::new(Vec::new()),
    };
    let (mut tree, id) = focused(Collection::tree_view("Explorer", model));
    press_key(&mut tree, id, Key::Down);
    tree.frame(WINDOW, &METRICS);
    // The wheel scrolls the selection, the root, far away.
    tree.handle(&wheel(Point::new(10.0, 50.0), 5000.0));
    tree.frame(WINDOW, &METRICS);
    assert!(chosen(&tree).is_none());
    asked_rows(&mut tree, id);
    // Left on it asks to collapse it: the model is asked for it alone, as
    // `Rows` says.
    let (actions, handled) = press_key(&mut tree, id, Key::Left);
    assert_eq!((actions, handled), (vec![ActionKind::Collapse], true));
    assert_eq!(asked_rows(&mut tree, id), [0]);
    // Right moves to its first child, and scrolls them back into view.
    assert_eq!(press_key(&mut tree, id, Key::Right).0, [ActionKind::Select]);
    assert_eq!(asked_rows(&mut tree, id), [0, 1]);
    tree.frame(WINDOW, &METRICS);
    let child = chosen(&tree).expect("the child on screen");
    assert_eq!(child.name, "child 1");
    assert!(inside(child.bounds, view(&tree, id)));
    // Only the rows it shows were asked for at that frame.
    let (first, count) = meeting(100_000, view(&tree, id).height, offset(&tree, id));
    assert_eq!(
        asked_rows(&mut tree, id),
        (first..first + count).collect::<Vec<_>>()
    );
}

#[test]
fn a_press_on_a_tree_item_s_chevron_selects_and_toggles_it() {
    let (mut tree, id) = single(Collection::tree_view("Explorer", explorer()));
    let row = view(&tree, id);
    // The chevron's box: after the row's padding, at its level.
    let chevron = Point::new(row.x + widgets::ROW_PADDING + 4.0, row.y + 10.0);
    let response = click(&mut tree, chevron);
    assert_eq!(
        kinds(&response, id),
        [ActionKind::Select, ActionKind::Expand]
    );
    assert_eq!(tree.focused(), Some(id));
    assert!(!tree.focus_visible(), "a press does not show focus");
    expand_src(&mut tree, id);
    tree.frame(WINDOW, &METRICS);
    let response = tree.handle(&down(chevron));
    assert_eq!(kinds(&response, id), [ActionKind::Collapse]);
    // A press on its text selects it only; a leaf has no chevron.
    let text = Point::new(row.x + 100.0, row.y + 30.0);
    assert_eq!(kinds(&tree.handle(&down(text)), id), [ActionKind::Select]);
    assert_eq!(selected(&tree, id), Some(1));
    let leaf = Point::new(chevron.x + widgets::TREE_INDENT, row.y + 30.0);
    assert!(kinds(&tree.handle(&down(leaf)), id).is_empty());
}

/// A table of problems: severity and message.
fn problems(count: usize) -> Collection {
    let rows = (1..=count)
        .map(|n| Row::cells(["error".to_string(), format!("problem {n}")]))
        .collect::<Vec<Row>>();
    Collection::table(
        "Problems",
        [Column::new("Severity", 80.0), Column::new("Message", 240.0)],
        rows,
    )
}

#[test]
fn a_table_moves_as_a_list_and_its_column_headers_activate() {
    let (mut tree, table) = focused(problems(30));
    assert_eq!(
        press_key(&mut tree, table, Key::Down).0,
        [ActionKind::Select]
    );
    assert_eq!(
        press_key(&mut tree, table, Key::End).0,
        [ActionKind::Select]
    );
    assert_eq!(selected(&tree, table), Some(29));
    assert_eq!(press_key(&mut tree, table, Key::Down), (vec![], true));
    // A page below the header: 276 units, 13 rows.
    tree.frame(WINDOW, &METRICS);
    assert_eq!(view(&tree, table).height, 276.0);
    press_key(&mut tree, table, Key::PageUp);
    assert_eq!(selected(&tree, table), Some(16));
    press_key(&mut tree, table, Key::Home);
    press_key(&mut tree, table, Key::PageDown);
    assert_eq!(selected(&tree, table), Some(13));
    assert_eq!(press_key(&mut tree, table, Key::Left), (vec![], false));
    assert_eq!(
        press_key(&mut tree, table, Key::Enter).0,
        [ActionKind::Activate]
    );

    // Tab reaches each column's header, which activates on Enter and on
    // Space coming up; the owner finds its column.
    let severity = tree.focus_next().expect("the first header");
    assert_eq!(Collection::column_of(&tree, severity), Some(0));
    assert_eq!(
        kinds(&tree.handle(&key(Key::Enter)), severity),
        [ActionKind::Activate]
    );
    tree.handle(&key(Key::Space));
    assert_eq!(
        kinds(&tree.handle(&key_up(Key::Space)), severity),
        [ActionKind::Activate]
    );
    // The arrows on a header are not the table's: its selection stays.
    let response = tree.handle(&key(Key::Down));
    assert!(!response.handled && response.actions.is_empty());
    assert_eq!(selected(&tree, table), Some(13));
    let message = tree.focus_next().expect("the second header");
    assert_eq!(Collection::column_of(&tree, message), Some(1));
    // A click on a header activates it.
    let bounds = tree.bounds(message).expect("bounds");
    let at = Point::new(bounds.x + 5.0, bounds.y + 5.0);
    tree.handle(&down(at));
    assert_eq!(
        kinds(&tree.handle(&up(at)), message),
        [ActionKind::Activate]
    );
    assert_eq!(Collection::column_of(&tree, table), None);
    // The header row and its headers are in the accessibility tree, then
    // the rows, each with its cells.
    tree.frame(WINDOW, &METRICS);
    let snapshot = tree.accessibility().to_string();
    assert!(
        snapshot.starts_with(
            "table \"Problems\" = \"row 14 of 30\" focusable\n\
             \x20 row \"Severity, Message\"\n\
             \x20   column header \"Severity\" focusable\n\
             \x20   column header \"Message\" focusable focused\n\
             \x20 row \"error, problem "
        ),
        "{snapshot}"
    );
    assert!(
        snapshot.contains(
            "\x20 row \"error, problem 14\" selected\n\
             \x20   cell \"error\"\n\
             \x20   cell \"problem 14\"\n"
        ),
        "{snapshot}"
    );
}

#[test]
fn a_table_names_its_rows_by_what_its_columns_show() {
    // One column, and rows of two cells: the second, which no column
    // shows, is in no name.
    let (tree, _) = single(Collection::table(
        "Users",
        [Column::new("Name", 100.0)],
        vec![
            Row::cells(["alice", "hidden id 7"]),
            Row::cells(["bob", "hidden id 8"]),
        ],
    ));
    assert_eq!(names(&tree), ["alice", "bob"]);
    assert!(!tree.accessibility().to_string().contains("hidden"));
    // A column 0 or NaN wide shows nothing: its cell is in no name, and its
    // header is no tab stop, though named.
    let (tree, table) = single(Collection::table(
        "Users",
        [
            Column::new("Name", 100.0),
            Column::new("Id", 0.0),
            Column::new("Key", f32::NAN),
            Column::new("Role", 60.0),
        ],
        vec![Row::cells(["alice", "7", "k", "admin"])],
    ));
    assert_eq!(names(&tree), ["alice, admin"]);
    let stops: Vec<String> = tree
        .tab_order()
        .into_iter()
        .filter(|&id| id != table)
        .filter_map(|id| tree.semantics(id).map(|semantics| semantics.name.clone()))
        .collect();
    assert_eq!(stops, ["Name", "Role"]);
    let snapshot = tree.accessibility().to_string();
    assert!(snapshot.contains("row \"Name, Role\"\n"), "{snapshot}");
    assert!(snapshot.contains("column header \"Id\"\n"), "{snapshot}");
    assert!(!snapshot.contains("cell \"7\""), "{snapshot}");
}

/// A million rows, made as they are asked for, recording the rows asked
/// for.
struct Million {
    asked: RefCell<Vec<usize>>,
}

impl Million {
    const LEN: usize = 1_000_000;

    fn new() -> Self {
        Self {
            asked: RefCell::new(Vec::new()),
        }
    }
}

impl Rows for Million {
    fn len(&self) -> usize {
        Self::LEN
    }

    fn row(&self, index: usize) -> Row {
        assert!(index < Self::LEN, "asked for row {index}");
        self.asked.borrow_mut().push(index);
        Row::cells([format!("line {}", index + 1), "b".into(), "c".into()])
    }
}

/// The rows the million's model was asked for since the last call.
fn asked(tree: &mut Tree, id: NodeId) -> Vec<usize> {
    widgets::update(tree, id, |list: &mut Collection| {
        list.model_as::<Million>()
            .map_or_else(Vec::new, |model| model.asked.take())
    })
    .expect("the list")
}

#[test]
fn a_million_rows_keep_nodes_only_for_the_rows_in_view() {
    let (mut tree, list) = single(Collection::list("Lines", Million::new()));
    // The root, the list, its body and its two gaps, and the 15 rows that
    // meet its 296 units.
    assert_eq!(view(&tree, list).height, 296.0);
    assert_eq!(tree.node_count(), 1 + 4 + 15);
    assert_eq!(asked(&mut tree, list), (0..15).collect::<Vec<_>>());
    assert_eq!(names(&tree).first().map(String::as_str), Some("line 1"));

    // Scrolled to the middle by the owner: those rows, and no others.
    let middle = (Million::LEN / 2) as f64 * f64::from(ROW_HEIGHT);
    widgets::update(&mut tree, list, |list: &mut Collection| {
        list.scroll_to(middle)
    })
    .expect("the list");
    let frame = tree.frame(WINDOW, &METRICS).expect("the rows moved");
    let area = view(&tree, list);
    assert!(frame.damage.iter().all(|rect| inside(*rect, area)));
    assert_eq!(tree.node_count(), 1 + 4 + 15);
    let middle_rows = Million::LEN / 2..Million::LEN / 2 + 15;
    assert_eq!(
        asked(&mut tree, list),
        middle_rows.clone().collect::<Vec<_>>()
    );
    let shown = names(&tree);
    assert_eq!(shown.first().map(String::as_str), Some("line 500001"));
    assert_eq!(shown.len(), 15);

    // To the end with the keyboard: End selects the last row and shows it.
    tree.set_focus(list, true).expect("focus");
    assert_eq!(press_key(&mut tree, list, Key::End).0, [ActionKind::Select]);
    tree.frame(WINDOW, &METRICS).expect("the rows moved");
    assert_eq!(tree.node_count(), 1 + 4 + 15);
    let (first, count) = meeting(Million::LEN, area.height, offset(&tree, list));
    assert_eq!((first + count, count), (Million::LEN, 15));
    assert_eq!(
        asked(&mut tree, list),
        (first..first + count).collect::<Vec<_>>()
    );
    let last = rows(&tree).pop().expect("a row");
    assert_eq!(last.name, "line 1000000");
    assert!(last.selected);
    assert!(inside(last.bounds, area), "{last:?}");
    assert_eq!(
        value(&tree, list).as_deref(),
        Some("row 1000000 of 1000000")
    );
    // An idle frame asks for nothing and draws nothing.
    assert_eq!(tree.frame(WINDOW, &METRICS), None);
    assert!(asked(&mut tree, list).is_empty());

    // Up a page: still the same nodes.
    press_key(&mut tree, list, Key::PageUp);
    tree.frame(WINDOW, &METRICS);
    assert_eq!(tree.node_count(), 1 + 4 + 15);
    assert_eq!(selected(&tree, list), Some(Million::LEN - 1 - 14));
}

#[test]
fn inside_a_scroller_only_the_rows_on_screen_have_nodes() {
    // A default list of a million rows, and a table of 64 columns, each in
    // a column that scrolls, 300 high.
    let columns: Vec<Column> = (0..64)
        .map(|n| Column::new(format!("C{n}"), 10.0))
        .collect();
    for collection in [
        Collection::list("Lines", Million::new()),
        Collection::table("Lines", columns, Million::new()),
    ] {
        let kind = collection.kind();
        let width = 1 + collection.columns().len();
        let mut tree = Tree::new();
        let (column, id) = in_scroller(&mut tree, 0.0, collection.node());
        tree.frame(WINDOW, &METRICS);
        // It measures at most MAX_VIEW_ROWS rows high, and its rows reach
        // as far, so it never grows from one layout to the next.
        let header = if kind == CollectionKind::Table {
            ROW_HEIGHT
        } else {
            0.0
        };
        let bounds = tree.bounds(id).expect("bounds");
        assert_eq!(
            bounds.height,
            header + MAX_VIEW_ROWS as f32 * ROW_HEIGHT + 2.0 * FOCUS_WIDTH
        );
        // The rows that meet the screen, from the body's top 2 or 22 units
        // down to the window's bottom, and the margin below them.
        let on_screen = 300.0 - view(&tree, id).y;
        let rows = (on_screen / ROW_HEIGHT).ceil() as usize + ROW_MARGIN;
        let header_nodes = if kind == CollectionKind::Table {
            width
        } else {
            0
        };
        // The root, the column and its two fillers, the collection's four
        // parts, its header and its rows.
        let nodes = 1 + 3 + 4 + header_nodes + rows * width;
        assert_eq!(tree.node_count(), nodes, "{kind:?}");
        assert_eq!(
            asked(&mut tree, id),
            (0..rows).collect::<Vec<_>>(),
            "{kind:?}"
        );
        // An idle frame asks for no row, and passes no layout.
        let passes = count_passes(&mut tree);
        tree.frame(WINDOW, &METRICS);
        passes.set(0);
        assert_eq!(tree.frame(WINDOW, &METRICS), None);
        assert_eq!(passes.get(), 0);
        assert!(asked(&mut tree, id).is_empty());
        // The tree has room: an insert elsewhere succeeds.
        assert!(tree.insert(tree.root(), Node::default()).is_ok());
        // The column scrolled a long way: the rows now on screen, and the
        // margin either side, are the only ones kept and asked for.
        tree.set_scroll_offset(column, Point::new(0.0, 5000.0))
            .expect("the column");
        tree.frame(WINDOW, &METRICS);
        let area = view(&tree, id);
        let band = (f64::from(-area.y), f64::from(300.0 - area.y));
        let real = f64::from(area.height);
        let (first, count) = kept(Million::LEN, real, real, 0.0, band);
        assert_eq!(count, 16 + 2 * ROW_MARGIN, "{kind:?}");
        assert_eq!(
            asked(&mut tree, id),
            (first..first + count).collect::<Vec<_>>()
        );
        assert_eq!(tree.node_count(), nodes + 2 + (count - rows) * width);
        // Down selects the first row, far above: the column scrolls back to
        // show it, and the rows kept are those on screen again.
        tree.set_focus(id, true).expect("focus");
        press_key(&mut tree, id, Key::Down);
        tree.frame(WINDOW, &METRICS);
        let row = chosen(&tree).expect("the first row");
        assert!(
            inside(row.bounds, Rect::new(0.0, 0.0, 400.0, 300.0)),
            "{row:?}"
        );
        assert!(tree.node_count() <= nodes + 2 + 2 * ROW_MARGIN * width);
        assert_eq!(tree.frame(WINDOW, &METRICS), None);
    }
}

#[test]
fn a_collection_keeps_at_most_max_view_rows_and_two_however_tall() {
    // A window a million units high, and a 64-column table filling it: its
    // rows show in at most MAX_VIEW_ROWS rows' height, so the tree keeps
    // room for every other node.
    let columns: Vec<Column> = (0..64)
        .map(|n| Column::new(format!("C{n}"), 10.0))
        .collect();
    let tall = Size::new(400.0, 1_000_000.0);
    let mut tree = Tree::new();
    let id = tree
        .insert(
            tree.root(),
            Collection::table("Lines", columns, Million::new()).node(),
        )
        .expect("the table");
    tree.frame(tall, &METRICS);
    let kept_rows = rows(&tree).len();
    assert_eq!(kept_rows, MAX_VIEW_ROWS, "the rows that fill its view");
    assert!(tree.node_count() <= 1 + 4 + 65 + (MAX_VIEW_ROWS + 2) * 65);
    assert_eq!(asked(&mut tree, id).len(), MAX_VIEW_ROWS);
    assert!(tree.insert(tree.root(), Node::default()).is_ok());
    // Nothing is drawn below its view: the body's lower gap holds the rest.
    let last = rows(&tree).pop().expect("a row");
    assert!(last.bounds.bottom() <= view(&tree, id).y + MAX_VIEW_ROWS as f32 * ROW_HEIGHT);
}

#[test]
fn in_a_scroller_a_press_selects_the_row_drawn_under_it_and_idle_frames_pass_nothing() {
    let mut tree = Tree::new();
    let (_, id) = in_scroller(
        &mut tree,
        0.0,
        Collection::list("Lines", Million::new()).node(),
    );
    let passes = count_passes(&mut tree);
    tree.frame(WINDOW, &METRICS);
    // A wheel of a line and a half scrolls the list by 30.
    assert!(tree.handle(&wheel(Point::new(10.0, 50.0), 1.5)).handled);
    tree.frame(WINDOW, &METRICS);
    assert_eq!(offset(&tree, id), 30.0);
    passes.set(0);
    for _ in 0..3 {
        assert_eq!(tree.frame(WINDOW, &METRICS), None);
    }
    assert_eq!(passes.get(), 0, "idle frames passed layout");
    // Each row's text is drawn where the offset puts its row, the first
    // cut by 10 at the top: on the baseline that centres a line of the UI
    // body in the whole row.
    let line = METRICS.line(&widgets::ROW_TEXT);
    let centred = (ROW_HEIGHT - (line.ascent + line.descent)) / 2.0 + line.ascent;
    let top = view(&tree, id).y;
    let commands = painted(&mut tree);
    for (k, row) in rows(&tree).iter().enumerate() {
        let index = 1 + k;
        let baseline = top + index as f32 * ROW_HEIGHT - 30.0 + centred;
        let drawn = commands.iter().find_map(|command| match command {
            Command::Text { origin, text, .. } if *text == row.name => Some(origin.y),
            _ => None,
        });
        let drawn = drawn.expect("its text");
        assert!(
            (drawn - baseline).abs() <= SLACK,
            "{}: {drawn} for {baseline}",
            row.name
        );
    }
    // A press selects the row drawn where it lands.
    for y in [3.0, 30.0, 50.0, 77.0, 299.0] {
        let at = Point::new(10.0, y);
        let under = rows(&tree)
            .into_iter()
            .find(|node| node.bounds.contains(at))
            .map(|node| node.name)
            .expect("a row there");
        click(&mut tree, at);
        tree.frame(WINDOW, &METRICS);
        let picked = chosen(&tree).map(|node| node.name);
        assert_eq!(picked.as_deref(), Some(under.as_str()), "at {y}");
    }
}

#[test]
fn every_row_on_screen_has_its_role_and_a_name() {
    let mut rows_with_a_blank = labels(30);
    rows_with_a_blank[2] = Row::new("  ");
    let tree_rows: Vec<Row> = (0..30)
        .map(|n| Row::new(format!("item {n}")).depth(n % 3))
        .collect();
    let mut table_rows: Vec<Row> = (0..30)
        .map(|n| Row::cells(["warning".to_string(), format!("unused {n}")]))
        .collect();
    table_rows[1] = Row::cells(["", " "]);
    let cases = [
        (
            Collection::list("Files", rows_with_a_blank),
            Role::ListItem,
            "row 3",
        ),
        (
            Collection::tree_view("Outline", tree_rows),
            Role::TreeItem,
            "item 2",
        ),
        (
            Collection::table(
                "Problems",
                [Column::new("Severity", 80.0), Column::new("Message", 240.0)],
                table_rows,
            ),
            Role::Row,
            "row 2",
        ),
    ];
    for (collection, role, nameless) in cases {
        let (mut tree, id) = single(collection);
        // Scrolled a little, so that rows are cut at both edges.
        tree.handle(&wheel(Point::new(10.0, 100.0), 2.5));
        tree.frame(WINDOW, &METRICS);
        let shown = rows(&tree);
        let area = view(&tree, id);
        for row in &shown {
            assert_eq!(row.role, role, "{row:?}");
            assert!(!row.name.trim().is_empty(), "{row:?}");
            assert_eq!(row.depth, 1, "a row is the collection's: {row:?}");
            // Its bounds are the part of it in the view.
            assert!(inside(row.bounds, area), "{row:?} is not in {area:?}");
        }
        // Every row that meets the view is there, and no other.
        let (first, count) = meeting(30, area.height, offset(&tree, id));
        assert_eq!(shown.len(), count, "{role:?}");
        assert_eq!(first, 2);
        // Scroll back to the top: the blank row is named by its place.
        tree.handle(&wheel(Point::new(10.0, 100.0), -10.0));
        tree.frame(WINDOW, &METRICS);
        assert!(names(&tree).iter().any(|name| name == nameless), "{role:?}");
        for node in tree.accessibility().nodes() {
            if node.focusable {
                assert!(!node.name.trim().is_empty(), "{node:?}");
            }
        }
    }
}

#[test]
fn the_selection_scrolls_into_view_the_least() {
    for collection in [
        Collection::list("Files", labels(100)),
        Collection::tree_view("Explorer", labels(100)),
        problems(100),
    ] {
        let kind = collection.kind();
        let (mut tree, id) = focused(collection);
        let height = f64::from(view(&tree, id).height);
        let page = (height / f64::from(ROW_HEIGHT)).floor() as usize;
        // The offset the tree's own `scroll_into_view` would give: the
        // least move that brings the row into the view.
        let least = |before: f64, row: usize| {
            let top = row as f64 * f64::from(ROW_HEIGHT);
            if top < before {
                top
            } else if top + f64::from(ROW_HEIGHT) > before + height {
                top + f64::from(ROW_HEIGHT) - height
            } else {
                before
            }
        };
        let check = |tree: &mut Tree, row: usize| {
            let before = offset(tree, id);
            tree.frame(WINDOW, &METRICS);
            assert_eq!(selected(tree, id), Some(row), "{kind:?}");
            assert_eq!(offset(tree, id), least(before, row), "{kind:?}: row {row}");
            let shown = chosen(tree).expect("the selection shows");
            assert!(
                inside(shown.bounds, view(tree, id)),
                "{kind:?}: row {row} at {:?}",
                shown.bounds
            );
        };
        // By the keyboard, down and back.
        press_key(&mut tree, id, Key::End);
        check(&mut tree, 99);
        press_key(&mut tree, id, Key::PageUp);
        check(&mut tree, 99 - page);
        press_key(&mut tree, id, Key::Home);
        check(&mut tree, 0);
        for _ in 0..30 {
            press_key(&mut tree, id, Key::Down);
        }
        check(&mut tree, 30);
        // By the owner, after the wheel scrolled it away.
        tree.handle(&wheel(Point::new(10.0, 100.0), 40.0));
        tree.frame(WINDOW, &METRICS);
        widgets::update(&mut tree, id, |view: &mut Collection| view.select(Some(70)))
            .expect("select");
        check(&mut tree, 70);
        // The selected row scrolled out by the wheel comes back on a key.
        tree.handle(&wheel(Point::new(10.0, 100.0), -40.0));
        tree.frame(WINDOW, &METRICS);
        assert!(chosen(&tree).is_none());
        press_key(&mut tree, id, Key::Down);
        check(&mut tree, 71);
        // A press on a row cut by the view's edge brings it in whole.
        let area = view(&tree, id);
        let cut = rows(&tree).into_iter().find(|node| {
            node.bounds.bottom() >= area.bottom() - SLACK && node.bounds.height < ROW_HEIGHT
        });
        if let Some(cut) = cut {
            let at = Point::new(area.x + 50.0, area.bottom() - 1.0);
            click(&mut tree, at);
            tree.frame(WINDOW, &METRICS);
            let now = chosen(&tree).expect("the row pressed");
            assert_eq!(now.name, cut.name);
            assert!(
                now.bounds.height == ROW_HEIGHT && inside(now.bounds, view(&tree, id)),
                "{now:?}"
            );
        }
    }
}

#[test]
fn a_selection_asked_for_before_a_layout_scrolls_in_that_layout_s_view() {
    // Before the first frame: row 10 is in a view of 296, so nothing moves.
    let mut list = Collection::list("Files", labels(100));
    list.select(Some(10));
    let mut tree = Tree::new();
    let id = tree.insert(tree.root(), list.node()).expect("the list");
    tree.frame(WINDOW, &METRICS);
    assert_eq!(offset(&tree, id), 0.0);
    assert_eq!(names(&tree).first().map(String::as_str), Some("name 1"));
    // The view grows from 96 to 496 in the turn row 20 is selected: it is
    // in the new view, so nothing moves.
    let mut tree = Tree::new();
    let id = tree
        .insert(tree.root(), Collection::list("Files", labels(100)).node())
        .expect("the list");
    tree.frame(Size::new(400.0, 100.0), &METRICS);
    widgets::update(&mut tree, id, |view: &mut Collection| view.select(Some(20))).expect("select");
    tree.frame(Size::new(400.0, 500.0), &METRICS);
    assert_eq!(offset(&tree, id), 0.0);
    // It shrinks in the turn the last row is selected: the row is brought
    // to the bottom of the smaller view.
    tree.set_focus(id, true).expect("focus");
    press_key(&mut tree, id, Key::End);
    tree.frame(Size::new(400.0, 100.0), &METRICS);
    assert_eq!(offset(&tree, id), 100.0 * 20.0 - 96.0);
    let last = chosen(&tree).expect("the last row");
    assert!(last.name == "name 100" && inside(last.bounds, view(&tree, id)));
}

#[test]
fn the_selection_is_scrolled_on_screen_by_the_scrollers_around() {
    // A default list of 100 in a column that scrolls: it is as tall as its
    // rows, so End scrolls the column, not the list.
    let mut tree = Tree::new();
    let (column, id) = in_scroller(
        &mut tree,
        0.0,
        Collection::list("Files", labels(100)).node(),
    );
    tree.frame(WINDOW, &METRICS);
    tree.set_focus(id, true).expect("focus");
    press_key(&mut tree, id, Key::End);
    tree.frame(WINDOW, &METRICS);
    let last = chosen(&tree).expect("the last row");
    assert_eq!(last.name, "name 100");
    assert!(
        inside(last.bounds, Rect::new(0.0, 0.0, 400.0, 300.0)),
        "{last:?}"
    );
    assert_eq!(offset(&tree, id), 0.0);
    // The least scroll: the row's bottom at the window's.
    let scrolled = tree.scroll_offset(column).expect("the column").y;
    assert_eq!(last.bounds.bottom(), 300.0, "scrolled {scrolled}");
    // A list 200 high below 250 units of other content: six Downs scroll
    // the column to show the sixth row.
    let mut tree = Tree::new();
    let list = styled(
        Collection::list("Files", labels(100)).node(),
        Style::column()
            .width(Length::Fill(1.0))
            .height(Length::Fixed(200.0)),
    );
    let (column, id) = in_scroller(&mut tree, 250.0, list);
    tree.frame(WINDOW, &METRICS);
    tree.set_focus(id, true).expect("focus");
    for _ in 0..6 {
        tree.handle(&key(Key::Down));
    }
    tree.frame(WINDOW, &METRICS);
    let sixth = chosen(&tree).expect("the sixth row");
    assert_eq!(sixth.name, "name 6");
    assert!(
        inside(sixth.bounds, Rect::new(0.0, 0.0, 400.0, 300.0)),
        "{sixth:?}"
    );
    assert_eq!(sixth.bounds.bottom(), 300.0);
    assert_eq!(offset(&tree, id), 0.0);
    assert!(tree.scroll_offset(column).is_some_and(|at| at.y > 0.0));
    // An idle frame after it passes nothing.
    assert_eq!(tree.frame(WINDOW, &METRICS), None);
}

#[test]
fn two_collections_and_the_owner_each_reveal_what_they_asked_for_in_one_turn() {
    // Three columns that scroll: a list in each of the first two, and in
    // the third a node below 1000 units of other content. Both lists
    // select a row far down, and the owner asks for that node, in one turn.
    let mut tree = Tree::new();
    let root = tree.root();
    tree.set_style(root, Style::row()).expect("the root");
    let column = || {
        Node::new(
            Style::column()
                .width(Length::Fixed(130.0))
                .height(Length::Fixed(300.0))
                .scroll(false, true),
        )
    };
    let columns = [(); 3].map(|()| tree.insert(root, column()).expect("a column"));
    let lists = [("Files", columns[0]), ("Symbols", columns[1])].map(|(name, into)| {
        tree.insert(into, Collection::list(name, labels(100)).node())
            .expect("a list")
    });
    let filler = Node::new(
        Style::default()
            .width(Length::Fill(1.0))
            .height(Length::Fixed(1000.0)),
    );
    tree.insert(columns[2], filler).expect("above");
    let asked = Node::new(
        Style::default()
            .width(Length::Fill(1.0))
            .height(Length::Fixed(20.0)),
    );
    let asked = tree.insert(columns[2], asked).expect("the owner's node");
    tree.frame(WINDOW, &METRICS);
    for list in lists {
        widgets::update(&mut tree, list, |list: &mut Collection| {
            list.select(Some(80))
        })
        .expect("the list");
    }
    tree.scroll_into_view(asked).expect("the owner's node");
    tree.frame(WINDOW, &METRICS);
    let shown: Vec<AccessNode> = rows(&tree)
        .into_iter()
        .filter(|node| node.selected)
        .collect();
    assert_eq!(shown.len(), 2, "{shown:?}");
    for (row, &column) in shown.iter().zip(&columns) {
        assert_eq!(row.name, "name 81");
        let area = tree.bounds(column).expect("bounds");
        assert!(inside(row.bounds, area), "{:?} in {area:?}", row.bounds);
    }
    let area = tree.bounds(columns[2]).expect("bounds");
    let at = tree.bounds(asked).expect("bounds");
    assert!(inside(at, area), "{at:?} in {area:?}");
    assert_eq!(tree.frame(WINDOW, &METRICS), None);
}

#[test]
fn an_idle_frame_is_none_and_scrolling_damages_only_the_collection() {
    let mut tree = Tree::new();
    let root = tree.root();
    tree.set_style(root, Style::row()).expect("the root");
    let side = styled(
        Collection::list("Files", labels(200)).node(),
        Style::column()
            .width(Length::Fixed(200.0))
            .height(Length::Fill(1.0)),
    );
    let list = tree.insert(root, side).expect("the list");
    let other = tree
        .insert(root, Collection::list("Other", labels(5)).node())
        .expect("another list");
    assert!(tree.frame(WINDOW, &METRICS).is_some());
    assert_eq!(tree.frame(WINDOW, &METRICS), None);
    let area = view(&tree, list);
    assert_eq!(area, Rect::new(0.0, 0.0, 200.0, 300.0));
    let elsewhere = tree.bounds(other).expect("bounds");
    let untouched = |damage: &[Rect]| damage.iter().all(|rect| meet(*rect, elsewhere).is_empty());
    let at = Point::new(50.0, 50.0);

    // The wheel scrolls the list by a line: the rows it moves, inside it.
    assert!(tree.handle(&wheel(at, 1.0)).handled);
    let frame = tree.frame(WINDOW, &METRICS).expect("the rows moved");
    assert!(!frame.damage.is_empty());
    for rect in &frame.damage {
        assert!(inside(*rect, area), "{rect:?}");
    }
    assert!(untouched(&frame.damage));
    assert_eq!(tree.frame(WINDOW, &METRICS), None);
    // By part of a row too.
    let logical = InputEvent::Wheel {
        delta: ScrollDelta::Logical { x: 0.0, y: 7.5 },
        position: at,
        modifiers: Modifiers::NONE,
    };
    assert!(tree.handle(&logical).handled);
    let frame = tree.frame(WINDOW, &METRICS).expect("the rows moved");
    assert!(frame.damage.iter().all(|rect| inside(*rect, area)));
    assert!(untouched(&frame.damage));
    assert_eq!(offset(&tree, list), 27.5);
    assert_eq!(tree.frame(WINDOW, &METRICS), None);

    // The wheel left the pointer over a row, which keeps its wash as the
    // rows scroll under it. Moving to another row damages the two, each
    // where it painted before (its text) and now.
    tree.handle(&hover(Point::new(50.0, 150.0)));
    let frame = tree.frame(WINDOW, &METRICS).expect("the hovered rows");
    assert_eq!(bands(&frame.damage, area), 2, "{:?}", frame.damage);
    assert_eq!(tree.frame(WINDOW, &METRICS), None);
    // Leaving the list damages the row it left only.
    tree.handle(&hover(Point::new(250.0, 150.0)));
    let frame = tree.frame(WINDOW, &METRICS).expect("the row left");
    let left: Vec<Rect> = frame
        .damage
        .iter()
        .copied()
        .filter(|rect| meet(*rect, elsewhere).is_empty())
        .collect();
    assert_eq!(bands(&left, area), 1, "{:?}", frame.damage);

    // Moving the selection damages the two rows it moves between, and not
    // the other list.
    tree.handle(&hover(Point::new(1.0, 1.0)));
    tree.frame(WINDOW, &METRICS);
    tree.set_focus(list, false).expect("focus");
    tree.handle(&key(Key::Down));
    tree.frame(WINDOW, &METRICS);
    tree.handle(&key(Key::Down));
    let frame = tree.frame(WINDOW, &METRICS).expect("the selection moved");
    assert_eq!(bands(&frame.damage, area), 2, "{:?}", frame.damage);
    assert!(untouched(&frame.damage));
    assert_eq!(tree.frame(WINDOW, &METRICS), None);
}

#[test]
fn the_wheel_scrolls_the_rows_and_passes_on_what_they_cannot_take() {
    // A list of 30 rows 100 high, and a default list of 1,000, each in a
    // column that scrolls.
    for fixed in [true, false] {
        let mut tree = Tree::new();
        let rows = if fixed { 30 } else { 1000 };
        let list = Collection::list("Files", labels(rows)).node();
        let list = if fixed {
            styled(
                list,
                Style::column()
                    .width(Length::Fill(1.0))
                    .height(Length::Fixed(100.0)),
            )
        } else {
            list
        };
        let (column, id) = in_scroller(&mut tree, 0.0, list);
        tree.frame(WINDOW, &METRICS);
        let at = Point::new(10.0, 50.0);
        let height = f64::from(view(&tree, id).height);
        let max = rows as f64 * 20.0 - height;
        // Up at the top: neither the list nor the column can take it.
        assert!(!tree.handle(&wheel(at, -1.0)).handled);
        // Down: the list takes it all until its last row shows.
        let mut expected = 0.0;
        while expected < max {
            assert!(tree.handle(&wheel(at, 1.0)).handled);
            tree.frame(WINDOW, &METRICS);
            expected = (expected + 20.0).min(max);
            assert_eq!(offset(&tree, id), expected, "fixed {fixed}");
            assert_eq!(tree.scroll_offset(column), Some(Point::ZERO));
        }
        if fixed {
            assert_eq!(names(&tree).last().map(String::as_str), Some("name 30"));
        }
        // Then it goes on to the column.
        assert!(tree.handle(&wheel(at, 1.0)).handled);
        tree.frame(WINDOW, &METRICS);
        assert_eq!(offset(&tree, id), max);
        assert_eq!(tree.scroll_offset(column), Some(Point::new(0.0, 20.0)));
    }
}

#[test]
fn the_wheel_over_a_disabled_collection_goes_on_to_the_scrollers_around() {
    let mut tree = Tree::new();
    let list = styled(
        Collection::list("Files", labels(30)).node(),
        Style::column()
            .width(Length::Fill(1.0))
            .height(Length::Fixed(100.0)),
    );
    let (column, id) = in_scroller(&mut tree, 0.0, list);
    tree.frame(WINDOW, &METRICS);
    tree.set_enabled(id, false).expect("disabled");
    tree.frame(WINDOW, &METRICS);
    assert!(tree.handle(&wheel(Point::new(10.0, 50.0), 1.0)).handled);
    tree.frame(WINDOW, &METRICS);
    assert_eq!(offset(&tree, id), 0.0);
    assert_eq!(tree.scroll_offset(column), Some(Point::new(0.0, 20.0)));
    assert_eq!(tree.scroll_offset(body(&tree, id)), Some(Point::ZERO));
}

#[test]
fn changes_to_the_model_keep_the_selection_on_a_row() {
    let (mut tree, list) = focused(Collection::list("Files", labels(10)));
    let edit = |tree: &mut Tree, change: &dyn Fn(&mut Collection)| {
        widgets::update(tree, list, |view: &mut Collection| change(view)).expect("the list");
        tree.frame(WINDOW, &METRICS);
    };
    press_key(&mut tree, list, Key::End);
    press_key(&mut tree, list, Key::Up);
    assert_eq!(selected(&tree, list), Some(8));
    // Rows added before it: it moves with its row.
    edit(&mut tree, &|view| {
        if let Some(rows) = view.model_mut::<Vec<Row>>() {
            rows.splice(0..0, [Row::new("new 1"), Row::new("new 2")]);
        }
        view.rows_inserted(0, 2);
    });
    assert_eq!(selected(&tree, list), Some(10));
    let marked: Vec<String> = rows(&tree)
        .into_iter()
        .filter(|node| node.selected)
        .map(|node| node.name)
        .collect();
    assert_eq!(marked, ["name 9"]);
    // Its row removed: the row now in its place.
    edit(&mut tree, &|view| {
        if let Some(rows) = view.model_mut::<Vec<Row>>() {
            rows.remove(10);
        }
        view.rows_removed(10..11);
    });
    assert_eq!(selected(&tree, list), Some(10));
    assert!(chosen(&tree).is_some_and(|node| node.name == "name 10"));
    // Rows removed before it: it moves up with its row, which is not the
    // last.
    edit(&mut tree, &|view| {
        if let Some(rows) = view.model_mut::<Vec<Row>>() {
            rows.extend([Row::new("end 1"), Row::new("end 2")]);
        }
        view.rows_inserted(11, 2);
    });
    assert_eq!(selected(&tree, list), Some(10));
    edit(&mut tree, &|view| {
        if let Some(rows) = view.model_mut::<Vec<Row>>() {
            rows.drain(0..2);
        }
        view.rows_removed(0..2);
    });
    assert_eq!(selected(&tree, list), Some(8));
    assert!(chosen(&tree).is_some_and(|node| node.name == "name 10"));
    // Rows removed after it leave it where it is.
    edit(&mut tree, &|view| {
        if let Some(rows) = view.model_mut::<Vec<Row>>() {
            rows.truncate(9);
        }
        view.rows_removed(9..100);
    });
    assert_eq!(selected(&tree, list), Some(8));
    edit(&mut tree, &|view| {
        if let Some(rows) = view.model_mut::<Vec<Row>>() {
            rows.splice(0..0, [Row::new("new 1"), Row::new("new 2")]);
        }
        view.rows_inserted(0, 2);
    });
    // Rows taken from the end without a word: cut to the last row.
    edit(&mut tree, &|view| {
        if let Some(rows) = view.model_mut::<Vec<Row>>() {
            rows.truncate(4);
        }
    });
    assert_eq!(selected(&tree, list), Some(3));
    assert_eq!(names(&tree), ["new 1", "new 2", "name 1", "name 2"]);
    assert_eq!(value(&tree, list).as_deref(), Some("row 4 of 4"));
    // Every row removed: none.
    edit(&mut tree, &|view| {
        if let Some(rows) = view.model_mut::<Vec<Row>>() {
            rows.clear();
        }
        view.rows_removed(0..4);
    });
    assert_eq!(selected(&tree, list), None);
    assert!(rows(&tree).is_empty());
    assert_eq!(tree.node_count(), 1 + 4);
    // A new model: nothing selected, at its start.
    edit(&mut tree, &|view| view.set_model(labels(3)));
    assert_eq!(selected(&tree, list), None);
    assert_eq!(names(&tree), ["name 1", "name 2", "name 3"]);
    assert_eq!(value(&tree, list).as_deref(), Some("3 rows"));
    // The owner's selection is cut to the rows there are.
    edit(&mut tree, &|view| view.select(Some(99)));
    assert_eq!(selected(&tree, list), Some(2));
}

#[test]
fn a_collection_put_in_another_s_place_builds_its_own_parts() {
    // A table's columns replaced: three headers, and three cells a row.
    let columns = [Column::new("Severity", 80.0), Column::new("Message", 240.0)];
    let rows_of = || vec![Row::cells(["error", "mismatched types", "src/main.rs"])];
    let (mut tree, id) = single(Collection::table("Problems", columns, rows_of()));
    assert_eq!(names(&tree), ["error, mismatched types"]);
    widgets::update(&mut tree, id, |view: &mut Collection| {
        *view = Collection::table(
            "Problems",
            [
                Column::new("Severity", 80.0),
                Column::new("Message", 200.0),
                Column::new("File", 100.0),
            ],
            rows_of(),
        );
    })
    .expect("the table");
    tree.frame(WINDOW, &METRICS);
    assert_eq!(
        tree.accessibility().to_string(),
        "table \"Problems\" = \"1 row\" focusable\n\
         \x20 row \"Severity, Message, File\"\n\
         \x20   column header \"Severity\" focusable\n\
         \x20   column header \"Message\" focusable\n\
         \x20   column header \"File\" focusable\n\
         \x20 row \"error, mismatched types, src/main.rs\"\n\
         \x20   cell \"error\"\n\
         \x20   cell \"mismatched types\"\n\
         \x20   cell \"src/main.rs\"\n"
    );
    assert_eq!(tree.node_count(), 1 + 4 + 4 + 4);
    // A list made a table: its rows have cells, which paint their text.
    let (mut tree, id) = single(Collection::list("Files", labels(2)));
    widgets::update(&mut tree, id, |view: &mut Collection| {
        *view = Collection::table("Files", [Column::new("Name", 100.0)], labels(2));
    })
    .expect("the list");
    tree.frame(WINDOW, &METRICS);
    let snapshot = tree.accessibility().to_string();
    assert!(
        snapshot.ends_with(
            "\x20 row \"name 1\"\n\x20   cell \"name 1\"\n\x20 row \"name 2\"\n\x20   cell \"name 2\"\n"
        ),
        "{snapshot}"
    );
    let commands = painted(&mut tree);
    assert!(commands.iter().any(|command| matches!(command,
        Command::Text { text, .. } if text == "name 2")));
    // And a table made a list: no header and no cells are left.
    widgets::update(&mut tree, id, |view: &mut Collection| {
        *view = Collection::list("Files", labels(2));
    })
    .expect("the table");
    tree.frame(WINDOW, &METRICS);
    assert_eq!(
        tree.accessibility().to_string(),
        "list \"Files\" = \"2 rows\" focusable\n  list item \"name 1\"\n  list item \"name 2\"\n"
    );
}

#[test]
fn a_hidden_collection_keeps_its_row_nodes_and_shows_them_again() {
    // Hidden and shown again, with nothing changed between.
    let (mut tree, list) = single(Collection::list("Files", labels(50)));
    let kept = |tree: &Tree| -> Vec<NodeId> {
        tree.children(body(tree, list))
            .map(<[_]>::to_vec)
            .expect("rows")
    };
    let first = kept(&tree);
    let style = *tree.style(list).expect("a style");
    tree.set_style(list, style.hidden()).expect("hidden");
    tree.frame(WINDOW, &METRICS);
    tree.set_style(list, style).expect("shown");
    tree.frame(WINDOW, &METRICS);
    assert_eq!(kept(&tree), first, "the same nodes");
    assert_eq!(names(&tree).len(), 15);
    // Again, with a row selected while it is hidden.
    let before: Vec<NodeId> = tree
        .children(body(&tree, list))
        .map(<[_]>::to_vec)
        .expect("rows");
    let count = tree.node_count();
    let style = *tree.style(list).expect("a style");
    tree.set_style(list, style.hidden()).expect("hidden");
    tree.frame(WINDOW, &METRICS);
    assert_eq!(tree.node_count(), count);
    assert!(rows(&tree).is_empty());
    // Row 30 selected while it is hidden, as an owner reveals the active
    // file.
    widgets::update(&mut tree, list, |view: &mut Collection| {
        view.select(Some(30))
    })
    .expect("select");
    tree.set_style(list, style).expect("shown");
    let passes = count_passes(&mut tree);
    tree.frame(WINDOW, &METRICS);
    let after: Vec<NodeId> = tree
        .children(body(&tree, list))
        .map(<[_]>::to_vec)
        .expect("rows");
    assert_eq!(before, after, "the same nodes");
    assert!(passes.get() <= 3, "{} passes", passes.get());
    // The least scroll in the view it has once shown.
    assert_eq!(view(&tree, list).height, 296.0);
    assert_eq!(offset(&tree, list), 31.0 * 20.0 - 296.0);
    // Removed, it takes its rows with it.
    tree.remove(list).expect("removed");
    assert_eq!(tree.node_count(), 2);
    assert!(tree.frame(WINDOW, &METRICS).is_some());
    assert_eq!(tree.frame(WINDOW, &METRICS), None);
}

#[test]
fn a_row_shows_and_names_at_most_max_cell_bytes_of_its_text() {
    // Ten megabytes of clusters of three bytes, an `e` and an accent: the
    // cap falls inside one.
    let long = "e\u{301}".repeat(10 * 1024 * 1024 / 3);
    for collection in [
        Collection::list("Lines", vec![Row::new(long.clone())]),
        Collection::table(
            "Lines",
            [Column::new("Line", 300.0)],
            vec![Row::cells([long.clone()])],
        ),
    ] {
        let (mut tree, _) = single(collection);
        let name = names(&tree).pop().expect("a row");
        assert!(name.len() <= MAX_CELL_BYTES && !name.is_empty());
        assert!(grapheme::is_boundary(&long, name.len()));
        assert!(long.starts_with(&name));
        let commands = painted(&mut tree);
        let shown: Vec<&String> = commands
            .iter()
            .filter_map(|command| match command {
                Command::Text { text, .. } => Some(text),
                _ => None,
            })
            .filter(|text| text.starts_with('e'))
            .collect();
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].len(), name.len());
        assert!(grapheme::is_boundary(&long, shown[0].len()));
    }
}

/// The colors the commands fill or stroke, in order.
fn colors(commands: &[Command]) -> Vec<Color> {
    commands
        .iter()
        .filter_map(|command| match *command {
            Command::Rect { color, .. }
            | Command::CutCornerRect { color, .. }
            | Command::Pill { color, .. }
            | Command::Border { color, .. }
            | Command::Brackets { color, .. }
            | Command::Line { color, .. }
            | Command::Text { color, .. }
            | Command::Icon { color, .. } => Some(color),
            _ => None,
        })
        .collect()
}

/// The commands a whole frame lists now.
fn painted(tree: &mut Tree) -> Vec<Command> {
    tree.invalidate_all();
    tree.frame(WINDOW, &METRICS)
        .expect("a frame")
        .list
        .commands()
        .to_vec()
}

/// The state a collection's first row is put in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum In {
    Rest,
    Hover,
    Selected,
    /// Selected and hovered: the selection shows.
    SelectedHover,
    /// Disabled, hovered and pressed: neither shows.
    Disabled,
}

/// The colors a one-row `collection` paints with its row in `state`.
fn looks(collection: Collection, state: In) -> Vec<Color> {
    let (mut tree, id) = single(collection);
    let row = view(&tree, id);
    let at = Point::new(row.x + 150.0, row.y + 10.0);
    match state {
        In::Rest => {}
        In::Hover => {
            tree.handle(&hover(at));
        }
        In::Selected | In::SelectedHover => {
            widgets::update(&mut tree, id, |view: &mut Collection| view.select(Some(0)))
                .expect("select");
            if state == In::SelectedHover {
                tree.handle(&hover(at));
            }
        }
        In::Disabled => {
            tree.set_enabled(id, false).expect("disabled");
            tree.handle(&hover(at));
            tree.handle(&down(at));
        }
    }
    let commands = painted(&mut tree);
    if state == In::Disabled {
        let layers = commands
            .iter()
            .filter(|command| {
                **command
                    == Command::PushLayer {
                        opacity: DISABLED_OPACITY,
                    }
            })
            .count();
        assert!(layers > 0, "{commands:?}");
    }
    colors(&commands)
}

#[test]
fn each_collection_paints_each_state_in_the_theme_s_tokens() {
    use Token::*;
    let c = |tokens: &[Token]| tokens.iter().map(|&token| color(token)).collect::<Vec<_>>();
    let list = || Collection::list("Files", vec![Row::new("main.rs")]);
    let folder = || Collection::tree_view("Explorer", vec![Row::new("src").expanded(false)]);
    let table = || {
        Collection::table(
            "Problems",
            [Column::new("Severity", 80.0), Column::new("Message", 240.0)],
            vec![Row::cells(["error", "mismatched types"])],
        )
    };
    // The header: its fill and underline, then each title.
    let header = [TableHead, Teal, Teal, Teal];
    let cases: Vec<(&str, Collection, In, Vec<Color>)> = vec![
        ("list", list(), In::Rest, c(&[Text])),
        ("list", list(), In::Hover, c(&[RowHover, Text])),
        ("list", list(), In::Selected, c(&[TealGlow, Text])),
        ("list", list(), In::SelectedHover, c(&[TealGlow, Text])),
        ("list", list(), In::Disabled, c(&[Text])),
        // A chevron's two strokes, then the text.
        ("tree", folder(), In::Rest, c(&[Text2, Text2, Text])),
        (
            "tree",
            folder(),
            In::Hover,
            c(&[RowHover, Text2, Text2, Text]),
        ),
        (
            "tree",
            folder(),
            In::Selected,
            c(&[TealGlow, Text2, Text2, Text]),
        ),
        ("tree", folder(), In::Disabled, c(&[Text2, Text2, Text])),
        // A row's rule, then its cells.
        (
            "table",
            table(),
            In::Rest,
            c(&[&header[..], &[Line, Text, Text]].concat()),
        ),
        (
            "table",
            table(),
            In::Hover,
            c(&[&header[..], &[RowHover, Line, Text, Text]].concat()),
        ),
        (
            "table",
            table(),
            In::Selected,
            c(&[&header[..], &[TealGlow, Line, Text, Text]].concat()),
        ),
        (
            "table",
            table(),
            In::Disabled,
            c(&[&header[..], &[Line, Text, Text]].concat()),
        ),
    ];
    for (name, collection, state, expected) in cases {
        assert_eq!(looks(collection, state), expected, "{name} {state:?}");
    }
}

/// Whether `commands` hold a focus outline: a `teal` border
/// [`FOCUS_WIDTH`] wide at `bounds`, inside them.
fn outline(commands: &[Command], bounds: Rect) -> bool {
    commands.iter().any(|command| {
        *command
            == Command::Border {
                rect: bounds,
                width: FOCUS_WIDTH,
                color: color(Token::Teal),
            }
    })
}

#[test]
fn a_column_header_paints_hover_pressed_and_focus_and_its_title_uppercase() {
    let table = || {
        Collection::table(
            "Problems",
            [Column::new("Severity", 80.0)],
            vec![Row::cells(["error"])],
        )
    };
    let header_label = |commands: &[Command]| {
        commands.iter().find_map(|command| match command {
            Command::Text { text, color, .. } if text == "SEVERITY" => Some(*color),
            _ => None,
        })
    };
    let (mut tree, id) = single(table());
    let commands = painted(&mut tree);
    assert_eq!(header_label(&commands), Some(color(Token::Teal)));
    // The cell's text is as written.
    assert!(commands.iter().any(|command| matches!(command,
        Command::Text { text, .. } if text == "error")));
    let at = Point::new(20.0, 10.0);
    tree.handle(&hover(at));
    assert_eq!(
        header_label(&painted(&mut tree)),
        Some(color(Token::TealHi))
    );
    tree.handle(&down(at));
    let commands = painted(&mut tree);
    assert_eq!(header_label(&commands), Some(color(Token::TealHi)));
    // Pressed, on `slate` above the underline, inside the table's padding.
    assert!(commands.iter().any(|command| *command
        == Command::Rect {
            rect: Rect::new(2.0, 2.0, 80.0, ROW_HEIGHT - 1.0),
            color: color(Token::Slate),
        }));
    tree.handle(&up(at));
    // Focus shows its outline inside it from the keyboard, and the
    // table's inside the table.
    let header = tree.focused().expect("the header took focus");
    tree.set_focus(header, true).expect("focus");
    let bounds = tree.bounds(header).expect("bounds");
    assert!(outline(&painted(&mut tree), bounds));
    tree.set_focus(id, true).expect("focus");
    let commands = painted(&mut tree);
    assert!(outline(&commands, tree.bounds(id).expect("bounds")));
    assert!(!outline(&commands, bounds));
}

#[test]
fn a_collection_s_focus_outline_is_inside_its_bounds_and_damages_only_itself() {
    // The doc example's list, filling a 200 by 100 window, and one beside
    // a button in a wider one.
    let window = Size::new(200.0, 100.0);
    let mut tree = Tree::new();
    let id = tree
        .insert(tree.root(), Collection::list("Explorer", labels(3)).node())
        .expect("the list");
    tree.frame(window, &METRICS);
    tree.set_focus(id, true).expect("focus");
    let frame = tree.frame(window, &METRICS).expect("the outline");
    let bounds = tree.bounds(id).expect("bounds");
    assert_eq!(bounds, Rect::new(0.0, 0.0, 200.0, 100.0));
    let borders: Vec<&Command> = frame
        .list
        .commands()
        .iter()
        .filter(|command| matches!(command, Command::Border { .. }))
        .collect();
    assert_eq!(
        borders,
        [&Command::Border {
            rect: bounds,
            width: FOCUS_WIDTH,
            color: color(Token::Teal),
        }]
    );
    // The rows sit inside the padding it is drawn in, so none covers it.
    assert!(
        rows(&tree)
            .iter()
            .all(|row| inside(row.bounds, Rect::new(2.0, 2.0, 196.0, 96.0)))
    );
    assert_eq!(frame.damage, [bounds]);
    // Beside another node, focus damages the collection only.
    let mut tree = Tree::new();
    let root = tree.root();
    tree.set_style(root, Style::row()).expect("the root");
    let list = styled(
        Collection::list("Explorer", labels(3)).node(),
        Style::column()
            .width(Length::Fixed(150.0))
            .height(Length::Fill(1.0))
            .padding(tidepool_ui::layout::Insets::all(FOCUS_WIDTH)),
    );
    let id = tree.insert(root, list).expect("the list");
    tree.insert(root, Collection::list("Other", labels(3)).node())
        .expect("another");
    tree.frame(WINDOW, &METRICS);
    tree.set_focus(id, true).expect("focus");
    let frame = tree.frame(WINDOW, &METRICS).expect("the outline");
    assert_eq!(frame.damage, [tree.bounds(id).expect("bounds")]);
}

#[test]
fn the_collections_paint_with_the_tree_s_theme() {
    // A theme whose collection tokens differ: each row's look follows it.
    let text = TIDEPOOL_DARK_TOML
        .replacen(
            "row-hover = { value = \"rgba(255,255,255,0.025)\"",
            "row-hover = { value = \"rgba(255,0,0,0.025)\"",
            1,
        )
        .replacen(
            "table-head = { value = \"rgba(41,53,58,0.60)\"",
            "table-head = { value = \"rgba(0,0,255,0.60)\"",
            1,
        );
    let theme = Arc::new(Theme::parse(&text, LIMITS).expect("a theme"));
    assert_ne!(theme.color(Token::RowHover), color(Token::RowHover));
    let (mut tree, id) = single(Collection::table(
        "Problems",
        [Column::new("Severity", 80.0)],
        vec![Row::cells(["error"])],
    ));
    tree.set_theme(Arc::clone(&theme));
    tree.handle(&hover(Point::new(20.0, view(&tree, id).y + 10.0)));
    let colors = colors(&painted(&mut tree));
    assert!(colors.contains(&theme.color(Token::RowHover)));
    assert!(colors.contains(&theme.color(Token::TableHead)));
    assert!(!colors.contains(&color(Token::RowHover)));
    assert!(!colors.contains(&color(Token::TableHead)));
}

#[test]
fn a_press_selects_the_row_under_it_and_takes_focus_without_showing_it() {
    let (mut tree, list) = single(Collection::list("Files", labels(50)));
    // Scrolled by half a row: a press 25 units down is on the second row.
    tree.handle(&wheel(Point::new(10.0, 10.0), 0.5));
    tree.frame(WINDOW, &METRICS);
    let at = Point::new(10.0, 25.0);
    let response = click(&mut tree, at);
    assert_eq!(kinds(&response, list), [ActionKind::Select]);
    assert_eq!(selected(&tree, list), Some(1));
    assert_eq!(tree.focused(), Some(list));
    assert!(!tree.focus_visible());
    // Pressing it again moves nothing.
    assert!(kinds(&click(&mut tree, at), list).is_empty());
    // Below the last row of a short list, a press selects nothing.
    let (mut tree, list) = single(Collection::list("Files", labels(3)));
    tree.handle(&down(Point::new(10.0, 200.0)));
    assert_eq!(selected(&tree, list), None);
    assert_eq!(tree.focused(), Some(list));
}

/// One thing done to a collection in the property test.
#[derive(Clone, Debug)]
enum Op {
    Key(Key),
    /// A press and its release, at this height in the window.
    Press(f32),
    /// The wheel, by lines.
    Wheel(f32),
    /// Rows inserted, and said.
    Insert {
        at: usize,
        count: usize,
    },
    /// Rows removed, and said.
    Remove {
        start: usize,
        len: usize,
    },
    /// Rows cut or added without a word.
    Silently(usize),
    /// The owner's selection.
    Select(Option<usize>),
    /// A new window height.
    Resize(f32),
}

const KEYS: [Key; 11] = [
    Key::Up,
    Key::Down,
    Key::PageUp,
    Key::PageDown,
    Key::Home,
    Key::End,
    Key::Enter,
    Key::Left,
    Key::Right,
    Key::Tab,
    Key::A,
];

/// A row the property test's model holds: some blank, some deeper.
fn random_row(rng: &mut Pcg32, n: u32) -> Row {
    let label = if rng.range(0..8u32) == 0 {
        String::new()
    } else {
        format!("r{n}")
    };
    let mut row =
        Row::cells([label, format!("c{n}"), format!("x{n}")]).depth(rng.range(0..4u32) as u16);
    if rng.range(0..3u32) == 0 {
        row = row.expanded(rng.range(0..2u32) == 0);
    }
    if rng.range(0..2u32) == 0 {
        row = row.parent(rng.range(0..60u32) as usize);
    }
    row
}

/// Where the property test puts its collection.
#[derive(Clone, Copy, Debug)]
enum Place {
    /// Filling the window.
    Root,
    /// In a column that scrolls, below this much other content, at its
    /// default style, or this high.
    Scroller { above: f32, high: Option<f32> },
}

/// A case: a kind, a place, the first rows and the operations, each with
/// whether a frame follows it, so that several may come between two
/// frames, as the events of one turn of the loop do.
type Case = (u32, Place, Vec<Row>, Vec<(Op, bool)>);

fn scenario(rng: &mut Pcg32, size: u32) -> Case {
    let kind = rng.range(0..3u32);
    let place = if rng.range(0..2u32) == 0 {
        Place::Root
    } else {
        Place::Scroller {
            above: rng.range(0.0..400.0f32),
            high: (rng.range(0..2u32) == 0).then(|| rng.range(0.0..600.0f32)),
        }
    };
    let rows = (0..rng.range(0..60u32))
        .map(|n| random_row(rng, n))
        .collect();
    let ops: Vec<Op> = (0..size.max(1))
        .map(|_| match rng.range(0..14u32) {
            0..=5 => Op::Key(*rng.choose(&KEYS).expect("keys")),
            6 => Op::Press(rng.range(0.0..WINDOW.height)),
            7 | 8 => Op::Wheel(rng.range(-12.0..12.0f32)),
            9 => Op::Insert {
                at: rng.range(0..70u32) as usize,
                count: rng.range(0..20u32) as usize,
            },
            10 => Op::Remove {
                start: rng.range(0..70u32) as usize,
                len: rng.range(0..30u32) as usize,
            },
            11 => Op::Silently(rng.range(0..80u32) as usize),
            12 => Op::Select(if rng.range(0..4u32) == 0 {
                None
            } else {
                Some(rng.range(0..90u32) as usize)
            }),
            _ => Op::Resize(rng.range(0.0..500.0f32)),
        })
        .collect();
    let ops = ops
        .into_iter()
        .map(|op| (op, rng.range(0..2u32) == 0))
        .collect();
    (kind, place, rows, ops)
}

/// Applies `op` to the collection `id` and its tree; returns whether it
/// asked to reveal the selection.
fn apply(tree: &mut Tree, id: NodeId, size: &mut Size, op: &Op) -> Result<bool, String> {
    let len = tree.widget::<Collection>(id).map_or(0, Collection::len);
    let mut reveal = false;
    match *op {
        Op::Key(pressed) => {
            let before = selected(tree, id);
            let focused = tree.focused() == Some(id);
            tree.handle(&key(pressed));
            let navigation = matches!(
                pressed,
                Key::Up | Key::Down | Key::PageUp | Key::PageDown | Key::Home | Key::End
            );
            reveal = navigation && focused && len > 0;
            if !navigation && pressed != Key::Left && pressed != Key::Right {
                let after = selected(tree, id);
                if before != after {
                    return Err(format!("{pressed:?} moved the selection"));
                }
            }
        }
        Op::Press(y) => {
            click(tree, Point::new(30.0, y));
        }
        Op::Wheel(lines) => {
            tree.handle(&wheel(Point::new(30.0, size.height / 2.0), lines));
        }
        Op::Insert { at, count } => {
            widgets::update(tree, id, |view: &mut Collection| {
                if let Some(rows) = view.model_mut::<Vec<Row>>() {
                    let at = at.min(rows.len());
                    let new: Vec<Row> = (0..count).map(|n| Row::new(format!("new {n}"))).collect();
                    rows.splice(at..at, new);
                }
                view.rows_inserted(at, count);
            })
            .map_err(|error| error.to_string())?;
        }
        Op::Remove { start, len } => {
            widgets::update(tree, id, |view: &mut Collection| {
                if let Some(rows) = view.model_mut::<Vec<Row>>() {
                    let start = start.min(rows.len());
                    let end = (start + len).min(rows.len());
                    rows.drain(start..end);
                }
                view.rows_removed(start..start + len);
            })
            .map_err(|error| error.to_string())?;
        }
        Op::Silently(new_len) => {
            widgets::update(tree, id, |view: &mut Collection| {
                if let Some(rows) = view.model_mut::<Vec<Row>>() {
                    rows.resize(new_len, Row::new("silent"));
                }
            })
            .map_err(|error| error.to_string())?;
        }
        Op::Select(row) => {
            widgets::update(tree, id, |view: &mut Collection| view.select(row))
                .map_err(|error| error.to_string())?;
            reveal = row.is_some() && len > 0;
        }
        Op::Resize(height) => {
            *size = Size::new(WINDOW.width, height);
        }
    }
    Ok(reveal)
}

/// The rows of `len` a collection keeps a node for, with its rows'
/// `area` scrolled to `offset` and the part from `top` to `bottom` on
/// screen, written out from the documentation: those that meet that part,
/// and [`ROW_MARGIN`] either side within the area. The rows reach no
/// further than the body's `real` height, which the area may round up.
fn kept(
    len: usize,
    area: f64,
    real: f64,
    offset: f64,
    (top, bottom): (f64, f64),
) -> (usize, usize) {
    let row = f64::from(ROW_HEIGHT);
    let reach = area.min(len as f64 * row - offset).min(real).max(0.0);
    let (top, bottom) = (top.clamp(0.0, reach), bottom.clamp(0.0, reach));
    if len == 0 || bottom <= top {
        return (0, 0);
    }
    let low = (offset / row).floor() as usize;
    let high = (((offset + reach) / row).ceil() as usize).min(len);
    let first = (((offset + top) / row).floor() as usize)
        .saturating_sub(ROW_MARGIN)
        .max(low);
    let end = ((((offset + bottom) / row).ceil() as usize) + ROW_MARGIN).min(high);
    if end <= first {
        (0, 0)
    } else {
        (first, end - first)
    }
}

/// What the property test knows of its tree.
struct Scene {
    id: NodeId,
    /// The scroller around the collection, if any.
    column: Option<NodeId>,
    /// The nodes besides the collection's: the root, the pass counter and
    /// a scroller's three.
    others: usize,
    passes: Rc<Cell<usize>>,
}

/// The invariants after a frame, given the offset before it and whether a
/// reveal was asked for since the last frame.
fn holds(
    tree: &mut Tree,
    scene: &Scene,
    size: Size,
    before: f64,
    reveal: bool,
) -> Result<(), String> {
    let id = scene.id;
    let Some(collection) = tree.widget::<Collection>(id) else {
        return Err("the collection is gone".into());
    };
    let (kind, len, columns) = (
        collection.kind(),
        collection.len(),
        collection.columns().len(),
    );
    let names_of: Vec<Row> = collection
        .model_as::<Vec<Row>>()
        .map_or_else(Vec::new, Clone::clone);
    let widths: Vec<f32> = collection.columns().iter().map(Column::width).collect();
    let selection = collection.selected();
    let now = collection.offset();
    if selection.is_some_and(|row| row >= len) {
        return Err(format!("row {selection:?} selected of {len}"));
    }
    // The body's content box, which the collection reads: rows that
    // overflow it by less than 1/64 of a unit do not scroll, as its
    // documentation says.
    let outer = view(tree, id);
    let content = tree.content_size(body(tree, id)).unwrap_or_default();
    let body_rect = Rect::new(outer.x, outer.y, content.width, content.height);
    let area = f64::from(body_rect.height.min(MAX_VIEW_ROWS as f32 * ROW_HEIGHT));
    let room = |extent: f64| {
        let room = extent - area;
        if room < 1.0 / 64.0 { 0.0 } else { room }
    };
    let max = room(len as f64 * f64::from(ROW_HEIGHT));
    if !(0.0..=max).contains(&now) {
        return Err(format!("offset {now} outside 0 to {max}"));
    }
    // What of the body is on screen: inside the window, and the scroller.
    let mut screen = Rect::new(0.0, 0.0, size.width, size.height);
    if let Some(column) = scene.column {
        screen = meet(screen, tree.bounds(column).unwrap_or_default());
    }
    let seen = meet(body_rect, screen);
    let band = if seen.is_empty() {
        (0.0, 0.0)
    } else {
        (
            f64::from(seen.y - body_rect.y),
            f64::from(seen.bottom() - body_rect.y),
        )
    };
    let (first, count) = kept(len, area, f64::from(body_rect.height), now, band);
    let shown = rows(tree);
    if shown.len() != count {
        return Err(format!(
            "{} rows kept, not {count} from {first} (offset {now}, band {band:?})",
            shown.len()
        ));
    }
    // Each row is named by its row of the model, as written.
    for (k, node) in shown.iter().enumerate() {
        let index = first + k;
        let row = names_of.get(index).cloned().unwrap_or_default();
        let expected = match kind {
            CollectionKind::Table => {
                // The cells its columns wider than 0 show.
                let cells: Vec<&str> = (0..columns)
                    .filter(|&column| widths[column] > 0.0)
                    .map(|column| row.cell(column))
                    .filter(|cell| !cell.trim().is_empty())
                    .collect();
                cells.join(", ")
            }
            _ => row.label().to_string(),
        };
        let expected = if expected.trim().is_empty() {
            format!("row {}", index + 1)
        } else {
            expected
        };
        if node.name != expected {
            return Err(format!(
                "row {index} is named {:?}, not {expected:?}",
                node.name
            ));
        }
        if node.selected != (selection == Some(index)) {
            return Err(format!("row {index}'s selected flag is {}", node.selected));
        }
    }
    let header = if kind == CollectionKind::Table {
        1 + columns
    } else {
        0
    };
    let width = if kind == CollectionKind::Table {
        1 + columns
    } else {
        1
    };
    let nodes = scene.others + 4 + header + count * width;
    if tree.node_count() != nodes {
        return Err(format!("{} nodes, not {nodes}", tree.node_count()));
    }
    for node in tree.accessibility().nodes() {
        if node.focusable && (node.role == Role::None || node.name.trim().is_empty()) {
            return Err(format!("an unnamed focusable node: {node:?}"));
        }
    }
    if let (true, Some(row)) = (reveal, selection) {
        // The least move of the offset, as `scroll_into_view` would make.
        let high = f64::from(ROW_HEIGHT);
        let top = row as f64 * high;
        let from = before.min(max);
        let least = if top < from || high > area {
            top
        } else if top + high > from + area {
            top + high - area
        } else {
            from
        };
        let least = least.clamp(0.0, max);
        // The tree fits a body's height to where it is in `f32`, so the
        // view a reveal saw can differ by a unit in the last place from
        // the view once the scrollers around have moved it.
        if (now - least).abs() > f64::from(SLACK) {
            return Err(format!(
                "row {row} revealed at {now}, not the least {least}"
            ));
        }
        // On screen, if the scroller and the window give it room.
        let room = ROW_HEIGHT;
        if area >= high && screen.height >= room && seen.height >= room {
            let Some(node) = shown.iter().find(|node| node.selected) else {
                return Err(format!("row {row} was revealed but is not kept"));
            };
            if !inside(node.bounds, screen) {
                return Err(format!(
                    "row {row} at {:?} is not in {screen:?}",
                    node.bounds
                ));
            }
        }
    }
    scene.passes.set(0);
    if tree.frame(size, &METRICS).is_some() {
        return Err("an idle frame drew".into());
    }
    if scene.passes.get() != 0 {
        return Err(format!(
            "an idle frame passed layout {} times",
            scene.passes.get()
        ));
    }
    Ok(())
}

#[test]
fn random_keys_scrolls_and_model_changes_keep_the_invariants() {
    check_sized(
        "random collection use",
        200,
        scenario,
        |(kind, place, first, ops)| {
            let collection = match kind {
                0 => Collection::list("Rows", first.clone()),
                1 => Collection::tree_view("Rows", first.clone()),
                _ => Collection::table(
                    "Rows",
                    [
                        Column::new("Name", 90.0),
                        Column::new("Hidden", 0.0),
                        Column::new("More", 400.0),
                    ],
                    first.clone(),
                ),
            };
            let mut tree = Tree::new();
            let passes = count_passes(&mut tree);
            let (id, column, others) = match *place {
                Place::Root => {
                    let id = tree
                        .insert(tree.root(), collection.node())
                        .map_err(|error| error.to_string())?;
                    (id, None, 2)
                }
                Place::Scroller { above, high } => {
                    let node = match high {
                        Some(high) => styled(
                            collection.node(),
                            Style::column()
                                .width(Length::Fill(1.0))
                                .height(Length::Fixed(high)),
                        ),
                        None => collection.node(),
                    };
                    let (column, id) = in_scroller(&mut tree, above, node);
                    (id, Some(column), 5)
                }
            };
            let scene = Scene {
                id,
                column,
                others,
                passes,
            };
            let mut size = WINDOW;
            tree.frame(size, &METRICS);
            tree.set_focus(id, true)
                .map_err(|error| error.to_string())?;
            // Whether an operation since the last frame asked to reveal the
            // selection, and no wheel came after it, which may scroll it
            // away again; and the offset before the frame.
            let mut reveal = false;
            for (step, (op, frame)) in ops.iter().enumerate() {
                let asked = apply(&mut tree, id, &mut size, op)?;
                reveal = match op {
                    Op::Wheel(_) | Op::Select(None) | Op::Press(_) => false,
                    _ => reveal || asked,
                };
                if *frame || step + 1 == ops.len() {
                    let before = offset(&tree, id);
                    tree.frame(size, &METRICS);
                    holds(&mut tree, &scene, size, before, reveal)
                        .map_err(|error| format!("after step {step}, {op:?}: {error}"))?;
                    reveal = false;
                }
            }
            Ok(())
        },
    );
}
