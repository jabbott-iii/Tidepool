# UML component diagrams

These diagrams show Tidepool as UML components: the crates, and inside some
of them their modules, with the interfaces each one provides and the
interfaces it uses. Mermaid has no UML component notation, so they are
flowcharts (ADR-0029): a provided interface is a small circle joined to its
component, and a required interface is an arrow from the component that
uses it to the circle. The groups are the subsystems of `docs/subsystems/`,
and every arrow between two crates is an edge that the dependency matrix in
`docs/01-architecture.md` allows.

Drawn from the code and docs as of roadmap task M0.14 (2026-11-08, version 0.1). What the code has is drawn solid; what the roadmap plans is dashed and labelled with the task that builds it (ADR-0029).

## Legend

| Style | Means |
| :- | :- |
| Blue box | A component the code has: a crate, a module, or a file of tests |
| White box with a dashed border | Planned: a crate that is only its doc comment, or a part not built yet; the label names the task that builds it |
| Blue circle | An interface the code provides, named by its public items |
| White circle with a dashed border | A planned interface, labelled with the task that adds it |
| Solid line between a box and a circle | The box provides the interface |
| Dotted line between a box and a circle | The box will provide the interface; the task is on the circle |
| Solid arrow into a circle | A required interface: the box uses it in the code now |
| Dotted arrow into a circle | A planned use, labelled with the task that adds it |
| Dark blue, rounded | A person |
| Grey box | A program outside Tidepool |
| Blue cylinder | Files in the repository |

- A circle names the code's public items where they exist (`json::parse`),
  the spec's names where the code has none yet (`Pty::spawn`), and words
  where the spec names none.
- An arrow between two crates is allowed by the using crate's row in the
  dependency matrix. Arrows inside one crate join its modules.
- A box that has code is drawn solid even where part of it is planned; its
  label says which part. A crate that is only its doc comment is drawn
  planned, with the tasks that give it code.
- A subsystem too big for one readable diagram is drawn in several, so the
  same component can appear in more than one.
- `tidepool-core` is left out of sections 1 to 5: every crate but `sys` may
  use it. Section 6 shows what uses it today.

## 1. Overview: the subsystems

The subsystems and the main interfaces between them, from the top layer
down: 1.1 what the app, the frontends and the editor use, 1.2 what the
agents, languages and toolchains offer one another, and 1.3 what all of
them stand on. Only formats and text (`codec` and `text`) and `xtask` have
code behind their interfaces; the app and the OS layer are their crate
roots (M0.1). Each arrow names the crate in its group that will use an
interface and the task that adds that use.

### 1.1 The app, the frontends and the editor

```mermaid
flowchart TB
    c_app["App: tidepool, the binary<br/>an empty main today, M0.1<br/>main loop planned M3.4"]

    i_front(("Frontend<br/>planned M3.4, M7.1"))
    i_editor(("editor views<br/>and commands<br/>planned M4.1, M4.6"))

    c_front["Frontends<br/>tidepool-gui<br/>tidepool-tui<br/>planned M2.6, M7.1"]
    c_editor["Editor and workspace<br/>tidepool-editor<br/>tidepool-workspace<br/>planned M4.1, M4.7"]

    i_draw(("widget tree,<br/>DrawList,<br/>TextMetrics<br/>planned M3.1"))
    i_raster(("Framebuffer,<br/>glyph atlas, layout<br/>planned M2.3, M2.5"))
    i_agentf(("completions,<br/>chat, edit proposals<br/>planned M9.4, M9.5"))
    i_langf(("highlights,<br/>LSP features,<br/>Markdown tree<br/>planned M6.2,<br/>M6.5, M6.7"))
    i_toolf(("terminal grid,<br/>git, mobile pipelines<br/>planned M5.8,<br/>M8.1, M10.4"))

    c_render["Rendering<br/>tidepool-gfx<br/>tidepool-font<br/>tidepool-ui<br/>planned M2.3, M2.8, M3.1"]
    c_agents["Agents<br/>tidepool-agent<br/>planned M9.3, M9.4"]
    c_lang["Languages<br/>tidepool-syntax<br/>tidepool-markdown<br/>tidepool-rpc<br/>tidepool-lang<br/>planned M6.1, M6.2,<br/>M6.5, M6.7"]
    c_tools["Toolchains<br/>tidepool-toolchain<br/>tidepool-vcs<br/>tidepool-term<br/>tidepool-mobile<br/>planned M5.1, M5.8,<br/>M8.1, M10.2"]

    c_app -.->|"app:<br/>planned M3.4, M7.1"| i_front
    c_app -.->|"app:<br/>planned M4.1"| i_editor
    i_front -.- c_front
    i_editor -.- c_editor

    c_front -.->|"gui, tui:<br/>planned M3.5, M7.1"| i_draw
    c_front -.->|"gui:<br/>planned M2.6"| i_raster
    c_editor -.->|"editor:<br/>planned M4.1"| i_draw
    c_editor -.->|"editor:<br/>planned M9.4, M9.5"| i_agentf
    c_editor -.->|"editor:<br/>planned M6.2, M6.6, M6.7"| i_langf
    c_editor -.->|"editor:<br/>planned M5.4, M8.2, M10.6"| i_toolf

    i_draw -.- c_render
    i_raster -.- c_render
    i_agentf -.- c_agents
    i_langf -.- c_lang
    i_toolf -.- c_tools

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    class c_app built
    class c_front,c_editor,c_render,c_agents,c_lang,c_tools planned
    class i_front,i_editor,i_draw,i_raster,i_agentf,i_langf,i_toolf planned
```

- The app's matrix row is "all crates above", so it may use every crate in
  the rows above its own (not `xtask`, `tidepool-bench` or
  `tidepool-testkit`); only its two main uses are drawn.
- The editor never depends on `gui` or `tui`, and the frontends never
  depend on the editor: the app connects them (`docs/01-architecture.md`).
- `Frontend` is the common interface of `docs/subsystems/frontends.md`. The
  spec does not say which crate defines the trait; `gui` and `tui` cannot
  share one from the app, and the crates both may use are `core`,
  `platform` and `ui`.

### 1.2 Agents, languages and toolchains

```mermaid
flowchart TB
    c_agents["Agents<br/>tidepool-agent<br/>planned M9.3, M9.4"]
    e_agents["Language agents<br/>TAP over stdio"]
    i_rpc(("JSON-RPC<br/>transport<br/>planned M6.1"))
    c_lang["Languages<br/>tidepool-syntax<br/>tidepool-markdown<br/>tidepool-rpc<br/>tidepool-lang<br/>planned M6.1, M6.2,<br/>M6.5, M6.7"]
    i_ws(("folders, trust,<br/>settings, buffers<br/>planned M4.7, M4.3"))
    i_tools(("tool registry,<br/>Spawn, sessions<br/>planned M5.1, M5.2"))
    c_editor["Editor and workspace<br/>tidepool-editor<br/>tidepool-workspace<br/>planned M4.1, M4.7"]
    c_tools["Toolchains<br/>tidepool-toolchain<br/>tidepool-vcs<br/>tidepool-term<br/>tidepool-mobile<br/>planned M5.1, M5.8,<br/>M8.1, M10.2"]
    e_progs["Outside programs<br/>rust-analyzer, git,<br/>curl, SDK tools"]

    c_agents -.->|"agent: planned M9.3"| e_agents
    c_agents -.->|"agent: planned M9.3"| i_rpc
    c_agents -.->|"agent: planned M9.3"| i_ws
    c_agents -.->|"agent: planned M9.3"| i_tools
    i_rpc -.- c_lang
    c_lang -.->|"lang: planned M6.7"| i_ws
    c_lang -.->|"lang: planned M6.7"| i_tools
    i_ws -.- c_editor
    i_tools -.- c_tools
    c_tools -.->|"toolchain: planned M5.2"| e_progs

    classDef external fill:#8a8a8a,stroke:#6b6b6b,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    class c_agents,c_lang,c_editor,c_tools planned
    class i_rpc,i_ws,i_tools planned
    class e_agents,e_progs external
```

