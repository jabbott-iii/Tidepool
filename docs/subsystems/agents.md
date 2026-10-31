# Subsystem: `tidepool-agent` and the Tidepool Agent Protocol (TAP)

AI features come from **language agents**: external processes that plug in
per language and speak TAP over stdio (ADR-0009). `tidepool-agent` hosts
them, decides what context they see, and merges their suggestions with
Tidepool's own offline predictive text.

## Contents

| Module | Provides |
| :- | :- |
| `manifest` | Loads `agent.toml` manifests from the user and workspace agent directories |
| `host` | Spawns agents through `toolchain::sessions`, negotiates capabilities, supervises (restart with backoff), shuts down |
| `tap` | TAP message types over `rpc`'s JSON-RPC transport |
| `context` | Builds each request's context: prefix/suffix windows, open buffers when granted, symbols from `lang` when granted; applies size caps, `.tidepoolignore` and the secret deny-list |
| `consent` | Per-workspace, per-agent grants: once / always / never, for each capability |
| `predict` | Offline predictive engine (below) |
| `merge` | Ranks and merges LSP completions, local predictions and agent suggestions |

## TAP v0.1

- **Transport:** JSON-RPC 2.0 with LSP's `Content-Length` framing on the
  agent's stdin/stdout. stderr is captured to the agent log.
- **Lifecycle:** `initialize` (protocol version, client capabilities,
  languages) → result with the agent's capabilities → `initialized`;
  `shutdown` → `exit`.
- **Document sync:** `textDocument/didOpen`, `didChange` (incremental),
  `didClose`, exactly as in LSP, only for documents the agent's grants
  cover.
- **Requests from Tidepool to the agent:**
  - `tap/inlineCompletion` `{ uri, version, position, prefix, suffix,
    trigger }` → `{ items: [{ insertText, range }] }`. Modeled on LSP's
    inline-completion request.
  - `tap/predict` `{ uri, version, position, prefix }` → `{ words: [..] }`,
    short next-word or phrase predictions, mainly for prose and Markdown.
  - `tap/chat` `{ conversation, message, attachments }` → streamed
    `tap/chatDelta` notifications, then a final result.
  - `tap/proposeEdits` `{ instruction, uri, range }` → an LSP-shaped
    `WorkspaceEdit`.
  - `$/cancelRequest`, as in LSP.
- **No commands or tools:** agents cannot ask Tidepool to run anything,
  open files beyond their grants, or write files. Edits are proposals.
- **Versioning:** `initialize` carries `tapVersion: "0.1"`. A breaking
  change bumps the major part; Tidepool keeps speaking the previous
  version for one release.

## Manifest (`agent.toml`)

```toml
format = 1
id = "tp-agent-claude"
name = "Claude"
version = "0.1.0"
command = "tp-agent-claude"          # resolved like any managed tool path
args = []
languages = ["rust", "markdown"]     # language-pack ids, or "*"
capabilities = ["inlineCompletion", "predict", "chat", "proposeEdits"]
requests = ["context.openBuffers"]   # optional: context.workspaceRead, context.symbols
network = true                       # declared; shown to the user, not enforced in v1
env = ["ANTHROPIC_API_KEY"]          # the only variables passed through from Tidepool's environment
```

## Context rules

- Tidepool assembles every byte an agent receives. Agents never get paths
  to read for themselves.
- **Deny-list** (always, in addition to `.tidepoolignore`): `.env*`,
  `*.pem`, `*.key`, `*.jks`, `*.keystore`, `*.p12`, `*.p8`,
  `*.mobileprovision`, `id_*`, `keystore.properties`, `.git/`, and any file
  the user marks private.
- Untrusted workspaces (ADR-0014) give agents only the focused buffer, and
  only with consent.
- Agent output is untrusted: sizes, ranges and document versions are
  validated; text is never executed or rendered as markup.
- Repository content can contain prompt injection aimed at agents. Because
  agents can only propose edits, the user reviews every change as a diff.

## Predictive text (`predict`)

Works with no agents and no network.

- Per-language token trigrams and identifier frequencies from open buffers
  and the workspace index, recency-weighted, with a memory cap.
- Word n-grams for Markdown and comments, plus a workspace word list.
- Exposed as completion items and, for the top prediction above a
  confidence threshold, as ghost text.

## Merging and display

- Sources: LSP completions (`lang`), local predictions, agent inline
  completions.
- Popup list for LSP and local items; ghost text for agent completions or
  the top local prediction. `Tab` accepts ghost text (Insert mode in vim
  mode).
- Requests are debounced (default 75 ms), cancelled on every keystroke,
  carry the buffer version, and time out (default 1.5 s). Results for an
  older version are dropped.
- One in-flight inline request per agent; at most two agents queried per
  keystroke, by user priority.

## Reference agents (`agents/`, separate workspace, std-only)

| Agent | What it does |
| :- | :- |
| `tp-agent-mock` | Deterministic fixtures, latency injection, malformed-output modes; used by tests |
| `tp-agent-local` | Plain HTTP to a user-run model server on `127.0.0.1`/`::1` only |
| `tp-agent-claude` | Anthropic's Messages API through `curl`; the API key comes from `ANTHROPIC_API_KEY` and reaches curl through `--config -` on stdin, never argv. Endpoints, headers and streaming format are taken from the current API docs at implementation time |

## Tests

- TAP conformance suite run against `tp-agent-mock`.
- Deny-list tests: files on the list never appear in any request.
- Latency: a mock agent that waits 2 s adds nothing to keystroke latency.
- Stale results are dropped after further typing.
- Crash/restart: the agent dies mid-request; the editor keeps working and
  the agent restarts with backoff.
- No API key appears in argv, logs or settings.
