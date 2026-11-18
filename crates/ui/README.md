# tidepool-ui

The UI toolkit: a retained widget tree, layout in logical units, focus and
input routing, the `tidepool-dark` theme in jabbott.dev's design language,
draw lists that both frontends render, and the accessibility tree. It knows
nothing about pixels, fonts or terminals.

It is part of [Tidepool](https://github.com/jabbott-iii/Tidepool), where
`editor` builds its views with it, and `tidepool-gui` and `tidepool-tui`
render its draw lists; it needs `tidepool-core`, whose geometry, colors and
input types it uses, `tidepool-codec`, whose TOML parser reads theme
files (ADR-0047), and `tidepool-text`, whose grapheme cluster boundaries
its fields edit by (ADR-0049).

Layer 2, services. It may use `core`, `codec` and `text`, as its row of
the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Version 0.1, not published (`publish = false`): the API changes as the
roadmap needs.

Built, with the roadmap task that built it:

- `tree`: `Tree`, an arena of nodes named by `NodeId`s (an index and a
  generation, so that an id kept after its node was removed names
  nothing; an id means something only in the tree that made it), at most
  256 levels deep and 1,048,576 nodes; each `Node` with
  a `Style`, `Semantics`, focusable and enabled flags and at most one
  `Widget`, which measures, paints and handles events, and whose state
  (hovered, pressed, focused, focus within, focus showing, disabled) the
  tree tracks as `NodeState` (ADR-0046) — M3.1.
- `layout`: rows, columns, stacks, grids and splits, with fixed, natural
  and filling lengths (shared by weight within their minimums and
  maximums, as CSS flexbox shares them), padding, gaps, alignment,
  clipping, opacity, hiding and scrolling, with `scroll_into_view`, which
  takes effect at the next layout, so that several focus moves, scrolls
  or changes between two frames reveal the last node asked for, from the
  offsets they left, and costs no pass when it scrolls nothing. The root
  fills the window, hidden or not. Every value is sanitized first, so NaN,
  infinite, negative and huge ones lay out without a panic, and children
  never overflow a parent that does not scroll, exactly in `f32`; natural
  sizes are remembered for a few hints per node, so a pass takes time
  about linear in the nodes times the depth — M3.1.
- `focus`: the tab order, `:focus-visible` (focus shows after the
  keyboard, not after a click), and moving focus forward and back,
  wrapping — M3.1.
- `input`: `Tree::handle` routes `core::input` events to the focused node,
  or to the node under the pointer or the one a press captured, and
  bubbles them up through the ancestors until a widget handles one; Tab
  moves focus and the wheel scrolls, chaining to outer scrollers, when no
  widget handles them; widgets emit actions for the owner. The hovered
  path follows the pointer, and is found again after each layout, so it
  follows content that scrolls under a pointer at rest. `hit_test`
  respects clips and paint order — M3.1.
- `metrics`: `TextStyle`, the `TextMetrics` trait the frontends implement
  (`gui` with `font`, M3.5; `tui` with cells, M7.1), and `FixedMetrics`
  for tests and headless use — M3.1.
- `draw`: `DrawList` and its commands (rects, cut-corner rects, pills,
  borders, bracket corners, lines, text, icons, images, clips and
  layers), in resolved colors (ADR-0046); `Tree::frame` repaints only the
  nodes whose data, state or layout changed, and returns the whole tree's
  list with the rects whose paint changed, at most 32, or `None` when
  nothing did. A widget's commands with NaN geometry are dropped (a clip
  with its own pop, so the clips around it hold), so list and damage
  agree; what waits for a frame stays bounded however many
  nodes come and go before it; and a text command's `Debug` shows its
  length only (ADR-0043) — M3.1.
- `a11y`: roles, `Semantics` and `Tree::accessibility`'s snapshot, with
  nodes of no role transparent and hidden ones left out. Every focusable
  node has a role and a name: the tree refuses a change that would break
  that. A field holding a secret leaves its value `None` — M3.1.
