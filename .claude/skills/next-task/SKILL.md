---
name: next-task
description: Pick the next unblocked task from docs/roadmap.md (or the task ID given) and carry it through the project's definition of done, leaving changes uncommitted for review.
argument-hint: "[task-id, e.g. M1.2]"
disable-model-invocation: true
---

# Work the next roadmap task

Requested task: `$ARGUMENTS` (empty means "pick the next unblocked task").

## 1. Choose the task

1. Open `docs/roadmap.md`.
2. If a task ID was given, use it. If its `Depends` tasks are not all
   checked, stop and tell the user which ones block it.
3. Otherwise, take the **first** unchecked task whose `Depends` are all
   checked.
   - Skip a task whose remaining work is marked `⏳ needs <OS/tool>` when
     this session can't provide it. Mention any task you skipped and why.
4. If the task says "ask the user to accept ADR-NNNN first" and that ADR is
   still `Proposed` in `docs/decisions.md`, present the ADR's decision and
   alternatives in a few sentences and ask. Record the answer with
   `/record-decision` (supersede or accept) before writing code.
5. Tell the user in one or two sentences which task you're starting and how
   you plan to approach it. Create a task list whose last item is
   "Verify and report".

## 2. Load context, and only the context you need

- `AGENTS.md` is already loaded. Re-read its "Non-negotiable rules" if
  unsure.
- Read the docs that `AGENTS.md` → "Read before working" lists for the
  crates the task touches.
- Read the existing code in those crates before writing new code. Follow
  established patterns and naming. Where the task says to port from Nadir,
  read the matching Nadir code first and port it with a note.
- If the spec and the code disagree, or the spec is ambiguous in a way that
  changes a public API, a file format, a protocol or the dependency matrix:
  stop, describe the conflict, propose the safest option, and ask.

## 3. Implement the smallest complete change

- Do exactly what the task's `Do` lines and acceptance criteria require.
- Use the subagent that owns the crate (table in `CLAUDE.md`).
- **New third-party crate needed?** Stop and run `/evaluate-dependency`.
- **OS bindings needed?** Follow `/add-ffi-binding`.
- **A new or changed outside tool?** Follow `/add-managed-tool`.
- **A design decision changed?** Run `/record-decision` and update the
  affected docs.
- Write the tests the acceptance criteria call for, beside the code.
- Ask `security-reviewer` to review when the change touches `sys`,
  `platform`, `toolchain`, `mobile`, `agent`, or any parser.
- If the change spreads beyond three crates, pause and tell the user the
  task may be mis-scoped.

## 4. Verify honestly

- Run every command in the task's `Verify` line.
- Then run `/verify` (which runs `cargo xtask ci` once it exists).
- Fix failures by fixing the code. **Never** weaken a lint, delete or
  `#[ignore]` a test, raise a tolerance, or re-bless goldens to make a check
  pass, unless the user explicitly agrees.
- Criteria that need another OS, a real tool, a device or manual
  observation:
  1. Do everything possible here.
  2. Leave the box unchecked.
  3. Add an indented `⏳ needs <OS/tool>: <exact remaining check>` line
     under the task.

## 5. Update the roadmap and docs

- Check the box **only** if every acceptance criterion is met and every
  verification command ran and passed in this session.
- Add brief indented `Note:` lines under the task where useful: pinned
  versions, measured numbers, tool facts checked and their sources.
- Update docs only where behavior or design actually changed.

## 6. Do not commit

Leave all changes uncommitted. Do not commit, amend, rebase, tag or push.

## 7. Report

- **Task:** ID and title, and whether its box is now checked.
- **Changes:** what changed, and the files affected.
- **Validation:** each command run, with its real result.
- **Not run:** checks that weren't run, and why. Include `⏳` items.
- **Assumptions** you made.
- **Unresolved issues and residual risks.**
- **Commit message:** a proposed Conventional Commits message.
- **Next:** the next unblocked task ID.
