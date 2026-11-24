# C4 architecture: containers and components

These diagrams show Tidepool in two of the C4 model's views. The container
view shows the programs, the files they use, and the people and outside
systems around them. The component views show the crates inside the
`tidepool` binary, the modules inside `xtask`, and the parts that exist only
for development: the fuzzer, the golden frames, the test harness, the
benchmark harness, and the fake tool crate still to come.

Drawn from the code and docs as of roadmap task M3.4 (2026-11-22, version 0.1), after M2.4, M2.7, M2.8, M2.5, M2.6, M3.1, M3.2, M3.5, M3.6, M3.7, M3.3 and M3.8. What the code has is drawn solid; what the roadmap plans is dashed and labelled with the task that builds it (ADR-0029).

## Legend

| Style | Means |
| :- | :- |
| Dark blue, rounded | A person |
| Blue box | Built: a container, crate or module the code has now |
| Blue cylinder | Files the code reads or writes now |
| Grey box or cylinder | Outside Tidepool: a program, a service, or the developer's own files |
| White box or cylinder with a dashed border | Planned: not built yet; the label names the task that builds it |
| Solid arrow | Uses, runs, reads, writes or depends on, as labelled, in the code now |
| Dotted arrow labelled "planned M#.#" | Planned: added by that roadmap task |

A crate that has code is drawn solid even where most of it is still
planned; its label says which parts are which. So `sys`, `platform` and
the binary are solid: M0.1 gave the first two a crate root that allows
`unsafe_code`, and the binary an empty `main`, M2.1 gave `sys` its
Linux declarations, and M2.2 gave `platform` its Linux window. A crate that is only its doc
comment, and for a library the `ReadmeDoctests` item that runs its README's
examples (M0.13) and the attribute that forbids `unsafe` in them (M0.15),
is drawn planned, with the task that gives it code first.

## 1. Containers

Tidepool is one binary for the developer, and dev-only tools for the people
who work on Tidepool. The two are drawn apart, and the binary in three
views: its frontends and files, the tools it drives, and the language
agents.

### 1.1 The tidepool binary, its frontends and its files

The binary is `crates/app`, run as `cargo tp`. It has a GUI and a TUI
frontend. Its `main` runs the event-driven main loop (M3.8, ADR-0012,
ADR-0059): it reads two settings, opens the GUI's window and shows the
shell (M3.4, ADR-0060), a `ui` tree the GUI paints, until the window is
closed; every other arrow from it is planned.

- Tidepool's TOML files are `settings.toml` in the user config directory,
  the keymaps and `languages/ID.toml` (M6.2). The default keymap,
  `keymaps/default.toml`, is compiled into `tidepool-editor` (M4.6,
  ADR-0064). The binary reads `settings.toml`'s `[ui] side_panel` and
  `bottom_panel` (M3.4, ADR-0060) and its `[keymap]` tables, the user's
  bindings (M4.6, ADR-0064), read-only, through `tidepool-workspace`'s
  settings (M4.7, ADR-0067), which can also write a value back with the
  file's comments kept; the binary writes nothing yet.
- The theme is not read at run time: `tidepool-ui` compiles
  `themes/tidepool-dark.toml` in with `include_str!`, and `gui`'s first
  frame takes its two text colors, `text` and `text-3`, from it (M3.2,
  ADR-0047); its backdrop is still `gfx`'s `BackdropStyle::jabbott_dev()`. The file is drawn with the repository's
  files (1.5); no task loads a user's theme yet.
- The managed-tool records are the `toolchains/TOOL.toml` descriptors and
  `toolchains.toml` in user data, which records what was found (M5.1).
- A workspace folder is one of the developer's projects, with its own
  `.tidepool/settings.toml`; the binary is to open, trust, edit and save
  it (M4.3), through `tidepool-workspace`'s folders and trust (M4.7).

```mermaid
flowchart LR
    p_dev(["Developer<br/>edits Rust and Markdown,<br/>builds and signs<br/>Android and iOS apps"])

    subgraph b_tidepool["Tidepool"]
        subgraph b_bin["The tidepool binary"]
            c_app["Main loop and wiring<br/>the event-driven loop (M3.8),<br/>the shell's wiring (M3.4),<br/>--print-startup-time (M2.6)"]
            c_gui["GUI frontend, tidepool-gui<br/>a native window<br/>in the jabbott.dev look<br/>today the first frame (M2.6),<br/>a ui tree it paints (M3.8),<br/>in the bundled theme (M3.2)"]
            c_tui["TUI frontend, tidepool-tui<br/>tidepool --tui, terminal cells<br/>planned M7.1"]
        end
        s_toml[("Settings and Tidepool's<br/>TOML files<br/>settings.toml read (M3.4),<br/>its [keymap] (M4.6),<br/>through workspace (M4.7),<br/>planned M6.2")]
        s_desc[("Managed-tool records<br/>descriptors, toolchains.toml<br/>planned M5.1")]
    end
    e_folders[("Workspace folders<br/>the developer's projects")]

    p_dev -->|"sees the first<br/>frame (M2.6)"| c_gui
    p_dev -.->|"planned M4.3:<br/>edits in it"| c_gui
    p_dev -.->|"planned M7.1:<br/>edits in a terminal"| c_tui
    c_app -->|"opens the window,<br/>loops until closed (M3.8)"| c_gui
    c_app -.->|"planned M7.1:<br/>with --tui"| c_tui
    c_app -->|"reads [ui] (M3.4),<br/>[keymap] (M4.6),<br/>through workspace (M4.7)"| s_toml
    c_app -.->|"planned M5.1:<br/>loads, records"| s_desc
    c_app -.->|"planned M4.3:<br/>opens, edits, saves"| e_folders

    classDef person fill:#08427b,stroke:#052e56,color:#ffffff
    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef external fill:#8a8a8a,stroke:#6b6b6b,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    class p_dev person
    class c_app,c_gui,s_toml built
    class c_tui,s_desc planned
    class e_folders external
```

### 1.2 The tools the binary drives

Every tool is found, installed, updated and started by
`tidepool-toolchain` (ADR-0002), with consent before any install; that
crate's first task is M5.1. The core binary opens no sockets but its
connection to the local X server, over a Unix socket only (ADR-0039),
and the network is reached only through child processes (ADR-0009). What each
arrow's task adds:

- `curl`, HTTPS only: every download, its SHA-256 checked (M5.3).
- rustup, the Rust toolchain and `cargo`: installed (M5.5), and cargo
  tasks run (M6.4).
- rust-analyzer: installed (M5.5), and spoken to in LSP over stdio (M6.7).
- `git`: detected (M5.5), and run for status and diff (M8.1).
- The JDK, the Android SDK and NDK, and bundletool: installed and driven
  (M10.1).
- Xcode and its command-line tools, on macOS only: detected, and the
  install guided (M11.2).
- A remote Mac (ADR-0008): `ios::remote` syncs the project to it with
  `rsync` over `ssh`, runs the iOS pipeline there and copies the artifacts
  back (M11.5).
- A debug adapter, `lldb-dap` or CodeLLDB: installed, and spoken to in DAP
  (M13.1).

```mermaid
flowchart LR
    c_app["The tidepool binary<br/>today the loop and the<br/>first frame (M3.8)"]
    e_curl["curl<br/>HTTPS only"]
    e_rust["rustup, the Rust<br/>toolchain, cargo"]
    e_ra["rust-analyzer<br/>LSP over stdio"]
    e_git["git"]
    e_android["JDK, Android SDK<br/>and NDK, bundletool"]
    e_xcode["Xcode and its<br/>command-line tools<br/>macOS only"]
    e_remote["A remote Mac<br/>reached over ssh,<br/>synced with rsync"]
    e_dap["Debug adapter<br/>lldb-dap or CodeLLDB"]

    c_app -.->|"planned M5.3:<br/>every download"| e_curl
    c_app -.->|"planned M5.5, M6.4:<br/>installs, cargo tasks"| e_rust
    c_app -.->|"planned M5.5, M6.7:<br/>installs, speaks LSP"| e_ra
    c_app -.->|"planned M5.5, M8.1:<br/>status and diff"| e_git
    c_app -.->|"planned M10.1:<br/>installs, drives them"| e_android
    c_app -.->|"planned M11.2:<br/>detects, guides install"| e_xcode
    c_app -.->|"planned M11.5:<br/>builds iOS there"| e_remote
    c_app -.->|"planned M13.1:<br/>installs, speaks DAP"| e_dap

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef external fill:#8a8a8a,stroke:#6b6b6b,color:#ffffff
    class c_app built
    class e_curl,e_rust,e_ra,e_git,e_android,e_xcode,e_remote,e_dap external
```

### 1.3 The language agents

Agents speak the Tidepool Agent Protocol (TAP) over stdio (M9.3). The
reference agents live in their own Cargo workspace, `agents/`, and are
never linked into Tidepool (`docs/01-architecture.md` → "Layers"):
`tp-agent-mock` (M9.2), and `tp-agent-local` and `tp-agent-claude` (M9.6),
which reach a model server through `curl`, or over plain HTTP on loopback.
Any other program with an `agent.toml` that speaks TAP is an agent too.

```mermaid
flowchart TB
    subgraph b_tidepool["Tidepool"]
        c_app["The tidepool binary<br/>today the loop and the<br/>first frame (M3.8)"]
        c_agents["Reference agents<br/>the agents/ workspace<br/>planned M9.2, M9.6"]
    end
    e_agents["Other language agents<br/>with an agent.toml,<br/>speaking TAP"]
    e_models["Model servers<br/>Anthropic's Messages API,<br/>a user-run server<br/>on 127.0.0.1"]

    c_app -.->|"planned M9.3:<br/>TAP over stdio"| c_agents
    c_app -.->|"planned M9.3:<br/>TAP over stdio"| e_agents
    c_agents -.->|"planned M9.6:<br/>curl, or HTTP on loopback"| e_models

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef external fill:#8a8a8a,stroke:#6b6b6b,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    class c_app built
    class c_agents planned
    class e_agents,e_models external
```

### 1.4 The development tools

`xtask` is built: the CI gate and fuzz smoke run through it, locally and on
GitHub Actions. It starts no program but `cargo` (ADR-0017, ADR-0027).

