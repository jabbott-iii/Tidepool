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
  `Token` (26; M3.6 adds eight for the controls, and M3.7 five for the
  collections, the card's tab and the code block's bar), with the solid
  color over `bg` and the 256-color index
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
- `widgets`: the collections (ADR-0052) — `Collection`, a list, tree
  view or table over a model of `Rows` its owner provides, which it asks
  for one `Row` at a time, only for the rows it shows (and a tree view's
  selected row, for Left and Right). Its rows, all 20 high, show in a
  view of at most 512 rows (`MAX_VIEW_ROWS`), which it measures no taller
  than; it keeps nodes only for the rows of that view on screen, cut to
  its clipping ancestors and the window, and two either side within the
  view, so at most 514 however it is laid out, in a scroller too, and
  binds them to other rows as each layout scrolls it, by its own offset:
  a million rows lay out and paint as a page does. The wheel scrolls it
  and passes on what it cannot take. It keeps a selection of one row,
  which Up, Down, Page Up, Page Down, Home, End and a press move and
  scroll into view, the least, within itself and by the scrollers around
  it (`ActionKind::Select`); Enter activates, and a tree view's Left and
  Right collapse, expand (`Collapse`, `Expand`, which the owner applies to
  its model) or move to the parent or first child; it stays on its row as
  the owner says rows were inserted or removed. The rows on screen are
  list items, tree items or rows with a cell for each `Column`, named as
  written by what they show, at most 4 KiB a cell (`MAX_CELL_BYTES`), and
  the collection's value is its row count and selection; a table's column
  headers take focus, if they are wider than 0, and activate for the
  owner to sort. Its focus outline is inside its bounds. A collection put
  in another's place builds its own parts. The table's header row and its
  rows' hover wash are two more tokens, `table-head` and `row-hover`; the
  selection is `teal-glow` — M3.7.
- `widgets`: M3.7's other components (ADR-0052) — `TabStrip`, a row of
  `Tab`s for panels (chrome, uppercase) or editors (file names, as
  written), one tab stop whose Left and Right (wrapping), Home and End
  select a tab, whose Delete, close controls and middle presses ask to
  close one (`Select`, `Dismiss`, about `target()`, which moves with its
  tab), its tabs in a track that scrolls sideways and scrolls the
  selected tab into view when it is selected or moves, or the track's
  width changes; a press finds a tab only where the last frame showed
  it, so never one the owner moved since; at most 256 tabs of at most 256
  bytes a label, and its name too; `Splitter`, the divider of a split it
  builds over `Layout::Split`, which keeps the ratio it was given or
  moved to and gives the split that ratio within the panes' minimums at
  each layout, so a window too small for it, then large again, has it
  back; dragged or stepped 10 units with the arrows along its axis, Home
  and End to the panes' minimums, emitting `Change`, its value the ratio
  shown as a percent; no position that is not finite drags it, and no
  key moves a split with no length; `Card`, a `ghost` box with a kicker,
  a title, a line of text and a right-edge tab (`READY`), holding its
  owner's buttons below them, which take focus as it does not, named by
  its title, its value the rest of what it shows; and `CodeBlock`, code
  of `CodeSpan`s each in a theme token, under a bar with its language
  (at most 64 bytes, `MAX_LANGUAGE_BYTES`) and a COPY button that
  activates, whose owner copies its own source; it keeps at most 200
  lines of 400 bytes and 32 spans, each line its start cut at a cluster
  boundary of the whole line, reading no more of a span than it keeps
  but the control characters it leaves out, and `CodeBlock::plain`
  splits lines at LF, CR LF and a lone CR (ADR-0019). After each layout
  pass, the tree binds these and the collections and passes again while
  that changes anything, so nested splits settle in one layout; only then
  do collections scroll a selection asked for, in the space they then
  have, and collections' reveals and the owner's `scroll_into_view` take
  effect, the owner's last. Moving a splitter repaints only what moves,
  and a divider binds only as the middle of its split's three children.
  Their
  looks, which the site gives the card and code block only, are in the
  spec; the card's tab and the code bar are three more tokens,
  `card-tab`, `on-card-tab` and `code-bar` — M3.7.

The tests check, among the rest, that children never overflow a parent
that does not scroll and that layout is idempotent and deterministic, over
random trees with random styles (`tests/layout.rs`), and that every
focusable node has a role and a name over random sequences of changes
(`tests/a11y.rs`); and that random edits of the bundled theme never make
its reader panic (`tests/theme.rs`); and that random edits of a field
keep its text one line within its limit and its caret and selection on
grapheme cluster boundaries (`tests/widgets.rs`, which also tests each
control's role, name, actions and colors); and that random keys,
presses, scrolls, resizes and model changes, at the root or in a
scroller, keep a collection's selection on a row, its nodes exactly those
of the rows on screen, every row named by what it shows and each reveal
the least (`tests/collections.rs`, which also tests the keys of the list,
tree view and table, a million rows kept as a page's nodes in and out of
a scroller, the damage of a scroll and the tokens each paints); and that
random inserts, removes, keys, presses and closes, with and without
frames between them, keep a tab strip's one valid selection, a target
that is one of its tabs and names its tab, and one tab stop
(`tests/components.rs`, which also tests the tab strip's, splitter's,
card's and code block's keys, actions, caps and looks, presses after the
owner moved tabs, nested splits, a resize after a clamp, drags cut short
or to no finite position, and a tab strip and a collection revealing in
one turn); and that random turns of nested splits, a tab strip, two
collections, owner reveals, wheels and resizes show what each turn asked
for, in the layout the splits settle on, and then idle
(`tests/settling.rs`). The fuzz target `theme`, in
`tests/fuzz.rs`, reads with small `Limits`, from seeds made by hand in
`fuzz/corpus/theme/`, some exactly at each limit, and from `themes/` in
place; it checks that a theme it reads keeps the format's rules and that
a refusal names a key on a line of the input (ADR-0027, ADR-0047).

`tidepool-gui` paints these draw lists into pixels with `gfx` and `font`,
measures text for them with `font` (its `FontMetrics`), and draws golden
frames of `ui`'s scenes (M3.5, ADR-0048), of each control in each state
(M3.6, ADR-0049), and of each of M3.7's components in each state
(ADR-0052).

Not yet, with the roadmap task that adds it:

- the overlays: tooltip, toast, modal and the command palette — M3.3.

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
  the states' looks); ADR-0052 (collections window their rows over their
  owner's model).
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
