// Copyright 2026 Joseph Anthony Abbott III
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! `cargo xtask unsafe-audit`: `unsafe` stays in `tidepool-sys` and
//! `tidepool-platform`, and each use there is documented.
//!
//! Ported from nadir xtask `unsafe_audit.rs`. Adapted for Tidepool: two
//! crates instead of three, `// SAFETY:` must be a line comment in the same
//! statement as the block, and the `# Safety` and `extern` block rules
//! below are new.
//!
//! The audit is specified in `docs/04-quality.md` → "`unsafe-audit`". It
//! checks `AGENTS.md` rule 4 and the mechanically checkable items of that
//! document's unsafe and FFI review checklist (declarations live only in
//! `sys`; every `pub unsafe fn` has a `# Safety` section). As implemented
//! here, a violation is reported for each of these:
//!
//! - **`unsafe` outside the two crates**: the keyword in any form (block,
//!   `fn`, `impl`, `trait`, `extern`, `#[unsafe(...)]`) in a file that is
//!   not under `crates/sys/` or `crates/platform/`, or is in a package
//!   nested in either directory, which is no part of the crate (ADR-0038).
//!   Every rule that names the two crates scopes them so. Each file is read
//!   with CR LF as LF, as rustc reads it.
//! - **An undocumented `unsafe` block or `unsafe impl`** in the two crates:
//!   no `//` comment whose text starts with `SAFETY:` between the end of
//!   the previous statement (the last `;`, `{` or `}`) and the keyword. Doc
//!   comments and `/* */` comments do not count.
//! - **The `unsafe_code` lint relaxed** (`allow`, `expect` or `warn`)
//!   anywhere but `crates/sys/src/lib.rs` and `crates/platform/src/lib.rs`.
//! - **A `pub unsafe fn` without `# Safety`** in the two crates (elsewhere
//!   the keyword alone is a violation), in any visibility that starts with
//!   `pub`, with or without `const`, `async` or an `extern` ABI, including
//!   declarations in `extern` blocks: its outer doc comments (or `#[doc =
//!   "..."]` attributes) have no `# Safety` line.
//! - **An `extern` block outside `tidepool-sys`**, or one in `sys` that is
//!   not declared `unsafe extern`.
//! - **An exported macro that takes its caller's tokens and holds
//!   `unsafe`**: in the two crates, a `#[macro_export]` `macro_rules!`
//!   (also through `cfg_attr`) whose body has a metavariable (`$e`,
//!   `$(...)`, not `$crate`) and `unsafe` in any form ([`File::unsafe_form`]:
//!   a block, `fn`, `impl`, `trait`, `extern` block, `#[unsafe(...)]`, a bare
//!   keyword), each one reported, wherever the metavariable is (ADR-0037).
//!   Expanded in another crate, the caller's code could run as `unsafe`
//!   with no `unsafe` token there: rustc does not apply the caller's
//!   `unsafe_code` lint to code from another crate's macro, so neither the
//!   lint nor this audit would see it, and no `// SAFETY:` comment in the
//!   macro can cover what callers pass. A body that does not close runs to
//!   the end of the file.
//! - **A library crate root that does not forbid `unsafe` in doc tests**
//!   (ADR-0036): the crate root of a library target (`[lib]`, of any crate
//!   type, [`LIBRARY_KINDS`]) of a member of a workspace under the root,
//!   other than the two crates' own, without [`FORBID_IN_DOC_TESTS`] as an
//!   inner attribute at crate level. rustdoc does not apply the workspace's
//!   `unsafe_code` lint to the doc tests it compiles, so the attribute has
//!   the compiler forbid `unsafe` in each of them. It must be written
//!   exactly so, token for token, at nesting depth 0: any other spelling
//!   counts as missing (under `cfg_attr`, with another lint in the same
//!   `attr`, `deny`, an outer `#[...]`, inside a `mod`, a raw identifier, a
//!   combining mark that makes another word).
//! - **A proc-macro crate** (ADR-0037): a target of kind [`PROC_MACRO`] in
//!   any workspace under the root, the two crates included. A proc macro
//!   can make the `unsafe` keyword from a string, with a span for which
//!   rustc skips the `unsafe_code` lint, and this audit reads only the
//!   string. That is its only violation: a proc macro is no library, so the
//!   rule above does not ask it for the attribute.
//! - **An exported macro that lets its caller's `unsafe` reach braces or an
//!   item it writes** (ADR-0038), outside the two crates: in a transcriber,
//!   a `tt` or `ident` metavariable, or a repetition that holds one, right
//!   before a `{`, `fn`, `impl`, `trait`, `extern` or `static` the macro
//!   writes, or as the name of an attribute it writes
//!   ([`File::caller_unsafe`]). rustc skips the `unsafe_code` lint for
//!   braces or an item from another crate's macro.
//! - **A macro that defines a macro** (ADR-0038), in any crate: a body that
//!   holds `macro_rules`, or `macro_rules` passed to a macro. What such a
//!   macro makes is never written out for a rule here to read.
//! - **`include_str` as a name of its own** (ADR-0038), outside the two
//!   crates: defined or imported, it would take the standard macro's place
//!   in `doc = include_str!(...)`.
//! - **`unsafe` in a code block of the documentation** (ADR-0037), in a
//!   file outside the two crates: in doc comments, in `#[doc = "..."]`
//!   strings, and in files that `doc = include_str!(...)` includes from
//!   inside the package ([`Audit::doc_includes`]), read by [`doc_tests`],
//!   which finds every fenced block, whatever its info string, and every
//!   indented one as CommonMark does, closely enough. The compiler cannot
//!   see `unsafe` that a doc test passes to another crate's exported macro
//!   when that macro writes the braces after it. A `doc =` value of another
//!   form, or an included file the audit cannot read, is a violation too.
//!   Since the macro rule above bans that macro shape at its source, this
//!   reader is a second layer (ADR-0038).
//!
//! The rules are only as good as the set of files they are applied to, and
//! the compiler reads more than the `.rs` files a walk finds. So the audit
//! also fails, closed, on whatever could bring code in that it did not
//! read:
//!
//! - **A symbolic link**, any, anywhere under the root: links are neither
//!   followed nor resolved ([`walk::Tree::links`]).
//! - **`#[path = "..."]` or `include!("...")`** (in any spelling: inside
//!   `cfg_attr`, as a raw identifier, through `std::`) whose argument is not
//!   a relative `.rs` path that stays in the file's own directory: no `..`,
//!   no absolute path, no hidden or `target` directory (in any case, or
//!   with a trailing `.` or space, which Windows drops), no `~` (an NTFS
//!   short name), no escapes. A file such a path names lies in the same
//!   crate and is audited.
//! - **A crate root the audit did not read**: a target of a workspace
//!   member (library, binary, test, example, bench, build script) whose
//!   `src_path`, as `cargo metadata` reports it, is not one of the audited
//!   files, such as `[lib] path = "src/lib.txt"`. A workspace cargo cannot
//!   describe offline is a violation too.
//! - **A crate root outside its package directory**: a target of a
//!   workspace member, of any kind, whose `src_path`, with links and `..`
//!   resolved, is not inside the directory of its package's `Cargo.toml`,
//!   such as `[lib] path = "../../lib.rs"`. From there a plain `mod target;`
//!   or `mod worktrees;` could compile a directory the walk skips (a
//!   workspace root's `target/`, `.claude/worktrees/`).
//! - **A skipped linked worktree inside a member's package directory**: a
//!   plain `mod` declaration of that crate could compile module files from
//!   it, which the walk skipped.
//! - **A file larger than the size bound, or not UTF-8**, a `.rs` file or a
//!   file included as documentation, and an included file named like a
//!   secret, which `xtask` never opens (`AGENTS.md` rule 7).
//! - **`crates/sys/src/lib.rs` or `crates/platform/src/lib.rs` not found**:
//!   the root is wrong, and an audit of the wrong directory would otherwise
//!   pass by finding nothing.
//!
//! Hidden directories are audited; only the repository's `.git`, the
//! `target` directory of a workspace root and linked worktrees of the
//! repository are skipped ([`walk::scan`]): build output anywhere else is
//! audited. Names are matched as the strictest file system would:
//! a file is Rust source whatever the case of its `.rs`, since macOS and
//! Windows open `x.RS` for `mod x;`.
//!
//! The audit reads source text, so it also covers code that the compiler
//! skips on this OS (other `cfg(target_os)` branches) and code in macros.
//! It does not check where `cfg(target_os)` appears (`AGENTS.md` rule 5):
//! `docs/04-quality.md` gives that to no check. Nor does it read a path
//! package that is not a member of its workspace, such as one under a
//! workspace root's `target/`, which the walk skips: `cargo xtask deps`
//! fails such a package, and the audit relies on it.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::check::Report;
use crate::deps;
use crate::error::Error;
use crate::lexer::{self, Comment, CommentKind, Doc, Lexed, Token, TokenKind};
use crate::metadata::{self, Resolve};
use crate::walk;

mod doc_tests;

use doc_tests::{DocLine, Fragment, Place};

/// The crates that may contain `unsafe`: package name and directory,
/// relative to the repository root.
const UNSAFE_CRATES: [(&str, &str); 2] = [
    ("tidepool-sys", "crates/sys"),
    ("tidepool-platform", "crates/platform"),
];

/// Which of [`UNSAFE_CRATES`] may declare `extern` blocks.
const DECLARATIONS: usize = 0;

/// The same crates, for messages.
const UNSAFE_NAMES: &str = "tidepool-sys and tidepool-platform";

/// The file in each of [`UNSAFE_CRATES`] that may relax the `unsafe_code`
/// lint, relative to the crate's directory.
const CRATE_ROOT: &str = "/src/lib.rs";

/// The kinds `cargo metadata` gives a library target: its crate types.
/// Cargo runs the doc tests of a `lib` or `rlib` library, and none of one
/// that is only `dylib`, `cdylib` or `staticlib`. All of them count, since
/// one more crate type (`crate-type = ["cdylib", "rlib"]`) starts its doc
/// tests, and the attribute costs nothing where there are none; for the
/// same reason a library's `doctest = false` is not read. An example with a
/// crate type is `example` here, and gets no doc tests. A `proc-macro`
/// target is no library here: none may exist ([`PROC_MACRO`]).
const LIBRARY_KINDS: [&str; 5] = ["lib", "rlib", "dylib", "cdylib", "staticlib"];

/// The kind `cargo metadata` gives a proc-macro crate's target, which no
/// workspace under the root may have, `sys` and `platform` included
/// (ADR-0037): a proc macro can make the `unsafe` keyword from a string,
/// with a span for which rustc skips the `unsafe_code` lint.
const PROC_MACRO: &str = "proc-macro";

/// The inner attribute with which a library's crate root forbids `unsafe`
/// in its doc tests (ADR-0036), exactly as it must be written, token for
/// token.
const FORBID_IN_DOC_TESTS: &str = "#![doc(test(attr(forbid(unsafe_code))))]";

/// The largest source file audited, in bytes. A larger one is a violation:
/// the audit fails closed instead of skipping it.
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

/// Runs the audit on every `.rs` file under `root`, then checks that every
/// crate root of every workspace under it was among them.
///
/// # Errors
///
/// [`Error::Read`] if a directory or file cannot be read.
pub(crate) fn run(root: &Path) -> Result<Report, Error> {
    let (mut audit, manifests) = audit_tree(root, MAX_FILE_BYTES)?;
    audit.crate_roots(root, &manifests);
    Ok(audit.report())
}

/// Audits every `.rs` file under `root` of at most `limit` bytes, and
/// returns the audit with the `Cargo.toml` files found on the way.
fn audit_tree(root: &Path, limit: u64) -> Result<(Audit, Vec<PathBuf>), Error> {
    let tree = walk::scan(root, |name| {
        is_rust_source(name) || metadata::is_manifest_name(name)
    })?;
    let mut audit = Audit {
        worktrees: walk::worktrees_note(root, &tree.worktrees),
        build_dir: walk::build_dir_note(root, &tree),
        worktree_dirs: tree.worktrees.clone(),
        limit,
        ..Audit::default()
    };
    for link in &tree.links {
        audit.violations.push(format!(
            "{}: a symbolic link, which the audit does not follow: the code it leads to was not \
             audited",
            walk::relative(root, link)
        ));
    }
    let (manifests, sources): (Vec<PathBuf>, Vec<PathBuf>) =
        tree.files.into_iter().partition(|path| {
            path.file_name()
                .is_some_and(|name| metadata::is_manifest_name(&name.to_string_lossy()))
        });
    // A manifest in one of the two crates' directories, other than the
    // crate's own, makes its directory a package of its own (ADR-0038).
    audit.nested = manifests
        .iter()
        .filter_map(|manifest| {
            let dir = walk::relative(root, manifest.parent()?);
            UNSAFE_CRATES
                .iter()
                .any(|(_, crate_dir)| {
                    dir.strip_prefix(crate_dir)
                        .is_some_and(|rest| rest.starts_with('/'))
                })
                .then(|| format!("{dir}/"))
        })
        .collect();
    for path in sources {
        let shown = walk::relative(root, &path);
        let resolved = metadata::canonical(&path);
        match walk::read_bounded(&path, limit) {
            Ok(text) => {
                let forbids = audit.file(&shown, &resolved, &text);
                audit.read.insert(resolved.clone(), forbids);
            }
            Err(walk::ReadError::TooLarge { limit }) => audit.violations.push(format!(
                "{shown}: larger than {limit} bytes, so it was not audited"
            )),
            Err(walk::ReadError::NotUtf8) => audit
                .violations
                .push(format!("{shown}: not UTF-8, so it was not audited")),
            Err(walk::ReadError::Io(source)) => return Err(Error::Read { path, source }),
        }
        audit.scanned.insert(resolved);
    }
    Ok((audit, manifests))
}

/// The findings of an audit in progress.
#[derive(Debug, Default)]
struct Audit {
    /// `unsafe` keywords seen in each crate of [`UNSAFE_CRATES`].
    uses: [usize; UNSAFE_CRATES.len()],
    /// Which crate roots of [`UNSAFE_CRATES`] have been seen.
    roots: [bool; UNSAFE_CRATES.len()],
    /// How many files have been audited.
    files: usize,
    /// The resolved paths of the files the walk found.
    scanned: BTreeSet<PathBuf>,
    /// The resolved paths of the files the audit read, which leaves out
    /// those too large or not UTF-8, each with whether it carries
    /// [`FORBID_IN_DOC_TESTS`] at crate level.
    read: BTreeMap<PathBuf, bool>,
    /// How many crate roots were checked.
    crate_roots: usize,
    /// How many library crate roots had to forbid `unsafe` in doc tests.
    library_roots: usize,
    /// How many code blocks of documentation were read for `unsafe`.
    doc_blocks: usize,
    /// The documented items that include a file with `doc =
    /// include_str!(...)`, read once the package that holds them is known.
    pending: Vec<Pending>,
    /// The largest file read, in bytes: a `.rs` file, or a file included as
    /// documentation.
    limit: u64,
    /// The directories, relative to the root and ending in `/`, of packages
    /// nested in the directory of one of [`UNSAFE_CRATES`]: their files are
    /// not that crate's.
    nested: Vec<String>,
    /// The note naming the linked worktrees the walk skipped, if any.
    worktrees: Option<String>,
    /// The note saying the walk searched the build directory the
    /// environment names, if it did.
    build_dir: Option<String>,
    /// The linked worktrees the walk skipped.
    worktree_dirs: Vec<PathBuf>,
    violations: Vec<String>,
}

impl Audit {
    /// Audits one file. `path` is relative to the repository root, with `/`
    /// between components, and `real` is the file, resolved. Returns whether
    /// the file carries [`FORBID_IN_DOC_TESTS`] at crate level
    /// ([`File::forbids_unsafe_in_doc_tests`]), which
    /// [`Audit::crate_roots`] asks of a library's crate root.
    fn file(&mut self, path: &str, real: &Path, text: &str) -> bool {
        self.files += 1;
        // rustc reads CR LF as LF before it lexes a file, so its doc
        // comments are read as rustc reads them.
        let normalized = text.replace("\r\n", "\n");
        let text = normalized.as_str();
        let nested = self.nested.iter().any(|dir| path.starts_with(dir.as_str()));
        let in_crate = |(_, dir): &(&str, &str)| {
            !nested
                && path
                    .strip_prefix(dir)
                    .is_some_and(|rest| rest.starts_with('/'))
        };
        let unsafe_crate = UNSAFE_CRATES.iter().position(in_crate);
        let root_of = UNSAFE_CRATES
            .iter()
            .position(|(_, dir)| path.strip_prefix(dir) == Some(CRATE_ROOT));
        if let Some(seen) = root_of.and_then(|index| self.roots.get_mut(index)) {
            *seen = true;
        }

        let lexed = lexer::lex(text);
        let file = File::new(&lexed);
        for (index, token) in lexed.tokens.iter().enumerate() {
            let line = token.line;
            if token.is_word("unsafe") {
                let Some(uses) = unsafe_crate.and_then(|index| self.uses.get_mut(index)) else {
                    self.violations.push(format!(
                        "{path}:{line}: `unsafe` is only allowed in {UNSAFE_NAMES}"
                    ));
                    continue;
                };
                *uses += 1;
                let what = match lexed.tokens.get(index + 1) {
                    Some(next) if next.is_punct("{") => Some("block"),
                    Some(next) if next.is_word("impl") => Some("impl"),
                    _ => None,
                };
                if let Some(what) = what
                    && !file.has_safety_comment(index)
                {
                    self.violations.push(format!(
                        "{path}:{line}: `unsafe` {what} has no `// SAFETY:` comment before it \
                         in the same statement"
                    ));
                }
                if file.is_pub_unsafe_fn(index) && !file.has_safety_section(index) {
                    self.violations.push(format!(
                        "{path}:{line}: `pub unsafe fn` has no `# Safety` section in its doc \
                         comment"
                    ));
                }
            } else if token.is_word("extern") && file.is_extern_block(index) {
                if unsafe_crate != Some(DECLARATIONS) {
                    self.violations.push(format!(
                        "{path}:{line}: `extern` block outside tidepool-sys: raw OS \
                         declarations live only there"
                    ));
                } else if !index
                    .checked_sub(1)
                    .and_then(|before| lexed.tokens.get(before))
                    .is_some_and(|before| before.is_word("unsafe"))
                {
                    self.violations.push(format!(
                        "{path}:{line}: `extern` block is not declared `unsafe extern`"
                    ));
                }
            } else if token.is_name("unsafe_code")
                && root_of.is_none()
                && let Some(level @ ("allow" | "expect" | "warn")) = file.lint_level(index)
            {
                self.violations.push(format!(
                    "{path}:{line}: `{level}(unsafe_code)` is only allowed in the crate roots \
                     of {UNSAFE_NAMES}"
                ));
            } else if token.is_name("macro_rules") {
                self.macro_rules(path, &file, index, unsafe_crate.is_some());
            } else if token.is_name("include_str")
                && unsafe_crate.is_none()
                && !lexed
                    .tokens
                    .get(index + 1)
                    .is_some_and(|next| next.is_punct("!"))
            {
                self.violations.push(format!(
                    "{path}:{line}: `include_str` as a name of its own, which defines or imports \
                     something named like the standard macro: a macro of that name would take \
                     its place in `doc = include_str!(...)`, and the audit would read another \
                     file than rustdoc does (ADR-0038)"
                ));
            } else if let Some(what) = file.file_reference(index)
                && !file.names_audited_file(index)
            {
                self.violations.push(format!(
                    "{path}:{line}: {what} does not name a relative `.rs` path inside this \
                     file's directory (no `..`, no hidden or `target` directory, no escapes), so \
                     the code it brings in may not be audited"
                ));
            }
        }
        if unsafe_crate.is_none() {
            self.documentation(path, real, &lexed);
        }
        file.forbids_unsafe_in_doc_tests()
    }