- `xtask` runs as `cargo xtask` with one of `ci`, `deps`, `layers`,
  `crates`, `features`, `unsafe-audit`, `headers`, `workflows`, `ucd`,
  `theme`, `golden`, `fuzz-smoke` and `site`. It starts `cargo` for `fmt`,
  `clippy`, `test`, `metadata`, `locate-project`, a `cargo test` of the
  golden scenes per drawing crate (ADR-0044), a `cargo test` per fuzz
  target, and `site`'s two `cargo run --example` (ADR-0030).
- `ci.yml` runs `cargo xtask ci`, whose `golden` step compares the golden
  frames, on Linux, macOS and Windows, on Linux under `xvfb-run` with the
  window tests on (ADR-0041), then `cargo xtask fuzz-smoke --secs 30` on
  Linux.
- `pages.yml` runs `cargo xtask site` once CI has passed on a push to
  `main`, and deploys the page it builds to GitHub Pages, at
  <https://jabbott.dev/Tidepool/> once the repository's Pages source is
  "GitHub Actions" (ADR-0030).
- `cargo` is the pinned toolchain, 1.99.0, from `rust-toolchain.toml`.

```mermaid
flowchart TB
    p_contrib(["Contributor<br/>works on Tidepool"])
    e_gha["GitHub Actions<br/>ci.yml on Linux (under<br/>xvfb-run), macOS and<br/>Windows; pages.yml"]
    e_cargo["cargo<br/>the pinned toolchain,<br/>1.99.0"]
    e_pages["GitHub Pages<br/>jabbott.dev/Tidepool/"]

    subgraph b_dev["Dev-only, in the repository"]
        c_xtask["xtask, run as cargo xtask<br/>ci, deps, layers, crates,<br/>features, unsafe-audit,<br/>headers, workflows, ucd,<br/>theme, golden, fuzz-smoke,<br/>site, bench"]
        c_bench["tidepool-bench, bench/<br/>the benchmark harness<br/>and the text suites (M1.9),<br/>the atlas fill (M2.5),<br/>the palette repaint (M3.3)"]
        c_testkit["tidepool-testkit<br/>planned: fake tools M5.1,<br/>fake LSP server M6.1"]
        s_baseline[("bench/baseline.json<br/>the reference machine's<br/>numbers (M1.9)")]
    end

    p_contrib -->|"runs cargo xtask"| c_xtask
    p_contrib -->|"runs cargo test,<br/>cargo tp"| e_cargo
    e_gha -->|"ci.yml: cargo xtask ci,<br/>then fuzz-smoke;<br/>pages.yml: cargo xtask site"| c_xtask
    e_gha -->|"pages.yml deploys<br/>target/site"| e_pages
    c_xtask -->|"starts only cargo"| e_cargo
    e_cargo -->|"cargo xtask bench:<br/>cargo run --release<br/>builds and runs it"| c_bench
    c_bench -->|"compares with it;<br/>--save writes it"| s_baseline
    e_cargo -.->|"planned M5.1, M6.1:<br/>tests use its fakes"| c_testkit

    classDef person fill:#08427b,stroke:#052e56,color:#ffffff
    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef external fill:#8a8a8a,stroke:#6b6b6b,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    classDef store fill:#1168bd,stroke:#0b4884,color:#ffffff
    class p_contrib person
    class c_xtask,c_bench built
    class s_baseline store
    class c_testkit planned
    class e_gha,e_cargo,e_pages external
```

- Which crates' tests may use `tidepool-testkit` is decided in M5.1: today
  the matrix lets only `tidepool-bench` use it (`docs/04-quality.md` →
  `layers`).

### 1.5 What the development tools read and write

`xtask` reads the repository's files and changes only three things: the
generated grapheme tables, the golden references of the scenes that
`golden --bless` names, and, after a replay that died, the name of the
input the replay died on. `site` also writes the demo page into
`target/site`, or the directory `--out` names, and `golden` what the
scenes drew, and a failing scene's images, into `target/golden`.
`tidepool-bench`, which `cargo xtask bench` runs, reads
`bench/baseline.json` and writes it only with `--save` (ADR-0035).
`theme` reads each `themes/*.toml`, and building `tidepool-ui` compiles
`themes/tidepool-dark.toml` in (M3.2, ADR-0047); building
`tidepool-editor` compiles `keymaps/default.toml` in, and the `keymap`
fuzz target reads `keymaps/` as seeds (M4.6, ADR-0064).
`codeql.yml` analyzes the Rust code and the workflows.

```mermaid
flowchart LR
    e_gha["GitHub Actions<br/>codeql.yml"]
    e_cargo["cargo<br/>started by xtask,<br/>or by the contributor"]

    subgraph b_dev["The repository"]
        c_xtask["xtask"]
        c_bench["tidepool-bench"]
        s_repo[("Manifests, lockfiles,<br/>.cargo/config.toml,<br/>source, READMEs,<br/>LICENSE and NOTICE<br/>copies, docs, workflows")]
        s_ucd[("crates/text/ucd/17.0.0<br/>the Unicode data files")]
        s_tables[("crates/text/src/pos/<br/>grapheme/tables.rs<br/>the generated tables")]
        s_fuzz[("fuzz/corpus/TARGET<br/>the seeds<br/>fuzz/crashes/TARGET<br/>the saved crashes")]
        s_site[("site/ the templates,<br/>docs/features.jsonc")]
        s_out[("target/site<br/>the demo page")]
        s_golden[("tests/golden/*.qoi<br/>the golden references")]
        s_goldout[("target/golden<br/>what the scenes drew,<br/>a failing scene's<br/>actual and diff images")]
        s_baseline[("bench/baseline.json")]
        s_themes[("themes/tidepool-dark.toml<br/>the bundled theme<br/>(M3.2)")]
        s_keymaps[("keymaps/default.toml<br/>the default keymap<br/>(M4.6)")]
    end

    e_gha -->|"codeql.yml:<br/>analyzes the Rust<br/>and the workflows"| s_repo
    c_xtask -->|"reads"| s_repo
    c_xtask -->|"ucd: checks each<br/>SHA-256 pin"| s_ucd
    c_xtask -->|"ucd writes it,<br/>--check compares"| s_tables
    c_xtask -->|"fuzz-smoke: checks<br/>its table against them,<br/>keeps died-HASH"| s_fuzz
    e_cargo -->|"cargo test runs<br/>the fuzz targets:<br/>reads the seeds,<br/>replays, saves crashes"| s_fuzz
    c_xtask -->|"features checks it;<br/>site fills the templates"| s_site
    c_xtask -->|"site writes it"| s_out
    c_xtask -->|"golden decodes them;<br/>--bless writes<br/>the scenes named"| s_golden
    c_xtask -->|"golden empties it,<br/>reads the PAM files,<br/>writes the images"| s_goldout
    e_cargo -->|"cargo test runs<br/>the golden scenes:<br/>each writes its PAM"| s_goldout
    e_cargo -->|"cargo xtask bench:<br/>cargo run --release"| c_bench
    c_bench -->|"reads; --save<br/>writes it"| s_baseline
    c_xtask -->|"theme: reads<br/>each themes/*.toml,<br/>checks its contrast"| s_themes
    e_cargo -->|"building tidepool-ui<br/>compiles it in<br/>(include_str!)"| s_themes
    e_cargo -->|"building tidepool-editor<br/>compiles it in; the<br/>keymap target reads it"| s_keymaps

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef external fill:#8a8a8a,stroke:#6b6b6b,color:#ffffff
    classDef store fill:#1168bd,stroke:#0b4884,color:#ffffff
    class c_xtask,c_bench built
    class s_repo,s_ucd,s_tables,s_fuzz,s_site,s_out,s_golden,s_goldout,s_baseline,s_themes,s_keymaps store
    class e_gha,e_cargo external
```

- `fuzz/crashes/` holds files only after a finding or a replay; the
  engine makes its directories as it needs them.

## 2. Components of the tidepool binary

The binary's crates by layer (`docs/01-architecture.md` → "Layers"), in
five diagrams. Every arrow is an edge of the dependency matrix. Only
`platform` (on `core` and `sys`, M2.2), `gfx` (on `core`, M2.3), `gui`
(on `core`, `platform`, `gfx` and `font`, M2.6, and `ui`, M3.2), the
binary (on `core`, `platform` and `gui`, M2.6, `ui`, M3.8, and `codec`
and `editor`, M3.4), `editor` (on `core` and `ui`, M3.4, `text`, M4.1,
and `codec`, M4.6) and `ui` (on
`core`, M3.1, `codec`, M3.2, and `text`, M3.6) declare dependencies yet; their
edges, and the three dev-dependencies on `tidepool-core` that the tests
of `codec`, `text` and `font` use, are solid. Every
other edge is dotted and labelled with the task the roadmap or spec ties to
its use. The diagrams run left to right, from a crate to the crates it
uses.

### 2.1 Layers 0 and 1: foundations, OS behavior, formats and engines

`core`, `codec` and `text` are built, `sys` has its Linux declarations
(M2.1, M2.2, M2.7), and `platform` its Linux window over X11 (M2.2), its
clipboard and the user's and tools' folders (M2.7). Those two
allow `unsafe_code`. `gfx` has its CPU rasterizer and the backdrop (M2.3),
and its golden scenes (M2.4). `font` has its TrueType parser and glyph
rasterizer (M2.8), and the bundled faces, fallback, layout, the glyph
atlas and its golden scenes (M2.5). The other two crates are empty.

