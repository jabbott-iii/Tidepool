# tidepool-workspace

The project model under the editor: workspace folders and ignore rules,
workspace trust, layered settings, the file tree, project search, open
buffers and session state.

It is part of [Tidepool](https://github.com/jabbott-iii/Tidepool), where
`editor`, `lang` and `agent` use it; its buffers are `tidepool-text`
documents, and its settings are TOML read with `tidepool-codec`.

Layer 2, services. It may use `core`, `platform`, `codec`, `text` and
`regex`, as its row of the dependency matrix in
[`docs/01-architecture.md`](../../docs/01-architecture.md) says; it uses
`core`, `codec` and `text` so far.

## Status

Version 0.1, not published (`publish = false`): the API changes as the
roadmap needs.

Built, with the roadmap task that built it:

- `folder`: a workspace root opened by its canonical path, its
  `.tidepool/` folder, and its ignore files: a `.gitignore` or a
  `.tidepoolignore` in any folder, and the root's `.git/info/exclude`,
  read as gitignore(5) says Git reads them, on bytes, with case kept.
  `.git` is always ignored, nothing inside an ignored folder can be
  included again, and a `.tidepoolignore` wins over every `.gitignore`.
  Each pattern is matched without backtracking, in time proportional to
  the path's length times the pattern's. An ignore file is untrusted
  input: it must be a regular file, in a folder reached from the root,
  neither through a symbolic link, of at most 256 KiB, 10,000 patterns
  and 4,096 bytes a pattern, and the files one `Ignore` holds at most
  1 MiB together — M4.7 (ADR-0067).
- `trust`: the folders the user trusts (ADR-0014), in `trust.toml` in the
  user data folder; a grant trusts its folder and every folder inside it,
  a folder is untrusted until then, and a trust file that cannot be used
  grants nothing and is never overwritten — M4.7.
- `settings`: typed settings that each part of Tidepool declares in a
  `Schema` (a boolean, a whole number in a range, a string from a list of
  choices, or a table), in layers: the defaults, the user's
  `settings.toml`, and a folder's `.tidepool/settings.toml`, which is not
  even read until the folder is trusted, and never through a symbolic
  link. A value that is not valid falls
  back to the layer below with a warning naming the file and the key; a
  value written back goes through `codec::toml`'s document model, so the
  file keeps its comments and unknown keys, and is saved through a
  temporary file and a rename — M4.7.
- two fuzz targets, `ignore` and `settings` (which also reads its input
  as a trust file), in `tests/fuzz.rs` — M4.7.

Not yet, with the roadmap task that adds it:

- `buffers`, `session`, and the file tree model in `files` — M4.3.
- `search`, and the file finder's index in `files` — M4.4.
- `watch` — M4.5.

Tasks and their state are in [`docs/roadmap.md`](../../docs/roadmap.md).

## Usage

A schema, the user's settings over the defaults, and a value written back
in memory with the file's comment kept. `SettingsFile::user` and
`SettingsFile::write` do the same with the files on disk, on a worker.

```rust
use tidepool_workspace::settings::{Kind, Schema, Scope, Settings, SettingsFile, Source, Value};

let mut schema = Schema::new();
schema.add("ui.side_panel", Kind::Bool { default: true })?;
schema.add("editor.tab_size", Kind::Integer { default: 4, min: 1, max: 16 })?;

let text = "# mine\n[editor]\ntab_size = 99\n";
let mut user = SettingsFile::from_bytes(Scope::User, "settings.toml", Some(text.as_bytes()));
let settings = Settings::resolve(&schema, Some(&user), None);
assert_eq!(settings.integer("editor.tab_size"), Some(4)); // 99 is out of range
assert_eq!(settings.warnings().len(), 1);

user.set(&schema, "editor.tab_size", &Value::Integer(2))?;
let saved = user.document().map(|document| document.as_str());
assert_eq!(saved, Some("version = 1\n\n# mine\n[editor]\ntab_size = 2\n"));
let settings = Settings::resolve(&schema, Some(&user), None);
assert_eq!(settings.source("editor.tab_size"), Some(Source::User));
# Ok::<(), tidepool_workspace::WorkspaceError>(())
```

## Documentation

- Design: [`docs/subsystems/editor.md`](../../docs/subsystems/editor.md)
  → `tidepool-workspace`
- Decisions in [`docs/decisions.md`](../../docs/decisions.md): ADR-0067
  (M4.7's folders, ignore files, trust and settings); ADR-0014
  (untrusted until trusted); ADR-0013 (settings are TOML); ADR-0031
  (the document model keeps the text it does not change); ADR-0025 (the
  atomic save follows links).
- API reference: `cargo doc -p tidepool-workspace --open`

## Rules

- `unsafe` is denied by the workspace lint, and in doc tests by the crate
  root's `#![doc(test(attr(forbid(unsafe_code))))]` (ADR-0036);
  `cargo xtask unsafe-audit` checks both.
- Ignore files, settings and trust files are parsers of untrusted input:
  they take limits, never panic, and have fuzz targets (ADR-0027). A
  workspace's own settings are read only once it is trusted (ADR-0014).
- Reading and writing files blocks, so it runs on workers, never on the
  main thread (`AGENTS.md` rule 6).
- Path-scoped rules in `.claude/rules/`:
  [`dependencies.md`](../../.claude/rules/dependencies.md) for its
  `Cargo.toml`.

## Checks

```bash
cargo test -p tidepool-workspace
cargo xtask fuzz-smoke --target ignore
cargo xtask fuzz-smoke --target settings
```

`cargo xtask ci` runs the whole gate.

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
