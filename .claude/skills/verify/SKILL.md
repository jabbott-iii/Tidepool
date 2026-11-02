---
name: verify
description: Run Tidepool's full verification suite (format, clippy with warnings as errors, tests, dependency and layer checks) and report exact results. Use before reporting any code change as done.
allowed-tools:
  - Bash(cargo fmt *)
  - Bash(cargo clippy *)
  - Bash(cargo test *)
  - Bash(cargo build *)
  - Bash(cargo xtask *)
  - Bash(sh scripts/check-deps.sh)
  - Bash(rustc -V)
---

Run Tidepool's verification suite from the repository root and report
exactly what happened.

1. Say which toolchain runs: `rustc -V`. `rust-toolchain.toml` pins
   1.99.0; if rustup cannot install it here, report that, and that every
   result below is from another toolchain.
2. Run `cargo xtask ci`. If `xtask` does not build, report its error and
   run, in order, continuing after failures so every result is known:
   - `cargo fmt --all -- --check`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo test --workspace`
   - `sh scripts/check-deps.sh`
3. Report a table: command, result (pass / fail / not run), and the key
   lines of output for failures (first error, failing test names, counts).
4. Report any `SKIPPED:` lines from gated tests separately. Skipped is not
   passed.
5. Do not modify files, silence lints, or skip tests to get a pass. If
   something fails, diagnose it and propose the fix; apply it only if the
   user asked you to fix failures.