```mermaid
flowchart LR
    subgraph L1["Layer 1"]
        k_platform["tidepool-platform<br/>built: the crate root, which<br/>allows unsafe_code (M0.1)<br/>window, events with the<br/>Waker, surface, on X11<br/>over a Unix socket (M2.2)<br/>clipboard, paths,<br/>dirs (M2.7)<br/>planned: watch M4.5,<br/>process M5.7, pty M5.9,<br/>terminal M7.1, macOS<br/>M11.7, M11.8, M11.1,<br/>Windows M12.1, M12.2"]
        k_codec["tidepool-codec<br/>json: JSON and JSONC,<br/>compact and pretty<br/>writers (M1.4)<br/>toml: the TOML 1.1<br/>parser with spans (M1.7)<br/>and Document, which<br/>edits it (M1.5)<br/>Limits, CodecError<br/>qoi: QOI images, decoded<br/>and encoded (M2.4)<br/>planned M10.7,<br/>M10.2, M11.3"]
        k_text["tidepool-text<br/>rope, pos with grapheme<br/>tables (M1.1)<br/>selection, transaction,<br/>history (M1.2)<br/>encoding, file with<br/>the atomic save (M1.3)"]
        k_regex["tidepool-regex<br/>planned M4.8: parser,<br/>NFA, Pike VM,<br/>Unicode classes"]
        k_gfx["tidepool-gfx<br/>framebuffer, raster,<br/>blend, clip, image,<br/>glyph, damage,<br/>backdrop (M2.3)<br/>golden scenes in<br/>tests/golden.rs (M2.4)"]
        k_font["tidepool-font<br/>ttf, outline, Limits,<br/>FontError (M2.8)<br/>bundled, fallback,<br/>layout, atlas, golden<br/>scenes (M2.5)"]
        k_markdown["tidepool-markdown<br/>planned M6.5:<br/>CommonMark and<br/>GFM parser"]
    end
    subgraph L0["Layer 0: foundations"]
        k_core["tidepool-core<br/>error, log, testing, rng,<br/>hash with sha256, cli,<br/>civil, input, geom,<br/>color, cancel (M0.2)<br/>testing::fuzz (M1.6)<br/>path (M1.8)"]
        k_sys["tidepool-sys<br/>built: the crate root, which<br/>allows unsafe_code (M0.1)<br/>dynlib, linux::libc,<br/>linux::x11 and its<br/>Xlib table (M2.1)<br/>write(2), setlocale,<br/>linux::evdev, typed<br/>text (M2.2)<br/>selections and<br/>properties (M2.7)<br/>planned: inotify M4.5,<br/>processes and PTY<br/>M5.7, M5.9, terminal<br/>M7.1, macOS M11.7,<br/>M11.8, Windows<br/>M12.1, M12.2"]
    end

    k_platform -->|"input, geom, log"| k_core
    k_platform -->|"Xlib, libc, evdev"| k_sys
    k_codec -->|"dev-dependency: tests"| k_core
    k_text -->|"dev-dependency: tests"| k_core
    k_regex -.->|"planned M4.8"| k_core
    k_gfx -->|"geom, color;<br/>tests: testing, rng"| k_core
    k_font -->|"dev-dependency: tests"| k_core
    k_markdown -.->|"planned M6.5"| k_core

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    class k_platform,k_codec,k_text,k_gfx,k_font,k_core,k_sys built
    class k_regex,k_markdown planned
```

- The libraries of `codec`, `text` and `font` use nothing from `core`;
  the tests of `codec` and `text` use `core::testing` (property tests and
  the fuzz engine) and `core::rng`, and `font`'s use the fuzz engine and
  `core::hash`'s FNV-1a hash.

### 2.2 Layer 2: services

`ui` has its tree, layout, focus, input routing, text metrics, draw
lists and accessibility tree (M3.1), its theme, which reads theme
files with `codec`'s TOML parser (M3.2, ADR-0047), its controls,
whose fields move and delete by grapheme cluster with `text`'s
`pos::grapheme` (M3.6, ADR-0049), and its collections, tab strip,
splitter, card and code block, which cut what they show at the same
boundaries (M3.7, ADR-0052), and its overlays: the modal dialog, the
tooltip, the toast, and the command palette, over a layer of overlays in
the tree, the palette's rows widening what a query matched to the same
boundaries (M3.3, ADR-0052). `workspace` has its folders and ignore
files, trust and settings (M4.7, ADR-0067), reading TOML with `codec`
and saving settings with `text::file`; the other four service crates are
empty. Each arrow is labelled with the task whose module first needs that
crate.

```mermaid
flowchart LR
    subgraph L2["Layer 2: services"]
        k_rpc["tidepool-rpc<br/>planned M6.1: JSON-RPC<br/>transport, LSP messages<br/>TAP messages M9.3<br/>DAP M13.1"]
        k_toolchain["tidepool-toolchain<br/>planned M5.1: descriptor,<br/>detect, registry<br/>process, sessions,<br/>secret M5.2<br/>download, install M5.3"]
        k_syntax["tidepool-syntax<br/>planned M6.2: Rust,<br/>Markdown and TOML<br/>lexers, language packs<br/>declarative pack<br/>lexers M13.2"]
        k_term["tidepool-term<br/>planned M5.8:<br/>VT parser and grid"]
        k_workspace["tidepool-workspace<br/>folder, trust,<br/>settings (M4.7)<br/>planned: buffers,<br/>session M4.3<br/>search M4.4, watch M4.5"]
        k_ui["tidepool-ui<br/>tree, layout, focus,<br/>input, metrics, draw,<br/>a11y, UiError (M3.1)<br/>theme (M3.2)<br/>widgets: the<br/>controls (M3.6),<br/>collections, tab strip,<br/>splitter, card, code<br/>block (M3.7)<br/>overlay, and the<br/>modal, tooltip, toast,<br/>palette (M3.3)"]
    end
    subgraph L01["Layers 0 and 1"]
        k_platform["tidepool-platform"]
        k_codec["tidepool-codec"]
        k_text["tidepool-text"]
        k_regex["tidepool-regex<br/>planned M4.8"]
        k_core["tidepool-core"]
    end

    k_rpc -.->|"planned M6.1"| k_core
    k_rpc -.->|"planned M6.1"| k_codec
    k_toolchain -.->|"planned M5.1"| k_core
    k_toolchain -.->|"planned M5.1"| k_codec
    k_toolchain -.->|"planned M5.1"| k_platform
    k_syntax -.->|"planned M6.2"| k_core
    k_syntax -.->|"planned M6.2"| k_codec
    k_syntax -.->|"planned M6.2"| k_text
    k_syntax -.->|"planned M13.2"| k_regex
    k_term -.->|"planned M5.8"| k_core
    k_term -.->|"planned M5.4"| k_platform
    k_workspace -->|"path, log (M4.7)"| k_core
    k_workspace -.->|"planned M4.3"| k_platform
    k_workspace -->|"toml (M4.7)"| k_codec
    k_workspace -->|"file::save, Rope (M4.7)"| k_text
    k_workspace -.->|"planned M4.4"| k_regex
    k_ui -->|"geom, color, input;<br/>tests: testing, rng"| k_core
    k_ui -->|"theme: toml, Limits,<br/>CodecError (M3.2)"| k_codec
    k_ui -->|"widgets' fields (M3.6),<br/>cuts (M3.7) and<br/>matches (M3.3):<br/>pos::grapheme"| k_text

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    class k_platform,k_codec,k_text,k_core,k_ui,k_workspace built
    class k_rpc,k_toolchain,k_syntax,k_term,k_regex planned
```

- `toolchain` first needs `platform` in M5.1, whose `detect` searches the
  standard tool locations that `platform::dirs` gives (M2.7). It reaches
  the OS through `platform::process` (M5.7, used from M5.2) and starts
  every outside program Tidepool drives. `term` gets its PTY from
  `platform::pty` (M5.9); the terminal panel (M5.4) reaches it through
  `term`, since `editor`'s row has no `platform`.

### 2.3 Layer 3: language and tool features

The four feature crates are empty. They reach outside programs only
through `toolchain`.

```mermaid
flowchart LR
    subgraph L3["Layer 3: features"]
        k_lang["tidepool-lang<br/>planned M6.7: registry,<br/>session, rust<br/>features M6.8, M6.3<br/>cargo M6.4<br/>markdown M6.6<br/>dap M13.1"]
        k_vcs["tidepool-vcs<br/>planned M8.1: git status,<br/>diff hunks, gutter<br/>blame and history M8.3"]
        k_agent["tidepool-agent<br/>planned M9.3: manifest,<br/>host, tap, context,<br/>consent<br/>predict, merge M9.4"]
        k_mobile["tidepool-mobile<br/>planned M10.2: inspect<br/>Android signing M10.3<br/>Gradle M10.4, iOS M11.3"]
    end
    subgraph L2["Layer 2: services"]
        k_rpc["tidepool-rpc<br/>planned M6.1"]
        k_toolchain["tidepool-toolchain<br/>planned M5.1"]
        k_syntax["tidepool-syntax<br/>planned M6.2"]
        k_workspace["tidepool-workspace<br/>folder, trust,<br/>settings (M4.7)"]
    end
    subgraph L01["Layers 0 and 1"]
        k_codec["tidepool-codec"]
        k_text["tidepool-text"]
        k_markdown["tidepool-markdown<br/>planned M6.5"]
        k_core["tidepool-core"]
    end

    k_lang -.->|"planned M6.7"| k_core
    k_lang -.->|"planned M6.4"| k_codec
    k_lang -.->|"planned M6.7"| k_text
    k_lang -.->|"planned M6.6"| k_markdown
    k_lang -.->|"planned M6.7"| k_rpc
    k_lang -.->|"planned M6.7"| k_toolchain
    k_lang -.->|"planned M6.7"| k_syntax
    k_lang -.->|"planned M6.7"| k_workspace
    k_vcs -.->|"planned M8.1"| k_core
    k_vcs -.->|"planned M8.1"| k_text
    k_vcs -.->|"planned M8.1"| k_toolchain
    k_agent -.->|"planned M9.3"| k_core
    k_agent -.->|"planned M9.3"| k_codec
    k_agent -.->|"planned M9.3"| k_text
    k_agent -.->|"planned M9.3"| k_rpc
    k_agent -.->|"planned M9.3"| k_toolchain
    k_agent -.->|"planned M9.3"| k_workspace
    k_mobile -.->|"planned M10.2"| k_core
    k_mobile -.->|"planned M10.2"| k_codec
    k_mobile -.->|"planned M10.3"| k_toolchain

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    class k_codec,k_text,k_core,k_workspace built
    class k_lang,k_vcs,k_agent,k_mobile,k_rpc,k_toolchain,k_syntax,k_markdown planned
```