- `theme`: `Theme`, read from a theme file by `Theme::parse` within
  `codec`'s `Limits`, and `Theme::tidepool_dark`, the bundled
  `themes/tidepool-dark.toml` compiled in: a value for each color
  `Token` (26; M3.6 adds eight for the controls), with the solid color over `bg` and the 256-color index
  where the file gives them, the cut-corner size and the two fonts. A
  key that is missing or unknown, a wrong type, a color that does not
  parse, a `solid` for an opaque value, a translucent `bg`, an index out
  of 0–255, a `cut` outside 0–64 and a name that is blank or not one
  line of plain text are refused as `UiError::Theme`, naming the key (a
  name that is not a bare key quoted) and its line, and a file that is
  not TOML as `UiError::ThemeSyntax`. Its
  `[contrast]` table is `cargo xtask theme`'s, which prints the spec's
  contrast table from it and fails when a pair the theme uses for text is
  below WCAG AA (ADR-0047) — M3.2.
- `widgets`: the controls, in jabbott.dev's design language — `Button`
  (ghost, teal, amber with cut corners, slate; 30 or 24 high), `Chip`,
  `TextInput` (a text input or the search field: one line of at most
  4,096 bytes, moved in and deleted by grapheme cluster, with selection,
  a caret placed by the pointer, and a secret mode that keeps its text
  out of its semantics and `Debug`), `ScrollBar`, `focus_outline`,
  `StatusDot`, `SectionTitle`, `Eyebrow`, `Kicker` and `Breadcrumbs`,
  a builder of a navigation row of `Crumb`s. Each control is a
  `Control`, which gives its node its role and the name it is given,
  focusable or not; `widgets::update` changes one and its semantics, and a handler
  sets its node's through `EventContext::set_semantics`. Each paints its
  rest, hover, pressed, focus and disabled looks in the tree's theme,
  which `PaintContext::theme` gives it: the tree holds the bundled theme
  (`Theme::bundled`, read once) until `Tree::set_theme` replaces it and
  repaints every node. The site's component colors that its `:root`
  lacks are eight more tokens; pressed and disabled looks, and no
  blurred glows, are ADR-0049's — M3.6.

