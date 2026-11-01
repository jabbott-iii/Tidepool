# 01 — Architecture

## Principles

1. **Layered crates, one-way dependencies.** Lower layers never know about
   higher ones. `cargo xtask layers` enforces the matrix below.
2. **The OS is quarantined.** OS declarations live in `sys`. OS behavior
   lives in `platform`. Nothing else touches an OS API, contains `unsafe`,
   or has a `target_os` cfg.
3. **Outside tools are processes, managed in one place.** rust-analyzer,
   git, the Android SDK, the JDK, Xcode, LSP servers, debug adapters and
   language agents run as child processes. Only `toolchain` finds, installs,
   updates and launches them (ADR-0002); the dev-only `xtask` runs `cargo`
   itself (ADR-0017). Other crates ask `toolchain` for a ready tool and
   speak to it over pipes.
4. **Frontends are thin.** `ui` builds frame-independent draw lists in
   logical units. `gui` rasterizes them into a window; `tui` maps them onto
   terminal cells. Every feature is written once, in `editor` (ADR-0003).
5. **The main thread never waits.** No file I/O, subprocess wait, LSP or
   agent round trip, or unbounded channel receive on the main thread.
   Plain `std` threads and channels; no async runtime (ADR-0012).
6. **Render on change, not on a clock.** The loop sleeps until input, a
   message or a timer deadline, and redraws only damaged regions (ADR-0011).
7. **The core binary opens no sockets.** Network access happens only in
   child processes the user approved: official installers, `curl`, `git`,
   `ssh`, language agents (ADR-0009).

## Layers

A crate may only depend on crates in lower layers, and only on the specific
crates its row in the dependency matrix allows.

| Layer | Crates |
| :- | :- |
| 6 — tools | `xtask`, `bench`, `testkit` (dev-only) |
| 5 — application | `app` (package `tidepool`, the binary) |
| 4 — views and frontends | `editor`, `gui`, `tui` |
| 3 — language and tool features | `lang`, `vcs`, `agent`, `mobile` |
| 2 — services | `rpc`, `toolchain`, `syntax`, `term`, `workspace`, `ui` |
| 1 — OS behavior, formats, pure engines | `platform`, `codec`, `text`, `regex`, `gfx`, `font`, `markdown` |
| 0 — foundations | `core`, `sys` |

Reference agents live in `agents/`, a separate Cargo workspace that follows
the same dependency policy and is never linked into Tidepool.

## Dependency matrix

A crate may depend on the workspace crates listed in its row and nothing
else. No crate may depend on a third-party crate unless the dependency
policy approves it.

| Crate | Allowed workspace dependencies |
| :- | :- |
| `tidepool-core` | — |
| `tidepool-sys` | — |
| `tidepool-platform` | `core`, `sys` |
| `tidepool-codec` | `core` |
| `tidepool-text` | `core` |
| `tidepool-regex` | `core` |
| `tidepool-gfx` | `core` |
| `tidepool-font` | `core` |
| `tidepool-markdown` | `core` |
| `tidepool-rpc` | `core`, `codec` |
| `tidepool-toolchain` | `core`, `platform`, `codec` |
| `tidepool-syntax` | `core`, `codec`, `text`, `regex` |
| `tidepool-term` | `core`, `platform` |
| `tidepool-workspace` | `core`, `platform`, `codec`, `text`, `regex` |
| `tidepool-ui` | `core` |
| `tidepool-lang` | `core`, `codec`, `text`, `markdown`, `rpc`, `toolchain`, `syntax`, `workspace` |
| `tidepool-vcs` | `core`, `text`, `toolchain` |
| `tidepool-agent` | `core`, `codec`, `text`, `rpc`, `toolchain`, `workspace` |
| `tidepool-mobile` | `core`, `codec`, `toolchain` |
| `tidepool-editor` | `core`, `codec`, `text`, `regex`, `markdown`, `syntax`, `term`, `workspace`, `ui`, `lang`, `vcs`, `agent`, `mobile` |
| `tidepool-gui` | `core`, `platform`, `codec`, `gfx`, `font`, `ui` |
| `tidepool-tui` | `core`, `platform`, `ui` |
| `tidepool` (app) | all crates above |
| `xtask` | `core`, `codec` |
| `tidepool-bench` | any workspace crate |
| `tidepool-testkit` | `core`, `codec`, `rpc` (used only as a dev-dependency) |