### 1.3 What they stand on: formats and text, and the OS layer

```mermaid
flowchart LR
    c_front["Frontends<br/>tidepool-gui<br/>tidepool-tui<br/>planned M2.6, M7.1"]
    c_editor["Editor and workspace<br/>tidepool-editor<br/>tidepool-workspace<br/>planned M4.1, M4.7"]
    c_agents["Agents<br/>tidepool-agent<br/>planned M9.3, M9.4"]
    c_lang["Languages<br/>tidepool-syntax<br/>tidepool-markdown<br/>tidepool-rpc<br/>tidepool-lang<br/>planned M6.1, M6.2,<br/>M6.5, M6.7"]
    c_tools["Toolchains<br/>tidepool-toolchain<br/>tidepool-vcs<br/>tidepool-term<br/>tidepool-mobile<br/>planned M5.1, M5.8,<br/>M8.1, M10.2"]
    c_xtask["xtask<br/>dev-only: the checks,<br/>fuzz-smoke, the demo page"]

    i_os_win(("window, events,<br/>present, terminal<br/>raw mode<br/>planned M2.2, M7.1"))
    i_text(("Rope, Transaction,<br/>History, file"))
    i_codec(("json: parse, write<br/>toml: parse"))
    i_regex(("regex matching<br/>planned M4.8"))
    i_os_watch(("watch<br/>planned M4.5"))
    i_os_proc(("process, PTY<br/>planned M5.7, M5.9"))

    c_fmt["Formats and text<br/>tidepool-codec: json, toml<br/>tidepool-text: rope to<br/>atomic save<br/>tidepool-regex:<br/>planned M4.8"]
    c_os["OS layer<br/>tidepool-sys<br/>tidepool-platform<br/>crate roots today, M0.1<br/>modules planned M2.1, M2.2"]
    e_os["OS libraries<br/>libc and libX11,<br/>AppKit and Win32 later"]
    e_cargo["cargo"]

    c_front -.->|"gui, tui:<br/>planned M2.6, M7.1"| i_os_win
    c_editor -.->|"editor, workspace:<br/>planned M4.1, M4.2, M4.3"| i_text
    c_editor -.->|"workspace:<br/>planned M4.7"| i_codec
    c_editor -.->|"editor, workspace:<br/>planned M4.4"| i_regex
    c_editor -.->|"workspace:<br/>planned M4.5"| i_os_watch
    c_agents -.->|"agent:<br/>planned M9.3"| i_text
    c_agents -.->|"agent:<br/>planned M9.3"| i_codec
    c_lang -.->|"syntax, lang:<br/>planned M6.2, M6.7"| i_text
    c_lang -.->|"rpc, syntax:<br/>planned M6.1, M6.2"| i_codec
    c_lang -.->|"syntax:<br/>planned M13.2"| i_regex
    c_tools -.->|"toolchain:<br/>planned M5.1"| i_codec
    c_tools -.->|"toolchain, term:<br/>planned M5.2, M5.4"| i_os_proc
    c_xtask -->|"xtask: cargo<br/>metadata, manifests"| i_codec
    c_xtask -->|"the one program<br/>it starts"| e_cargo

    i_os_win -.- c_os
    i_text --- c_fmt
    i_codec --- c_fmt
    i_regex -.- c_fmt
    i_os_watch -.- c_os
    i_os_proc -.- c_os
    c_os -.->|"sys:<br/>planned M2.1"| e_os

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef external fill:#8a8a8a,stroke:#6b6b6b,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    class c_xtask,c_fmt,c_os,i_text,i_codec built
    class c_front,c_editor,c_agents,c_lang,c_tools planned
    class i_os_win,i_regex,i_os_watch,i_os_proc planned
    class e_os,e_cargo external
```

## 2. Text and formats

`tidepool-codec` and `tidepool-text` are built, and neither has a library
dependency: `tidepool-core` is a dev-dependency of each, for their tests
(6.5). `tidepool-regex` is an empty library until M4.8.

### 2.1 tidepool-codec

`json` (M1.4) and `toml` (M1.7) parse within `Limits` and fail with
`CodecError`, and `json` also writes. `xtask` is their only user today:
`metadata.rs` reads `cargo metadata` with `json::parse_bytes`, and
`crates.rs` reads the members' manifests with `toml::parse`.

```mermaid
flowchart LR
    c_xtask["xtask<br/>metadata.rs, crates.rs"]
    c_rpc["tidepool-rpc<br/>planned M6.1"]
    c_lang["tidepool-lang<br/>planned M6.4"]
    c_toolchain["tidepool-toolchain<br/>planned M5.1"]
    c_syntax["tidepool-syntax<br/>planned M6.2"]
    c_agent["tidepool-agent<br/>planned M9.3"]
    c_editor["tidepool-editor<br/>planned M4.6, M13.4"]
    c_workspace["tidepool-workspace<br/>planned M4.7"]
    c_mobile["tidepool-mobile<br/>planned M10.2, M11.3"]

    i_jparse(("json::parse,<br/>parse_bytes"))
    i_jwrite(("json::<br/>to_string_compact,<br/>to_string_pretty"))
    i_tparse(("toml::parse,<br/>parse_bytes"))
    i_limits(("Limits,<br/>CodecError"))
    i_tdoc(("TOML document<br/>model and writer<br/>planned M1.5"))
    i_qoi(("qoi<br/>planned M2.4"))
    i_png(("inflate,<br/>deflate, png<br/>planned M10.7"))
    i_zip(("zip, xml<br/>planned M10.2"))
    i_plist(("plist<br/>planned M11.3"))

    c_codec["tidepool-codec<br/>json: RFC 8259 and JSONC, M1.4<br/>toml: TOML 1.1 with spans, M1.7"]

    c_xtask -->|"cargo metadata"| i_jparse
    c_xtask -->|"members' manifests"| i_tparse
    c_xtask -->|"both"| i_limits
    c_xtask -.->|"golden frames,<br/>planned M2.4"| i_qoi
    c_rpc -.->|"messages,<br/>planned M6.1"| i_jparse
    c_rpc -.->|"messages,<br/>planned M6.1"| i_jwrite
    c_lang -.->|"cargo diagnostics,<br/>planned M6.4"| i_jparse
    c_toolchain -.->|"descriptors,<br/>planned M5.1"| i_tparse
    c_syntax -.->|"language packs,<br/>planned M6.2"| i_tparse
    c_agent -.->|"agent.toml,<br/>planned M9.3"| i_tparse
    c_editor -.->|"keymaps,<br/>planned M4.6"| i_tparse
    c_editor -.->|"image viewer,<br/>planned M13.4"| i_png
    c_workspace -.->|"settings written back,<br/>planned M4.7"| i_tdoc
    c_mobile -.->|"inspect,<br/>planned M10.2"| i_zip
    c_mobile -.->|"ExportOptions.plist,<br/>planned M11.3"| i_plist

    i_jparse --- c_codec
    i_jwrite --- c_codec
    i_tparse --- c_codec
    i_limits --- c_codec
    i_tdoc -.- c_codec
    i_qoi -.- c_codec
    i_png -.- c_codec
    i_zip -.- c_codec
    i_plist -.- c_codec

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    class c_xtask,c_codec,i_jparse,i_jwrite,i_tparse,i_limits built
    class c_rpc,c_lang,c_toolchain,c_syntax,c_agent,c_editor,c_workspace,c_mobile planned
    class i_tdoc,i_qoi,i_png,i_zip,i_plist planned
```

