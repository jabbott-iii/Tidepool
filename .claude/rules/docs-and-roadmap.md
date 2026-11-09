---
paths:
  - "docs/**"
  - "AGENTS.md"
  - "CLAUDE.md"
  - "README.md"
  - "CONTRIBUTING.md"
  - "SECURITY.md"
  - "crates/*/README.md"
  - "xtask/README.md"
  - "bench/README.md"
  - "diagrams/**"
  - "site/**"
---

# Rules for docs, the roadmap and READMEs

- **Some docs are read by checks; keep their shape.**
  - `cargo xtask layers` parses the "Dependency matrix" section of
    `docs/01-architecture.md` on every run, under strict rules
    (`docs/04-quality.md` → `layers`): one table, no raw HTML outside code,
    no other line in the section with a `|`. Run `cargo xtask layers` after
    editing that file.
  - A test in `xtask` reads the table of reference agents in
    `docs/subsystems/agents.md` (its `` | `tp-agent-… `` rows). Run
    `cargo test -p xtask` after editing it.
  - `cargo xtask crates` takes the task IDs a crate README may name from
    the task lines of `docs/roadmap.md` (`- [ ] **M1.3 — ` and
    `- [x] **M1.3 — `), and reads each crate README (below). Run it after
    editing a README or a task line.
  - `cargo xtask features` reads `docs/features.jsonc` and the roadmap's
    task lines: ticking a task updates its feature's summary and `ticked`
    in the same change. Run it after editing either.
- **Status lives in seven places:** the roadmap's ticks, the status
  paragraph of `AGENTS.md`, the status line of the root `README.md`, the
  `## Status` section (and the `## Usage` line of a crate with nothing to
  use yet) of each crate's `README.md`, what `diagrams/` draws solid or
  dashed (ADR-0029), the features' `ticked` lists and summaries in
  `docs/features.jsonc`, and what the demo page's two diagrams in
  `site/index.html` draw solid or dashed (ADR-0030). Keep them in
  agreement, and do not add status to other docs.
- **Write what exists in the present tense, and what does not with its
  task:** "`core::path` lands in M1.8". Never describe a command, crate,
  module or file as existing before it does.
- **The roadmap** (ADR-0024):
  - Task IDs never change. A new task takes the next free number in its
    milestone and is listed where it is to be done; a split task keeps its
    ID on the part that finishes what it promised.
  - Keep each task's shape: `- [ ] **M1.3 — area: title**`, then
    `Depends`, `Decide first` (if any), `Do`, `Accept`, `Verify`, and
    `Note:` and `⏳ needs …` lines. `/next-task` reads it.
  - Never tick a box unless every acceptance criterion is met and every
    `Verify` command ran and passed in this session. Never edit a finished
    task's text; add a dated `Note:` instead.
- **ADRs** are appended, never rewritten. Change an accepted ADR only by
  superseding it, and only the user accepts one (`/record-decision`).
- **Crate READMEs** keep the shape of ADR-0028 (Nadir's, with ADR-0023's
  Rules and Checks): `# <package name>`, what the crate is, how it fits in
  Tidepool, the layer line, then `## Status`, `## Usage`,
  `## Documentation`, `## Rules`, `## Checks` and `## License`, each once
  and in that order (`## Features` only for a crate with Cargo features,
  between Usage and Documentation). A library's Rust examples run as doc
  tests through `ReadmeDoctests` in its `src/lib.rs`, so they use only the
  API that exists; a binary's README has no Rust block. Every task ID they
  name must exist in the roadmap. Every member keeps byte-for-byte copies
  of the root's `LICENSE` and `NOTICE`. `cargo xtask crates` checks the
  first line, the sections and their order, the task IDs and the copies,
  not the rest of the shape, nor whether what a README calls built is
  built.
- **Diagrams** (`diagrams/`, ADR-0029) are Mermaid in Markdown, drawn from
  the code and the specs. What the code has is solid; what the roadmap
  plans is dashed and names the task that builds it; class diagrams show
  only types that exist. The code wins: a change to what a diagram draws
  updates it, and a task that builds something drawn as planned makes it
  solid. No check reads them, so parse a changed block before you finish.
- **Specs and code agree.** Where rustdoc is authoritative (as for
  `codec::json`), the spec summarizes it; change both in the same change.
- **Cite, don't restate.** Point to the ADR, spec section or task rather
  than copying its text into another doc, so there is one place to update.
- **No secrets** in any doc or example, not even fake-looking ones.
