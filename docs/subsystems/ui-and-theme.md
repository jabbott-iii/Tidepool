# Subsystem: `tidepool-ui` and the jabbott.dev design language

`tidepool-ui` is the toolkit every view is built from. Its default theme,
`tidepool-dark`, follows jabbott.dev (ADR-0004). The reference is the site's
stylesheet (`jabbott-iii/jabbott-iii.github.io`, `assets/css/style.css`),
whose own header calls it a dark "capsuleer UI": deep-space backdrop,
translucent hairline panels, teal signal accents and squared uppercase type.

`ui` depends on `core`, on `codec`, whose TOML parser reads theme
files (ADR-0047), and on `text`, whose grapheme cluster boundaries its
fields edit by (ADR-0049). It knows nothing about pixels, fonts or
terminals: it lays out in logical units and emits draw lists that `gui`
and `tui` render. Widgets live in the tree's nodes, which routes events to them
and up through their ancestors, and draw lists carry resolved colors
(ADR-0046).

## Contents

M3.1 built `tree`, `layout`, `focus`, `input`, `metrics`, `draw` and
`a11y`, M3.2 `theme`, M3.6 the controls of `widgets`, and M3.7 its list,
tree view, table, tab strip, splitter, card and code block; its overlays
are planned (M3.3), and no roadmap task builds `anim` yet. The rustdoc of each built module is authoritative; its row
summarizes it.