- The YAML subset, CSV and INI in the spec's table are in the backlog, with
  no task, so they are not drawn.
- `ui`'s `theme` loads `themes/tidepool-dark.toml`
  (`docs/subsystems/ui-and-theme.md`, M3.2), but the matrix lets `ui`
  depend only on `core`, so `ui` cannot call `toml::parse`. No arrow is
  drawn from `ui`: the specs do not say which crate reads the theme file.

### 2.2 tidepool-text and tidepool-regex

`tidepool-text` has every module its spec plans (M1.1, M1.2, M1.3); this
diagram draws who will use its interfaces, and 2.3 how its modules use one
another. Each module box names the task that built it.

```mermaid
flowchart LR
    c_editor["tidepool-editor<br/>planned M4.1, M4.2, M4.4"]
    c_workspace["tidepool-workspace<br/>planned M4.3, M4.4"]
    c_syntax["tidepool-syntax<br/>planned M6.2, M13.2"]
    c_lang["tidepool-lang<br/>planned M6.7"]
    c_agent["tidepool-agent<br/>planned M9.3"]

    i_rope(("Rope: insert, remove,<br/>replace, slice"))
    i_pos(("byte_to_line_col,<br/>line_col_to_byte, Unit,<br/>grapheme boundaries"))
    i_tx(("ChangeSet: apply,<br/>invert, compose, map<br/>Transaction::change_each"))
    i_hist(("History: commit,<br/>undo, redo, jump_to,<br/>checkpoint, mark_saved"))
    i_file(("file::load,<br/>file::save"))
    i_regex(("linear-time<br/>matching<br/>planned M4.8"))

    c_rope["text::rope<br/>M1.1"]
    c_pos["text::pos<br/>M1.1"]
    c_tx["text::transaction<br/>M1.2"]
    c_hist["text::history<br/>M1.2"]
    c_file["text::file<br/>M1.3"]
    c_regex["tidepool-regex<br/>an empty library<br/>planned M4.8"]

    c_editor -.->|"text rendering from<br/>the rope, planned M4.1"| i_rope
    c_agent -.->|"prefix and suffix,<br/>planned M9.3"| i_rope
    c_editor -.->|"grapheme motions,<br/>planned M4.2"| i_pos
    c_lang -.->|"UTF-16 positions,<br/>planned M6.7"| i_pos
    c_editor -.->|"multi-cursor,<br/>planned M4.2"| i_tx
    c_syntax -.->|"re-lex new ranges,<br/>planned M6.2"| i_tx
    c_lang -.->|"didChange,<br/>planned M6.7"| i_tx
    c_editor -.->|"undo and redo,<br/>planned M4.2"| i_hist
    c_workspace -.->|"saves,<br/>planned M4.3"| i_hist
    c_workspace -.->|"buffers,<br/>planned M4.3"| i_file
    c_editor -.->|"find and replace,<br/>planned M4.4"| i_regex
    c_workspace -.->|"project search,<br/>planned M4.4"| i_regex
    c_syntax -.->|"pack lexers,<br/>planned M13.2"| i_regex

    i_rope --- c_rope
    i_pos --- c_pos
    i_tx --- c_tx
    i_hist --- c_hist
    i_file --- c_file
    i_regex -.- c_regex

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    class c_rope,c_pos,c_tx,c_hist,c_file built
    class i_rope,i_pos,i_tx,i_hist,i_file built
    class c_editor,c_workspace,c_syntax,c_lang,c_agent,c_regex,i_regex planned
```

- `pos` adds its conversions and grapheme queries to `Rope` itself, so its
  circle names `Rope` methods.

### 2.3 Inside tidepool-text

`history` applies change sets and restores selections, a `Transaction`
holds the selection after its change, and `file` loads and saves through
`encoding`.

```mermaid
flowchart TB
    c_hist["text::history<br/>M1.2"]
    c_file["text::file<br/>M1.3"]
    i_tx(("ChangeSet,<br/>Transaction"))
    c_tx["text::transaction<br/>M1.2"]
    i_sel(("Selection:<br/>validate, map"))
    c_sel["text::selection<br/>M1.2"]
    i_enc(("decode, encode,<br/>Decoder::push"))
    c_enc["text::encoding<br/>M1.3"]

    c_hist -->|"applies"| i_tx
    c_hist -->|"before and after"| i_sel
    c_tx -->|"selection after"| i_sel
    c_file -->|"reads and writes"| i_enc
    i_tx --- c_tx
    i_sel --- c_sel
    i_enc --- c_enc

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    class c_hist,c_file,c_tx,c_sel,c_enc,i_tx,i_sel,i_enc built
```

- Every module takes or returns a `Rope`, and `Selection::map` also maps
  through a `ChangeSet`; those arrows are left out to keep the view
  readable.

## 3. The OS layer and rendering

Only the crate roots of `sys` and `platform` are built (M0.1), so they are
drawn solid with their planned modules in their labels; `gfx`, `font` and
`ui` are empty libraries.

### 3.1 tidepool-sys and tidepool-platform: windows and the terminal

`sys` declares the OS interfaces and `platform` wraps them safely; they are
the only crates that allow `unsafe_code`. The modules follow
`docs/subsystems/sys-and-platform.md`, on Linux first; macOS (M11.7, M11.8)
and Windows (M12.1, M12.2) follow. This diagram draws what the frontends
will use and the declarations under it, and 3.2 the rest of `platform`.

```mermaid
flowchart TB
    c_gui["tidepool-gui<br/>planned M2.6"]
    c_tui["tidepool-tui<br/>planned M7.1"]

    i_window(("Window<br/>planned M2.2"))
    i_events(("wait_events,<br/>Waker<br/>planned M2.2"))
    i_surface(("surface:<br/>present<br/>planned M2.2"))
    i_clip(("clipboard<br/>planned M2.7"))
    i_terminal(("terminal:<br/>raw mode guard<br/>planned M7.1"))

    c_platform["tidepool-platform<br/>the crate root today, M0.1<br/>modules planned M2.2 on"]
    i_decl(("dynlib, libc,<br/>x11 declarations<br/>planned M2.1"))
    c_sys["tidepool-sys<br/>the crate root today, M0.1<br/>declarations planned M2.1"]
    e_os["OS libraries<br/>libc and libX11 on Linux<br/>AppKit, Win32 later"]

    c_gui -.->|"planned M2.6"| i_window
    c_gui -.->|"planned M2.6"| i_events
    c_gui -.->|"planned M2.6"| i_surface
    c_gui -.->|"planned M4.2"| i_clip
    c_tui -.->|"planned M7.1"| i_terminal

    i_window -.- c_platform
    i_events -.- c_platform
    i_surface -.- c_platform
    i_clip -.- c_platform
    i_terminal -.- c_platform

    c_platform -.->|"planned M2.2"| i_decl
    i_decl -.- c_sys
    c_sys -.->|"binds, planned M2.1"| e_os

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef external fill:#8a8a8a,stroke:#6b6b6b,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    class c_platform,c_sys built
    class c_gui,c_tui planned
    class i_window,i_events,i_surface,i_clip,i_terminal,i_decl planned
    class e_os external
```

- `Waker::wake` may be called from any thread and interrupts
  `wait_events`; the main loop gets it from the frontend (`Frontend::waker`).

### 3.2 tidepool-platform: paths, watching, processes and the PTY

