# tidepool-sys

Hand-written raw FFI declarations for the OS APIs Tidepool calls: libc, X11,
the Objective-C runtime and AppKit, Win32 and ConPTY. It declares and does
nothing else; `tidepool-platform` wraps every declaration in a safe API, and
no other crate uses it.

Layer 0, foundations. It may use no workspace crate, as its row of the
dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Built, with the roadmap task that built it:

- the crate root, the one place besides `platform`'s that allows
  `unsafe_code` — M0.1.

Not yet, with the roadmap task that adds it:

- `dynlib!`, `linux::libc` and `linux::x11` — M2.1.
- the declarations for processes and the PTY — M5.7, M5.9.
- macOS — M11.7, M11.8.
- Windows — M12.1, M12.2.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Design

- [`docs/subsystems/sys-and-platform.md`](../../docs/subsystems/sys-and-platform.md)
- [`docs/03-platforms.md`](../../docs/03-platforms.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0001 (no
  binding crates); ADR-0010 (X11 through `dlopen`, Proposed).

## Rules

- `unsafe` is allowed here: this is one of the two crates that may hold it
  (`AGENTS.md` rule 4). Each block carries a `// SAFETY:` comment, and each
  `pub unsafe fn` a `# Safety` section.
- Path-scoped rules in `.claude/rules/`:
  [`unsafe-and-ffi.md`](../../.claude/rules/unsafe-and-ffi.md),
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.
- Declare only what is called, cite the header for each item, and assert
  every struct's layout: follow `/add-ffi-binding`.

## Checks

```bash
cargo build -p tidepool-sys
cargo xtask unsafe-audit
```

`cargo xtask ci` runs the whole gate.

## License

Apache-2.0, like the rest of Tidepool: see [`LICENSE`](../../LICENSE).