| Module | Provides |
| :- | :- |
| `tree` | `Tree`, a retained tree of nodes named by `NodeId`s (an index and a generation, so a removed node's id names nothing), at most `MAX_DEPTH` (256) levels and `MAX_NODES` (2^20) nodes. Each `Node` has a `Style`, `Semantics`, focusable and enabled flags and at most one `Widget`, which measures, paints and handles events (ADR-0046); the tree tracks each node's `NodeState`: hovered, pressed, focused, focus within, focus visible, disabled |
| `layout` | Row, column, stack, grid and split layout in logical units, with fixed, natural (`Content`) and filling lengths, minimums and maximums, padding, gaps, alignment, clipping, opacity, hiding and scrolling; every value is sanitized first, and a child never overflows a parent that does not scroll, exactly in `f32`. `scroll_into_view` takes effect at the next layout |
| `focus` | The tab order (focusable, enabled, visible nodes in pre-order), `focus_next` and `focus_previous`, wrapping, and `focus-visible`: shown after a key, not after a click |
| `input` | `Tree::handle`: key and text events go to the focused node, pointer events to the node under the pointer (or the one a press captured), then up through their ancestors until a widget handles one; the defaults, Tab moving focus and the wheel scrolling, come after; widgets emit actions for the owner, and what none handled comes back to it. `hit_test` respects clips and paint order |
| `metrics` | `TextStyle` and the `TextMetrics` trait (implemented by `gui` with `font`, M3.5, and by `tui` with cells, M7.1), and `FixedMetrics` for tests and headless use |
| `draw` | `DrawList` of `Command`s in logical units and resolved `core::Color`s (ADR-0046), which a widget takes from the tree's theme through `PaintContext::theme` (ADR-0049): rect, cut-corner rect, pill, border, bracket corners, line, text run, icon (an `IconId`), image, clip push/pop, layer opacity. `Tree::frame` repaints only the nodes whose data, state or layout changed, and returns the whole tree's list with the damage, the rects whose paint changed, or `None` when nothing did |
| `theme` | `Theme`, the token set read from a theme file (`themes/*.toml`, the format of ADR-0047) by `Theme::parse` within `codec::Limits`, and the bundled `tidepool-dark` (`Theme::tidepool_dark`): a value for each `Token`, with its solid color and 256-color index where the file gives them, `cut` and the two fonts; a key that is missing or unknown, a wrong type, a color that does not parse or a translucent `bg` is refused, naming the key and, unless it is missing, its line. The tree holds the theme its widgets paint with (`Tree::theme`, `Tree::set_theme`, which repaints every node), the bundled one by default, read once (`Theme::bundled`); the controls' component colors are tokens too (ADR-0049) |
| `widgets` | The controls (M3.6, ADR-0049): `Button` (ghost, teal, amber, slate; 30 or 24 high), `Chip`, `TextInput` (a text input or the search field, one line edited by grapheme cluster), `ScrollBar`, `StatusDot`, `SectionTitle`, `Eyebrow`, `Kicker` and `Crumb`, each a `Control` that gives its node a role and the name it is given, changed with `widgets::update`; `Breadcrumbs`, which builds a navigation row of crumbs; and `focus_outline`, which the focusable controls paint. The collections (M3.7, ADR-0052): `Collection`, a list, tree view or table of its owner's `Rows`, asked for one `Row` at a time, whose rows, all `ROW_HEIGHT` high, show in a view of at most `MAX_VIEW_ROWS` rows (10,240 units, which it measures no taller than); it keeps nodes only for the rows of that view on screen (cut to its clipping ancestors and the window, in a scroller too) and `ROW_MARGIN` either side within the view, so at most `MAX_VIEW_ROWS` and two, binds them to other rows as each layout scrolls it by its own offset, and keeps a selection of one row that the keyboard and the pointer move and scroll into view, within itself and by the scrollers around it (actions `Select`, `Activate`, and a tree view's `Expand` and `Collapse`); what a row shows and names is at most `MAX_CELL_BYTES` a cell, and a table's row is named by the cells its columns show; a table's `Column`s have headers that activate. With them (M3.7, ADR-0052): `TabStrip` of `Tab`s (`TabKind` chrome or content), one tab stop whose arrows, Home and End select a tab, wrapping, Delete or a close control or a middle press asking to close one (`Select`, `Dismiss`, about `target()`), the tabs in a track that scrolls sideways, which itself scrolls the selected tab into view when it is selected or moves, or the track's width changes, a press finding a tab only where the last frame showed it, and the target moving with its tab, at most `MAX_TABS` and `MAX_TAB_LABEL_BYTES` a label and the strip's name; `Splitter`, the divider of a `Layout::Split`, dragged or stepped with the arrows along its axis, Home and End, emitting `Change`, which keeps the ratio it was given or moved to and gives the split that ratio kept within the panes' minimums at each layout, so that a split too small for it has it back with room, and moves for no position that is not finite and no key before the split has a length; `Card`, a box with a kicker, title, line of text and a right-edge tab, holding its owner's buttons, named by its title, its value the rest; and `CodeBlock` of `CodeSpan`s in the theme's tokens, under a bar with its language (cut to `MAX_LANGUAGE_BYTES`) and a COPY button that activates, whose owner copies its own source, keeping at most `MAX_CODE_LINES` lines of `MAX_CODE_LINE_BYTES` and `MAX_CODE_SPANS`, each line's start cut at a cluster boundary of the whole line (past the span limit too), reading no more of a span than it keeps but the control characters it leaves out, and splitting plain text at ADR-0019's line ends. After each layout pass the tree binds these widgets and the collections, and passes again while that changes anything, at most eight times, so nested splits settle in one layout; only then do collections scroll a selection asked for, in the space they then have, and the tree reveal their rows and the owner's `scroll_into_view` in turn, the owner's last, from that settled layout, settling the binding again after. A splitter's move repaints only what moves, and a divider binds only as the middle of its split's three children. Planned (M3.3): tooltip, toast, modal, command palette |
| `a11y` | Accessibility tree: role, name, value, state per node (exposed to OS bridges in M14.2); `Tree::accessibility`'s snapshot leaves out nodes of no role and hidden ones |
| `anim` | Optional, timer-driven transitions (hover fades, status pulse); off when reduced motion is set |

## Invariants