    /// The rules for the `macro_rules` at `index` of the file at `path`:
    /// anywhere, no macro defines a macro (ADR-0038); in the two crates
    /// (`in_unsafe_crate`), no exported macro that takes its caller's tokens
    /// holds `unsafe` ([`File::exported_unsafe_wrappers`]); elsewhere, no
    /// exported macro lets its caller's `unsafe` reach braces or an item it
    /// writes ([`File::caller_unsafe`]). A macro named `include_str` is the
    /// rule of [`Audit::file`] on that name.
    fn macro_rules(
        &mut self,
        path: &str,
        file: &File<'_, '_>,
        index: usize,
        in_unsafe_crate: bool,
    ) {
        let line = file.token(index).map_or(0, |token| token.line);
        let Some(definition) = file.macro_definition(index) else {
            // Not a definition: a name of something else, which is fine,
            // unless it is passed to a macro, which could define a macro
            // with it.
            if file.in_macro_call(index) {
                self.violations.push(format!(
                    "{path}:{line}: `macro_rules` passed to a macro, which could define a macro \
                     with it that no rule here reads (ADR-0038)"
                ));
            }
            return;
        };
        let name = definition.name;
        for inner in definition.body.clone().filter(|&at| {
            file.token(at)
                .is_some_and(|token| token.is_name("macro_rules"))
        }) {
            let inner_line = file.token(inner).map_or(line, |token| token.line);
            self.violations.push(format!(
                "{path}:{inner_line}: the macro `{name}` defines a macro (its body holds \
                 `macro_rules`): what it makes is never written out, so no rule here can read it \
                 (ADR-0038)"
            ));
        }
        if in_unsafe_crate {
            for (line, what, name) in file.exported_unsafe_wrappers(index) {
                self.violations.push(format!(
                    "{path}:{line}: `unsafe` {what} in the exported macro `{name}`, which takes \
                     tokens from its caller (`$...`): they could run as `unsafe` in the caller's \
                     crate, where neither its `unsafe_code` lint nor this audit sees them, and no \
                     `// SAFETY:` comment here can cover them. Make it a function, or leave the \
                     `unsafe` to the caller"
                ));
            }
            return;
        }
        if definition.exported {
            for (line, variable, fragment, place) in file.caller_unsafe(&definition) {
                self.violations.push(format!(
                    "{path}:{line}: the exported macro `{name}` puts `${variable}`, a `{fragment}` \
                     fragment its caller can make the keyword `unsafe`, {place}: rustc does not \
                     apply the caller's `unsafe_code` lint to braces or an item from another \
                     crate's macro, in doc tests or in code. Use a fragment that cannot be \
                     `unsafe` (`path`, `ty`, `vis`), or leave the `unsafe` to the caller \
                     (ADR-0038)"
                ));
            }
        }
    }

    /// Reads the documentation of the file at `path` (resolved, `real`) for
    /// `unsafe` in its code blocks (ADR-0037), and reports the `doc =`
    /// values it cannot read. An item that includes a file waits for
    /// [`Audit::doc_includes`].
    fn documentation(&mut self, path: &str, real: &Path, lexed: &Lexed<'_>) {
        let docs = doc_tests::gather(lexed);
        for line in docs.unreadable {
            self.violations.push(format!(
                "{path}:{line}: a `doc =` value that is not a string literal or `include_str!` of \
                 one, which the audit cannot read, so the doc tests it may hold were not checked \
                 for `unsafe` (ADR-0037)"
            ));
        }
        for item in docs.items {
            if item
                .iter()
                .any(|fragment| matches!(fragment, Fragment::Include { .. }))
            {
                self.pending.push(Pending {
                    path: path.to_string(),
                    real: real.to_path_buf(),
                    item,
                });
                continue;
            }
            let fragments = item
                .into_iter()
                .filter_map(|fragment| match fragment {
                    Fragment::Text { lines, sugared } => Some((lines, sugared)),
                    Fragment::Include { .. } => None,
                })
                .collect();
            self.doc_tests(path, fragments);
        }
    }

    /// Reports each `unsafe` in the code blocks of one item's documentation,
    /// from the file at `path`.
    fn doc_tests(&mut self, path: &str, fragments: Vec<(Vec<DocLine>, bool)>) {
        for block in doc_tests::code_blocks(fragments) {
            self.doc_blocks += 1;
            let text = block
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            for token in lexer::lex(&text).tokens {
                let Some(place) = token
                    .is_word("unsafe")
                    .then(|| token.line.checked_sub(1).and_then(|index| block.get(index)))
                    .flatten()
                    .map(|line| &line.at)
                else {
                    continue;
                };
                let tail = format!(
                    "a code block in the documentation, which rustdoc may compile as a doc test, \
                     holds `unsafe`, which only {UNSAFE_NAMES} may use: every block counts, \
                     `ignore` and `text` too, since the `unsafe_code` lint can miss `unsafe` that \
                     another crate's macro expands. Inline code in prose is fine (ADR-0037)"
                );
                self.violations.push(match place {
                    Place::Here(line) => format!("{path}:{line}: {tail}"),
                    Place::Included { file, line, by } => {
                        format!("{file}:{line}: included as documentation by {path}:{by}: {tail}")
                    }
                });
            }
        }
    }

    /// Reads the documentation that waited for its included files
    /// ([`Audit::documentation`]), now that `packages` (each member's
    /// resolved directory and name) are known. An included file must lie in
    /// the package directory that holds the `.rs` file, the deepest one if
    /// several do, reached by a relative path without links
    /// ([`is_included_path`]), and be a UTF-8 file within the size bound.
    fn doc_includes(&mut self, real_root: &Path, packages: &[(PathBuf, String)]) {
        for pending in std::mem::take(&mut self.pending) {
            let package = packages
                .iter()
                .filter(|(dir, _)| pending.real.starts_with(dir))
                .max_by_key(|(dir, _)| dir.components().count())
                .map(|(dir, _)| dir);
            let mut fragments = Vec::new();
            for fragment in pending.item {
                match fragment {
                    Fragment::Text { lines, sugared } => fragments.push((lines, sugared)),
                    Fragment::Include { path, line } => {
                        match self.include(real_root, &pending.real, package, &path, line) {
                            Ok(lines) => fragments.push((lines, false)),
                            Err(reason) => self.violations.push(format!(
                                "{}:{line}: `doc = include_str!(\"{path}\")` {reason}, so the doc \
                                 tests in it were not checked for `unsafe` (ADR-0037)",
                                pending.path
                            )),
                        }
                    }
                }
            }
            self.doc_tests(&pending.path, fragments);
        }
    }

    /// The lines of the file that `doc = include_str!(path)` on line `by` of
    /// the `.rs` file `real` names, or why it cannot be read.
    fn include(
        &self,
        real_root: &Path,
        real: &Path,
        package: Option<&PathBuf>,
        path: &str,
        by: usize,
    ) -> Result<Vec<DocLine>, String> {
        let Some(package) = package else {
            return Err("is in a file of no workspace member's package".to_string());
        };
        let outside = || {
            format!(
                "does not name a relative path inside its package's directory {} (no hidden or \
                 `target` directory, no escapes)",
                walk::relative(real_root, package)
            )
        };
        if !is_included_path(path) {
            return Err(outside());
        }
        let mut resolved = real.parent().map(Path::to_path_buf).ok_or_else(outside)?;
        for component in path.split('/') {
            if component == ".." {
                if !resolved.pop() {
                    return Err(outside());
                }
            } else {
                resolved.push(component);
            }
        }
        if !resolved.starts_with(package) {
            return Err(outside());
        }
        let shown = walk::relative(real_root, &resolved);
        // `AGENTS.md` rule 7: xtask never opens a file named like a secret.
        if resolved
            .file_name()
            .is_some_and(|name| deps::is_secret_name(&name.to_string_lossy()))
        {
            return Err(format!(
                "names {shown}, a file named like a secret, which xtask never opens"
            ));
        }
        match fs::canonicalize(&resolved) {
            Ok(target) if target == resolved => {}
            Ok(_) => {
                return Err(format!(
                    "names {shown}, which is reached through a symbolic link"
                ));
            }
            Err(error) => return Err(format!("names {shown}, which cannot be read: {error}")),
        }
        let text = match walk::read_bounded(&resolved, self.limit) {
            Ok(text) => text,
            Err(walk::ReadError::TooLarge { limit }) => {
                return Err(format!("names {shown}, which is larger than {limit} bytes"));
            }
            Err(walk::ReadError::NotUtf8) => {
                return Err(format!("names {shown}, which is not UTF-8"));
            }
            Err(walk::ReadError::Io(error)) => {
                return Err(format!("names {shown}, which cannot be read: {error}"));
            }
        };
        Ok(doc_tests::split_lines(&text)
            .into_iter()
            .enumerate()
            .map(|(index, line)| DocLine {
                text: line.to_string(),
                at: Place::Included {
                    file: shown.clone(),
                    line: index + 1,
                    by,
                },
            })
            .collect())
    }

    /// Checks that the crate root of every target of every member of the
    /// workspaces that `manifests` belong to is a file the audit read, that
    /// no target is a proc macro ([`PROC_MACRO`]), and that each library's
    /// crate root, but those of [`UNSAFE_CRATES`], forbids `unsafe` in its
    /// doc tests ([`FORBID_IN_DOC_TESTS`]). It also checks that each crate
    /// root lies inside its package directory ([`outside_package`]), and
    /// then reads the files that documentation includes
    /// ([`Audit::doc_includes`]), now that the packages are known.
    fn crate_roots(&mut self, root: &Path, manifests: &[PathBuf]) {
        // The paths that rule compares are resolved, so its messages show
        // them relative to the resolved root.
        let real_root = metadata::canonical(root);
        // The two crates are known by name and directory both: a package
        // named like one elsewhere, or another package in one's directory,
        // is not exempt.
        let unsafe_crates: Vec<(&str, PathBuf)> = UNSAFE_CRATES
            .iter()
            .map(|(name, dir)| (*name, metadata::canonical(&root.join(dir))))
            .collect();
        // Every member's package directory, for the worktree rule below.
        let mut packages: Vec<(PathBuf, String)> = Vec::new();
        for workspace in metadata::workspaces(root, manifests, Resolve::No) {
            let graph = match workspace.graph {
                Ok(graph) => graph,
                Err(error) => {
                    self.violations.push(format!(
                        "{}: cargo cannot describe this workspace offline, so its crate roots \
                         were not checked: {error}",
                        workspace.shown
                    ));
                    continue;
                }
            };
            for package in graph.packages.iter().filter(|package| package.is_member) {
                let package_dir = package.manifest_path.parent().map(metadata::canonical);
                if let Some(dir) = &package_dir {
                    packages.push((dir.clone(), package.name.clone()));
                }
                let may_use_unsafe = unsafe_crates
                    .iter()
                    .any(|(name, dir)| package.name == *name && package_dir.as_ref() == Some(dir));
                for target in &package.targets {
                    self.crate_roots += 1;
                    let src = metadata::canonical(&target.src_path);
                    // A proc macro gets this violation alone: it is no
                    // library, so the doc-test rule below does not ask it for
                    // a line it must not need.
                    if target.kind.split(", ").any(|kind| kind == PROC_MACRO) {
                        self.violations.push(format!(
                            "{}: the {} target `{}` of `{}` is a proc-macro crate, which no \
                             workspace under the repository may have, {UNSAFE_NAMES} included: a \
                             proc macro can make the `unsafe` keyword from a string, with a span \
                             for which rustc skips the `unsafe_code` lint, and this audit reads \
                             only the string (ADR-0037)",
                            walk::relative(&real_root, &src),
                            target.kind,
                            target.name,
                            package.name
                        ));
                    }
                    if !self.scanned.contains(&src) {
                        self.violations.push(format!(
                            "{}: the {} target `{}` of `{}` starts from a file the audit did not \
                             read; a crate root must be a `.rs` file in the repository, outside \
                             build output and not behind a symbolic link",
                            walk::relative(root, &target.src_path),
                            target.kind,
                            target.name,
                            package.name
                        ));
                    } else if is_library(&target.kind)
                        && !may_use_unsafe
                        // One too large or not UTF-8 is reported as that.
                        && let Some(&forbids) = self.read.get(&src)
                    {
                        self.library_roots += 1;
                        if !forbids {
                            self.violations.push(format!(
                                "{}: the {} target `{}` of `{}` does not forbid `unsafe` in its \
                                 doc tests, and rustdoc does not apply the workspace's \
                                 `unsafe_code` lint to them. Add this line to its crate root after \
                                 the `//!` doc comment, exactly as written (an inner attribute at \
                                 crate level, not under `cfg_attr`, with nothing else in it): \
                                 `{FORBID_IN_DOC_TESTS}` (ADR-0036)",
                                // `src` is resolved, as the root is here.
                                walk::relative(&real_root, &src),
                                target.kind,
                                target.name,
                                package.name
                            ));
                        }
                    }
                    if let Some((src, dir)) =
                        outside_package(&target.src_path, &package.manifest_path)
                    {
                        let dir = match walk::relative(&real_root, &dir) {
                            dir if dir.is_empty() => ".".to_string(),
                            dir => dir,
                        };
                        self.violations.push(format!(
                            "{}: the {} target `{}` of `{}` starts from a file outside its package \
                             directory {dir}: a crate root must lie in the directory of its \
                             package's Cargo.toml, or a plain `mod` declaration in it could \
                             compile files the audit skips (a workspace root's `target/`, a \
                             linked worktree)",
                            walk::relative(&real_root, &src),
                            target.kind,
                            target.name,
                            package.name
                        ));
                    }
                }
            }
        }
        for worktree in &self.worktree_dirs {
            let resolved = metadata::canonical(worktree);
            if let Some((_, name)) = packages.iter().find(|(dir, _)| resolved.starts_with(dir)) {
                self.violations.push(format!(
                    "{}: a linked worktree inside the package directory of `{name}`, which the \
                     audit skipped: a `mod` declaration of that crate could compile code from it. \
                     Move the worktree out of every package (as in `.claude/worktrees/`)",
                    walk::relative(root, worktree)
                ));
            }
        }
        self.doc_includes(&real_root, &packages);
    }

    /// Finishes the audit.
    fn report(mut self) -> Report {
        // Without this, an audit of the wrong directory would pass by
        // finding nothing.
        for ((_, dir), seen) in UNSAFE_CRATES.iter().zip(self.roots) {
            if !seen {
                self.violations.push(format!(
                    "{dir}{CRATE_ROOT} was not found: is this the repository root?"
                ));
            }
        }
        let mut notes = vec!["`unsafe` uses per crate (review any growth):".to_string()];
        let width = UNSAFE_CRATES
            .iter()
            .map(|(name, _)| name.len())
            .max()
            .unwrap_or(0);
        for ((name, _), uses) in UNSAFE_CRATES.iter().zip(self.uses) {
            notes.push(format!("  {name:<width$}  {uses:>5}"));
        }
        notes.push(format!("Rust files scanned: {}", self.files));
        notes.push(format!("crate roots checked: {}", self.crate_roots));
        notes.push(format!(
            "library crate roots that must forbid `unsafe` in doc tests: {}",
            self.library_roots
        ));
        notes.push(format!(
            "code blocks of documentation read for `unsafe`: {}",
            self.doc_blocks
        ));
        notes.extend(self.worktrees);
        notes.extend(self.build_dir);
        Report {
            notes,
            violations: self.violations,
            ..Report::default()
        }
    }
}

/// A `macro_rules!` definition ([`File::macro_definition`]).
#[derive(Debug)]
struct MacroDefinition<'a> {
    /// Its name: an identifier, or a metavariable's name inside a macro.
    name: &'a str,
    /// Whether it is `#[macro_export]`ed, also through `cfg_attr`.
    exported: bool,
    /// Its body's tokens, inside the delimiters.
    body: Range<usize>,
}

/// A documented item that includes a file ([`Audit::documentation`]).
#[derive(Debug)]
struct Pending {
    /// The `.rs` file, as messages show it, and resolved.
    path: String,
    real: PathBuf,
    /// The item's pieces of documentation.
    item: Vec<Fragment>,
}

/// Where `src_path`, the crate root of a target of the package whose
/// manifest is `manifest_path`, lies outside that package's directory,
/// compared with links and `..` resolved: the resolved crate root and the
/// package directory. `None` when it lies inside, and when the crate root
/// cannot be resolved: then it is not a file the audit read either, which
/// [`Audit::crate_roots`] reports. A package directory that cannot be
/// resolved holds nothing, so every crate root is outside it.
fn outside_package(src_path: &Path, manifest_path: &Path) -> Option<(PathBuf, PathBuf)> {
    let src = fs::canonicalize(src_path).ok()?;
    let dir = manifest_path.parent().unwrap_or(manifest_path);
    match fs::canonicalize(dir) {
        Ok(dir) if src.starts_with(&dir) => None,
        Ok(dir) => Some((src, dir)),
        Err(_) => Some((src, dir.to_path_buf())),
    }
}

/// One lexed file, with the lookups the rules need.
struct File<'l, 'a> {
    tokens: &'l [Token<'a>],
    comments: &'l [Comment<'a>],
    /// For each token, where the statement it is in starts: the offset
    /// just past the last `;`, `{` or `}` before it, or 0.
    statement_start: Vec<usize>,
}

