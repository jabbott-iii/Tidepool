# Subsystem: `tidepool-editor` and `tidepool-workspace`

`editor` is where features live: the editor view, the keymap engine
(including vim mode), and every panel. It renders through `ui` and never
knows which frontend is drawing. `workspace` is the project model under it.

## `tidepool-workspace`

| Module | Provides |
| :- | :- |
| `folder` | Workspace roots, `.tidepool/` directory, `.tidepoolignore` and `.gitignore` matching |
| `trust` | Trust state per folder (ADR-0014), stored in user data |
| `settings` | Defaults → user `settings.toml` → workspace `.tidepool/settings.toml`; unknown keys preserved; invalid values fall back with a warning |
| `files` | File tree model, fuzzy file finder index |
| `watch` | Debounced file events from `platform`, with a polling fallback |
| `search` | Project search on workers with `tidepool-regex`; streaming results |
| `buffers` | Open buffers keyed by canonical path; external-change detection and reload/merge prompts |
| `session` | Open editors, layout, cursors; restored on start |

## `tidepool-editor`

### Editor view

- Renders a rope through `syntax` highlights, with gutter (line numbers,
  git markers, diagnostics, breakpoints), folding, soft wrap, rulers,
  indent guides, inlay hints, ghost text, minimap (later).
- Multi-cursor and rectangular selection.
- Large-file mode above 16 MB: read-only by default, no highlighting or
  LSP, chunked loading.
- Images (PNG) and binary files (hex view) open in viewers instead (M13.4).

### Commands and keymaps

- Every action is a named command (`editor.action.copy`,
  `workspace.openFile`, `git.stage`…) with a palette entry.
- **The keymap engine is modal** (ADR-0006): a mode stack plus a key
  sequence parser. The default keymap (M4) uses one mode; vim mode (M7) adds
  the rest (`docs/subsystems/vim-mode.md`).
- Keymaps are TOML; user bindings override defaults; conflicts are listed
  in the keymap viewer.

### Shell layout (jabbott.dev look, `docs/subsystems/ui-and-theme.md`)

```
┌─ TIDEPOOL ── workspace ─────────────────────────── title strip, teal glow rule ─┐
│ ▣ │ EXPLORER            │ breadcrumb / path                                        │
│ ⌕ │ ▌OPEN EDITORS       │                                                          │
│ ⑂ │  main.rs            │                editor (editor-bg)                         │
│ ⚙ │ ▌TIDEPOOL           │                                                          │
│ ⬡ │  crates/            ├──────────────────────────────────────────────────────────┤
│ ✦ │  docs/              │ PROBLEMS  TERMINAL  OUTPUT  DEVICES     bottom panel       │
├───┴─────────────────────┴──────────────────────────────────────────────────────────┤
│ ● rust-analyzer  ● git main  NORMAL  Ln 12, Col 4  UTF-8  LF  Rust     status bar   │
└────────────────────────────────────────────────────────────────────────────────────┘
```

Activity bar views: Explorer, Search, Source Control, Toolchains, Devices
(Android/iOS), Agents.

What is built (M3.4, ADR-0060): `editor::shell` builds this frame in a `ui`
tree, and the `tidepool` binary shows it in `gui`'s window. The rustdoc of
`editor::shell` is authoritative for its sizes and behavior; in short:

- a title strip 40 high, with the brand, the workspace's line (`NO
  FOLDER` until M4.7 opens one) and the header's `teal` glow rule; an
  activity bar 48 wide with an outline icon for each view (`gui::icons`)
  and a tooltip naming it; a status bar 24 high on `bar`, with
  rust-analyzer's and git's status dots, off until their tasks, and the
  palette's shortcut;
- the side panel beside a splitter, at 22% of the space beside the bar
  and at least 160 wide, and the bottom panel under one, the editor area
  above it at 70% of the height; each panel can be hidden, and its space
  goes to the editor area;
- every view and bottom tab empty until its task, saying so in a line;
  the editor area shows the backdrop, with a welcome line;
- the command palette lists the shell's 13 commands, ranked by a
  subsequence matcher that scores word starts and runs higher; Ctrl+Shift+P
  or F1 opens it, Ctrl+B and Ctrl+J toggle the panels, for a key no widget
  handled and no modal blocked; M4.6's command registry replaces the list;
- a tooltip shows 500 ms after the pointer rests on its node, and at once
  for keyboard focus, through the main loop's deadline;
- the settings `[ui] side_panel` and `bottom_panel` in `settings.toml`
  say which panels show at start, read by the binary, read-only until
  M4.7 moves them into `workspace::settings`.

### Panels

| Panel | Backed by | Milestone |
| :- | :- | :- |
| Explorer, Open editors | `workspace` | M4 |
| Search (project) | `workspace::search` | M4 |
| Problems | `lang` diagnostics, cargo tasks | M6 |
| Terminal | `term` | M5 |
| Toolchains | `toolchain` | M5 |
| Outline, Markdown preview | `lang`, `markdown` | M6 |
| Source control, diff, blame | `vcs` | M8 |
| Agents, chat, edit proposals | `agent` | M9 |
| SDK manager, Devices, Logcat, Build artifacts | `mobile`, `toolchain` | M10 |
| Simulators, Signing identities | `mobile` | M11 |
| Debug (variables, stack, breakpoints, console) | `lang::dap` | M13 |

### Dialogs that protect the user

- **Trust prompt** when opening a folder.
- **Install plan** before any managed install (steps, sources, licenses).
- **Edit proposal** diff before any agent change.
- **Signing** password prompts that never echo and never persist.

## Tests

- Command tests drive `editor` headlessly and assert buffer, cursor and
  view state.
- Golden frames of the shell at 1280×800 and 1920×1080, with the palette
  open, and at scale 2, drawn by the `tidepool` binary's `tests/golden.rs`
  over the backdrop (M3.4, ADR-0060); TUI snapshots of the same layout
  (M7.5).
- The shell's behavior, headless (`crates/editor/tests/shell.rs`, M3.4):
  its layout at three sizes, hidden panels, the activity bar, the palette
  and its ranking, the shortcuts, the tooltip's timer, the accessibility
  tree, and idleness.
- Session restore round-trips.
- Responsiveness suite with slow fakes (`docs/04-quality.md`).
