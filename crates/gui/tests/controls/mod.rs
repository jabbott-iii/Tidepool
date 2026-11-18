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

//! The golden scenes of `ui`'s controls (M3.6, ADR-0049): one for each
//! control, at scale 1 and 2, a grid of a row for each variant and a
//! column for each state: rest, hover, focus, pressed (the spec's
//! "active") and disabled.
//!
//! Each cell is a tree of its own holding the control centred in it, put
//! in its state as a user would put it: a pointer move to the control's
//! centre, Tab, a press there without its release, or
//! `Tree::set_enabled` with the pointer over it and pressing. A control that takes no focus stays at rest in the
//! focus column. Each cell is painted by [`Painter`] over the theme's
//! `bg` and copied into the scene, which is written as the other scenes
//! are.

use tidepool_core::geom::{DevicePoint, DeviceSize, Point, Scale, Size};
use tidepool_core::input::{InputEvent, Key, Modifiers, MouseButton};
use tidepool_gfx::framebuffer::Framebuffer;
use tidepool_ui::layout::{Align, Axis, Insets, Length, Style};
use tidepool_ui::theme::Token;
use tidepool_ui::widgets::{
    Breadcrumbs, Button, ButtonKind, ButtonSize, Chip, Control, Eyebrow, Kicker, ScrollBar,
    SectionTitle, Status, StatusDot, TextInput,
};
use tidepool_ui::{Node, NodeId, Tree};

use super::{painter, save, token};

/// A state a cell shows, in column order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Rest,
    Hover,
    Focus,
    Pressed,
    Disabled,
}

const STATES: [State; 5] = [
    State::Rest,
    State::Hover,
    State::Focus,
    State::Pressed,
    State::Disabled,
];

/// Inserts a row's control under the cell's root, and returns the node the
/// pointer aims at and the node a disabled cell disables.
type Make = fn(&mut Tree, NodeId) -> (NodeId, NodeId);

/// Inserts `node`, which the pointer aims at and which is disabled.
fn one(tree: &mut Tree, parent: NodeId, node: Node) -> (NodeId, NodeId) {
    let id = tree.insert(parent, node).expect("a control");
    (id, id)
}

