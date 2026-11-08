# tidepool-core

Tidepool's foundations, which every other crate uses: errors with context,
logging with secret redaction, the property-test harness and the fuzzer
behind each parser crate's fuzz targets, a seeded RNG, hashing (SHA-256 for
download integrity only), argument parsing, UTC time, and the geometry,
color and input types that `platform`, `ui`, `editor` and both frontends
share. It has no dependencies, no `unsafe` and no OS-specific code.

Layer 0, foundations. It may use no workspace crate, as its row of the
dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Built, with the roadmap task that built it:

- `error`, `log`, `testing`, `rng`, `hash`, `cli`, `civil`, `input`, `geom`,
  `color` and `cancel`, as `docs/subsystems/core.md` → "Public API names"
  lists them — M0.2.
- `testing::fuzz`, the engine of the fuzz targets that each parser crate
  has in its `tests/fuzz.rs`: seeds and saved crashes replayed by every
  `cargo test`, mutations from a seed when `cargo xtask fuzz-smoke` asks,
  panics and hangs caught, the written memory (`VmHWM`) checked on Linux
  from `/proc`, and the reserved memory (`VmPeak`) too when
  `TIDEPOOL_FUZZ_SECS` is set, even to 0 (in the runs `fuzz-smoke` starts,
  and in the reproduce command of a saved reserved-memory crash, each one
  target to a process), findings saved whole to `fuzz/crashes/`, a replay
  that leaves the input a process died on for `fuzz-smoke` to keep as
  `died-<hash>`, and no symbolic link followed on the way. Ported in part
  from Nadir's fuzzer — M1.6.

Not yet, with the roadmap task that adds it:

- `path`: UTF-8 path helpers, canonicalization, and the display of paths
  that are not UTF-8 — M1.8.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Design

- [`docs/subsystems/core.md`](../../docs/subsystems/core.md)
- [`docs/04-quality.md`](../../docs/04-quality.md) ("Fuzz smoke"), which
  summarizes `testing::fuzz`; its rustdoc is authoritative.
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0016
  (SHA-256 for downloads only); ADR-0027 (the fuzz targets and their
  engine).

## Rules

- `unsafe` is denied by the workspace lint, and `cargo xtask unsafe-audit`
  checks.
- No OS-specific code: `testing::fuzz` learns it is not on Linux when
  `/proc/self/status` cannot be read, and skips the memory checks, saying
  so. Like the rest of the crate it never prints; it reports through its
  return values and panic messages.
- `tests/fuzz_memory.rs` holds the one test of the reserved-memory check,
  in a process of its own: the check sees the whole process, and the unit
  tests' threads would move it.
- Path-scoped rules in `.claude/rules/`:
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.

## Checks

```bash
cargo test -p tidepool-core
```

`cargo xtask ci` runs the whole gate.

## License

Apache-2.0, like the rest of Tidepool: see [`LICENSE`](../../LICENSE).
