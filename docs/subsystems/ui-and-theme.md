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
tree view, table, tab strip, splitter, card and code block. M3.3 builds
the layer of overlays, `overlay`, and in `widgets` the modal dialog, the
tooltip, the toast and the command palette. No roadmap task builds
`anim` yet. The rustdoc of each built module is authoritative; its row
summarizes it.

| Module | Provides |
| :- | :- |
| `tree` | `Tree`, a retained tree of nodes named by `NodeId`s (an index and a generation, so a removed node's id names nothing), at most `MAX_DEPTH` (256) levels below the root of its layer and `MAX_NODES` (2^20) nodes, overlays included. Each `Node` has a `Style`, `Semantics`, focusable and enabled flags and at most one `Widget`, which measures, paints and handles events (ADR-0046); the tree tracks each node's `NodeState`: hovered, pressed, focused, focus within, focus visible, disabled. Removing an overlay's root closes the overlay. A node may have a tooltip's text (`Tree::set_tooltip`), kept apart from its semantics. A node a modal dialog disables for want of room keeps its own enabled flag, which stays its owner's; its state says it is disabled either way. So a toast its widget keeps out of view for want of room keeps its style, which stays its owner's |
| `overlay` | The layer of overlays (M3.3, ADR-0052, ADR-0053): `Tree::open_overlay` adds a root over the main root's layer, at most `MAX_OVERLAYS` (64), and `Tree::close_overlay` removes one; layers are laid out, painted, hit tested, and in the accessibility tree in one order, from the bottom up (`Tree::overlays`). An overlay that is not modal and is anchored to a node is hosted by that node's layer: it goes right after it and the overlays it already hosts, so it paints just above it and is live or inert with it, however it got there (opened, re-anchored, or there when a hidden modal is shown again); any other goes on top, and a host that closes hands its overlays to its own host. An `Anchor` places each: in the window by alignment within a margin, or on a `Side` of a node, `gap` away, lined up by alignment, flipped to the other side if it fits only there (or to the side with more room if neither fits), then kept inside the window, each laid out after its anchor's layer, and no ring of overlays each anchored to the next let in; a removed anchor leaves it where the node last was, hidden or not, and the tree never closes it. An `OverlayKind` says how it takes input: `Modal`, `Modeless` (hit where its nodes are: a menu, a toast, or a tooltip, which the pointer can rest on), or `PassThrough` (never hit, no focus: a hint such as a drag's image). A toast's overlay keeps the anchor `Toast` gives it: `set_overlay_anchor` refuses it (`UiError::FixedAnchor`). The topmost shown modal one is the active modal: the layers below it are inert (no hover, presses, events, focus, tab stops, or accessibility nodes, and a point outside the live layers hits the modal's root); opening it remembers the focused node and gives focus to its root if the root takes focus (ADR-0053), which is not a tab stop; Tab stays within it and the layers above it; closing it gives focus back to the node if it can still take it, or to none, which `close_overlay` returns for the owner to choose; modals closed out of order hand on the node, to the nearest one above also when it remembered none or one since removed, and one closed while focus is on an overlay above it, such as a toast's button, hands it to that overlay, which gives focus back there when it closes, unless focus has left it since. While a modal is active, focus never rests on no node if the modal's default can take it: its root, or the command palette's field, also when the default is enabled or made focusable again. Whenever the active modal changes (opened, shown again, uncovered as the one over it closes, or closed or hidden, leaving another active or none), a press counts as on the active modal's root, or on nothing, reaching no widget and coming back blocked, until the pointer has moved more than `input::PRESS_SLOP` (4) device pixels from where it rested (ADR-0055, ADR-0056), so neither a click aimed before a dialog opened, nor a double click's second with a little jitter, whether its first opened a dialog or closed one, acts. `Tree::modal`, `Tree::inert`, `Tree::overlay_of`, `Tree::set_overlay_anchor` |
| `layout` | Row, column, stack, grid and split layout in logical units, with fixed, natural (`Content`) and filling lengths, minimums and maximums, padding, gaps, alignment, clipping, opacity, hiding and scrolling; every value is sanitized first, and a child never overflows a parent that does not scroll, exactly in `f32`. `scroll_into_view` takes effect at the next layout |
| `focus` | The tab order (focusable, enabled, visible nodes in pre-order, layer by layer, in the live layers that take input: an active modal's and those above it only; a modal overlay's root takes focus but is not a tab stop), `focus_next` and `focus_previous`, wrapping, and `focus-visible`: shown after a key, not after a click |
| `input` | `Tree::handle`: key and text events go to the focused node (or, with none, to the active modal's root), but Escape first to an open tooltip still anchored to its shown node, pointer events to the node under the pointer (or the one a press captured), then up through their ancestors until a widget handles one; the defaults, Tab moving focus and the wheel scrolling, come after; widgets emit actions for the owner, and what none handled comes back to it, `Response::blocked` while a modal is active, so that the owner runs none of the window's shortcuts for it. `hit_test` respects clips and paint order, tries the overlays from the top down (a tooltip not where it covers the part of its own node that shows, so it never keeps the pointer from it, nor lets it through to what else it covers), and finds nothing below an active modal; no event goes to an inert node. After the active modal changed, until the pointer has moved past the slop, a press goes to no widget and comes back blocked (ADR-0055, ADR-0056); a pointer position that is not finite places nothing. An action is its node's, but a palette's list's are its palette's, which learns at once what its list's press selected |
| `metrics` | `TextStyle` and the `TextMetrics` trait (implemented by `gui` with `font`, M3.5, and by `tui` with cells, M7.1), and `FixedMetrics` for tests and headless use. `TextMetrics::scale` says how many device pixels a logical unit is (1 by default; `gui`'s, its scale's), which each layout records for what `ui` measures in device pixels: the press guard's slop (ADR-0056) |
| `draw` | `DrawList` of `Command`s in logical units and resolved `core::Color`s (ADR-0046), which a widget takes from the tree's theme through `PaintContext::theme` (ADR-0049): rect, cut-corner rect, pill, border, bracket corners, line, text run, icon (an `IconId`), image, clip push/pop, layer opacity. `Tree::frame` repaints only the nodes whose data, state or layout changed, and returns the whole tree's list, the main root's layer then each overlay's, with the damage, the rects whose paint changed, or `None` when nothing did. `PaintContext::clip` is what a node's paint is cut to: the window alone for an overlay's root |
| `theme` | `Theme`, the token set read from a theme file (`themes/*.toml`, the format of ADR-0047) by `Theme::parse` within `codec::Limits`, and the bundled `tidepool-dark` (`Theme::tidepool_dark`): a value for each `Token`, with its solid color and 256-color index where the file gives them, `cut` and the two fonts; a key that is missing or unknown, a wrong type, a color that does not parse or a translucent `bg` is refused, naming the key and, unless it is missing, its line. The tree holds the theme its widgets paint with (`Tree::theme`, `Tree::set_theme`, which repaints every node), the bundled one by default, read once (`Theme::bundled`); the controls' component colors are tokens too (ADR-0049) |
| `widgets` | The tooltip (M3.3, ADR-0052): `Tooltip::open` shows a node's tooltip text in a modeless overlay below it (above it if there is no room), `panel-solid`, at most `TOOLTIP_MAX_WIDTH` wide and `MAX_TOOLTIP_LINES` lines; `Tree::tooltip_target` names the node whose tooltip should show now and why (`TooltipCause::Pointer` or `Focus`: focus once it moves to another node, the pointer once it moves; a key that moves no focus changes neither), which the owner's delay timer follows, the pointer on an open tooltip counting as on its node while the tooltip is still anchored to it and it is shown and live; Escape goes to it first and it emits `Dismiss`, and then no node is the target until the pointer or focus moves to another (WCAG 1.4.13); `Tree::set_tooltip` changes an open tooltip's text too. The toast (M3.3, ADR-0052, ADR-0055): `Toast::show` opens each in a modeless overlay of its own at the window's bottom-right, which keeps that anchor, in one column, the newest at the bottom, taking no focus; at most `MAX_TOASTS`, one more closing the oldest the user does not hold (`UiError::TooManyToasts` if every one is held). No two toasts overlap: one with no room in the window waits out of view, the oldest first, and one shown waits so until a layout places it. While one is held (pointer, press or focus), or the pointer is over where one shows, inert or not, the held ones keep their places and the rest theirs where they meet nothing, a new one going above them (a window of another height settles them anew); so does the column after a toast closes under the pointer, until the pointer moves past the slop, so a double click's second click on a close control reaches no toast slid under it. In the order of the overlays, each toast stands just above the last modal opened before it showed, hidden or not: so a toast shown while a modal is open, or hidden, is live above it while it is active, and one shown before stays inert under it; a toast whose block (it and the overlays its nodes host) would overlap the active modal's block waits just under it, inert, until it would stand clear or that modal closes or is hidden. The order is worked out anew from when each overlay opened and each toast showed at each change of the overlays and after each layout, so it holds whatever happened between (`Toast::waiting`). `ToastKind` info, warning or error (a status or an alert), an optional action, a close control and Escape that emit `Dismiss`; a blank one refused; its overlay clips what it holds, so a long action's label paints nothing outside it; `Toast::held` says when the owner's timeout waits: while the user holds it, it waits, or it is inert under a modal. The command palette (M3.3, ADR-0052, ADR-0054): `Palette::open`, a modal overlay at the top center, `PALETTE_WIDTH` wide, holding a search field that takes focus (and takes it again whenever focus would rest on no node) and emits `Change`, over a `Collection` of the owner's ranked `Row`s, which never takes focus, or an empty line; Up and Down move its selection wrapping, Page Up and Page Down by `PALETTE_ROWS` stopping at the ends, each emitting `Select` only when the selection moves, Enter `Activate` and Escape `Dismiss`, all from the palette's root, by key or pointer; a press selects an item and its release activates it if it is over that item, the item is still selected and no items were given since the press; new items keep the selection on its item when exactly one of them has its text, else select the first; the row a press selects is the palette's at once, so a key in the same batch moves on from it; a press on no control comes back blocked; its field is a combo box whose active option is the selected item; `Palette::selected`, `Palette::set_items`. A `Row`'s `matched` byte ranges, at most `MAX_MATCHES`, moved through its cleaning, cut to its label and widened to whole clusters, are drawn in `MATCH_TEXT` (Saira Bold) and `teal-hi`, each run at the pen the runs before it leave, none past the row's right edge. The modal dialog (M3.3, ADR-0052, ADR-0053): `Modal`, a builder whose `open` is the only way to put one in a tree, as a modal overlay centered in the window at least `MODAL_MARGIN` from its edges, `MODAL_WIDTH` wide, with focus on it as it opens. It holds a title and at most `MAX_MODAL_LINES` lines of text set when it is made, read only as far as kept (its title and each line at most `MAX_CELL_BYTES`; a longer line is cut and the last read) and cleaned of control characters and Unicode's default-ignorable code points (the bidirectional formatting characters and the tag characters among them) but the zero width joiner and non-joiner and variation selector 16, a set a unit test checks against the pinned `DerivedCoreProperties.txt`; on top of that set, the cleaner keeps whole the three RGI emoji tag sequences (UTS #51: the flags of England, Scotland and Wales, a black flag, the tags of `gbeng`, `gbsct` or `gbwls`, and the cancel tag), and only those, so no other text rides in tags; its owner's content, in a body (`Modal::body`); and its owner's buttons, in a row (`Modal::actions`). Its title and text give up room first in a short window, and it disables its body and row while they are squeezed, apart from their own enabled flags, a body made while it is squeezed at once; Escape emits `Dismiss` from it, and its owner closes it; a dialog named by its title, its value its text. The controls (M3.6, ADR-0049): `Button` (ghost, teal, amber, slate; 30 or 24 high; its label cut to its bounds), `Chip`, `TextInput` (a text input or the search field, one line edited by grapheme cluster, dropping what a dialog's text leaves out from what is typed or pasted, a flag's tag sequence kept only when it comes whole), `ScrollBar`, `StatusDot`, `SectionTitle`, `Eyebrow`, `Kicker` and `Crumb`, each a `Control` that gives its node a role and the name it is given, changed with `widgets::update`; `Breadcrumbs`, which builds a navigation row of crumbs; and `focus_outline`, which the focusable controls paint. The collections (M3.7, ADR-0052): `Collection`, a list, tree view or table of its owner's `Rows`, asked for one `Row` at a time, whose rows, all `ROW_HEIGHT` high, show in a view of at most `MAX_VIEW_ROWS` rows (10,240 units, which it measures no taller than); it keeps nodes only for the rows of that view on screen (cut to its clipping ancestors and the window, in a scroller too) and `ROW_MARGIN` either side within the view, so at most `MAX_VIEW_ROWS` and two, binds them to other rows as each layout scrolls it by its own offset, and keeps a selection of one row that the keyboard and the pointer move and scroll into view, within itself and by the scrollers around it (actions `Select`, `Activate`, and a tree view's `Expand` and `Collapse`); what a row shows and names is at most `MAX_CELL_BYTES` a cell, its control, bidirectional formatting, and invisible format characters shown and named as U+FFFD so that no two names look alike (the three flags' tag sequences kept whole, as the cleaner keeps them), and a table's row is named by the cells its columns show; a table's `Column`s have headers that activate. With them (M3.7, ADR-0052): `TabStrip` of `Tab`s (`TabKind` chrome or content), one tab stop whose arrows, Home and End select a tab, wrapping, Delete or a close control or a middle press asking to close one (`Select`, `Dismiss`, about `target()`), the tabs in a track that scrolls sideways, which itself scrolls the selected tab into view when it is selected or moves, or the track's width changes, a press finding a tab only where the last frame showed it, and the target moving with its tab, at most `MAX_TABS` and `MAX_TAB_LABEL_BYTES` a label and the strip's name; `Splitter`, the divider of a `Layout::Split`, dragged or stepped with the arrows along its axis, Home and End, emitting `Change`, which keeps the ratio it was given or moved to and gives the split that ratio kept within the panes' minimums at each layout, so that a split too small for it has it back with room, and moves for no position that is not finite and no key before the split has a length; `Card`, a box with a kicker, title, line of text and a right-edge tab, holding its owner's buttons, named by its title, its value the rest; and `CodeBlock` of `CodeSpan`s in the theme's tokens, under a bar with its language (cut to `MAX_LANGUAGE_BYTES`) and a COPY button that activates, whose owner copies its own source, keeping at most `MAX_CODE_LINES` lines of `MAX_CODE_LINE_BYTES` and `MAX_CODE_SPANS`, each line's start cut at a cluster boundary of the whole line (past the span limit too), reading no more of a span than it keeps but the control characters it leaves out, and splitting plain text at ADR-0019's line ends. After each layout pass the tree binds these widgets and the collections, and passes again while that changes anything, at most eight times, so nested splits settle in one layout; only then do collections scroll a selection asked for, in the space they then have, and the tree reveal their rows and the owner's `scroll_into_view` in turn, the owner's last, from that settled layout, settling the binding again after. A splitter's move repaints only what moves, and a divider binds only as the middle of its split's three children |
| `a11y` | Accessibility tree: role, name, value, state per node (exposed to OS bridges in M14.2); `Tree::accessibility`'s snapshot leaves out nodes of no role and hidden ones, lists the overlays after the main root's layer, shows no node of a pass-through overlay as focusable, and, while a modal is active, holds only the live layers, the modal's root marked `modal` (`aria-modal`). A node's `description` is its tooltip's text (`aria-describedby`), and a node that keeps focus while its keys act on a list's selection, a palette's field, names that item its `active` option (`aria-activedescendant`) |
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
- While a modal overlay is active, no node below it, nor any overlay
  anchored to one, is hovered, pressed, or focused, gets an event, is a
  tab stop, or is in the accessibility snapshot, and what nothing handles
  comes back blocked (M3.3, ADR-0052). A press counts as on the modal
  until the pointer has moved since it became active, and only the toasts
  shown after it are live above it, kept clear of its bounds (ADR-0055).
- An idle UI emits no damage. Animations are opt-in, timer-based and stop
  when the window is unfocused.

## Tokens (`themes/tidepool-dark.toml`)

Values are taken from jabbott.dev's `:root`. "Solid" values are the
translucent tokens composited over `bg`, for surfaces drawn without a
backdrop and for the TUI. `on-amber`, `on-teal` and `comment` are the
colors the contrast rules below name (ADR-0047). The last 13, from
`ghost` on, are the colors the site's components use beyond its `:root`,
which the controls (ADR-0049) and the collections (ADR-0052) take as
tokens. `bronze`, `bronze-hi`, `purple` and `purple-hi` are the colors
of the site's backdrop drawings (ADR-0057): no widget uses them yet, and
`gfx`'s backdrop keeps its own copies of them, as `gfx` sits below `ui`;
a test of `gui` holds the copies to these tokens. The file writes each
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
| `bronze` | `#cd7f32` | — | 173 | The backdrop's circuits (ADR-0057) |
| `bronze-hi` | `#ffc98f` | — | 222 | Highlight of `bronze`: the circuits' sparks |
| `purple` | `#9a6bff` | — | 99 | The backdrop's synapses (ADR-0057) |
| `purple-hi` | `#e3d6ff` | — | 189 | Highlight of `purple`: the synapses' sparks |
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
| Modal | — (the site has none; its popup panel, `.deploy__panel`, a `.panel` with `.panel--brackets`, is the nearest) | Confirmations and consents: a dialog centered in the window, 320 wide (the popup's 390 at the IDE's scale) and at least 12 from the window's edges, 14 in; `panel`'s solid value (the popup is 97% opaque over a blur, which no draw command paints; ADR-0053), a 1 px `line` border and the 16 px `teal` bracket corners (top-left, bottom-right) over it; its title in the section title's look (a 3 px `teal` rule, Saira 13 px 500, 0.04em, uppercase, `text`), then at most four lines of the UI body in `text-2`, as written; its owner's content, 10 below; its owner's buttons in a row at its bottom, at its right, 8 apart. The window under it is dimmed with `panel` (ADR-0053), so what is inert reads as set back; a dialog over another dims it again. In a window too short for it, its text gives up whole lines first, and nothing it holds paints outside it. The popup's shadow and backdrop blur are left out |
| Tooltip | — (the site has none) | A node's tooltip: `panel-solid` (the spec's color for menus and tooltips), a 1 px `line` border, its lines in Saira 12 px, `text`, as written, 8 px in across and 4 px down, at most 320 wide; 4 px below its node, or above it near the window's bottom. It shows for the pointer and for keyboard focus, after the owner's delay; the pointer can rest on it, and Escape closes it first (WCAG 1.4.13) |
| Toast | — (the site has none) | Notices in the window's bottom-right corner, 12 px from its edges, 8 px apart, the newest at the bottom, at most four: 320 wide, `panel`'s solid value (opaque, as a dialog's), a 1 px `line` border and a 3 px stripe down the left in `teal` (info), `amber` (warning) or `error` (error, read at once as an alert); its text the UI body, `text`, as written; a 16 px close cross as a tab's, and an optional small teal action button at its right, all cut to its bounds. One waiting under a dialog it would cover is dimmed with the window (ADR-0055); one with no room in a short window is not drawn |
| Command palette | — (the site has none; its search field and popup panel are the nearest) | At the top center of the window, 12 px from its top, 480 wide (one and a half times a dialog's), dimmed window, opaque `panel` fill, `line` border and bracket corners as a modal dialog (ADR-0054); the search field's look, full width, then up to ten rows of a list, 10 px below it, each as a list's row (hover `row-hover`, selected `teal-glow`) with the characters the query matched in Saira Bold (700) and `teal-hi`, the rest of the row's text running at their widths; with no items, a line in the UI body, `text-2`, saying so |
| Breadcrumbs | `.breadcrumbs` | Editor breadcrumb: uppercase separators `/` in `teal`, path segments as written |
| Focus | `:focus-visible` | 2 px `teal` outline, 3 px offset, keyboard focus only; a field shows its `teal` border and 3 px `teal-ring` instead, whenever it has focus; a collection and a column header draw it inside their bounds instead, as they fill a panel or the window, which would cut an outline outside them off |
| Selection | `::selection` | `teal-glow` (35%); on `editor-bg` that is `#275152`; text stays `text` (7.81:1) |
| Backdrop | `.space` (radial gradients, stars, grid) and its two `svg.space__art` drawings | Welcome screen and empty editor area only. Rendered once per size into the backdrop cache: the purple, bronze and slate gradients, the stars, the grid, and the two drawings, drawn still (ADR-0057): the bronze circuits at the left edge and the purple synapses at the right, each as tall as the window, 2:5 wide and at most 40% of its width, fading toward the middle, at 70% opacity in a window at most 640 px wide. The star drift and the drawings' sparks are motion, off unless `ui.ambient_motion = true`; neither is drawn yet |

The site has no pressed, disabled, plain text input, scrollbar, list,
tree view, tab strip, splitter, modal, tooltip, toast or palette style, and paints glows with blurred `box-shadow`s; the
controls' states are ADR-0049's, and the collections' ADR-0052's; the
overlays' are derived from the site's popup panel, brackets, section
title, search field, buttons and status colors (M3.3). A control paints from its node's `NodeState`:

| State | Look |
| :- | :- |
| Rest | As the Components table above; a text input is a `slate` rect, its text `text` and its placeholder `text-2`; a scrollbar has no track and a `line-2` thumb, a pill 6 wide in its 10 and at least 24 long; a row of a list, tree view or table has no fill, and the selected row is `teal-glow` in every state; a tab and a card as the Components table above; a splitter's `line` rule; a modal, a tooltip, a toast and the palette as the Components table above |
| Hover | The site's `:hover`: ghost gains a `teal` border and a `teal-hi` label; teal a `teal-fill` fill and a `teal` border; amber is `amber-hi`; slate `slate-hi` with a `teal-hi` label; a chip and a field gain a `teal-line` border, but a selected chip turns `teal-hi`; a scrollbar gains a `line` track and a `teal-glow` thumb; a link's text is `teal-hi`; a row gains the `row-hover` wash (`.prose tbody tr:hover`), and a column header's title is `teal-hi`; a tab gains the `row-hover` wash and a `teal-hi` label, and a close control a `teal-wash` circle and a `teal-hi` cross; a splitter shows a 2 px `teal-glow` bar; a card its `teal-line` border, `teal-fill` ring and `teal-hi` title; COPY `teal-wash` with a `teal-hi` label; a modal has no hover look of its own, its buttons show theirs, and the inert window under it shows none; a toast's close cross gains a `teal-wash` circle and turns `teal-hi`, as a tab's; a palette's item gains the `row-hover` wash; a tooltip has no hover look, and stays while the pointer rests on it |
| Pressed (the task's "active") | The hover look with the fill one step stronger: ghost `teal-wash`, teal `teal-glow`, a chip `slate-hi`; a slate button gains a `teal-line` border; an amber button and a selected chip keep their rest fill inside a 2 px rim of their hover fill; a scrollbar's thumb is `teal`; a link's text is `teal`; a column header is `slate` above its underline, its title `teal-hi`; a press on a row selects it, so it shows the selection; a tab is `teal-wash`, and selected by the press; a close control is on `teal-fill`; a splitter, pressed or dragged, shows a 2 px `teal` bar; COPY is `teal-fill`; a modal has no pressed look, its buttons show theirs; a toast's close cross is on `teal-fill`; a press on a palette's item selects it (`teal-glow`) |
| Focus | The focus outline when focus shows; a field's border and ring whenever it has focus, with its caret, which does not blink; a collection's outline is 2 px inside its bounds, in the 2 px of padding its default style leaves so no row covers it, and its selected row stays `teal-glow`; a column header's, a tab strip's (in the same padding) and a splitter's are inside their bounds too; a card shows none, as its buttons take focus; a modal dialog, which has focus as it opens (ADR-0053), shows the outline 3 px outside its bounds while that focus shows, as nothing near the window's edges cuts it off (it is at least 12 from them) and keys go to it, until Tab moves focus to its first control; a palette's focus is in its field, which shows its border and ring, and its selected item is `teal-glow`; a toast's close control shows the outline; a tooltip shows for its node's keyboard focus |
| Disabled | The rest look in a layer at 40% opacity, with no hover or pressed look (WCAG exempts inactive controls from its minimums); a modal keeps its dimming of the window and its opaque fill, so nothing shows through it, and dims the rest; a modal too short for its content and buttons disables them, which then look disabled too; a disabled toast dims all it paints; a tooltip shows for a disabled node too |

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
  has looked at them; and the modal dialog's 2 scenes, painted by `gui`
  (`crates/gui/tests/overlays/mod.rs`, M3.3), a row for each of two
  dialogs over a window and a column for each state: rest, with the
  pointer over the inert window; hover, on its first button; focus, on
  the dialog itself, shown by a key, with its outline (ADR-0053);
  pressed, its first button; and disabled; blessed once the owner has
  looked at them. Beside them, in the same module, the tooltip's 2
  scenes, a row for each tooltip (one line, two lines, flipped above its
  node) and a column for each state of its node (the pointer on it,
  keyboard focus on it, the pointer resting on the tooltip, the node
  disabled); the toast's 2, a row for each kind (info, warning with an
  action, error of two lines) and a column for each state (rest, its
  close control hovered, focus on it, its action pressed, disabled); and
  the palette's 2, a row for each list (matches, in Saira Bold, no
  results, a long list wrapped to its last item) and a column for each
  state (rest, an item hovered, Down twice, an item pressed; it has no
  disabled state, as an owner disables no palette it opens); blessed once
  the owner has looked at them.
- The controls' behavior (`tests/widgets.rs`, M3.6): each one's role and
  name, what activates or changes it, the tokens it paints in each state,
  fields edited by grapheme cluster, the scrollbar dragged and paged, the
  tree's theme, a button's label cut to its bounds; and a property test
  of random edits, after each of which
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
- The overlays (`tests/overlays.rs`, M3.3, ADR-0052, ADR-0053): a modal
  makes the rest inert at once (no hover, press, event, focus, wheel, tab
  stop, or accessibility node below it, and a point outside it hits its
  root); what it keeps from the window comes back blocked, and window
  focus events never do; Tab and Shift+Tab stay within it and the
  overlays above it; an overlay anchored to a node below it, of each kind
  and however it got there (opened after it, re-anchored, or there when
  a hidden modal is shown again), is inert, takes no Tab, is not hit and
  paints under its dimming, while one anchored to the window or to its
  own button is live and above it; closing it gives focus back, showing
  as the last key or click left it, and to none when that node was
  removed, disabled, hidden, or made not focusable, or when none had
  focus, but not away from a live overlay above it, which gives it back
  when it closes; stacked modals
  closed in either order, the upper one having remembered no node or one
  since removed; a hidden modal makes nothing inert until it shows, and
  a disabled one still does; overlays painted, hit, and listed in order,
  a pass-through one never hit nor shown focusable; window anchors, with
  margins and fills, and node anchors, flipped, kept inside the window,
  following their node as it scrolls and while hidden; a removed anchor;
  bad ids, bad anchors, and `MAX_OVERLAYS`; the damage of opening,
  moving, and closing; a collection in an overlay; and the modal
  dialog's focus on itself as it opens and its outline after a key, its
  role, name, value, Escape (one held press closes one dialog, a field's
  too), a blank title refused, its body, layout, a short window that
  takes its text first and disables controls it cannot show whole, its
  text measured and painted with the same metrics, its caps, `Debug`,
  cleaning, and tokens, at rest and disabled; a press that waits for the
  pointer to move after a modal becomes active (opened, uncovered, shown
  again, a drag, another button, a pointer not yet seen, a position that
  is not finite), past 4 device pixels at the window's scale, after the
  last modal closes or is hidden too, and a double click with a pixel of
  jitter on a palette item that cannot accept the prompt it opens
  (ADR-0055, ADR-0056); a body made while the dialog is squeezed,
  squeezed at once; a modal's default enabled again taking focus; with a
  property test of
  random opens, closes, removals, keys, presses, focus changes, hiding,
  and frames, after each of which no inert node is hovered, pressed,
  focused, a tab stop, or exposed, an overlay anchored to a node is
  inert with it and painted under the modal if inert, a modal's root
  that can take focus has it as it opens, focus went back as the rules
  say, focus rests on no node while a modal's default can take it, every
  shown overlay lies inside the window, a hit takes input, and an idle
  frame draws nothing; with unit tests of the placement, the
  margin, the overlay records kept in step with the roots and after
  their hosts, the dialog's bounded and cleaned text, its refusal when
  the tree has no room for it, and a doc test that a `Modal` cannot be
  put in a node by hand.
- The tooltip, the toast and the palette (`tests/popups.rs`, M3.3,
  ADR-0052, ADR-0054, ADR-0055): the tooltip's target, for the pointer
  and for keyboard focus, focus once it moves and not on a key that moves
  none (a modifier, a letter, Escape); WCAG 1.4.13's dismissible (Escape
  first, from a field and from a dialog, its repeat taken, and no target
  after it until the pointer or focus moves to another node), hoverable
  (the pointer resting on it, and a press on it reaching nothing it
  covers, but never its own node when it is forced over it) and
  persistent; its place, flipped above, its width, tokens, role, the
  description it gives its node, kept through a control's update, its
  text changed while open, its caps, cleaning and errors; no target on
  an inert node, nor through a stale tooltip (its node hidden, or it
  moved above a modal). The toast shown in the corner without taking
  focus, in one column, at most `MAX_TOASTS`, the oldest not held closed
  and none moved while one is held, held while hovered or focused, its
  action, close control and Escape, its roles and stripe, caps and
  cleaning, a blank one refused, and over a modal: live and in Tab's
  reach when shown after it, while one shown before stays inert; waiting
  under a dialog it would cover, held, and live again once it stands
  clear or the dialog closes; and a dialog closed while focus is on it
  handing its node on. The palette at the top with focus in its field
  and its accessibility tree (combo box, active option, results, the
  empty line); keyboard navigation (Up and Down wrapping, pages stopping
  at the ends, `Select` only on a move, Home and End left to the field,
  Enter at once); `Change` on each edit; the matched ranges drawn, cut,
  widened and joined, in Saira Bold at its widths, with a property test
  that random ranges draw the label as far as it shows and never panic,
  and the measuring a long label with 64 ranges costs pinned; a row's
  hidden characters shown and named as U+FFFD, its ranges still lined
  up; the pointer (hover, a press selecting, its release over the item
  activating, elsewhere not; items given mid-click activating nothing; a
  release lost to the window or a dialog leaving the next click whole; a
  press on no control blocked); the selection kept across new items; focus
  back in the field when it would rest on none; a short window where the
  list gives up room; over a modal dialog; caps and errors; and every
  popup's idle frame drawing nothing. Besides (M3.3's fourth round): a
  toast in its place in the order above the last modal opened before
  it, hidden or not, worked out again to the same; one shown while a
  dialog is hidden, live above it once shown; one waiting for a modal
  hidden since, waiting no more; one kept clear of what its and the
  dialog's nodes host; its anchor refused to an owner; no two toasts
  overlapping, one with no room waiting out of view, held, and a new one
  out of view until placed; one hidden and shown again while another is
  held, placed anew; a double click on a close control reaching no
  other toast, with a modal and without; a 4 KiB action label painting
  nothing outside its toast; a toast forgetting a dialog's node once
  focus leaves it; a key right after a press on an item moving on from
  it; a range past a label's end marking nothing; a tooltip letting the
  pointer through only where its node shows; and a field dropping
  bidirectional and invisible characters typed or pasted, but emoji
  joiners. The overlays' property test also opens tooltips, toasts and
  palettes, closes toasts, and presses Escape and types, among its
  steps, and models a closed modal's node forgotten once focus leaves
  the overlay it was handed to.
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
