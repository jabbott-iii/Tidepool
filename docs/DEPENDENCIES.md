# Dependency Policy and Ledger

**Status:** Proposed (Decision D1 in [`PLAN.md`](PLAN.md)). Becomes binding once accepted as an ADR.

Tidepool depends on nothing beyond what is strictly necessary to run on an operating system.
This file is the single source of truth for what that means and for every non-`std` thing the
project uses. **Any change to what Tidepool depends on updates this ledger in the same change.**

Applies to everything in this repository: `crates/`, `agents/`, tests, benches, examples, and
build tooling. It does **not** apply to user projects that Tidepool edits or builds.

## Tiers

### Tier 0 — Rust toolchain (always allowed)

- `core`, `alloc`, `std` (and `proc_macro` only for an in-repo proc-macro crate, which itself
  needs an ADR).
- Toolchain components installed through rustup: `rustc`, `cargo`, `rustfmt`, `clippy`,
  `rust-analyzer`, `miri`.

### Tier 1 — Operating-system interfaces (allowed, with rules)

Hand-written FFI to libraries and frameworks that ship with the operating system, plus OS wire
protocols spoken directly.

- All bindings live in `crates/tp-sys`. No `-sys` crates, no `bindgen`, no `build.rs` that
  compiles C.
- Every `unsafe` block has a `// SAFETY:` comment; `tp-sys` exposes safe wrappers only.
- Each newly linked library or framework gets a ledger row below before merge.

### Tier 2 — External programs (allowed, optional at runtime)

Programs the user installs, discovered at runtime and run as subprocesses with an argv vector
(never through a shell). Never linked, never bundled, never downloaded by Tidepool without the
user's explicit action. A missing tool disables only the feature that needs it, with a
diagnostic that names the tool and how it was searched for.

### Tier 3 — Third-party Rust crates (forbidden by default)

Includes normal, dev-, build-, and target-specific dependencies, proc macros, git dependencies,
alternate registries, `[patch]`, and vendored copies of third-party crate source.

An exception requires all of:
1. An ADR (`/adr`) explaining the need, the alternatives considered (including writing it
   in-house and using a Tier 2 program), and the maintenance and security cost;
2. A security review (`security-reviewer` agent) of the crate and its transitive tree;
3. Explicit owner approval recorded in the ADR;
4. An exact version pin and a ledger row below.

### Assets (allowed, with license review)

Fonts, icons, test corpora, and similar non-code files are allowed when their license is
compatible with distributing Tidepool under Apache-2.0. Record license and source below.
Test-only assets must not be embedded in release binaries.

## Enforcement

- `scripts/check-deps.sh` resolves every Cargo workspace offline and fails if any package has a
  non-path source. It fails closed (resolution errors count as failures).
- `.claude/settings.json` denies `cargo add` and runs the check through a hook after edits to
  any `Cargo.toml` or `Cargo.lock`.
- `scripts/ci.sh` (M0) runs the check with the rest of the verification suite.
- Review: the `architect` and `security-reviewer` agents check new Tier 1 and Tier 2 entries.

## Ledger

### Tier 1 — OS interfaces

| Interface | Platform | Used for | Crate | Milestone | Status |
| --- | --- | --- | --- | --- | --- |
| libc (termios, `poll`, `ioctl`, signals) | Linux, macOS | TUI raw mode, input, resize | `tp-sys` | M2 | Planned |
| libc (`openpty`/`forkpty`) | Linux, macOS | Integrated terminal | `tp-sys` | M4 | Planned |
| inotify | Linux | File watching | `tp-sys` | M4 | Planned |
| FSEvents / CoreServices | macOS | File watching | `tp-sys` | M4 | Planned |
| kernel32 (console, ConPTY, ReadDirectoryChangesW) | Windows | TUI, terminal, watching | `tp-sys` | M4 | Planned |
| X11 protocol over Unix socket (no Xlib) | Linux | GUI window, input, clipboard | `tp-gui` (std only) | M8 | Planned (D3) |
| libobjc, AppKit, CoreFoundation, CoreGraphics | macOS | GUI | `tp-sys` | M8 | Planned |
| user32, gdi32 | Windows | GUI | `tp-sys` | M8 | Planned |

### Tier 2 — External programs

| Program | Used for | Milestone | Status |
| --- | --- | --- | --- |
| `cargo`, `rustc` (+ `rustfmt`, `clippy` via cargo) | Rust build/test/run/lint tasks | M3 | Planned |
| `rust-analyzer` | Rust language server | M3 | Planned |
| `git` | Version-control features | M4 | Planned |
| User's shell (`$SHELL`, `cmd`/PowerShell) | Integrated terminal | M4 | Planned |
| `curl` | HTTPS for the reference Claude agent (`agents/`) | M5 | Planned (D8) |
| Local model server (user-run, loopback HTTP) | Reference local agent | M5 | Planned |
| JDK: `keytool`, `jarsigner`, `javac` | Android keystores, AAB signing, Java compile | M6 | Planned |
| Android SDK build-tools: `aapt2`, `d8`, `zipalign`, `apksigner` | Android resources, dex, alignment, APK signing | M6 | Planned |
| Android platform `android.jar` | Compile/link target | M6 | Planned |
| Android NDK (clang toolchain) | Linking Rust/C native code | M6 | Planned |
| `bundletool` (jar) | Building AABs, APK sets for testing | M6 | Planned |
| `adb` | Install and run on devices/emulators | M6 | Planned |
| Gradle wrapper (`gradlew`, project-provided) | Gradle-project builds | M6 | Planned |
| Xcode: `xcodebuild`, `xcrun`, `codesign`, `security`, `simctl` | iOS build, sign, verify, simulator | M7 | Planned |
| `ssh`, `rsync` | Remote-Mac iOS builds | M7 | Planned (D7) |
| `rcodesign` | Optional Apple re-signing on non-macOS hosts | M7 | Optional (D7) |
| DAP adapter (e.g. `lldb-dap`, CodeLLDB) | Debugging | M9 | Planned |
| Other LSP servers (per language pack) | Semantic features for other languages | M9 | Planned |

### Assets

| Asset | License | Source | Use | Milestone | Status |
| --- | --- | --- | --- | --- | --- |
| Monospace font (TBD) | SIL OFL 1.1 (expected) | TBD | GUI default font | M8 | Pending D4 |
| CommonMark spec examples | CC-BY-SA 4.0 (verify) | commonmark.org spec | Test-only fixtures | M3 | Pending D12 |

### Tier 3 — Approved exceptions

None.