- Layout is deterministic for the same tree, metrics and size.
- Every focusable widget has a role and an accessible name: the tree
  refuses a focusable node without them (M3.1). Every control, focusable
  or not, gives its node its role and the name its owner gives it
  (`Control::node`, M3.6); the owner names the ones that take no focus,
  the scrollbar among them, which the tree cannot check.
- A secret (a password, a key) is never a node's `Semantics` value, and
  `Debug` shows a text a widget paints, or a node's value, as its length
  only (ADR-0043).
- Widgets never read colors directly; they take a token's value from the
  tree's `Theme`, which `PaintContext::theme` gives them as they paint
  (ADR-0049). A widget never holds a theme; `Tree::set_theme` repaints
  every node.
- **Uppercase is for chrome only.** Labels, eyebrows, buttons, table
  headers and section titles are uppercase with letter-spacing, as on the
  site. User content (file names, paths, code, symbols, search text,
  commit messages) is never case-transformed.
- An idle UI emits no damage. Animations are opt-in, timer-based and stop
  when the window is unfocused.

## Tokens (`themes/tidepool-dark.toml`)

Values are taken from jabbott.dev's `:root`. "Solid" values are the
translucent tokens composited over `bg`, for surfaces drawn without a
backdrop and for the TUI. `on-amber`, `on-teal` and `comment` are the
colors the contrast rules below name (ADR-0047). The last 13, from
`ghost` on, are the colors the site's components use beyond its `:root`,
which the controls (ADR-0049) and the collections (ADR-0052) take as
tokens. The file writes each
color as `value`, `solid` and `ansi` (the 256-color index), `cut` under
`[sizes]` and the fonts under `[fonts]`; `ui::theme`'s rustdoc is the
format's reference.

