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
- **Status lives in four places:** the roadmap's ticks, the status
  paragraph of `AGENTS.md`, the status line of the root `README.md`, and
  the `## Status` section of each crate's `README.md`. Keep them in
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
- **Crate READMEs** keep the shape of ADR-0023: `# <package name>`, a
  paragraph, the layer line, then `## Status`, `## Design`, `## Rules`,
  `## Checks` and `## License`. Every task ID they name must exist in the
  roadmap.
- **Specs and code agree.** Where rustdoc is authoritative (as for
  `codec::json`), the spec summarizes it; change both in the same change.
- **Cite, don't restate.** Point to the ADR, spec section or task rather
  than copying its text into another doc, so there is one place to update.
- **No secrets** in any doc or example, not even fake-looking ones.
