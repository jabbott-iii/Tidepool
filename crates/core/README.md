# tidepool-core

Tidepool's foundations, which every other crate uses: errors with context,
logging with secret redaction, the property-test harness and the fuzzer
behind each parser crate's fuzz targets, a seeded RNG, hashing (SHA-256 for
download integrity only), argument parsing, UTC time, and the geometry,
color and input types that `platform`, `ui`, `editor` and both frontends
share. It has no dependencies, no `unsafe` and no OS-specific code.

It is part of [Tidepool](https://github.com/jabbott-iii/Tidepool), an IDE
for Linux, macOS and Windows, written in Rust with no third-party crates.
Every other member but `tidepool-sys` may use it: today `xtask` parses its command line,
checks the Unicode data's SHA-256 and runs the fuzz targets with it, and the
tests of `codec` and `text` use its property harness and fuzzer.

Layer 0, foundations. It may use no workspace crate, as its row of the
dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says.

## Status

Version 0.1, not published (`publish = false`): the API changes as the
roadmap needs.

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

## Usage

A workspace crate whose row of the matrix allows it depends on it by path:

```toml
[dependencies]
tidepool-core = { path = "../core" }
```

Parse a command line. There is no schema: take each option, then call
`finish`, which rejects whatever is left. An error about an argument names
its position (too many arguments names the limit), and `context` says what
was being done:

```rust
use tidepool_core::cli::Args;
use tidepool_core::{CoreError, ErrorContext};

fn main() -> Result<(), CoreError> {
    let mut args = Args::new(["fuzz-smoke", "--secs", "5", "--target=json", "--replay"])?;
    assert_eq!(args.subcommand().as_deref(), Some("fuzz-smoke"));
    let secs: u64 = args.parsed("--secs")?.unwrap_or(60);
    let target = args.value("--target")?;
    let replay = args.flag("--replay");
    assert!(args.finish()?.is_empty());
    assert_eq!((secs, target.as_deref(), replay), (5, Some("json"), true));

    let mut args = Args::new(["--secs", "5", "--verbose"])?;
    let _secs: Option<u64> = args.parsed("--secs")?;
    let error = args
        .finish()
        .context("reading the command line")
        .expect_err("nothing took `--verbose`");
    assert_eq!(
        error.to_string(),
        "while reading the command line: unexpected argument `--verbose` (argument 3)"
    );
    Ok(())
}
```

A property test. Each case draws its input from a seeded `Pcg32`, which
gives the same numbers on every OS; a failure panics with the seed, the
input and the command that runs it again:

```rust
use tidepool_core::Pcg32;
use tidepool_core::testing::check;

let mut rng = Pcg32::new(42, 0);
let mut replay = Pcg32::new(42, 0);
assert_eq!(rng.range(0..100u32), replay.range(0..100u32));

check(
    "a shuffle keeps every element",
    200,
    |rng| {
        let len = rng.range(0..20u32);
        let mut items: Vec<u32> = (0..len).collect();
        rng.shuffle(&mut items);
        (len, items)
    },
    |(len, items)| {
        let mut sorted = items.clone();
        sorted.sort_unstable();
        if sorted == (0..*len).collect::<Vec<u32>>() {
            Ok(())
        } else {
            Err(format!("{items:?} is not a permutation of 0..{len}"))
        }
    },
);
```

Check a download against the SHA-256 digest pinned for it, the one use
SHA-256 has here (ADR-0016), and log the result. The logger never blocks
on I/O, and it keeps the last lines in a ring buffer:

```rust
use tidepool_core::hash::{Sha256, sha256, to_hex};
use tidepool_core::log::{self, info};

let pinned = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
let download = b"abc";
assert_eq!(to_hex(&sha256(download)), pinned);

// The same digest, from the bytes as they arrive.
let mut hasher = Sha256::new();
hasher.update(b"a");
hasher.update(b"bc");
assert_eq!(to_hex(&hasher.finalize()), pinned);

log::set_stderr(false);
info!("download matches its pinned SHA-256");
assert!(log::tail(16).iter().any(|line| line.ends_with("matches its pinned SHA-256")));
```

Colors in the syntax theme files use, blended source-over on premultiplied
alpha, and logical rectangles converted to device pixels, rounding outward
so that the pixels cover the rectangle:

```rust
use tidepool_core::geom::{DeviceRect, Rect, Scale};
use tidepool_core::{Color, CoreError};

fn main() -> Result<(), CoreError> {
    let accent = Color::parse("#5ccbcb")?;
    let veil = Color::parse("rgba(16, 19, 21, 0.5)")?;
    assert_eq!(veil.a, 128);
    let shown = veil.premultiply().over(accent.premultiply()).unpremultiply();
    assert_eq!(shown, Color::rgb(54, 111, 112));
    assert_eq!(shown.to_string(), "#366f70");

    let scale = Scale::new(1.5).expect("1.5 is finite and above zero");
    let device = scale.to_device(Rect::new(0.5, 0.5, 10.0, 10.0));
    assert_eq!(device, DeviceRect::new(0, 0, 16, 16));
    Ok(())
}
```

## Documentation

- Design: [`docs/subsystems/core.md`](../../docs/subsystems/core.md)
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0016
  (SHA-256 for downloads only); ADR-0027 (the fuzz targets and their
  engine).
- [`docs/04-quality.md`](../../docs/04-quality.md) ("Fuzz smoke"), which
  summarizes `testing::fuzz`; its rustdoc is authoritative.
- API reference: `cargo doc -p tidepool-core --open`

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

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