- `lang` and `agent` start their servers and agents through
  `toolchain::sessions` (M5.2) and speak to them over `rpc`'s JSON-RPC
  transport; `vcs` runs `git` through `toolchain::process`.

### 2.4 Layer 4: the editor

`tidepool-editor` is where every feature is written once (ADR-0003); it
draws through `ui` and never depends on `gui` or `tui`. Its thirteen edges
are added as its parts land: those to `core` and `ui` came with the shell
(M3.4), the one to `text` with the editor view (M4.1), and the one to
`codec` with the keymaps (M4.6).

```mermaid
flowchart LR
    k_editor["tidepool-editor<br/>the shell (M3.4)<br/>the editor view (M4.1)<br/>commands, keymaps (M4.6)<br/>planned: vim mode M7.2<br/>every panel"]

    subgraph L3["Layer 3"]
        k_lang["tidepool-lang<br/>planned M6.7"]
        k_vcs["tidepool-vcs<br/>planned M8.1"]
        k_agent["tidepool-agent<br/>planned M9.3"]
        k_mobile["tidepool-mobile<br/>planned M10.2"]
    end
    subgraph L2["Layer 2"]
        k_syntax["tidepool-syntax<br/>planned M6.2"]
        k_term["tidepool-term<br/>planned M5.8"]
        k_workspace["tidepool-workspace<br/>folder, trust,<br/>settings (M4.7)"]
        k_ui["tidepool-ui<br/>(M3.1, M3.2, M3.6,<br/>M3.7, M3.3, M4.1)"]
    end
    subgraph L01["Layers 0 and 1"]
        k_codec["tidepool-codec"]
        k_text["tidepool-text"]
        k_regex["tidepool-regex<br/>planned M4.8"]
        k_markdown["tidepool-markdown<br/>planned M6.5"]
        k_core["tidepool-core"]
    end

    k_editor -->|"input, geometry (M3.4)"| k_core
    k_editor -->|"Rope, Selection,<br/>clusters (M4.1)"| k_text
    k_editor -->|"Tree, controls,<br/>palette (M3.4);<br/>the view's widgets,<br/>scrollbars (M4.1)"| k_ui
    k_editor -->|"toml: keymaps (M4.6)"| k_codec
    k_editor -.->|"planned M4.3:<br/>explorer, tabs"| k_workspace
    k_editor -.->|"planned M4.4:<br/>find and replace"| k_regex
    k_editor -.->|"planned M5.4:<br/>terminal panel"| k_term
    k_editor -.->|"planned M6.2:<br/>highlighting"| k_syntax
    k_editor -.->|"planned M6.7:<br/>diagnostics"| k_lang
    k_editor -.->|"planned M6.6:<br/>Markdown preview"| k_markdown
    k_editor -.->|"planned M8.2:<br/>source control panel"| k_vcs
    k_editor -.->|"planned M9.4: completion<br/>merge, ghost text"| k_agent
    k_editor -.->|"planned M10.6:<br/>SDK, devices, logcat"| k_mobile

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    class k_editor,k_codec,k_text,k_core,k_ui,k_workspace built
    class k_lang,k_vcs,k_agent,k_mobile,k_syntax,k_term,k_regex,k_markdown planned
```

### 2.5 Layers 4 and 5: the frontends and the binary

Today `gui` paints `ui`'s draw lists with its painter (M3.5, ADR-0048)
in its window (M3.8, ADR-0059): the first frame's lines (M2.6), a `ui`
tree in `text` and `text-3` from `ui`'s bundled theme (M3.2, ADR-0047),
over the backdrop it builds off the main thread, until the shell replaces
them with the shell (M3.4, ADR-0060), whose activity bar shows `gui`'s
first icons; `tui` is to map them onto cells (M7.1). The frontends
know nothing of editors, languages or tools
(`docs/subsystems/frontends.md`). `app` connects
them to `editor`'s shell (M3.4), and is to show its editor view from
M4.3, when files open (ADR-0063); it reads its settings through
`workspace::settings` (M4.7, ADR-0067). The matrix lets `app` use every
crate in the rows above its own; only eight edges are drawn, each
labelled with the task whose work needs it.

```mermaid
flowchart LR
    k_app["tidepool, crates/app:<br/>the binary<br/>--print-startup-time (M2.6)<br/>the main loop (M3.8)<br/>the shell's wiring,<br/>settings (M3.4, M4.7)"]
    subgraph L4["Layer 4: views, frontends"]
        k_editor["tidepool-editor<br/>the shell (M3.4)<br/>the editor view (M4.1)<br/>commands, keymaps (M4.6)"]
        k_gui["tidepool-gui<br/>Gui, first_frame,<br/>GuiError (M2.6)<br/>the theme's text<br/>colors (M3.2)<br/>Painter, FontMetrics,<br/>Icon (M3.5)<br/>render, the backdrop's<br/>builders (M3.8)<br/>icons, set_icon (M3.4)"]
        k_tui["tidepool-tui<br/>planned M7.1: renderer<br/>and input decoder"]
    end
    subgraph L2["Layer 2"]
        k_ui["tidepool-ui<br/>(M3.1, M3.2, M3.6,<br/>M3.7, M3.3, M4.1)"]
        k_workspace["tidepool-workspace<br/>folder, trust,<br/>settings (M4.7)"]
    end
    subgraph L01["Layers 0 and 1"]
        k_gfx["tidepool-gfx"]
        k_font["tidepool-font"]
        k_platform["tidepool-platform"]
        k_codec["tidepool-codec"]
        k_text["tidepool-text"]
        k_core["tidepool-core"]
    end

    k_app -->|"Gui (M2.6)"| k_gui
    k_app -.->|"planned M7.1: --tui"| k_tui
    k_app -->|"the shell (M3.4),<br/>the user's keymap (M4.6),<br/>planned M4.3:<br/>the editor view"| k_editor
    k_app -->|"Tree: input (M3.8),<br/>the shell's tree (M3.4)"| k_ui
    k_app -->|"toml: settings (M3.4),<br/>[keymap] (M4.6)"| k_codec
    k_app -->|"settings: Schema,<br/>SettingsFile (M4.7)"| k_workspace
    k_app -->|"WindowConfig (M2.6),<br/>Event (M3.8),<br/>user_dirs (M3.4)"| k_platform
    k_app -->|"command line (M2.6)"| k_core
    k_gui -->|"geometry,<br/>colors (M2.6)"| k_core
    k_gui -->|"window, events,<br/>present (M2.6),<br/>Waker (M3.8)"| k_platform
    k_gui -->|"backdrop, glyph<br/>masks, damage (M2.6);<br/>every primitive,<br/>layers (M3.5)"| k_gfx
    k_gui -->|"bundled faces,<br/>layout, atlas (M2.6);<br/>outlines for icons (M3.5)"| k_font
    k_gui -->|"Theme: text,<br/>text-3 (M3.2);<br/>draw lists, TextMetrics (M3.5);<br/>Tree::frame (M3.8);<br/>tests: the controls (M3.6),<br/>the components (M3.7),<br/>the overlays (M3.3)"| k_ui
    k_tui -.->|"planned M7.1"| k_core
    k_tui -.->|"planned M7.1:<br/>terminal guard"| k_platform
    k_tui -.->|"planned M7.1:<br/>draw lists, theme"| k_ui
    k_editor -->|"Tree, controls,<br/>palette (M3.4);<br/>the view's widgets,<br/>scrollbars (M4.1)"| k_ui
    k_editor -->|"input, geometry (M3.4)"| k_core
    k_editor -->|"Rope, Selection,<br/>clusters (M4.1)"| k_text
    k_editor -->|"toml: keymaps (M4.6)"| k_codec

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    class k_app,k_editor,k_gui,k_platform,k_codec,k_text,k_gfx,k_font,k_core,k_ui,k_workspace built
    class k_tui planned
```

- The matrix also lets `gui` use `codec`; no task or spec gives it a use
  yet, so no edge is drawn.
- `editor` never depends on `gui` or `tui`, and the frontends never depend
  on `editor` (`docs/01-architecture.md` → "Dependency matrix").

## 3. Components of xtask

`xtask` links only `tidepool-core` and `tidepool-codec`, its row in the
matrix, and starts no program but `cargo`. Its modules are drawn in three
views: the commands and the checks, what the checks build on, and what each
check reads, runs and writes.

### 3.1 Modules: the commands and the checks

`main.rs` runs `ci`, one check, `golden`, `fuzz-smoke`, `site` or `bench`,
and exits 0, 1 or 2. `check` holds the `ci` steps in order, `theme` the
last (M3.2), and in its `LATER` table the commands still to come; `site`
runs the same steps through `check`'s one runner (ADR-0030). `golden` is a step of `ci`, and
`main.rs` also runs it on its own, for its `--bless` (ADR-0044). `bench`
is handled before the command line is parsed, so every argument after it
goes to `tidepool-bench` (ADR-0035).

