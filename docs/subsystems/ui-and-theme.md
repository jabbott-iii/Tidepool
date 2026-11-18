# Subsystem: `tidepool-ui` and the jabbott.dev design language

`tidepool-ui` is the toolkit every view is built from. Its default theme,
`tidepool-dark`, follows jabbott.dev (ADR-0004). The reference is the site's
stylesheet (`jabbott-iii/jabbott-iii.github.io`, `assets/css/style.css`),
whose own header calls it a dark "capsuleer UI": deep-space backdrop,
translucent hairline panels, teal signal accents and squared uppercase type.

`ui` depends only on `core`. It knows nothing about pixels, fonts or
terminals: it lays out in logical units and emits draw lists that `gui` and
`tui` render. Widgets live in the tree's nodes, which routes events to them
and up through their ancestors, and draw lists carry resolved colors
(ADR-0046).

## Contents

M3.1 built `tree`, `layout`, `focus`, `input`, `metrics`, `draw` and
`a11y`. `theme` and `widgets` are planned, with their tasks in the crate's
`README.md`; no roadmap task builds `anim` yet. The rustdoc of each built module is authoritative; its row
summarizes it.

| Module | Provides |
| :- | :- |
| `tree` | `Tree`, a retained tree of nodes named by `NodeId`s (an index and a generation, so a removed node's id names nothing), at most `MAX_DEPTH` (256) levels and `MAX_NODES` (2^20) nodes. Each `Node` has a `Style`, `Semantics`, focusable and enabled flags and at most one `Widget`, which measures, paints and handles events (ADR-0046); the tree tracks each node's `NodeState`: hovered, pressed, focused, focus within, focus visible, disabled |
| `layout` | Row, column, stack, grid and split layout in logical units, with fixed, natural (`Content`) and filling lengths, minimums and maximums, padding, gaps, alignment, clipping, opacity, hiding and scrolling; every value is sanitized first, and a child never overflows a parent that does not scroll, exactly in `f32`. `scroll_into_view` takes effect at the next layout |
| `focus` | The tab order (focusable, enabled, visible nodes in pre-order), `focus_next` and `focus_previous`, wrapping, and `focus-visible`: shown after a key, not after a click |
| `input` | `Tree::handle`: key and text events go to the focused node, pointer events to the node under the pointer (or the one a press captured), then up through their ancestors until a widget handles one; the defaults, Tab moving focus and the wheel scrolling, come after; widgets emit actions for the owner, and what none handled comes back to it. `hit_test` respects clips and paint order |
| `metrics` | `TextStyle` and the `TextMetrics` trait (implemented by `gui` with `font`, M3.5, and by `tui` with cells, M7.1), and `FixedMetrics` for tests and headless use |
| `draw` | `DrawList` of `Command`s in logical units and resolved `core::Color`s (ADR-0046): rect, cut-corner rect, pill, border, bracket corners, line, text run, icon (an `IconId`), image, clip push/pop, layer opacity. `Tree::frame` repaints only the nodes whose data, state or layout changed, and returns the whole tree's list with the damage, the rects whose paint changed, or `None` when nothing did |
| `theme` | Token set loaded from `themes/*.toml`; component styles derived from tokens |
| `widgets` | Button, chip, tab strip, tree view, list, table, text input, search field, scrollbar, splitter, tooltip, toast, modal, command palette, status dot, card, section title, eyebrow, kicker |
| `a11y` | Accessibility tree: role, name, value, state per node (exposed to OS bridges in M14.2); `Tree::accessibility`'s snapshot leaves out nodes of no role and hidden ones |
| `anim` | Optional, timer-driven transitions (hover fades, status pulse); off when reduced motion is set |

## Invariants

- Layout is deterministic for the same tree, metrics and size.
- Every interactive widget has a role and an accessible name. The tree
  refuses a focusable node without them (M3.1); a control that takes no
  focus gives itself `Semantics` (M3.6).
- A secret (a password, a key) is never a node's `Semantics` value, and
  `Debug` shows a text a widget paints, or a node's value, as its length
  only (ADR-0043).
- Widgets never read colors directly; they ask `theme` for a token
  (M3.2; until then a widget paints the colors its owner gives it,
  ADR-0046).
- **Uppercase is for chrome only.** Labels, eyebrows, buttons, table
  headers and section titles are uppercase with letter-spacing, as on the
  site. User content (file names, paths, code, symbols, search text,
  commit messages) is never case-transformed.
- An idle UI emits no damage. Animations are opt-in, timer-based and stop
  when the window is unfocused.

## Tokens (`themes/tidepool-dark.toml`)

Values are taken from jabbott.dev's `:root`. "Solid" values are the
translucent tokens composited over `bg`, for surfaces drawn without a
backdrop and for the TUI.

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
| `cut` | 10 px | — | — | Cut-corner size for primary buttons |
| `font-ui` | Saira | — | — | UI chrome |
| `font-mono` | JetBrains Mono | — | — | Code, terminal, numbers, paths |

## Contrast (WCAG 2.x, measured)

Computed from the values above. `cargo xtask theme` recomputes these and
fails if a pair the theme uses for text drops below its minimum (4.5:1 for
normal text, 3:1 for large text and UI glyphs).

| Text token | on `bg` | on `editor-bg` | on `panel` (solid) | on `slate` |
| :- | :- | :- | :- | :- |
| `text` | 17.37 | 17.10 | 16.79 | 11.19 |
| `text-2` | 10.22 | 10.07 | 9.88 | 6.59 |
| `text-3` | 5.45 | 5.36 | 5.26 | **3.51 (large only)** |
| `teal` | 10.12 | 9.97 | 9.78 | 6.52 |
| `amber` | 11.65 | 11.47 | 11.26 | 7.50 |
| `error` | 8.58 | 8.44 | 8.29 | 5.53 |
| comment `#6f7d82` | 4.60 | 4.53 | **4.44 (large only)** | **2.96 (fails)** |

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
| Card + tab | `.card`, `.card__tab` | Toolchain, device and agent cards; right-edge pill label (`READY`, `UPDATE`) |
| Status dot | `.status-dot` | Toolchain/LSP/agent state: `green` ready, `amber` busy or outdated, `error` failed, `text-3` off. Pulse only when motion is on |
| Table | `.prose th/td` | Problems, SDK packages, devices, git changes: uppercase `teal` headers on `slate` at 60% with a 1 px `teal` underline; `line` row rules; hover wash 2.5% white |
| Code block | `.highlighter-rouge`, `.code-bar` | Markdown preview and hover docs: `editor-bg`, 1 px `line-2` border, 2 px `teal` left border, a code bar with the language label and COPY |
| Breadcrumbs | `.breadcrumbs` | Editor breadcrumb: uppercase separators `/` in `teal`, path segments as written |
| Focus | `:focus-visible` | 2 px `teal` outline, 3 px offset, keyboard focus only |
| Selection | `::selection` | `teal-glow` (35%); on `editor-bg` that is `#275152`; text stays `text` (7.81:1) |
| Backdrop | `.space` (radial gradients, planet, stars, grid) | Welcome screen and empty editor area only. Rendered once per size into the backdrop cache; the star drift animation is off unless `ui.ambient_motion = true` |

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
  active, disabled) at scale 1 and 2 (M3.6, M3.3).
- Layout property tests: children never overflow a non-scrolling parent;
  layout is idempotent and deterministic (`tests/layout.rs`, M3.1).
- A property test that every focusable node has a role and a name through
  random sequences of changes (`tests/a11y.rs`, M3.1); routing, focus and
  frames and their damage in `tests/input.rs` and `tests/frame.rs`.
- `xtask theme` (M3.2): the contrast table above, recomputed from the
  theme file.
- Accessibility tree snapshot for the main shell, once there is one
  (M3.4): every focusable node has a role and name.
