---
paths:
  - "**/Cargo.toml"
  - "**/Cargo.lock"
  - ".cargo/**"
  - "xtask/**"
  - "rust-toolchain.toml"
---

# Rules for manifests and dependencies

- **No third-party crates.** Do not add any non-workspace crate to
  `[dependencies]`, `[dev-dependencies]` or `[build-dependencies]` in any
  manifest, including under `agents/`. The approved set in
  `docs/02-dependency-policy.md` is empty.
- **If you think a crate is needed, stop.**
  1. Run `/evaluate-dependency <crate> <need>`.
  2. Present the evaluation to the user.
  3. Change nothing until the user approves.
- **Outside programs are not dependencies to add here.** They are managed
  tools (ADR-0002); use `/add-managed-tool`.
- **Workspace dependencies must follow the matrix** in
  `docs/01-architecture.md`. If a task seems to need a new edge, stop and
  ask. Do not edit the matrix to make code compile.
- **Lints:** every member uses `[lints] workspace = true`. Never weaken
  `[workspace.lints]`, `clippy.toml` or `unsafe_code`. Never add
  `#![allow(...)]` without a comment explaining why the lint is wrong there.
- **Toolchain:** don't change `rust-toolchain.toml` as a side effect of
  another task.
- **Lockfile:** `Cargo.lock` must stay free of `source = ` entries. Check
  with `cargo xtask deps`; the hook runs it after every manifest edit, and
  falls back to `sh scripts/check-deps.sh` when `xtask` does not build.
- **Members:** every member inherits `edition`, `version` and `license`
  from the workspace, sets `publish = false` and
  `[lints] workspace = true`, and gives no target an `edition` of its own;
  `cargo xtask crates` checks all of it, and that the workspace's edition
  is 2024. A new member also gets a `README.md` (ADR-0023, ADR-0028),
  copies of the root's `LICENSE` and `NOTICE` (ADR-0028), a row in the
  dependency matrix, and a line in `AGENTS.md`'s workspace map; a new
  library other than `sys` and `platform` also gets
  `#![doc(test(attr(forbid(unsafe_code))))]` in its crate root, which
  `cargo xtask unsafe-audit` checks (ADR-0036). No member may be a
  proc-macro crate (ADR-0037).
- **Build scripts:** `build.rs` may only emit `cargo:` directives. No
  network access, no running external tools, no compiling C.
- **xtask** never uploads, publishes, signs with real credentials, reads
  secrets or calls the network.
