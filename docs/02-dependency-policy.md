# 02 — Dependency policy

## The rule

> **Build it ourselves unless building it is unreasonable.** A third-party
> crate is used only when it passes the exception test below, the user has
> approved it, and it is recorded in the register in this document.

The approved set of third-party crates is **empty** (ADR-0001). `Cargo.lock`
must contain only workspace packages. `scripts/check-deps.sh` fails
otherwise, and `cargo xtask deps` replaces it in M0.3.

Outside programs are a separate category: they are **managed tools**
(ADR-0002). Tidepool installs, updates, configures and runs them as child
processes. They are never linked, bundled or reimplemented.

## What counts as a dependency

| Counts (needs approval) | Does not count |
| :- | :- |
| Any crate from crates.io or git in any `[dependencies]`, `[dev-dependencies]` or `[build-dependencies]` table | The Rust toolchain and `std` |
| Vendored or copied third-party source code, including "just one file" | OS libraries and frameworks we call through FFI (listed below) |
| A build script that downloads anything or runs third-party code | Managed tools run as child processes (listed below) |
| A git submodule containing code | Fonts and other assets with a compatible license (listed below) |
| A `cargo install`-ed tool that CI depends on | GitHub Actions used by CI, pinned by commit SHA (`docs/04-quality.md`) |
| Bundling an outside program's binary in Tidepool's download (ADR-0002) | Code the owner wrote in another repository (such as Nadir), ported in with a note in the change |

Reading a public specification (LSP, DAP, CommonMark, TrueType, ZIP,
DEFLATE, PNG, QOI, TOML, JSON, VT/ECMA-48, the APK and AAB formats) and
implementing it ourselves is **not** a dependency. That is the intended path.

Depending on another repository's crates across repositories (for example,
a path or git dependency on Nadir's `codec`) **does** count. Port the code
instead.

### OS libraries we rely on (not dependencies, but listed for audit)

| OS | Libraries / frameworks | How |
| :- | :- | :- |
| Linux | `libc` (glibc), `libX11.so.6` (ADR-0010); later `libXi.so.6`, `libXcursor.so.1` | glibc linked; everything else loaded with `dlopen`, so a missing library gives a clear error |
| macOS | `libobjc`, `Foundation`, `AppKit`, `CoreFoundation`, `CoreGraphics`, `QuartzCore`, `CoreServices` (FSEvents) | `#[link(name = "...", kind = "framework")]` in `tidepool-sys` |
| Windows | `kernel32`, `user32`, `gdi32`, `shell32`, `ole32`, `imm32` (later) | Linked import libraries from the MSVC toolchain |

### Managed tools (ADR-0002)

Tidepool finds, installs, updates and drives these through their official
channels. Each has a descriptor in `toolchains/` (version, source, checksum,
license) and a row in the Toolchains panel. Details:
`docs/subsystems/toolchains.md`.

| Tool | Used for | Installed and updated through | Milestone |
| :- | :- | :- | :- |
| rustup, `cargo`, `rustc`, `rustfmt`, `clippy` | Rust builds, tests, lints | rustup (official installer, with consent) | M5 |
| rust-analyzer | Rust language server | `rustup component add rust-analyzer` | M5 |
| git | Version control | OS package manager or the official installer (user-run when it needs admin) | M5 |
| `curl` | HTTPS downloads for installers and the reference Claude agent | Ships with the OS (Linux distros, macOS, Windows 10+); otherwise the OS package manager | M5 |
| JDK (`java`, `javac`, `keytool`, `jarsigner`) | Android builds and signing | OS package manager, or an existing Android Studio JBR | M10 |
| Android SDK command-line tools (`sdkmanager`, `avdmanager`) | Installing the rest of the SDK | Official download, SHA-256 checked; licenses accepted by the user via `sdkmanager --licenses` | M10 |
| SDK build-tools (`aapt2`, `d8`, `zipalign`, `apksigner`), platform-tools (`adb`), platforms, NDK, emulator | Android build, sign, install, run | `sdkmanager` | M10 |
| `bundletool` | Building and testing `.aab` files | Official release jar, pinned version and SHA-256 | M10 |
| Gradle wrapper (`gradlew`) | Gradle-project builds | Provided by the user's project; runs only in trusted workspaces | M10 |
| Xcode, Command Line Tools (`xcodebuild`, `xcrun`, `codesign`, `security`, `simctl`) | iOS build, sign, run (macOS only) | Apple: the App Store or developer download; `xcode-select --install` for the CLT | M11 |
| `ssh`, `rsync` | Remote-Mac iOS builds | Ships with the OS, or the OS package manager | M11 |
| `rcodesign` | Optional Apple re-signing on non-macOS hosts | Official release, pinned version and SHA-256 | M11 (optional) |
| Debug adapters (`lldb-dap` or CodeLLDB) | Debugging | rustup/LLVM or the adapter's official release | M13 |
| Other LSP servers (per language pack) | Semantic features for other languages | Each server's official channel | M13 |
| Language agents | AI features | Installed by the user; Tidepool runs them per their manifest | M9 |

