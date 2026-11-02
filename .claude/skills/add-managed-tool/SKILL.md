---
name: add-managed-tool
description: Use when Tidepool must find, install, update or drive a new outside program (or change how it handles an existing one), such as rust-analyzer, git, an SDK package, Xcode tools, a debug adapter or an LSP server. Adds the descriptor, detection, install plan, tests and Toolchains-panel row per ADR-0002.
argument-hint: "<tool> <what Tidepool needs it for>"
---

# Add or update a managed tool

Request: `$ARGUMENTS`

Outside programs are managed tools (ADR-0002): Tidepool installs, updates,
configures and drives them through their official channels, and never
bundles or reimplements them. Read `docs/subsystems/toolchains.md` and the
"Managed tools" table in `docs/02-dependency-policy.md` first.

## 1. Research the tool (with the web tools; cite sources)

- Official install and update channel per OS (installer, package manager,
  vendor tool such as `rustup` or `sdkmanager`), and whether it needs
  admin rights.
- Official download URL and published checksum, if Tidepool downloads
  anything itself.
- License, and how the user accepts it.
- How to detect it: env vars, standard locations, `PATH` name, version
  command and output format. Check the installed tool's `--help` if this
  session has it.
- Output formats Tidepool will parse (machine-readable flags such as
  `--porcelain`, `--message-format=json`, `-z`).
- If a fact can't be verified, write "unverified" and leave the roadmap
  box unchecked with a `⏳` note.

## 2. Descriptor (`toolchains/<tool>.toml`)

- `id`, purpose, platforms, minimum and recommended versions, detection
  rules, install/update steps per OS, download URL + SHA-256 (pinned; never
  fetched at runtime), license entry, and the features it unlocks.
- Run `cargo xtask tools`. Until M5.1 lands it exits 1 as not yet
  implemented; say so rather than report a pass.

## 3. Code in `tidepool-toolchain`

- Detection and version parsing (with a timeout).
- Install and update plans: every step is listed to the user before
  anything runs; downloads go through `download` and are SHA-256 checked;
  license prompts and admin commands run in the integrated terminal when
  the user presses Run; nothing is piped into prompts.
- Spawning through `process`/`sessions` only: argv vector, explicit env,
  output caps, kill on shutdown. Secrets only through `SecretArg` /
  `SecretEnv`.
- Respect workspace files that pin versions (`rust-toolchain.toml`, Gradle
  wrapper) and workspace trust (ADR-0014).

## 4. Tests

- A fake tool in `crates/testkit/fake-tools/` reproducing real output for
  found, missing, outdated and error cases, including slow responses.
- Parser fuzz target for any output format parsed.
- A gated real-tool test (`TIDEPOOL_IT_<AREA>=1`) that prints
  `SKIPPED: <reason>` when unavailable.
- A test that no constructed argv contains a secret, if the tool takes any.

## 5. UI and docs

- A Toolchains-panel card (status dot, version, path, actions) and, if
  needed, the tool's own panel in `editor`.
- Add or update the row in `docs/02-dependency-policy.md` → "Managed tools"
  and in `docs/subsystems/toolchains.md`.
- Ask `security-reviewer` to review.

## 6. Report

Sources checked, descriptor values, tests run with results, gated tests
not run, and anything still unverified.