impl<'l, 'a> File<'l, 'a> {
    fn new(lexed: &'l Lexed<'a>) -> Self {
        let mut statement_start = Vec::with_capacity(lexed.tokens.len());
        let mut start = 0;
        for token in &lexed.tokens {
            statement_start.push(start);
            if token.is_punct(";") || token.is_punct("{") || token.is_punct("}") {
                start = token.end();
            }
        }
        Self {
            tokens: &lexed.tokens,
            comments: &lexed.comments,
            statement_start,
        }
    }

    fn token(&self, index: usize) -> Option<&Token<'a>> {
        self.tokens.get(index)
    }

    /// The comments that start at or after `from` and before `to`.
    fn comments_between(&self, from: usize, to: usize) -> &'l [Comment<'a>] {
        let first = self
            .comments
            .partition_point(|comment| comment.offset < from);
        let end = self.comments.partition_point(|comment| comment.offset < to);
        self.comments.get(first..end).unwrap_or(&[])
    }

    /// Whether a `// SAFETY:` comment lies between the start of the
    /// statement the token at `index` is in and the token.
    fn has_safety_comment(&self, index: usize) -> bool {
        let (Some(token), Some(&from)) = (self.token(index), self.statement_start.get(index))
        else {
            return false;
        };
        self.comments_between(from, token.offset)
            .iter()
            .any(|comment| {
                comment.kind == CommentKind::Line
                    && comment.doc == Doc::No
                    && comment.body().trim_start().starts_with("SAFETY:")
            })
    }

    /// The index of the token after an optional `extern` ABI that starts at
    /// `index`: `extern`, `extern "C"`, or nothing.
    fn skip_abi(&self, index: usize) -> usize {
        if !self
            .token(index)
            .is_some_and(|token| token.is_word("extern"))
        {
            return index;
        }
        match self.token(index + 1) {
            Some(abi) if abi.kind == TokenKind::Literal => index + 2,
            _ => index + 1,
        }
    }

    /// Whether the `extern` at `index` opens a block of declarations
    /// (`extern "C" {`), rather than an ABI of a function or function
    /// pointer, or `extern crate`.
    fn is_extern_block(&self, index: usize) -> bool {
        self.token(self.skip_abi(index))
            .is_some_and(|token| token.is_punct("{"))
    }

    /// Whether the `unsafe` at `index` begins the qualifiers of a function
    /// item (`unsafe fn name`, `unsafe extern "C" fn name`), not of a
    /// function pointer type.
    fn is_unsafe_fn(&self, index: usize) -> bool {
        let after = self.skip_abi(index + 1);
        self.token(after).is_some_and(|token| token.is_word("fn"))
            && self.token(after + 1).is_some_and(Token::is_ident)
    }

    /// Where the visibility of the function whose `unsafe` is at `index`
    /// starts, if it is `pub` or `pub(...)`.
    fn visibility_start(&self, index: usize) -> Option<usize> {
        let mut at = index;
        while at > 0
            && self
                .token(at - 1)
                .is_some_and(|token| token.is_word("const") || token.is_word("async"))
        {
            at -= 1;
        }
        let before = at.checked_sub(1)?;
        let token = self.token(before)?;
        if token.is_word("pub") {
            return Some(before);
        }
        if token.is_punct(")") {
            let open = self.matching_open(before, "(", ")")?;
            let pub_index = open.checked_sub(1)?;
            if self.token(pub_index)?.is_word("pub") {
                return Some(pub_index);
            }
        }
        None
    }

    /// Whether the `unsafe` at `index` qualifies a `pub` function.
    fn is_pub_unsafe_fn(&self, index: usize) -> bool {
        self.is_unsafe_fn(index) && self.visibility_start(index).is_some()
    }

    /// The index of the `close` that closes the `open` at `index`.
    fn matching_close(&self, index: usize, open: &str, close: &str) -> Option<usize> {
        let mut depth = 0_usize;
        for (at, token) in self.tokens.iter().enumerate().skip(index) {
            if token.is_punct(open) {
                depth += 1;
            } else if token.is_punct(close) {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(at);
                }
            }
        }
        None
    }

    /// Whether an outer attribute of the item that starts at `item` (a
    /// `#[...]` right before it, or before another such attribute) names
    /// `name` anywhere, as `#[macro_export]` or inside `cfg_attr` does.
    fn has_attribute_naming(&self, item: usize, name: &str) -> bool {
        let mut start = item;
        while let Some(close) = start.checked_sub(1)
            && self.token(close).is_some_and(|token| token.is_punct("]"))
            && let Some(open) = self.matching_open(close, "[", "]")
            && let Some(hash) = open.checked_sub(1)
            && self.token(hash).is_some_and(|token| token.is_punct("#"))
        {
            if self
                .tokens
                .get(open..close)
                .is_some_and(|attribute| attribute.iter().any(|token| token.is_name(name)))
            {
                return true;
            }
            start = hash;
        }
        false
    }

    /// For the `macro_rules` at `index`, if it is `#[macro_export]`ed (also
    /// through `cfg_attr`) and its body holds a metavariable other than
    /// `$crate`: each `unsafe` in its body, in any form, as the line, the
    /// form ([`File::unsafe_form`]) and the macro's name. A body that does
    /// not close runs to the end of the file.
    fn exported_unsafe_wrappers(&self, index: usize) -> Vec<(usize, &'static str, &'a str)> {
        let mut found = Vec::new();
        let (Some(bang), Some(name), Some(open)) = (
            self.token(index + 1),
            self.token(index + 2),
            self.token(index + 3),
        ) else {
            return found;
        };
        if !bang.is_punct("!")
            || !name.is_ident()
            || !self.has_attribute_naming(index, "macro_export")
        {
            return found;
        }
        let close = match open.text {
            "{" => "}",
            "(" => ")",
            "[" => "]",
            _ => return found,
        };
        let end = self
            .matching_close(index + 3, open.text, close)
            .unwrap_or(self.tokens.len());
        let takes_tokens = (index + 4..end).any(|dollar| {
            self.token(dollar).is_some_and(|t| t.is_punct("$"))
                && !self
                    .token(dollar + 1)
                    .is_some_and(|after| after.is_word("crate"))
        });
        if !takes_tokens {
            return found;
        }
        for at in index + 4..end {
            if let Some(token) = self.token(at).filter(|token| token.is_word("unsafe")) {
                found.push((token.line, self.unsafe_form(at), name.text));
            }
        }
        found
    }

    /// The `macro_rules!` definition whose `macro_rules` is at `index`, if it
    /// is one: `macro_rules ! name` (or a metavariable `$name`, inside
    /// another macro) then a delimited body. A body that does not close runs
    /// to the end of the file.
    fn macro_definition(&self, index: usize) -> Option<MacroDefinition<'a>> {
        if !self.token(index + 1)?.is_punct("!") {
            return None;
        }
        let (name, open) = match self.token(index + 2)? {
            dollar if dollar.is_punct("$") => (self.token(index + 3)?.text, index + 4),
            name if name.is_ident() => (name.text, index + 3),
            _ => return None,
        };
        let close = match self.token(open)?.text {
            "{" => "}",
            "(" => ")",
            "[" => "]",
            _ => return None,
        };
        let opener = self.token(open)?.text;
        let end = self
            .matching_close(open, opener, close)
            .unwrap_or(self.tokens.len());
        Some(MacroDefinition {
            name,
            exported: self.has_attribute_naming(index, "macro_export"),
            body: open + 1..end,
        })
    }

    /// Whether the token at `index` is in the arguments of a macro call
    /// (`name!(...)`, also through groups nested in them). An inner
    /// attribute's `#![` is no call.
    fn in_macro_call(&self, index: usize) -> bool {
        let mut depth = 0_usize;
        for at in (0..index).rev() {
            let Some(token) = self.token(at) else {
                return false;
            };
            if ["}", ")", "]"].iter().any(|close| token.is_punct(close)) {
                depth += 1;
            } else if ["{", "(", "["].iter().any(|open| token.is_punct(open)) {
                if depth > 0 {
                    depth -= 1;
                    continue;
                }
                let before = |back: usize| at.checked_sub(back).and_then(|i| self.token(i));
                if before(1).is_some_and(|t| t.is_punct("!"))
                    && !before(2).is_some_and(|t| t.is_punct("#"))
                {
                    return true;
                }
            }
        }
        false
    }

    /// The delimited group that opens at `index`: its delimiters' indexes.
    fn group(&self, index: usize) -> Option<(usize, usize)> {
        let open = self.token(index)?.text;
        let close = match open {
            "{" => "}",
            "(" => ")",
            "[" => "]",
            _ => return None,
        };
        Some((index, self.matching_close(index, open, close)?))
    }

    /// The arms of a macro's `body`: each matcher's and transcriber's tokens,
    /// inside their delimiters. What cannot be read as arms is one more
    /// transcriber with an empty matcher, so that it is still checked.
    fn macro_arms(&self, body: &Range<usize>) -> Vec<(Range<usize>, Range<usize>)> {
        let mut arms = Vec::new();
        let mut at = body.start;
        while at < body.end {
            if self.token(at).is_some_and(|token| token.is_punct(";")) {
                at += 1;
                continue;
            }
            let arm = self.group(at).and_then(|(open, close)| {
                let arrow =
                    self.token(close + 1)?.is_punct("=") && self.token(close + 2)?.is_punct(">");
                let transcriber = self.group(close + 3).filter(|_| arrow)?;
                Some((open + 1..close, transcriber))
            });
            let Some((matcher, (open, close))) = arm.filter(|(_, (_, close))| *close < body.end)
            else {
                break;
            };
            arms.push((matcher, open + 1..close));
            at = close + 1;
        }
        if at < body.end {
            arms.push((at..at, at..body.end));
        }
        arms
    }

    /// For an exported macro: each place in a transcriber where a `tt` or
    /// `ident` metavariable, or a repetition that holds one, comes right
    /// before a token the macro writes itself that `unsafe` can precede
    /// (`{`, `fn`, `impl`, `trait`, `extern`, `static`), or names an
    /// attribute the macro writes (ADR-0038). Each as the line, the
    /// metavariable, its fragment, and where it is.
    ///
    /// A metavariable's fragment comes from the matchers. One that no
    /// matcher declares, or that arms declare differently, is a `tt`. The
    /// token after it is found past `$(`, past the `)` that closes a
    /// repetition with its separator and `*`, `+` or `?`; another
    /// metavariable there, or the end of a group, is no violation.
    fn caller_unsafe(
        &self,
        definition: &MacroDefinition<'a>,
    ) -> Vec<(usize, &'a str, &'a str, String)> {
        let arms = self.macro_arms(&definition.body);
        let mut fragments: BTreeMap<&str, Option<&str>> = BTreeMap::new();
        for (matcher, _) in &arms {
            for at in matcher.clone() {
                let declared = (
                    self.token(at),
                    self.token(at + 1),
                    self.token(at + 2),
                    self.token(at + 3),
                );
                if let (Some(dollar), Some(name), Some(colon), Some(fragment)) = declared
                    && dollar.is_punct("$")
                    && name.is_ident()
                    && !name.is_word("crate")
                    && colon.is_punct(":")
                    && fragment.is_ident()
                {
                    fragments
                        .entry(name.text)
                        .and_modify(|known| {
                            if *known != Some(fragment.text) {
                                *known = None;
                            }
                        })
                        .or_insert(Some(fragment.text));
                }
            }
        }
        let mut found = Vec::new();
        for (_, transcriber) in &arms {
            for at in transcriber.clone() {
                let (Some(dollar), Some(name)) = (self.token(at), self.token(at + 1)) else {
                    continue;
                };
                if !dollar.is_punct("$") || !name.is_ident() || name.is_word("crate") {
                    continue;
                }
                let fragment = fragments.get(name.text).copied().flatten().unwrap_or("tt");
                if fragment != "tt" && fragment != "ident" {
                    continue;
                }
                let place = if self.names_attribute(at) {
                    Some(format!(
                        "as the name of an attribute it writes (`#[${} ...]`)",
                        name.text
                    ))
                } else {
                    self.written_after(at + 2, transcriber.end, &fragments)
                        .into_iter()
                        .find_map(|next| {
                            let token = self.token(next)?;
                            let item = ["fn", "impl", "trait", "extern", "static"]
                                .iter()
                                .any(|word| token.is_word(word));
                            (token.is_punct("{") || item).then(|| {
                                format!("right before `{}`, which it writes itself", token.text)
                            })
                        })
                };
                if let Some(place) = place {
                    found.push((dollar.line, name.text, fragment, place));
                }
            }
        }
        found
    }

    /// Whether the metavariable whose `$` is at `index` is the first thing
    /// in an attribute the macro writes, through any repetitions that open
    /// before it: `#[$k ...]`, `#![$k ...]`, `#[$($k)* ...]`, or the
    /// attribute that a `cfg_attr` in one applies (`#[cfg_attr(p, $k)]`).
    fn names_attribute(&self, index: usize) -> bool {
        let mut at = index;
        while at >= 2
            && self.token(at - 1).is_some_and(|token| token.is_punct("("))
            && self.token(at - 2).is_some_and(|token| token.is_punct("$"))
        {
            at -= 2;
        }
        let before = |back: usize| at.checked_sub(back).and_then(|i| self.token(i));
        let opens_attribute = before(1).is_some_and(|t| t.is_punct("["))
            && (before(2).is_some_and(|t| t.is_punct("#"))
                || (before(2).is_some_and(|t| t.is_punct("!"))
                    && before(3).is_some_and(|t| t.is_punct("#"))));
        let applied_by_cfg_attr =
            before(1).is_some_and(|t| t.is_punct(",")) && self.lint_level(at) == Some("cfg_attr");
        opens_attribute || applied_by_cfg_attr
    }

    /// The indexes of the tokens that a transcriber, which ends at `end`,
    /// can write itself next, at or after `index`, in any expansion: into a
    /// repetition (`$(`), past it too when it can be empty (`*`, `?`), past
    /// the `)` that ends one to what follows or to its start again, and its
    /// separator, which it writes between rounds. A metavariable there is
    /// the caller's, and ends that path, unless it is a `vis`, which can be
    /// empty; so does the end of a group or of the transcriber.
    fn written_after(
        &self,
        index: usize,
        end: usize,
        fragments: &BTreeMap<&str, Option<&str>>,
    ) -> Vec<usize> {
        let is_operator = |at: usize| {
            self.token(at)
                .is_some_and(|token| ["*", "+", "?"].iter().any(|op| token.is_punct(op)))
        };
        // Past the `)` at `close` of a repetition: its separator, if any, and
        // the index after its operator.
        let after_repetition = |close: usize| -> (Option<usize>, usize) {
            if is_operator(close + 1) {
                (None, close + 2)
            } else if is_operator(close + 2) {
                (Some(close + 1), close + 3)
            } else {
                (None, close + 1)
            }
        };
        let mut found = Vec::new();
        let mut seen = BTreeSet::new();
        let mut paths = vec![index];
        while let Some(at) = paths.pop() {
            if at >= end || !seen.insert(at) {
                continue;
            }
            let Some(token) = self.token(at) else {
                continue;
            };
            if token.is_punct("$") {
                let Some(next) = self.token(at + 1) else {
                    continue;
                };
                if next.is_punct("(") {
                    let Some(close) = self.matching_close(at + 1, "(", ")") else {
                        continue;
                    };
                    paths.push(at + 2);
                    let (_, after) = after_repetition(close);
                    let may_be_empty = self
                        .token(after - 1)
                        .is_some_and(|op| op.is_punct("*") || op.is_punct("?"));
                    if may_be_empty {
                        paths.push(after);
                    }
                } else if next.is_ident()
                    && !next.is_word("crate")
                    && fragments.get(next.text).copied().flatten() == Some("vis")
                {
                    paths.push(at + 2);
                } else if next.is_word("crate") {
                    found.push(at);
                }
                continue;
            }
            if token.is_punct(")") {
                let Some(open) = self.matching_open(at, "(", ")") else {
                    continue;
                };
                let repetition = open
                    .checked_sub(1)
                    .and_then(|before| self.token(before))
                    .is_some_and(|before| before.is_punct("$"));
                if repetition {
                    let (separator, after) = after_repetition(at);
                    found.extend(separator);
                    paths.push(after);
                    paths.push(open + 1);
                }
                continue;
            }
            if token.is_punct("]") || token.is_punct("}") {
                continue;
            }
            found.push(at);
        }
        found
    }

    /// What the `unsafe` at `index` begins, for messages: a block, an
    /// `impl`, a `trait`, a function, an `extern` block, the attribute form
    /// (`#[unsafe(no_mangle)]`), or a bare keyword.
    fn unsafe_form(&self, index: usize) -> &'static str {
        match self.token(index + 1) {
            Some(next) if next.is_punct("{") => "block",
            Some(next) if next.is_word("impl") => "impl",
            Some(next) if next.is_word("trait") || next.is_word("auto") => "trait",
            Some(next) if next.is_punct("(") => "attribute",
            Some(next) if next.is_word("fn") => "fn",
            Some(next) if next.is_word("extern") => {
                if self.is_extern_block(index + 1) {
                    "`extern` block"
                } else {
                    "fn"
                }
            }
            _ => "keyword",
        }
    }

    /// The index of the `open` that the `close` at `index` closes.
    fn matching_open(&self, index: usize, open: &str, close: &str) -> Option<usize> {
        let mut depth = 0_usize;
        for at in (0..=index).rev() {
            let token = self.token(at)?;
            if token.is_punct(close) {
                depth += 1;
            } else if token.is_punct(open) {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(at);
                }
            }
        }
        None
    }

    /// Whether the documentation of the `pub unsafe fn` whose `unsafe` is
    /// at `index` has a `# Safety` line: in its outer doc comments, or in
    /// `#[doc = "..."]` attributes, before its visibility.
    fn has_safety_section(&self, index: usize) -> bool {
        let Some(item) = self.visibility_start(index) else {
            return false;
        };
        let Some(item_offset) = self.token(item).map(|token| token.offset) else {
            return false;
        };
        let mut start = item;
        let mut doc_attribute = false;
        // Step back over the outer attributes (`#[...]`) of the item.
        while let Some(close) = start.checked_sub(1)
            && self.token(close).is_some_and(|token| token.is_punct("]"))
            && let Some(open) = self.matching_open(close, "[", "]")
            && let Some(hash) = open.checked_sub(1)
            && self.token(hash).is_some_and(|token| token.is_punct("#"))
        {
            if self
                .token(open + 1)
                .is_some_and(|token| token.is_name("doc"))
            {
                doc_attribute |= self.tokens.get(open..close).is_some_and(|attribute| {
                    attribute.iter().any(|token| {
                        token.kind == TokenKind::Literal && token.text.contains("# Safety")
                    })
                });
            }
            start = hash;
        }
        let from = start
            .checked_sub(1)
            .and_then(|before| self.token(before))
            .map_or(0, Token::end);
        doc_attribute
            || self
                .comments_between(from, item_offset)
                .iter()
                .filter(|comment| comment.doc == Doc::Outer)
                .any(|comment| {
                    comment
                        .body()
                        .lines()
                        .any(|line| line.trim().trim_start_matches('*').trim() == "# Safety")
                })
    }

    /// The lint level that applies to the lint name at `index`: the word
    /// before the parenthesis that encloses it, as in
    /// `allow(dead_code, unsafe_code)`.
    fn lint_level(&self, index: usize) -> Option<&'a str> {
        let mut depth = 0_usize;
        for at in (0..index).rev() {
            let token = self.token(at)?;
            if token.is_punct(")") {
                depth += 1;
            } else if token.is_punct("(") {
                if depth == 0 {
                    let level = self.token(at.checked_sub(1)?)?;
                    return level.is_ident().then_some(level.text);
                }
                depth -= 1;
            }
        }
        None
    }

    /// Whether the token at `index` is inside an attribute: the `[` that
    /// encloses it follows `#` or `#!`.
    fn in_attribute(&self, index: usize) -> bool {
        let mut depth = 0_usize;
        for at in (0..index).rev() {
            let Some(token) = self.token(at) else {
                return false;
            };
            if token.is_punct("]") {
                depth += 1;
            } else if token.is_punct("[") {
                if depth == 0 {
                    let before = |back: usize| at.checked_sub(back).and_then(|i| self.token(i));
                    return before(1).is_some_and(|t| t.is_punct("#"))
                        || (before(1).is_some_and(|t| t.is_punct("!"))
                            && before(2).is_some_and(|t| t.is_punct("#")));
                }
                depth -= 1;
            }
        }
        false
    }

    /// If the token at `index` begins a reference to another source file,
    /// how to name it in a message: the `path` of a `#[path = ...]`
    /// attribute, or the `include` of `include!(...)`.
    fn file_reference(&self, index: usize) -> Option<&'static str> {
        let token = self.token(index)?;
        let next = self.token(index + 1)?;
        if token.is_name("path") && next.is_punct("=") && self.in_attribute(index) {
            Some("`#[path = ...]`")
        } else if token.is_name("include") && next.is_punct("!") {
            Some("`include!`")
        } else {
            None
        }
    }

    /// Whether the file carries [`FORBID_IN_DOC_TESTS`] as an inner
    /// attribute at crate level: the same tokens, at nesting depth 0 (in no
    /// `mod`, function, macro or other attribute), so that whitespace and
    /// comments between them do not matter, and any other spelling does not
    /// count. A closing delimiter with nothing open, which the compiler
    /// rejects anyway, makes the depth unknown, and the answer is no.
    fn forbids_unsafe_in_doc_tests(&self) -> bool {
        let wanted = lexer::lex(FORBID_IN_DOC_TESTS).tokens;
        let mut depth = 0_usize;
        for (index, token) in self.tokens.iter().enumerate() {
            if depth == 0
                && self
                    .tokens
                    .get(index..index + wanted.len())
                    .is_some_and(|found| {
                        found.iter().zip(&wanted).all(|(found, wanted)| {
                            (found.kind, found.text) == (wanted.kind, wanted.text)
                        })
                    })
            {
                return true;
            }
            if token.is_punct("(") || token.is_punct("[") || token.is_punct("{") {
                depth += 1;
            } else if token.is_punct(")") || token.is_punct("]") || token.is_punct("}") {
                let Some(outer) = depth.checked_sub(1) else {
                    return false;
                };
                depth = outer;
            }
        }
        false
    }

    /// Whether the file reference at `index` ([`File::file_reference`])
    /// names, with a single string literal, a path the audit reads.
    fn names_audited_file(&self, index: usize) -> bool {
        let literal = if self.token(index).is_some_and(|token| token.is_name("path")) {
            // `path = "..."`
            self.token(index + 2)
        } else {
            // `include!("...")`, with any kind of delimiter around it.
            let open = self.token(index + 2);
            let close = self.token(index + 4);
            let delimited = matches!(
                (open.map(|t| t.text), close.map(|t| t.text)),
                (Some("("), Some(")")) | (Some("["), Some("]")) | (Some("{"), Some("}"))
            );
            if !delimited {
                return false;
            }
            self.token(index + 3)
        };
        literal
            .filter(|token| token.kind == TokenKind::Literal)
            .and_then(|token| string_value(token.text))
            .is_some_and(is_audited_relative_path)
    }
}

