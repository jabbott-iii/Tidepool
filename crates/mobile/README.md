# tidepool-mobile

Building, signing, verifying and installing Android and iOS apps, always
through the platforms' official tools, and an inspector for APK, AAB and IPA
files.

Layer 3, language and tool features. It may use `core`, `codec` and
`toolchain`, as its row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Nothing yet: the crate is only its doc comment, which names its spec.

Not yet, with the roadmap task that adds it:

- `inspect` — M10.2.
- Android signing — M10.3.
- Gradle projects — M10.4.
- Gradle-free apps — M10.5.
- devices and logcat — M10.6.
- iOS: Xcode projects, Xcode-free apps, remote Macs — M11.3, M11.4, M11.5.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Design

- [`docs/subsystems/mobile.md`](../../docs/subsystems/mobile.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0007
  (signing is delegated, Proposed); ADR-0008 (iOS on macOS, Proposed).

## Rules

- `unsafe` is denied by the workspace lint, and `cargo xtask unsafe-audit`
  checks.
- Path-scoped rules in `.claude/rules/`:
  [`toolchains-and-secrets.md`](../../.claude/rules/toolchains-and-secrets.md),
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.
- Every change goes to the `security-reviewer` subagent.

## Checks

```bash
cargo test -p tidepool-mobile
```

`cargo xtask ci` runs the whole gate.

## License

Apache-2.0, like the rest of Tidepool: see [`LICENSE`](../../LICENSE).
