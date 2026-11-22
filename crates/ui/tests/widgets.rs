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

//! M3.6's controls, through the public API only: each one's role and name
//! in the accessibility tree, what activates or changes it, the colors it
//! paints in each state, fields edited by grapheme cluster, the scrollbar
//! dragged and paged, breadcrumbs rebuilt, `widgets::update`, and the
//! tree's theme (ADR-0049). A property test edits fields at random.

use std::sync::Arc;

use tidepool_core::color::Color;
use tidepool_core::geom::{Point, Rect, Size};
use tidepool_core::input::{InputEvent, Key, Modifiers, MouseButton};
use tidepool_core::rng::Pcg32;
use tidepool_core::testing::check_sized;
use tidepool_text::pos::grapheme;
use tidepool_ui::draw::{Command, DrawList, PaintContext};
use tidepool_ui::input::{Action, ActionKind, Response};
use tidepool_ui::layout::{Axis, Length, Style};
use tidepool_ui::metrics::FixedMetrics;
use tidepool_ui::theme::{LIMITS, TIDEPOOL_DARK_TOML, Theme, Token};
use tidepool_ui::widgets::{
    self, Breadcrumbs, Button, ButtonKind, ButtonSize, Chip, Control, Crumb, DISABLED_OPACITY,
    Eyebrow, Kicker, MAX_INPUT_BYTES, ScrollBar, SectionTitle, Status, StatusDot, TextInput,
};
use tidepool_ui::{Node, NodeId, Tree, UiError, Widget};

const METRICS: FixedMetrics = FixedMetrics::DEFAULT;
const WINDOW: Size = Size::new(400.0, 300.0);

/// The width of a character of a field's text in [`METRICS`]: 0.6 em of
/// 13 units.
const CHAR: f32 = 7.8;

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

/// Space pressed and released, as the platform sends it: its key going
/// down, the space it types, and its key coming up.
fn space() -> [InputEvent; 3] {
    [key(Key::Space), text(" "), key_up(Key::Space)]
}

fn text(text: &str) -> InputEvent {
    InputEvent::Text(text.to_string())
}

fn down_with(at: Point, modifiers: Modifiers) -> InputEvent {
    InputEvent::MouseDown {
        button: MouseButton::Left,
        position: at,
        modifiers,
    }
}

fn down(at: Point) -> InputEvent {
    down_with(at, Modifiers::NONE)
}

fn up(at: Point) -> InputEvent {
    InputEvent::MouseUp {
        button: MouseButton::Left,
        position: at,
        modifiers: Modifiers::NONE,
    }
}

fn hover(at: Point) -> InputEvent {
    InputEvent::MouseMove {
        position: at,
        modifiers: Modifiers::NONE,
    }
}

fn color(token: Token) -> Color {
    Theme::bundled().color(token)
}

