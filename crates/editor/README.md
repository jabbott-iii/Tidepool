# tidepool-editor

Where Tidepool's features live: the editor view, named commands and the
modal keymap engine (vim mode included), and every panel. It renders through
`ui` and never knows which frontend draws it.

It is part of [Tidepool](https://github.com/jabbott-iii/Tidepool), where
the `tidepool` binary connects it to the frontends; it uses the services and
features below it, and never `gui` or `tui`.

Layer 4, views and frontends. It may use `core`, `codec`, `text`, `regex`,
`markdown`, `syntax`, `term`, `workspace`, `ui`, `lang`, `vcs`, `agent` and
`mobile`, as its row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Version 0.1, not published (`publish = false`): the API changes as the
roadmap needs.

Built, with the roadmap task that built it:

- the shell, `shell::Shell`: the title strip, the activity bar with a
  button and tooltip for each view, the side panel and the bottom panel
  beside splitters, each of which can be hidden, the editor area, the
  status bar, and the command palette, built in a `ui` tree and answering
  its events through `ui` alone; the palette ranks the shell's commands
  with a subsequence matcher (`shell::commands::rank`), Ctrl+Shift+P or F1
  opens it, Ctrl+B and Ctrl+J toggle the panels, and a tooltip shows
  500 ms after the pointer rests, at once for keyboard focus — M3.4
  (ADR-0060). Every view and tab is empty until its task, and says so.
  From M4.6 the palette lists the shell's 14 commands from its command
  registry, `palette.open` among them, and its four keys are the default
  keymap's, which the shell's owner can replace (`Shell::set_keymap`).
  From M4.2 the registry also holds the 40 editing commands, which the
  default keymap binds; the palette lists the commands the shell runs,
  so it lists them once M4.3 gives the shell an editor.
- the editor view, `view::EditorView`: a `text` rope's text in a `ui`
  tree, with a gutter of line numbers, the selection's ranges and a
  steady cursor bar at each head while it has focus, the focus outline
  inside it while keyboard focus shows, the current line's
  wash, tab stops, a marker for each hidden character, soft wrap (off at
  first), and scrolling by the wheel and two `ui` scrollbars; it lays out
  only the lines on screen and cuts a line past 10,000 bytes
  (`view::MAX_LINE_BYTES`), and maps points to byte offsets and back,
  which editing places carets with. Its errors are `EditorError`s. The
  binary does not show it until M4.3 opens a file — M4.1 (ADR-0063).
- named commands, `command::Registry`: each an id, such as
  `view.toggleSidePanel` (dotted segments of ASCII letters and digits,
  each starting with a lowercase letter, at most 128 bytes), and its
  palette title, ranked for the palette by the subsequence matcher, which
  moved here from the shell; a bad or taken id is an `EditorError` — M4.6
  (ADR-0064).
- the modal keymap engine, `keymap`: keys in `ctrl+shift+p` notation
  (`keymap::Sequence`, shown as `Ctrl+Shift+P`); keymaps read from TOML
  layers, the default one (`keymaps/default.toml`, compiled in, one mode,
  `insert`) and the user's over it, where an empty string unbinds, within
  `keymap::Limits` (4,096 bindings and 32 modes a layer, 8 steps); the
  conflicts, overrides and errors found, each with its layer, mode and
  keys (`keymap::Problem`); and `keymap::Engine`, a stack of modes and a
  pending sequence that waits without a timeout, looking keys up in the
  top mode only, entering and leaving modes with `mode.enter.<mode>` and
  `mode.leave`, and saying whether a key was an auto-repeat — M4.6
  (ADR-0064, ADR-0006). Nothing shows a pending sequence or the problems
  on screen yet: no task builds the keymap viewer.
- editing, `edit::Editor`: an editor view over a buffer's rope and its
  `text::History`, running 40 editing commands (`edit::Command`, ids such
  as `editor.cursorWordLeftSelect` and `editor.undo`) on every cursor at
  once: motions by grapheme cluster, word, line (to a goal column), the
  line's ends, the text's ends and a page, each with a `…Select` twin;
  typing, deleting a cluster or a word, a new line with the line's
  indentation, indenting with 4 spaces and outdenting; one undo step a
  command, typing coalesced a word at a time; the clipboard as requests
  its owner carries out (`edit::Request`), whole lines when only carets
  copy, and a paste spread over the cursors when it has a line for each;
  adding cursors above and below, and back to one; and the mouse:
  click, Shift, Alt, drag and double click. On a line past 10,000 bytes
  a motion reads at most that far either side of the cursor. Tab and
  Shift+Tab stay with a focused editor. The default keymap binds the
  commands as Visual Studio Code does on Linux. The binary shows no
  editor, and does not route the window's clipboard to one, until M4.3 —
  M4.2 (ADR-0065; a keystroke at 10,000 carets of 100 MB takes about
  19 ms, ADR-0066).
Not yet, with the roadmap task that adds it:

- explorer, tabs, splits, the trust prompt — M4.3.
- search — M4.4.
- the terminal panel — M5.4.
- the Toolchains panel — M5.6.
- highlighting — M6.2.
- language features — M6.7, M6.8, M6.3.
- the Markdown preview — M6.6.
- vim mode, with its modes and bindings by typed character — M7.2,
  M7.3, M7.4.
- git panels — M8.2, M8.3.
- AI completion, chat and proposals — M9.4, M9.5.
- mobile panels — M10.6, M11.6.
- image and hex viewers — M13.4.
- Lua plugins: the host, the API, the Plugins view, and the API's
  definitions for lua-language-server — M15.5, M15.6, M15.7, M15.8.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Usage

A workspace crate whose row of the matrix allows it depends on it by path:

```toml
[dependencies]
tidepool-editor = { path = "../editor" }
```

Build the shell in a tree, lay it out, and give it events; the frontend
paints the tree's frames (the `tidepool` binary does it with
`tidepool-gui`):

