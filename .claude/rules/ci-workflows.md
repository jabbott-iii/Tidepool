---
paths:
  - ".github/**"
---

# Rules for CI workflows and GitHub configuration

Read `docs/04-quality.md` → "CI" and ADR-0022 first.

- **Allowed actions only.** `ci.yml` may use `actions/checkout` and
  `actions/cache`; `codeql.yml` may use `actions/checkout` and
  `github/codeql-action/init` and `analyze`. Another action, or a new
  workflow, needs ADR-0022's table extended first: ask the user.
- **Pin by commit SHA.** Every `uses:` names a full 40-hex commit SHA, with
  the release tag in a comment on the same line:
  `uses: actions/checkout@<sha> # v7.0.1`. Look it up with
  `git ls-remote --tags https://github.com/<owner>/<repo> 'refs/tags/vX.Y.Z' 'refs/tags/vX.Y.Z^{}'`,
  and use the peeled `^{}` SHA when the tag is annotated. If you cannot look
  it up in this session, write `# TODO(pin): <action>@<tag>` and say so.
- **Least privilege.** Top-level `permissions:` is `{}` or
  `contents: read`. A job gets `write` only where ADR-0022's table says
  (today, CodeQL's analysis job: `security-events`).
- **Never** `pull_request_target`, and every `actions/checkout` sets
  `persist-credentials: false`.
- **No untrusted input in scripts.** Never expand `${{ github.event.* }}`,
  a branch name or a pull request's title inside a `run:` script; pass it
  through `env:` and quote it.
- **Commands stay paused.** Every `run:` step that reads or prints
  anything from the checkout (files, history, names) or runs the
  repository's code (today the toolchain step and the gate's) runs its
  commands between `::stop-commands::<token>` and `::<token>::`, after
  `exec 2>&1` (`docs/04-quality.md` → "CI"). The step makes the token
  fresh, in a variable it unsets first and never exports, and fails
  before running anything unless it is 32 lowercase hex digits. Keep that
  when editing those steps, and do the same in a new one.
- **One key a line.** `cargo xtask workflows` (M0.8) reads lines, not
  YAML, so no flow-style `{ uses: … }` and no `uses:` in a block scalar.
- **Offline cargo.** Cargo steps run with `CARGO_NET_OFFLINE: "true"`; the
  workspace has no third-party crates to fetch.
- **Secrets** live only in GitHub's secret storage. Docs name them, never
  their values, and no step prints one.
- **Dependabot** updates `github-actions` only; there is no `cargo` entry
  (ADR-0022).
- **Templates:** keep `pull_request_template.md` and `ISSUE_TEMPLATE/` in
  line with `CONTRIBUTING.md` and `SECURITY.md`.
- **After editing,** parse the YAML (and say how), and from M0.8 run
  `cargo xtask workflows`.
