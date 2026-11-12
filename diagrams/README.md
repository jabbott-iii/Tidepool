# Diagrams

Pictures of how Tidepool fits together, drawn from the code and the docs as
of roadmap task M0.13 (2026-11-08, version 0.1). Each file is Markdown with
Mermaid blocks, which GitHub draws. Most of Tidepool is still a design, so
the diagrams draw both: what the code has is solid, and what the roadmap
plans is dashed and labelled with the task that builds it (ADR-0029).

| File | Shows |
| :- | :- |
| [`c4-architecture.md`](c4-architecture.md) | C4 views: the containers (the `tidepool` binary and the dev tools, the files they use, the managed tools and agents they drive) and the components inside them: the crates by layer with the dependency matrix's edges, `xtask`'s modules, and the fuzz engine and its targets |
| [`class-diagrams.md`](class-diagrams.md) | UML class diagrams of the types the code has, in `core`, `codec`, `text`, `xtask`, `bench` and `sys`, and a list of the main types the specs name for the crates still to come |
| [`component-diagrams.md`](component-diagrams.md) | UML component diagrams: each subsystem's crates with the interfaces they provide and the ones they require |
| [`sequence-diagrams.md`](sequence-diagrams.md) | UML sequence diagrams: `cargo xtask ci`, `fuzz-smoke` and a fuzz run, loading and saving a file, an edit with undo and redo; and, planned, startup, a keystroke, an LSP request, a tool install, an agent's proposal and an Android build |
| [`data-flow-and-state.md`](data-flow-and-state.md) | Data-flow diagrams of a file, a tool install, an agent's context and fuzz smoke; state diagrams of a fuzz run, the undo history, a saved file, workspace trust, vim modes, a managed tool and a language session |

## Reading them

- Each file starts with its legend. In the flowcharts, blue boxes are what
  is built, grey ones are outside Tidepool, and white boxes with a dashed
  border are planned; a dotted arrow is a planned use. Sequence and state
  diagrams of planned work say so in their titles and notes. A planned
  element names its roadmap task (`planned M5.2`), whose state is in
  [`docs/roadmap.md`](../docs/roadmap.md).
- Crate edges are the dependency matrix's in
  [`docs/01-architecture.md`](../docs/01-architecture.md), and no other.
- Planned parts use the names their specs in
  [`docs/subsystems/`](../docs/subsystems/) give; the specs, not the
  diagrams, are the design.

## Keeping them true

When the code and a diagram disagree, the code is right. A change to what
a diagram draws, such as a crate's dependencies, a type drawn here or a
flow, updates the diagram in the same change, and a task that builds
something drawn as planned makes it solid (`AGENTS.md` → "Definition of
done"). No check reads these files: after changing a block, see that it
still draws, in GitHub's preview or another Mermaid renderer.