```mermaid
flowchart LR
    m_main["main.rs<br/>parses the command line<br/>with core::cli::Args<br/>runs ci, one check,<br/>golden, fuzz-smoke,<br/>site or bench,<br/>exits 0, 1 or 2"]
    m_check["check<br/>the ci steps: deps, fmt,<br/>clippy, test, golden, layers,<br/>crates, features, unsafe-audit,<br/>headers, workflows, ucd --check,<br/>theme<br/>LATER: tools, package"]
    subgraph b_checks["The checks"]
        m_deps["deps<br/>no third-party code<br/>unless approved"]
        m_layers["layers<br/>only the matrix's edges"]
        m_crates["crates<br/>the members' manifests,<br/>READMEs, and copies<br/>of LICENSE and NOTICE"]
        m_features["features<br/>every ticked task<br/>in a feature"]
        m_unsafe["unsafe_audit<br/>unsafe only in sys and<br/>platform, documented"]
        m_headers["headers<br/>the Apache-2.0 header<br/>on every source file"]
        m_workflows["workflows<br/>allowed actions, SHA pins,<br/>token permissions"]
        m_ucd["ucd<br/>the grapheme tables<br/>from the Unicode data"]
        m_golden["golden<br/>the golden scenes against<br/>tests/golden/*.qoi;<br/>--bless writes references"]
        m_theme["theme<br/>each themes/*.toml meets<br/>WCAG AA for the pairs<br/>it uses for text"]
    end
    m_fuzz["fuzz<br/>fuzz-smoke: the TARGETS<br/>table, a cargo test per<br/>target, the verdict"]
    m_site["site, with site::page<br/>the demo page: the gate,<br/>two examples, the features,<br/>into target/site"]
    m_bench["bench<br/>passes every argument<br/>after bench to<br/>tidepool-bench"]
    m_cargo["cargo<br/>the module that<br/>starts cargo"]
    k_core["tidepool-core"]

    m_main --> m_check
    m_main -->|"golden, for<br/>its --bless"| m_golden
    m_main -->|"fuzz-smoke"| m_fuzz
    m_main -->|"site"| m_site
    m_main -->|"bench, before<br/>parsing"| m_bench
    m_bench -->|"run: cargo run<br/>--release"| m_cargo
    m_site -->|"run_gate: the<br/>ci steps"| m_check
    m_main -->|"cli::Args"| k_core
    m_check --> m_deps
    m_check --> m_layers
    m_check --> m_crates
    m_check --> m_features
    m_check --> m_unsafe
    m_check --> m_headers
    m_check --> m_workflows
    m_check --> m_ucd
    m_check --> m_golden
    m_check --> m_theme
    m_check -->|"fmt, clippy, test"| m_cargo
    m_golden -->|"run_with_env:<br/>cargo test of<br/>the scenes"| m_cargo

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    class m_main,m_check,m_deps,m_layers,m_crates,m_features,m_unsafe,m_headers,m_workflows,m_ucd,m_golden,m_theme,m_fuzz,m_site,m_bench,m_cargo,k_core built
```

- `tools` and `package` are names in `check`'s `LATER` table and fail as
  not yet implemented (M5.1, M11.6, M12.4, M14.4); `ci`'s summary lists
  `tools`.

### 3.2 Modules: what the checks build on

An arrow goes to each module whose items a module uses. Arrows to `error`
and `walk`, which nearly every module uses, are left out, as are those to
`check` for its `Report` and `printable`. So `workflows`, which uses only
those, is not drawn here.

```mermaid
flowchart LR
    m_deps["deps"]
    m_layers["layers"]
    m_crates["crates"]
    m_unsafe["unsafe_audit"]
    m_headers["headers"]
    m_fuzz["fuzz"]
    m_golden["golden"]
    m_theme["theme<br/>WCAG 2.x contrast"]
    m_check["check<br/>print: a result<br/>as ci prints a step"]
    m_walk["walk<br/>finds files without<br/>following a link, reads<br/>within a size bound,<br/>escapes paths"]
    m_ucd["ucd"]
    m_metadata["metadata<br/>cargo metadata,<br/>read with codec::json"]
    m_matrix["matrix<br/>the dependency matrix,<br/>parsed from<br/>docs/01-architecture.md"]
    m_lexer["lexer<br/>just enough of a Rust<br/>lexer for the audit"]
    m_cargo["cargo<br/>argv only, stdin closed,<br/>CARGO_NET_OFFLINE=true<br/>reads SKIPPED: and<br/>test result: lines"]
    m_error["error<br/>the Error type"]
    k_core["tidepool-core"]
    k_codec["tidepool-codec"]
    e_cargo["cargo<br/>the one program<br/>xtask starts"]

    m_deps --> m_metadata
    m_deps -->|"without_userinfo"| m_cargo
    m_layers --> m_metadata
    m_layers -->|"parse"| m_matrix
    m_crates --> m_metadata
    m_crates -->|"classify"| m_matrix
    m_crates -->|"toml"| k_codec
    m_theme -->|"read_file"| m_crates
    m_theme -->|"toml"| k_codec
    m_theme -->|"color"| k_core
    m_unsafe --> m_metadata
    m_unsafe -->|"lex"| m_lexer
    m_headers -->|"ucd::OUTPUT,<br/>exempt"| m_ucd
    m_ucd -->|"hash::sha256"| k_core
    m_fuzz -->|"run_keeping_lines"| m_cargo
    m_fuzz -->|"testing::fuzz,<br/>hash::fnv1a64,<br/>cli::Args"| k_core
    m_golden --> m_metadata
    m_golden -->|"run_with_env"| m_cargo
    m_golden -->|"qoi"| k_codec
    m_golden -->|"cli::Args"| k_core
    m_golden -->|"print"| m_check
    m_metadata -->|"capture"| m_cargo
    m_metadata -->|"json"| k_codec
    m_walk -->|"capture: cargo<br/>locate-project"| m_cargo
    m_cargo -->|"golden::DIR_VAR,<br/>removed from<br/>every child"| m_golden
    m_cargo -->|"spawns"| e_cargo

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef external fill:#8a8a8a,stroke:#6b6b6b,color:#ffffff
    class m_deps,m_layers,m_crates,m_unsafe,m_headers,m_fuzz,m_golden,m_theme,m_check,m_walk,m_ucd,m_metadata,m_matrix,m_lexer,m_cargo,m_error,k_core,k_codec built
    class e_cargo external
```

`features` and `site` (ADR-0030) build on the modules above. `site` runs
the steps of `ci` through `check::run_gate`, the runner `ci` itself uses,
and starts the two examples through `cargo::capture`.

```mermaid
flowchart LR
    m_site["site, with site::page<br/>the demo page"]
    m_check["check<br/>run_gate, CI, LATER"]
    m_features["features"]
    m_crates["crates<br/>task_line, read_file"]
    m_metadata["metadata"]
    m_cargo["cargo<br/>capture"]
    k_core["tidepool-core"]
    k_codec["tidepool-codec"]

    m_site -->|"the ci steps"| m_check
    m_site -->|"parse, tasks,<br/>status"| m_features
    m_site -->|"read_file"| m_crates
    m_site -->|"the members"| m_metadata
    m_site -->|"the examples"| m_cargo
    m_site -->|"toml, json"| k_codec
    m_site -->|"civil, cli::Args"| k_core
    m_features -->|"task_line,<br/>read_file"| m_crates
    m_features -->|"json: JSONC"| k_codec

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    class m_site,m_check,m_features,m_crates,m_metadata,m_cargo,k_core,k_codec built
```

### 3.3 What each check reads, runs and writes

The checks find and read their files through `walk`, which never follows a
symbolic link and asks `cargo locate-project` which directories are
workspace roots; `ucd`, `fuzz` and `golden` also look at their own
directories without following a link, and `theme` its `themes/`. Only
`ucd`, `fuzz` and `golden`, with `--bless`, change anything in the
repository. `golden` also empties
and rewrites `target/golden` when a crate draws, with or without `--bless`; `site` writes its
output directory, and `target/golden` through the `golden` step of its
gate.

```mermaid
flowchart LR
    subgraph b_checks["xtask"]
        m_deps["deps"]
        m_layers["layers"]
        m_crates["crates"]
        m_features["features"]
        m_unsafe["unsafe_audit"]
        m_headers["headers"]
        m_workflows["workflows"]
        m_ucd["ucd"]
        m_fuzz["fuzz"]
        m_golden["golden"]
        m_theme["theme"]
        m_site["site"]
        m_bench["bench"]
        m_steps["check: the fmt, clippy and test steps of ci"]
    end
    e_cargo["cargo"]
    s_manifests[("Cargo.toml and Cargo.lock files, .cargo/config.toml in any directory")]
    s_allow[("xtask/deps-allowlist.txt")]
    s_every[("Every file in the repository<br/>for compiled artifacts and links")]
    s_arch[("docs/01-architecture.md<br/>the dependency matrix")]
    s_roadmap[("docs/roadmap.md<br/>the task lines")]
    s_readmes[("Each member's README.md")]
    s_license[("LICENSE and NOTICE<br/>the root's, and the copies<br/>beside each member's<br/>Cargo.toml")]
    s_rs[("Every .rs file")]
    s_source[("Source files: .rs, .sh, .github/workflows YAML,<br/>.html, .css, .js")]
    s_features[("docs/features.jsonc")]
    s_site[("site/: index.html, llms.txt,<br/>tidepool.css, tidepool.js;<br/>rust-toolchain.toml")]
    s_git[(".git: HEAD, its loose ref<br/>or packed-refs")]
    s_out[("target/site, or --out DIR,<br/>marked .tidepool-site")]
    s_wf[(".github/workflows YAML files")]
    s_ucd[("crates/text/ucd/17.0.0")]
    s_tables[("crates/text/src/pos/grapheme/tables.rs")]
    s_fuzz[("fuzz/, fuzz/corpus/, fuzz/crashes/")]
    s_golden[("tests/golden/SCENE.qoi<br/>the references")]
    s_goldout[("target/golden: rendered/PACKAGE/SCENE.pam,<br/>SCENE.actual.qoi, SCENE.diff.qoi")]
    s_themes[("themes/*.toml")]

    m_deps -->|"reads lockfiles as text, cargo configs"| s_manifests
    m_deps -->|"reads"| s_allow
    m_deps -->|"walks"| s_every
    m_deps -->|"cargo metadata --offline --locked, each workspace"| e_cargo
    m_layers -->|"parses the matrix"| s_arch
    m_layers -->|"cargo metadata --offline --no-deps"| e_cargo
    m_crates -->|"reads each member's and the root's Cargo.toml"| s_manifests
    m_crates -->|"the first line, the sections<br/>in order, the task IDs"| s_readmes
    m_crates -->|"compares each member's<br/>copies with the root's,<br/>byte for byte"| s_license
    m_crates -->|"reads the task IDs"| s_roadmap
    m_crates -->|"cargo metadata --offline --no-deps"| e_cargo
    m_features -->|"reads"| s_features
    m_features -->|"reads the task lines"| s_roadmap
    m_site -->|"reads the templates,<br/>copies the two files"| s_site
    m_site -->|"reads HEAD's hash<br/>in a local build"| s_git
    m_site -->|"writes the page,<br/>removes only what<br/>it wrote before"| s_out
    m_site -->|"the ci steps, then<br/>cargo run --example, twice"| e_cargo
    m_site -->|"the gate's golden step:<br/>empties and rewrites it"| s_goldout
    m_bench -->|"cargo run --release --quiet -p tidepool-bench"| e_cargo
    m_unsafe -->|"lexes"| s_rs
    m_unsafe -->|"cargo metadata --offline --no-deps, the crate roots"| e_cargo
    m_headers -->|"checks the header at the top of each"| s_source
    m_workflows -->|"reads line by line"| s_wf
    m_ucd -->|"checks five SHA-256 pins, parses three files"| s_ucd
    m_ucd -->|"writes it, or with --check compares"| s_tables
    m_fuzz -->|"checks the table against them, keeps died-HASH"| s_fuzz
    m_fuzz -->|"cargo test --profile fuzz, a target at a time"| e_cargo
    m_golden -->|"cargo metadata --offline --no-deps: the members with a golden test"| e_cargo
    m_golden -->|"cargo test --package PACKAGE --test golden, TIDEPOOL_GOLDEN_DIR set"| e_cargo
    m_golden -->|"decodes each; --bless writes the scenes named"| s_golden
    m_golden -->|"empties it, reads the PAM files, writes a failing scene's images"| s_goldout
    m_theme -->|"reads each with codec toml, its colors with core color; prints the contrast table"| s_themes
    m_steps -->|"fmt --check, clippy -D warnings, test --workspace"| e_cargo

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef external fill:#8a8a8a,stroke:#6b6b6b,color:#ffffff
    classDef store fill:#1168bd,stroke:#0b4884,color:#ffffff
    class m_deps,m_layers,m_crates,m_features,m_unsafe,m_headers,m_workflows,m_ucd,m_fuzz,m_golden,m_theme,m_site,m_bench,m_steps built
    class s_manifests,s_allow,s_every,s_arch,s_roadmap,s_readmes,s_license,s_rs,s_source,s_wf,s_ucd,s_tables,s_fuzz,s_golden,s_goldout,s_features,s_site,s_git,s_out,s_themes store
    class e_cargo external
```