```rust
use std::time::Instant;

use tidepool_core::geom::Size;
use tidepool_core::input::{InputEvent, Key, Modifiers};
use tidepool_editor::shell::{Command, Flow, Panels, Shell, commands};
use tidepool_ui::Tree;
use tidepool_ui::metrics::FixedMetrics;

let mut tree = Tree::new();
let mut shell = Shell::new(&mut tree, Panels::default())?;
let window = Size::new(1280.0, 800.0);
assert!(tree.frame(window, &FixedMetrics::DEFAULT).is_some());

// Ctrl+B hides the side panel.
let ctrl_b = InputEvent::KeyDown { key: Key::B, modifiers: Modifiers::CTRL, repeat: false };
assert_eq!(shell.handle(&mut tree, &ctrl_b, Instant::now())?, Flow::Continue);
assert!(!shell.panels().side);

// The palette ranks the commands: "tsp" finds the side panel's toggle.
assert_eq!(commands::rank("tsp")[0].command, Command::ToggleSidePanel);
# Ok::<(), tidepool_ui::UiError>(())
```

Build an editor view over a rope, set its selection and soft wrap, and
keep it in step: give it each event through `handle`, and call `update`
after each frame, with the metrics the frames are drawn with:

```rust
use tidepool_core::geom::{Point, Size};
use tidepool_core::input::{InputEvent, Modifiers, ScrollDelta};
use tidepool_editor::view::EditorView;
use tidepool_text::{Cursor, Rope, Selection};
use tidepool_ui::Tree;
use tidepool_ui::metrics::FixedMetrics;

let text: String = (1..=100).map(|n| format!("let x{n} = {n};\n")).collect();
let mut tree = Tree::new();
let root = tree.root();
let view = EditorView::insert(&mut tree, root, "main.rs", Rope::from(text.as_str()))?;
view.set_selection(&mut tree, Selection::single(Cursor::new(0, 3)))?;
view.set_wrap(&mut tree, true)?;

let (size, metrics) = (Size::new(640.0, 480.0), FixedMetrics::DEFAULT);
assert!(tree.frame(size, &metrics).is_some());
view.update(&mut tree, &[], &metrics)?;

// A notch of the wheel scrolls three rows.
let notch = InputEvent::Wheel {
    delta: ScrollDelta::Lines { x: 0.0, y: 1.0 },
    position: Point::new(100.0, 100.0),
    modifiers: Modifiers::NONE,
};
view.handle(&mut tree, &notch, &metrics)?;
assert_eq!(view.position(&tree)?.line, 3);
# Ok::<(), tidepool_editor::EditorError>(())
```

Read a keymap against a registry of commands, with a user's layer over
it, and resolve keys with the engine; its owner runs the commands it
resolves to by their ids:

