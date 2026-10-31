---
name: next-task
description: Pick up the next unchecked item from the current milestone in docs/PLAN.md, plan it, implement it with the owning agent, verify it, and report. Use when the user wants to continue building Tidepool.
argument-hint: "[milestone or task text, optional]"
disable-model-invocation: true
---

Continue Tidepool development from the plan.

1. Read `docs/PLAN.md`. Select the task:
   - If arguments were given (`$ARGUMENTS`), use the milestone or checklist item they name.
   - Otherwise take the first unchecked `- [ ]` item in the earliest milestone that has one.
   - Check §10: if a decision this task depends on is not yet recorded as an Accepted ADR in
     `docs/adr/`, stop and ask the owner to decide it (offer the plan's recommendation).
2. Read the code and docs the task touches. State the task, its acceptance criteria, the files
   you expect to change, and the owning agent (table in `CLAUDE.md`).
3. If the task needs a design choice not already settled, use the `architect` agent first and
   present the recommendation. Ask the owner before proceeding with anything that changes a
   public API, file format, protocol, or dependency.
4. Implement with the owning agent. Keep the change inside the task's scope.
5. Use `test-engineer` to add tests mapped to the acceptance criteria.
6. Use `security-reviewer` if the change touches `tp-sys`, subprocesses, signing, secrets,
   untrusted-input parsers, workspace trust, or AI context. Address Critical/High findings.
7. Run `/verify`.
8. Tick the checklist item in `docs/PLAN.md` only if its acceptance evidence exists. Otherwise
   leave it unchecked and say what is missing.
9. Report: summary, files changed, validation run (with results), checks not run, assumptions,
   open issues, residual risks, and a proposed commit message. Do not commit.
