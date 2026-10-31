---
name: verify
description: Run Tidepool's full verification suite (format check, clippy with warnings as errors, tests, dependency-policy check) and report exact results. Use before reporting any code change as done.
allowed-tools:
  - Bash(cargo fmt *)
  - Bash(cargo clippy *)
  - Bash(cargo test *)
  - Bash(cargo build *)
  - Bash(sh scripts/check-deps.sh)
  - Bash(sh scripts/ci.sh)
---

Run Tidepool's verification suite from the repository root and report exactly what happened.

1. If `Cargo.toml` does not exist yet (before milestone M0), run only
   `sh scripts/check-deps.sh`, report that there is no workspace to build, and stop.
2. If `scripts/ci.sh` exists, run `sh scripts/ci.sh`. Otherwise run, in order, continuing after
   failures so every result is known:
   - `cargo fmt --all -- --check`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo test --workspace`
   - `sh scripts/check-deps.sh`
3. Report a table: command, result (pass / fail / not run), and the key lines of output for
   failures (first error, failing test names, counts).
4. Report any `SKIPPED:` lines from gated tests separately. Skipped is not passed.
5. Do not modify files, silence lints, or skip tests to get a pass. If something fails, diagnose
   it and propose the fix; apply it only if the user asked you to fix failures.
