# Subsystem: vim mode (in `tidepool-editor`)

Vim-style modal editing for both frontends (ADR-0006). The user requires it
in the terminal UI; because it lives in `editor`, the GUI gets it too.

Turn it on with `editor.keymap = "vim"` in `settings.toml`. The first TUI
launch asks whether to turn it on.

## Design

- The keymap engine (M4.2) is modal from the start. The default keymap is a
  single `Insert`-like mode; vim mode adds the others. Both resolve keys
  into the same `editor` commands, so undo, multi-cursor, LSP actions and
  agents behave the same.
- **Modes:** `Normal`, `Insert`, `Replace`, `Visual`, `VisualLine`,
  `VisualBlock`, `OperatorPending`, `CommandLine` (`:`), `Search` (`/`, `?`).
- **Command grammar:** `["register] [count] operator [count] motion|text-object`,
  or `[count] action`. A parser state machine consumes keys and emits a
  complete command or waits; `Esc` cancels.
- **Undo:** each Normal-mode command and each Insert session is one undo
  step, matching Vim.
- **Dot-repeat** replays the last change, including the text typed in its
  Insert session.
- **Registers:** unnamed `"`, `0`–`9`, `a`–`z` (append with `A`–`Z`), `-`,
  `_` (black hole), `+` and `*` (system clipboard via the frontend), `/`
  (last search), `.` (last insert), `:` (last command).
- **Mode display:** the status bar shows `NORMAL`, `INSERT`, `VISUAL`… in
  the chip style; the cursor is a block in Normal mode and a bar in Insert.
- **Leader key:** `editor.vim.leader` (default `\`), so users can bind
  Tidepool commands (`<leader>f` → file finder).
- **Mappings:** `[editor.vim.map.normal]` etc. in settings: `"jk" =
  "<Esc>"` style, non-recursive only. No Vimscript, no `.vimrc` loading
  (non-goal). An importer for simple `nnoremap`/`inoremap` lines is backlog.

## Supported subset (v1)

| Group | Commands |
| :- | :- |
| Motions | `h j k l`, `w W b B e E ge gE`, `0 ^ $ g_`, `gg G` (with count = line), `f F t T ; ,`, `%`, `{ }`, `( )`, `H M L`, `*  #`, `n N`, `` ` `` and `'` to marks |
| Operators | `d c y`, `>` `<`, `=` (LSP format range), `g~ gu gU`, `gq` (wrap) |
| Text objects | `iw aw iW aW`, `is as`, `ip ap`, `i( a( ib ab`, `i[ a[`, `i{ a{ iB aB`, `i< a<`, `i" a" i' a'`, ``i` a` ``, `it at` |
| Actions | `x X`, `s S`, `r`, `R`, `J gJ`, `p P`, `u`, `Ctrl-r`, `.`, `~`, `i a I A o O`, `Ctrl-a Ctrl-x`, `D C Y` |
| Visual | `v V Ctrl-v`, `o`, `gv`, operators on selections, block insert `I`/`A` |
| Marks | `m{a-z}`, `` `{a-z} ``, `'{a-z}`, ```` `` ```` (jump back) |
| Macros | `q{a-z}` … `q`, `@{a-z}`, `@@` |
| Search | `/ ?` with `tidepool-regex` syntax (documented differences from Vim's), `n N`, `*`, `#` |
| Scrolling | `Ctrl-d Ctrl-u Ctrl-f Ctrl-b Ctrl-e Ctrl-y`, `zz zt zb` |
| Windows | `Ctrl-w h/j/k/l`, `Ctrl-w s/v/c/o` mapped to Tidepool splits |
| Insert mode | `Ctrl-w`, `Ctrl-u`, `Ctrl-r {reg}`, `Ctrl-o {cmd}`, `Ctrl-n`/`Ctrl-p` (completion popup) |
| Ex commands | `:w :q :wq :x :q! :wa :qa`, `:e {file}`, `:sp :vs`, `:{n}`, `:s/…/…/[gci]` and `:%s`, `:g/…/d`, `:noh`, `:set` for `number relativenumber wrap ignorecase smartcase tabstop shiftwidth expandtab`, `:b {n}`, `:bn :bp`, `:registers`, `:marks` |
| IDE bindings | `gd` (go to definition), `gr` (references), `K` (hover), `[d ]d` (diagnostics), `<leader>a` (code actions), `gcc`/`gc` (toggle comment) |

Anything not listed is a no-op with a status-bar message
(`Not supported in vim mode: <keys>`), never a silent wrong edit.

## Interaction with other features

- **Multi-cursor:** in vim mode, `Ctrl-n` on a word in Normal mode adds a
  cursor at the next match (like vim-multiple-cursors); commands apply to
  every cursor.
- **Completion and ghost text:** shown only in Insert mode. `Tab` accepts
  ghost text; `Ctrl-n`/`Ctrl-p` move through the popup.
- **Terminal panel:** keys go to the terminal program while the terminal
  has focus. `Ctrl-\ Ctrl-n` returns focus to the editor (as in Neovim).
- **Vim inside the integrated terminal** is a separate thing and always
  works (`docs/subsystems/toolchains.md`, `tidepool-term`).

## Tests

- **Conformance suite** (`crates/editor/tests/vim/`): each row of the table
  has cases written as `before | keys | after` fixtures, with the cursor
  marked. Where a real `vim` is available (`TIDEPOOL_IT_VIM=1`), a checker
  replays the same keys in `vim -u NONE -N` headless and compares buffers.
- Property test: any key sequence leaves the buffer valid, the cursor in
  bounds, and `u` repeated enough times restores the original text.
- TUI snapshot: a scripted vim-mode session renders mode chips and cursor
  shapes as expected.
