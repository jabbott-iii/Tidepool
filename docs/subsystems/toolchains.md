# Subsystem: `tidepool-toolchain`, `tidepool-vcs` and `tidepool-term`

`toolchain` is the only crate that finds, installs, updates, configures and
launches outside programs (ADR-0002). `vcs` builds git features on the
managed `git`. `term` is the integrated terminal, which is also where
installs that need admin rights run, in front of the user.

## `tidepool-toolchain`

### Contents

| Module | Provides |
| :- | :- |
| `descriptor` | Loads `toolchains/*.toml`: id, purpose, platforms, detection, version rules, install/update/uninstall methods, checksums, license text location, dependent features |
| `detect` | Finds a tool: settings override → env vars (`ANDROID_HOME`, `JAVA_HOME`, `DEVELOPER_DIR`...) → standard locations → `PATH`. Runs its version command with a timeout and parses the output |
| `registry` | The live state of every tool: `Missing`, `Found { path, version }`, `Outdated`, `Installing`, `Broken { reason }`, plus which features each unlocks |
| `install` | Install and update plans: a list of steps (download, verify, run installer, accept license, run package manager) shown to the user before anything runs |
| `download` | HTTPS via a `curl` child process (`--proto =https --tlsv1.2 --fail --location`), to a temp file, then SHA-256 check against the descriptor before use (ADR-0016) |
| `process` | `Spawn` builder: program from the registry, argv vector, explicit environment, working dir, stdin/stdout/stderr pipes, timeout, cancellation, kill-on-drop, kill tree on shutdown |
| `secret` | `SecretArg` and `SecretEnv` types that redact themselves in logs and `Debug`, plus 0600 temp files in a 0700 directory removed on drop |
| `sessions` | Long-lived child processes (LSP servers, agents, `adb logcat`, emulators) with restart policy and backoff |

### Managed tools in v1

| Tool | Detect | Install | Update | Verify ready |
| :- | :- | :- | :- | :- |
| rustup | `rustup --version`; `~/.cargo/bin` | `rustup-init` (official URL, SHA-256), run as the user | `rustup self update` | `rustup show` |
| Rust toolchain | `rustc -vV`, `cargo -V` | `rustup toolchain install stable` (or the workspace's `rust-toolchain.toml`) | `rustup update` | `cargo -V` |
| rust-analyzer | `rust-analyzer --version` via `rustup which rust-analyzer` | `rustup component add rust-analyzer` | follows the toolchain | LSP `initialize` succeeds |
| git | `git --version` | Plan with the OS package-manager command (user presses Run in the terminal), or `xcode-select --install` on macOS, or `winget install --id Git.Git` on Windows | Same channel | `git --version` ≥ descriptor minimum |
| JDK | `JAVA_HOME`, Android Studio's bundled JBR, `java -version` | OS package-manager plan | Same channel | `javac -version`, `keytool -help` |
| Android cmdline-tools | `$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager` | Official zip via `download`, SHA-256 from the descriptor, unpacked to `$ANDROID_HOME/cmdline-tools/latest` | `sdkmanager --update` | `sdkmanager --version` |
| SDK packages | `sdkmanager --list_installed` | `sdkmanager "build-tools;…" "platform-tools" "platforms;android-…" "ndk;…" "emulator"` | `sdkmanager --update` | Files present and runnable |
| bundletool | Path in `toolchains.toml` | Official release jar via `download`, SHA-256 pinned | New descriptor version | `java -jar bundletool.jar version` |
| Xcode (macOS) | `xcode-select -p`, `xcodebuild -version` | Opens the App Store page; offers `xcode-select --install` for the CLT | Apple's channel | `xcodebuild -checkFirstLaunchStatus`, license status |

Descriptor versions and checksums are updated by a reviewed change, never
at runtime. `cargo xtask tools` checks every descriptor.

### Rules

- **Consent first.** Every install or update shows its plan (steps,
  sources, sizes, licenses) and runs only when the user confirms.
- **Licenses are accepted by the user.** `sdkmanager --licenses` and the
  Xcode license run in the integrated terminal for the user to read and
  answer. Tidepool never pipes `yes` into a license prompt.
- **No silent elevation.** Commands that need `sudo` or an admin prompt run
  in the integrated terminal, visibly, only when the user presses Run.
- **Verify before use.** A download that fails its SHA-256 is deleted and
  reported; nothing from it runs.
- **Standard locations.** Install where the tool's own installer would, so
  other tools share the install. Never move or delete a tool Tidepool did
  not install.
- **Respect the workspace.** A `rust-toolchain.toml` or Gradle wrapper in
  the workspace decides the version used there.
- **Untrusted workspaces** (ADR-0014) get detection only: no project-owned
  executables (Gradle wrapper, build scripts) run.
- **Process hygiene.** argv only, never a shell; explicit environment;
  output size caps; every child is killed on shutdown.

### Toolchains panel

- One card per tool: name, status dot, version, path, the features it
  unlocks, and actions (Install, Update, Locate…, Configure, Show log).
- A first-run checklist on the welcome screen: Rust, rust-analyzer, git,
  and, when the workspace looks like an Android or iOS project, the SDKs.

## `tidepool-vcs`

- All git access runs through `toolchain::process` with `git` from the
  registry and `-c core.quotepath=off`, `--no-pager`, `-z` output where
  available, and `GIT_TERMINAL_PROMPT=0` (credentials prompts go to the
  integrated terminal instead).
- Status: `git status --porcelain=v2 --branch -z`.
- Diffs: `git diff --no-color --no-ext-diff -U0` parsed into hunks for the
  gutter; full diffs for the diff view.
- Blame: `git blame --porcelain`; log: `git log -z --format=…`.
- Writes (stage, unstage, commit, branch switch, stash) happen only on an
  explicit user action in the UI. Commit runs `git commit` with the message
  on stdin (`-F -`), so the user's hooks and signing config apply.
- Parsers for porcelain v2, diff hunks and blame are fuzz targets.

## `tidepool-term`

- VT parser (ECMA-48 + xterm subset): C0/C1 controls, CSI (cursor, erase,
  scroll regions, SGR incl. truecolor), OSC 0/2 (title), 8 (hyperlinks),
  52 (clipboard, off by default), DEC private modes (alternate screen,
  bracketed paste, mouse 1000/1002/1006, application cursor keys), wide
  characters.
- Grid with scrollback (default 10,000 lines), reflow on resize, selection
  and search.
- PTY from `platform` (`openpty` family, ConPTY on Windows).
- **Full-screen programs work:** `vim`, `nvim`, `less`, `htop` and `git
  add -p` run correctly. A scripted session with each is a TUI-snapshot
  test.

## Tests

- Fake tools in `testkit` reproduce real version strings, `--list_installed`
  output, porcelain v2, error exits and slow responses.
- Descriptor tests: every descriptor parses, has an HTTPS source, a SHA-256
  for downloads, and a license entry (`cargo xtask tools`).
- Download tests: checksum mismatch deletes the file and runs nothing.
- Secret tests: no `SecretArg` value appears in any log line, `Debug`
  output or argv of a constructed `Command`.
- Real-tool tests: `TIDEPOOL_IT_RA=1` (rust-analyzer), `TIDEPOOL_IT_GIT=1`,
  `TIDEPOOL_IT_ANDROID=1`, `TIDEPOOL_IT_IOS=1`.
- VT parser conformance against recorded byte streams from real programs.
