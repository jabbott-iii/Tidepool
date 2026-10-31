# Subsystem: `tidepool-syntax`, `tidepool-markdown`, `tidepool-rpc` and `tidepool-lang`

Language support: highlighting, Markdown, the JSON-RPC protocols (LSP, DAP,
TAP), and language sessions that tie them to managed tools. Rust and
Markdown are first-class first (ADR-0005).

## `tidepool-syntax`

| Module | Provides |
| :- | :- |
| `lexer` | Incremental lexer interface: re-lex from the nearest safe restart state before an edit; stop when states converge |
| `rust` | Hand-written Rust lexer: raw strings (`r#"…"#`), byte and C strings, nested block comments, doc comments, lifetimes vs char literals, attributes, macro invocations, numeric suffixes |
| `markdown` | Line-oriented Markdown highlighter (headings, emphasis, code spans and fences, links, lists, tables, front matter) |
| `toml` | TOML highlighter (needed for `Cargo.toml` and Tidepool's own files) |
| `pack` | Language packs (`languages/*.toml`): id, file globs, first-line patterns, comment tokens, brackets, auto-close pairs, indentation rules, declarative lexer rules on `tidepool-regex`, LSP server, debug adapter, formatter, agents |
| `brackets` | Bracket matching and rainbow depth |
| `indent` | Indentation rules and auto-indent |

Highlighting of buffers above 1 MB runs on a worker against a rope
snapshot; results carry the buffer version.

## `tidepool-markdown`

- CommonMark parser (target: every example in the CommonMark spec) plus
  GFM extensions: tables, task lists, strikethrough, autolinks.
- Produces a document tree with source ranges, used for the preview,
  outline, folding and link checking.
- No HTML rendering engine: inline HTML is shown as code in the preview.
- The preview follows jabbott.dev's `.prose` styles
  (`docs/subsystems/ui-and-theme.md`).

## `tidepool-rpc`

- JSON-RPC 2.0 over `Content-Length` framing, shared by LSP, DAP (which
  uses the same framing with its own message shapes) and TAP.
- Request IDs, cancellation (`$/cancelRequest`), timeouts, and progress.
- Message size cap (default 64 MiB) and depth limits from `codec::json`.
- Typed LSP messages for the features `lang` uses; unknown fields are
  ignored, unknown methods answered with `MethodNotFound`.
- Framing and message parsing are fuzz targets.

## `tidepool-lang`

| Module | Provides |
| :- | :- |
| `registry` | Language-pack registry; picks the language for a buffer |
| `session` | One LSP session per (server, workspace root): start through `toolchain::sessions`, `initialize` with `positionEncoding` negotiation (`utf-8` preferred, `utf-16` default), incremental sync, crash restart |
| `features` | Diagnostics, completion, hover, signature help, definition/declaration/type/implementation, references, rename, formatting, code actions, inlay hints, semantic tokens, document and workspace symbols, folding ranges |
| `rust` | rust-analyzer settings; build scripts and proc macros off in untrusted workspaces (ADR-0014); `rust-toolchain.toml` respected |
| `cargo` | Tasks: `check`, `build`, `test`, `run`, `clippy`, `fmt`, with `--message-format=json-diagnostic-rendered-ansi` parsed into problems; test discovery with `cargo test -- --list` |
| `markdown` | Outline, relative-link and anchor checks, table formatting, word count |
| `dap` | Debug sessions (M13): launch/attach, breakpoints, stack, scopes, variables, evaluate |

### Positions

LSP positions default to UTF-16 code units. All conversions go through
`tidepool-text`, and tests cover non-ASCII lines (combining marks, emoji,
CJK).

## Tests

- Golden token streams for the Rust, Markdown and TOML highlighters.
- Incremental lexing equals a full re-lex after random edits (property).
- CommonMark spec examples (test-only fixtures, licensed CC-BY-SA 4.0;
  see `docs/02-dependency-policy.md`).
- LSP client against the fake server: lifecycle, sync, cancellation,
  crash, malformed messages, UTF-16 positions.
- `TIDEPOOL_IT_RA=1`: rust-analyzer on Tidepool's own workspace reports
  diagnostics and resolves a known definition.
