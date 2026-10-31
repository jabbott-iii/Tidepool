---
paths:
  - "crates/toolchain/**"
  - "crates/mobile/**"
  - "crates/vcs/**"
  - "toolchains/**"
---

# Rules for managed tools, processes and secrets

Read `docs/subsystems/toolchains.md` and, for mobile work,
`docs/subsystems/mobile.md` first.

- **Only `toolchain` spawns programs.** argv vector, explicit environment,
  timeout, output caps, kill-on-drop. Never `sh -c`, `cmd /c`, or a command
  string built from input.
- **Installs:** official sources only; the user confirms a plan that lists
  every step first; downloads go through `curl` and must match the pinned
  SHA-256 in the descriptor before anything from them runs.
- **Licenses** (`sdkmanager --licenses`, Xcode) are shown to and accepted
  by the user in the integrated terminal. Never pipe `yes` into a prompt.
- **No silent elevation.** Commands needing `sudo` or an admin prompt run
  visibly in the integrated terminal when the user presses Run.
- **Never bundle** a tool's binary in the repo or the package (ADR-0002).
- **Tool facts change.** Before relying on a flag or output format, check
  the current official docs and the installed tool's `--help`/version, and
  cite what you checked in the task note.
- **Secrets** (keystore and key passwords, API keys, signing identities):
  - use `SecretArg`/`SecretEnv`; they redact in logs and `Debug`
  - never on argv: stdin, a per-child env var, or a 0600 file in a 0700
    temp dir removed on every path
  - never stored by Tidepool in v1; never in fixtures or snapshots
  - never read keystores, `.p12`, `.p8`, `.pem`, `.key` files or
    provisioning profiles directly; pass their paths to the official tool
- **Signing is delegated** to `apksigner`, `jarsigner`, `codesign`
  (ADR-0007). After signing, always run the verifier.
- **Untrusted workspaces** run no project-owned executables (Gradle
  wrapper, build scripts) (ADR-0014).
- **git writes** (stage, commit, branch, stash) happen only on an explicit
  user action in the UI; commit messages go on stdin.
- **Tests** use fake tools and throwaway keys generated at test time;
  real-tool tests are gated by `TIDEPOOL_IT_*`. Add a test that no
  constructed argv contains a secret.
- Ask `security-reviewer` to review every change here before reporting done.
