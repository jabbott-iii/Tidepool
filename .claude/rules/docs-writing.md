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
  API reference;
- commit messages and pull request descriptions, whose form is in
  "Commit messages" below.

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

## Commit messages

Commits follow [Conventional Commits
1.0.0](https://www.conventionalcommits.org/en/v1.0.0/) from 2026-11-22
on. Earlier commits keep the older form, with the area first, as in
`ui: overlays (M3.3)`. Never rewrite history to change them (`AGENTS.md`,
rule 11).

- **The subject** is `<type>(<scope>): <summary> (<IDs>)`, for example:
  - `feat(text): encodings and atomic save (M1.3)`
  - `fix(platform,core): CI on ubuntu-26.04 and macOS (ADR-0050, ADR-0051)`
  - `docs: accept ADR-0017 (xtask runs cargo itself)`
- **The type** says what kind of change the commit is. The spec defines
  `feat` and `fix`. The others are the ones the spec cites from
  `@commitlint/config-conventional` (the Angular convention):

  | Type | Use it for |
  | :- | :- |
  | `feat` | Something new Tidepool can do, such as a roadmap task's code |
  | `fix` | A bug fixed, a failing check or CI run included |
  | `docs` | Docs only: the specs, ADRs, the roadmap, diagrams, READMEs, rules, and skills |
  | `test` | Tests only: test code, golden references, and fuzz seeds |
  | `refactor` | Code changed with no change in behavior |
  | `perf` | Code made faster or smaller, with no change in behavior |
  | `build` | Manifests, the lockfile, the toolchain pin, and `.cargo/` |
  | `ci` | `.github/`: workflows, Dependabot, and templates |
  | `style` | Formatting only, such as `cargo fmt` |
  | `chore` | Anything else that changes no code Tidepool ships |

  A commit that does several of these takes the type of its main
  purpose: a task's code, with its tests and docs, is `feat`.
- **The scope** is the area the commit changes: a crate's short name
  (`ui`, `text`, `platform`), `xtask`, `bench`, `site`, or `fuzz`. Write
  two areas with a comma and no space, `(platform,core)`. A change to
  more than two is probably mis-scoped (`CLAUDE.md` → "Working style"),
  unless it spans the whole repository, which takes no scope.
- **The summary** names what the commit brings, as a phrase, the way the
  roadmap names its tasks. It starts in lowercase unless it starts with a
  name or an identifier, and has no final period.
- **The IDs:** the roadmap task, and the ADRs the commit records or
  follows, go in parentheses at the end. Keep the subject to about 72
  characters, so `git log --oneline` stays readable; when the ADRs make
  it longer, name them in the body instead.
- **A breaking change** of a public API, a file format, a protocol,
  configuration, data, or behavior is allowed only when the task
  authorizes it, and is recorded in an ADR (`AGENTS.md`, rule 10). Its
  commit puts `!` before the colon, as in `feat(ui)!: …`, and adds a
  `BREAKING CHANGE:` footer that says what breaks and names the ADR. The
  spec accepts either; Tidepool uses both, so `git log --oneline` shows
  the break and the footer explains it.
- **The body** starts one blank line after the subject, wraps at 72
  columns, and says what changed and why, in the plain words the rest of
  this file asks for.
- **Footers** follow the body after a blank line, in the git trailer
  form the spec uses: a token with `-` in place of spaces, a colon and a
  space, then the value, as in `Refs: #123`. Use only footers about the
  change itself, such as `BREAKING CHANGE:` and `Refs:`. Never add one
  that credits an agent or a tool, such as a `Co-Authored-By:` line for
  it, a `Claude-Session:` line, or a "Generated with …" line (`AGENTS.md`,
  rule 11).
- **Case:** types and scopes are lowercase. `BREAKING CHANGE` is
  uppercase, as the spec requires.
- **Pull requests:** a pull request's title takes the same form as a
  subject. Its description follows `.github/pull_request_template.md` and
  the rest of this file.

## Before finishing: the two-reader check

1. Reread the draft as a second-year computer science student. Look for
   undefined terms and skipped steps.
2. Reread it as a senior engineer. Look for anything vague, oversimplified,
   or wrong.