- Every cargo child runs in the root with `CARGO_NET_OFFLINE=true` and its
  standard input closed; `ci` compiles nothing once `deps` has failed.
- `fuzz` renames the `.last-input` a replay wrote into `died-HASH`, or
  removes it when it is already a saved crash, through `core`'s
  `keep_died`.

## 4. The dev-only parts

What exists only to test and measure Tidepool: the fuzzer, the
property-test harness, the benchmark harness and the golden frames are
built; the fakes are planned.

### 4.1 The fuzz engine and its targets

Each parser crate has its targets in `tests/fuzz.rs`, run by every `cargo
test` on the saved crashes and seeds alone, and on mutated inputs when
`fuzz-smoke` sets `TIDEPOOL_FUZZ_SECS` (ADR-0027).

```mermaid
flowchart TB
    c_smoke["xtask fuzz-smoke, xtask/src/fuzz.rs<br/>TARGETS: json, toml and qoi in tidepool-codec, encoding in tidepool-text, ttf in tidepool-font, theme in tidepool-ui, keymap in tidepool-editor"]
    e_cargo["cargo test --profile fuzz -p PACKAGE --test fuzz -- --exact TARGET --nocapture<br/>with TIDEPOOL_FUZZ_SECS, TIDEPOOL_FUZZ_SEED, and TIDEPOOL_FUZZ_REPLAY=1 for --replay"]
    subgraph b_codec["tidepool-codec: tests/fuzz.rs"]
        t_json["test json<br/>json::parse_bytes and json::parse, both dialects, small Limits"]
        t_toml["test toml<br/>toml::parse_bytes and toml::parse, spans checked"]
        t_qoi["test qoi<br/>qoi::decode with small Limits, each image encoded and decoded again"]
    end
    subgraph b_text["tidepool-text: tests/fuzz.rs"]
        t_enc["test encoding<br/>decode, Decoder fed in pieces, encode"]
    end
    subgraph b_font["tidepool-font: tests/fuzz.rs"]
        t_ttf["test ttf<br/>ttf::Font::parse with small Limits, outlines, rasterize twice, kerning"]
    end
    subgraph b_ui["tidepool-ui: tests/fuzz.rs"]
        t_theme["test theme<br/>theme::Theme::parse with small Limits, what reads keeps the format's rules (M3.2)"]
    end
    subgraph b_editor["tidepool-editor: tests/fuzz.rs"]
        t_keymap["test keymap<br/>keymap::Keymap::read as a file and as a user layer, Sequence::parse, and an Engine over each keymap, with small Limits (M4.6)"]
    end
    subgraph b_fuzz["tidepool-core: testing::fuzz"]
        f_api["fuzz.rs<br/>Target: new, seeds_in, dictionary, memory_cap, run<br/>Outcome, Summary, Finding, FuzzError, keep_died"]
        f_engine["engine<br/>the target on its own thread with a 2 MiB stack<br/>the caller is the watchdog, HANG_LIMIT 5 s"]
        f_mutate["mutate<br/>1 to 16 changes to a seed: bit flips, inserts, deletes, splices, tokens, length steps, boundary integers"]
        f_memory["memory<br/>VmHWM against memory_cap, VmPeak against reserved_cap"]
        f_paths["paths<br/>each directory looked at without following a link, made one level at a time"]
    end
    s_corpus[("fuzz/corpus/json, toml, qoi, encoding, ttf, theme, keymap<br/>the seeds")]
    s_inplace[("crates/codec/tests/json, crates/codec/tests/toml-test, themes/ and keymaps/<br/>more seeds, read in place")]
    s_crashes[("fuzz/crashes/TARGET<br/>KIND-HASH findings, .last-input, died-HASH")]
    e_proc["Linux: /proc/self/status"]

    c_smoke -->|"one cargo test a target"| e_cargo
    e_cargo -->|"runs"| t_json
    e_cargo -->|"runs"| t_toml
    e_cargo -->|"runs"| t_qoi
    e_cargo -->|"runs"| t_enc
    e_cargo -->|"runs"| t_ttf
    e_cargo -->|"runs"| t_theme
    e_cargo -->|"runs"| t_keymap
    t_json -->|"Target"| f_api
    t_toml -->|"Target"| f_api
    t_qoi -->|"Target"| f_api
    t_enc -->|"Target"| f_api
    t_ttf -->|"Target"| f_api
    t_theme -->|"Target"| f_api
    t_keymap -->|"Target"| f_api
    f_api -->|"run_with: the saved crashes first, then the seeds"| f_paths
    f_api --> f_engine
    f_engine --> f_mutate
    f_engine --> f_memory
    f_engine -->|"saves findings, a replay's .last-input"| f_paths
    f_paths -->|"reads"| s_corpus
    f_paths -->|"reads"| s_inplace
    f_paths -->|"reads, writes whole through a hidden file"| s_crashes
    f_memory -->|"reads"| e_proc
    c_smoke -->|"Summary::parse of each fuzz-result line, keep_died"| f_api
    c_smoke -->|"checks the TARGETS table against fuzz/corpus and fuzz/crashes"| s_corpus

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef external fill:#8a8a8a,stroke:#6b6b6b,color:#ffffff
    classDef store fill:#1168bd,stroke:#0b4884,color:#ffffff
    class c_smoke,t_json,t_toml,t_qoi,t_enc,t_ttf,t_theme,t_keymap,f_api,f_engine,f_mutate,f_memory,f_paths built
    class s_corpus,s_inplace,s_crashes store
    class e_cargo,e_proc external
```

- Each later parser adds a target with its task: `regex` (M4.8), `vt`
  (M5.8), framing in `rpc` (M6.1), the Markdown parser (M6.5), git's
  parsers (M8.1), `inflate` and `png` (M10.7), and `zip` and `xml`
  (M10.2).

### 4.2 Test support: property tests, benchmarks and fakes

The property-test harness in `core::testing` (`check`, `check_sized` and
`assert_approx`, seeded by `TIDEPOOL_SEED`, and `rng::Pcg32`) is built and
used by the tests of `codec` and `text`, through a dev-dependency, and by
those of `ui` (M3.1), whose library depends on `core`. The benchmark harness,
`tidepool-bench` (M1.9), warms each suite up, takes samples within a time
budget, and reports the median and p95 against `bench/baseline.json`; its
suites measure the text engine: rope edits, positions, 10,000-caret
keystrokes, typing and graphemes (ADR-0035), the glyph atlas's fill
(M2.5), the command palette's repaint, its text measured with
`gui`'s `FontMetrics` as the window measures it (M3.3), and the editor
view: a wheel notch through a 10,000-line file at 1080p, with soft wrap
off and on, and the first layout of a 10,000-byte line, each handled as
the main loop does and its damage painted with `gui`'s `Painter` into a
`gfx` framebuffer (M4.1).
`tidepool-testkit` is an
empty crate; its edges are the matrix's row for it. `testkit` keeps fake
tools in
`crates/testkit/fake-tools/` (M5.1), and the fake LSP server's transcripts
in `crates/testkit/transcripts/` (M6.1).