```rust
use tidepool_codec::toml;
use tidepool_core::input::{InputEvent, Key, Modifiers};
use tidepool_editor::command::Registry;
use tidepool_editor::keymap::{Engine, Keymap, Limits, ProblemKind, Resolution};

let mut registry = Registry::new();
registry.register("editor.save", "File: Save")?;
registry.register("editor.saveAll", "File: Save All")?;
let default = r#"
version = 1
start = "normal"

[keymap.normal]
i = "mode.enter.insert"
"ctrl+s" = "editor.save"

[keymap.insert]
escape = "mode.leave"
"#;
// The user's layer, as `settings.toml`'s `[keymap.<mode>]` tables give it.
let settings = toml::parse(
    "[keymap.normal]\n\"ctrl+s\" = \"editor.saveAll\"\n\"ctrl+k ctrl+s\" = \"editor.save\"\n",
    tidepool_codec::Limits::DEFAULT,
)
.expect("TOML");
let user = settings.get("keymap").and_then(|keymap| keymap.as_table());
let keymap = Keymap::read(default, user, &registry, Limits::DEFAULT)?;
// The user's Ctrl+S overrides the default's: listed, not a conflict.
assert_eq!(keymap.problems().len(), 1);
assert_eq!(keymap.problems()[0].kind(), ProblemKind::Override);

let mut engine = Engine::new(keymap);
let down = |key, modifiers| InputEvent::KeyDown { key, modifiers, repeat: false };
assert_eq!(
    engine.handle(&down(Key::S, Modifiers::CTRL)),
    Resolution::Command { id: "editor.saveAll", repeat: false }
);
// A sequence waits for its next key, with no timeout.
assert_eq!(engine.handle(&down(Key::K, Modifiers::CTRL)), Resolution::Pending);
assert_eq!(
    engine.handle(&down(Key::S, Modifiers::CTRL)),
    Resolution::Command { id: "editor.save", repeat: false }
);
// `i` enters Insert mode, where only Insert's bindings count.
engine.handle(&down(Key::I, Modifiers::NONE));
assert_eq!(engine.mode(), "insert");
assert_eq!(engine.handle(&down(Key::S, Modifiers::CTRL)), Resolution::Unbound);
# Ok::<(), tidepool_editor::EditorError>(())
```

Run editing commands on an editor, and carry out its clipboard
requests; the owner resolves keys with its keymap engine and runs the
command an id names:

```rust
use std::time::Instant;

use tidepool_core::geom::Size;
use tidepool_editor::edit::{Command, Editor, Motion, Request};
use tidepool_text::Rope;
use tidepool_ui::Tree;
use tidepool_ui::metrics::FixedMetrics;

let mut tree = Tree::new();
let root = tree.root();
let mut editor = Editor::insert(&mut tree, root, "main.rs", Rope::from("a\nb\n"))?;
let (size, metrics) = (Size::new(640.0, 480.0), FixedMetrics::DEFAULT);
assert!(tree.frame(size, &metrics).is_some());
let now = Instant::now();

// A second cursor below the first, then typing at both.
editor.run(&mut tree, Command::AddCursorBelow, now, &metrics)?;
editor.type_text(&mut tree, "> ", now, &metrics)?;
assert_eq!(*editor.text(), "> a\n> b\n");

// With only carets, a copy takes their whole lines.
let copy = editor.run(&mut tree, Command::Copy, now, &metrics)?;
assert_eq!(copy.request, Some(Request::Copy(Rope::from("> a\n> b\n"))));

// The id the keymap names runs the same command.
let id = Command::Move { motion: Motion::End, select: false }.id();
assert_eq!(Command::from_id(id).map(|c| c.title()), Some("Cursor: Move to Line End"));
# Ok::<(), tidepool_editor::EditorError>(())
```

## Documentation

