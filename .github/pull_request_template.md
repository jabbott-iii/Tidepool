<!--
Thank you for contributing to Tidepool. Read CONTRIBUTING.md before opening this
pull request: the project rules in AGENTS.md apply to every change.
A pull request needs an issue on which a maintainer confirmed the change
(CONTRIBUTING.md, "Contribution workflow"), unless it fixes a typo or a broken link.
Replace the comments below, and remove the sections that do not apply.
-->

## Linked issue and roadmap task

Closes #

- Roadmap task: <!-- such as M1.3, or "none" -->
- [ ] A maintainer confirmed the change on the linked issue.
- [ ] The task's `Decide first` questions, if any, are answered and recorded in `docs/decisions.md`.

## Summary

<!-- What changed and why. Keep the pull request to the one issue or task. -->

## Type of change

- [ ] Bug fix
- [ ] Roadmap task (every acceptance criterion in `docs/roadmap.md` is met)
- [ ] New feature or change outside the roadmap (recorded in `docs/decisions.md`)
- [ ] Breaking change: a public API, a file format Tidepool writes, a protocol (TAP), the dependency matrix or a command line
- [ ] Managed tool: a new or changed descriptor, detection or install plan
- [ ] Performance
- [ ] Documentation
- [ ] Build, CI or `xtask`
- [ ] Tests or refactor only, with no change in behavior

## Validation

<!-- Tick only what you ran, and paste the output below. Say which checks
you could not run, and why: a check that needs macOS or Windows, a real
tool (rust-analyzer, the Android SDK, Xcode), or a display, for example. -->

- [ ] `cargo xtask ci` (deps, fmt, clippy `-D warnings`, tests, layers, crates, unsafe-audit, headers, workflows, ucd)
- [ ] `cargo xtask fuzz-smoke --target <target>`, for a parser change (from M1.6)
- [ ] `cargo xtask golden`, for a change to what `gfx`, `font`, `ui`, `gui` or `tui` draw (from M2.4)
- [ ] `cargo xtask bench <filter>`, for performance work (paste the numbers before and after; from M1.9)
- [ ] Gated real-tool tests (`TIDEPOOL_IT_RA`, `TIDEPOOL_IT_GIT`, `TIDEPOOL_IT_ANDROID`, `TIDEPOOL_IT_IOS`): say which, and that none printed `SKIPPED:`
- [ ] `cargo tp`, checked by hand (describe what you saw below)

**Tested on:** <!-- OS and version (Linux X11 or XWayland, macOS, Windows), CPU, and `rustc -V` -->

**Not run, and why:** <!-- or "none" -->

<details>
<summary>Command output</summary>

```text
paste output here
```

</details>

## Checklist

- [ ] No crate from outside the workspace is added, and no `Cargo.lock` gains a `source =` entry (`docs/02-dependency-policy.md`).
- [ ] Each crate depends only on the crates its row of the dependency matrix in `docs/01-architecture.md` allows.
- [ ] `unsafe` is only in `tidepool-sys` and `tidepool-platform`; each block has a `// SAFETY:` comment, and each `pub unsafe fn` a `# Safety` section.
- [ ] `#[cfg(target_os = ...)]` is only where `AGENTS.md` allows it.
- [ ] Outside programs are started only through `tidepool-toolchain`, with an argv vector; no install runs without the user's confirmation.
- [ ] The main thread waits on no file, child process, language server or agent.
- [ ] No `unwrap()` in library code, every `pub` item is documented, and no lint is weakened.
- [ ] Tests cover the change. No test is deleted or ignored, no tolerance is raised and no golden image is re-blessed to make a check pass, unless the summary says why.
- [ ] A change to a format Tidepool writes bumps its version, with a migration or a clear error for old files, and a fixture test.
- [ ] Docs are updated where behavior or design changed: the subsystem spec, the crate's `README.md`, an ADR in `docs/decisions.md`, and for a roadmap task its tick.
- [ ] New source files start with the license header (ADR-0021).
- [ ] No secrets, keys, keystores, signing material, tokens or build output (`target/`) are included.
- [ ] If a coding agent wrote part of this change, I have reviewed all of it, and the agent followed `AGENTS.md`.

## Security and compatibility

<!--
Security: does this change how untrusted input is read (parsers, protocols,
terminal output), `unsafe` code or OS bindings, how programs are started or
installed, workspace trust, secrets, what an agent sees, or the CI workflows
and their permissions? Describe the effect.
Never describe an undisclosed vulnerability here: report it privately as
SECURITY.md describes.
Compatibility: does this change a public API, a file format, TAP, a command's
arguments or a key binding? Write "None" if neither applies.
-->

## Screenshots or recordings

<!-- For a change to what Tidepool draws: before and after. Remove this
section otherwise. -->

## Notes for reviewers

<!-- What to look at first, known limits, and follow-up work. -->
