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

## Public API names

Named in M0.2, where the table above leaves them open. Rustdoc on each item
is the reference.

- `error`: `CoreError`; `ErrorContext::{context, with_context}` on any
  `Result`, giving `WithContext<E>`, which displays as
  `while <what>: <source>` and returns the wrapped error from `source()`.
- `log`:
  - Filter and sinks: `set_level`, `set_target_level`, `enabled`,
    `set_stderr`, `open_file(path, FileConfig)`, `close_file`, `tail(n)`,
    `RING_CAPACITY`, `RING_MAX_LINE_BYTES` (4 KiB: the ring cuts longer
    lines, so it stays near 4 MiB; stderr and the file get them whole).
    `FileConfig::default()` is 4 MiB per file, 3 rotated copies.
  - Format: one record is `<time> <LEVEL> <target>: <message>`. Control
    characters other than tab and newline (C0, DEL, C1) are written escaped
    as `\u{1b}`, trailing line breaks are dropped, and continuation lines
    are indented two spaces, so no message can drive a terminal or forge a
    record.
  - Threading: stderr and the file are written by one `tidepool-log`
    thread through a bounded queue (`QUEUE_CAPACITY` lines,
    `QUEUE_MAX_BYTES`), each line to the file first. When it is full, lines
    are dropped and counted (`dropped_lines`), never waited for; the ring
    buffer is synchronous and keeps every line. A stderr write that blocks
    stalls the file too, but never a caller. `flush()` and
    `flush_timeout(d)` wait for the writer, for shutdown and tests only.
  - Redaction: `register_secret(&str)`. The secret, and the escaped forms
    `{:?}`, `escape_debug` and control-character escaping give it, are
    replaced with `REDACTED` (`[REDACTED]`) in each line before the line
    reaches any sink. Registering re-scrubs the ring buffer and the writer
    queue; lines already written to stderr or the file cannot be recalled,
    so register a secret before anything can log it. A line that redaction
    cannot make safe (a secret overlapping the marker) is withheld from
    every sink and counted (`withheld_lines`); a secret that is part of the
    marker (`"E"`, `"ACT"`) withholds every line in which anything is
    redacted. The registry's `Debug` never shows values. Redacting command
    lines (`SecretArg`, `SecretEnv`) belongs to `toolchain`, not here.
- `testing`: `check`, `check_sized`, `SEED_VAR` (`TIDEPOOL_SEED`),
  `DEFAULT_SEED` (ASCII `"TIDEPOOL"`), `MAX_SIZE` (100), `assert_approx!`
  with the `Approx` trait. A failure report ends with
  `to reproduce: TIDEPOOL_SEED=0x… cargo test -p <package> <test path>`.
- `rng`: `Pcg32::{new(seed, stream), next_u32, next_f32, range, shuffle,
  choose}`.
- `hash`: `fnv1a64`, `crc32`, `Sha256::{new, update, finalize}`, `sha256`,
  `SHA256_LEN`, `to_hex`; `FastHasher`, `FastHashMap`, `FastHashSet` (not
  HashDoS-resistant).
- `cli`: `Args::{new, with_limits, from_env, subcommand, flag, value,
  parsed, finish}` with `Limits` (default 65,536 arguments of up to
  128 KiB). Errors carry the argument's 1-based position.
- `civil`: `DateTime::{now, from_system_time, from_unix_seconds,
  file_stamp}`; `Display` is RFC 3339 UTC with milliseconds.
- `geom`: `Point`, `Size`, `Rect` (`f32`); `DevicePoint`, `DeviceSize`,
  `DeviceRect` (`i32`); `Scale::{new, factor, to_device, to_logical}`.
  Points round to nearest, sizes up, rects outward (edges within 1/64 px of
  a pixel boundary count as on it).
- `color`: `Color::{rgb, rgba, parse, premultiply}`, `FromStr`, `Display`
  (`#rrggbb`, or `#rrggbbaa` when not opaque); `PremultipliedColor::{over,
  unpremultiply}`. `Color::parse` accepts `#RRGGBB`, `#RRGGBBAA` and
  `rgba(R, G, B, A)` (`R`, `G`, `B` integers 0–255; `A` a decimal 0–1 with
  at most 9 places, becoming `round(A × 255)`, halves up), with optional
  ASCII whitespace; at most `MAX_COLOR_LEN` (64) bytes; errors carry the
  byte offset and what was expected.
- `input`: `Key` (`Key::ALL`, `usage`), `Modifiers` (`SHIFT`, `CTRL`,
  `ALT`, `SUPER`), `MouseButton`, `ScrollDelta`, `InputEvent`.
- `cancel`: `CancelToken::{new, cancel, is_cancelled}`;
  `Deadline::{after, at, NEVER, instant, remaining, expired}`.

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
