# tidepool-mobile

Building, signing, verifying and installing Android and iOS apps, always
through the platforms' official tools, and an inspector for APK, AAB and IPA
files.

It is part of [Tidepool](https://github.com/jabbott-iii/Tidepool), where
`editor`'s mobile panels use it; it drives the official tools through
`tidepool-toolchain`.

Layer 3, language and tool features. It may use `core`, `codec` and
`toolchain`, as its row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Version 0.1, not published (`publish = false`): the API changes as the
roadmap needs.

Nothing yet: besides what every library carries (ADR-0028, ADR-0036),
the crate is only its doc comment, which names its spec.

Not yet, with the roadmap task that adds it:

- `inspect` — M10.2.
- Android signing — M10.3.
- Gradle projects — M10.4.
- Gradle-free apps — M10.5.
- devices and logcat — M10.6.
- iOS: Xcode projects, Xcode-free apps, remote Macs — M11.3, M11.4, M11.5.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Usage

Nothing to use yet: `inspect` lands in M10.2.

## Documentation

- Design: [`docs/subsystems/mobile.md`](../../docs/subsystems/mobile.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0007
  (signing is delegated, Proposed); ADR-0008 (iOS on macOS, Proposed).
- API reference: `cargo doc -p tidepool-mobile --open`

## Rules

- `unsafe` is denied by the workspace lint, and in doc tests by the crate
  root's `#![doc(test(attr(forbid(unsafe_code))))]` (ADR-0036);
  `cargo xtask unsafe-audit` checks both.
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

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
