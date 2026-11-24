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

What is built (M4.7, ADR-0067): `folder`, `trust` and `settings`. Their
rustdoc is authoritative for the rules, names and limits; in short:

- **A folder is one root,** opened by its canonical path, so that trust
  and every comparison of paths see one spelling. Its own files are in
  `.tidepool/` under it. A workspace of several roots holds one `Folder`
  for each.
- **Ignore files follow gitignore(5).** A `.gitignore` or
  `.tidepoolignore` in any folder applies below it, and the root's
  `.git/info/exclude` beneath them all. A `.git` is always ignored;
  nothing inside an ignored folder can be included again; then the
  `.tidepoolignore` files decide, the deepest first, then the
  `.gitignore` files, then the exclude file, so a `.tidepoolignore` can
  include again what a `.gitignore` ignores. Patterns are matched on
  bytes with case kept, by stepping a set of states through the path
  rather than by backtracking, so a hostile pattern such as `*a*a*a…b`
  costs the path's length times its own, not time exponential in its
  stars. Each file is untrusted input: a regular file in a folder reached
  from the root, neither through a symbolic link (as Git reads them), of
  at most 256 KiB, 10,000 patterns and 4,096 bytes a pattern, and all the
  files one `Ignore` holds at most 1 MiB together, since a lookup costs
  about the path's length times every pattern on its way. Files above the
  root, and Git's global excludes file, are not read.
- **Trust is a list of grants** in `trust.toml` in the user data folder.
  A folder is trusted when it is a grant or inside one, since trusting a
  folder already runs the build scripts of the folders in it; declining
  the prompt stores nothing. A trust file that cannot be used grants
  nothing and is never overwritten.
- **Settings are typed and layered.** Each part of Tidepool declares its
  settings in a `Schema`: a boolean, a whole number in a range, a string
  from a list of choices, or a table its owner reads (the keymap). A value
  comes from the defaults, then the user's `settings.toml`, then the
  folder's `.tidepool/settings.toml`, which is not even read until the
  folder is trusted, since a project's settings can bind keys and, later,
  name programs to run. A value that is not valid falls back to the layer
  below with a warning naming the file and the key; a file that cannot be
  used (not TOML, larger than 1 MiB, of a `version` other than 1) gives
  none of its values. A missing `version` means 1.
- **Writing back** reads the file again, sets one value through
  `codec::toml`'s document model, which keeps every other byte, comments
  and unknown keys included, and saves it through `text::file::save`. A
  file that cannot be used is never replaced, and a workspace's file is
  read and written only at `.tidepool/settings.toml` itself, never
  through a symbolic link, so a setting never edits another of the
  project's files. `Settings::resolve` uses a workspace's file only when
  a constructor that asks the trust store made it.

The binary reads its panel settings and `[keymap]` through `settings`
(M4.7). M4.3 opens folders, shows the trust prompt, declares the editor's
settings and gives the keymap a trusted folder's `[keymap]` layer; M4.5
reloads settings when the file changes.

## `tidepool-editor`

### Editor view

- Renders a rope through `syntax` highlights, with gutter (line numbers,
  git markers, diagnostics, breakpoints), folding, soft wrap, rulers,
  indent guides, inlay hints, ghost text, minimap (later).
- Multi-cursor and rectangular selection.
- Large-file mode above 16 MB: read-only by default, no highlighting or
  LSP, chunked loading.
- Images (PNG) and binary files (hex view) open in viewers instead (M13.4).

What is built (M4.1, ADR-0063): `editor::view::EditorView` shows a
`text` rope in a `ui` tree, painted through draw lists alone, so `gui`
and `tui` can both show it; the binary shows none until M4.3 opens a
file. The rustdoc of `editor::view` is authoritative for its sizes and
behavior; in short:

