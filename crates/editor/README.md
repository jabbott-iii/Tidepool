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

Not yet, with the roadmap task that adds it:

- the editor view — M4.1.
- commands and the keymap engine — M4.6.
- editing commands, multi-cursor, undo and clipboard — M4.2.
- explorer, tabs, splits, the trust prompt — M4.3.
- search — M4.4.
- the terminal panel — M5.4.
- the Toolchains panel — M5.6.
- highlighting — M6.2.
- language features — M6.7, M6.8, M6.3.
- the Markdown preview — M6.6.
- vim mode — M7.2, M7.3, M7.4.
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

## Documentation

- Design: [`docs/subsystems/editor.md`](../../docs/subsystems/editor.md),
  [`docs/subsystems/vim-mode.md`](../../docs/subsystems/vim-mode.md),
  [`docs/subsystems/ui-and-theme.md`](../../docs/subsystems/ui-and-theme.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0003 (GUI
  first, features in `editor`); ADR-0006 (vim mode, Proposed); ADR-0060
  (the shell in `editor::shell`, its icons, settings and palette).
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
```

The tests build the shell headless: its areas tile the window at
800 × 600, 1280 × 800 and 1920 × 1080 and stay inside a smaller one;
hidden panels give their space to the rest; Ctrl+B and Ctrl+J toggle
them; the activity bar switches views and hides the shown one; the
palette ranks the commands as its query changes, runs the first with
Enter, closes with Escape and blocks the shortcuts while open; every
command runs; a tooltip waits 500 ms for the pointer and none for focus;
the accessibility tree names every part; and an idle shell draws nothing.
The matcher's unit tests check word starts, runs, the best alignment and
the matched ranges. The shell's golden scenes are drawn by the
`tidepool` binary's `tests/golden.rs`, which paints it with `gui`;
`cargo xtask golden` compares them, and `cargo xtask ci` runs the whole
gate.

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