### Assets

| Asset | License | Source | Use |
| :- | :- | :- | :- |
| Saira (static Regular, Medium, SemiBold, Bold `.ttf`) | SIL OFL 1.1 (verify the file's license on import) | Google Fonts / the Saira project | UI chrome, matching jabbott.dev (ADR-0004) |
| JetBrains Mono (Regular, Italic, SemiBold `.ttf`) | SIL OFL 1.1 (verify on import) | JetBrains Mono releases | Code and terminal (ADR-0004) |
| CommonMark spec examples | CC-BY-SA 4.0 (verify) | commonmark.org | Test-only fixtures, never in release binaries (M6.5) |

## The exception test

Before proposing any crate, first try to **change the design so the need
disappears**:

- choose a simpler format or subset (static fonts instead of variable fonts)
- cut scope (no complex shaping in v1)
- move the work into a managed tool (HTTPS through `curl`, signing through
  `apksigner`/`jarsigner`/`codesign`, git through the `git` CLI)

If the need remains, a crate may be proposed only when at least one of these
is true:

1. **Disproportionate cost.** Building it is estimated above 4
   engineer-weeks, the capability is not central to the IDE, and no design
   change or managed tool avoids it.
2. **Specialist correctness with severe failure modes.** The domain demands
   expert knowledge, bugs are hard to detect by testing, and the
   consequences are severe. Examples: TLS, signing cryptography. Tidepool's
   current design avoids both (ADR-0007, ADR-0009, ADR-0016).
3. **Dev-only tooling with no reasonable equivalent.** It never ships in the
   Tidepool binary, and our own version would be far weaker. Example:
   coverage-guided fuzzing.

"It's popular" and "it would be faster to start" are not reasons.

## Exception process

1. Run `/evaluate-dependency <crate> <need>` and write the evaluation. It
   covers: the need and design changes considered; a build-it-ourselves
   estimate; whether a managed tool could do it; the crate's license,
   maintainers and release cadence; its transitive dependency count and
   `unsafe` use; its binary-size impact; and whether it ships.
2. The **user** approves or rejects it. Agents never approve dependencies.
3. If approved:
   - Add a row to the register with status `Approved` and an ADR reference.
   - Add the exact pin to `xtask/deps-allowlist.txt` (`name version ADR-NNNN`).
   - Use an exact version requirement (`= x.y.z`).
   - Wrap the crate behind one module or crate boundary of ours.
4. Review approved dependencies on every upgrade. Upgrades are their own
   change.

## Build-vs-buy register

**Status** is one of: **Build** (we write it), **Managed** (a managed tool
does it, ADR-0002), **Proposed** (awaiting user decision), **Approved**
(user said yes, ADR recorded), **Rejected**.

The **fallback** column is a crate we would evaluate if the **switch
trigger** fires. It is not approved.

| # | Capability | Status | Our approach | Fallback (not approved) | Switch trigger |
| :- | :- | :- | :- | :- | :- |
| 1 | Win32 bindings | Build | Hand-written in `tidepool-sys::windows`, only what we call, with layout tests | `windows-sys` | More than about 1,500 declarations, or more than 2 layout bugs reach `main` in one milestone |
| 2 | Objective-C runtime glue | Build | Own `msg_send!` with exact fn-pointer casts (the pattern Nadir uses) | `objc2` | More than 2 ABI bugs in one milestone |
| 3 | X11 bindings | Build | Hand-written, `dlopen`-loaded symbol table (ADR-0010) | `x11-dl` | Same as #1 |
| 4 | Windowing and event loop | Build | `tidepool-platform`, one module per OS | `winit` | Not expected |
| 5 | 2D rasterization | Build | CPU rasterizer in `tidepool-gfx` (ADR-0011) | `tiny-skia` | Frame budget missed at 1440p after profiling |
| 6 | Font parsing and rasterization | Build | TrueType `glyf` outlines, grayscale coverage, glyph atlas in `tidepool-font` | `ttf-parser` + `fontdue` | Rendering defects on the bundled fonts not fixed in 2 weeks |
| 7 | Text shaping (complex scripts) | Build (deferred) | Kerning and simple ligature-free layout in v1 | `rustybuzz` | A user need for Arabic, Indic or other shaped scripts |
| 8 | Rope / text buffer | Build | B-tree rope in `tidepool-text` | `ropey` | Not expected |
| 9 | Regex | Build | Linear-time Pike VM in `tidepool-regex` | `regex` | Missing features block language packs for 3+ languages |
| 10 | Syntax highlighting | Build | Hand-written Rust and Markdown lexers; TOML language packs | `tree-sitter` | Not expected (also a C dependency) |
| 11 | Markdown | Build | CommonMark + GFM parser in `tidepool-markdown` | `pulldown-cmark` | Not expected |
| 12 | JSON / TOML / XML / plist / YAML / CSV | Build | `tidepool-codec`, with limits and fuzzing | `serde` family | Not expected |
| 13 | LSP / DAP types and JSON-RPC | Build | `tidepool-rpc` over `codec::json` | `lsp-types`, `lsp-server` | Not expected |
| 14 | git | Managed | `git` CLI with porcelain v2 output (ADR-0002) | `gix` | Not expected |
| 15 | Terminal emulation | Build | VT parser and grid in `tidepool-term` | `vte`, `alacritty_terminal` | Not expected |
| 16 | PTY | Build | `openpty`/`forkpty`, ConPTY in `tidepool-platform` | `portable-pty` | Not expected |
| 17 | File watching | Build | inotify, FSEvents, ReadDirectoryChangesW in `tidepool-platform` | `notify` | Not expected |
| 18 | DEFLATE / ZIP / PNG / QOI | Build | `tidepool-codec` (Nadir has inflate, PNG and QOI to port) | `miniz_oxide`, `zip`, `png` | Not expected |
| 19 | SHA-256 (download integrity) | Build | FIPS 180-4, test vectors, in `tidepool-core` (ADR-0016) | `sha2` | Not expected |
| 20 | HTTP and TLS | Managed | `curl` child process; the core binary has no sockets (ADR-0009) | `ureq` + `rustls` | Not expected |
| 21 | APK / AAB signing | Managed | `apksigner`, `jarsigner` (ADR-0007) | — | — |
| 22 | Apple code signing | Managed | `codesign` on macOS; optional `rcodesign` (ADR-0007, ADR-0008) | — | — |
| 23 | Vim emulation | Build | Modal keymap engine in `tidepool-editor` (ADR-0006) | — | — |
| 24 | Clipboard | Build | X11 selections, `NSPasteboard`, Win32 clipboard; OSC 52 in the TUI | `arboard` | Not expected |
| 25 | Accessibility bridges | Build (M14) | AT-SPI, NSAccessibility, UI Automation from the `ui` accessibility tree | `accesskit` | The M14 estimate exceeds 6 engineer-weeks |
| 26 | Async runtime | Not needed | `std` threads and channels (ADR-0012) | `tokio` | Not expected |
| 27 | Property testing | Build | `tidepool-core::testing` (`check`, `check_sized`) | `proptest` | Not expected |
| 28 | Benchmarking | Build | `tidepool-bench`: warm-up, N samples, median/p95, JSON baseline | `criterion` | Not expected |
| 29 | Fuzzing (stable, CI) | Build | Mutation fuzz smoke in `xtask` | — | — |
| 30 | Coverage-guided fuzzing (optional, nightly) | **Proposed** | `cargo fuzz` with `libfuzzer-sys` in an excluded `fuzz/` workspace, dev-only | — | User decision in M14 (criterion 3) |
| 31 | CLI parsing, logging, errors, time | Build | `tidepool-core::{cli, log, error, civil}` | `clap`, `tracing`, `thiserror`, `chrono` | Not expected |

## Enforcement

- **`cargo xtask deps`** (M0.3) reads `cargo metadata --format-version 1`
  with our own JSON parser. It fails if any package has a non-null `source`
  that is not in `xtask/deps-allowlist.txt`, and on the other rules in
  `docs/04-quality.md` → "`xtask` checks".
- **Claude Code hook** `.claude/hooks/deps-guard.sh` runs `xtask deps`,
  then `xtask layers`, after edits to a `Cargo.toml`, a `Cargo.lock` or a
  cargo configuration file. If `xtask` cannot be built, it falls back to
  `sh scripts/check-deps.sh`, which resolves every workspace offline from
  its lockfile and fails closed if any package has a non-path source. The
  hook still exits 2 then, because the fallback is incomplete. While the
  root lockfile is out of date, it checks only what the root manifests
  declare, says how to update the lockfile offline, and exits 2
  (`docs/04-quality.md` → "Command line and rules").
- **`cargo xtask layers`** checks the workspace graph against the matrix in
  `docs/01-architecture.md`.
- **`cargo xtask tools`** (not yet implemented: roadmap M5.1; until then it
  exits 1) will check that every descriptor in `toolchains/` has a pinned
  version, an official HTTPS source, a SHA-256 where a download is
  involved, and a license entry.
- **Path-scoped rule** `.claude/rules/dependencies.md` loads when an agent
  opens a Cargo manifest.
- **Claude Code permissions** ask before `cargo add`, `cargo remove`,
  `cargo update` and `cargo install`.