| Token | Value | Solid | 256-color | Use |
| :- | :- | :- | :- | :- |
| `bg` | `#0b0c0d` | — | 232 | Window background |
| `bg-raise` | `#121517` | — | 233 | Raised surfaces |
| `panel` | `rgba(16,19,21,0.78)` | `#0f1113` | 233 | Side panels, palette, popups |
| `panel-solid` | `#14181a` | — | 234 | Menus, tooltips |
| `slate` | `#29353a` | — | 236 | Inputs, chips, slate buttons |
| `slate-2` | `#1b2326` | — | 235 | Media wells, image viewer |
| `line` | `rgba(255,255,255,0.08)` | `#1f1f20` | 234 | Hairline borders, separators |
| `line-2` | `rgba(255,255,255,0.16)` | `#323334` | 236 | Stronger borders, code blocks |
| `text` | `#eef2f3` | — | 255 | Primary text |
| `text-2` | `#b3bdc1` | — | 250 | Secondary text |
| `text-3` | `#7d898e` | — | 102 | Muted text, eyebrows, line numbers |
| `teal` | `#5ccbcb` | — | 80 | Accent: links, focus, active state, keywords |
| `teal-hi` | `#8fe5e5` | — | 116 | Hover and active text |
| `teal-wash` | `rgba(92,203,203,0.10)` | `#131f20` | 234 | Teal button fill, hover wash |
| `teal-glow` | `rgba(92,203,203,0.35)` | — | — | Glows, selection (`#275152` on the editor) |
| `amber` | `#f2c200` | — | 220 | Primary action, warnings |
| `amber-hi` | `#ffd84a` | — | 221 | Primary action hover |
| `green` | `#3ddc97` | — | 42 | Ready / success / git added |
| `rust` | `#f09a4a` | — | 215 | Numbers in code; Rust-flavored accents |
| `violet` | `#a58cff` | — | 141 | Attributes, properties |
| `error` | `#ff8a80` | — | 210 | Errors, git deleted |
| `editor-bg` | `#0a0f11` | — | 233 | Editor and code blocks (site's code-block background) |
| `bar` | `rgba(30,37,40,0.72)` | `#191e20` | 234 | Status bar, command bar |
| `on-amber` | `#111111` | — | — | Text on `amber`: the primary button's label |
| `on-teal` | `#071012` | — | — | Text on `teal`: an active chip's label |
| `comment` | `#6f7d82` | — | — | Comments in code, only on `editor-bg` |
| `ghost` | `rgba(255,255,255,0.03)` | `#121314` | 233 | Ghost button fill |
| `teal-line` | `rgba(92,203,203,0.55)` | `#387576` | 66 | Teal button border; hovered chip and field border |
| `teal-fill` | `rgba(92,203,203,0.20)` | `#1b3233` | 235 | Teal button hover fill |
| `slate-hi` | `#34444a` | — | 238 | Slate button hover fill; pressed chip |
| `teal-ring` | `rgba(92,203,203,0.15)` | `#17292a` | 235 | Focused field's 3 px ring |
| `green-halo` | `rgba(61,220,151,0.16)` | `#132d23` | 235 | Status dot halo: ready |
| `amber-halo` | `rgba(242,194,0,0.16)` | `#30290b` | 235 | Status dot halo: busy |
| `error-halo` | `rgba(255,138,128,0.16)` | `#32201f` | 235 | Status dot halo: failed |
| `table-head` | `rgba(41,53,58,0.60)` | `#1d2528` | 235 | Table header row (`slate` at 60%) |
| `row-hover` | `rgba(255,255,255,0.025)` | `#111213` | 233 | Hovered row of a list, tree view or table |
| `card-tab` | `#e8f3f3` | — | 255 | A card's tab, its pill label |
| `on-card-tab` | `#0b1416` | — | 233 | Text on `card-tab` |
| `code-bar` | `rgba(41,53,58,0.45)` | `#181e21` | 234 | A code block's bar (`slate` at 45%) |
| `cut` | 10 px | — | — | Cut-corner size for primary buttons |
| `font-ui` | Saira | — | — | UI chrome |
| `font-mono` | JetBrains Mono | — | — | Code, terminal, numbers, paths |

## Contrast (WCAG 2.x, measured)

Computed from the values above, with WCAG 2.x's relative luminance. `cargo
xtask theme` prints this table from the theme file, and a test compares it
with this one byte for byte. The file's `[contrast]` names the table's
rows and columns, and its `[[contrast.use]]` entries the pairs the theme
uses for text, each with its size: the check fails if one drops below its
minimum (4.5:1 for normal text, 3:1 for large text and UI glyphs),
unrounded (ADR-0047). A translucent surface is its solid value over `bg`,
and otherwise composited over the color the entry names, as for the
editor's selection, `teal-glow` over `editor-bg`.

| Text token | on `bg` | on `editor-bg` | on `panel` (solid) | on `slate` |
| :- | :- | :- | :- | :- |
| `text` | 17.37 | 17.10 | 16.79 | 11.19 |
| `text-2` | 10.22 | 10.07 | 9.88 | 6.59 |
| `text-3` | 5.45 | 5.36 | 5.26 | **3.51 (large only)** |
| `teal` | 10.12 | 9.97 | 9.78 | 6.52 |
| `amber` | 11.65 | 11.47 | 11.26 | 7.50 |
| `error` | 8.58 | 8.44 | 8.29 | 5.53 |
| `comment` | 4.60 | 4.53 | **4.44 (large only)** | **2.96 (fails)** |

Rules that follow:

- On `slate` (inputs, chips), placeholder and muted text use `text-2`, not
  `text-3` as the site does.
- The comment color is used only on `editor-bg`. Code shown on other
  surfaces (hover popups, completion docs) uses an `editor-bg` well, so the
  same palette stays legible.
- Dark text on amber (`#111111`, 11.23) and on teal (`#071012`, 9.95) meets
  AA; use those exact values.

## Typography

| Role | Face | Size / line height | Weight | Case / tracking |
| :- | :- | :- | :- | :- |
| Brand (title bar) | Saira | 15 px | 600 | Uppercase, 0.28em |
| Brand subline (workspace) | Saira | 10 px | 500 | Uppercase, 0.42em, `text-3` |
| Section title | Saira | 13 px | 500 | Uppercase, 0.04em, 3 px `teal` left rule |
| Eyebrow (panel group label) | Saira | 11 px | 600 | Uppercase, 0.28em, `text-3` |
| Kicker (category over a title) | Saira | 11 px | 600 | Uppercase, 0.16em, `teal` |
| Button / chip / tab label | Saira | 12 px | 600 | Uppercase, 0.10em (chip 0.08em) |
| Table header | Saira | 11 px | 600 | Uppercase, 0.10em, `teal` |
| UI body (lists, tree, dialogs) | Saira | 13 px / 1.5 | 400 | As written |
| Code editor, terminal | JetBrains Mono | 14 px / 1.5 (setting) | 400; 600 for emphasis; italic for comments | As written |
| Status bar, paths, counts | JetBrains Mono | 12 px | 400 | As written |

The site's sizes are larger (16 px body); the IDE uses a denser scale with
the same ratios and tracking. Users can scale the whole UI.

## Components

| Component | jabbott.dev source | In Tidepool |
| :- | :- | :- |
| Header with glow rule | `.site-header`, `::after` gradient | Title bar strip: brand mark (teal) + `TIDEPOOL` + workspace subline; 1 px `line` bottom border with a centered `teal` gradient at 40% opacity |
| Panel | `.panel` | `panel` fill (over the cached backdrop), 1 px `line` border, no radius |
| Bracket corners | `.panel--brackets` | 16 px `teal` corner brackets (top-left, bottom-right) on the focused panel, command palette and modals |
| Section title | `.section-title` | Panel headers: 3 px `teal` left rule, uppercase |
| Eyebrow | `.eyebrow` | Group labels in side panels (`EXPLORER`, `OPEN EDITORS`) |
| Buttons | `.btn`, `--ghost`, `--teal`, `--amber`, `--slate` | Ghost (default), teal (secondary), amber (primary, with `cut` corners: top-right and bottom-left), slate (quiet). Heights 30 / 24 px |
| Chips | `.chip`, `.is-active` | Search filters, problem-severity filters, language pills; fully rounded; active = `teal` fill with `#071012` text |
| Search field | `.search input` | Pill on `slate`; focus = `teal` border + 3 px glow at 15% |
| Card + tab | `.card`, `.card__tab` | Toolchain, device and agent cards: a `ghost` box with a 1 px `line` border, 12 px in; a kicker (Saira 11 px 500, 0.06em, uppercase, `text-3`), the title (Saira 16 px 500 / 1.3, `text`) and a line of text (the UI body, `text-2`), then its buttons; a right-edge tab 8 px down, 18 high (`READY`, `UPDATE`: Saira 10 px 700, 0.10em, uppercase, `on-card-tab` on `card-tab`, rounded on the left, with a 4 px `teal` stripe on its right edge). Hovered: a `teal-line` border, a 1 px `teal-fill` ring outside it, the title `teal-hi`. It takes no focus; its buttons do. The site's media well, image and soft shadow are left out |
| Status dot | `.status-dot` | Toolchain/LSP/agent state: `green` ready, `amber` busy or outdated, `error` failed, `text-3` off. Pulse only when motion is on |
| Table | `.prose th/td` | Problems, SDK packages, devices, git changes: uppercase `teal` headers (Saira 11 px 600, 0.10em) on `table-head` (`slate` at 60%) with a 1 px `teal` underline, each a focusable column header the owner sorts by; cells in the UI body, as written; `line` row rules; hover wash `row-hover` (2.5% white); the selected row `teal-glow`. Rows 20 high; column widths are the owner's |
| List | — (the site has none) | Open editors, search results, outline: rows 20 high of the UI body in `text`, as written, 8 px in; hover `row-hover`; the selected row `teal-glow`, its text still `text` |
| Tree view | — (the site has none) | Explorer, symbols: the list's rows, indented 16 px a level, with a `text-2` chevron (`›` collapsed, `⌄` expanded) in a 16 px box before the text |
| Code block | `.highlighter-rouge`, `.code-bar` | Markdown preview and hover docs: `editor-bg`, 1 px `line-2` border, 2 px `teal` left border, a code bar 28 high of `code-bar` (`slate` at 45%) with a 1 px `line` rule below, the language (JetBrains Mono 10 px, 0.12em, uppercase, `text-3`) and COPY (`.code-copy`: Saira 11 px 600, 0.10em, uppercase, `teal` in a 1 px `teal-glow` border; hovered `teal-wash` with a `teal-hi` label, pressed `teal-fill`). Code in JetBrains Mono 13 px / 1.5, 12 px in and 10 below the bar, each span in the theme token its owner gives: no syntax tokens of its own, as the "Editor colors" below are the editor's |
| Tab strip | — (the site has none) | Panel tabs (`PROBLEMS`, `TERMINAL`: the button label, uppercase) and editor tabs (file names in the UI body, as written), 30 high, 12 px either side, closable ones with a 16 px close control 6 px after the label (a `text-2` cross); labels `text-2`, the selected tab's `text` over a 2 px `teal` rule, a 1 px `line` rule under them all |
| Splitter | — (the site has none) | The divider between two panes, 8 wide: a 1 px `line` rule down its middle |
| Breadcrumbs | `.breadcrumbs` | Editor breadcrumb: uppercase separators `/` in `teal`, path segments as written |
| Focus | `:focus-visible` | 2 px `teal` outline, 3 px offset, keyboard focus only; a field shows its `teal` border and 3 px `teal-ring` instead, whenever it has focus; a collection and a column header draw it inside their bounds instead, as they fill a panel or the window, which would cut an outline outside them off |
| Selection | `::selection` | `teal-glow` (35%); on `editor-bg` that is `#275152`; text stays `text` (7.81:1) |
| Backdrop | `.space` (radial gradients, planet, stars, grid) | Welcome screen and empty editor area only. Rendered once per size into the backdrop cache; the star drift animation is off unless `ui.ambient_motion = true` |

The site has no pressed, disabled, plain text input, scrollbar, list,
tree view, tab strip or splitter style, and paints glows with blurred `box-shadow`s; the
controls' states are ADR-0049's, and the collections' ADR-0052's. A
control paints from its node's `NodeState`:

| State | Look |
| :- | :- |
| Rest | As the Components table above; a text input is a `slate` rect, its text `text` and its placeholder `text-2`; a scrollbar has no track and a `line-2` thumb, a pill 6 wide in its 10 and at least 24 long; a row of a list, tree view or table has no fill, and the selected row is `teal-glow` in every state; a tab and a card as the Components table above; a splitter's `line` rule |
| Hover | The site's `:hover`: ghost gains a `teal` border and a `teal-hi` label; teal a `teal-fill` fill and a `teal` border; amber is `amber-hi`; slate `slate-hi` with a `teal-hi` label; a chip and a field gain a `teal-line` border, but a selected chip turns `teal-hi`; a scrollbar gains a `line` track and a `teal-glow` thumb; a link's text is `teal-hi`; a row gains the `row-hover` wash (`.prose tbody tr:hover`), and a column header's title is `teal-hi`; a tab gains the `row-hover` wash and a `teal-hi` label, and a close control a `teal-wash` circle and a `teal-hi` cross; a splitter shows a 2 px `teal-glow` bar; a card its `teal-line` border, `teal-fill` ring and `teal-hi` title; COPY `teal-wash` with a `teal-hi` label |
| Pressed (the task's "active") | The hover look with the fill one step stronger: ghost `teal-wash`, teal `teal-glow`, a chip `slate-hi`; a slate button gains a `teal-line` border; an amber button and a selected chip keep their rest fill inside a 2 px rim of their hover fill; a scrollbar's thumb is `teal`; a link's text is `teal`; a column header is `slate` above its underline, its title `teal-hi`; a press on a row selects it, so it shows the selection; a tab is `teal-wash`, and selected by the press; a close control is on `teal-fill`; a splitter, pressed or dragged, shows a 2 px `teal` bar; COPY is `teal-fill` |
| Focus | The focus outline when focus shows; a field's border and ring whenever it has focus, with its caret, which does not blink; a collection's outline is 2 px inside its bounds, in the 2 px of padding its default style leaves so no row covers it, and its selected row stays `teal-glow`; a column header's, a tab strip's (in the same padding) and a splitter's are inside their bounds too; a card shows none, as its buttons take focus |
| Disabled | The rest look in a layer at 40% opacity, with no hover or pressed look (WCAG exempts inactive controls from its minimums) |

The ghost button's inset glow, the teal button's hover glow and the status
dot's soft glow are left out, as no draw command blurs; the field's focus
ring and the dot's halo are solid, translucent shapes behind it.

## Editor colors

From the site's syntax palette (`.highlight .*`):

| Scope | Color | Style |
| :- | :- | :- |
| Comment, doc comment | `#6f7d82` | Italic |
| Keyword, built-in type, macro bang | `teal` `#5ccbcb` | — |
| String, char, byte string | `#e9c46a` | — |
| Number | `rust` `#f09a4a` | — |
| Variable, constant, static | `#9fc7ff` | — |
| Function, type, module, tag | `text` `#eef2f3` | — |
| Attribute, property, field, lifetime | `violet` `#a58cff` | — |
| Operator, punctuation | `#9aa7ab` | — |
| Invalid | `#ff8a80` | — |
| Markdown heading | `teal-hi` | 600 |
| Markdown link | `teal` | Underline on hover |
| Markdown inline code | `#a9ecec` on 8% teal with a 16% teal border | — |

| Editor element | Value |
| :- | :- |
| Background | `editor-bg` |
| Current line | 2.5% white wash |
| Line numbers | `text-3`; current line `text-2` |
| Cursor | `teal` bar (insert), `teal` block at 60% (vim normal mode) |
| Matching bracket | 1 px `teal` outline |
| Ghost text (AI / prediction) | `text-3`, italic |
| Diagnostics | error `error`, warning `amber`, info `teal`, hint `text-3` dotted |
| Git gutter | added `green`, modified `amber`, deleted `error` |
| Search match | `amber` at 25%; current match 1 px `amber` outline |

## TUI mapping

- Truecolor when `COLORTERM` is `truecolor` or `24bit`; otherwise the
  256-color column above.
- Translucency uses the solid values. Cut corners and bracket corners
  become box-drawing corners (`┌ ┘`) in `teal`. Section titles use a `▌`
  in `teal` before the title.
- Uppercase and tracking: uppercase is kept; letter-spacing is dropped.
- The status dot is `●`.

## Tests

- Golden frames for every widget in each state (rest, hover, focus,
  active, disabled) at scale 1 and 2: the controls' 16 scenes, painted by
  `gui` (`crates/gui/tests/controls/mod.rs`, M3.6), each a grid of a row
  for each variant and a column for each state, every cell its own tree
  put in its state by real events; and M3.7's 14 scenes, painted by `gui`
  (`crates/gui/tests/components/mod.rs`): the list, tree view, table, tab
  strip, splitter, card and code block, each a grid of a row for each
  variant and a column for each state, with each one's own (a selected
  row or tab, a column header hovered and pressed, the splitter dragged,
  COPY hovered and pressed), whose references are blessed once the owner
  has looked at them; the overlays' come with M3.3.
- The controls' behavior (`tests/widgets.rs`, M3.6): each one's role and
  name, what activates or changes it, the tokens it paints in each state,
  fields edited by grapheme cluster, the scrollbar dragged and paged, the
  tree's theme; and a property test of random edits, after each of which
  a field's text is one line within its limit and its caret and
  selection on cluster boundaries.
- The collections' behavior (`tests/collections.rs`, M3.7, ADR-0052):
  the keys of the list, tree view and table, at their ends, with no rows
  and with the selection scrolled away; a million rows kept as nodes only
  for the rows on screen, at the start, the middle and the end, the model
  asked for those rows only, and in a scroller too, where a 64-column
  table leaves the tree room, as it does in a window a million units
  high; every row on screen with its role and a name from what it shows,
  each cell capped; the selection scrolled into view the least, also in
  the turn the window resizes or the collection is shown again, and by
  the scrollers around; a press selecting the row drawn under it, and idle
  frames that pass no layout; scrolling, hovering and selecting damaging
  only the rows they change; the wheel passing on what the rows cannot
  take, and over a disabled collection; changes to the model keeping the
  selection on its row; a collection put in another's place building its
  own parts; two collections and the owner each revealing what they asked
  for in one turn; row nodes kept while hidden; the focus outline inside the
  bounds; the tokens each paints in each state, and the tree's theme; and
  a property test of random keys, presses, scrolls, resizes and model
  changes, several to a frame, at the root or in a scroller, after which
  the selection is a row, the rows kept and the nodes are exactly those
  the window gives, every focusable node and row is named by what it
  shows, a selection asked for is scrolled the least and shows, and an
  idle frame draws nothing and passes no layout.
- The other components' behavior (`tests/components.rs`, M3.7,
  ADR-0052): the tab strip's keys, wrapping, presses, closes, overflow,
  caps, damage and looks, a press after the owner moved tabs and before a
  frame, its target through the owner's changes, its reveal after a
  resize, an insert or a label and beside a collection's in either order,
  with a property test of random inserts, removes, keys, presses and
  closes, with and without a frame after each, after each of which the
  selection and the target are tabs, the target names its tab until it
  is removed, and, after a frame, there is a node and a name for each tab
  and one tab stop, and an idle frame draws nothing; the splitter's keys
  along each axis, its drag, the panes' minimums, a resize after a clamp,
  nested splits, drags to no finite position, cut short or hidden, keys
  with no length, its name's cap, the damage of a move, a second divider
  in a split, its value and looks; the card's role,
  value, layout, tab and looks; and the code block's COPY, spans, caps
  (many spans to a line, the language) and looks; with unit tests of the
  cut within the text's head, the bounded cleaning of code, its lines and
  the tree's list of bound widgets.