/// A cell of `size` logical units in `state`, holding what `make` makes,
/// painted at `factor`.
fn cell(size: Size, factor: f32, make: Make, state: State) -> Framebuffer {
    let mut tree = Tree::new();
    let root = tree.root();
    tree.set_style(
        root,
        Style::stack()
            .padding(Insets::all(8.0))
            .align(Align::Center, Align::Center),
    )
    .expect("a style");
    let (target, disable) = make(&mut tree, root);
    let scale = Scale::new(factor).expect("a scale");
    let mut painter = painter();
    tree.layout(size, &painter.metrics(scale));
    let bounds = tree.bounds(target).expect("laid out");
    let centre = Point::new(
        bounds.x + bounds.width / 2.0,
        bounds.y + bounds.height / 2.0,
    );
    match state {
        State::Rest => {}
        State::Hover => {
            tree.handle(&InputEvent::MouseMove {
                position: centre,
                modifiers: Modifiers::NONE,
            });
        }
        State::Focus => {
            tree.handle(&InputEvent::KeyDown {
                key: Key::Tab,
                modifiers: Modifiers::NONE,
                repeat: false,
            });
        }
        State::Pressed => {
            tree.handle(&InputEvent::MouseDown {
                button: MouseButton::Left,
                position: centre,
                modifiers: Modifiers::NONE,
            });
        }
        // Disabled, with the pointer over it and pressing: a disabled
        // control shows neither (ADR-0049).
        State::Disabled => {
            tree.set_enabled(disable, false).expect("disabled");
            tree.handle(&InputEvent::MouseMove {
                position: centre,
                modifiers: Modifiers::NONE,
            });
            tree.handle(&InputEvent::MouseDown {
                button: MouseButton::Left,
                position: centre,
                modifiers: Modifiers::NONE,
            });
        }
    }
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

/// Paints a grid of `rows`, a cell of `size` for each state, at `factor`,
/// and writes it as `name`.
fn grid(name: &str, size: Size, factor: f32, rows: &[Make]) {
    let (cell_width, cell_height) = ((size.width * factor) as i32, (size.height * factor) as i32);
    let device = DeviceSize::new(
        cell_width * STATES.len() as i32,
        cell_height * rows.len() as i32,
    );
    let mut scene = Framebuffer::new(device).expect("a scene");
    let bounds = scene.bounds();
    for (row, &make) in rows.iter().enumerate() {
        for (column, &state) in STATES.iter().enumerate() {
            let painted = cell(size, factor, make, state);
            let at = DevicePoint::new(cell_width * column as i32, cell_height * row as i32);
            tidepool_gfx::image::copy(&mut scene, bounds, &painted.view(), at);
        }
    }
    save(name, &scene);
}

const BUTTON_CELL: Size = Size::new(120.0, 46.0);

fn buttons() -> Vec<Make> {
    vec![
        |tree, root| one(tree, root, Button::new("Cancel", ButtonKind::Ghost).node()),
        |tree, root| one(tree, root, Button::new("Open", ButtonKind::Teal).node()),
        |tree, root| one(tree, root, Button::new("Build", ButtonKind::Amber).node()),
        |tree, root| one(tree, root, Button::new("Close", ButtonKind::Slate).node()),
        |tree, root| {
            let button = Button::new("Cancel", ButtonKind::Ghost).size(ButtonSize::Small);
            one(tree, root, button.node())
        },
        |tree, root| {
            let button = Button::new("Open", ButtonKind::Teal).size(ButtonSize::Small);
            one(tree, root, button.node())
        },
        |tree, root| {
            let button = Button::new("Build", ButtonKind::Amber).size(ButtonSize::Small);
            one(tree, root, button.node())
        },
        |tree, root| {
            let button = Button::new("Close", ButtonKind::Slate).size(ButtonSize::Small);
            one(tree, root, button.node())
        },
    ]
}

const CHIP_CELL: Size = Size::new(100.0, 40.0);

fn chips() -> Vec<Make> {
    vec![
        |tree, root| one(tree, root, Chip::new("Errors").node()),
        |tree, root| one(tree, root, Chip::new("Rust").selected(true).node()),
    ]
}

const FIELD_CELL: Size = Size::new(224.0, 46.0);

fn text_inputs() -> Vec<Make> {
    vec![
        |tree, root| {
            let input = TextInput::new("Branch").placeholder("Branch name");
            one(tree, root, input.node())
        },
        |tree, root| {
            let mut input = TextInput::new("Branch");
            input.set_text("main");
            one(tree, root, input.node())
        },
        |tree, root| {
            let mut input = TextInput::new("Branch");
            input.set_text("feature/ui-controls");
            input.select(8..19);
            one(tree, root, input.node())
        },
        |tree, root| {
            let mut input = TextInput::new("Keystore password").secret();
            input.set_text("hunter2");
            one(tree, root, input.node())
        },
    ]
}

fn search_fields() -> Vec<Make> {
    vec![
        |tree, root| {
            let input = TextInput::search("Find in files").placeholder("Search");
            one(tree, root, input.node())
        },
        |tree, root| {
            let mut input = TextInput::search("Find in files");
            input.set_text("fn paint");
            one(tree, root, input.node())
        },
    ]
}

const SCROLLBAR_CELL: Size = Size::new(100.0, 100.0);

/// A scrollbar on `axis` over 400 units of content, a view of 100 at 100.
fn scrollbar(tree: &mut Tree, root: NodeId, axis: Axis) -> (NodeId, NodeId) {
    let mut bar = ScrollBar::new(axis, "Explorer");
    bar.set_range(400.0, 100.0);
    bar.set_offset(100.0);
    let style = match axis {
        Axis::Vertical => Style::stack()
            .width(Length::Fixed(10.0))
            .height(Length::Fixed(80.0)),
        Axis::Horizontal => Style::stack()
            .width(Length::Fixed(80.0))
            .height(Length::Fixed(10.0)),
    };
    let mut node = bar.node();
    node.style = style;
    // The thumb is 24 long, from 18.67 along the 80 of the track, so the
    // pointer, at the bar's centre, is on it.
    one(tree, root, node)
}

fn scrollbars() -> Vec<Make> {
    vec![
        |tree, root| scrollbar(tree, root, Axis::Vertical),
        |tree, root| scrollbar(tree, root, Axis::Horizontal),
    ]
}

const DOT_CELL: Size = Size::new(40.0, 30.0);

fn status_dots() -> Vec<Make> {
    vec![
        |tree, root| one(tree, root, StatusDot::new("git", Status::Ready).node()),
        |tree, root| one(tree, root, StatusDot::new("git", Status::Busy).node()),
        |tree, root| one(tree, root, StatusDot::new("git", Status::Failed).node()),
        |tree, root| one(tree, root, StatusDot::new("git", Status::Off).node()),
    ]
}

const TITLE_CELL: Size = Size::new(150.0, 36.0);

fn titles() -> Vec<Make> {
    vec![
        |tree, root| one(tree, root, SectionTitle::new("Problems").node()),
        |tree, root| one(tree, root, Eyebrow::new("Explorer").node()),
        |tree, root| one(tree, root, Kicker::new("Toolchain").node()),
    ]
}

const CRUMB_CELL: Size = Size::new(260.0, 36.0);

fn breadcrumbs() -> Vec<Make> {
    vec![|tree, root| {
        let crumbs = Breadcrumbs::new("Breadcrumbs", ["crates", "ui", "src", "widgets.rs"]);
        let nav = crumbs.insert(tree, root).expect("breadcrumbs");
        let first = tree.children(nav).expect("crumbs")[0];
        (first, nav)
    }]
}

#[test]
fn ui_buttons() {
    grid("ui_buttons", BUTTON_CELL, 1.0, &buttons());
}

#[test]
fn ui_buttons_scale_2() {
    grid("ui_buttons_scale_2", BUTTON_CELL, 2.0, &buttons());
}

#[test]
fn ui_chips() {
    grid("ui_chips", CHIP_CELL, 1.0, &chips());
}

#[test]
fn ui_chips_scale_2() {
    grid("ui_chips_scale_2", CHIP_CELL, 2.0, &chips());
}

#[test]
fn ui_text_input() {
    grid("ui_text_input", FIELD_CELL, 1.0, &text_inputs());
}

#[test]
fn ui_text_input_scale_2() {
    grid("ui_text_input_scale_2", FIELD_CELL, 2.0, &text_inputs());
}

#[test]
fn ui_search_field() {
    grid("ui_search_field", FIELD_CELL, 1.0, &search_fields());
}

#[test]
fn ui_search_field_scale_2() {
    grid("ui_search_field_scale_2", FIELD_CELL, 2.0, &search_fields());
}

#[test]
fn ui_scrollbar() {
    grid("ui_scrollbar", SCROLLBAR_CELL, 1.0, &scrollbars());
}

#[test]
fn ui_scrollbar_scale_2() {
    grid("ui_scrollbar_scale_2", SCROLLBAR_CELL, 2.0, &scrollbars());
}

#[test]
fn ui_status_dot() {
    grid("ui_status_dot", DOT_CELL, 1.0, &status_dots());
}

#[test]
fn ui_status_dot_scale_2() {
    grid("ui_status_dot_scale_2", DOT_CELL, 2.0, &status_dots());
}

#[test]
fn ui_titles() {
    grid("ui_titles", TITLE_CELL, 1.0, &titles());
}

#[test]
fn ui_titles_scale_2() {
    grid("ui_titles_scale_2", TITLE_CELL, 2.0, &titles());
}

#[test]
fn ui_breadcrumbs() {
    grid("ui_breadcrumbs", CRUMB_CELL, 1.0, &breadcrumbs());
}

#[test]
fn ui_breadcrumbs_scale_2() {
    grid("ui_breadcrumbs_scale_2", CRUMB_CELL, 2.0, &breadcrumbs());
}
