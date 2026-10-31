# Subsystem: `tidepool-core`

Foundation types used by every other crate. No `unsafe`, no OS calls, no
dependencies. Much of it can be ported from Nadir's `nadir-core` (same
owner); port the code, don't depend on the crate.

## Contents

| Module | Provides |
| :- | :- |
| `error` | `CoreError` and the `ErrorContext` helper for "while doing X" context |
| `log` | Macros `error!`, `warn!`, `info!`, `debug!`, `trace!` with a module target; sinks: stderr, rotating file, in-memory ring buffer (last 1,000 lines, for the Output panel and crash reports); redaction hook for secrets |
| `testing` | Property-test harness (`check`, `check_sized`), `assert_approx!` |
| `rng` | `Pcg32` (seedable), for tests and jitter in restart backoff |
| `hash` | `fnv1a64`, a deterministic non-cryptographic hasher with `FastHashMap`/`FastHashSet`, `crc32` (IEEE), and `sha256` for download integrity only (ADR-0016) |
| `cli` | Minimal argument parser (`--flag`, `--key value`, positional) for `tidepool`, `xtask` and `agents/` |
| `civil` | UTC date/time from `SystemTime` for log, crash and session file names |
| `input` | `Key` (physical keys), `Modifiers`, `MouseButton`, `InputEvent`, shared by `platform`, `ui`, `editor` and both frontends |
| `geom` | `Point`, `Size`, `Rect` in logical units (`f32`) and device pixels (`i32`), scale conversion |
| `color` | `Color` (sRGB `u8` RGBA), premultiplied blending, hex parsing (`#5ccbcb`, `rgba(…)` for theme files) |
| `path` | UTF-8 path helpers, canonicalization, display of non-UTF-8 paths |
| `cancel` | `CancelToken` (`Arc<AtomicBool>`) and `Deadline` |

## Invariants

- `sha256` matches the NIST FIPS 180-4 test vectors (empty string, `abc`,
  the 448-bit and 896-bit messages, one million `a`s). It is used only to
  compare a download against a pinned digest.
- `crc32("123456789") == 0xCBF43926`.
- `Pcg32` output is identical across OSes for the same seed.
- Log redaction removes every registered secret from every sink before it
  is written.

## Tests

- Property tests for `geom` conversions and `color` blending.
- SHA-256 and CRC-32 test vectors.
- Logger rotation in a temp directory under `target/`.
- `civil` against known timestamps, including leap years.
