---
paths:
  - "**/*.md"
  - "docs/**"
  - "diagrams/**"
  - "site/**"
  - "**/*.rs"
---

# Rules for writing documentation

These rules apply to everything written for people to read:
- `docs/`, with the roadmap, the ADRs, and the feature summaries in
  `docs/features.jsonc`;
- the READMEs, `AGENTS.md`, `CLAUDE.md`, `CONTRIBUTING.md`, and
  `SECURITY.md`;
- the diagrams and the demo page (`site/`, with `llms.txt`);
- Rust doc comments (`///` and `//!`), which `cargo doc` turns into the
  API reference.

Other files tell you which docs to update and how to keep them in shape:
- `CONTRIBUTING.md` → "Documentation" says which docs a change must
  update.
- `docs-and-roadmap.md` says how to keep their shape and their status.

This file says how to write them.

Write for two readers at once:
- a second-year computer science student who is new to Tidepool and to
  the parts of an IDE it builds from scratch;
- a senior engineer who needs every detail.

## Keep jargon down

- **Use a plain phrase** whenever it is just as exact.
- **Use a technical term only** when it carries meaning a plain phrase
  can't, or when the reader needs the term to search for it or use the
  tool.
- **Explain each term a student might not know** the first time it
  appears, in a short clause or parenthetical. Then use it freely. For
  example: "damage (the parts of a frame that changed and must be drawn
  again)", or "grapheme cluster (what a reader sees as one character,
  such as an e and the accent that combines with it)".
- **Spell out acronyms on first use,** as in "Tidepool Agent Protocol
  (TAP)" or "Language Server Protocol (LSP)". Universal ones such as
  HTML and API don't need it.
- **Drop insider shorthand:**
  - Avoid slang such as "footgun" and "yak shave".
  - An architecture decision record (ADR) or a roadmap task needs a
    clause saying what the record decided or what the task builds, not
    just its ID. Write "ADR-0039 (Xlib reaches the X server over a local
    Unix socket only, never TCP)", not "see ADR-0039", and "M3.7 (the
    list, tree view, and table)", not "the M3.7 widgets". A reference
    after a sentence that already says what was decided is fine, as in
    "`Debug` shows a pasted text's length only (ADR-0043)".
  - The clause names the decision. It does not copy the record's text
    (`docs-and-roadmap.md` → "Cite, don't restate").

## Don't give up meaning

- **Accuracy wins over simplicity.** Keep the exact numbers, limits,
  terms, and caveats: "at most 512 rows (`MAX_VIEW_ROWS`)", not "a few
  hundred rows".
- **If a simpler sentence would be wrong or misleading,** keep the precise
  wording and add an explanation beside it.
- **Don't remove details senior engineers need.** Explain them so
  everyone else can follow.

## Help newer readers follow

- **Explain why** a design choice was made, not only what it is.
- **Introduce one idea at a time,** with concrete examples. An analogy
  goes next to the real explanation, never in place of it.
- **Leave out "obviously" and "trivially".**
- **Keep paragraphs short.**
- **Write headings that make the page easy to skim:** "Why collections
  keep nodes only for the rows on screen", not "Notes".

## In doc comments

A reader lands on one item's page, not at the top of the crate.
- Explain a term once, in the module docs (`//!`) or on the item that
  defines it.
- Elsewhere, link to that explanation with an intra-doc link, such as
  ``[`Rope`]`` or ``[`Tree`]``, instead of repeating it on every item.
- `RUSTDOCFLAGS='-D warnings' cargo doc --no-deps` fails on a link that
  leads nowhere.

## Use American English

- **Spelling:**
  - -or, not -our: color, behavior, neighbor, honor
  - -er, not -re: center, centered
  - -ize, not -ise, where British English uses -ise: initialize,
    serialize, normalize
  - one l before -ed and -ing when the word's last syllable isn't
    stressed: canceled, labeled, modeling, tunneling (but controlled and
    compelled keep two)
  - and gray, catalog, spelled (not spelt), acknowledgment, and license
    as both noun and verb
- **Grammar:**
  - Write complete sentences whose subjects and verbs agree.
  - Keep one tense for the same kind of statement. For what exists, that
    is the present tense; what does not exist yet goes with the task that
    builds it (`docs-and-roadmap.md`).
- **Punctuation:**
  - Use the serial comma (the comma before the final "and" or "or" in a
    list of three or more), as in "numbers, limits, and caveats".
  - Most existing pages leave it out. Add it in the sentences you write
    or change.
- **Quote names exactly:** code identifiers, file names, the fields of
  APIs and file formats, command output, and third-party names keep their
  own spelling, even when it is British. ARIA's `aria-labelledby` keeps
  its two l's.
- **Fix only the text you change.** Don't sweep a whole page for spelling
  or commas as part of another task (`AGENTS.md`, rule 10: the smallest
  complete change). Mention the ones you notice in your summary instead.

## Before finishing: the two-reader check

1. Reread the draft as a second-year computer science student. Look for
   undefined terms and skipped steps.
2. Reread it as a senior engineer. Look for anything vague, oversimplified,
   or wrong.