```mermaid
flowchart TB
    c_app["tidepool, the app<br/>an empty main, M0.1<br/>main loop planned M3.4"]
    c_workspace["tidepool-workspace<br/>planned M4.5, M4.7"]
    c_toolchain["tidepool-toolchain<br/>planned M5.1, M5.2"]
    c_term["tidepool-term<br/>planned M5.4"]

    i_paths(("paths, dirs<br/>planned M2.7"))
    i_watch(("watch<br/>planned M4.5"))
    i_process(("process<br/>planned M5.7"))
    i_pty(("Pty::spawn<br/>planned M5.9"))

    c_platform["tidepool-platform<br/>the crate root today, M0.1<br/>modules planned M2.2 on"]

    c_app -.->|"settings,<br/>planned M3.4"| i_paths
    c_workspace -.->|"trust in user data,<br/>planned M4.7"| i_paths
    c_workspace -.->|"debounced events,<br/>planned M4.5"| i_watch
    c_toolchain -.->|"standard locations,<br/>planned M5.1"| i_paths
    c_toolchain -.->|"Spawn,<br/>planned M5.2"| i_process
    c_term -.->|"terminal panel,<br/>planned M5.4"| i_pty

    i_paths -.- c_platform
    i_watch -.- c_platform
    i_process -.- c_platform
    i_pty -.- c_platform

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    class c_app,c_platform built
    class c_workspace,c_toolchain,c_term planned
    class i_paths,i_watch,i_process,i_pty planned
```

- Of the crates in the binary, only `toolchain`, `term`, `workspace`, `gui`,
  `tui` and the app may depend on `platform` (`tidepool-bench` may use any
  crate), so the editor's terminal panel (M5.4) reaches the PTY through
  `term`.

### 3.3 tidepool-ui

`gfx`, `font` and `ui` are pure: no `unsafe` and no OS calls. `ui` lays out
in logical units and emits draw lists; `gui` rasterizes them with `gfx` and
`font` (3.4), and `tui` maps them onto cells. `TextMetrics` is a trait that
`ui` defines and each frontend implements.

```mermaid
flowchart TB
    c_editor["tidepool-editor<br/>planned M4.1, M4.3"]
    c_gui["tidepool-gui<br/>planned M3.5"]
    c_tui["tidepool-tui<br/>planned M7.1"]
    c_bridge["OS accessibility bridges<br/>AT-SPI, NSAccessibility,<br/>UI Automation<br/>planned M14.2"]

    i_widgets(("widgets<br/>planned M3.6, M3.3"))
    i_tree(("tree, layout,<br/>focus, input<br/>planned M3.1"))
    i_theme(("theme tokens<br/>planned M3.2"))
    i_drawlist(("DrawList<br/>and damage<br/>planned M3.1"))
    i_metrics(("TextMetrics<br/>planned M3.1"))
    i_a11y(("accessibility<br/>tree<br/>planned M3.1"))
    c_ui["tidepool-ui<br/>an empty library<br/>planned M3.1, M3.2"]

    c_editor -.->|"editor view,<br/>planned M4.1"| i_tree
    c_editor -.->|"editor view,<br/>planned M4.1"| i_drawlist
    c_editor -.->|"explorer and tabs,<br/>planned M4.3"| i_widgets
    c_gui -.->|"paints,<br/>planned M3.5"| i_drawlist
    c_gui -.->|"implements,<br/>planned M3.5"| i_metrics
    c_tui -.->|"maps onto cells,<br/>planned M7.1"| i_drawlist
    c_tui -.->|"implements,<br/>planned M7.1"| i_metrics
    c_bridge -.->|"planned M14.2"| i_a11y

    i_widgets -.- c_ui
    i_tree -.- c_ui
    i_theme -.- c_ui
    i_drawlist -.- c_ui
    i_metrics -.- c_ui
    i_a11y -.- c_ui

    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    class c_editor,c_gui,c_tui,c_bridge,c_ui planned
    class i_widgets,i_tree,i_theme,i_drawlist,i_metrics,i_a11y planned
```

- The accessibility bridges (M14.2) read `ui`'s tree, and no spec names
  the crate that holds them. `platform` may not depend on `ui`, so they
  cannot live in `platform` alone.

### 3.4 tidepool-gfx and tidepool-font

`gui` is the only crate that will use them.

```mermaid
flowchart LR
    c_gui["tidepool-gui<br/>planned M2.6, M3.5"]

    i_fb(("Framebuffer<br/>planned M2.3"))
    i_raster(("raster, blend,<br/>clip, glyph blit<br/>planned M2.3"))
    i_backdrop(("backdrop cache<br/>planned M2.3"))
    i_damage(("damage merging<br/>planned M2.3"))
    c_gfx["tidepool-gfx<br/>an empty library<br/>planned M2.3"]

    i_ttf(("ttf, outline<br/>planned M2.8"))
    i_atlas(("glyph atlas<br/>planned M2.5"))
    i_layout(("layout, fallback<br/>planned M2.5"))
    c_font["tidepool-font<br/>an empty library<br/>planned M2.8, M2.5"]

    c_gui -.->|"planned M2.6"| i_fb
    c_gui -.->|"planned M2.6"| i_raster
    c_gui -.->|"planned M2.6"| i_backdrop
    c_gui -.->|"planned M3.5"| i_damage
    c_gui -.->|"planned M2.6"| i_atlas
    c_gui -.->|"TextMetrics,<br/>planned M3.5"| i_layout

    i_fb -.- c_gfx
    i_raster -.- c_gfx
    i_backdrop -.- c_gfx
    i_damage -.- c_gfx
    i_ttf -.- c_font
    i_atlas -.- c_font
    i_layout -.- c_font

    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    class c_gui,c_gfx,c_font planned
    class i_fb,i_raster,i_backdrop,i_damage,i_ttf,i_atlas,i_layout planned
```

- `gfx`'s glyph blit draws coverage masks from `font`'s atlas, but `gfx`
  may depend only on `core`: `gui` hands the masks over, and no arrow
  joins `gfx` and `font`. `ttf` and `outline` serve `font`'s own atlas.

## 4. Languages and toolchains

None of the crates of this section is built: `toolchain`, `vcs`, `term`,
`mobile`, `rpc`, `syntax`, `markdown` and `lang` are empty libraries.

### 4.1 tidepool-toolchain, tidepool-vcs, tidepool-term and tidepool-mobile

Only `toolchain` finds, installs, updates and starts outside programs
(ADR-0002), and `vcs` and `mobile` run theirs through it. `term` parses
what a shell writes to the PTY that `platform` gives it.

