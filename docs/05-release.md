# 05 — Release

How Tidepool itself is built, packaged, signed and shipped. Building and
signing users' Android and iOS apps is a feature, covered in
`docs/subsystems/mobile.md`.

## Cargo profiles

```toml
[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
panic = "unwind"      # the panic hook writes a crash report and restores the terminal
debug = "line-tables-only"
strip = "debuginfo"   # full symbols archived separately per build

[profile.xtask]
inherits = "release"
debug = false
```

## Versioning

- SemVer. `0.x` until M14. The version lives in `[workspace.package]`.
- Settings, language packs, themes, tool descriptors and TAP each carry
  their own format version. A breaking change bumps it and ships a
  migration or a clear "not supported" error.

## Packaging (`cargo xtask package <os>`)

`xtask package` never signs with real credentials; signing happens only in
CI release jobs.

### Linux

- `tidepool-<version>-x86_64-linux.tar.gz`: the binary, `tidepool.desktop`,
  icons (PNG sizes 16–512), `LICENSE`, `LICENSES.txt` (fonts: SIL OFL 1.1;
  Unicode data: Unicode License v3, ADR-0018).
- AppImage is backlog.

### macOS

- `Tidepool.app` with `Info.plist`, `.icns` (built with `iconutil`), a
  universal binary (`lipo`), fonts in `Contents/Resources`.
- CI signs with a Developer ID Application certificate (`codesign
  --options runtime --timestamp`), notarizes with `notarytool`, staples, and
  builds a `.dmg` (`hdiutil`).

### Windows

- `tidepool-<version>-x86_64-windows.zip` with the binary, resources
  compiled with `rc.exe` (icon, version info, DPI manifest) and licenses.
- CI signs with `signtool` when a certificate is configured. An installer is
  backlog.

## Release pipeline (CI, on tag `v*`)

1. Run `cargo xtask ci` on all three OSes.
2. Package per OS.
3. Sign and notarize in protected environments. Secrets exist only in CI
   secret storage; the docs list their **names** only.
4. Smoke-test each package: `tidepool --smoke-test` opens a fixture
   workspace offscreen, edits, saves and exits 0.
5. Attach artifacts and SHA-256 sums to a draft GitHub release for a human
   to publish.

## Crash reporting

- The panic hook writes `crashes/<timestamp>.txt` in the user data directory
  (version, OS, backtrace, last 200 log lines with secrets redacted),
  restores the terminal in TUI mode, and tries to save unsaved buffers to
  `recovery/`.
- No crash upload in v1 (backlog, with a privacy review).

## Updates

- No auto-update in v1. Tidepool can tell the user a newer release exists
  only if they turn on the check, which runs through `curl` against the
  GitHub releases API.

## Release checklist (manual items)

- [ ] Every budget in `docs/00-overview.md` is met on the reference machine,
      or the exception has an ADR.
- [ ] Fuzz smoke clean for 30 minutes per target.
- [ ] A clean user account on each OS installs and runs Tidepool, and the
      Toolchains panel installs rust-analyzer and detects git.
- [ ] An Android app builds, signs and installs from the packaged build;
      an iOS app on macOS.
- [ ] Font license files and the Unicode license (ADR-0018) are included
      and unchanged.
- [ ] `git diff` of the release reviewed for secret material.
