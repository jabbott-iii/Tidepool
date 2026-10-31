---
name: adr
description: Draft a new Architecture Decision Record in docs/adr/ from the template, with status Proposed. Use when a design or dependency decision needs to be recorded or an open decision from docs/PLAN.md §10 is being settled.
argument-hint: "[decision title or PLAN decision ID, e.g. D2]"
disable-model-invocation: true
---

Draft an ADR for: $ARGUMENTS

1. Read `docs/adr/0000-template.md`, list existing ADRs in `docs/adr/`, and pick the next number
   (four digits, e.g. `0001`). File name: `docs/adr/NNNN-short-kebab-title.md`.
2. If the argument names a decision ID from `docs/PLAN.md` §10 (e.g. `D2`), use that row's
   question and recommendation as the starting point and reference the ID.
3. Use the `architect` agent to fill in context, at least two options (including defer), the
   dependency-tier and security impact of each, and a recommendation. Cite evidence.
4. Set **Status: Proposed** and today's date. Never mark an ADR Accepted yourself; tell the owner
   what they need to decide.
5. If the decision affects dependencies, add or update the matching row in
   `docs/DEPENDENCIES.md` with status Pending.
6. Report the file path and the decision the owner needs to make. Do not commit.