`editor` never depends on `gui` or `tui`, and the frontends never depend on
`editor`. `app` connects them.

## Workspace layout

```
.
├── Cargo.toml                # [workspace], [workspace.lints], [workspace.package], profiles
├── rust-toolchain.toml       # pinned stable toolchain + rustfmt, clippy
├── clippy.toml               # allow-unwrap-in-tests = true, allow-expect-in-tests = true
├── .cargo/config.toml        # aliases: xtask, tp
├── crates/{core,sys,platform,codec,text,regex,gfx,font,markdown,rpc,toolchain,
│           syntax,term,workspace,ui,lang,vcs,agent,mobile,editor,gui,tui,app,testkit}/
├── xtask/
├── bench/
├── agents/                   # separate workspace: tp-agent-mock, tp-agent-local, tp-agent-claude
├── assets/fonts/             # Saira and JetBrains Mono (SIL OFL 1.1) with their license files
├── assets/icons/             # Tidepool's own icon set (hand-made, SVG-like path data)
├── languages/                # built-in language packs (TOML)
├── toolchains/               # managed-tool descriptors (TOML): versions, URLs, SHA-256
├── themes/                   # tidepool-dark.toml (jabbott.dev tokens)
├── tests/golden/             # golden frames (.qoi) and TUI cell snapshots (.txt)
├── fuzz/corpus/              # seed inputs per parser
├── scripts/check-deps.sh     # interim dependency check until `cargo xtask deps` (M0.3)
└── docs/
```

`.cargo/config.toml`:

```toml
[alias]
xtask = "run --profile xtask --package xtask --"
tp    = "run --package tidepool --"
```

Workspace lints (`Cargo.toml`); every member uses `[lints] workspace = true`:

```toml
[workspace.lints.rust]
unsafe_code = "deny"              # sys and platform override with #![allow(unsafe_code)]
unsafe_op_in_unsafe_fn = "deny"
missing_docs = "warn"
unreachable_pub = "warn"

[workspace.lints.clippy]
undocumented_unsafe_blocks = "deny"
multiple_unsafe_ops_per_block = "deny"
unwrap_used = "deny"
print_stdout = "deny"
print_stderr = "deny"
dbg_macro = "deny"
todo = "deny"
```

`unsafe_code` is `deny`, not `forbid`, because `sys` and `platform` need
`#![allow(unsafe_code)]` at their crate roots. `cargo xtask unsafe-audit`
checks that the allow appears nowhere else.

## The main loop

There is one main thread. It owns the frontend (window or terminal), the UI
tree, every open buffer and all editor state.

```rust
pub fn run(args: Args) -> Result<(), Error> {
    let config = Config::load(&args)?;                          // settings.toml; bad values → defaults + warning
    let mut frontend = Frontend::open(&config, args.mode)?;     // gui::Window or tui::Terminal
    let (tx, rx) = std::sync::mpsc::channel::<Msg>();           // workers, child readers, timers → main
    let mut app = App::new(config, tx, frontend.waker())?;      // the waker interrupts wait_events from any thread
    let mut events = Vec::with_capacity(64);

    loop {
        let deadline = app.next_deadline();                     // cursor blink, debounce, toast; None = wait forever
        frontend.wait_events(deadline, &mut events)?;           // blocks: input, resize, focus, wake, or deadline
        for event in events.drain(..) {
            if app.handle_input(event) == Flow::Quit { return app.shutdown(); }
        }
        for msg in rx.try_iter().take(MAX_MSGS_PER_TURN) {      // bounded so a chatty server can't starve input
            app.handle_message(msg);                            // LSP results, agent deltas, tool output, file events
        }
        app.fire_timers(std::time::Instant::now());
        if let Some(frame) = app.build_frame(frontend.metrics()) {  // None when nothing is damaged
            frontend.present(&frame)?;                          // gui: rasterize damage + blit; tui: diff cells
        }
    }
}
```