/// The value of a plain (`"..."`) or raw (`r#"..."#`) string literal
/// without escapes. `None` for any other literal, or one with a backslash.
fn string_value(literal: &str) -> Option<&str> {
    let value = match literal.strip_prefix('r') {
        Some(raw) => {
            let hashes = raw.len() - raw.trim_start_matches('#').len();
            let quoted = raw.get(hashes..raw.len().checked_sub(hashes)?)?;
            quoted.strip_prefix('"')?.strip_suffix('"')?
        }
        None => literal.strip_prefix('"')?.strip_suffix('"')?,
    };
    (!value.contains('\\')).then_some(value)
}

/// Whether `path`, relative to a source file's directory, names a `.rs`
/// file that the audit reads: it ends in `.rs` and every component is an
/// ordinary name, so it cannot leave the directory, enter a hidden or
/// `target` directory, or be absolute.
///
/// The names are compared as the strictest file system would: macOS and
/// Windows ignore case by default, so `Target/x.rs` opens a file in the
/// skipped `target/` there; Windows drops a trailing `.` or space from a
/// component, so `target./x.rs` does too; and NTFS can give a long or
/// hidden name a short alias with a `~` in it (`CLAUDE~1` for `.claude`).
/// Such names are rejected on every OS, which costs no real module path.
fn is_audited_relative_path(path: &str) -> bool {
    path.ends_with(".rs")
        && !path.contains(':')
        && path.split('/').all(|component| {
            !component.is_empty()
                && !component.starts_with('.')
                && !component.ends_with(['.', ' '])
                && !component.eq_ignore_ascii_case("target")
                && !component.contains(['\\', '~'])
        })
}

/// Whether `path`, relative to a source file's directory, may name a file
/// that `doc = include_str!(...)` includes: `/`-separated components, each
/// `..` or an ordinary name (not hidden, not `target` in any case, with no
/// trailing `.` or space, no `\`, `~` or `:`), as [`is_audited_relative_path`]
/// has them but for `..`, which may step up as long as the file stays in its
/// package ([`Audit::include`]).
fn is_included_path(path: &str) -> bool {
    !path.contains(':')
        && path.split('/').all(|component| {
            component == ".."
                || (!component.is_empty()
                    && !component.starts_with('.')
                    && !component.ends_with(['.', ' '])
                    && !component.eq_ignore_ascii_case("target")
                    && !component.contains(['\\', '~']))
        })
}

/// Whether a target of the kinds `kind` (as [`metadata::Target::kind`]
/// joins them) is a library ([`LIBRARY_KINDS`]).
fn is_library(kind: &str) -> bool {
    kind.split(", ").any(|kind| LIBRARY_KINDS.contains(&kind))
}