```mermaid
flowchart TB
    c_editor["tidepool-editor<br/>panels, planned M5.4,<br/>M8.2, M10.6"]

    i_grid(("VT parser<br/>and grid<br/>planned M5.8"))
    c_term["tidepool-term<br/>planned M5.8"]
    i_status(("status,<br/>diff hunks<br/>planned M8.1"))
    i_blame(("blame, log<br/>planned M8.3"))
    c_vcs["tidepool-vcs<br/>planned M8.1"]
    i_inspect(("inspect<br/>planned M10.2"))
    i_android(("android::sign,<br/>gradle, direct,<br/>devices<br/>planned M10.3,<br/>M10.4, M10.5, M10.6"))
    i_ios(("ios::xcode,<br/>ios::sign, ios::direct,<br/>ios::remote<br/>planned M11.3,<br/>M11.4, M11.5"))
    c_mobile["tidepool-mobile<br/>planned M10.2,<br/>M10.3, M11.3"]

    i_desc(("descriptor<br/>planned M5.1"))
    i_registry(("detect,<br/>registry<br/>planned M5.1"))
    i_spawn(("process:<br/>Spawn<br/>planned M5.2"))
    i_sessions(("sessions<br/>planned M5.2"))
    i_secret(("SecretArg,<br/>SecretEnv<br/>planned M5.2"))
    i_install(("download,<br/>install plans<br/>planned M5.3"))
    c_toolchain["tidepool-toolchain<br/>planned M5.1, M5.2, M5.3"]

    i_process(("platform:<br/>process<br/>planned M5.7"))
    i_pty(("platform:<br/>Pty<br/>planned M5.9"))
    c_platform["tidepool-platform<br/>the crate root today, M0.1<br/>process, Pty planned<br/>M5.7, M5.9"]
    i_tparse(("codec:<br/>toml::parse"))
    i_zip(("codec: zip<br/>planned M10.2"))
    c_codec["tidepool-codec"]

    e_tools["Managed tools<br/>rustup, rust-analyzer, git,<br/>JDK, Android SDK,<br/>bundletool, Xcode"]
    e_curl["curl<br/>HTTPS downloads"]

    c_editor -.->|"terminal panel,<br/>planned M5.4"| i_grid
    c_editor -.->|"source control,<br/>planned M8.2"| i_status
    c_editor -.->|"diff view, blame,<br/>planned M8.3"| i_blame
    c_editor -.->|"devices, artifacts,<br/>planned M10.6"| i_android
    c_editor -.->|"simulators, signing,<br/>planned M11.6"| i_ios

    i_grid -.- c_term
    i_status -.- c_vcs
    i_blame -.- c_vcs
    i_inspect -.- c_mobile
    i_android -.- c_mobile
    i_ios -.- c_mobile

    c_term -.->|"planned M5.4"| i_pty
    c_vcs -.->|"git from the registry,<br/>planned M8.1"| i_registry
    c_vcs -.->|"planned M8.1"| i_spawn
    c_mobile -.->|"planned M10.3"| i_spawn
    c_mobile -.->|"keystore passwords,<br/>planned M10.3"| i_secret
    c_mobile -.->|"adb logcat, emulators,<br/>planned M10.6"| i_sessions
    c_mobile -.->|"planned M10.2"| i_zip

    i_desc -.- c_toolchain
    i_registry -.- c_toolchain
    i_spawn -.- c_toolchain
    i_sessions -.- c_toolchain
    i_secret -.- c_toolchain
    i_install -.- c_toolchain

    c_toolchain -.->|"planned M5.2"| i_process
    c_toolchain -.->|"descriptors,<br/>planned M5.1"| i_tparse
    c_toolchain -.->|"argv, never a shell,<br/>planned M5.2"| e_tools
    c_toolchain -.->|"planned M5.3"| e_curl

    i_process -.- c_platform
    i_pty -.- c_platform
    i_tparse --- c_codec
    i_zip -.- c_codec

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef external fill:#8a8a8a,stroke:#6b6b6b,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    class c_codec,c_platform,i_tparse built
    class c_editor,c_term,c_vcs,c_mobile,c_toolchain planned
    class i_grid,i_status,i_blame,i_inspect,i_android,i_ios planned
    class i_desc,i_registry,i_spawn,i_sessions,i_secret,i_install,i_process,i_pty,i_zip planned
    class e_tools,e_curl external
```

- `lang` (M6.7) and `agent` (M9.3) use `sessions` too; they are drawn in
  4.3 and 5.2.
- The editor's Toolchains panel (M5.6) is backed by `toolchain` in
  `docs/subsystems/editor.md`, and installs that need admin rights run in
  the integrated terminal (`docs/subsystems/toolchains.md`). The matrix
  does not let the editor use `toolchain`, nor `toolchain` use `term`, so
  only the app can join them. Neither edge is drawn, and no arrow goes into
  `install` here.

### 4.2 Languages: what the editor uses

`tidepool-lang` keeps one session per server and workspace root, and gives
the editor its features and services; `syntax` gives it highlights, and
`markdown` the document tree of the preview.

```mermaid
flowchart TB
    c_editor["tidepool-editor<br/>planned M6.2, M6.6, M6.7"]

    i_features(("features: diagnostics,<br/>navigation, completion<br/>planned M6.7,<br/>M6.8, M6.3"))
    i_cargo(("cargo tasks<br/>planned M6.4"))
    i_mdsvc(("Markdown outline,<br/>link checks<br/>planned M6.6"))
    i_dapsvc(("dap: debug<br/>sessions<br/>planned M13.1"))
    i_lexer(("incremental lexers,<br/>highlights<br/>planned M6.2"))
    i_mdtree(("document tree<br/>with source ranges<br/>planned M6.5"))

    c_lang["tidepool-lang<br/>planned M6.7: registry,<br/>session, rust<br/>features M6.8, M6.3<br/>cargo M6.4, markdown M6.6<br/>dap M13.1"]
    c_syntax["tidepool-syntax<br/>planned M6.2"]
    c_markdown["tidepool-markdown<br/>planned M6.5"]

    c_editor -.->|"planned M6.7,<br/>M6.8, M6.3"| i_features
    c_editor -.->|"problems panel,<br/>planned M6.4"| i_cargo
    c_editor -.->|"outline,<br/>planned M6.6"| i_mdsvc
    c_editor -.->|"debug panel,<br/>planned M13.1"| i_dapsvc
    c_editor -.->|"highlighting,<br/>planned M6.2"| i_lexer
    c_editor -.->|"preview,<br/>planned M6.6"| i_mdtree

    i_features -.- c_lang
    i_cargo -.- c_lang
    i_mdsvc -.- c_lang
    i_dapsvc -.- c_lang
    i_lexer -.- c_syntax
    i_mdtree -.- c_markdown

    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    class c_editor,c_lang,c_syntax,c_markdown planned
    class i_features,i_cargo,i_mdsvc,i_dapsvc,i_lexer,i_mdtree planned
```

### 4.3 Languages: tidepool-lang, tidepool-rpc and the servers

`rpc` frames JSON-RPC 2.0 with `Content-Length` headers on codec's `json`,
for LSP, DAP and TAP. `lang` starts each server through toolchain's
`sessions`, and talks to it through `rpc`.

```mermaid
flowchart TB
    c_lang["tidepool-lang<br/>planned M6.7, M13.1"]

    i_framing(("JSON-RPC 2.0,<br/>Content-Length<br/>framing<br/>planned M6.1"))
    i_lsp(("LSP messages<br/>planned M6.1"))
    i_dap(("DAP messages<br/>planned M13.1"))
    i_sessions(("toolchain:<br/>sessions<br/>planned M5.2"))

    c_rpc["tidepool-rpc<br/>planned M6.1, M13.1"]
    c_toolchain["tidepool-toolchain<br/>planned M5.2"]
    i_json(("codec: json::parse,<br/>to_string_compact"))
    c_codec["tidepool-codec"]
    e_ra["rust-analyzer<br/>an LSP server,<br/>over pipes"]
    e_dapad["lldb-dap or CodeLLDB<br/>a debug adapter"]

    c_lang -.->|"planned M6.7"| i_framing
    c_lang -.->|"planned M6.7"| i_lsp
    c_lang -.->|"planned M13.1"| i_dap
    c_lang -.->|"starts servers,<br/>planned M6.7"| i_sessions
    c_lang -.->|"LSP,<br/>planned M6.7"| e_ra
    c_lang -.->|"DAP,<br/>planned M13.1"| e_dapad

    i_framing -.- c_rpc
    i_lsp -.- c_rpc
    i_dap -.- c_rpc
    i_sessions -.- c_toolchain
    c_rpc -.->|"planned M6.1"| i_json
    c_toolchain -.->|"starts,<br/>planned M6.7"| e_ra
    i_json --- c_codec

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef external fill:#8a8a8a,stroke:#6b6b6b,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    class c_codec,i_json built
    class c_lang,c_rpc,c_toolchain planned
    class i_framing,i_lsp,i_dap,i_sessions planned
    class e_ra,e_dapad external
```

- TAP messages: `AGENTS.md`'s workspace map and `rpc`'s README put them in
  `rpc` (M9.3), and `docs/subsystems/agents.md` puts the TAP message types
  in `agent`'s `tap` module, over `rpc`'s transport. Section 5 draws them
  in `agent` (5.1).

### 4.4 Languages: what tidepool-lang and tidepool-syntax stand on

