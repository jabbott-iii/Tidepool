---
paths:
  - "crates/agent/**"
  - "agents/**"
---

# Rules for AI agents and context

Read `docs/subsystems/agents.md` first.

- **Agents are external processes speaking TAP** (ADR-0009). The IDE core
  has no HTTP client, TLS or AI SDK. Reference agents in `agents/` are
  std-only too; HTTPS goes through `curl`.
- **TAP changes** are versioned, backward-compatible for one release, and
  reflected in `docs/subsystems/agents.md` in the same change; `architect`
  reviews them first.
- **The IDE assembles all context.** Apply size caps, `.tidepoolignore`, the
  secret deny-list, consent and workspace trust before any byte is sent.
  Agents never receive paths to read themselves.
- **Agent edits are proposals:** shown as a diff, applied only when the user
  accepts, undoable as one step. TAP v1 has no way to run commands.
- **Agent output is untrusted** (prompt injection is possible): validate
  sizes, ranges, versions and JSON; never execute or render it as markup.
- **Responsiveness:** never wait on an agent on the main thread; debounce,
  cancel on every keystroke, time out, drop stale versions.
- **Keys:** the Claude agent reads `ANTHROPIC_API_KEY` from its environment
  and passes it to curl via `--config -` on stdin. Never log headers or
  keys. Check the current Anthropic API docs for endpoints and streaming.
- **Tests:** deny-list, consent, latency with a slow mock, stale results,
  crash/restart. Network tests are opt-in and print `SKIPPED:` otherwise.
- Ask `security-reviewer` to review context assembly and consent changes.