- Design: [`docs/subsystems/editor.md`](../../docs/subsystems/editor.md),
  [`docs/subsystems/vim-mode.md`](../../docs/subsystems/vim-mode.md),
  [`docs/subsystems/ui-and-theme.md`](../../docs/subsystems/ui-and-theme.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0003 (GUI
  first, features in `editor`); ADR-0006 (vim mode as a modal keymap
  engine in `editor`); ADR-0065 (the editing commands in `editor::edit`,
  Up and Down by line, long lines read in a window, the clipboard as
  requests, and Visual Studio Code's keys); ADR-0060
  (the shell in `editor::shell`, its icons, settings and palette);
  ADR-0063 (the editor view in `editor::view`, soft wrap off at first,
  hidden characters as markers, and the `current-line` token); ADR-0064
  (the command registry, and keymaps as TOML layers in `ctrl+shift+p`
  notation, the user's in `settings.toml`).
- API reference: `cargo doc -p tidepool-editor --open`

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
cargo test -p tidepool-editor
cargo xtask golden
cargo xtask fuzz-smoke --target keymap
cargo xtask bench editor
```

The editing tests (`tests/edit.rs`) check every motion, with goal
columns over short lines and tabs and with clusters of several chars and
CR LF; typing, deleting a cluster, a word, a CR LF and a selection; new
lines with the indentation and the text's line break; indenting and
outdenting; undo steps, typing coalesced a word at a time and split by a
pause or a move, and the selection each restores; cursors added at their
goal column, edits at every cursor, 1,000 carets typing together, and
back to one cursor; the clipboard's requests, whole lines pasted above,
and spread pastes; the mouse; Tab kept from the tree and typed text only
with focus; every command at both ends of four small texts; motions on a
200 KB cluster and 200 KB of regional indicators staying within their
window; and a property test, which prints its seed when it fails, that
random commands, typing, pastes and selections keep the selection in the
text, the view showing the editor's text, and that undo brings the first
text back. Unit tests check the bounded motions on long lines, words,
columns and indentation.

The tests build the shell headless: its areas tile the window at
800 × 600, 1280 × 800 and 1920 × 1080 and stay inside a smaller one;
hidden panels give their space to the rest; Ctrl+B and Ctrl+J toggle
them; the activity bar switches views and hides the shown one; the
palette ranks the commands as its query changes, runs the first with
Enter, closes with Escape and blocks the shortcuts while open; every
command runs; a tooltip waits 500 ms for the pointer and none for focus;
the accessibility tree names every part; and an idle shell draws nothing.
The matcher's unit tests check word starts, runs, the best alignment and
the matched ranges. From M4.6 the shell's tests also check that the
palette lists its registry's 14 commands, that `palette.open` reopens it,
that a keymap from the shell's owner replaces the default one, and that a
key a widget handled or a modal blocked never reaches the keymap.

The keymap's tests (`tests/keymap.rs`) check the default keymap (no
problems, and every binding naming a registered command); a keymap of two
modes, `normal` entering `insert` with `i` and `insert` leaving with
Escape, switching between them and looking keys up in the top mode only;
a sequence waiting for its next key, canceled by a wrong one, and cleared
by a change of mode; auto-repeats resolving like presses and saying so;
modifier keys alone changing nothing; the user's layer, its overrides, its
unbinding and its modes; the same keys twice in a layer, keys that start
another binding's in either layer, and each error, reported with their
layer, mode and keys and escaped in messages; the limits at their
defaults, with 4,096 bindings and 32 modes a layer and 8 steps, and user
tables of 100,000 entries that are not modes, or are bad bindings in one
mode, giving problems within the documented bound; a file that is not a
keymap; and a property test, which prints its seed when it fails, that
the engine's stack and pending steps stay in bounds. Unit
tests check the notation, with a property test that keys in any spelling
read and show in one form that reads back the same, the command ids, and
the registry. The fuzz target `keymap` (`tests/fuzz.rs`, seeds in
`fuzz/corpus/keymap/` and `keymaps/`) reads any text as a keymap file, as
the user's layer and as keys, and presses keys the input chooses, the
keymap's own bindings among them; a test checks that its driver can press
every default binding. `cargo xtask fuzz-smoke --target keymap` mutates
it.

The editor view's tests build it headless too: the gutter's numbers,
right-aligned, `text-2` where a cursor's head is, and a gutter that grows
at 1,000 lines; tab stops; markers for NUL, NEL (U+0085, a C1 control
character), U+202E, U+200B and U+2028, but none for the tab, LF, CR LF or a lone CR, and the flag of
Wales whole; selections behind their text, a selected line break, and
several cursors; cursors only while the view has focus; the focus
outline for keyboard focus, from `set_focus` or Tab, and none for a
click; the current
line's wash only with a caret and nothing selected; sideways scrolling by
Shift and by a horizontal delta, with soft wrap off, up to the widest
line laid out so far; with it on, rows that break after a space or
inside a word, numbered on their first row only; the scrollbars' ranges
and dragging them; the long-line cut at a grapheme cluster boundary and
its marker, and bounded frames of a 100 MB line; a 10,000-line file
scrolled to its end a notch at a time with at most 52 draw commands a
frame; a property test, which prints its seed when it fails, that points
and byte offsets round-trip at grapheme cluster boundaries; bad positions
and selections refused; the view's accessibility node; and an idle view
drawing nothing. Unit tests check its layouts: pieces, markers inside a
cluster, wrapping, and caret places. The goldens of the view and its
`tidepool-bench` suite are not written yet.

The shell's golden scenes are drawn by the
`tidepool` binary's `tests/golden.rs`, which paints it with `gui`;
`cargo xtask golden` compares them, and `cargo xtask ci` runs the whole
gate.

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