```mermaid
flowchart TB
    c_lang["tidepool-lang<br/>planned M6.4, M6.6, M6.7"]
    c_syntax["tidepool-syntax<br/>planned M6.2, M13.2"]

    i_pack(("language packs<br/>planned M6.2"))
    i_mdtree(("markdown:<br/>document tree<br/>planned M6.5"))
    i_ws(("workspace:<br/>folders, trust<br/>planned M4.7"))
    i_textapi(("text: ChangeSet,<br/>pos conversions"))
    i_json(("codec: json::parse,<br/>to_string_compact"))
    i_tparse(("codec:<br/>toml::parse"))
    i_regex(("regex<br/>planned M4.8"))

    c_markdown["tidepool-markdown<br/>planned M6.5"]
    c_workspace["tidepool-workspace<br/>planned M4.7"]
    c_text["tidepool-text"]
    c_codec["tidepool-codec"]
    c_regex["tidepool-regex<br/>planned M4.8"]

    c_lang -.->|"registry,<br/>planned M6.7"| i_pack
    c_lang -.->|"outline, links,<br/>planned M6.6"| i_mdtree
    c_lang -.->|"roots, trust,<br/>planned M6.7"| i_ws
    c_lang -.->|"positionEncoding,<br/>didChange,<br/>planned M6.7"| i_textapi
    c_lang -.->|"cargo diagnostics,<br/>planned M6.4"| i_json
    i_pack -.- c_syntax
    c_syntax -.->|"re-lex,<br/>planned M6.2"| i_textapi
    c_syntax -.->|"packs,<br/>planned M6.2"| i_tparse
    c_syntax -.->|"pack lexers,<br/>planned M13.2"| i_regex

    i_mdtree -.- c_markdown
    i_ws -.- c_workspace
    i_textapi --- c_text
    i_json --- c_codec
    i_tparse --- c_codec
    i_regex -.- c_regex

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    class c_text,c_codec,i_textapi,i_json,i_tparse built
    class c_lang,c_syntax,c_markdown,c_workspace,c_regex planned
    class i_pack,i_mdtree,i_ws,i_regex planned
```

- LSP positions default to UTF-16, and every conversion goes through
  `tidepool-text`; the offset between a CR and its LF, which LSP cannot
  express, is moved by `lang` (M6.7).

## 5. Agents

`tidepool-agent` hosts language agents, decides what context each one
sees, and merges their suggestions with offline predictions
(`docs/subsystems/agents.md`). It is an empty library: `manifest`, `host`,
`tap`, `context` and `consent` come in M9.3, and `predict` and `merge` in
M9.4.

### 5.1 Inside tidepool-agent

What the editor will use, and how the modules will use one another.

```mermaid
flowchart TB
    c_editor["tidepool-editor<br/>planned M9.4, M9.5"]

    i_merged(("merged completions:<br/>popup items, ghost text<br/>planned M9.4"))
    i_tapmsg(("TAP v0.1 messages:<br/>initialize,<br/>tap/inlineCompletion,<br/>tap/predict, tap/chat,<br/>tap/proposeEdits<br/>planned M9.3"))

    subgraph sg_agent["tidepool-agent"]
        c_merge["merge<br/>planned M9.4"]
        c_predict["predict<br/>trigrams, word n-grams<br/>planned M9.4"]
        c_tap["tap<br/>planned M9.3"]
        c_context["context<br/>size caps, .tidepoolignore,<br/>the secret deny-list<br/>planned M9.3"]
        c_consent["consent<br/>planned M9.3"]
        c_host["host<br/>planned M9.3"]
        c_manifest["manifest<br/>planned M9.3"]
    end

    i_predict(("local predictions<br/>planned M9.4"))
    i_ctx(("each request's<br/>context<br/>planned M9.3"))
    i_grants(("grants: once,<br/>always, never<br/>planned M9.3"))
    i_manifests(("agent.toml<br/>manifests<br/>planned M9.3"))

    c_editor -.->|"popup and ghost text,<br/>planned M9.4"| i_merged
    c_editor -.->|"chat panel, edit proposals,<br/>planned M9.5"| i_tapmsg
    i_merged -.- c_merge
    i_tapmsg -.- c_tap

    c_merge -.->|"planned M9.4"| i_predict
    c_merge -.->|"agent inline completions,<br/>planned M9.4"| i_tapmsg
    c_host -.->|"initialize, shutdown,<br/>planned M9.3"| i_tapmsg
    c_tap -.->|"prefix, suffix, buffers,<br/>planned M9.3"| i_ctx
    c_context -.->|"planned M9.3"| i_grants
    c_host -.->|"planned M9.3"| i_manifests

    i_predict -.- c_predict
    i_ctx -.- c_context
    i_grants -.- c_consent
    i_manifests -.- c_manifest

    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    class c_editor,c_merge,c_predict,c_tap,c_context,c_consent,c_host,c_manifest planned
    class i_merged,i_tapmsg,i_predict,i_ctx,i_grants,i_manifests planned
```

- The spec does not say which module sends each request. The arrows follow
  its module table: `context` builds each request's context, and `host`
  spawns, negotiates with and supervises the agents.

### 5.2 What tidepool-agent stands on

```mermaid
flowchart LR
    c_tap["agent: tap<br/>planned M9.3"]
    c_host["agent: host<br/>planned M9.3"]
    c_context["agent: context<br/>planned M9.3"]
    c_predict["agent: predict<br/>planned M9.4"]
    c_manifest["agent: manifest<br/>planned M9.3"]

    i_framing(("rpc: JSON-RPC<br/>transport<br/>planned M6.1"))
    i_sessions(("toolchain:<br/>sessions<br/>planned M5.2"))
    i_folder(("workspace:<br/>folder, trust<br/>planned M4.7"))
    i_buffers(("workspace:<br/>buffers<br/>planned M4.3"))
    i_rope(("text: Rope"))
    i_tparse(("codec:<br/>toml::parse"))

    c_rpc["tidepool-rpc<br/>planned M6.1"]
    c_toolchain["tidepool-toolchain<br/>planned M5.2"]
    c_workspace["tidepool-workspace<br/>planned M4.3, M4.7"]
    c_text["tidepool-text"]
    c_codec["tidepool-codec"]
    e_agent["Language agents,<br/>separate processes<br/>tp-agent-mock, tp-agent-local,<br/>tp-agent-claude<br/>planned M9.2, M9.6"]

    c_tap -.->|"planned M9.3"| i_framing
    c_host -.->|"spawns, supervises,<br/>planned M9.3"| i_sessions
    c_host -.->|"stdin and stdout,<br/>planned M9.3"| e_agent
    c_context -.->|".tidepoolignore, trust,<br/>planned M9.3"| i_folder
    c_context -.->|"open buffers when<br/>granted, planned M9.3"| i_buffers
    c_context -.->|"prefix and suffix<br/>windows, planned M9.3"| i_rope
    c_predict -.->|"open buffers,<br/>planned M9.4"| i_rope
    c_manifest -.->|"planned M9.3"| i_tparse

    i_framing -.- c_rpc
    i_sessions -.- c_toolchain
    i_folder -.- c_workspace
    i_buffers -.- c_workspace
    i_rope --- c_text
    i_tparse --- c_codec

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    class c_text,c_codec,i_rope,i_tparse built
    class c_tap,c_host,c_context,c_predict,c_manifest planned
    class c_rpc,c_toolchain,c_workspace,e_agent planned
    class i_framing,i_sessions,i_folder,i_buffers planned
```

- Every byte an agent receives is assembled by `context`; agents never get
  paths to read, and cannot ask Tidepool to run anything or write files.
- The spec gives `context` symbols from `lang`, and `merge` LSP completions
  from `lang`, but the matrix's row for `agent` has no `lang`. No arrow is
  drawn from `agent` to `lang`: the editor would have to pass them in.

## 6. xtask and the dev tools

