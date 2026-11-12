# tidepool-platform

Safe OS behavior on top of `tidepool-sys`: windows, the event loop and its
waker, presenting a framebuffer, the clipboard, terminal raw mode, PTYs,
processes, file watching and the per-OS paths. No `target_os` cfg shows in
its public API.

It is part of [Tidepool](https://github.com/jabbott-iii/Tidepool), where it
is the only crate that uses `tidepool-sys`: `toolchain`, `term`,
`workspace`, both frontends and the binary reach the OS through it.

Layer 1, OS behavior, formats and pure engines. It may use `core` and `sys`,
as its row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Version 0.1, not published (`publish = false`): the API changes as the
roadmap needs.

Built, with the roadmap task that built it:

- the crate root, which allows `unsafe_code` — M0.1.

Not yet, with the roadmap task that adds it:

- `window`, `events` with the `Waker`, and `surface` on Linux (X11) — M2.2.
- `clipboard`, `paths` and `dirs` on Linux — M2.7.
- `watch` (inotify) — M4.5.
- `process` on Linux — M5.7.
- `pty` on Linux — M5.9.
- `terminal`, the raw-mode guard for the TUI — M7.1.
- macOS — M11.7, M11.8, M11.1.
- Windows — M12.1, M12.2.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Usage

Nothing to use yet: `window`, `events` with the `Waker`, and `surface` land
in M2.2.

## Documentation

- Design:
  [`docs/subsystems/sys-and-platform.md`](../../docs/subsystems/sys-and-platform.md)
- Platforms: [`docs/03-platforms.md`](../../docs/03-platforms.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0010 (X11
  through `dlopen`); ADR-0012 (the main loop, Proposed).
- API reference: `cargo doc -p tidepool-platform --open`

## Rules

- `unsafe` is allowed here: this is one of the two crates that may hold it
  (`AGENTS.md` rule 4). Each block carries a `// SAFETY:` comment, and each
  `pub unsafe fn` a `# Safety` section.
- Path-scoped rules in `.claude/rules/`:
  [`unsafe-and-ffi.md`](../../.claude/rules/unsafe-and-ffi.md),
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.
- Every change here goes to the `security-reviewer` subagent.

## Checks

```bash
cargo test -p tidepool-platform    # with `xvfb-run` from M2.2
cargo xtask unsafe-audit
```

`cargo xtask ci` runs the whole gate.

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