/// A tree holding `node` under the root, laid out.
fn single(node: Node) -> (Tree, NodeId) {
    let mut tree = Tree::new();
    let id = tree.insert(tree.root(), node).expect("a node");
    tree.layout(WINDOW, &METRICS);
    (tree, id)
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

/// What `id` painted in a frame drawn now: its commands, as the whole
/// list holds them (one node of a single-node tree).
fn painted(tree: &mut Tree) -> Vec<Command> {
    tree.invalidate_all();
    tree.frame(WINDOW, &METRICS)
        .expect("a frame")
        .list
        .commands()
        .to_vec()
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

fn text_color(commands: &[Command]) -> Option<Color> {
    commands.iter().find_map(|command| match command {
        Command::Text { color, .. } => Some(*color),
        _ => None,
    })
}

fn has_focus_outline(commands: &[Command], bounds: Rect) -> bool {
    commands.iter().any(|command| {
        matches!(command, Command::Border { rect, width, color }
            if *width == widgets::FOCUS_WIDTH
                && *color == self::color(Token::Teal)
                && *rect == Rect::new(bounds.x - 5.0, bounds.y - 5.0, bounds.width + 10.0, bounds.height + 10.0))
    })
}

#[test]
fn every_control_has_a_role_and_a_name() {
    let mut tree = Tree::new();
    let root = tree.root();
    let mut bar = ScrollBar::new(Axis::Vertical, "Explorer");
    bar.set_range(400.0, 100.0);
    let nodes = [
        Button::new("Build", ButtonKind::Amber).node(),
        Button::new("Cancel", ButtonKind::Ghost).node(),
        Chip::new("Errors").selected(true).node(),
        TextInput::new("Commit message").node(),
        TextInput::search("Find in files").node(),
        TextInput::new("Keystore password").secret().node(),
        bar.node(),
        StatusDot::new("rust-analyzer", Status::Busy).node(),
        SectionTitle::new("Problems").node(),
        Eyebrow::new("Explorer").node(),
        Kicker::new("Toolchain").node(),
    ];
    for node in nodes {
        tree.insert(root, node).expect("a control");
    }
    Breadcrumbs::new("Breadcrumbs", ["crates", "ui.rs"])
        .insert(&mut tree, root)
        .expect("breadcrumbs");
    assert_eq!(
        tree.accessibility().to_string(),
        "button \"Build\" focusable\n\
         button \"Cancel\" focusable\n\
         toggle button \"Errors\" focusable checked\n\
         text input \"Commit message\" = \"\" focusable\n\
         search input \"Find in files\" = \"\" focusable\n\
         text input \"Keystore password\" focusable\n\
         scroll bar \"Explorer\" = \"0%\"\n\
         status \"rust-analyzer\" = \"busy\"\n\
         heading \"Problems\"\n\
         heading \"Explorer\"\n\
         label \"Toolchain\"\n\
         navigation \"Breadcrumbs\"\n  link \"crates\" focusable\n  label \"ui.rs\"\n"
    );
    for node in tree.accessibility().nodes() {
        assert!(!node.name.trim().is_empty(), "{node:?}");
    }
}

#[test]
fn a_button_activates_on_a_click_on_it_on_enter_and_on_space_coming_up() {
    let (mut tree, save) = single(Button::new("Save", ButtonKind::Teal).node());
    let inside = Point::new(5.0, 5.0);
    tree.handle(&down(inside));
    assert_eq!(
        kinds(&tree.handle(&up(inside)), save),
        [ActionKind::Activate]
    );
    // Released off the button, the press does nothing.
    tree.handle(&down(inside));
    assert!(kinds(&tree.handle(&up(Point::new(300.0, 200.0))), save).is_empty());
    tree.set_focus(save, true).expect("focus");
    for activating in [Key::Enter, Key::NumpadEnter] {
        assert_eq!(
            kinds(&tree.handle(&key(activating)), save),
            [ActionKind::Activate]
        );
    }
    // Space activates it as it comes up, after the space it typed.
    let [press, typed, release] = space();
    let pressed = tree.handle(&press);
    assert!(pressed.handled && pressed.actions.is_empty());
    assert!(tree.handle(&typed).handled);
    assert_eq!(kinds(&tree.handle(&release), save), [ActionKind::Activate]);
    // Space coming up without having gone down on it does nothing.
    assert!(!tree.handle(&key_up(Key::Space)).handled);
    // A held key's repeats, and Ctrl+Enter, do not activate it.
    let repeat = InputEvent::KeyDown {
        key: Key::Enter,
        modifiers: Modifiers::NONE,
        repeat: true,
    };
    assert!(kinds(&tree.handle(&repeat), save).is_empty());
    let chord = tree.handle(&key_with(Key::Enter, Modifiers::CTRL));
    assert!(kinds(&chord, save).is_empty());
    assert!(!chord.handled, "Ctrl+Enter is the owner's");
    // Disabled, nothing reaches it.
    tree.set_enabled(save, false).expect("disabled");
    tree.handle(&down(inside));
    assert!(tree.handle(&up(inside)).actions.is_empty());
}

#[test]
fn buttons_paint_each_state_in_the_theme_s_tokens() {
    let ghost = |look: &str| {
        let (mut tree, id) = single(Button::new("Run", ButtonKind::Ghost).node());
        let at = Point::new(5.0, 5.0);
        match look {
            "hover" => {
                tree.handle(&hover(at));
            }
            "pressed" => {
                tree.handle(&down(at));
            }
            "focus" => tree.set_focus(id, true).expect("focus"),
            "disabled" => tree.set_enabled(id, false).expect("disabled"),
            _ => {}
        }
        let bounds = tree.bounds(id).expect("bounds");
        (painted(&mut tree), bounds)
    };
    let (rest, bounds) = ghost("rest");
    assert_eq!(
        colors(&rest),
        [color(Token::Ghost), color(Token::Line2), color(Token::Text)]
    );
    assert!(!has_focus_outline(&rest, bounds));
    let (hovered, _) = ghost("hover");
    assert_eq!(
        colors(&hovered),
        [
            color(Token::Ghost),
            color(Token::Teal),
            color(Token::TealHi)
        ]
    );
    let (pressed, _) = ghost("pressed");
    assert_eq!(
        colors(&pressed),
        [
            color(Token::TealWash),
            color(Token::Teal),
            color(Token::TealHi)
        ]
    );
    let (focused, bounds) = ghost("focus");
    assert!(has_focus_outline(&focused, bounds));
    let (disabled, _) = ghost("disabled");
    assert_eq!(
        disabled.first(),
        Some(&Command::PushLayer {
            opacity: DISABLED_OPACITY
        })
    );
    assert_eq!(disabled.last(), Some(&Command::PopLayer));
    assert_eq!(
        colors(&disabled),
        colors(&rest),
        "disabled is the rest look, dimmed"
    );

    // The amber button is cut by the theme's `cut`, top-right and
    // bottom-left, and its label is uppercase in `on-amber`.
    let (mut tree, _) = single(
        Button::new("Build", ButtonKind::Amber)
            .size(ButtonSize::Small)
            .node(),
    );
    let commands = painted(&mut tree);
    let Command::CutCornerRect {
        rect,
        cut,
        corners,
        color: fill,
    } = &commands[0]
    else {
        panic!("{commands:?}");
    };
    assert_eq!(
        (rect.height, *cut),
        (widgets::BUTTON_HEIGHT_SMALL, Theme::bundled().cut())
    );
    assert_eq!(
        *corners,
        tidepool_ui::draw::Corners::TOP_RIGHT_AND_BOTTOM_LEFT
    );
    assert_eq!(*fill, color(Token::Amber));
    // Its label, cut to the button.
    assert_eq!(commands[1], Command::PushClip { rect: *rect });
    assert!(matches!(&commands[2], Command::Text { text, color, .. }
        if text == "BUILD" && *color == self::color(Token::OnAmber)));
    assert_eq!(commands[3], Command::PopClip);
}

#[test]
fn a_label_wider_than_its_button_paints_nothing_outside_it() {
    // A row 120 wide squeezes a button whose label is far wider.
    let mut tree = Tree::new();
    let row = tree
        .insert(tree.root(), Node::new(Style::row().fixed(120.0, 30.0)))
        .expect("a row");
    let label = format!("Allow{}", " ".repeat(255));
    let button = tree
        .insert(row, Button::new(label, ButtonKind::Teal).node())
        .expect("a button");
    let commands = painted(&mut tree);
    let bounds = tree.bounds(button).expect("bounds");
    assert!(bounds.width <= 120.0);
    let at = commands
        .iter()
        .position(|command| matches!(command, Command::Text { .. }))
        .expect("its label");
    assert_eq!(commands[at - 1], Command::PushClip { rect: bounds });
    assert_eq!(commands[at + 1], Command::PopClip);
}

/// The state a control is put in, as a user would put it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum In {
    Rest,
    Hover,
    Pressed,
    Focus,
    /// Disabled, with the pointer over it and pressing on it: a disabled
    /// control shows neither.
    Disabled,
}

/// The colors `node` paints in `state`, alone in a tree, without the
/// focus outline's.
fn looks(node: Node, state: In) -> Vec<Color> {
    let (mut tree, id) = single(node);
    let bounds = tree.bounds(id).expect("bounds");
    let centre = Point::new(
        bounds.x + bounds.width / 2.0,
        bounds.y + bounds.height / 2.0,
    );
    match state {
        In::Rest => {}
        In::Hover => {
            tree.handle(&hover(centre));
        }
        In::Pressed => {
            tree.handle(&down(centre));
        }
        In::Focus => tree.set_focus(id, true).expect("focus"),
        In::Disabled => {
            tree.set_enabled(id, false).expect("disabled");
            tree.handle(&hover(centre));
            tree.handle(&down(centre));
        }
    }
    let commands = painted(&mut tree);
    if state == In::Disabled {
        assert_eq!(
            commands.first(),
            Some(&Command::PushLayer {
                opacity: DISABLED_OPACITY
            })
        );
    }
    let outline = Color::rgba(0, 0, 0, 0);
    let mut colors = colors(&commands);
    if has_focus_outline(&commands, bounds) {
        colors.pop();
        colors.push(outline);
    }
    colors
}

/// A control's name, how to make it, a state and the colors it paints in
/// that state.
type Case = (&'static str, fn() -> Node, In, Vec<Color>);

#[test]
fn each_control_paints_each_state_in_the_tokens_adr_0049_gives() {
    use Token::*;
    let c = |tokens: &[Token]| tokens.iter().map(|&token| color(token)).collect::<Vec<_>>();
    let outlined = |tokens: &[Token]| {
        let mut colors = c(tokens);
        colors.push(Color::rgba(0, 0, 0, 0));
        colors
    };
    // Each control's colors in order: fills and borders, then its text.
    let cases: Vec<Case> = vec![
        (
            "ghost",
            || Button::new("Go", ButtonKind::Ghost).node(),
            In::Rest,
            c(&[Ghost, Line2, Text]),
        ),
        (
            "ghost",
            || Button::new("Go", ButtonKind::Ghost).node(),
            In::Hover,
            c(&[Ghost, Teal, TealHi]),
        ),
        (
            "ghost",
            || Button::new("Go", ButtonKind::Ghost).node(),
            In::Pressed,
            c(&[TealWash, Teal, TealHi]),
        ),
        (
            "ghost",
            || Button::new("Go", ButtonKind::Ghost).node(),
            In::Focus,
            outlined(&[Ghost, Line2, Text]),
        ),
        (
            "ghost",
            || Button::new("Go", ButtonKind::Ghost).node(),
            In::Disabled,
            c(&[Ghost, Line2, Text]),
        ),
        (
            "teal",
            || Button::new("Go", ButtonKind::Teal).node(),
            In::Rest,
            c(&[TealWash, TealLine, TealHi]),
        ),
        (
            "teal",
            || Button::new("Go", ButtonKind::Teal).node(),
            In::Hover,
            c(&[TealFill, Teal, TealHi]),
        ),
        (
            "teal",
            || Button::new("Go", ButtonKind::Teal).node(),
            In::Pressed,
            c(&[TealGlow, Teal, TealHi]),
        ),
        (
            "teal",
            || Button::new("Go", ButtonKind::Teal).node(),
            In::Disabled,
            c(&[TealWash, TealLine, TealHi]),
        ),
        (
            "amber",
            || Button::new("Go", ButtonKind::Amber).node(),
            In::Rest,
            c(&[Amber, OnAmber]),
        ),
        (
            "amber",
            || Button::new("Go", ButtonKind::Amber).node(),
            In::Hover,
            c(&[AmberHi, OnAmber]),
        ),
        (
            "amber",
            || Button::new("Go", ButtonKind::Amber).node(),
            In::Pressed,
            c(&[AmberHi, Amber, OnAmber]),
        ),
        (
            "amber",
            || Button::new("Go", ButtonKind::Amber).node(),
            In::Focus,
            outlined(&[Amber, OnAmber]),
        ),
        (
            "amber",
            || Button::new("Go", ButtonKind::Amber).node(),
            In::Disabled,
            c(&[Amber, OnAmber]),
        ),
        (
            "slate",
            || Button::new("Go", ButtonKind::Slate).node(),
            In::Rest,
            c(&[Slate, Text]),
        ),
        (
            "slate",
            || Button::new("Go", ButtonKind::Slate).node(),
            In::Hover,
            c(&[SlateHi, TealHi]),
        ),
        (
            "slate",
            || Button::new("Go", ButtonKind::Slate).node(),
            In::Pressed,
            c(&[SlateHi, TealLine, TealHi]),
        ),
        (
            "slate",
            || Button::new("Go", ButtonKind::Slate).node(),
            In::Disabled,
            c(&[Slate, Text]),
        ),
        (
            "chip",
            || Chip::new("Rust").node(),
            In::Rest,
            c(&[Slate, Teal]),
        ),
        (
            "chip",
            || Chip::new("Rust").node(),
            In::Hover,
            c(&[Slate, TealLine, Slate, Teal]),
        ),
        (
            "chip",
            || Chip::new("Rust").node(),
            In::Pressed,
            c(&[SlateHi, TealLine, SlateHi, Teal]),
        ),
        (
            "chip",
            || Chip::new("Rust").node(),
            In::Focus,
            outlined(&[Slate, Teal]),
        ),
        (
            "chip",
            || Chip::new("Rust").node(),
            In::Disabled,
            c(&[Slate, Teal]),
        ),
        (
            "selected chip",
            || Chip::new("Rust").selected(true).node(),
            In::Rest,
            c(&[Teal, OnTeal]),
        ),
        (
            "selected chip",
            || Chip::new("Rust").selected(true).node(),
            In::Hover,
            c(&[TealHi, OnTeal]),
        ),
        (
            "selected chip",
            || Chip::new("Rust").selected(true).node(),
            In::Pressed,
            c(&[TealHi, Teal, OnTeal]),
        ),
        (
            "selected chip",
            || Chip::new("Rust").selected(true).node(),
            In::Disabled,
            c(&[Teal, OnTeal]),
        ),
        (
            "field",
            || TextInput::new("Name").placeholder("Hint").node(),
            In::Rest,
            c(&[Slate, Text2]),
        ),
        (
            "field",
            || TextInput::new("Name").placeholder("Hint").node(),
            In::Hover,
            c(&[Slate, TealLine, Text2]),
        ),
        // A press gives it focus: its ring, border and caret.
        (
            "field",
            || TextInput::new("Name").placeholder("Hint").node(),
            In::Pressed,
            c(&[TealRing, Slate, Teal, Text2, Teal]),
        ),
        (
            "field",
            || TextInput::new("Name").placeholder("Hint").node(),
            In::Focus,
            c(&[TealRing, Slate, Teal, Text2, Teal]),
        ),
        (
            "field",
            || TextInput::new("Name").placeholder("Hint").node(),
            In::Disabled,
            c(&[Slate, Text2]),
        ),
        (
            "search",
            || TextInput::search("Find").node(),
            In::Rest,
            c(&[Slate, Text2, Slate, Text2]),
        ),
        (
            "search",
            || TextInput::search("Find").node(),
            In::Hover,
            c(&[Slate, TealLine, Slate, Text2, Slate, Text2]),
        ),
        (
            "search",
            || TextInput::search("Find").node(),
            In::Focus,
            c(&[TealRing, Slate, Teal, Slate, Text2, Slate, Text2, Teal]),
        ),
        (
            "search",
            || TextInput::search("Find").node(),
            In::Disabled,
            c(&[Slate, Text2, Slate, Text2]),
        ),
        (
            "link",
            || Crumb::new("src", false).node(),
            In::Rest,
            c(&[Text2]),
        ),
        (
            "link",
            || Crumb::new("src", false).node(),
            In::Hover,
            c(&[TealHi]),
        ),
        (
            "link",
            || Crumb::new("src", false).node(),
            In::Pressed,
            c(&[Teal]),
        ),
        (
            "link",
            || Crumb::new("src", false).node(),
            In::Focus,
            outlined(&[Text2]),
        ),
        (
            "link",
            || Crumb::new("src", false).node(),
            In::Disabled,
            c(&[Text2]),
        ),
        (
            "current crumb",
            || Crumb::new("lib.rs", true).node(),
            In::Hover,
            c(&[Text3]),
        ),
        (
            "current crumb",
            || Crumb::new("lib.rs", true).node(),
            In::Pressed,
            c(&[Text3]),
        ),
        (
            "ready dot",
            || StatusDot::new("git", Status::Ready).node(),
            In::Hover,
            c(&[GreenHalo, Green]),
        ),
        (
            "busy dot",
            || StatusDot::new("git", Status::Busy).node(),
            In::Rest,
            c(&[AmberHalo, Amber]),
        ),
        (
            "failed dot",
            || StatusDot::new("git", Status::Failed).node(),
            In::Disabled,
            c(&[ErrorHalo, Error]),
        ),
        (
            "section title",
            || SectionTitle::new("Problems").node(),
            In::Pressed,
            c(&[Teal, Text]),
        ),
        (
            "eyebrow",
            || Eyebrow::new("Explorer").node(),
            In::Hover,
            c(&[Text3]),
        ),
        (
            "kicker",
            || Kicker::new("Rust").node(),
            In::Disabled,
            c(&[Teal]),
        ),
    ];
    for (name, make, state, expected) in cases {
        assert_eq!(looks(make(), state), expected, "{name} {state:?}");
    }
}

#[test]
fn a_scrollbar_paints_each_state() {
    use Token::*;
    let make = || {
        let mut bar = ScrollBar::new(Axis::Vertical, "Explorer");
        bar.set_range(400.0, 100.0);
        let mut node = bar.node();
        node.style = Style::stack().fixed(10.0, 80.0);
        node
    };
    // The thumb, a quarter of 80 long at the top: the pointer at (5, 10)
    // is on it.
    let state = |state: In| {
        let (mut tree, id) = single(make());
        let at = Point::new(5.0, 10.0);
        match state {
            In::Hover => {
                tree.handle(&hover(at));
            }
            In::Pressed => {
                tree.handle(&down(at));
            }
            In::Disabled => {
                tree.set_enabled(id, false).expect("disabled");
                tree.handle(&hover(at));
            }
            In::Rest | In::Focus => {}
        }
        colors(&painted(&mut tree))
    };
    assert_eq!(state(In::Rest), [color(Line2)]);
    assert_eq!(state(In::Hover), [color(Line), color(TealGlow)]);
    assert_eq!(state(In::Pressed), [color(Line), color(Teal)]);
    assert_eq!(state(In::Disabled), [color(Line2)]);
}

#[test]
fn a_chip_toggles_and_says_so() {
    let (mut tree, chip) = single(Chip::new("Warnings").node());
    let at = Point::new(5.0, 5.0);
    tree.handle(&down(at));
    assert_eq!(kinds(&tree.handle(&up(at)), chip), [ActionKind::Change]);
    assert!(tree.widget::<Chip>(chip).expect("a chip").is_selected());
    assert_eq!(tree.semantics(chip).and_then(|s| s.checked), Some(true));
    let selected = painted(&mut tree);
    assert_eq!(
        colors(&selected),
        [color(Token::TealHi), color(Token::OnTeal)],
        "hovered"
    );
    tree.set_focus(chip, true).expect("focus");
    let changes: Vec<ActionKind> = space()
        .iter()
        .flat_map(|event| kinds(&tree.handle(event), chip))
        .collect();
    assert_eq!(changes, [ActionKind::Change]);
    assert_eq!(tree.semantics(chip).and_then(|s| s.checked), Some(false));
}

/// A field alone in a tree, focused, with a frame drawn so that clicks
/// find its caret stops.
fn field(input: TextInput) -> (Tree, NodeId) {
    let (mut tree, id) = single(input.node());
    tree.set_focus(id, true).expect("focus");
    tree.frame(WINDOW, &METRICS).expect("a frame");
    (tree, id)
}

fn field_text(tree: &Tree, id: NodeId) -> String {
    tree.widget::<TextInput>(id)
        .expect("a field")
        .text()
        .to_string()
}

fn field_selection(tree: &Tree, id: NodeId) -> std::ops::Range<usize> {
    tree.widget::<TextInput>(id).expect("a field").selection()
}

#[test]
fn a_field_types_moves_and_deletes_by_grapheme_cluster() {
    let (mut tree, id) = field(TextInput::new("Name"));
    // `e` and a combining acute, a thumbs up with a skin tone, and a
    // family joined by zero-width joiners: three clusters, 29 bytes.
    let typed = tree.handle(&text(
        "e\u{301}\u{1f44d}\u{1f3fd}\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}",
    ));
    assert_eq!(kinds(&typed, id), [ActionKind::Change]);
    let value = "e\u{301}\u{1f44d}\u{1f3fd}\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}";
    assert_eq!(field_text(&tree, id), value);
    assert_eq!(
        tree.semantics(id).and_then(|s| s.value.clone()).as_deref(),
        Some(value)
    );
    // Backspace takes the whole family.
    tree.handle(&key(Key::Backspace));
    assert_eq!(field_text(&tree, id), "e\u{301}\u{1f44d}\u{1f3fd}");
    // Left steps over the thumbs up as one; Delete takes it.
    tree.handle(&key(Key::Left));
    assert_eq!(field_selection(&tree, id), 3..3);
    tree.handle(&key(Key::Delete));
    assert_eq!(field_text(&tree, id), "e\u{301}");
    // Home, then Shift+Right selects the `é`; typing replaces it.
    tree.handle(&key(Key::Home));
    tree.handle(&key_with(Key::Right, Modifiers::SHIFT));
    assert_eq!(field_selection(&tree, id), 0..3);
    tree.handle(&text("a"));
    assert_eq!(field_text(&tree, id), "a");
    // Ctrl+A selects all; Left collapses to its start, Right to its end.
    tree.handle(&text("bc"));
    tree.handle(&key_with(Key::A, Modifiers::CTRL));
    assert_eq!(field_selection(&tree, id), 0..3);
    tree.handle(&key(Key::Left));
    assert_eq!(field_selection(&tree, id), 0..0);
    tree.handle(&key_with(Key::End, Modifiers::SHIFT));
    tree.handle(&key(Key::Right));
    assert_eq!(field_selection(&tree, id), 3..3);
    // At either end, Backspace and Delete change nothing.
    assert!(kinds(&tree.handle(&key(Key::Delete)), id).is_empty());
    tree.handle(&key(Key::Home));
    assert!(kinds(&tree.handle(&key(Key::Backspace)), id).is_empty());
}

#[test]
fn a_field_keeps_one_line_within_its_limit() {
    let (mut tree, id) = field(TextInput::new("Name"));
    tree.handle(&text("a\r\nb\nc\td\re\u{7}\u{1b}[31mf\u{85}"));
    assert_eq!(field_text(&tree, id), "a b c d e[31mf");
    let mut input = TextInput::new("Name");
    input.set_text(&"x".repeat(MAX_INPUT_BYTES + 10));
    assert_eq!(input.text().len(), MAX_INPUT_BYTES);
    // A two-byte char that would cross the limit is left out.
    input.set_text(&"x".repeat(MAX_INPUT_BYTES - 1));
    assert!(!input.replace_selection("é"));
    assert_eq!(input.text().len(), MAX_INPUT_BYTES - 1);
}

#[test]
fn a_field_submits_dismisses_and_leaves_the_owner_its_keys() {
    let (mut tree, id) = field(TextInput::search("Find"));
    assert_eq!(
        kinds(&tree.handle(&key(Key::Enter)), id),
        [ActionKind::Submit]
    );
    assert_eq!(
        kinds(&tree.handle(&key(Key::Escape)), id),
        [ActionKind::Dismiss]
    );
    // Keys that type text are kept from the owner's shortcuts.
    assert!(tree.handle(&key(Key::J)).handled);
    assert!(
        tree.handle(&key_with(Key::Digit1, Modifiers::SHIFT))
            .handled
    );
    // The clipboard's keys, Ctrl+Shift+A, and Up and Down, are the owner's.
    for owners in [
        key_with(Key::A, Modifiers::CTRL | Modifiers::SHIFT),
        key_with(Key::C, Modifiers::CTRL),
        key_with(Key::V, Modifiers::CTRL),
        key(Key::Up),
        key(Key::PageDown),
        key(Key::F5),
    ] {
        assert!(!tree.handle(&owners).handled, "{owners:?}");
    }
}

#[test]
fn a_click_places_the_caret_and_a_drag_selects() {
    let (mut tree, id) = field(TextInput::new("Name"));
    tree.handle(&text("abcdef"));
    tree.frame(WINDOW, &METRICS).expect("a frame");
    let left = widgets::FIELD_PADDING;
    let at = |chars: f32| Point::new(left + chars * CHAR, 15.0);
    tree.handle(&down(at(2.2)));
    assert_eq!(field_selection(&tree, id), 2..2);
    tree.handle(&hover(at(4.6)));
    tree.handle(&up(at(4.6)));
    assert_eq!(field_selection(&tree, id), 2..5);
    tree.handle(&down_with(at(0.1), Modifiers::SHIFT));
    tree.handle(&up(at(0.1)));
    assert_eq!(field_selection(&tree, id), 0..2, "the anchor stays");
    // Past the end of the text, still on the field, the caret goes to the
    // end.
    tree.handle(&down(at(20.0)));
    assert_eq!(field_selection(&tree, id), 6..6);
}

#[test]
fn a_focused_field_paints_its_ring_selection_and_caret() {
    let (mut tree, id) = field(TextInput::new("Name"));
    tree.handle(&text("abc"));
    tree.handle(&key_with(Key::Left, Modifiers::SHIFT));
    let commands = painted(&mut tree);
    let bounds = tree.bounds(id).expect("bounds");
    assert_eq!(
        commands[0],
        Command::Rect {
            rect: Rect::new(
                bounds.x - 3.0,
                bounds.y - 3.0,
                bounds.width + 6.0,
                bounds.height + 6.0
            ),
            color: color(Token::TealRing),
        }
    );
    assert_eq!(
        colors(&commands),
        [
            color(Token::TealRing),
            color(Token::Slate),
            color(Token::Teal),
            color(Token::TealGlow),
            color(Token::Text),
            color(Token::Teal),
        ],
        "ring, fill, border, selection, text, caret"
    );
    // The selection covers the last char; the caret sits before it.
    let Command::Rect { rect, .. } = commands[4] else {
        panic!("{commands:?}");
    };
    assert_eq!(rect.x, widgets::FIELD_PADDING + 2.0 * CHAR);
    assert!((rect.width - CHAR).abs() < 1e-4, "{rect:?}");
    let caret = commands.iter().rev().find_map(|command| match command {
        Command::Rect { rect, color } if *color == self::color(Token::Teal) => Some(*rect),
        _ => None,
    });
    let caret = caret.expect("a caret");
    assert_eq!(
        (caret.x, caret.width),
        (widgets::FIELD_PADDING + 2.0 * CHAR, widgets::CARET_WIDTH)
    );
    // Unfocused and empty, it shows its placeholder in `text-2`.
    let (mut tree, _) = single(TextInput::new("Name").placeholder("Branch name").node());
    let commands = painted(&mut tree);
    assert_eq!(
        colors(&commands),
        [color(Token::Slate), color(Token::Text2)]
    );
}

#[test]
fn a_field_scrolls_its_text_to_keep_the_caret_in_view() {
    let (mut tree, id) = field(TextInput::new("Name"));
    tree.handle(&text(&"m".repeat(60)));
    let commands = painted(&mut tree);
    let origin = commands
        .iter()
        .find_map(|command| match command {
            Command::Text { origin, .. } => Some(*origin),
            _ => None,
        })
        .expect("text");
    let content = widgets::FIELD_WIDTH - 2.0 * widgets::FIELD_PADDING;
    let room = content - widgets::CARET_WIDTH;
    assert!(
        (origin.x - (widgets::FIELD_PADDING - (60.0 * CHAR - room))).abs() < 1e-3,
        "{origin:?}"
    );
    // Home brings the start back.
    tree.handle(&key(Key::Home));
    let commands = painted(&mut tree);
    assert!(commands.iter().any(|command| matches!(command,
        Command::Text { origin, .. } if origin.x == widgets::FIELD_PADDING)));
    let _ = id;
}

#[test]
fn a_secret_field_never_shows_or_gives_its_text() {
    let (mut tree, id) = field(TextInput::new("Keystore password").secret());
    tree.handle(&text("hunter2\u{1f511}"));
    assert_eq!(tree.semantics(id).and_then(|s| s.value.clone()), None);
    let input = tree.widget::<TextInput>(id).expect("a field");
    assert_eq!(input.text(), "hunter2\u{1f511}");
    let debug = format!("{input:?}");
    assert!(!debug.contains("hunter2"), "{debug}");
    assert!(debug.contains("<11 bytes>"), "{debug}");
    tree.handle(&key_with(Key::A, Modifiers::CTRL));
    assert_eq!(
        tree.widget::<TextInput>(id)
            .and_then(TextInput::selected_text),
        None
    );
    let commands = painted(&mut tree);
    let shown = commands.iter().find_map(|command| match command {
        Command::Text { text, .. } => Some(text.clone()),
        _ => None,
    });
    assert_eq!(
        shown.as_deref(),
        Some("\u{2022}".repeat(8).as_str()),
        "a bullet a cluster"
    );
    assert!(!format!("{:?}", tree.accessibility()).contains("hunter2"));
}

#[test]
fn a_scrollbar_drags_and_pages_its_offset() {
    let mut bar = ScrollBar::new(Axis::Vertical, "Explorer");
    bar.set_range(1000.0, 250.0);
    let mut tree = Tree::new();
    let root = tree.root();
    tree.set_style(root, Style::row()).expect("a row");
    let id = tree.insert(root, bar.node()).expect("a bar");
    tree.layout(WINDOW, &METRICS);
    assert_eq!(tree.bounds(id), Some(Rect::new(0.0, 0.0, 10.0, 300.0)));
    // The thumb is 75 long (a quarter of 300), at the top; dragging it 45
    // down moves the view a fifth of the way: 150 of 750.
    tree.handle(&down(Point::new(5.0, 10.0)));
    let dragged = tree.handle(&hover(Point::new(5.0, 55.0)));
    assert_eq!(kinds(&dragged, id), [ActionKind::Change]);
    tree.handle(&up(Point::new(5.0, 55.0)));
    let offset = || tree.widget::<ScrollBar>(id).expect("a bar").offset();
    assert!((offset() - 150.0).abs() < 1e-3, "{}", offset());
    assert_eq!(
        tree.semantics(id).and_then(|s| s.value.clone()).as_deref(),
        Some("20%")
    );
    // A press below the thumb pages down by a view.
    tree.handle(&down(Point::new(5.0, 280.0)));
    tree.handle(&up(Point::new(5.0, 280.0)));
    assert!((tree.widget::<ScrollBar>(id).expect("a bar").offset() - 400.0).abs() < 1e-3);
    // Everything fitting, it paints nothing.
    widgets::update::<ScrollBar, _>(&mut tree, id, |bar| bar.set_range(100.0, 250.0))
        .expect("update");
    tree.invalidate_all();
    let frame = tree.frame(WINDOW, &METRICS).expect("a frame");
    assert!(frame.list.commands().is_empty(), "{:?}", frame.list);
    assert_eq!(
        tree.semantics(id).and_then(|s| s.value.clone()).as_deref(),
        Some("0%")
    );
}

#[test]
fn a_scrollbar_over_a_huge_range_keeps_its_thumb_finite() {
    let mut bar = ScrollBar::new(Axis::Vertical, "Log");
    bar.set_range(f32::MAX, f32::MAX / 4.0);
    bar.set_offset(f32::MAX / 2.0);
    let mut node = bar.node();
    node.style = Style::stack().fixed(10.0, 80.0);
    let (mut tree, id) = single(node);
    let commands = painted(&mut tree);
    let thumb = commands
        .iter()
        .find_map(|command| match command {
            Command::Pill { rect, .. } => Some(*rect),
            _ => None,
        })
        .expect("a thumb");
    // A quarter of 80 is under the shortest thumb, 24; it lies two thirds
    // of the way down the other 56.
    assert!(
        (thumb.height - widgets::MIN_THUMB).abs() < 1e-3
            && (thumb.y - 56.0 * 2.0 / 3.0).abs() < 1e-3,
        "{thumb:?}"
    );
    // Dragging it half its travel up moves the view to a third of the
    // range, which an overflowing product would get wrong.
    tree.handle(&down(Point::new(5.0, 45.0)));
    tree.handle(&hover(Point::new(5.0, 45.0 - 56.0 / 3.0)));
    tree.handle(&up(Point::new(5.0, 45.0 - 56.0 / 3.0)));
    let offset = tree.widget::<ScrollBar>(id).expect("a bar").offset();
    let third = f32::MAX * 0.75 / 3.0;
    assert!(
        offset.is_finite() && (offset / third - 1.0).abs() < 1e-3,
        "{offset}"
    );
    // Dragging it, now from 18.67 to 42.67, to the top moves the view
    // there.
    tree.handle(&down(Point::new(5.0, 30.0)));
    tree.handle(&hover(Point::new(5.0, 0.0)));
    tree.handle(&up(Point::new(5.0, 0.0)));
    assert_eq!(tree.widget::<ScrollBar>(id).expect("a bar").offset(), 0.0);
}

#[test]
fn a_status_dot_paints_its_halo_and_off_has_none() {
    let (mut tree, id) = single(StatusDot::new("git", Status::Ready).node());
    let commands = painted(&mut tree);
    assert_eq!(
        colors(&commands),
        [color(Token::GreenHalo), color(Token::Green)]
    );
    widgets::update::<StatusDot, _>(&mut tree, id, |dot| dot.set_status(Status::Off))
        .expect("update");
    assert_eq!(colors(&painted(&mut tree)), [color(Token::Text3)]);
    assert_eq!(
        tree.semantics(id).and_then(|s| s.value.clone()).as_deref(),
        Some("off")
    );
}

#[test]
fn titles_paint_uppercase_and_name_as_written() {
    let (mut tree, id) = single(SectionTitle::new("Open editors").node());
    let commands = painted(&mut tree);
    assert_eq!(colors(&commands), [color(Token::Teal), color(Token::Text)]);
    assert!(matches!(&commands[1], Command::Text { text, .. } if text == "OPEN EDITORS"));
    assert_eq!(
        tree.semantics(id).map(|s| s.name.as_str()),
        Some("Open editors")
    );
    let (mut tree, _) = single(Eyebrow::new("Explorer").node());
    assert_eq!(text_color(&painted(&mut tree)), Some(color(Token::Text3)));
    let (mut tree, _) = single(Kicker::new("Rust").node());
    assert_eq!(text_color(&painted(&mut tree)), Some(color(Token::Teal)));
}

#[test]
fn breadcrumbs_link_their_segments_and_rebuild() {
    let mut tree = Tree::new();
    let root = tree.root();
    let nav = Breadcrumbs::new("Breadcrumbs", ["crates", "ui", "lib.rs"])
        .insert(&mut tree, root)
        .expect("breadcrumbs");
    tree.layout(WINDOW, &METRICS);
    let children = tree.children(nav).expect("children").to_vec();
    assert_eq!(children.len(), 5);
    let crates = children[0];
    assert!(
        tree.widget::<Crumb>(crates)
            .is_some_and(|crumb| !crumb.is_current())
    );
    assert!(
        tree.widget::<Crumb>(children[4])
            .is_some_and(Crumb::is_current)
    );
    assert_eq!(tree.focusable(children[4]), Some(false));
    // Paths are painted as written; the separators in `teal`.
    let commands = painted(&mut tree);
    let texts: Vec<(&str, Color)> = commands
        .iter()
        .filter_map(|command| match command {
            Command::Text { text, color, .. } => Some((text.as_str(), *color)),
            _ => None,
        })
        .collect();
    assert_eq!(
        texts,
        [
            ("crates", color(Token::Text2)),
            ("/", color(Token::Teal)),
            ("ui", color(Token::Text2)),
            ("/", color(Token::Teal)),
            ("lib.rs", color(Token::Text3)),
        ]
    );
    let at = tree.bounds(crates).expect("bounds").origin();
    let at = Point::new(at.x + 2.0, at.y + 2.0);
    tree.handle(&down(at));
    assert_eq!(
        tree.handle(&up(at)).actions,
        [Action {
            node: crates,
            kind: ActionKind::Activate
        }]
    );
    // Segments with a blank link change nothing.
    let before = tree.accessibility().to_string();
    assert_eq!(
        Breadcrumbs::set_segments(&mut tree, nav, &["docs", "", "x"]),
        Err(UiError::Unnamed {
            role: tidepool_ui::Role::Link
        })
    );
    assert_eq!(tree.accessibility().to_string(), before);
    Breadcrumbs::set_segments(&mut tree, nav, &["docs", "roadmap.md"]).expect("rebuilt");
    assert_eq!(
        tree.accessibility().to_string(),
        "navigation \"Breadcrumbs\"\n  link \"docs\" focusable\n  label \"roadmap.md\"\n"
    );
}

#[test]
fn update_changes_a_control_and_its_semantics() {
    let (mut tree, id) = single(Button::new("Save", ButtonKind::Ghost).node());
    widgets::update::<Button, _>(&mut tree, id, |button| button.set_label("Save all"))
        .expect("update");
    assert_eq!(
        tree.semantics(id).map(|s| s.name.as_str()),
        Some("Save all")
    );
    assert_eq!(
        widgets::update::<Chip, _>(&mut tree, id, |_| ()),
        Err(UiError::WrongWidget { id })
    );
    // A label that would leave a focusable button unnamed is changed, but
    // the node keeps its name.
    let unnamed = widgets::update::<Button, _>(&mut tree, id, |button| button.set_label(" "));
    assert_eq!(
        unnamed,
        Err(UiError::Unnamed {
            role: tidepool_ui::Role::Button
        })
    );
    assert_eq!(
        tree.semantics(id).map(|s| s.name.as_str()),
        Some("Save all")
    );
    tree.remove(id).expect("removed");
    assert_eq!(
        widgets::update::<Button, _>(&mut tree, id, |_| ()),
        Err(UiError::UnknownNode { id })
    );
    // A crumb that becomes the current page stops taking focus.
    let (mut tree, crumb) = single(Crumb::new("src", false).node());
    tree.set_focus(crumb, true).expect("focus");
    widgets::update::<Crumb, _>(&mut tree, crumb, |c| *c = Crumb::new("src", true))
        .expect("update");
    assert_eq!(tree.focusable(crumb), Some(false));
    assert_eq!(tree.focused(), None);
}

#[test]
fn a_click_after_an_edit_and_before_a_frame_goes_to_the_end() {
    // The stops painted for "abc" would put a click one char in at byte
    // 1, inside the wave's four bytes; the click goes to the end instead,
    // and typing there is safe.
    let (mut tree, id) = field(TextInput::new("Name"));
    tree.handle(&text("abc"));
    tree.frame(WINDOW, &METRICS).expect("a frame");
    tree.handle(&key_with(Key::A, Modifiers::CTRL));
    tree.handle(&text("\u{1f30a}"));
    let at = Point::new(widgets::FIELD_PADDING + CHAR, 15.0);
    tree.handle(&down(at));
    tree.handle(&up(at));
    assert_eq!(field_selection(&tree, id), 4..4);
    tree.handle(&text("x"));
    tree.handle(&key(Key::Backspace));
    tree.handle(&key(Key::Backspace));
    assert_eq!(field_text(&tree, id), "");
    // Stops of "abc" give byte 1 for this click, which is a boundary of
    // "xyz" too; the click still goes to the end, as the text changed.
    tree.handle(&key_with(Key::A, Modifiers::CTRL));
    tree.handle(&text("xyz"));
    tree.handle(&down(at));
    tree.handle(&up(at));
    assert_eq!(field_selection(&tree, id), 3..3);
    tree.handle(&key_with(Key::A, Modifiers::CTRL));
    tree.handle(&key(Key::Delete));
    // After the next frame, clicks use its stops again. The field paints
    // what it painted for "abc" before, so the frame damages nothing, but
    // the field painted.
    tree.handle(&text("abc"));
    assert_eq!(tree.frame(WINDOW, &METRICS), None);
    tree.handle(&down(at));
    tree.handle(&up(at));
    assert_eq!(field_selection(&tree, id), 1..1);
}

#[test]
fn space_types_nothing_where_the_owner_moves_focus_on_activation() {
    // A button and a field: the owner moves focus to the field when the
    // button is activated, as a dialog's "Rename" might.
    let mut tree = Tree::new();
    let root = tree.root();
    tree.set_style(root, Style::row()).expect("a row");
    let rename = tree
        .insert(root, Button::new("Rename", ButtonKind::Ghost).node())
        .expect("a button");
    let name = tree
        .insert(root, TextInput::new("Name").node())
        .expect("a field");
    tree.layout(WINDOW, &METRICS);
    tree.set_focus(rename, true).expect("focus");
    for event in space() {
        let response = tree.handle(&event);
        assert!(response.handled, "{event:?}");
        if kinds(&response, rename) == [ActionKind::Activate] {
            tree.set_focus(name, true).expect("focus");
        }
    }
    assert_eq!(tree.focused(), Some(name), "activated");
    assert_eq!(field_text(&tree, name), "", "the space was the button's");
}

#[test]
fn space_is_called_off_by_what_happens_while_it_is_held() {
    let mut tree = Tree::new();
    let root = tree.root();
    tree.set_style(root, Style::row()).expect("a row");
    let ok = tree
        .insert(root, Button::new("OK", ButtonKind::Ghost).node())
        .expect("a button");
    let name = tree
        .insert(root, TextInput::new("Name").node())
        .expect("a field");
    tree.layout(WINDOW, &METRICS);
    let activations = |tree: &mut Tree, events: &[InputEvent]| -> usize {
        tree.set_focus(ok, true).expect("focus");
        events
            .iter()
            .map(|event| kinds(&tree.handle(event), ok).len())
            .sum()
    };
    let shift = key(Key::LeftShift);
    let held = InputEvent::KeyDown {
        key: Key::Space,
        modifiers: Modifiers::NONE,
        repeat: true,
    };
    // Shift, a modifier, going down between keeps it.
    assert_eq!(
        activations(&mut tree, &[key(Key::Space), shift, key_up(Key::Space)]),
        1
    );
    // The window losing focus calls it off: the platform then reports the
    // release of every key held.
    assert_eq!(
        activations(
            &mut tree,
            &[key(Key::Space), InputEvent::FocusLost, key_up(Key::Space)]
        ),
        0
    );
    tree.handle(&InputEvent::FocusGained);
    // Focus moving away and back while it is held calls it off.
    tree.set_focus(ok, true).expect("focus");
    tree.handle(&key(Key::Space));
    tree.set_focus(name, false).expect("focus");
    tree.set_focus(ok, true).expect("focus");
    assert!(kinds(&tree.handle(&key_up(Key::Space)), ok).is_empty());
    // So do another key going down and a press; a repeat does not arm it.
    assert_eq!(
        activations(
            &mut tree,
            &[key(Key::Space), key(Key::J), key_up(Key::Space)]
        ),
        0
    );
    let at = tree.bounds(ok).expect("bounds").origin();
    let at = Point::new(at.x + 2.0, at.y + 2.0);
    assert_eq!(
        activations(
            &mut tree,
            &[key(Key::Space), down(at), up(at), key_up(Key::Space)]
        ),
        1,
        "the click's, not the space's"
    );
    assert_eq!(activations(&mut tree, &[held, key_up(Key::Space)]), 0);
    // Enter going down while it is held activates once, for Enter.
    assert_eq!(
        activations(
            &mut tree,
            &[key(Key::Space), key(Key::Enter), key_up(Key::Space)]
        ),
        1
    );
}

#[test]
fn a_release_over_another_control_does_not_activate_the_pressed_one() {
    // Two buttons stacked, `cancel` over the left of `save`: a press on
    // `save`'s right, released over `cancel`, is on `save`'s bounds but
    // not on it.
    let mut tree = Tree::new();
    let root = tree.root();
    tree.set_style(root, Style::stack()).expect("a stack");
    let mut wide = Button::new("Save", ButtonKind::Ghost).node();
    wide.style = Style::stack().fixed(100.0, 30.0);
    let save = tree.insert(root, wide).expect("save");
    let mut narrow = Button::new("Cancel", ButtonKind::Ghost).node();
    narrow.style = Style::stack().fixed(40.0, 30.0);
    let cancel = tree.insert(root, narrow).expect("cancel");
    tree.layout(WINDOW, &METRICS);
    tree.handle(&down(Point::new(80.0, 10.0)));
    let released = tree.handle(&up(Point::new(10.0, 10.0)));
    assert!(kinds(&released, save).is_empty(), "{released:?}");
    assert!(
        kinds(&released, cancel).is_empty(),
        "nor the one it was not pressed on"
    );
    // Nor does a release over it that no press on it captured: the window
    // lost focus while the button was down, which ends the capture, and
    // the release reaches the control as a plain release over it.
    tree.handle(&down(Point::new(80.0, 10.0)));
    tree.handle(&InputEvent::FocusLost);
    tree.handle(&InputEvent::FocusGained);
    let released = tree.handle(&up(Point::new(80.0, 10.0)));
    assert!(released.handled, "it reached the button");
    assert!(kinds(&released, save).is_empty(), "{released:?}");
}

#[test]
fn a_control_made_focusable_without_a_name_keeps_its_node_as_it_was() {
    let (mut tree, crumb) = single(Crumb::new("src", true).node());
    let changed = widgets::update::<Crumb, _>(&mut tree, crumb, |c| *c = Crumb::new(" ", false));
    assert_eq!(
        changed,
        Err(UiError::Unnamed {
            role: tidepool_ui::Role::Link
        })
    );
    assert_eq!(tree.semantics(crumb).map(|s| s.name.as_str()), Some("src"));
    assert_eq!(tree.focusable(crumb), Some(false));
}

#[test]
fn breadcrumbs_too_deep_for_the_tree_leave_it_as_it_was() {
    // A row at the deepest level the tree allows has no room for crumbs.
    let mut tree = Tree::new();
    let mut parent = tree.root();
    for _ in 0..tidepool_ui::tree::MAX_DEPTH - 1 {
        parent = tree.insert(parent, Node::default()).expect("a level");
    }
    let count = tree.node_count();
    let crumbs = Breadcrumbs::new("Breadcrumbs", ["crates", "ui"]);
    assert!(matches!(
        crumbs.insert(&mut tree, parent),
        Err(UiError::TooDeep { .. })
    ));
    assert_eq!(tree.node_count(), count);
    assert_eq!(tree.children(parent).map(<[_]>::len), Some(0));
}

#[test]
fn breadcrumbs_with_a_blank_link_leave_the_tree_as_it_was() {
    let mut tree = Tree::new();
    let root = tree.root();
    let crumbs = Breadcrumbs::new("Breadcrumbs", ["", "home", "x"]);
    assert_eq!(
        crumbs.insert(&mut tree, root),
        Err(UiError::Unnamed {
            role: tidepool_ui::Role::Link
        })
    );
    assert_eq!(tree.children(root).map(<[_]>::len), Some(0));
    assert_eq!(tree.node_count(), 1);
}

/// Paints its node in the theme's `teal`.
struct Swatch;

impl Widget for Swatch {
    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        list.rect(cx.bounds(), cx.theme().color(Token::Teal));
    }
}

#[test]
fn the_tree_holds_the_theme_its_widgets_paint_with() {
    let mut tree = Tree::new();
    assert!(Arc::ptr_eq(tree.theme(), &Theme::bundled()));
    let node = Node::new(
        Style::stack()
            .width(Length::Fixed(10.0))
            .height(Length::Fixed(10.0)),
    );
    tree.insert(tree.root(), node.with_widget(Swatch))
        .expect("a node");
    tree.frame(WINDOW, &METRICS).expect("the first frame");
    // An equal theme changes nothing; another repaints the whole window.
    tree.set_theme(Arc::new(Theme::tidepool_dark()));
    assert_eq!(tree.frame(WINDOW, &METRICS), None);
    let pink = TIDEPOOL_DARK_TOML.replacen(
        "teal = { value = \"#5ccbcb\", ansi = 80 }",
        "teal = { value = \"#ff00ff\", ansi = 201 }",
        1,
    );
    let pink = Theme::parse(&pink, LIMITS).expect("a theme");
    tree.set_theme(Arc::new(pink));
    let frame = tree.frame(WINDOW, &METRICS).expect("a frame");
    assert_eq!(
        frame.damage,
        [Rect::new(0.0, 0.0, WINDOW.width, WINDOW.height)]
    );
    assert_eq!(
        colors(frame.list.commands()),
        [Color::rgb(0xff, 0x00, 0xff)]
    );
}

/// One random edit of a field, a click on it, or a frame.
#[derive(Clone, Debug)]
enum Edit {
    Type(String),
    Key(Key, Modifiers),
    Select(usize, usize),
    /// A press and release this many chars from the text's start.
    Click(f32, Modifiers),
    Frame,
}

/// Text to type: ASCII, combining marks, emoji with modifiers and joiners,
/// regional indicators, CR LF and controls.
const PIECES: &[&str] = &[
    "a",
    "Z",
    " ",
    "é",
    "e\u{301}",
    "\u{301}",
    "\u{1f44d}",
    "\u{1f3fd}",
    "\u{200d}",
    "\u{1f468}",
    "\u{1f1fa}",
    "\u{1f1f8}",
    "\r\n",
    "\n",
    "\t",
    "\u{7}",
    "\u{85}",
    "한",
    "\u{1100}\u{1161}",
    "\u{fe0f}",
    "\u{2028}",
];

const KEYS: &[Key] = &[
    Key::Left,
    Key::Right,
    Key::Home,
    Key::End,
    Key::Backspace,
    Key::Delete,
    Key::A,
    Key::Enter,
];

fn edits(rng: &mut Pcg32, size: u32) -> Vec<Edit> {
    (0..size.max(1))
        .map(|_| match rng.range(0..5u32) {
            // Now and then a paste of up to 3,000 bytes, so that the
            // limit is reached.
            0 if rng.range(0..8u32) == 0 => {
                Edit::Type("a\u{301}b".repeat(rng.range(1..751u32) as usize))
            }
            0 => {
                let pieces = rng.range(1..6u32);
                Edit::Type(
                    (0..pieces)
                        .map(|_| *rng.choose(PIECES).expect("pieces"))
                        .collect(),
                )
            }
            1 => {
                let modifiers = match rng.range(0..3u32) {
                    0 => Modifiers::NONE,
                    1 => Modifiers::SHIFT,
                    _ => Modifiers::CTRL,
                };
                Edit::Key(*rng.choose(KEYS).expect("keys"), modifiers)
            }
            2 => Edit::Select(rng.range(0..64u32) as usize, rng.range(0..64u32) as usize),
            3 => {
                let modifiers = if rng.range(0..2u32) == 0 {
                    Modifiers::NONE
                } else {
                    Modifiers::SHIFT
                };
                Edit::Click(rng.next_f32() * 24.0, modifiers)
            }
            _ => Edit::Frame,
        })
        .collect()
}

#[test]
fn random_edits_keep_a_field_one_line_on_cluster_boundaries() {
    check_sized("random field edits", 300, edits, |edits| {
        let (mut tree, id) = field(TextInput::new("Name"));
        for edit in edits {
            match edit {
                Edit::Type(typed) => {
                    tree.handle(&text(typed));
                }
                Edit::Key(pressed, modifiers) => {
                    tree.handle(&key_with(*pressed, *modifiers));
                }
                Edit::Select(from, to) => {
                    widgets::update::<TextInput, _>(&mut tree, id, |input| {
                        input.select(*from..*to)
                    })
                    .map_err(|error| error.to_string())?;
                }
                // Clicks land on the boundaries of the last paint, which
                // edits since may have made stale: no frame is drawn
                // between edits but for `Frame`.
                Edit::Click(chars, modifiers) => {
                    let at = Point::new(widgets::FIELD_PADDING + chars * CHAR, 15.0);
                    tree.handle(&down_with(at, *modifiers));
                    tree.handle(&up(at));
                }
                Edit::Frame => {
                    tree.frame(WINDOW, &METRICS);
                }
            }
            let input = tree.widget::<TextInput>(id).ok_or("no field")?;
            let value = input.text();
            if value.len() > MAX_INPUT_BYTES || value.chars().any(char::is_control) {
                return Err(format!("{:?}", value));
            }
            let selection = input.selection();
            for end in [selection.start, selection.end, input.caret()] {
                if !grapheme::is_boundary(value, end) {
                    return Err(format!("{end} is no boundary of {value:?}"));
                }
            }
            if tree.semantics(id).and_then(|s| s.value.as_deref()) != Some(value) {
                return Err("the node's value is not the text".to_string());
            }
        }
        Ok(())
    });
}