Everything in this section is built except `tidepool-bench`,
`tidepool-testkit` and the `xtask` commands still to come. `xtask` is
dev-only and never shipped. It uses `core` and `codec`, the two crates its
matrix row allows, and starts one program, `cargo` (ADR-0017).

### 6.1 xtask: the checks of ci

`xtask` holds the checks behind `cargo xtask ci`, and `fuzz-smoke` (6.4).
`crates` checks the root's and each member's manifest, each README's first
line, sections and task IDs, and that each member's `LICENSE` and `NOTICE`
are copies of the root's, byte for byte.

```mermaid
flowchart TB
    p_dev(["Developer or CI<br/>runs cargo xtask"])

    subgraph sg_xtask["xtask"]
        c_main["main.rs, check.rs<br/>the command line,<br/>ci and its steps"]
        c_later["bench, golden, theme,<br/>tools, package<br/>not yet implemented<br/>planned M1.9, M2.4,<br/>M3.2, M5.1, M11.6,<br/>M12.4, M14.4"]
        i_step(("Step::Own:<br/>run, returning<br/>a Report"))
        c_deps["deps.rs"]
        c_layers["layers.rs"]
        c_crates["crates.rs"]
        c_features["features.rs"]
        c_unsafe["unsafe_audit.rs"]
        c_headers["headers.rs"]
        c_workflows["workflows.rs"]
        c_ucd["ucd.rs"]
    end
    %% An invisible link: it only keeps the planned commands beside the step circle.
    c_main ~~~ c_later

    f_matrix[("docs/01-architecture.md<br/>the dependency<br/>matrix")]
    f_readmes[("docs/roadmap.md<br/>the root's Cargo.toml,<br/>LICENSE and NOTICE<br/>each member's<br/>Cargo.toml, README.md,<br/>LICENSE and NOTICE")]
    f_ucd[("crates/text/ucd/17.0.0<br/>and the grapheme<br/>tables")]
    f_workflows[(".github/workflows")]
    f_features[("docs/features.jsonc")]

    p_dev -->|"cargo xtask"| c_main
    c_main -->|"each check of ci"| i_step
    i_step --- c_deps
    i_step --- c_layers
    i_step --- c_crates
    i_step --- c_features
    i_step --- c_unsafe
    i_step --- c_headers
    i_step --- c_workflows
    i_step --- c_ucd

    c_layers -->|"reads"| f_matrix
    c_crates -->|"reads, and compares<br/>each copy<br/>with the root's"| f_readmes
    c_ucd -->|"reads, and writes<br/>the tables<br/>without --check"| f_ucd
    c_workflows -->|"reads"| f_workflows
    c_features -->|"reads, with<br/>the roadmap's<br/>task lines"| f_features

    classDef person fill:#08427b,stroke:#052e56,color:#ffffff
    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    classDef store fill:#1168bd,stroke:#0b4884,color:#ffffff
    class p_dev person
    class c_main,i_step,c_deps,c_layers,c_crates,c_features,c_unsafe,c_headers,c_workflows,c_ucd built
    class c_later planned
    class f_matrix,f_readmes,f_ucd,f_workflows,f_features store
```

- `headers` and `unsafe-audit` also read the source files, and `deps` the
  manifests, lockfiles and cargo configuration; those files are left out.
- `cargo xtask site` runs these same steps through `check::run_gate`,
  then the two examples, and fills `site/` into `target/site` (6.2,
  ADR-0030).
- The commands still to come exit 1 with "not yet implemented". The table
  of them in `check.rs` still says `bench` has no roadmap task; M1.9 is
  that task.

### 6.2 xtask: the shared modules

`metadata.rs` runs `cargo metadata` through `cargo.rs`, which starts every
`cargo` that `xtask` runs, and `matrix.rs` reads the dependency matrix and
tells which lines of a document are text.

```mermaid
flowchart TB
    c_main["main.rs, check.rs"]
    c_deps["deps.rs"]
    c_layers["layers.rs"]
    c_crates["crates.rs"]
    c_unsafe["unsafe_audit.rs"]
    c_fuzz["fuzz.rs"]
    c_site["site.rs, site/page.rs<br/>the demo page"]
    i_gate(("check::run_gate,<br/>Gate"))

    i_meta(("metadata::load"))
    i_matrix(("matrix::parse,<br/>classify"))
    c_meta["metadata.rs"]
    c_matrix["matrix.rs"]
    i_cargo(("cargo::run,<br/>capture,<br/>run_keeping_lines"))
    c_cargo["cargo.rs"]
    e_cargo["cargo<br/>the one program xtask starts"]

    c_deps --> i_meta
    c_layers --> i_meta
    c_crates --> i_meta
    c_unsafe --> i_meta
    c_layers -->|"the matrix"| i_matrix
    c_crates -->|"which lines count"| i_matrix
    c_main -->|"fmt, clippy, test"| i_cargo
    c_fuzz -->|"cargo test of<br/>each target"| i_cargo
    c_site -->|"the ci steps"| i_gate
    i_gate --- c_main
    c_site --> i_meta
    c_site -->|"cargo run<br/>--example"| i_cargo

    i_meta --- c_meta
    i_matrix --- c_matrix
    c_meta -->|"cargo metadata"| i_cargo
    i_cargo --- c_cargo
    c_cargo -->|"argv,<br/>CARGO_NET_OFFLINE=true"| e_cargo

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef external fill:#8a8a8a,stroke:#6b6b6b,color:#ffffff
    class c_main,c_deps,c_layers,c_crates,c_unsafe,c_fuzz,c_site,c_meta,c_matrix,c_cargo built
    class i_meta,i_matrix,i_cargo,i_gate built
    class e_cargo external
```

### 6.3 xtask: what it uses from core and codec

```mermaid
flowchart LR
    c_main["main.rs, check.rs"]
    c_ucd["ucd.rs"]
    c_fuzz["fuzz.rs"]
    c_meta["metadata.rs"]
    c_crates["crates.rs"]
    c_features["features.rs"]
    c_site["site.rs, site/page.rs"]

    i_cli(("core: cli::Args,<br/>CoreError"))
    i_hash(("core:<br/>hash::sha256,<br/>to_hex, fnv1a64"))
    i_fuzzapi(("core:<br/>testing::fuzz<br/>Summary, Stop,<br/>keep_died"))
    i_json(("codec:<br/>json::parse_bytes"))
    i_toml(("codec:<br/>toml::parse"))
    i_civil(("core:<br/>civil::DateTime"))

    c_core["tidepool-core<br/>built M0.2, M1.6"]
    c_codec["tidepool-codec<br/>built M1.4, M1.7"]

    c_main -->|"the command line"| i_cli
    c_fuzz --> i_cli
    c_ucd -->|"sha256 pins"| i_hash
    c_fuzz -->|"seeds, file names"| i_hash
    c_fuzz -->|"reads each<br/>result line"| i_fuzzapi
    c_meta -->|"parses cargo<br/>metadata's output"| i_json
    c_crates -->|"members' manifests"| i_toml
    c_features -->|"docs/features.jsonc,<br/>as JSONC"| i_json
    c_site -->|"rust-toolchain.toml"| i_toml
    c_site -->|"writes the JSON-LD"| i_json
    c_site -->|"the year of<br/>a local build"| i_civil
    c_site -->|"--out"| i_cli

    i_cli --- c_core
    i_hash --- c_core
    i_fuzzapi --- c_core
    i_json --- c_codec
    i_toml --- c_codec
    i_civil --- c_core

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    class c_main,c_ucd,c_fuzz,c_meta,c_crates,c_features,c_site,c_core,c_codec built
    class i_cli,i_hash,i_fuzzapi,i_json,i_toml,i_civil built
```

### 6.4 fuzz-smoke and the fuzz targets