The tests check, among the rest, that children never overflow a parent
that does not scroll and that layout is idempotent and deterministic, over
random trees with random styles (`tests/layout.rs`), and that every
focusable node has a role and a name over random sequences of changes
(`tests/a11y.rs`); and that random edits of the bundled theme never make
its reader panic (`tests/theme.rs`); and that random edits of a field
keep its text one line within its limit and its caret and selection on
grapheme cluster boundaries (`tests/widgets.rs`, which also tests each
control's role, name, actions and colors). The fuzz target `theme`, in
`tests/fuzz.rs`, reads with small `Limits`, from seeds made by hand in
`fuzz/corpus/theme/`, some exactly at each limit, and from `themes/` in
place; it checks that a theme it reads keeps the format's rules and that
a refusal names a key on a line of the input (ADR-0027, ADR-0047).

`tidepool-gui` paints these draw lists into pixels with `gfx` and `font`,
measures text for them with `font` (its `FontMetrics`), and draws golden
frames of `ui`'s scenes (M3.5, ADR-0048) and of each control in each
state (M3.6, ADR-0049).

Not yet, with the roadmap task that adds it:

- collections and overlays: list, tree view, table, tab strip, splitter,
  card, code block, tooltip, toast, modal and the command palette —
  M3.3.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Usage

A workspace crate whose row of the matrix allows it depends on it by path:

```toml
[dependencies]
tidepool-ui = { path = "../ui" }
```

Build a side panel, an editor area and a button in the bundled theme's
colors, lay them out, move focus with Tab, take a frame and read the
accessibility tree:

```rust
use tidepool_core::color::Color;
use tidepool_core::geom::{Rect, Size};
use tidepool_core::input::{InputEvent, Key, Modifiers};
use tidepool_ui::a11y::{Role, Semantics};
use tidepool_ui::draw::{Command, DrawList, PaintContext};
use tidepool_ui::layout::{Length, Style};
use tidepool_ui::metrics::FixedMetrics;
use tidepool_ui::theme::{Theme, Token};
use tidepool_ui::{Node, Tree, UiError, Widget};

/// Fills its node with one color.
struct Fill(Color);

impl Widget for Fill {
    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        list.rect(cx.bounds(), self.0);
    }
}

fn main() -> Result<(), UiError> {
    // `gui` measures with its fonts and `tui` in cells; this is for tests.
    let metrics = FixedMetrics::DEFAULT;
    let window = Size::new(800.0, 600.0);
    // The bundled theme's `editor-bg` and `amber`.
    let theme = Theme::tidepool_dark();
    let (editor_bg, amber) = (theme.color(Token::EditorBg), theme.color(Token::Amber));
    assert_eq!(amber, Color::rgb(0xf2, 0xc2, 0x00));

    let mut tree = Tree::new();
    let root = tree.root();
    tree.set_style(root, Style::row())?;
    tree.set_semantics(root, Semantics::new(Role::Window, "Tidepool"))?;
    // A side panel 240 wide, and an editor area that fills the rest.
    let side = Style::column().width(Length::Fixed(240.0)).height(Length::Fill(1.0));
    let side = tree.insert(root, Node::new(side))?;
    let editor = tree.insert(root, Node::new(Style::stack().fill()).with_widget(Fill(editor_bg)))?;
    // A focusable node needs a role and a name.
    let save = Node::new(Style::stack().fixed(96.0, 30.0))
        .with_semantics(Semantics::new(Role::Button, "Save"))
        .focusable()
        .with_widget(Fill(amber));
    let save = tree.insert(side, save)?;

    tree.layout(window, &metrics);
    assert_eq!(tree.bounds(side), Some(Rect::new(0.0, 0.0, 240.0, 600.0)));
    assert_eq!(tree.bounds(editor), Some(Rect::new(240.0, 0.0, 560.0, 600.0)));
    assert_eq!(tree.bounds(save), Some(Rect::new(0.0, 0.0, 96.0, 30.0)));

    // No widget handles Tab, so focus moves to the button, and shows.
    let tab = InputEvent::KeyDown { key: Key::Tab, modifiers: Modifiers::NONE, repeat: false };
    assert!(tree.handle(&tab).handled);
    assert_eq!(tree.focused(), Some(save));
    assert!(tree.focus_visible());

    // The first frame damages the whole window and lists what each widget
    // painted, in pre-order; an idle frame is `None`.
    let frame = tree.frame(window, &metrics).expect("the first frame draws");
    assert_eq!(frame.damage, vec![Rect::new(0.0, 0.0, 800.0, 600.0)]);
    assert_eq!(frame.list.commands(), &[
        Command::Rect { rect: Rect::new(0.0, 0.0, 96.0, 30.0), color: amber },
        Command::Rect { rect: Rect::new(240.0, 0.0, 560.0, 600.0), color: editor_bg },
    ]);
    assert_eq!(tree.frame(window, &metrics), None);

    assert_eq!(
        tree.accessibility().to_string(),
        "window \"Tidepool\"\n  button \"Save\" focusable focused\n"
    );
    Ok(())
}
```

## Documentation

- Design:
  [`docs/subsystems/ui-and-theme.md`](../../docs/subsystems/ui-and-theme.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0004 (the
  jabbott.dev design language); ADR-0046 (widgets in the tree's nodes,
  routing up through ancestors, and resolved colors in draw lists);
  ADR-0047 (theme files read with `codec::toml`, their format, and the
  pairs `cargo xtask theme` checks); ADR-0049 (the controls: the tree
  holds the theme, component tokens, grapheme editing with `text`, and
  the states' looks).
- API reference: `cargo doc -p tidepool-ui --open`

## Rules

- `unsafe` is denied by the workspace lint, and in doc tests by the crate
  root's `#![doc(test(attr(forbid(unsafe_code))))]` (ADR-0036);
  `cargo xtask unsafe-audit` checks both.
- Path-scoped rules in `.claude/rules/`:
  [`ui-style.md`](../../.claude/rules/ui-style.md),
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.

## Checks

```bash
cargo test -p tidepool-ui
cargo xtask theme
cargo xtask fuzz-smoke --target theme
cargo xtask golden    # ui's scenes and controls, which tidepool-gui paints (M3.5, M3.6)
```

`cargo xtask ci` runs the whole gate.

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
