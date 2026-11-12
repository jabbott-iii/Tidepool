# Security Policy

## Supported versions

Tidepool is in development, at version 0.1, and has no releases yet. There
are no maintenance branches: security fixes land on `main`.

| Version | Supported |
| :- | :- |
| `main` | :white_check_mark: |
| Earlier commits | :x: |

Once releases begin (`docs/05-release.md`, roadmap M14), the newest release
is supported as well.

## Reporting a vulnerability

Report vulnerabilities privately, through GitHub's private vulnerability
reporting:

1. On the repository's **Security** tab, select **Report a
   vulnerability**, or go straight to
   <https://github.com/jabbott-iii/Tidepool/security/advisories/new>.
2. Fill in the form. Only you, the maintainer, and the collaborators the
   maintainer adds to the report can see it.

Do not report a vulnerability in a public issue, discussion, pull request
or commit message.

Include as much of this as you can:

- The commit (`git rev-parse HEAD`).
- The OS and its version, and the CPU architecture.
- The input that triggers it: attach a small file (a JSON or TOML file, a
  font, an archive, a terminal byte stream, a server's messages) to the
  private report rather than linking to it.
- The steps to reproduce it, or a proof of concept. A failing test is
  ideal.
- The impact: what an attacker has to control, and what they gain.
- A suggested fix, if you have one.

Remove user names, paths, keys, passwords and other private data from any
log or output you attach.

## What to expect

Tidepool has a single maintainer, so responses are best effort. A report
goes through these steps:

1. **Acknowledgement.** The maintainer replies in the private report.
2. **Assessment.** The maintainer confirms whether it is a vulnerability,
   which commits it affects and how severe it is, and discusses it with you
   there.
3. **Fix.** The fix is prepared privately, in the advisory's temporary
   private fork when needed, and lands on `main`.
4. **Disclosure.** Once the fix is on `main`, the maintainer publishes a
   GitHub security advisory, credits you unless you would rather not be,
   and requests a CVE through GitHub when one is warranted.

If a report is declined, for example as out of scope, the maintainer
explains why in the report. Please keep the details private until the
advisory is published, or until you and the maintainer agree on a date.

## Scope

Tidepool opens files and projects it did not write, starts other programs,
installs tools and talks to language servers and agents. It treats every
byte it reads from them as possibly malformed or hostile. In scope, as each
part lands on the roadmap:

- **Parsers and protocols:** the formats in `tidepool-codec` (JSON and
  JSONC today; TOML, XML, plist, YAML, CSV, INI, DEFLATE, ZIP, PNG and QOI
  later), fonts, Markdown, the terminal's VT parser, git's output, and the
  LSP, DAP and TAP messages. For example:
  - a panic, a hang or a stack overflow on any input;
  - memory use past the limits a parser is given (`Limits`);
  - an archive entry that is written outside its destination.
- **Text positions and edits from outside:** a position or edit from a
  language server or an agent that crashes the editor or corrupts a buffer
  (ADR-0019, ADR-0020).
- **`unsafe` code and OS bindings** in `tidepool-sys` and
  `tidepool-platform`: undefined behavior that safe code can reach, such as
  a binding whose layout does not match the OS's, or a safe wrapper that is
  not sound.
- **Child processes and managed tools:** a command line built from file
  contents, a shell where an argv vector belongs, an install or update that
  runs without the user's confirmation, a download used without its pinned
  SHA-256 matching, or a license accepted for the user.
- **Workspace trust:** project code (build scripts, proc macros, Gradle,
  cargo tasks) that runs in a workspace the user has not trusted
  (ADR-0014).
- **Secrets:** a keystore password, signing identity or API key that
  reaches a command line, a log, a setting, a crash report or an agent.
- **AI context:** a file on the deny-list, or outside the user's grants,
  that reaches an agent; an agent's output that is executed or applied
  without the user accepting it (ADR-0009).
- **Network:** the Tidepool binary opening a socket of its own.
- **Build automation and CI:** `xtask`, which must never reach the network
  or read secrets; the Claude Code hook in `.claude/hooks/`; and the
  workflows in `.github/workflows/`, such as script injection, an unpinned
  action, or a token with more permissions than its job needs; and the
  demo page they publish at https://jabbott.dev/Tidepool/, such as
  repository text reaching it unescaped, or a write token reaching the
  repository's code.

Out of scope:

- **What a trusted project's own code does.** Trusting a workspace lets its
  build scripts and tasks run with your privileges.
- **Bugs in the managed tools themselves** (rust-analyzer, git, the Android
  SDK, Xcode, curl, an agent you installed), in the OS, or in the Rust
  toolchain. Report those to their maintainers, and tell us if Tidepool
  needs to work around one.
- **What a language agent does with the context you granted it,** and the
  services it talks to.
- **The cost of a large input within the limits,** such as the time a valid
  file at the size limit takes to parse.
- **Attacks that need control of your account,** such as replacing
  Tidepool's executable or a managed tool.

Tidepool has no third-party crates (see
[`docs/02-dependency-policy.md`](docs/02-dependency-policy.md)), so there
are no dependency advisories to track beyond Rust's standard library.

## How the code is kept safe

- `unsafe` is allowed in two crates only, and `cargo xtask unsafe-audit`
  fails any `unsafe` block in their source without a `// SAFETY:`
  comment. Every other library forbids `unsafe` in its doc tests too
  (ADR-0036), the audit reads their doc-test code for it, no crate may be
  a proc macro (ADR-0037), no macro outside the two crates may carry a
  caller's `unsafe` into braces or an item it writes, and no macro may
  define a macro (ADR-0038); review checks the `// SAFETY:` comments
  in the two crates' own doc tests.
- `cargo xtask deps` fails the build if a workspace gains a package from
  outside the repository, and the Claude Code hook runs it after every
  manifest edit.
- Every parser takes `Limits`, and has a fuzz target that
  `cargo xtask fuzz-smoke` runs; CI runs it on Linux, and every
  `cargo test` replays the seeds and the saved crashes (ADR-0027).
- CI's workflows pin each action by its full commit SHA, and their
  tokens read the repository only, but for CodeQL's upload of its results
  to code scanning and the demo page's deploy job, which writes only to
  Pages (ADR-0030). `cargo xtask workflows` checks the pins, and that no
  job writes beyond its workflow's row, on every push; CodeQL scans the
  Rust code and the workflows; and Dependabot proposes new pins once a
  release is a week old (ADR-0022).

## Known issues

Vulnerabilities that were reported are disclosed as GitHub security
advisories once fixed (see [What to expect](#what-to-expect)). Published
advisories are listed at
<https://github.com/jabbott-iii/Tidepool/security/advisories>.