Rules:

- **Input first.** All pending input is handled before messages, so typing
  is never queued behind background work.
- **Stale results are dropped.** Every request to a worker, server or agent
  carries the buffer version it was made for; a result for an older version
  is discarded.
- **Bounded work per turn.** Message handling is capped per loop turn, and
  any handler that could take more than ~1 ms moves its work to a worker.
- **Shutdown** asks child processes to exit (LSP `shutdown`/`exit`, agent
  `shutdown`), waits up to 2 s on a worker, then kills them. No orphans.

## Data flow

```
OS input ─► platform::Event ─► ui (focus, routing) ─► editor (keymap: default or vim mode)
                                                           │ commands
                                                           ▼
                                   text (rope transactions) ─► buffer version++
                                                           │
             ┌──────────────── snapshot (cheap rope clone) ┴──────────────────┐
             ▼                          ▼                         ▼             ▼
   syntax (highlight, worker)   lang ─► rpc ─► LSP server   agent ─► TAP agent   vcs ─► git
             │                          │                         │             │
             └──────── Msg (versioned) ─┴───────── mpsc ──────────┴─────────────┘
                                                           ▼
                         editor views ─► ui draw list (logical units, damage)
                                                           ▼
                    gui: gfx + font rasterize ─► platform present   |   tui: cells ─► terminal
```

## Threading model

| Thread | Owns | Talks to others via |
| :- | :- | :- |
| Main | Frontend, UI tree, buffers, editor state, settings | `mpsc` channels in; job queues out |
| Workers (N = clamp(cores − 1, 1, 4)) | File reads, search, indexing, highlighting of large buffers, hashing | Jobs in, versioned results out |
| One reader per child pipe | Framing and parsing for LSP, DAP, TAP, git, tool output | Parsed messages to main |
| PTY readers (one per terminal) | VT parsing into the terminal grid | Damage notifications to main |
| File watcher (one) | inotify / FSEvents / ReadDirectoryChangesW | Changed paths to main, debounced |

- Only main touches the UI tree or editor state.
- Buffers are shared with other threads only as immutable rope snapshots.
- Child processes are always spawned and reaped through `toolchain`, so
  shutdown can find every one of them.

## Frontends

| | `gui` (default) | `tui` (`tidepool --tui`) |
| :- | :- | :- |
| Output | CPU raster into a framebuffer, damage rectangles, platform blit | Cell grid diff, truecolor escape sequences (256-color fallback) |
| Type | Bundled Saira (UI chrome) and JetBrains Mono (code) | The terminal's font |
| jabbott.dev look | Full: translucent panels, cut corners, bracket corners, glow | Colors, uppercase labels, box-drawing borders; no translucency or fonts |
| Input | Keyboard, mouse, scroll, IME basics, clipboard | Keys (CSI/SS3, kitty keyboard protocol when offered), SGR mouse, bracketed paste, OSC 52 clipboard |
| Vim mode | `editor.keymap = "vim"`, off by default | Same setting; the first TUI launch asks whether to turn it on |

## Configuration and user data

- Settings live in `settings.toml` in the user config directory, with
  overrides in `<workspace>/.tidepool/settings.toml` (paths per OS are in
  `docs/03-platforms.md`). Keymaps (including vim mode), theme, fonts,
  editor behavior, toolchain locations and agent consent live here.
- The file is versioned. Unknown keys are preserved on save; invalid values
  fall back to defaults with a logged warning, never a crash.
- **Secrets are never stored** by Tidepool in v1: no keystore passwords, API
  keys or signing identities in any file Tidepool writes.
- Workspace trust decisions and session state (open files, layout) live in
  the user data directory.
- Logs: `tidepool.log` plus 3 rotated copies in the user data directory (on
  macOS, `~/Library/Logs/Tidepool`). Command lines are logged with secret
  arguments redacted.
- Managed tools install to each tool's standard location (rustup's own
  directories, the Android SDK where Android Studio would put it), so other
  tools and IDEs share them. Tidepool records only what it found or
  installed, in `toolchains.toml` in user data.
