---
paths:
  - "crates/ui/**"
  - "crates/gui/**"
  - "crates/tui/**"
  - "crates/editor/**"
  - "themes/**"
  - "assets/**"
---

# Rules for UI and the jabbott.dev look

Read `docs/subsystems/ui-and-theme.md` first. The reference is jabbott.dev's
stylesheet (`jabbott-iii/jabbott-iii.github.io`, `assets/css/style.css`).

- **Tokens only.** Widgets and views take colors, sizes and fonts from the
  theme. No literal colors in code outside `themes/` and tests.
- **Uppercase is for chrome** (labels, buttons, eyebrows, table headers,
  section titles), with the spec's letter-spacing. Never transform user
  content: file names, paths, code, symbols, search text, commit messages.
- **Contrast:** every text/background pair meets WCAG AA; `cargo xtask
  theme` checks the theme file. Follow the spec's surface rules (no
  `text-3` on `slate`; comment color only on `editor-bg`).
- **Fonts:** Saira for chrome, JetBrains Mono for code, terminal, numbers
  and paths. Only the bundled static `.ttf` files; their OFL license files
  stay beside them.
- **Motion is opt-in.** No animation unless `ui.ambient_motion` is on;
  never animate while the window is unfocused; idle emits no damage.
- **Accessibility:** every interactive widget has a role and name in the
  `a11y` tree; keyboard focus shows the `teal` focus outline.
- **Frontend-agnostic:** features live in `editor` and render through `ui`
  draw lists. `gui` and `tui` contain no feature logic.
- **TUI:** use the theme's solid and 256-color values; box-drawing for
  borders and bracket corners; restore the terminal on every exit path.
- **Vim mode** follows `docs/subsystems/vim-mode.md`; unsupported keys show
  a status message instead of guessing.
- **Goldens:** a visual change re-blesses only after the user has reviewed
  the diff images.