- the text in JetBrains Mono 14 px / 1.5 on `editor-bg`, with a gutter of
  right-aligned line numbers that does not scroll sideways (`text-3`, and
  `text-2` on a line with a cursor's head), each selection in `teal-glow`
  behind its text, a steady `teal` bar at each cursor's head while the
  view has focus, and the `current-line` wash on a line holding a caret
  with nothing selected, so never under a selection;
- tabs to stops four spaces apart; each hidden character `ui`'s
  `is_hidden` names, but the tab, as a U+FFFD marker in `error`, the
  flags of England, Scotland and Wales whole; a line past 10,000 bytes
  cut at a grapheme cluster boundary (what a reader sees as one
  character), with a `text-3` ellipsis where it is cut;
- soft wrap, off at first and on per view, which breaks a line after the
  last space or tab that fits, or else at a cluster boundary, numbering
  only its first row;
- scrolling that costs what is on screen: only the lines shown are laid
  out, and kept until the text, the wrap, the width or the metrics
  change; the wheel scrolls three rows a notch, sideways with Shift or a
  horizontal delta, and at most a view's height an event; `ui`'s
  scrollbars count lines down and units across (the widest line laid
  out so far);
- points map to byte offsets and back, and an offset scrolls into view,
  which editing (M4.2) places and follows carets with; a bad position or
  selection is an `EditorError`, never a panic (ADR-0019).

### Commands and keymaps

- Every action is a named command (`editor.action.copy`,
  `workspace.openFile`, `git.stage`…) with a palette entry.
- **The keymap engine is modal** (ADR-0006): a mode stack plus a key
  sequence parser. The default keymap (M4) uses one mode; vim mode (M7) adds
  the rest (`docs/subsystems/vim-mode.md`).
- Keymaps are TOML; user bindings override defaults; conflicts are listed
  in the keymap viewer.

What is built (M4.6, ADR-0064): `editor::command`, a registry of named
commands, and `editor::keymap`, keymaps read from TOML layers and the
modal engine that resolves keys as they go down, which the shell runs its
shortcuts through. The rustdoc of the two modules is authoritative for
their rules, names and limits; in short:

- a command is an id and its palette title. An id is one or more segments
  joined by `.`, each of ASCII letters and digits and starting with a
  lowercase letter, at most 128 bytes; registering a bad id, a taken one,
  or one whose first segment is `mode` (the engine's) is an error. The
  registry runs nothing itself: its owner runs a command by its id, so a
  plugin's commands (M15.6) can come and go. The palette ranks the
  registry's titles with M3.4's subsequence matcher, which moved into
  `editor::command`;
- the shell registers its 14 commands, `view.showExplorer` to
  `window.close` and `palette.open` ("Command Palette: Show"), and from
  M4.2 the 40 editing commands after them ("Editing" below);
- keys are steps separated by one space, each its modifiers (`ctrl`,
  `shift`, `alt`, `super`, each at most once, in any order) and one key,
  joined by `+`: `ctrl+shift+p`, `ctrl+k ctrl+s`. A key is named by its
  unshifted US-layout label (`a`, `1`, `/`) or by `core::input::Key`'s
  name in lowercase (`f1`, `pageup`); a modifier key (`leftshift`) ends no
  step. Names are matched without regard to case, and keys are shown in
  one form, `Ctrl+Shift+P`, which reads back the same. A step matches a
  key going down with exactly its modifiers;
- a keymap file has `version = 1`, its `start` mode, and `[keymap.<mode>]`
  tables of `"keys" = "command id"`. The default keymap is
  `keymaps/default.toml`, compiled in: `start = "insert"`, and
  `[keymap.insert]` with the four shortcuts the shell had, and from M4.2
  the editing commands' 41 bindings. The user's
  bindings are `settings.toml`'s `[keymap.<mode>]` tables, a layer over
  it, read once at start by the binary, where an empty string unbinds the
  default's keys;
- a binding may name `mode.enter.<mode>`, which pushes a mode the keymap
  has, or `mode.leave`, which pops one, never the last; the stack holds at
  most 32 modes;
- the engine looks keys up in the top mode only. A started sequence waits
  for its next key with no timeout; a key that continues no binding
  cancels it and is consumed; a modifier key going down alone changes
  nothing; a change of mode clears the pending steps; and an auto-repeat
  resolves like a press, saying so, so the shell ignores repeats of its
  commands while M4.2's editing commands can repeat;
- reading never fails for a binding: each problem is kept with the
  keymap, naming the layer, the mode and the keys. Conflicts (the same
  keys twice in one mode and layer, where the first wins; keys that start
  another binding's in one mode, in either layer, where the shorter wins)
  and errors (keys that do not read, a value that is not a string, an
  unknown command or mode, a bad mode, a layer past its limits) are logged
  by the binary as warnings, and overrides (a user binding of a default's
  keys) at info, the first 64 and then how many more there were;
- reading takes its limits, by default 4,096 bindings and 32 modes a layer
  and 8 steps a sequence, never panics, and has a fuzz target, `keymap`.
  Every entry of a layer counts toward its limits before it is looked at,
  so a layer gives at most `max_modes + 1` problems about its modes and
  `2 × max_bindings + 1` about its bindings, however large its tables.

Nothing shows a pending sequence or the problems on screen yet: no task
builds the keymap viewer, and the status bar shows a mode from vim mode on
(M7.2).

### Editing

What is built (M4.2, ADR-0065): `editor::edit::Editor`, an editor view
over a buffer's rope and its `History`, which runs the editing commands on
every cursor at once, answers typed text and the mouse, and asks its owner
for the clipboard. The rustdoc of `editor::edit` is authoritative for its
rules; in short:

- 40 commands with `editor.` ids (`editor.cursorWordLeftSelect`,
  `editor.deleteLeft`, `editor.undo`, `editor.copy`…), registered by the
  shell after its own 14, so that the default keymap binds them. The
  palette lists the commands the shell runs, so it lists these once M4.3
  puts an editor in the shell;
- motions, each with a `…Select` twin: a grapheme cluster (what a reader
  sees as one character) left or right, a selection collapsing to its
  start or end; a word, a run of word characters or of Visual Studio
  Code's separators (`` `~!@#$%^&*()-=+[{]}\|;:'",.<>/? ``), staying on
  the line; a line up or down to a goal column (a tab to its next stop
  of 4, any other cluster one), by line also with soft wrap on; Home to
  the indentation's end or the line's start; End; the text's start and
  end; and a page, as many lines as rows fit;
- edits: typed text replacing each selection; deleting each selection or
  the cluster or word before or after each caret, a CR LF as one; Enter
  with the line's indentation and the editor's line break (the text's
  first, or LF); Tab to the next stop with spaces, or 4 spaces at the
  start of each line a selection spans; Shift+Tab removing a tab or up to
  4 spaces;
- one undo step a command, typing coalesced within 1 s
  (`TYPING_WINDOW`) with the cursors where it left them, a word at a time,
  and repeated deletions coalesced the same way; undo and redo restore the
  selection of their step;
- the clipboard as requests the owner carries out: a copy or a cut hands
  over a rope; carets copy their whole lines, which paste above the
  caret's line; several cursors copy their selections joined by the line
  break, and a paste of as many lines as cursors gives each one line;
- cursors added a line above or below at the goal column, Alt+click, and
  Escape back to one; a click places the caret, Shift extends, a drag
  selects, a double click selects a word;
- on a line longer than `view::MAX_LINE_BYTES` (10,000 bytes), a motion or
  a deletion reads at most that many bytes either side of the cursor, so
  no keystroke reads more than about 20 KB for each cursor; inside a
  cluster or a word
  longer than that, it stops at the edge, on a char boundary;
- Tab and Shift+Tab stay with a focused editor instead of moving focus,
  for the owner's keymap.

The default keymap binds them as Visual Studio Code does on Linux:
arrows, Ctrl+arrows, Home and End, Ctrl+Home and Ctrl+End, Page Up and
Page Down, each with Shift to select; Backspace, Delete, Ctrl+Backspace and
Ctrl+Delete; Enter, Tab and Shift+Tab; Ctrl+Z, Ctrl+Shift+Z and Ctrl+Y;
Ctrl+C, Ctrl+X and Ctrl+V; Ctrl+A; Shift+Alt+Up and Shift+Alt+Down; and
Escape. The binary shows no editor until M4.3, which also routes the
window's clipboard to it.

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
  FOLDER` until M4.3 opens one) and the header's `teal` glow rule; an
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
- the command palette lists the 14 commands of the shell's command
  registry (M4.6), ranked by a subsequence matcher that scores word starts
  and runs higher; in the default keymap, Ctrl+Shift+P or F1 opens it,
  and Ctrl+B and Ctrl+J toggle the panels, on a key going down, for a key
  no widget handled and no modal blocked;
- a tooltip shows 500 ms after the pointer rests on its node, and at once
  for keyboard focus, through the main loop's deadline;
- the settings `[ui] side_panel` and `bottom_panel` in `settings.toml`
  say which panels show at start, read by the binary through
  `workspace::settings` (M4.7), read-only.

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
- Golden frames of the editor view alone, drawn by the same
  `tests/golden.rs` (M4.1, ADR-0063): a Rust-like text with tab-indented
  lines, hidden characters' markers, a selection over two lines and a
  second caret, with keyboard focus, at 800 × 480 with soft wrap off and
  on, and at 400 × 240 at scale 2; 10,000 lines scrolled to their end
  and sideways, with pointer focus (the cursor, and no focus outline);
  and a line past 10,000 bytes scrolled to its cut marker.
- The scrolling budget (M4.1): `tidepool-bench`'s `editor` suites scroll
  the view a wheel notch at a time through 10,000 lines at 1920 × 1080,
  with soft wrap off, and on through lines a fifth of which wrap,
  painting each frame's damage, and time the notch that first lays out
  a 10,000-byte line (`docs/04-quality.md` → "Benchmarks").
- The shell's behavior, headless (`crates/editor/tests/shell.rs`, M3.4):
  its layout at three sizes, hidden panels, the activity bar, the palette
  and its ranking, the shortcuts, the tooltip's timer, the accessibility
  tree, and idleness; and from M4.6 the palette's 14 commands, a keymap
  from the shell's owner in place of the default one, and keys a widget
  handled or a modal blocked never reaching the keymap.
- Keymaps and the engine, headless (`crates/editor/tests/keymap.rs`,
  M4.6): the default keymap, with no problems and every binding naming a
  registered command; a keymap of two modes switching between them;
  pending sequences, their cancel, auto-repeats and modifier keys; the
  user's layer, its overrides and its modes; each conflict and error
  reported; the limits at their defaults; the file's errors; and a
  property test of the engine's bounds. Unit tests check the notation
  (with a property test that any spelling of keys reads back the same),
  the command ids and the registry. The fuzz target `keymap`
  (`crates/editor/tests/fuzz.rs`) reads any text as a keymap file, as the
  user's layer and as keys (`docs/04-quality.md` → "Fuzz smoke").
- The editor view's behavior, headless (`crates/editor/tests/view.rs`,
  M4.1): the gutter, tab stops, hidden characters' markers, selections and
  cursors, the current line, scrolling with soft wrap off and on, the
  scrollbars, the long-line cut, a 10,000-line file scrolled to its end in
  bounded frames, points and byte offsets round-tripping (a property
  test), bad selections, the accessibility tree, and idleness.
- The editing commands, headless (`crates/editor/tests/edit.rs`, M4.2,
  ADR-0065): every motion, with its goal column and on CR LF and
  clusters of several chars; typing, deleting, new lines, indenting and
  outdenting; undo steps, a word of typing at a time, and the selection
  they restore; 1,000 carets typing together; the clipboard's requests,
  whole lines and spread pastes; the mouse; Tab kept from the tree;
  every command at the ends of four small texts; motions bounded on a
  200 KB cluster and on 200 KB of regional indicators; and a property
  test, which prints its seed when it fails, that random commands keep
  the selection in the text and undo brings the first text back. Unit
  tests check the bounded motions on long lines.
- The keystroke budgets (M4.2): `tidepool-bench`'s `editor/keystroke`
  suite types at 10,000 carets of 100 MB at 1920 × 1080, and its
  `editor/crafted` suites run seven keystrokes on each of M1.1's crafted
  100 MB lines, each painting its frame (`docs/04-quality.md` →
  "Benchmarks").
- Folders, trust and settings, headless (`crates/workspace/tests/`,
  M4.7): `folder.rs` matches 98 cases whose answers Git 2.43.0's `git
  check-ignore` gave, the precedence of the three kinds of ignore file
  on a tree, a walker's reading, files past each limit and past the
  budget of all of them, a linked `.gitignore` and a linked folder left
  unread, and hostile patterns in linear time;
  `trust.rs` the untrusted default, grants that persist and cover their
  subfolders, revoking, another store's grant kept, unusable files never
  overwritten, the limits, and a link out of a trusted folder; and
  `settings.rs` the layers, each kind's fallback and its logged warning,
  unusable files, an untrusted workspace's file never read, writing back
  with comments and unknown keys kept, writes refused unchanged, a
  workspace's file or `.tidepool` that is a link (to a file outside the
  folder, or to its own `Cargo.toml`) neither read nor written, and a
  workspace's file that no trusted folder gave left unused. The fuzz targets `ignore` and
  `settings` (`crates/workspace/tests/fuzz.rs`) check the precedence
  rules and compare the matcher with a port of Git's `wildmatch`, and
  resolve, edit and read settings and trust files
  (`docs/04-quality.md` → "Fuzz smoke").
- Session restore round-trips.
- Responsiveness suite with slow fakes (`docs/04-quality.md`).
