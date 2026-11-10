# tidepool-bench

Tidepool's own benchmark harness and suites: warm-up, N samples, median and
p95, against a JSON baseline. Dev-only; it never ships.

It is part of [Tidepool](https://github.com/jabbott-iii/Tidepool)'s
development tooling: `cargo xtask bench` builds its binary in the release
profile and runs its suites, which may measure any crate of the workspace
(ADR-0035). Today they measure `tidepool-text`.

Layer 6, tools (dev-only). It may use any workspace crate, as its row of the
dependency matrix in [`docs/01-architecture.md`](../docs/01-architecture.md)
says; it uses `core`, `codec` and `text`.

## Status

Version 0.1, not published (`publish = false`): the API changes as the
roadmap needs.

Built, with the roadmap task that built it:

- `harness`: a suite's setup is untimed and it hands the bencher one
  routine. The bencher warms it up, runs a fast routine in batches of
  about 2 ms, takes samples for a second but never fewer than 10, and
  reports the median and the nearest-rank p95 per iteration, in
  nanoseconds. A routine that changes its input gets a new one for each
  iteration, made outside the timing. A run first primes the allocator,
  so that a suite's numbers do not depend on which suites ran before it
  (`harness::prime_allocator`) — M1.9.
- `baseline`: `bench/baseline.json`, written sorted with `codec::json`:
  a format version; the machine, which is the name given with `--machine`
  and the OS, architecture, thread count and CPU model the harness reads;
  and each suite's median, p95 and sample count — M1.9.
- `cli` and the binary: `[FILTER] [--save --machine NAME]` and the report,
  as ADR-0035 decides — M1.9.
- `suites::text`, 18 suites of what M1.1 and M1.2 measured by hand. They
  cover rope edits, position conversions in each unit, and a 10,000-caret
  keystroke on 400 KB and on 100 MB, with its undo and redo on 400 KB.
  They also cover typing into 100 MB, and grapheme queries on M1.1's
  crafted 100 MB lines — M1.9.
- `bench/baseline.json`, the 18 text suites recorded with `--save` on the
  reference machine, where two runs back to back agreed within 10% on
  every median (the roadmap's note on M1.9 has the numbers) — M1.9.

Not yet, with the roadmap task that adds it:

- suites for the editor (M4.1, M4.2), project search (M4.4), predictive
  text and its latency (M9.4), and the performance pass (M14.3).

Tasks and their state are in [`docs/roadmap.md`](../docs/roadmap.md).

## Usage

Run it through `xtask`, which builds it in the release profile and passes
every argument on:

```bash
cargo xtask bench text           # every suite whose name contains "text"
cargo xtask bench text/history   # only the history suites
cargo xtask bench --help         # the usage, and every suite's name
cargo xtask bench text --save --machine "MSI Pulse GL66 11UGKV"
```

A run prints each suite's median and p95 as it finishes. Where
`bench/baseline.json` was recorded on a machine with the same OS,
architecture, thread count and CPU model, each median also shows its change,
and one more than 10% slower is marked; the run still exits 0. `--save`
writes the suites it measured into the file and keeps the others. It refuses
a file recorded on another machine: move that file aside first.

A suite is a name and a function. The function sets up untimed, then hands
the bencher one routine: `iter` for one that leaves its input as it found
it (it may walk a list of inputs with a counter), and `iter_batched` for one
that changes its input, which then gets a new one each time, made and
dropped outside the timing. Add a suite to its crate's list in
`src/suites/`; `cargo xtask bench` uses `Settings::DEFAULT`, and primes the
allocator before the first suite:

```rust
use std::hint::black_box;
use std::time::Duration;

use tidepool_bench::{BenchError, Bencher, Settings, Suite, run_suite};

fn sum(b: &mut Bencher) -> Result<(), BenchError> {
    let numbers: Vec<u64> = (0..1_000).collect(); // untimed
    b.iter(|| black_box(&numbers).iter().sum::<u64>());
    Ok(())
}

let suite = Suite { name: "example/sum/1k", run: sum };
let quick = Settings {
    warm_up: Duration::ZERO,
    sample_target: Duration::from_micros(100),
    budget: Duration::ZERO,
    min_samples: 5,
    max_samples: 5,
};
let summary = run_suite(&suite, &quick)?;
assert_eq!(summary.samples, 5);
assert!(summary.p95_ns >= summary.median_ns);
# Ok::<(), BenchError>(())
```

## Documentation

- Design: [`docs/04-quality.md`](../docs/04-quality.md) ("Benchmarks")
- Decisions in [`docs/decisions.md`](../docs/decisions.md): ADR-0017
  (`xtask` runs cargo itself) and ADR-0035 (the child `cargo xtask bench`
  starts, the command line, the comparison and the baseline file).
- API reference: `cargo doc -p tidepool-bench --open`

## Rules

- `unsafe` is denied by the workspace lint, and `cargo xtask unsafe-audit`
  checks.
- It starts no program and opens no socket. It reads only the baseline and,
  as a plain file with no platform `cfg`, `/proc/cpuinfo`, and writes only
  the baseline, and that only with `--save` (ADR-0035).
- A change that slows a suite by more than 10% against the baseline says so
  and why (`docs/04-quality.md`).
- Path-scoped rules in `.claude/rules/`:
  [`dependencies.md`](../.claude/rules/dependencies.md) for its
  `Cargo.toml`.

## Checks

```bash
cargo test -p tidepool-bench     # the harness, baseline and report, with fake suites
cargo xtask bench text           # the suites themselves, in release
```

The tests never build a 100 MB text: they run the 400 KB suites, and the
100 MB suites' routines on small inputs. `cargo xtask ci` runs the whole
gate, which does not run the benchmarks.

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