```mermaid
flowchart LR
    t_codec["tidepool-codec tests<br/>tests/json.rs, toml.rs<br/>and qoi.rs"]
    t_text["tidepool-text tests<br/>tests/edits.rs, model.rs,<br/>positions.rs, unit tests"]
    t_ui["tidepool-ui tests<br/>tests/layout.rs, a11y.rs,<br/>theme.rs, widgets.rs,<br/>unit tests"]
    c_xtask["xtask"]
    c_bench["tidepool-bench, bench/<br/>harness, baseline, cli,<br/>suites::text, suites::font,<br/>suites::ui, suites::editor,<br/>the binary"]
    s_baseline[("bench/baseline.json")]
    c_testkit["tidepool-testkit<br/>dev-only<br/>planned M5.1: fake tools<br/>planned M6.1: fake LSP<br/>server, transcripts"]
    k_core["tidepool-core<br/>testing: check,<br/>check_sized,<br/>assert_approx<br/>rng: Pcg32"]
    k_codec["tidepool-codec"]
    k_text["tidepool-text"]
    k_font["tidepool-font"]
    k_gfx["tidepool-gfx"]
    k_ui["tidepool-ui"]
    k_gui["tidepool-gui"]
    k_editor["tidepool-editor"]
    k_rpc["tidepool-rpc<br/>planned M6.1"]

    t_codec -->|"dev-dependency:<br/>property tests"| k_core
    t_text -->|"dev-dependency:<br/>property tests"| k_core
    t_ui -->|"property tests"| k_core
    c_xtask -->|"cargo xtask bench:<br/>through cargo run<br/>--release"| c_bench
    c_bench -->|"read and written<br/>with codec::json"| s_baseline
    c_bench -->|"measures; file::save<br/>writes the baseline"| k_text
    c_bench -->|"measures the<br/>atlas's fill"| k_font
    c_bench -->|"json"| k_codec
    c_bench -->|"cli::Args,<br/>rng::Pcg32"| k_core
    c_bench -->|"repaints a Palette<br/>of matched rows (M3.3)"| k_ui
    c_bench -->|"measures its text<br/>with FontMetrics (M3.3);<br/>paints the view<br/>with Painter (M4.1)"| k_gui
    c_bench -->|"scrolls an EditorView<br/>(M4.1)"| k_editor
    c_bench -->|"the window's<br/>framebuffers (M4.1)"| k_gfx
    c_testkit -.->|"planned M5.1"| k_core
    c_testkit -.->|"planned M6.1"| k_codec
    c_testkit -.->|"planned M6.1"| k_rpc

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    classDef store fill:#1168bd,stroke:#0b4884,color:#ffffff
    class t_codec,t_text,t_ui,c_xtask,c_bench,k_core,k_codec,k_text,k_font,k_gfx,k_ui,k_gui,k_editor built
    class s_baseline store
    class c_testkit,k_rpc planned
```

- The matrix lets `tidepool-bench` use any workspace crate, and lets only
  it use `tidepool-testkit`, and only from `[dev-dependencies]`. The edges
  from the tests of `toolchain`, `vcs`, `mobile`, `rpc` and `lang` to
  `testkit` are decided in M5.1, so none is drawn.
- The mock agent `tp-agent-mock` (M9.2), in `agents/`, plays the same part
  for TAP: the conformance suite runs against it.

### 4.3 The golden frames

`cargo xtask golden`, and the `golden` step of `ci`, draw each drawing
crate's scenes and compare what they drew with the references in
`tests/golden/`, exactly for `gfx` and `gui` and within 1 for `font`
(ADR-0044, ADR-0048). The matrix lets `xtask` use only `core` and
`codec`, and `gfx` and `font` only `core`, so the scenes are tests that
write what they drew as PAM files (`font`'s blending their glyphs'
coverage themselves, and `gui`'s painting `ui` trees with its painter,
`ui`'s controls among them, M3.6, its other components, M3.7, and its
overlays, M3.3),
and `xtask` reads those,
decodes the QOI references and writes the QOI images. `golden` asks
`cargo metadata` which members have a `golden` test; each needs a row in
`SCENES`, which gives its tolerance.

```mermaid
flowchart TB
    c_golden["xtask golden, xtask/src/golden.rs<br/>SCENES: tidepool-gfx, tolerance 0; tidepool-font, tolerance 1; tidepool-gui, tolerance 0<br/>cargo xtask golden, with --bless SCENE..., and the golden step of ci"]
    e_cargo["cargo test --package PACKAGE --test golden<br/>with TIDEPOOL_GOLDEN_DIR=target/golden/rendered/PACKAGE"]
    subgraph b_gfx["tidepool-gfx: tests/golden.rs"]
        t_scenes["14 scenes, one test each, gfx_fill_rect to gfx_backdrop_scale_2<br/>each primitive, clipping, a layer, images, glyph masks, the backdrop at scale 1 and 2"]
    end
    subgraph b_font["tidepool-font: tests/golden.rs"]
        t_font["8 scenes, one test each, font_saira to font_layout_scale_2<br/>both faces at 11 to 24 px, the uppercase labels, fallback, truncation, wrapping and tabs, at scale 1 and 2"]
    end
    k_font["tidepool-font library<br/>bundled, layout, Atlas"]
    subgraph b_gui["tidepool-gui: tests/golden.rs, with tests/controls/mod.rs, tests/components/mod.rs and tests/overlays/mod.rs"]
        t_ui["23 scenes, one test each, ui_rect to ui_scale_1_5<br/>ui trees painted by the painter: each draw command, clips and layers, at scale 1 and 2, and a mix at 1.5"]
        t_controls["16 scenes in controls/mod.rs, one test each, ui_buttons to ui_breadcrumbs_scale_2 (M3.6)<br/>ui's buttons, chips, text input, search field, scrollbar, status dot, titles and breadcrumbs, each a grid of a row a variant and a column a state: rest, hover, focus, pressed, disabled, at scale 1 and 2"]
        t_components["14 scenes in components/mod.rs, one test each, ui_list to ui_code_block_scale_2 (M3.7)<br/>ui's list, tree view, table, tab strip, splitter, card and code block, each a grid of a row a variant and a column a state: rest, hover, focus, pressed, disabled and each one's own, such as a selected row or tab, at scale 1 and 2"]
        t_overlays["8 scenes in overlays/mod.rs, one test each, ui_modal to ui_palette_scale_2 (M3.3)<br/>ui's modal dialog over a window it makes inert, tooltip, toast, and command palette, each a grid of a row a variant and a column a state, such as hover, keyboard focus, pressed and disabled, at scale 1 and 2"]
    end
    k_gui["tidepool-gui library<br/>Painter, FontMetrics, Icon"]
    k_gfx["tidepool-gfx library<br/>Framebuffer, raster, image, glyph, backdrop"]
    k_codec["tidepool-codec<br/>qoi::decode, qoi::encode"]
    s_rendered[("target/golden/rendered/PACKAGE/SCENE.pam<br/>premultiplied RGBA, rows from the top")]
    s_images[("target/golden/SCENE.actual.qoi and SCENE.diff.qoi<br/>a failing scene's images")]
    s_refs[("tests/golden/SCENE.qoi<br/>the references")]

    c_golden -->|"one cargo test a drawing crate"| e_cargo
    e_cargo -->|"runs"| t_scenes
    e_cargo -->|"runs"| t_font
    e_cargo -->|"runs"| t_ui
    e_cargo -->|"runs"| t_controls
    e_cargo -->|"runs"| t_components
    e_cargo -->|"runs"| t_overlays
    t_ui -->|"lays out with tidepool-ui, paints with"| k_gui
    t_ui -->|"each writes what it painted"| s_rendered
    t_controls -->|"puts ui::widgets in each state by events, paints each cell with"| k_gui
    t_controls -->|"each writes its grid"| s_rendered
    t_components -->|"puts each component in each state by events, paints each cell with"| k_gui
    t_components -->|"each writes its grid"| s_rendered
    t_overlays -->|"opens each overlay, puts it in each state by events, paints each cell with"| k_gui
    t_overlays -->|"each writes its grid"| s_rendered
    t_scenes -->|"draws with"| k_gfx
    t_scenes -->|"each writes what it drew"| s_rendered
    t_font -->|"lays out and rasterizes with"| k_font
    t_font -->|"each writes what it blended"| s_rendered
    c_golden -->|"empties target/golden, then reads them"| s_rendered
    c_golden -->|"decodes each; --bless writes the scenes named"| s_refs
    c_golden -->|"writes, for a scene that fails"| s_images
    c_golden -->|"decode, encode"| k_codec

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef external fill:#8a8a8a,stroke:#6b6b6b,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    classDef store fill:#1168bd,stroke:#0b4884,color:#ffffff
    class c_golden,t_scenes,t_font,t_ui,t_controls,t_components,t_overlays,k_gfx,k_font,k_gui,k_codec built
    class s_rendered,s_images,s_refs store
    class e_cargo external
```

- A scene passes when its reference has its size and no channel of any
  pixel differs from it by more than its crate's tolerance. A scene with
  no reference fails, as does a reference that no scene drew; `--bless`
  writes nothing when a scene command failed, a file could not be read,
  or a name is no scene's, but another scene's failure does not stop it.
- `ci` draws each scene twice: `test` runs the same tests with
  `TIDEPOOL_GOLDEN_DIR` unset, so they draw and write nothing, and then
  `golden` draws them again and compares.

## The crates

Every workspace member, its layer, its `unsafe` policy, what it has and
what is planned, and its declared workspace dependencies. "deny" is the
workspace's `unsafe_code = "deny"`, which every member inherits through
`[lints] workspace = true`; "allowed" is the `#![allow(unsafe_code)]` in
the crate's `src/lib.rs`. The edges each crate may have are in
`docs/01-architecture.md` → "Dependency matrix". Every member also has a
`README.md` in ADR-0028's shape and copies of the root's `LICENSE` and
`NOTICE`, and every library a `ReadmeDoctests` item that runs its README's
examples as doc tests (M0.13) and, but `sys` and `platform`, the attribute
that forbids `unsafe` in them (M0.15); the table leaves those out.

