# tidepool-toolchain

The one crate that finds, installs, updates and starts outside programs:
tool descriptors, detection, consented install plans, SHA-256-checked
downloads through `curl`, process supervision and secrets that never reach a
command line or a log.

Layer 2, services. It may use `core`, `platform` and `codec`, as its row of
the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Nothing yet: the crate is only its doc comment, which names its spec.

Not yet, with the roadmap task that adds it:

- `descriptor`, `detect`, `registry` and `cargo xtask tools` — M5.1.
- `process`, `sessions` and `secret` — M5.2.
- `download` and `install` — M5.3.
- the Rust toolchain, rust-analyzer and git — M5.5.
- the JDK, the Android SDK and bundletool — M10.1.
- Xcode — M11.2.
- Windows install plans — M12.3.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Design

- [`docs/subsystems/toolchains.md`](../../docs/subsystems/toolchains.md)
- [`docs/02-dependency-policy.md`](../../docs/02-dependency-policy.md)
  ("Managed tools")
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0002
  (managed tools); ADR-0016 (download checksums); ADR-0017 (only `xtask`
  runs `cargo` outside this crate).

## Rules

- `unsafe` is denied by the workspace lint, and `cargo xtask unsafe-audit`
  checks.
- Path-scoped rules in `.claude/rules/`:
  [`toolchains-and-secrets.md`](../../.claude/rules/toolchains-and-secrets.md),
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.
- Add or change a tool with `/add-managed-tool`. Every change goes to the
  `security-reviewer` subagent.

## Checks

```bash
cargo test -p tidepool-toolchain
cargo xtask tools    # from M5.1
```

`cargo xtask ci` runs the whole gate.

## License

Apache-2.0, like the rest of Tidepool: see [`LICENSE`](../../LICENSE).