- The bound widgets together (`tests/settling.rs`, M3.7, ADR-0052): a
  reveal, the owner's or a collection's, taking effect in the layout the
  splitters settle on in the same turn, at a resize, with a key and in a
  restored session's first frame; and a property test of random turns of
  nested splits, a tab strip, two collections (one in a scroller with
  other content), owner reveals, wheels and resizes, after each of which
  every split has its ratio in the space it has, the selected tab shows,
  a selection or node asked for shows, and the next frame is idle and
  passes no layout.
- Layout property tests: children never overflow a non-scrolling parent;
  layout is idempotent and deterministic (`tests/layout.rs`, M3.1).
- A property test that every focusable node has a role and a name through
  random sequences of changes (`tests/a11y.rs`, M3.1); routing, focus and
  frames and their damage in `tests/input.rs` and `tests/frame.rs`.
- `cargo xtask theme` (M3.2): the contrast table above, recomputed from
  the theme file, and the pairs it uses for text at their minimums; its
  tests break each rule, and change a token so that a pair fails AA.
- `ui`'s theme reader (`src/theme.rs`, `tests/theme.rs`, M3.2): the
  bundled file is the token table above, each rule is refused with its key
  and, but for a missing key, its line, and random edits of the file never make it panic; its fuzz
  target, `theme` (`tests/fuzz.rs`), runs it under `cargo xtask
  fuzz-smoke` (`docs/04-quality.md` → "Fuzz smoke").
- Accessibility tree snapshot for the main shell, once there is one
  (M3.4): every focusable node has a role and name.
