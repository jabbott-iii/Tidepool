---
name: record-decision
description: Append an architecture decision record to docs/decisions.md when a design decision is made, changed, accepted or superseded, and update the docs that reference it.
argument-hint: "<short decision title, or ADR-NNNN to accept or supersede>"
---

# Record an architecture decision

Title: `$ARGUMENTS`

1. Read `docs/decisions.md`. The next number is the highest `ADR-NNNN` plus
   one, zero-padded to four digits.
2. **Accepting a `Proposed` ADR the user just approved:** change only its
   `Status` line to `Accepted`, and add the user's words and date to its
   Context if they added conditions. Then go to step 5.
3. Otherwise draft a new ADR using the template at the top of that file:
   - **Status:** `Accepted` only if the user explicitly made or approved the
     decision in this conversation. Otherwise `Proposed`.
   - **Date:** today's date.
   - **Context, Decision, Consequences, Alternatives considered:** factual
     and specific, with numbers where they exist.
   - **Revisit when:** a concrete, observable trigger, not "if needed".
4. **If this supersedes an earlier ADR:** change only that ADR's `Status`
   line to `Superseded by ADR-NNNN`. Do not rewrite its text. Append the new
   ADR at the end of `docs/decisions.md`.
5. Update every doc that states the decision: `AGENTS.md`, the overview,
   architecture, dependency policy, platform and subsystem docs, the
   roadmap, the READMEs of the crates it shapes, and `CONTRIBUTING.md` or
   `SECURITY.md` when it changes how people work or report. Reference the
   ADR number in each place you change. If it answers a task's
   `Decide first` line, say so in a note under that task.
6. Report the ADR number and title, its status, and the docs updated. If
   it's `Proposed`, say what the user needs to decide.