`fuzz-smoke` runs each target with `cargo test` and reads the result line
the engine prints.

```mermaid
flowchart TB
    c_main["xtask main.rs, check.rs"]
    i_fuzzrun(("fuzz::Options,<br/>fuzz::run"))
    c_fuzz["xtask fuzz.rs<br/>cargo xtask fuzz-smoke"]
    e_cargo["cargo test, profile fuzz<br/>the fuzz test of each package"]

    c_cfz["codec tests/fuzz.rs<br/>targets json, toml"]
    c_tfz["text tests/fuzz.rs<br/>target encoding"]
    c_more["fuzz targets to come:<br/>qoi, ttf, regex, vt,<br/>rpc framing,<br/>Markdown,<br/>git's parsers,<br/>inflate, png,<br/>zip, xml<br/>planned M2.4, M2.8,<br/>M4.8, M5.8, M6.1,<br/>M6.5, M8.1,<br/>M10.7, M10.2"]

    i_target(("testing::fuzz:<br/>Target, Outcome,<br/>assert_clean"))
    i_summary(("Summary::parse,<br/>keep_died"))
    i_cparse(("json::parse_bytes,<br/>toml::parse_bytes"))
    i_tdec(("Decoder::push,<br/>decode, encode"))

    subgraph sg_core["tidepool-core"]
        c_engine["testing::fuzz<br/>the fuzz engine,<br/>ADR-0027, M1.6"]
        c_rng["rng, M0.2"]
    end
    i_pcg(("rng: Pcg32"))
    c_codec["tidepool-codec library"]
    c_text["tidepool-text library"]
    f_corpus[("fuzz/corpus seeds,<br/>fuzz/crashes findings")]

    c_main -->|"fuzz-smoke"| i_fuzzrun
    i_fuzzrun --- c_fuzz
    c_fuzz -->|"runs each target"| e_cargo
    e_cargo -->|"runs"| c_cfz
    e_cargo -->|"runs"| c_tfz
    c_fuzz -->|"reads the<br/>fuzz-result line"| i_summary
    c_fuzz -->|"checks rows<br/>and seeds"| f_corpus

    c_cfz -->|"json, toml"| i_target
    c_tfz -->|"encoding"| i_target
    c_more -.->|"planned M2.4,<br/>M2.8, M4.8,<br/>M5.8, M6.1,<br/>M6.5, M8.1,<br/>M10.7, M10.2"| i_target
    c_cfz -->|"fuzzes"| i_cparse
    c_tfz -->|"fuzzes"| i_tdec

    i_target --- c_engine
    i_summary --- c_engine
    c_engine -->|"mutations"| i_pcg
    i_pcg --- c_rng
    c_engine -->|"reads seeds,<br/>saves findings"| f_corpus
    i_cparse --- c_codec
    i_tdec --- c_text

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef external fill:#8a8a8a,stroke:#6b6b6b,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    classDef store fill:#1168bd,stroke:#0b4884,color:#ffffff
    class c_main,c_fuzz,c_cfz,c_tfz,c_engine,c_rng,c_codec,c_text built
    class i_fuzzrun,i_target,i_summary,i_pcg,i_cparse,i_tdec built
    class c_more planned
    class e_cargo external
    class f_corpus store
```

- A `fuzz-smoke` target is a test in its crate's `tests/fuzz.rs`, seeds in
  `fuzz/corpus/<target>/` and a row in `fuzz.rs`'s table (ADR-0027).

### 6.5 The property harness, bench and testkit

`core`'s property harness, its `Pcg32` and its fuzz engine (6.4) are the
only code from `core` that the tests of `codec` and `text` use, and neither
library uses `core`. `xtask` also uses `core`'s `cli`, `hash` and
`testing::fuzz` in its own code (6.3).

```mermaid
flowchart TB
    c_ctests["codec tests<br/>json.rs, toml.rs"]
    c_ttests["text tests<br/>model, positions, edits,<br/>unit tests"]
    c_bench["tidepool-bench<br/>an empty library<br/>harness and text suites<br/>planned M1.9"]

    i_check(("testing: check,<br/>check_sized"))
    i_pcg(("rng: Pcg32"))
    i_cparse(("json::parse_bytes,<br/>toml::parse_bytes"))
    i_tall(("Rope, ChangeSet,<br/>Selection, History"))

    subgraph sg_core["tidepool-core"]
        c_testing["testing<br/>the property harness, M0.2"]
        c_rng["rng, M0.2"]
    end
    c_codec["tidepool-codec library"]
    c_text["tidepool-text library"]

    c_testkit["tidepool-testkit<br/>an empty library<br/>planned M5.1, M6.1"]
    i_fakes(("fake tools,<br/>fake LSP server<br/>planned M5.1, M6.1"))

    c_ctests --> i_check
    c_ctests --> i_pcg
    c_ctests -->|"tests"| i_cparse
    c_ttests --> i_check
    c_ttests --> i_pcg
    c_ttests -->|"against<br/>String models"| i_tall
    c_bench -.->|"text suites,<br/>planned M1.9"| i_tall

    i_check --- c_testing
    c_testing -->|"seeded cases"| i_pcg
    i_pcg --- c_rng
    i_cparse --- c_codec
    i_tall --- c_text
    i_fakes -.- c_testkit

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    class c_ctests,c_ttests,c_testing,c_rng,c_codec,c_text built
    class i_check,i_pcg,i_cparse,i_tall built
    class c_bench,c_testkit,i_fakes planned
```

- `testkit`'s fakes have no arrow into them: the matrix lets only
  `tidepool-bench` use `tidepool-testkit`, and M5.1 settles the edges
  before the first fake tool (`docs/04-quality.md` → `layers`).

## Sources

Docs:

- `docs/01-architecture.md`: layers, the dependency matrix, the main loop.
- `docs/subsystems/core.md`, `text-and-formats.md`, `sys-and-platform.md`,
  `rendering.md`, `ui-and-theme.md`, `frontends.md`, `editor.md`,
  `languages.md`, `toolchains.md`, `mobile.md`, `agents.md`.
- `docs/04-quality.md`: own harnesses, fakes, fuzz smoke, `xtask` checks.
- `docs/roadmap.md`: every task ID drawn; `docs/decisions.md`: ADR-0002,
  ADR-0017, ADR-0027, ADR-0028, ADR-0029.
- `AGENTS.md` (workspace map) and each member's `README.md` (`## Status`).

Code:

- Manifests: `crates/*/Cargo.toml`, `xtask/Cargo.toml`, `bench/Cargo.toml`.
- The crate roots: `crates/sys/src/lib.rs`, `crates/platform/src/lib.rs`,
  `crates/app/src/main.rs`.
- `crates/codec/src/`: `lib.rs`, `limits.rs`, `error.rs`, `json.rs`,
  `json/parse.rs`, `json/write.rs`, `toml.rs`, `toml/parse.rs`,
  `toml/table.rs`.
- `crates/text/src/`: `lib.rs`, `rope.rs`, `pos.rs`, `pos/grapheme.rs`,
  `selection.rs`, `transaction.rs`, `history.rs`, `encoding.rs`, `file.rs`.
- `crates/core/src/`: `lib.rs`, `testing.rs`, `testing/fuzz.rs`, `hash.rs`,
  `cli.rs`, `rng.rs`.
- Tests: `crates/codec/tests/fuzz.rs`, `json.rs`, `toml.rs`;
  `crates/text/tests/fuzz.rs`, `model.rs`, `positions.rs`, `edits.rs`.
- `xtask/src/`: `main.rs`, `check.rs`, `cargo.rs`, `metadata.rs`,
  `matrix.rs`, `deps.rs`, `layers.rs`, `crates.rs`, `unsafe_audit.rs`,
  `headers.rs`, `workflows.rs`, `ucd.rs`, `fuzz.rs`.