| Package | Path | Layer | `unsafe` | Built (task) | Planned (tasks) | Declared workspace dependencies |
| :- | :- | :- | :- | :- | :- | :- |
| `tidepool-core` | `crates/core` | 0, foundations | deny | `error`, `log`, `testing`, `rng`, `hash`, `cli`, `civil`, `input`, `geom`, `color`, `cancel` (M0.2); `testing::fuzz` (M1.6); `path` (M1.8) | — | none |
| `tidepool-sys` | `crates/sys` | 0, foundations | allowed | the crate root (M0.1); `dynlib`, `linux::libc`, `linux::x11` with the `Xlib` table (M2.1); `write(2)`, `setlocale`, `linux::evdev`, typed text and variadic tables (M2.2); the selections and properties the clipboard calls (M2.7) | M4.5, M5.7, M5.9, M7.1, M11.7, M11.8, M12.1, M12.2 | none |
| `tidepool-platform` | `crates/platform` | 1, OS behavior, formats, engines | allowed | the crate root (M0.1); `window`, `events` with the `Waker`, `surface` and the error, on Linux through X11 (M2.2); `clipboard`, `paths` and `dirs` on Linux (M2.7) | M4.5, M5.7, M5.9, M7.1, M11.7, M11.8, M11.1, M12.1, M12.2 | `core`, `sys` |
| `tidepool-codec` | `crates/codec` | 1, OS behavior, formats, engines | deny | `json` (M1.4), `toml` (M1.7) and its `Document` (M1.5), fuzz targets `json` and `toml` (M1.6); `qoi` and its fuzz target (M2.4) | M10.7, M10.2, M11.3 | `core`, dev only |
| `tidepool-text` | `crates/text` | 1, OS behavior, formats, engines | deny | `rope`, `pos` (M1.1); `selection`, `transaction`, `history` (M1.2); `encoding`, `file` (M1.3); fuzz target `encoding` (M1.6) | none: every planned module exists | `core`, dev only |
| `tidepool-regex` | `crates/regex` | 1, OS behavior, formats, engines | deny | nothing | M4.8 | none |
| `tidepool-gfx` | `crates/gfx` | 1, OS behavior, formats, engines | deny | `framebuffer`, `clip`, `blend`, `raster`, `image`, `glyph`, `damage`, `backdrop` and the error (M2.3); the golden scenes in `tests/golden.rs` (M2.4) | none: every planned module exists | `core` |
| `tidepool-font` | `crates/font` | 1, OS behavior, formats, engines | deny | `ttf`, `outline`, `Limits` and the error, and fuzz target `ttf` (M2.8); `bundled`, `fallback`, `layout`, `atlas`, and the golden scenes in `tests/golden.rs` (M2.5) | none: every planned module exists | `core`, dev only |
| `tidepool-markdown` | `crates/markdown` | 1, OS behavior, formats, engines | deny | nothing | M6.5 | none |
| `tidepool-rpc` | `crates/rpc` | 2, services | deny | nothing | M6.1, M9.3, M13.1 | none |
| `tidepool-toolchain` | `crates/toolchain` | 2, services | deny | nothing | M5.1, M5.2, M5.3, M5.5, M10.1, M11.2, M12.3, M15.8 | none |
| `tidepool-syntax` | `crates/syntax` | 2, services | deny | nothing | M6.2, M13.2, M13.3, M15.8 | none |
| `tidepool-term` | `crates/term` | 2, services | deny | nothing | M5.8, M5.4 | none |
| `tidepool-workspace` | `crates/workspace` | 2, services | deny | `folder` with its ignore files, `trust`, `settings` and the error, and fuzz targets `ignore` and `settings` (M4.7) | M4.3, M4.4, M4.5 | `core`, `codec`, `text` |
| `tidepool-ui` | `crates/ui` | 2, services | deny | `tree`, `layout`, `focus`, `input`, `metrics`, `draw`, `a11y` and the error (M3.1); `theme`, with the bundled `themes/tidepool-dark.toml`, the error's theme variants and fuzz target `theme` (M3.2); `widgets`, the controls, the theme the tree holds and `UiError::WrongWidget` (M3.6); `widgets`' collections (list, tree view, table), tab strip, splitter, card and code block, which each layout binds (M3.7); `overlay`, `widgets`' modal dialog, tooltip, toast, and command palette, `Row::matched`, `Response::blocked`, the accessibility tree's `modal`, `description`, and `active`, `TextMetrics::scale`, `input::PRESS_SLOP`, and the error's overlay variants (M3.3); the `current-line` token, and `widgets::is_hidden` and `widgets::flag_at` made public for the editor view (M4.1) | none: `anim` has no task yet | `core`, `codec`, `text` |
| `tidepool-lang` | `crates/lang` | 3, language and tool features | deny | nothing | M6.7, M6.8, M6.3, M6.4, M6.6, M13.1 | none |
| `tidepool-vcs` | `crates/vcs` | 3, language and tool features | deny | nothing | M8.1, M8.3 | none |
| `tidepool-agent` | `crates/agent` | 3, language and tool features | deny | nothing | M9.3, M9.4 | none |
| `tidepool-mobile` | `crates/mobile` | 3, language and tool features | deny | nothing | M10.2, M10.3, M10.4, M10.5, M10.6, M11.3, M11.4, M11.5 | none |
| `tidepool-editor` | `crates/editor` | 4, views and frontends | deny | the shell, `shell`, with its commands and matcher, `shell::commands`, and its widgets, `shell::parts` (M3.4); the editor view, `view`, with its error, `EditorError` (M4.1); the command registry, `command`, and the keymaps and their engine, `keymap`, with the default keymap `keymaps/default.toml` and fuzz target `keymap` (M4.6); editing, `edit`, with the editing commands' bindings in the default keymap (M4.2) | M4.3, M4.4, M5.4, M5.6, M6.2, M6.7, M6.8, M6.3, M6.6, M7.2, M7.3, M7.4, M8.2, M8.3, M9.4, M9.5, M10.6, M11.6, M13.4, M15.5, M15.6, M15.7, M15.8 | `core`, `codec`, `text`, `ui` |
| `tidepool-gui` | `crates/gui` | 4, views and frontends | deny | `Gui`, `first_frame`, the error, and the window test (M2.6), with `Gui::render` painting `ui` frames, the backdrop built on threads of its own and `first_frame::tree` (M3.8); the first frame's text colors from `ui`'s bundled theme (M3.2); `paint::Painter`, `metrics::FontMetrics`, `icon::Icon` and `ui`'s golden scenes (M3.5); the golden scenes of `ui`'s controls in `tests/controls/mod.rs` (M3.6); the golden scenes of `ui`'s M3.7 components in `tests/components/mod.rs` (M3.7); the golden scenes of `ui`'s overlays in `tests/overlays/mod.rs` (M3.3); the activity bar's six icons, `icons`, and `Gui::set_icon` (M3.4) | — | `core`, `platform`, `gfx`, `font`, `ui` |
| `tidepool-tui` | `crates/tui` | 4, views and frontends | deny | nothing | M7.1, M7.5 | none |
| `tidepool` (app) | `crates/app` | 5, application | deny | a `main` (M0.1) that opens the window, shows the first frame and takes `--print-startup-time` (M2.6); the event-driven main loop, with its window test (M3.8); the shell's wiring, the settings and the shell's golden scenes (M3.4); the editor view's golden scenes (M4.1); the user's keymap, `settings.toml`'s `[keymap]` (M4.6); the settings read through `workspace::settings` (M4.7) | M4.9, M7.1, M7.5, M14.4 | `core`, `platform`, `codec`, `workspace`, `gui`, `ui`, `editor` |
| `xtask` | `xtask` | 6, tools | deny | `deps`, `layers`, `unsafe-audit`, `ci` (M0.3), with `unsafe-audit`'s rules for doc tests, proc macros and pass-through macros (M0.15); `headers` (M0.7); `workflows` (M0.8); `crates` (M0.9), with ADR-0028's sections and order and the `LICENSE` and `NOTICE` copies (M0.13); escaped paths, links never resolved (M0.10); `ucd` (M1.1); `fuzz-smoke` (M1.6); `features` and `site`, with `headers` for HTML, CSS and JS (M0.14, which awaits its published page); `bench` (M1.9); `golden`, a step of `ci` (M2.4); `theme`, the last step of `ci` (M3.2) | `tools` (M5.1), `package` (M11.6, M12.4, M14.4) | `core`, `codec` |
| `tidepool-bench` | `bench` | 6, tools | deny | `harness`, `baseline`, `cli`, `suites::text` and the binary (M1.9); `suites::font` (M2.5); `suites::ui` (M3.3); `suites::editor` (M4.1), with the keystroke suites (M4.2) | suites for M4.4, M9.4, M14.3, M15.2, M15.5 | `core`, `codec`, `text`, `font`, `gfx`, `ui`, `gui`, `editor` |
| `tidepool-testkit` | `crates/testkit` | 6, tools (dev-only) | deny | nothing | M5.1, M6.1 | none |

## Sources

- `Cargo.toml`, `.cargo/config.toml`, `rust-toolchain.toml` and each
  member's `Cargo.toml`
- each member's `README.md` (`## Status`), and the root's `LICENSE` and
  `NOTICE` and each member's copies
- `crates/core/src/lib.rs`, `crates/core/src/testing.rs`,
  `crates/core/src/testing/fuzz.rs` and its modules `engine.rs`,
  `memory.rs`, `mutate.rs` and `paths.rs`
- `crates/codec/src/lib.rs`, `crates/codec/src/qoi.rs`,
  `crates/codec/tests/fuzz.rs`; `crates/gfx/tests/golden.rs`,
  `crates/gui/tests/golden.rs`, `crates/gui/tests/controls/mod.rs`,
  `crates/gui/tests/components/mod.rs` and
  `crates/gui/tests/overlays/mod.rs`, and the references in
  `tests/golden/`
- `crates/text/src/lib.rs`, `crates/text/tests/fuzz.rs`
- `crates/sys/src/lib.rs`, `dynlib.rs`, `linux.rs`, `linux/libc.rs`,
  `linux/x11.rs`; `crates/platform/src/lib.rs`,
  `crates/ui/src/lib.rs`, `crates/ui/src/theme.rs`,
  `crates/ui/src/overlay.rs`,
  `crates/ui/src/widgets.rs` and `crates/ui/src/widgets/`,
  `crates/gui/src/first_frame.rs`, `themes/tidepool-dark.toml`,
  `crates/app/src/main.rs`, every file in
  `bench/src/`, `crates/testkit/src/lib.rs`
- `xtask/Cargo.toml` and every file in `xtask/src/`
- `.github/workflows/ci.yml`, `.github/workflows/codeql.yml`
- `docs/00-overview.md`; `docs/01-architecture.md` for the layers, the
  dependency matrix and the data flow; `docs/04-quality.md` for the checks,
  golden frames, fuzz smoke, benchmarks and fakes; `docs/roadmap.md` for
  what is planned
- `docs/subsystems/toolchains.md`, `agents.md`, `languages.md`,
  `editor.md`, `frontends.md`, `ui-and-theme.md`, `rendering.md`,
  `sys-and-platform.md` and `mobile.md`
- `docs/decisions.md`: ADR-0002, ADR-0003, ADR-0008, ADR-0009, ADR-0010,
  ADR-0017, ADR-0018, ADR-0027, ADR-0028, ADR-0029, ADR-0044, ADR-0046,
  ADR-0047, ADR-0049, ADR-0052, ADR-0053, ADR-0054, ADR-0055 and ADR-0056