/// Whether the walk audits a file named `name`: Rust source, with `.rs` in
/// any case, since on macOS and Windows `mod x;` opens `x.RS`.
fn is_rust_source(name: &str) -> bool {
    name.len() > 3
        && name
            .get(name.len() - 3..)
            .is_some_and(|extension| extension.eq_ignore_ascii_case(".rs"))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write as _;

    use super::*;
    use crate::walk::testing::{TempTree, symlink};

    /// Audits `files` (path and text), plus the two crate roots so that
    /// the audit does not complain about those, and returns the report.
    fn audit(files: &[(&str, &str)]) -> Report {
        let mut audit = Audit::default();
        for (_, dir) in UNSAFE_CRATES {
            let root = format!("{dir}{CRATE_ROOT}");
            if !files.iter().any(|(path, _)| *path == root) {
                audit.file(&root, Path::new(&root), "#![allow(unsafe_code)]\n");
            }
        }
        for (path, text) in files {
            audit.file(path, Path::new(path), text);
        }
        audit.report()
    }

    fn violations(files: &[(&str, &str)]) -> Vec<String> {
        audit(files).violations
    }

    /// The violations for `text` as a file of `tidepool-platform`.
    fn platform(text: &str) -> Vec<String> {
        violations(&[("crates/platform/src/window.rs", text)])
    }

    /// The violations for `text` as a file of `tidepool-sys`.
    fn sys(text: &str) -> Vec<String> {
        violations(&[("crates/sys/src/linux/x11.rs", text)])
    }

    fn outside(path: &str, line: usize) -> String {
        format!("{path}:{line}: `unsafe` is only allowed in tidepool-sys and tidepool-platform")
    }

    fn undocumented(line: usize, what: &str) -> String {
        format!(
            "crates/platform/src/window.rs:{line}: `unsafe` {what} has no `// SAFETY:` comment \
             before it in the same statement"
        )
    }

    fn no_safety_section(path: &str, line: usize) -> String {
        format!("{path}:{line}: `pub unsafe fn` has no `# Safety` section in its doc comment")
    }

    // --- Where `unsafe` may appear ---

    #[test]
    fn unsafe_outside_the_two_crates_is_a_violation() {
        assert_eq!(
            violations(&[(
                "crates/text/src/rope.rs",
                "fn f(p: *const u8) -> u8 {\n    // SAFETY: documented, but still not allowed.\n    \
                 unsafe { *p }\n}\n"
            )]),
            [outside("crates/text/src/rope.rs", 3)]
        );
    }

    #[test]
    fn every_form_of_unsafe_outside_the_two_crates_is_a_violation() {
        for text in [
            "unsafe fn f() {}",
            "unsafe impl Send for X {}",
            "unsafe trait Pod {}",
            "#[unsafe(no_mangle)] fn f() {}",
            "macro_rules! m { () => { unsafe { f() } } }",
            "#[cfg(target_os = \"windows\")] fn f() { unsafe { g() } }",
            "type F = unsafe extern \"C\" fn();",
        ] {
            assert_eq!(
                violations(&[("crates/text/src/lib.rs", text)]).len(),
                1,
                "{text}"
            );
        }
    }

    #[test]
    fn every_directory_but_the_two_crates_is_outside() {
        for path in [
            "crates/core/src/lib.rs",
            "crates/text/tests/rope.rs",
            "crates/app/src/main.rs",
            "xtask/src/main.rs",
            "agents/tp-agent-mock/src/main.rs",
            "bench/src/main.rs",
            "build.rs",
            // Names that only start like an unsafe crate's directory.
            "crates/system/src/lib.rs",
            "crates/sys.rs",
            "crates/platform-tools/src/lib.rs",
            "vendor/crates/sys/src/lib.rs",
        ] {
            assert_eq!(
                violations(&[(path, "// SAFETY: no.\nunsafe { f() }")]),
                [outside(path, 2)]
            );
        }
        for path in [
            "crates/sys/src/lib.rs",
            "crates/sys/src/linux/x11.rs",
            "crates/sys/tests/layout.rs",
            "crates/platform/src/linux/window.rs",
            "crates/platform/build.rs",
        ] {
            assert_eq!(
                violations(&[(path, "// SAFETY: fine.\nunsafe { f() }")]),
                Vec::<String>::new(),
                "{path}"
            );
        }
    }

    #[test]
    fn unsafe_in_comments_and_strings_is_not_a_violation() {
        let text = "//! Never use `unsafe { }` here.\n\
                    /// Prose may say unsafe, and inline code `unsafe { f() }`.\n\
                    // #![allow(unsafe_code)]\n\
                    /* unsafe impl Send for X {} */\n\
                    const HELP: &str = \"unsafe { } is denied; #![allow(unsafe_code)]\";\n\
                    const RAW: &str = r#\"unsafe impl Send for X {}\"#;\n\
                    const C: char = '\\'';\nconst S: &str = \"unsafe\";\n\
                    fn is_unsafe(unsafe_code: bool) -> bool { unsafe_code }\n\
                    const E: &str = \"extern \\\"C\\\" { fn f(); }\";\n";
        assert_eq!(
            violations(&[("crates/text/src/lib.rs", text)]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_shebang_line_hides_nothing_and_counts_for_nothing() {
        // rustc drops the first line of any `.rs` file that starts with `#!`
        // not followed by `[`, so a quote in it opens no string: this module
        // compiles into a crate that denies `unsafe_code`, with the lint
        // allowed and an `unsafe` block.
        let path = "crates/text/src/m.rs";
        assert_eq!(
            violations(&[(
                path,
                "#!/x \"\n#![allow(unsafe_code)]\n/// F.\npub fn f() -> u8 {\n    let x = 1u8;\n    \
                 let p = &x as *const u8;\n    unsafe { *p }\n}\n// \"\n"
            )]),
            [
                format!(
                    "{path}:2: `allow(unsafe_code)` is only allowed in the crate roots of \
                     tidepool-sys and tidepool-platform"
                ),
                outside(path, 7),
            ]
        );
        // And what is on that line is not code.
        assert_eq!(
            violations(&[(path, "#!/usr/bin/unsafe\nfn f() {}\n")]),
            Vec::<String>::new()
        );
    }

    // --- Doc tests (ADR-0037) ---

    /// The violation for `unsafe` on `line` of the documentation of `path`.
    fn doc_test(path: &str, line: usize) -> String {
        format!(
            "{path}:{line}: a code block in the documentation, which rustdoc may compile as a doc \
             test, holds `unsafe`, which only tidepool-sys and tidepool-platform may use: every \
             block counts, `ignore` and `text` too, since the `unsafe_code` lint can miss `unsafe` \
             that another crate's macro expands. Inline code in prose is fine (ADR-0037)"
        )
    }

    /// The violation for a `doc =` value the audit cannot read.
    fn unreadable_doc(path: &str, line: usize) -> String {
        format!(
            "{path}:{line}: a `doc =` value that is not a string literal or `include_str!` of one, \
             which the audit cannot read, so the doc tests it may hold were not checked for \
             `unsafe` (ADR-0037)"
        )
    }

    #[test]
    fn unsafe_in_any_code_block_of_the_documentation_is_a_violation() {
        let path = "crates/text/src/lib.rs";
        for (text, lines) in [
            // Fenced, either way, with any info string.
            (
                "/// Reads.\n///\n/// ```\n/// let x = unsafe { *p };\n/// ```\nfn f() {}",
                vec![4],
            ),
            ("//! ~~~\n//! unsafe { f() }\n//! ~~~", vec![2]),
            (
                "/// ```ignore\n/// unsafe { f() }\n/// ```\nfn f() {}",
                vec![2],
            ),
            ("/// ```text\n/// unsafe-audit\n/// ```\nfn f() {}", vec![2]),
            (
                "/// ```compile_fail\n/// unsafe { f() }\n/// ```\nfn f() {}",
                vec![2],
            ),
            (
                "/// ```sh\n/// cargo xtask unsafe-audit\n/// ```\nfn f() {}",
                vec![2],
            ),
            // Hidden lines are code too, and so is every `unsafe` on a line.
            (
                "/// ```\n/// # unsafe { f() }\n/// unsafe { g() }; unsafe { h() }\n/// ```\nfn f() {}",
                vec![2, 3, 3],
            ),
            // Indented.
            (
                "/// Reads.\n///\n///     unsafe { f() }\nfn f() {}",
                vec![3],
            ),
            // In a list item, and in a block quote.
            (
                "/// - An item:\n///\n///   ```\n///   unsafe { f() }\n///   ```\nfn f() {}",
                vec![4],
            ),
            (
                "/// > ```\n/// > unsafe { f() }\n/// > ```\nfn f() {}",
                vec![2],
            ),
            // `/** */` and `/*! */`.
            (
                "/**\n * ```\n * unsafe { f() }\n * ```\n */\nfn f() {}",
                vec![3],
            ),
            ("/*! ```\nunsafe { f() }\n``` */", vec![2]),
            // A doc attribute's string, its escapes decoded: a line break
            // and the fence, and under `cfg_attr`.
            (
                "#[doc = \"```\\nunsafe { f() }\\n```\"]\nfn f() {}",
                vec![1],
            ),
            (
                "#[doc = r#\"\n```\nunsafe { f() }\n```\"#]\nfn f() {}",
                vec![3],
            ),
            (
                "#[cfg_attr(doc, doc = \"```\")]\n#[cfg_attr(doc, doc = \"unsafe { f() }\")]\n/// ```\nfn f() {}",
                vec![2],
            ),
            (
                "#[doc = \"\\x60\\x60\\x60\\nunsafe { f() }\\n\\u{60}``\"]\nfn f() {}",
                vec![1],
            ),
            // A fence opened in an item's outer docs runs into the inner
            // docs of the module it opens, as rustdoc joins them.
            (
                "/// ```text\nmod m {\n    //! ```\n    //! ```\n    //! unsafe { f() }\n    //! ```\n}",
                vec![5],
            ),
        ] {
            assert_eq!(
                violations(&[(path, text)]),
                lines
                    .iter()
                    .map(|&line| doc_test(path, line))
                    .collect::<Vec<_>>(),
                "{text}"
            );
        }
    }

    #[test]
    fn unsafe_in_prose_and_in_code_that_is_no_block_is_fine() {
        for text in [
            "//! Never write `unsafe { }`: the word unsafe is prose.\n",
            "/// - A list item that says unsafe,\n///   continued at its content, where unsafe is still prose:\n///\n///     more of the item, after a blank line.\nfn f() {}",
            "/// Prose\n///     continued, even unsafe, as an indented line cannot interrupt a paragraph.\nfn f() {}",
            "/// ```\n/// // unsafe in a comment\n/// let s = \"unsafe in a string\";\n/// let r#unsafe = 1;\n/// ```\nfn f() {}",
            "/// ```\n/// let unsafé = 1;\n/// ```\nfn f() {}",
            "// ```\n// unsafe { f() }\n// ```\n//// ```\n//// unsafe { f() }\n//// ```\nfn f() {}",
            "const S: &str = \"/// ```\\n/// unsafe { f() }\\n/// ```\";\n",
        ] {
            assert_eq!(
                violations(&[("crates/text/src/lib.rs", text)]),
                Vec::<String>::new(),
                "{text}"
            );
        }
    }

    #[test]
    fn a_doc_value_the_audit_cannot_read_is_a_violation() {
        let path = "crates/text/src/lib.rs";
        assert_eq!(
            violations(&[(path, "#[doc = concat!(\"a\", \"b\")]\nfn f() {}\n")]),
            [unreadable_doc(path, 1)]
        );
        assert_eq!(
            violations(&[(
                path,
                "macro_rules! documented {\n    ($d:expr) => {\n        #[doc = $d]\n        pub fn f() {}\n    };\n}\n"
            )]),
            [unreadable_doc(path, 3)]
        );
        assert_eq!(
            violations(&[(path, "#![doc = env!(\"DOCS\")]\n")]),
            [unreadable_doc(path, 1)]
        );
    }

    #[test]
    fn sys_and_platform_may_show_unsafe_in_their_doc_tests() {
        let text = "/// ```\n/// // SAFETY: an example.\n/// unsafe { f() }\n/// ```\n#[doc = concat!(\"a\")]\nfn f() {}\n";
        assert_eq!(platform(text), Vec::<String>::new());
        assert_eq!(sys(text), Vec::<String>::new());
    }

    #[test]
    fn a_file_included_as_documentation_is_read_too() {
        let tree = TempTree::new("audit-doc-include");
        tree.write(
            "Cargo.toml",
            "[workspace]\nresolver = \"3\"\nmembers = [\"crates/text\"]\n",
        );
        tree.write("crates/sys/src/lib.rs", "#![allow(unsafe_code)]\n");
        tree.write("crates/platform/src/lib.rs", "#![allow(unsafe_code)]\n");
        tree.write(
            "crates/text/Cargo.toml",
            "[package]\nname = \"text\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        );
        tree.write(
            "crates/text/src/lib.rs",
            format!(
                "//! Text.\n{FORBID_IN_DOC_TESTS}\n\n#[cfg(doctest)]\n#[doc = include_str!(\"../README.md\")]\n\
                 struct ReadmeDoctests;\n"
            ),
        );
        tree.write(
            "crates/text/README.md",
            "# text\n\nSays unsafe in prose.\n\n```rust\nlet x = 1;\nunsafe { f(x) }\n```\n",
        );
        let (violations, notes) = audit_with_crate_roots(&tree);
        assert_eq!(
            violations,
            [doc_test("crates/text/README.md", 7).replacen(
                ": a code block",
                ": included as documentation by crates/text/src/lib.rs:5: a code block",
                1
            )]
        );
        assert!(
            notes.contains(&"code blocks of documentation read for `unsafe`: 1".to_string()),
            "{notes:?}"
        );

        // Each way it cannot be read is a violation of its own.
        let include = |path: &str, reason: &str| {
            format!(
                "crates/text/src/lib.rs:5: `doc = include_str!(\"{path}\")` {reason}, so the doc \
                 tests in it were not checked for `unsafe` (ADR-0037)"
            )
        };
        let outside = "does not name a relative path inside its package's directory crates/text \
                       (no hidden or `target` directory, no escapes)";
        tree.write("README.md", "# Root\n");
        tree.write("crates/text/.hidden.md", "x\n");
        tree.write("crates/text/target/x.md", "x\n");
        tree.write("crates/text/latin.md", [b"caf\xe9\n".as_slice()].concat());
        let lib = |path: &str| {
            format!(
                "//! Text.\n{FORBID_IN_DOC_TESTS}\n\n#[cfg(doctest)]\n#[doc = include_str!(\"{path}\")]\n\
                 struct ReadmeDoctests;\n"
            )
        };
        for (path, reason) in [
            ("../../README.md", outside),
            ("../.hidden.md", outside),
            ("../target/x.md", outside),
            ("../src/../../README.md", outside),
            (
                "../latin.md",
                "names crates/text/latin.md, which is not UTF-8",
            ),
            (
                "../signing.pem",
                "names crates/text/signing.pem, a file named like a secret, which xtask never opens",
            ),
        ] {
            tree.write("crates/text/src/lib.rs", lib(path));
            assert_eq!(
                audit_with_crate_roots(&tree).0,
                [include(path, reason)],
                "{path}"
            );
        }
        // The OS says why it cannot open a missing file.
        tree.write("crates/text/src/lib.rs", lib("../missing.md"));
        let (violations, _) = audit_with_crate_roots(&tree);
        let missing = include(
            "../missing.md",
            "names crates/text/missing.md, which cannot be read",
        );
        assert!(
            violations.len() == 1
                && violations[0].starts_with(missing.split(", so the doc").next().unwrap_or("")),
            "{violations:?}"
        );
        // A path with an escape is no value the audit reads at all.
        tree.write("crates/text/src/lib.rs", lib("..\\\\README.md"));
        assert_eq!(
            audit_with_crate_roots(&tree).0,
            [unreadable_doc("crates/text/src/lib.rs", 5)]
        );
        // Through a symbolic link, when this system can make one.
        tree.write("crates/text/real.md", "```\nunsafe {}\n```\n");
        if symlink(
            &tree,
            "crates/text/linked.md",
            &tree.path().join("crates/text/real.md"),
        )
        .is_ok()
        {
            tree.write("crates/text/src/lib.rs", lib("../linked.md"));
            let (violations, _) = audit_with_crate_roots(&tree);
            assert!(
                violations.contains(&include(
                    "../linked.md",
                    "names crates/text/linked.md, which is reached through a symbolic link"
                )),
                "{violations:?}"
            );
        }
    }

    // --- Macros (ADR-0038) ---

    /// The violation for the exported macro `name` of `crates/text/src/lib.rs`
    /// that puts `$variable`, of `fragment`, at `place` on `line`.
    fn passes_unsafe_on(
        line: usize,
        name: &str,
        variable: &str,
        fragment: &str,
        place: &str,
    ) -> String {
        format!(
            "crates/text/src/lib.rs:{line}: the exported macro `{name}` puts `${variable}`, a \
             `{fragment}` fragment its caller can make the keyword `unsafe`, {place}: rustc does \
             not apply the caller's `unsafe_code` lint to braces or an item from another crate's \
             macro, in doc tests or in code. Use a fragment that cannot be `unsafe` (`path`, `ty`, \
             `vis`), or leave the `unsafe` to the caller (ADR-0038)"
        )
    }

    /// `place` for the token `token` that a macro writes itself.
    fn before(token: &str) -> String {
        format!("right before `{token}`, which it writes itself")
    }

    #[test]
    fn an_exported_macro_may_not_let_its_caller_s_unsafe_reach_braces_or_an_item() {
        let attribute = |variable: &str| {
            format!("as the name of an attribute it writes (`#[${variable} ...]`)")
        };
        for (arms, variable, fragment, place) in [
            // g1's `k1`.
            (
                "($k:tt, $($t:tt)*) => { $k { $($t)* } };",
                "k",
                "tt",
                before("{"),
            ),
            ("($k:tt) => { $k fn f() {} };", "k", "tt", before("fn")),
            (
                "($k:tt) => { $k impl Send for S {} };",
                "k",
                "tt",
                before("impl"),
            ),
            ("($k:tt) => { $k trait T {} };", "k", "tt", before("trait")),
            (
                "($k:tt) => { $k extern \"C\" fn f() {} };",
                "k",
                "tt",
                before("extern"),
            ),
            (
                "($k:tt) => { $k static X: i32 = 0; };",
                "k",
                "tt",
                before("static"),
            ),
            (
                "($name:ident) => { $name { x: 1 } };",
                "name",
                "ident",
                before("{"),
            ),
            // The name of an attribute it writes.
            (
                "($k:tt) => { #[$k(no_mangle)] pub extern \"C\" fn f() {} };",
                "k",
                "tt",
                attribute("k"),
            ),
            ("($k:tt) => { #![$k] };", "k", "tt", attribute("k")),
            (
                "($($k:tt)*) => { #[$($k)*] fn f() {} };",
                "k",
                "tt",
                attribute("k"),
            ),
            (
                "($k:tt) => { #[cfg_attr(all(), $k(no_mangle))] fn f() {} };",
                "k",
                "tt",
                attribute("k"),
            ),
            // Through repetitions, their separators, and what can be empty.
            ("($($k:tt)*) => { $($k)* { 1 } };", "k", "tt", before("{")),
            ("($($k:tt),*) => { $($k),* { 1 } };", "k", "tt", before("{")),
            (
                "($k:tt $($t:tt)*) => { $k $( { $t } )* };",
                "k",
                "tt",
                before("{"),
            ),
            (
                "($k:tt $($t:expr)*) => { $k $($t)* { 1 } };",
                "k",
                "tt",
                before("{"),
            ),
            (
                "($k:tt $($t:expr)?) => { $k $($t)? fn f() {} };",
                "k",
                "tt",
                before("fn"),
            ),
            (
                "($k:tt, $v:vis) => { $k $v fn f() {} };",
                "k",
                "tt",
                before("fn"),
            ),
            ("($($k:tt)*) => { $($k)fn* };", "k", "tt", before("fn")),
            // A metavariable no matcher declares, or arms declare
            // differently, is a `tt`.
            ("() => { $u { 1 } };", "u", "tt", before("{")),
            (
                "($k:path) => { $k { 1 } }; ($k:tt, x) => { 1 };",
                "k",
                "tt",
                before("{"),
            ),
        ] {
            let text = format!("#[macro_export]\nmacro_rules! m {{\n    {arms}\n}}\n");
            assert_eq!(
                violations(&[("crates/text/src/lib.rs", &text)]),
                [passes_unsafe_on(3, "m", variable, fragment, &place)],
                "{arms}"
            );
        }
        // Exported through `cfg_attr`, one arm per line.
        assert_eq!(
            violations(&[(
                "crates/text/src/lib.rs",
                "#[cfg_attr(all(), macro_export)]\nmacro_rules! m {\n    () => { 1 };\n    ($k:tt) => {\n        $k { 1 }\n    };\n}\n"
            )]),
            [passes_unsafe_on(5, "m", "k", "tt", &before("{"))]
        );
    }

    #[test]
    fn exported_macros_that_cannot_pass_unsafe_on_are_fine() {
        for arms in [
            // A fragment that cannot be `unsafe` before braces or an item.
            "($name:path { $($f:ident : $v:expr),* }) => { $name { $($f: $v),* } };",
            "($e:expr) => { if $e { 1 } else { 0 } };",
            "($v:vis $name:ident) => { $v fn $name() {} };",
            "($t:ty) => { impl Clone for $t { fn clone(&self) -> Self { *self } } };",
            // `tt` fragments inside a group, before another metavariable,
            // or before tokens `unsafe` cannot precede.
            "($($t:tt)*) => { ::core::format_args!($($t)*) };",
            "($($arg:tt)+) => { $crate::log::__log($crate::log::Level::Error, ::core::module_path!(), ::core::format_args!($($arg)+)) };",
            "($left:expr, $right:expr $(,)?) => { $crate::testing::__assert_approx($left, $right, None, None) };",
            "($a:tt $b:tt) => { $a $b };",
            "($k:tt) => { ($k) { 1 } };",
            "($k:tt) => { let x = $k; };",
            "() => { $crate::f() };",
        ] {
            let text = format!("#[macro_export]\nmacro_rules! m {{\n    {arms}\n}}\n");
            assert_eq!(
                violations(&[("crates/text/src/lib.rs", &text)]),
                Vec::<String>::new(),
                "{arms}"
            );
        }
        // Not exported: its braces are no other crate's.
        assert_eq!(
            violations(&[(
                "crates/text/src/lib.rs",
                "macro_rules! k1 {\n    ($k:tt, $($t:tt)*) => { $k { $($t)* } };\n}\n"
            )]),
            Vec::<String>::new()
        );
        // In sys and platform, whose exported macros have their own rule
        // (ADR-0038 scopes this one to the other crates).
        let k1 =
            "#[macro_export]\nmacro_rules! k1 {\n    ($k:tt, $($t:tt)*) => { $k { $($t)* } };\n}\n";
        assert_eq!(platform(k1), Vec::<String>::new());
        assert_eq!(sys(k1), Vec::<String>::new());
    }

    #[test]
    fn a_k1_shaped_macro_exported_from_sys_or_platform_passes_as_documented() {
        // The documented limit (ADR-0038; `docs/04-quality.md`, "Also not
        // covered"): the rule for that shape is the other crates', and the
        // two crates' own rule looks for `unsafe` in the macro, of which a
        // `k1` has none. Review keeps such a macro out.
        let k1 =
            "#[macro_export]\nmacro_rules! k1 {\n    ($k:tt, $($t:tt)*) => { $k { $($t)* } };\n}\n";
        assert_eq!(platform(k1), Vec::<String>::new());
        assert_eq!(sys(k1), Vec::<String>::new());
        // In any other crate it fails.
        assert_eq!(
            violations(&[("crates/text/src/lib.rs", k1)]),
            [passes_unsafe_on(3, "k1", "k", "tt", &before("{"))]
        );
    }

    #[test]
    fn no_macro_defines_a_macro_in_any_crate() {
        let generator = "macro_rules! define_wrap {\n    ($d:tt, $name:ident) => {\n        #[macro_export]\n        \
                         macro_rules! $name {\n            ($d($d t:tt)*) => {{ $d($d t)* }};\n        }\n    };\n}\n\
                         define_wrap!($, wrap_gen);\n";
        let defines = |path: &str| {
            format!(
                "{path}:4: the macro `define_wrap` defines a macro (its body holds `macro_rules`): \
                 what it makes is never written out, so no rule here can read it (ADR-0038)"
            )
        };
        assert_eq!(
            platform(generator),
            [defines("crates/platform/src/window.rs")]
        );
        assert_eq!(sys(generator), [defines("crates/sys/src/linux/x11.rs")]);
        assert_eq!(
            violations(&[("crates/text/src/lib.rs", generator)]),
            [defines("crates/text/src/lib.rs")]
        );
        // `macro_rules` passed to a macro, which could make one with it.
        assert_eq!(
            violations(&[(
                "crates/text/src/lib.rs",
                "make!(macro_rules, $);\nmake! { [macro_rules] }\n"
            )]),
            [1, 2].map(|line| format!(
                "crates/text/src/lib.rs:{line}: `macro_rules` passed to a macro, which could \
                     define a macro with it that no rule here reads (ADR-0038)"
            ))
        );
        // A name of something else is fine.
        assert_eq!(
            violations(&[(
                "crates/text/src/lib.rs",
                "fn macro_rules() {}\nfn f(x: X) { x.macro_rules(); let macro_rules = 1; }\n"
            )]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn include_str_may_not_be_defined_or_imported_outside_sys_and_platform() {
        let path = "crates/text/src/lib.rs";
        let shadows = |line: usize| {
            format!(
                "{path}:{line}: `include_str` as a name of its own, which defines or imports \
                 something named like the standard macro: a macro of that name would take its \
                 place in `doc = include_str!(...)`, and the audit would read another file than \
                 rustdoc does (ADR-0038)"
            )
        };
        for text in [
            "macro_rules! include_str { ($x:literal) => { \"x\" }; }",
            "use mac::my_inc as include_str;",
            "use std::include_str;",
            "use m::{include_str, other};",
            "#[macro_export]\nmacro_rules! r#include_str { () => {} }",
        ] {
            assert_eq!(
                violations(&[(path, text)]),
                [shadows(text.lines().count())],
                "{text}"
            );
        }
        for text in [
            "#[doc = include_str!(\"../README.md\")]\nstruct ReadmeDoctests;",
            "const S: &str = ::std::include_str!(\"x\");",
            "// macro_rules! include_str {}",
            "const S: &str = \"include_str\";",
        ] {
            assert_eq!(violations(&[(path, text)]), Vec::<String>::new(), "{text}");
        }
        assert_eq!(
            platform("macro_rules! include_str { () => {} }"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_package_nested_in_sys_or_platform_is_no_part_of_them() {
        // Its files lose the two crates' exemptions: its doc tests are
        // read, and `unsafe` in it is outside them. The crates' own files
        // keep theirs.
        let tree = TempTree::new("audit-nested-package");
        tree.write("crates/sys/Cargo.toml", "");
        tree.write(
            "crates/sys/src/lib.rs",
            "#![allow(unsafe_code)]\n//! ```\n//! unsafe { f() }\n//! ```\n",
        );
        tree.write("crates/platform/src/lib.rs", "#![allow(unsafe_code)]\n");
        tree.write("crates/sys/probe/Cargo.toml", "");
        tree.write(
            "crates/sys/probe/src/lib.rs",
            "//! A probe nested in sys.\n//!\n//! ```\n//! unsafe { f() }\n//! ```\n\
             fn g() {\n    // SAFETY: no.\n    unsafe { f() }\n}\n",
        );
        let (audit, _) = audit_tree(tree.path(), MAX_FILE_BYTES).unwrap();
        assert_eq!(
            audit.report().violations,
            [
                outside("crates/sys/probe/src/lib.rs", 8),
                doc_test("crates/sys/probe/src/lib.rs", 4),
            ]
        );
    }

    #[test]
    fn the_reader_s_mechanical_gaps_are_read_as_rustdoc_reads_them() {
        // The minimal reproductions of ADR-0038's fixed gaps.
        let path = "crates/text/src/lib.rs";
        for (gap, text, line) in [
            // 1: a closing fence is followed by spaces only, not a tab or
            // other white space.
            (
                1,
                "/// ```\n/// /*\n/// ```\t\n/// */ unsafe { f() }\n/// ```\nfn f() {}",
                4,
            ),
            (
                1,
                "/// ```\n/// /*\n/// ```\u{a0}\n/// */ unsafe { f() }\n/// ```\nfn f() {}",
                4,
            ),
            // 2: a list marker is followed by a space or a tab.
            (
                2,
                "/// -\u{a0}item\n///\n///     unsafe { f() }\nfn f() {}",
                3,
            ),
            (
                2,
                "/// -\u{c}item\n///\n///     unsafe { f() }\nfn f() {}",
                3,
            ),
            (
                2,
                "/// 1.\u{a0}item\n///\n///     unsafe { f() }\nfn f() {}",
                3,
            ),
            // 3: a line of other white space is text, not blank, nor a
            // heading or a thematic break.
            (
                3,
                "/// Text.\n/// \u{a0}\n///     /*\n///\n///     unsafe { f() } // */\nfn f() {}",
                5,
            ),
            (
                3,
                "/// Text.\n/// #\u{a0}x\n///     /*\n///\n///     unsafe { f() } // */\nfn f() {}",
                5,
            ),
            (
                3,
                "/// Text.\n/// *\u{a0}*\u{a0}*\n///     /*\n///\n///     unsafe { f() } // */\nfn f() {}",
                5,
            ),
            // 4: an empty list item before a blank line closes.
            (4, "/// -\n///\n///     unsafe { f() }\nfn f() {}", 3),
            (4, "/// 10)\n///\n///     unsafe { f() }\nfn f() {}", 3),
            // 5: a setext underline ends the paragraph.
            (
                5,
                "/// Text.\n/// ===\n///     unsafe { f() }\nfn f() {}",
                3,
            ),
            (5, "/// Text.\n/// -\n///     unsafe { f() }\nfn f() {}", 3),
            (
                5,
                "/// - a\n///   ===\n///       unsafe { f() }\nfn f() {}",
                3,
            ),
            // 11: a footnote definition is a container.
            (
                11,
                "/// [^1]: ```\n///     unsafe { f() }\n///     ```\nfn f() {}",
                2,
            ),
            (
                11,
                "/// [^n]: Text\n///     ```\n///     unsafe { f() }\n///     ```\nfn f() {}",
                3,
            ),
            // 12: a CR is a line break, alone or before an LF.
            (
                12,
                "#[doc = \"Text.\\r```\\nunsafe { f() }\\n```\"]\nfn f() {}",
                1,
            ),
            (
                12,
                "#[doc = \"Text.\\x0d```\\nunsafe { f() }\\n```\"]\nfn f() {}",
                1,
            ),
            (
                12,
                "#[doc = \"Text.\\u{d}```\\r\\nunsafe { f() }\\r\\n```\"]\nfn f() {}",
                1,
            ),
            // 13: a CR LF file's block doc comment.
            (
                13,
                "/**\r\n * Text.\r\n *\r\n *     unsafe { f() }\r\n */\r\nfn f() {}\r\n",
                4,
            ),
            // 14: a last line of only `*` before `*/` is dropped.
            (
                14,
                "/**\n * ```\n * unsafe { f() }\n * ```\n***\n*/\nfn f() {}",
                3,
            ),
            (14, "/*!\n * ```\n * unsafe { f() }\n * ```\n*\n*/", 3),
            // 16: `doc = "..."` that a macro turns into an attribute.
            (
                16,
                "macro_rules! m { ($m:meta) => { #[$m] pub fn g() {} }; }\nm!(doc = \"```\\nunsafe { f() }\\n```\");",
                2,
            ),
        ] {
            assert_eq!(
                violations(&[(path, text)]),
                [doc_test(path, line)],
                "gap {gap}: {text:?}"
            );
        }
        // 16: `doc` that a macro completes cannot be read.
        assert_eq!(
            violations(&[(
                path,
                "macro_rules! m { ($($t:tt)*) => { #[doc $($t)*] pub fn g() {} }; }"
            )]),
            [unreadable_doc(path, 1)]
        );
    }

    #[test]
    fn an_included_file_breaks_lines_at_a_lone_cr_too() {
        let tree = TempTree::new("audit-doc-include-cr");
        tree.write(
            "Cargo.toml",
            "[workspace]\nresolver = \"3\"\nmembers = [\"crates/text\"]\n",
        );
        tree.write("crates/sys/src/lib.rs", "#![allow(unsafe_code)]\n");
        tree.write("crates/platform/src/lib.rs", "#![allow(unsafe_code)]\n");
        tree.write(
            "crates/text/Cargo.toml",
            "[package]\nname = \"text\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        );
        tree.write(
            "crates/text/src/lib.rs",
            format!("//! Text.\n{FORBID_IN_DOC_TESTS}\n\n#[doc = include_str!(\"../p.md\")]\npub fn f() {{}}\n"),
        );
        tree.write("crates/text/p.md", "Text.\r```\nunsafe { f() }\n```\n");
        assert_eq!(
            audit_with_crate_roots(&tree).0,
            [doc_test("crates/text/p.md", 3).replacen(
                ": a code block",
                ": included as documentation by crates/text/src/lib.rs:4: a code block",
                1
            )]
        );
    }

    // --- `SAFETY:` comments ---

    #[test]
    fn a_documented_block_passes() {
        for text in [
            "// SAFETY: `p` is valid.\nunsafe { *p }",
            "    // SAFETY: `p` is valid.\n    let x = unsafe { *p };",
            "f(); // SAFETY: `p` is valid.\nunsafe { *p }",
            "// SAFETY: `p` is valid.\nunsafe{ *p }",
            "// SAFETY: `p` is valid.\nunsafe\n{ *p }",
            "// Context first.\n//\n// SAFETY: `p` is valid.\nunsafe { *p }",
            "// SAFETY: there is a lot to say:\n// - one\n// - two\n// - three\n// - four\n\
             unsafe { *p }",
            "// SAFETY: fine.\n#[allow(clippy::let_and_return)]\nlet x =\n    unsafe { *p };",
            "fn f() {\n    // SAFETY: fine.\n    unsafe { g() }\n}",
            "f(\n    // SAFETY: a.\n    unsafe { a() },\n    // SAFETY: b.\n    unsafe { b() },\n);",
            "match x {\n    // SAFETY: valid.\n    Some(p) => unsafe { *p },\n    None => 0,\n}",
            "// SAFETY: the handle is only used on one thread.\nunsafe impl Send for X {}",
            "//SAFETY: no space is fine.\nunsafe { *p }",
        ] {
            assert_eq!(platform(text), Vec::<String>::new(), "{text}");
        }
    }

    #[test]
    fn an_undocumented_block_is_a_violation() {
        assert_eq!(platform("unsafe { *p }"), [undocumented(1, "block")]);
        assert_eq!(
            platform("fn f() {\n    let x = unsafe { *p };\n}"),
            [undocumented(2, "block")]
        );
        assert_eq!(
            platform("struct X;\n\nunsafe impl<T: Send> Send for X<T> {}"),
            [undocumented(3, "impl")]
        );
        for text in [
            // An ordinary comment.
            "// `p` is valid.\nunsafe { *p }",
            // The wrong spelling.
            "// safety: `p` is valid.\nunsafe { *p }",
            "// SAFETY `p` is valid.\nunsafe { *p }",
            "// Safety: `p` is valid.\nunsafe { *p }",
            "// See the SAFETY: note below.\nunsafe { *p }",
            // Not a `//` comment.
            "/* SAFETY: `p` is valid. */\nunsafe { *p }",
            "/// SAFETY: `p` is valid.\nunsafe { *p }",
            // Not a comment at all.
            "let s = \"// SAFETY: no\";\nunsafe { *p }",
            // After the keyword.
            "unsafe { *p } // SAFETY: too late.",
            "unsafe {\n    // SAFETY: too late.\n    *p\n}",
            // In an earlier statement.
            "// SAFETY: for `a`.\na();\nunsafe { b() };",
            "// SAFETY: for `a`.\nunsafe { a() };\nunsafe { b() };",
            "// SAFETY: for the function.\nfn f() {\n    unsafe { g() }\n}",
            "// SAFETY: for `a`.\nf(unsafe { a() }, unsafe { b() });",
            // A vertical tab is white space to rustc: this is a block.
            "unsafe\u{b}{ *p }",
        ] {
            assert_eq!(platform(text).len(), 1, "{text}");
        }
    }

    fn exported_wrapper(line: usize, what: &str, name: &str) -> String {
        format!(
            "crates/platform/src/window.rs:{line}: `unsafe` {what} in the exported macro `{name}`, \
             which takes tokens from its caller (`$...`): they could run as `unsafe` in the \
             caller's crate, where neither its `unsafe_code` lint nor this audit sees them, and no \
             `// SAFETY:` comment here can cover them. Make it a function, or leave the `unsafe` to \
             the caller"
        )
    }

    #[test]
    fn an_exported_macro_may_not_wrap_its_caller_s_tokens_in_unsafe() {
        // `tidepool_platform::in_unsafe!(*p)` in a crate that denies
        // `unsafe_code` compiles, and that crate has no `unsafe` token.
        assert_eq!(
            platform(
                "/// Runs `$e` as unsafe.\n#[macro_export]\nmacro_rules! in_unsafe {\n    \
                 ($e:expr) => {{\n        // SAFETY: delegated to the caller.\n        \
                 unsafe { $e }\n    }};\n}\n"
            ),
            [exported_wrapper(6, "block", "in_unsafe")]
        );
        for (text, line, what) in [
            (
                "#[macro_export]\nmacro_rules! all {\n    ($($t:tt)*) => {\n        // SAFETY: x.\n        \
                 unsafe { f($($t)*) }\n    };\n}\n",
                5,
                "block",
            ),
            (
                "#[macro_export]\nmacro_rules! send {\n    ($t:ty) => {\n        // SAFETY: no.\n        \
                 unsafe impl Send for $t {}\n    };\n}\n",
                5,
                "impl",
            ),
            // The whole `unsafe impl` counts, its body too.
            (
                "#[macro_export]\nmacro_rules! body {\n    ($e:expr) => {\n        // SAFETY: x.\n        \
                 unsafe impl Sync for A {\n            fn f() { $e }\n        }\n    };\n}\n",
                5,
                "impl",
            ),
            (
                "#[cfg_attr(all(), macro_export)]\n#[doc(hidden)]\nmacro_rules! m ( ($e:expr) => \
                 {\n    // SAFETY: x.\n    unsafe { $e }\n} );\n",
                5,
                "block",
            ),
            // Any `unsafe` in a macro that takes tokens, wherever they go:
            // after it, in the body of an `unsafe fn` it writes, under the
            // attribute form, or in an `unsafe extern` block.
            (
                "#[macro_export]\nmacro_rules! m {\n    ($e:expr) => {{\n        let x: i32 = $e;\n        \
                 // SAFETY: `x` is only a number.\n        unsafe { f(x) }\n    }};\n}\n",
                6,
                "block",
            ),
            (
                "#[macro_export]\nmacro_rules! in_unsafe_fn {\n    ($($t:tt)*) => {\n        \
                 unsafe fn __body() { $($t)* }\n    };\n}\n",
                4,
                "fn",
            ),
            (
                "#[macro_export]\nmacro_rules! unmangled {\n    ($i:item) => {\n        \
                 #[unsafe(no_mangle)]\n        $i\n    };\n}\n",
                4,
                "attribute",
            ),
            (
                "#[macro_export]\nmacro_rules! pod {\n    ($t:ty) => {\n        unsafe trait Pod {}\n    \
                 };\n}\n",
                4,
                "trait",
            ),
            (
                "#[macro_export]\nmacro_rules! kw {\n    ($k:tt) => { unsafe $k };\n}\n",
                3,
                "keyword",
            ),
            (
                "#[macro_export]\nmacro_rules! ptr {\n    ($t:ty) => { type F = unsafe extern \"C\" fn($t); };\n}\n",
                3,
                "fn",
            ),
        ] {
            let name = text
                .split("macro_rules! ")
                .nth(1)
                .and_then(|rest| rest.split([' ', '(']).next())
                .unwrap_or("");
            assert_eq!(
                platform(text),
                [exported_wrapper(line, what, name)],
                "{text}"
            );
        }
        // An exported macro with two forms reports each.
        assert_eq!(
            platform(
                "#[macro_export]\nmacro_rules! both {\n    ($($t:tt)*) => {{\n        \
                 unsafe fn body() { $($t)* }\n        // SAFETY: x.\n        unsafe { body() }\n    \
                 }};\n}\n"
            ),
            [
                exported_wrapper(4, "fn", "both"),
                exported_wrapper(6, "block", "both")
            ]
        );
        // A body that never closes runs to the end of the file.
        assert_eq!(
            platform(
                "#[macro_export]\nmacro_rules! open {\n    ($e:expr) => {\n        // SAFETY: x.\n        \
                 unsafe { $e }\n"
            ),
            [exported_wrapper(5, "block", "open")]
        );
        // An `unsafe extern` block, which only sys may declare.
        assert_eq!(
            sys(
                "#[macro_export]\nmacro_rules! declare_c {\n    ($($t:tt)*) => {\n        \
                 unsafe extern \"C\" { $($t)* }\n    };\n}\n"
            ),
            [exported_wrapper(4, "`extern` block", "declare_c").replace(
                "crates/platform/src/window.rs",
                "crates/sys/src/linux/x11.rs"
            )]
        );
        // Not exported, wrapping only `$crate` paths, or no `unsafe` at all:
        // nothing to report.
        for text in [
            "macro_rules! local {\n    ($e:expr) => {{\n        // SAFETY: x.\n        unsafe { $e }\n    \
             }};\n}\n",
            "#[macro_export]\nmacro_rules! pid {\n    () => {{\n        // SAFETY: no preconditions.\n        \
             unsafe { $crate::ffi::getpid() }\n    }};\n}\n",
            "#[macro_export]\nmacro_rules! id {\n    ($e:expr) => { $e };\n}\n",
            "#[macro_export]\nmacro_rules! named {\n    ($e:expr) => { r#unsafe($e) };\n}\n",
        ] {
            assert_eq!(platform(text), Vec::<String>::new(), "{text}");
        }
        // In sys too.
        assert_eq!(
            sys("#[macro_export]\nmacro_rules! w {\n    ($e:expr) => {\n        // SAFETY: x.\n        \
                 unsafe { $e }\n    };\n}\n")
            .len(),
            1
        );
    }

    #[test]
    fn declarations_and_attributes_need_no_safety_comment() {
        let text = "/// Calls `g`.\n///\n/// # Safety\n///\n/// Never.\npub unsafe fn f() {}\n\
                    pub unsafe trait Pod {}\n#[unsafe(no_mangle)]\nextern \"C\" fn h() {}\n\
                    unsafe fn private() {}\npub type F = unsafe extern \"C\" fn(i32) -> i32;\n";
        assert_eq!(platform(text), Vec::<String>::new());
    }

    // --- `# Safety` sections ---

    #[test]
    fn a_pub_unsafe_fn_needs_a_safety_section() {
        let path = "crates/platform/src/window.rs";
        assert_eq!(
            platform("/// Reads `p`.\npub unsafe fn read(p: *const u8) -> u8 { 0 }"),
            [no_safety_section(path, 2)]
        );
        for text in [
            "pub unsafe fn f() {}",
            "pub(crate) unsafe fn f() {}",
            "pub(in crate::x) unsafe fn f() {}",
            "pub const unsafe fn f() {}",
            "pub async unsafe fn f() {}",
            "pub unsafe extern \"C\" fn f() {}",
            "/// # Safety is not a heading here.\npub unsafe fn f() {}",
            "/// Docs.\n#[inline]\n// # Safety\npub unsafe fn f() {}",
            "// # Safety\npub unsafe fn f() {}",
            "/// # Safety\n/// Fine.\nfn other() {}\npub unsafe fn f() {}",
            "/// ## Safety\npub unsafe fn f() {}",
        ] {
            assert_eq!(
                platform(text),
                [no_safety_section(path, text.lines().count())],
                "{text}"
            );
        }
    }

    #[test]
    fn a_safety_section_in_any_doc_form_passes() {
        for text in [
            "/// Reads `p`.\n///\n/// # Safety\n///\n/// `p` is valid.\npub unsafe fn f() {}",
            "/// # Safety\n#[inline]\n#[must_use]\npub unsafe fn f() -> u8 { 0 }",
            "/**\n * Reads.\n *\n * # Safety\n *\n * Valid.\n */\npub unsafe fn f() {}",
            "/** # Safety */ pub(crate) const unsafe fn f() {}",
            "#[doc = \"# Safety\"]\npub unsafe fn f() {}",
            "/// # Safety\n///\n/// Valid.\npub unsafe extern \"C\" fn f() {}",
            "impl X {\n    /// # Safety\n    /// Valid.\n    pub unsafe fn f(&self) {}\n}",
            // Not public: no section needed.
            "unsafe fn f() {}",
            "pub fn f() { g() }",
        ] {
            assert_eq!(platform(text), Vec::<String>::new(), "{text}");
        }
    }

    #[test]
    fn declarations_in_extern_blocks_follow_the_same_rule() {
        let text = "// source: unistd.h\nunsafe extern \"C\" {\n    /// See `pipe(2)`.\n    \
                    pub unsafe fn pipe(fds: *mut i32) -> i32;\n    \
                    /// See `close(2)`.\n    pub fn close(fd: i32) -> i32;\n}\n";
        assert_eq!(
            sys(text),
            [no_safety_section("crates/sys/src/linux/x11.rs", 4)]
        );
    }

    // --- `extern` blocks ---

    #[test]
    fn extern_blocks_belong_in_sys_and_are_unsafe() {
        assert_eq!(
            sys(
                "unsafe extern \"C\" {\n    pub fn XOpenDisplay() -> *mut u8;\n}\n\
                 unsafe extern \"system\" { pub fn GetLastError() -> u32; }\n\
                 unsafe extern { }\nextern crate alloc;\nextern \"C\" fn callback() {}\n"
            ),
            Vec::<String>::new()
        );
        assert_eq!(
            sys("extern \"C\" {\n    pub fn XOpenDisplay() -> *mut u8;\n}\n"),
            ["crates/sys/src/linux/x11.rs:1: `extern` block is not declared `unsafe extern`"]
        );
        assert_eq!(
            platform("unsafe extern \"C\" { fn f(); }\n"),
            [
                "crates/platform/src/window.rs:1: `extern` block outside tidepool-sys: raw OS \
              declarations live only there"
            ]
        );
        // Elsewhere both rules apply to `unsafe extern`, one to `extern`.
        assert_eq!(
            violations(&[(
                "crates/text/src/lib.rs",
                "unsafe extern \"C\" { fn f(); }\n"
            )])
            .len(),
            2
        );
        assert_eq!(
            violations(&[("crates/text/src/lib.rs", "extern \"C\" { fn f(); }\n")]),
            [
                "crates/text/src/lib.rs:1: `extern` block outside tidepool-sys: raw OS \
              declarations live only there"
            ]
        );
        assert_eq!(
            violations(&[(
                "crates/text/src/lib.rs",
                "extern crate std;\npub extern \"C\" fn f() {}\n"
            )]),
            Vec::<String>::new()
        );
    }

    // --- The `unsafe_code` lint ---

    #[test]
    fn the_crate_roots_may_allow_unsafe_code() {
        let report = audit(&[
            (
                "crates/sys/src/lib.rs",
                "//! Docs.\n#![allow(unsafe_code)]\n",
            ),
            ("crates/platform/src/lib.rs", "#![allow(unsafe_code)]\n"),
        ]);
        assert_eq!(report.violations, Vec::<String>::new());
    }

    #[test]
    fn nothing_else_may_allow_unsafe_code() {
        let not_allowed = |path: &str, line: usize, level: &str| {
            format!(
                "{path}:{line}: `{level}(unsafe_code)` is only allowed in the crate roots of \
                 tidepool-sys and tidepool-platform"
            )
        };
        for path in [
            "crates/text/src/lib.rs",
            "crates/app/src/main.rs",
            "xtask/src/main.rs",
            // Not the crate root, although it is an unsafe crate.
            "crates/sys/src/linux/x11.rs",
            "crates/sys/src/linux/lib.rs",
            "crates/platform/tests/lib.rs",
        ] {
            assert_eq!(
                violations(&[(path, "//! Docs.\n#![allow(unsafe_code)]\n")]),
                [not_allowed(path, 2, "allow")]
            );
        }

        let path = "crates/text/src/lib.rs";
        for (text, level) in [
            ("#[allow(unsafe_code)] mod ffi;", "allow"),
            ("#![allow(dead_code, unsafe_code)]", "allow"),
            ("#![allow(unsafe_code, dead_code)]", "allow"),
            ("#![allow(clippy::all, unsafe_code)]", "allow"),
            ("#![allow(unsafe_code, reason = \"FFI\")]", "allow"),
            ("#![allow( unsafe_code )]", "allow"),
            ("#![cfg_attr(not(test), allow(unsafe_code))]", "allow"),
            (
                "#![cfg_attr(all(unix, not(test)), allow(dead_code, unsafe_code))]",
                "allow",
            ),
            ("#[expect(unsafe_code)] mod ffi;", "expect"),
            ("#![warn(unsafe_code)]", "warn"),
            // Raw identifiers name the same lint and level.
            ("#![allow(r#unsafe_code)]", "allow"),
            ("#![r#allow(unsafe_code)]", "allow"),
            ("#![r#allow(r#unsafe_code)]", "allow"),
            ("#![cfg_attr(not(test), r#expect(unsafe_code))]", "expect"),
        ] {
            assert_eq!(
                violations(&[(path, text)]),
                [not_allowed(path, 1, level)],
                "{text}"
            );
        }
    }

    #[test]
    fn stricter_levels_and_mere_mentions_of_the_lint_are_fine() {
        for text in [
            "#![deny(unsafe_code)]",
            "#![forbid(unsafe_code)]",
            "#![deny(missing_docs, unsafe_code)]",
            "#![cfg_attr(not(test), deny(allow(x), unsafe_code))]",
            "// #![allow(unsafe_code)]",
            "const HELP: &str = \"#![allow(unsafe_code)]\";",
            "let unsafe_code = 1;",
            "check(unsafe_code);",
            "unsafe_code",
            "(unsafe_code)",
            "#![allow(dead_code)]",
        ] {
            assert_eq!(
                violations(&[("crates/text/src/lib.rs", text)]),
                Vec::<String>::new(),
                "{text}"
            );
        }
    }

    // --- The whole audit ---

    #[test]
    fn uses_are_counted_per_crate() {
        let report = audit(&[
            (
                "crates/sys/src/linux/libc.rs",
                "unsafe extern \"C\" { fn a(); }\n// unsafe in a comment\n",
            ),
            (
                "crates/platform/src/pty.rs",
                "// SAFETY: fine.\nlet x = unsafe { a() };\n// SAFETY: fine.\nunsafe impl Send for X {}\n\
                 pub unsafe trait Pod {}\n#[unsafe(no_mangle)]\nextern \"C\" fn f() {}\n",
            ),
            ("crates/text/src/lib.rs", "pub fn safe() {}\n"),
        ]);
        assert_eq!(report.violations, Vec::<String>::new());
        assert_eq!(
            report.notes,
            [
                "`unsafe` uses per crate (review any growth):",
                "  tidepool-sys           1",
                "  tidepool-platform      4",
                "Rust files scanned: 5",
                "crate roots checked: 0",
                "library crate roots that must forbid `unsafe` in doc tests: 0",
                "code blocks of documentation read for `unsafe`: 0",
            ]
        );
    }

    // --- Code the audit cannot see ---

    fn unreadable(path: &str, line: usize, what: &str) -> String {
        format!(
            "{path}:{line}: {what} does not name a relative `.rs` path inside this file's \
             directory (no `..`, no hidden or `target` directory, no escapes), so the code it \
             brings in may not be audited"
        )
    }

    #[test]
    fn a_module_path_or_include_outside_the_audit_is_a_violation() {
        let path = "crates/text/src/lib.rs";
        for (text, what) in [
            ("#[path = \"ffi.txt\"] mod ffi;", "`#[path = ...]`"),
            ("#[path = \"../.gen/m.rs\"] mod m;", "`#[path = ...]`"),
            ("#[path = \"../../sys/src/x.rs\"] mod x;", "`#[path = ...]`"),
            ("#[path = \"/tmp/x.rs\"] mod x;", "`#[path = ...]`"),
            ("#[path = \"C:/x.rs\"] mod x;", "`#[path = ...]`"),
            ("#[path = \"a\\\\b.rs\"] mod x;", "`#[path = ...]`"),
            ("#[path = \"gen/.hidden/x.rs\"] mod x;", "`#[path = ...]`"),
            ("#[path = \"target/x.rs\"] mod x;", "`#[path = ...]`"),
            (
                "#[cfg_attr(not(target_os = \"linux\"), path = \"Target/x.rs\")] mod x;",
                "`#[path = ...]`",
            ),
            ("include!(\"TARGET/x.rs\");", "`include!`"),
            ("#[path = \"a//x.rs\"] mod x;", "`#[path = ...]`"),
            ("#[path = b\"x.rs\"] mod x;", "`#[path = ...]`"),
            (
                "#[cfg_attr(unix, path = \"x.txt\")] mod x;",
                "`#[path = ...]`",
            ),
            ("#[r#path = \"x.txt\"] mod x;", "`#[path = ...]`"),
            (
                "#[path = concat!(\"x\", \".rs\")] mod x;",
                "`#[path = ...]`",
            ),
            ("include!(\"ffi.txt\");", "`include!`"),
            ("std::include!(\"../x.rs\");", "`include!`"),
            ("include![\"x.txt\"];", "`include!`"),
            (
                "include!(concat!(env!(\"OUT_DIR\"), \"/gen.rs\"));",
                "`include!`",
            ),
            (
                "macro_rules! m { ($f:literal) => { include!($f) } }",
                "`include!`",
            ),
            ("r#include!(\"x.txt\");", "`include!`"),
        ] {
            assert_eq!(
                violations(&[(path, text)]),
                [unreadable(path, 1, what)],
                "{text}"
            );
        }
        // The same in the two unsafe crates: a file they bring in must be
        // audited too, or a block in it could skip its `// SAFETY:`.
        assert_eq!(
            platform("#[path = \"x.txt\"] mod x;"),
            [unreadable(
                "crates/platform/src/window.rs",
                1,
                "`#[path = ...]`"
            )]
        );
    }

    #[test]
    fn a_module_path_or_include_the_audit_reads_is_fine() {
        for text in [
            "#[path = \"linux/mod.rs\"] mod os;",
            "#[cfg(test)]\n#[path = \"tests/x.rs\"]\nmod tests;",
            "#[cfg_attr(unix, path = \"unix.rs\")] mod imp;",
            "#[path = r\"plain.rs\"] mod plain;",
            "#[path = r#\"raw.rs\"#] mod raw;",
            "include!(\"generated/table.rs\");",
            "core::include! { \"table.rs\" }",
            // Not a file reference at all.
            "let path = \"../x.txt\";",
            "fn f(path: &str) {}",
            "struct S { path: String }",
            "#[doc = \"path = ../x\"] fn f() {}",
            "const S: &str = \"include!(\\\"x.txt\\\")\";",
            "// include!(\"x.txt\");",
            "include_str!(\"../README.md\");",
            "macro_rules! include { () => {} }",
        ] {
            assert_eq!(
                violations(&[("crates/text/src/lib.rs", text)]),
                Vec::<String>::new(),
                "{text}"
            );
        }
    }

    #[test]
    fn string_values_and_audited_paths() {
        assert_eq!(string_value("\"a/b.rs\""), Some("a/b.rs"));
        assert_eq!(string_value("r\"a.rs\""), Some("a.rs"));
        assert_eq!(string_value("r##\"a.rs\"##"), Some("a.rs"));
        assert_eq!(string_value("\"a\\x41.rs\""), None);
        assert_eq!(string_value("b\"a.rs\""), None);
        assert_eq!(string_value("'a'"), None);
        assert_eq!(string_value("r#\"open"), None);
        assert_eq!(string_value("\"open"), None);
        for good in [
            "x.rs",
            "a/b/c.rs",
            "linux/mod.rs",
            "a-b_c.rs",
            "Linux/Mod.rs",
            "targets/x.rs",
            "my.target/x.rs",
        ] {
            assert!(is_audited_relative_path(good), "{good}");
        }
        for bad in [
            "",
            "x",
            "x.txt",
            "x.rs/",
            "../x.rs",
            "./x.rs",
            "a/../x.rs",
            "/x.rs",
            ".x.rs",
            "a/.b/x.rs",
            "target/x.rs",
            "a/target/x.rs",
            // What case-insensitive file systems (macOS, Windows) and
            // Windows' path rules open as `target/x.rs` or `.claude/x.rs`.
            "Target/x.rs",
            "TARGET/x.rs",
            "a/tArGeT/x.rs",
            "target./x.rs",
            "target /x.rs",
            "target. ./x.rs",
            "CLAUDE~1/x.rs",
            "a/GIT~1/x.rs",
            "C:x.rs",
            "a//x.rs",
            "a\\x.rs",
        ] {
            assert!(!is_audited_relative_path(bad), "{bad}");
        }
    }

    #[test]
    fn rust_source_is_found_whatever_the_case_of_its_extension() {
        for name in ["lib.rs", "x.RS", "x.Rs", "MOD.rS"] {
            assert!(is_rust_source(name), "{name}");
        }
        for name in [".rs", "rs", "x.rs.txt", "x.rsx", "xrs", "x.r"] {
            assert!(!is_rust_source(name), "{name}");
        }
        // `mod x;` behind `cfg(not(target_os = "linux"))` compiles `x.RS`
        // on macOS and Windows: the audit reads it on every OS.
        let tree = TempTree::new("audit-case");
        tree.write("crates/sys/src/lib.rs", "#![allow(unsafe_code)]\n");
        tree.write("crates/platform/src/lib.rs", "#![allow(unsafe_code)]\n");
        tree.write(
            "crates/text/src/lib.rs",
            "#[cfg(not(target_os = \"linux\"))]\nmod x;\n",
        );
        tree.write("crates/text/src/x.RS", "fn f() { unsafe { g() } }\n");
        tree.write("crates/text/Cargo.TOML", "");
        let (audit, manifests) = audit_tree(tree.path(), MAX_FILE_BYTES).unwrap();
        assert_eq!(manifests, [tree.path().join("crates/text/Cargo.TOML")]);
        assert_eq!(
            audit.report().violations,
            [outside("crates/text/src/x.RS", 1)]
        );
    }

    #[test]
    fn a_linked_worktree_is_not_audited_and_is_named() {
        // A copy of the repository in `.claude/worktrees/x`, registered as
        // git does: its copy of sys's crate root is not at `crates/sys/`.
        let tree = TempTree::new("audit-worktree");
        tree.write("crates/sys/src/lib.rs", "#![allow(unsafe_code)]\n");
        tree.write("crates/platform/src/lib.rs", "#![allow(unsafe_code)]\n");
        let worktree = ".claude/worktrees/x";
        tree.write(
            &format!("{worktree}/crates/sys/src/lib.rs"),
            "#![allow(unsafe_code)]\n",
        );
        let real = fs::canonicalize(tree.path()).unwrap();
        tree.write(
            &format!("{worktree}/.git"),
            format!("gitdir: {}\n", real.join(".git/worktrees/x").display()),
        );
        tree.write(
            ".git/worktrees/x/gitdir",
            format!("{}\n", real.join(worktree).join(".git").display()),
        );
        let (audit, _) = audit_tree(tree.path(), MAX_FILE_BYTES).unwrap();
        let report = audit.report();
        assert_eq!(report.violations, Vec::<String>::new());
        assert_eq!(
            report.notes.last().map(String::as_str),
            Some(
                "skipped linked worktrees of this repository (run xtask in each to check it): \
                 .claude/worktrees/x"
            )
        );
        // Not registered (git's side gone): audited, and the copy fails.
        fs::remove_file(tree.path().join(".git/worktrees/x/gitdir")).unwrap();
        let (audit, _) = audit_tree(tree.path(), MAX_FILE_BYTES).unwrap();
        assert_eq!(
            audit.report().violations,
            [
                ".claude/worktrees/x/crates/sys/src/lib.rs:1: `allow(unsafe_code)` is only \
                 allowed in the crate roots of tidepool-sys and tidepool-platform"
            ]
        );
    }

    #[test]
    fn every_crate_root_lies_inside_its_package_directory() {
        // `text`'s library is the workspace root's `lib.rs`, whose `mod
        // target;` would compile the skipped `target/`, and its build script
        // is in `crates/`. `core`'s library leaves its package and comes
        // back, which is inside once resolved.
        let tree = TempTree::new("audit-root-outside");
        tree.write(
            "Cargo.toml",
            "[workspace]\nresolver = \"3\"\nmembers = [\"crates/text\", \"crates/core\"]\n",
        );
        tree.write("crates/sys/src/lib.rs", "#![allow(unsafe_code)]\n");
        tree.write("crates/platform/src/lib.rs", "#![allow(unsafe_code)]\n");
        let package = "[package]\nversion = \"0.1.0\"\nedition = \"2024\"\n";
        tree.write(
            "crates/text/Cargo.toml",
            format!(
                "{package}name = \"text\"\nbuild = \"../build.rs\"\n\n\
                 [lib]\npath = \"../../lib.rs\"\n"
            ),
        );
        tree.write("lib.rs", format!("{FORBID_IN_DOC_TESTS}\nmod target;\n"));
        tree.write("target/mod.rs", "pub fn f() { unsafe { g() } }\n");
        tree.write("crates/build.rs", "fn main() {}\n");
        tree.write(
            "crates/core/Cargo.toml",
            format!("{package}name = \"core\"\n\n[lib]\npath = \"../core/src/lib.rs\"\n"),
        );
        tree.write("crates/core/src/lib.rs", format!("{FORBID_IN_DOC_TESTS}\n"));
        let (mut audit, manifests) = audit_tree(tree.path(), MAX_FILE_BYTES).unwrap();
        audit.crate_roots(tree.path(), &manifests);
        let report = audit.report();
        let outside = |src: &str, kind: &str, name: &str| {
            format!(
                "{src}: the {kind} target `{name}` of `text` starts from a file outside its \
                 package directory crates/text: a crate root must lie in the directory of its \
                 package's Cargo.toml, or a plain `mod` declaration in it could compile files \
                 the audit skips (a workspace root's `target/`, a linked worktree)"
            )
        };
        assert_eq!(
            report.violations,
            [
                outside("lib.rs", "lib", "text"),
                outside("crates/build.rs", "custom-build", "build-script-build"),
            ]
        );
        assert!(
            report.notes.contains(&"crate roots checked: 3".to_string()),
            "{:?}",
            report.notes
        );
    }

    // --- Doc tests (ADR-0036) ---

    /// Whether `text` carries the attribute as [`File`] sees it.
    fn forbids(text: &str) -> bool {
        File::new(&lexer::lex(text)).forbids_unsafe_in_doc_tests()
    }

    #[test]
    fn only_the_exact_attribute_at_crate_level_forbids_unsafe_in_doc_tests() {
        for text in [
            "#![doc(test(attr(forbid(unsafe_code))))]",
            "//! Docs.\n\n// Why.\n#![doc(test(attr(forbid(unsafe_code))))]\n\npub mod a;\n",
            "#![allow(dead_code)]\n#![doc(test(attr(forbid(unsafe_code))))]\n",
            // Whitespace and comments between the tokens do not matter.
            "#! [ doc ( test ( attr ( forbid ( unsafe_code ) ) ) ) ]",
            "#![doc(\n    test(attr(forbid(unsafe_code)))\n)]",
            "#![doc(test(/* for README examples too */ attr(forbid(unsafe_code))))]",
            // After code at depth 0 that opens and closes what it opens.
            "#![cfg_attr(test, allow(dead_code))]\n#![doc(test(attr(forbid(unsafe_code))))]",
            // On line 1 it is an inner attribute, not a shebang: the first
            // token after the `#!` is `[`, past rustc's white space (vertical
            // tab and U+200E included) and comments that are not doc
            // comments, on any line. A byte-order mark before it is dropped.
            "\u{feff}#![doc(test(attr(forbid(unsafe_code))))]",
            "#!\n[doc(test(attr(forbid(unsafe_code))))]",
            "#!\u{b}\u{200e}[doc(test(attr(forbid(unsafe_code))))]",
            "#! // why\n[doc(test(attr(forbid(unsafe_code))))]",
            "#!/* why */[doc(test(attr(forbid(unsafe_code))))]",
            "#!/***/ //// \n[doc(test(attr(forbid(unsafe_code))))]",
            // After a shebang, which rustc drops, the next line is code: an
            // open quote there does not make the attribute a string.
            "#!/usr/bin/env x \"\n#![doc(test(attr(forbid(unsafe_code))))]\n// \"",
        ] {
            assert!(forbids(text), "{text}");
        }
        for text in [
            // Missing.
            "",
            "//! Docs.\n\npub mod a;\n",
            "#![forbid(unsafe_code)]",
            "#![doc(test(attr(forbid(unsafe_op_in_unsafe_fn))))]",
            // Not code.
            "// #![doc(test(attr(forbid(unsafe_code))))]",
            "/* #![doc(test(attr(forbid(unsafe_code))))] */",
            "//! ```\n//! #![doc(test(attr(forbid(unsafe_code))))]\n//! ```\n",
            "/// #![doc(test(attr(forbid(unsafe_code))))]\nfn f() {}",
            "const S: &str = \"#![doc(test(attr(forbid(unsafe_code))))]\";",
            "#![doc = \"test(attr(forbid(unsafe_code)))\"]",
            // Under `cfg_attr`.
            "#![cfg_attr(all(), doc(test(attr(forbid(unsafe_code)))))]",
            "#![cfg_attr(doctest, doc(test(attr(forbid(unsafe_code)))))]",
            // Other lints in the same `attr`, or anything else beside it.
            "#![doc(test(attr(forbid(unsafe_code, missing_docs))))]",
            "#![doc(test(attr(forbid(missing_docs, unsafe_code))))]",
            "#![doc(test(attr(forbid(unsafe_code), deny(warnings))))]",
            "#![doc(test(attr(deny(warnings), forbid(unsafe_code))))]",
            "#![doc(test(attr(forbid(unsafe_code,))))]",
            "#![doc(test(no_crate_inject, attr(forbid(unsafe_code))))]",
            // A level a doc test could relax.
            "#![doc(test(attr(deny(unsafe_code))))]",
            "#![doc(test(attr(warn(unsafe_code))))]",
            // An outer attribute, which documents one item.
            "#[doc(test(attr(forbid(unsafe_code))))]\nmod m {}",
            "#[doc(test(attr(forbid(unsafe_code))))]",
            // Not at crate level: in a module, a function, a macro.
            "mod m {\n    #![doc(test(attr(forbid(unsafe_code))))]\n}\n",
            "pub mod a {\n    pub mod b {\n        #![doc(test(attr(forbid(unsafe_code))))]\n    }\n}\n",
            "mod m {\n#![doc(test(attr(forbid(unsafe_code))))]",
            "fn f() {\n    #![doc(test(attr(forbid(unsafe_code))))]\n}",
            "macro_rules! m {\n    () => { #![doc(test(attr(forbid(unsafe_code))))] };\n}",
            "m!(#![doc(test(attr(forbid(unsafe_code))))]);",
            "m![#![doc(test(attr(forbid(unsafe_code))))]];",
            // Raw identifiers: rustc reads them, but this form is not the one.
            "#![r#doc(test(attr(forbid(unsafe_code))))]",
            "#![doc(r#test(attr(forbid(unsafe_code))))]",
            "#![doc(test(r#attr(forbid(unsafe_code))))]",
            "#![doc(test(attr(r#forbid(unsafe_code))))]",
            "#![doc(test(attr(forbid(r#unsafe_code))))]",
            // Cut short, or after a closer with nothing open.
            "#![doc(test(attr(forbid(unsafe_code))))",
            "#![doc(test(attr(forbid(unsafe_code)))]",
            "}\n#![doc(test(attr(forbid(unsafe_code))))]",
            // On a shebang line, which rustc drops: one that starts with
            // `#!` where the first token after it is not `[` (a doc comment
            // counts as one), up to its line break.
            "#!/x #![doc(test(attr(forbid(unsafe_code))))]",
            "#!#![doc(test(attr(forbid(unsafe_code))))]",
            "#!/usr/bin/env -S cargo +nightly -Zscript #![doc(test(attr(forbid(unsafe_code))))]",
            "\u{feff}#!/x #![doc(test(attr(forbid(unsafe_code))))]",
            "#!/x #![doc(test(attr(forbid(unsafe_code))))]\r\nfn f() {}\r\n",
            "#! /// a doc comment comes first [doc(test(attr(forbid(unsafe_code))))]",
            "#!//! a doc comment comes first\n[doc(test(attr(forbid(unsafe_code))))]",
            "#!/** a doc comment comes first */[doc(test(attr(forbid(unsafe_code))))]",
            "#!/*! a doc comment comes first */[doc(test(attr(forbid(unsafe_code))))]",
            "#!/* never closed [doc(test(attr(forbid(unsafe_code))))]",
            // A character outside ASCII that is not white space joins the
            // word before it, as it does for rustc: `unsafe_codé` is an
            // unknown lint, `attŕ` an unknown attribute.
            "#![doc(test(attr(forbid(unsafe_code\u{301}))))]",
            "#![doc(test(attr\u{301}(forbid(unsafe_code))))]",
            "#![doc(test\u{301}(attr(forbid(unsafe_code))))]",
            "#![doc(test(attr(forbid\u{301}(unsafe_code))))]",
            "#![doc\u{301}(test(attr(forbid(unsafe_code))))]",
            "#![doc(test(attr(forbid(unsafe_code\u{200d}))))]",
            "#![doc(test(attr(forbid(unsafe_code\u{200c}))))]",
            "#![doc(test(attr(forbid(unsafe_code\u{fe0f}))))]",
            "#![doc(test(attr(forbid(unsafe_code\u{ad}))))]",
            "#![doc(test(attr(forbid(unsafe_code\u{feff}))))]",
            "#![doc(test(attr(forbid(\u{301}unsafe_code))))]",
            "#![doc(test(attr(forbid(unsafe_codé))))]",
        ] {
            assert!(!forbids(text), "{text}");
        }
    }

    /// The violation for the library target `name` of the package
    /// `package`, of the kinds `kind`, whose crate root `src` lacks the
    /// attribute.
    fn lets_doc_tests_use_unsafe(src: &str, kind: &str, name: &str, package: &str) -> String {
        format!(
            "{src}: the {kind} target `{name}` of `{package}` does not forbid `unsafe` in its doc \
             tests, and rustdoc does not apply the workspace's `unsafe_code` lint to them. Add \
             this line to its crate root after the `//!` doc comment, exactly as written (an \
             inner attribute at crate level, not under `cfg_attr`, with nothing else in it): \
             `#![doc(test(attr(forbid(unsafe_code))))]` (ADR-0036)"
        )
    }

    /// The sorted violations and the notes of a whole audit of `tree`,
    /// crate roots included.
    fn audit_with_crate_roots(tree: &TempTree) -> (Vec<String>, Vec<String>) {
        let (mut audit, manifests) = audit_tree(tree.path(), MAX_FILE_BYTES).unwrap();
        audit.crate_roots(tree.path(), &manifests);
        let mut report = audit.report();
        report.violations.sort();
        (report.violations, report.notes)
    }

    #[test]
    fn every_library_crate_root_but_the_two_crates_forbids_unsafe_in_doc_tests() {
        let tree = TempTree::new("audit-doc-tests");
        tree.write(
            "Cargo.toml",
            "[workspace]\nresolver = \"3\"\nmembers = [\"crates/*\"]\n",
        );
        let package = |name: &str| {
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n")
        };
        // The two crates' libraries need nothing.
        tree.write("crates/sys/Cargo.toml", package("tidepool-sys"));
        tree.write("crates/sys/src/lib.rs", "#![allow(unsafe_code)]\n");
        tree.write("crates/platform/Cargo.toml", package("tidepool-platform"));
        tree.write("crates/platform/src/lib.rs", "#![allow(unsafe_code)]\n");
        // A library without it fails; its other targets need nothing: a
        // test, a build script, an example with a crate type of its own.
        tree.write(
            "crates/text/Cargo.toml",
            format!(
                "{}\n[[example]]\nname = \"e\"\ncrate-type = [\"staticlib\"]\n",
                package("text")
            ),
        );
        tree.write("crates/text/src/lib.rs", "//! Text.\n\npub fn f() {}\n");
        tree.write("crates/text/tests/t.rs", "#[test]\nfn t() {}\n");
        tree.write("crates/text/build.rs", "fn main() {}\n");
        tree.write("crates/text/examples/e.rs", "//! An example.\n");
        // Every crate type of a library counts.
        tree.write(
            "crates/ffi/Cargo.toml",
            format!("{}\n[lib]\ncrate-type = [\"cdylib\"]\n", package("ffi")),
        );
        tree.write("crates/ffi/src/lib.rs", "//! FFI.\n");
        // A binary needs nothing, beside a library or alone.
        tree.write("crates/both/Cargo.toml", package("both"));
        tree.write(
            "crates/both/src/lib.rs",
            format!("//! Both.\n\n// Why.\n{FORBID_IN_DOC_TESTS}\n\npub fn f() {{}}\n"),
        );
        tree.write("crates/both/src/main.rs", "fn main() {}\n");
        tree.write("crates/app/Cargo.toml", package("app"));
        tree.write("crates/app/src/main.rs", "fn main() {}\n");

        let (violations, notes) = audit_with_crate_roots(&tree);
        let mut expected = [
            lets_doc_tests_use_unsafe("crates/text/src/lib.rs", "lib", "text", "text"),
            lets_doc_tests_use_unsafe("crates/ffi/src/lib.rs", "cdylib", "ffi", "ffi"),
        ];
        expected.sort();
        assert_eq!(violations, expected);
        // sys 1, platform 1, text 4, ffi 1, both 2, app 1; the libraries of
        // text, ffi and both.
        for note in [
            "crate roots checked: 10",
            "library crate roots that must forbid `unsafe` in doc tests: 3",
        ] {
            assert!(notes.contains(&note.to_string()), "{note}: {notes:?}");
        }

        // With the attribute, they pass.
        for dir in ["text", "ffi"] {
            tree.write(
                &format!("crates/{dir}/src/lib.rs"),
                format!("//! Docs.\n{FORBID_IN_DOC_TESTS}\n"),
            );
        }
        assert_eq!(audit_with_crate_roots(&tree).0, Vec::<String>::new());
    }

    /// The violation for the proc-macro target `name` of `package`, whose
    /// crate root is `src`.
    fn proc_macro(src: &str, name: &str, package: &str) -> String {
        format!(
            "{src}: the proc-macro target `{name}` of `{package}` is a proc-macro crate, which no \
             workspace under the repository may have, tidepool-sys and tidepool-platform \
             included: a proc macro can make the `unsafe` keyword from a string, with a span for \
             which rustc skips the `unsafe_code` lint, and this audit reads only the string \
             (ADR-0037)"
        )
    }

    #[test]
    fn no_workspace_may_have_a_proc_macro_crate_sys_and_platform_included() {
        // Each gets that violation alone, with the doc-test attribute or
        // without it: a proc macro is no library to ask for the line.
        let tree = TempTree::new("audit-proc-macro");
        tree.write(
            "Cargo.toml",
            "[workspace]\nresolver = \"3\"\nmembers = [\"crates/*\"]\n",
        );
        let package = |name: &str| {
            format!(
                "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
                 [lib]\nproc-macro = true\n"
            )
        };
        tree.write("crates/sys/Cargo.toml", package("tidepool-sys"));
        tree.write("crates/sys/src/lib.rs", "#![allow(unsafe_code)]\n");
        tree.write("crates/platform/Cargo.toml", package("tidepool-platform"));
        tree.write("crates/platform/src/lib.rs", "#![allow(unsafe_code)]\n");
        tree.write("crates/gfx/Cargo.toml", package("gfx"));
        tree.write(
            "crates/gfx/src/lib.rs",
            format!("//! Gfx.\n{FORBID_IN_DOC_TESTS}\n"),
        );
        tree.write("crates/plain/Cargo.toml", package("plain"));
        tree.write("crates/plain/src/lib.rs", "//! Plain.\n");
        let (violations, notes) = audit_with_crate_roots(&tree);
        let mut expected = [
            proc_macro("crates/sys/src/lib.rs", "tidepool_sys", "tidepool-sys"),
            proc_macro(
                "crates/platform/src/lib.rs",
                "tidepool_platform",
                "tidepool-platform",
            ),
            proc_macro("crates/gfx/src/lib.rs", "gfx", "gfx"),
            proc_macro("crates/plain/src/lib.rs", "plain", "plain"),
        ];
        expected.sort();
        assert_eq!(violations, expected);
        assert!(
            notes.contains(
                &"library crate roots that must forbid `unsafe` in doc tests: 0".to_string()
            ),
            "{notes:?}"
        );
    }

    #[test]
    fn only_the_two_crates_by_name_and_directory_are_exempt() {
        // A package in sys's directory under another name, and one named
        // like platform elsewhere, are libraries like any other.
        let tree = TempTree::new("audit-doc-tests-exempt");
        tree.write(
            "Cargo.toml",
            "[workspace]\nresolver = \"3\"\nmembers = [\"crates/sys\", \"crates/plat\"]\n",
        );
        tree.write(
            "crates/sys/Cargo.toml",
            "[package]\nname = \"not-sys\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        );
        tree.write("crates/sys/src/lib.rs", "#![allow(unsafe_code)]\n");
        tree.write("crates/platform/src/lib.rs", "#![allow(unsafe_code)]\n");
        tree.write(
            "crates/plat/Cargo.toml",
            "[package]\nname = \"tidepool-platform\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        );
        tree.write("crates/plat/src/lib.rs", "//! Not platform.\n");
        let mut expected = [
            lets_doc_tests_use_unsafe("crates/sys/src/lib.rs", "lib", "not_sys", "not-sys"),
            lets_doc_tests_use_unsafe(
                "crates/plat/src/lib.rs",
                "lib",
                "tidepool_platform",
                "tidepool-platform",
            ),
        ];
        expected.sort();
        assert_eq!(audit_with_crate_roots(&tree).0, expected);
    }

    #[test]
    fn a_library_crate_root_the_audit_could_not_read_is_reported_only_as_that() {
        // Both carry the line, but the audit read neither: one is not
        // UTF-8, the other is over the bound. It cannot tell, and says so
        // once, rather than also asking for a line that is there.
        let tree = TempTree::new("audit-doc-tests-unread");
        tree.write(
            "Cargo.toml",
            "[workspace]\nresolver = \"3\"\nmembers = [\"crates/latin\", \"crates/big\"]\n",
        );
        tree.write("crates/sys/src/lib.rs", "#![allow(unsafe_code)]\n");
        tree.write("crates/platform/src/lib.rs", "#![allow(unsafe_code)]\n");
        let package = |name: &str| {
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n")
        };
        tree.write("crates/latin/Cargo.toml", package("latin"));
        tree.write(
            "crates/latin/src/lib.rs",
            [
                format!("{FORBID_IN_DOC_TESTS}\n").as_bytes(),
                b"//! Caf\xe9.\n",
            ]
            .concat(),
        );
        tree.write("crates/big/Cargo.toml", package("big"));
        tree.write(
            "crates/big/src/lib.rs",
            format!("{FORBID_IN_DOC_TESTS}\n{}\n", "// ".repeat(30)),
        );
        let (mut audit, manifests) = audit_tree(tree.path(), 64).unwrap();
        audit.crate_roots(tree.path(), &manifests);
        let report = audit.report();
        assert_eq!(
            report.violations,
            [
                "crates/big/src/lib.rs: larger than 64 bytes, so it was not audited",
                "crates/latin/src/lib.rs: not UTF-8, so it was not audited",
            ]
        );
        assert!(
            report.notes.contains(
                &"library crate roots that must forbid `unsafe` in doc tests: 0".to_string()
            ),
            "{:?}",
            report.notes
        );
    }

    #[test]
    fn library_kinds() {
        for kind in [
            "lib",
            "rlib",
            "dylib",
            "cdylib",
            "staticlib",
            "cdylib, rlib",
        ] {
            assert!(is_library(kind), "{kind}");
        }
        // A proc macro is banned instead (ADR-0037).
        for kind in [
            "proc-macro",
            "bin",
            "test",
            "bench",
            "example",
            "custom-build",
            "",
        ] {
            assert!(!is_library(kind), "{kind}");
        }
    }

    #[test]
    fn a_missing_crate_root_is_a_violation() {
        let mut audit = Audit::default();
        audit.file(
            "crates/sys/src/lib.rs",
            Path::new("crates/sys/src/lib.rs"),
            "#![allow(unsafe_code)]\n",
        );
        assert_eq!(
            audit.report().violations,
            ["crates/platform/src/lib.rs was not found: is this the repository root?"]
        );
        assert_eq!(Audit::default().report().violations.len(), 2);
    }

    #[test]
    fn the_tree_audit_reads_every_rust_file_and_fails_closed() {
        let tree = TempTree::new("audit-run");
        // A workspace root, whose `target` is build output.
        tree.write("Cargo.toml", "[workspace]\n");
        tree.write("crates/sys/src/lib.rs", "#![allow(unsafe_code)]\n");
        tree.write("crates/platform/src/lib.rs", "#![allow(unsafe_code)]\n");
        tree.write(
            "crates/text/src/deep/er/mod.rs",
            "unsafe impl Send for X {}\n",
        );
        tree.write("target/debug/build/out.rs", "unsafe { f() }\n");
        tree.write("crates/text/src/binary.rs", [0xff, 0xfe]);
        tree.write("build.rs", "fn main() { unsafe { f() } }\n");
        // Hidden directories are audited.
        tree.write("crates/text/.gen/m.rs", "#![allow(unsafe_code)]\n");
        // A file over the bound fails rather than being skipped.
        tree.write("crates/text/src/big.rs", "// ".repeat(30));
        let (audit, manifests) = audit_tree(tree.path(), 64).expect("the tree can be read");
        assert_eq!(manifests, [tree.path().join("Cargo.toml")]);
        let report = audit.report();
        assert_eq!(
            report.violations,
            [
                outside("build.rs", 1),
                "crates/text/.gen/m.rs:1: `allow(unsafe_code)` is only allowed in the crate roots \
                 of tidepool-sys and tidepool-platform"
                    .to_string(),
                "crates/text/src/big.rs: larger than 64 bytes, so it was not audited".to_string(),
                "crates/text/src/binary.rs: not UTF-8, so it was not audited".to_string(),
                outside("crates/text/src/deep/er/mod.rs", 1),
            ]
        );
        assert_eq!(report.notes[3], "Rust files scanned: 5");
    }

    #[test]
    fn a_symbolic_link_is_a_violation() {
        let tree = TempTree::new("audit-link");
        let outside_tree = TempTree::new("audit-link-outside");
        outside_tree.write("evil.rs", "#![allow(unsafe_code)]\nunsafe { f() }\n");
        tree.write("crates/sys/src/lib.rs", "#![allow(unsafe_code)]\n");
        tree.write("crates/platform/src/lib.rs", "#![allow(unsafe_code)]\n");
        let target = outside_tree.path().join("evil.rs");
        if let Err(reason) = symlink(&tree, "crates/text/src/s.rs", &target) {
            let _ = writeln!(
                std::io::stderr(),
                "SKIPPED: a_symbolic_link_is_a_violation: cannot make a link here: {reason}"
            );
            return;
        }
        let (audit, _) = audit_tree(tree.path(), MAX_FILE_BYTES).unwrap();
        assert_eq!(
            audit.report().violations,
            [
                "crates/text/src/s.rs: a symbolic link, which the audit does not follow: the code \
                 it leads to was not audited"
            ]
        );
    }

    #[test]
    fn the_real_repository_passes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let report = run(&root).expect("the real repository can be read");
        assert_eq!(report.violations, Vec::<String>::new());
    }
}
