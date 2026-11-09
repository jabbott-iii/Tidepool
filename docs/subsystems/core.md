# Subsystem: `tidepool-core`

Foundation types used by every other crate. No `unsafe`, no OS calls, no
dependencies. Much of it can be ported from Nadir's `nadir-core` (same
owner); port the code, don't depend on the crate.

## Contents

| Module | Provides |
| :- | :- |
| `error` | `CoreError` and the `ErrorContext` helper for "while doing X" context |
| `log` | Macros `error!`, `warn!`, `info!`, `debug!`, `trace!` with a module target; sinks: stderr, rotating file, in-memory ring buffer (last 1,000 lines, for the Output panel and crash reports); redaction hook for secrets |
| `testing` | Property-test harness (`check`, `check_sized`), `assert_approx!`; `testing::fuzz`, the fuzzer behind each parser crate's fuzz targets (ADR-0027) |
| `rng` | `Pcg32` (seedable), for tests and jitter in restart backoff |
| `hash` | `fnv1a64`, a deterministic non-cryptographic hasher with `FastHashMap`/`FastHashSet`, `crc32` (IEEE), and `sha256` for download integrity only (ADR-0016) |
| `cli` | Minimal argument parser (`--flag`, `--key value`, positional) for `tidepool`, `xtask` and `agents/` |
| `civil` | UTC date/time from `SystemTime` for log, crash and session file names |
| `input` | `Key` (physical keys), `Modifiers`, `MouseButton`, `InputEvent`, shared by `platform`, `ui`, `editor` and both frontends |
| `geom` | `Point`, `Size`, `Rect` in logical units (`f32`) and device pixels (`i32`), scale conversion |
| `color` | `Color` (sRGB `u8` RGBA), premultiplied blending, hex parsing (`#5ccbcb`, `rgba(…)` for theme files) |
| `path` | UTF-8 path helpers, canonicalization, containment, display of non-UTF-8 paths |
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
- `testing::fuzz` (M1.6, ADR-0027): `Target::{new, seeds_in, dictionary,
  memory_cap, reserved_cap, name, corpus, crashes, run, run_with}`;
  `Config::from_env`; `Outcome::{is_clean, hung, summary, assert_clean}`
  and its `Display` report; `Summary::parse` (the `fuzz-result:` line
  `fuzz-smoke` reads) with `PeakMemory` and `Reading`;
  `Finding::{path, site_in_std}` with its `input` (`Found`: `File` or
  `Range`) and `earlier`; `Kind::file_prefix`, `Slowest`, `Memory`, `Skip`,
  `Stop::{code, describe}`; `FuzzError`; `keep_died`, returning
  `Option<Kept>` (`New` or `Existing`, `Kept::path`). Constants: `SECS_VAR`
  (`TIDEPOOL_FUZZ_SECS`), `SEED_VAR` (`TIDEPOOL_FUZZ_SEED`), `REPLAY_VAR`
  (`TIDEPOOL_FUZZ_REPLAY`), `LAST_INPUT`, `DIED_PREFIX`, `RESULT_PREFIX`,
  `HANG_LIMIT` (5 s), `STACK_SIZE` (2 MiB), `MAX_LEN` (64 KiB),
  `MAX_SEED_BYTES` (1 MiB), `DEFAULT_MEMORY_CAP` (64 MiB),
  `DEFAULT_RESERVED_CAP` (256 MiB). Its rustdoc is the reference, and
  `docs/04-quality.md` → "Fuzz smoke" summarizes it.
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
- `path` (M1.8; ADR-0032, ADR-0033):
  - **`display(&path)`** returns `PathDisplay`, for people and logs only.
    - Valid UTF-8 is written as it is.
    - Each byte that is not part of valid UTF-8 is written as `\xNN`, with
      uppercase hex. On Windows an unpaired surrogate shows as its WTF-8
      bytes, `\xED\xA0\x80` for U+D800, the same on every OS.
    - Each control character (U+0000–U+001F, U+007F, U+0080–U+009F, tab
      and newline included), bidirectional control (U+061C, U+200E,
      U+200F, U+202A–U+202E, U+2066–U+2069) and line or paragraph
      separator (U+2028, U+2029) is written as `core::log` writes an
      escape, `\u{1b}`. That is `xtask`'s set (ADR-0033), so a display is
      one line and cannot reorder how the rest of its line is drawn.
    - Nothing else is escaped, a backslash and quotes included.
    - `Debug` is the same text in double quotes.
    - It borrows, never allocates, and ignores width, fill and precision.
    - A path is opened, compared and passed to a child by its `Path`,
      never by its display.
  - **`to_utf8(&path)`** returns `Result<&str, PathError>`: the path as
    text, for what holds only text (TOML, JSON, URIs, the UI). `NotUtf8`
    names the path by its display.
  - **`canonicalize(path)`** is for a path that exists.
    - The result is absolute, with every link and every `.` and `..`
      resolved as the OS resolves them (`fs::canonicalize`).
    - On Windows the `\\?\` prefix is dropped exactly when Win32 reads the
      plain form back unchanged (`std::path::absolute(plain) == plain`).
      That is std's own test before it passes a directory to a child.
      There is no length rule, and a name that is not valid UTF-16 is
      tested like any other.
    - Win32 rewrites a path's last name (a trailing dot or space, a device
      name) and drops one final dot from each name inside it, but keeps a
      space inside it. So `\\?\` stays for any path Win32 reads otherwise:
      `\\?\C:\w\space ` and `\\?\C:\w\dir.\a.txt` stay verbatim, while
      `C:\w\space \a.txt` is plain. Forms can alternate down a tree, so an
      ancestor of a canonical path is taken by canonicalizing it, never
      with `Path::parent`.
    - A relative path starts at the process's current directory.
  - **`resolve(path)`** is the same, except that the end of the path need
    not exist yet.
    - The OS canonicalizes the whole path first; if it exists, that is the
      answer. Otherwise the path is walked from its root one name at a
      time, each name looked at once with `fs::symlink_metadata`, so the
      work is one look per name of the path and of the link targets it
      follows, besides the canonicalization at the start and the end. No
      part in the middle is canonicalized: that would repeat the OS's work,
      and on Windows it would open a directory that may only be passed
      through.
    - A link is followed as the OS follows it, from the link's own
      directory, whether it leads anywhere or not, and its target is walked
      the same way (an absolute one from its root). On Windows a relative
      target's `..` are applied to its text first. An empty target (macOS
      and the BSDs allow one) is `NotFound`, as those OSes answer.
    - A name below a missing name is missing too, and a `..` after a
      missing name cancels it, one for one, without asking the file system
      again.
    - A final separator, or on Unix a final `/.`, asks for a directory:
      after a file it is `NotADirectory`, and a missing last name is
      planned as a directory. On Windows so does a separator left once
      Win32 trims a final space or dot (`file.txt\ `).
    - `resolve` follows 40 links at most, counting every link it follows
      across the call: when the whole path does not exist, every link on
      the path and in the targets it follows, as the OS counts them. So
      `TooManyLinks` comes on any OS for a chain or loop through names that
      do not exist yet. Once a path is made, the OS's limit decides (32 on
      macOS, 63 on Windows).
    - On Windows its probes use the verbatim spelling of the path, `\\.\`
      paths included, so Win32 cannot read a leading part again (a
      trailing space, a device name).
      A name Windows reads as a drive (`c:x`, a stream), a `.` a verbatim
      path keeps, and a link target that is neither relative nor absolute
      are `InvalidInput`. Paths are built by appending text, never by
      pushing components.
    - `resolve(p)` equals `canonicalize(p)` once `p`'s missing names have
      been made.
  - **`under(path, root)`** returns `Option<&Path>`: the part of `path`
    inside `root`, compared component by component. It is `Some("")` when
    they are equal.
    - It returns `None` when either path is relative or holds `.` or `..`.
    - A plain and a verbatim Windows prefix for the same volume count as
      equal. A root that is a bare verbatim prefix (`\\?\C:`, or
      `\\?\UNC\srv\share` with no `\` after it) holds nothing.
    - Every part of the answer is one name that `std` reads back as
      itself, so a part such as `c:x` gives `None`, and `root.join(part)`
      has `path`'s names. It names the same file only when `root` has
      `path`'s form or `path` is plain: a verbatim `path` under a plain
      `root` joins to the plain form Win32 reads as something else (a
      device, a name without its trailing space or dot). Callers keep
      `path`, or canonicalize or resolve the join again.
    - It does no I/O and folds no case, so both sides must come from
      `canonicalize` or `resolve`.
  - **`PathError`** has the variants `Empty`, `NotUtf8 { path }`,
    `Io { path, source }` and `TooManyLinks { path }`, plus a `kind()`
    method (`TooManyLinks` is `Other`, since the kind for a link loop is
    not stable). Messages name the path by its display, as
    ``cannot resolve `<path>`: <OS error>``.
  - `canonicalize` and `resolve` block on the file system and run on
    workers, never on the main thread (rule 6).
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
- A path's display holds no character of the escaped set and shows every
  byte that is not part of valid UTF-8. A UTF-8 path with none of those
  characters displays unchanged.
- For a path that exists, `resolve(p) == canonicalize(p)`. For one that
  does not, `resolve(p)` is what `canonicalize(p)` gives once the missing
  names exist: directories, then the last name as a file (as a directory
  if the path ends in a separator), and a dangling link's target.
- On Linux neither function rewrites a name. On macOS `realpath(3)` gives
  each name that exists as the file system stores it (its case, and on
  HFS+ its decomposed form), which `core::path` keeps unchanged. It
  refuses a path over 1,024 bytes, so both functions fail for one there.
  `core::path`'s library code has no platform `cfg`.

## Tests

- Property tests for `geom` conversions and `color` blending.
- SHA-256 and CRC-32 test vectors.
- Logger rotation in a temp directory under `target/`.
- `civil` against known timestamps, including leap years.
- The display of bytes that are not UTF-8 is tested with paths built in
  the test: `cfg(unix)` builds the bytes and `cfg(windows)` builds an
  unpaired surrogate. Every such `cfg` lives in `src/path/tests.rs`
  (ADR-0032); the tests of Windows' prefixes ask at run time whether `std`
  parses them (ADR-0033).
- `canonicalize` and `resolve` are tested in a temporary directory under
  `target/`, covering file and directory links, links that lead nowhere,
  loops, the 40-link limit, a missing tail, `..`, a final separator and
  relative paths. A plan from `resolve` is made, then compared with
  `canonicalize`. Three tests bound the time: of 2,000 names and `..`
  (missing or not), of 40 links through missing names, and of 40 absolute
  links into a tree 300 deep against `canonicalize` of the made path.
- Windows' names (a trailing space inside a path, `c:x`, a verbatim `.`,
  a `\\.\` spelling, a final `\ `) are tested where `std` parses
  Windows' prefixes, asked at run time.
- Each link test prints `SKIPPED:` where the OS or the account cannot make
  or follow the link.
