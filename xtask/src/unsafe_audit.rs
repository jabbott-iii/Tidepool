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
//!   not under `crates/sys/` or `crates/platform/`.
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
//! - **An exported macro that wraps its caller's tokens in `unsafe`**: in
//!   the two crates, a `#[macro_export]` `macro_rules!` with an `unsafe`
//!   block or `unsafe impl` that holds a metavariable (`$e`, `$(...)`, not
//!   `$crate`). Expanded in another crate, the caller's code would run as
//!   `unsafe` with no `unsafe` token there: rustc does not apply the
//!   caller's `unsafe_code` lint to code from another crate's macro, so
//!   neither the lint nor this audit would see it, and no `// SAFETY:`
//!   comment in the macro can cover what callers pass.
//!
//! The rules are only as good as the set of files they are applied to, and
//! the compiler reads more than the `.rs` files a walk finds. So the audit
//! also fails, closed, on whatever could bring code in that it did not
//! read:
//!
//! - **A symbolic link** to a directory, or named `*.rs` or `Cargo.toml`
//!   (in any case), anywhere under the root: links are not followed.
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
//! - **A file larger than the size bound, or not UTF-8.**
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
//! `docs/04-quality.md` gives that to no check.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::check::Report;
use crate::error::Error;
use crate::lexer::{self, Comment, CommentKind, Doc, Lexed, Token, TokenKind};
use crate::metadata::{self, Resolve};
use crate::walk;

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
    for path in sources {
        let shown = walk::relative(root, &path);
        match walk::read_bounded(&path, limit) {
            Ok(text) => audit.file(&shown, &text),
            Err(walk::ReadError::TooLarge { limit }) => audit.violations.push(format!(
                "{shown}: larger than {limit} bytes, so it was not audited"
            )),
            Err(walk::ReadError::NotUtf8) => audit
                .violations
                .push(format!("{shown}: not UTF-8, so it was not audited")),
            Err(walk::ReadError::Io(source)) => return Err(Error::Read { path, source }),
        }
        audit.scanned.insert(metadata::canonical(&path));
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
    /// How many crate roots were checked.
    crate_roots: usize,
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
    /// between components.
    fn file(&mut self, path: &str, text: &str) {
        self.files += 1;
        let in_crate = |(_, dir): &(&str, &str)| {
            path.strip_prefix(dir)
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
            } else if token.is_name("macro_rules") && unsafe_crate.is_some() {
                for (line, what, name) in file.exported_unsafe_wrappers(index) {
                    self.violations.push(format!(
                        "{path}:{line}: `unsafe` {what} in the exported macro `{name}` wraps tokens \
                         its caller passes in (`$...`): they would run as `unsafe` in the caller's \
                         crate, where neither its `unsafe_code` lint nor this audit sees them, and \
                         no `// SAFETY:` comment here can cover them. Make it a function, or leave \
                         the `unsafe` to the caller"
                    ));
                }
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
    }

    /// Checks that the crate root of every target of every member of the
    /// workspaces that `manifests` belong to is a file the audit read.
    /// It also checks that each crate root lies inside its package
    /// directory ([`outside_package`]).
    fn crate_roots(&mut self, root: &Path, manifests: &[PathBuf]) {
        // The paths that rule compares are resolved, so its messages show
        // them relative to the resolved root.
        let real_root = metadata::canonical(root);
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
                if let Some(dir) = package.manifest_path.parent() {
                    packages.push((metadata::canonical(dir), package.name.clone()));
                }
                for target in &package.targets {
                    self.crate_roots += 1;
                    if !self
                        .scanned
                        .contains(&metadata::canonical(&target.src_path))
                    {
                        self.violations.push(format!(
                            "{}: the {} target `{}` of `{}` starts from a file the audit did not \
                             read; a crate root must be a `.rs` file in the repository, outside \
                             build output and not behind a symbolic link",
                            walk::relative(root, &target.src_path),
                            target.kind,
                            target.name,
                            package.name
                        ));
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
        notes.extend(self.worktrees);
        notes.extend(self.build_dir);
        Report {
            notes,
            violations: self.violations,
            ..Report::default()
        }
    }
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

    /// For the `macro_rules` at `index`, if it is `#[macro_export]`ed: each
    /// `unsafe` block or `unsafe impl` in its body that holds a
    /// metavariable other than `$crate`, as the line, what it is, and the
    /// macro's name.
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
        let Some(end) = self.matching_close(index + 3, open.text, close) else {
            return found;
        };
        for at in index + 4..end {
            let Some(token) = self.token(at).filter(|token| token.is_word("unsafe")) else {
                continue;
            };
            let extent = match self.token(at + 1) {
                Some(next) if next.is_punct("{") => self
                    .matching_close(at + 1, "{", "}")
                    .map(|close| ("block", close)),
                Some(next) if next.is_word("impl") => (at + 2..end)
                    .find(|&brace| self.token(brace).is_some_and(|t| t.is_punct("{")))
                    .and_then(|brace| self.matching_close(brace, "{", "}"))
                    .map(|close| ("impl", close)),
                _ => None,
            };
            let Some((what, last)) = extent else {
                continue;
            };
            let wraps = (at + 1..last).any(|dollar| {
                self.token(dollar).is_some_and(|t| t.is_punct("$"))
                    && !self
                        .token(dollar + 1)
                        .is_some_and(|after| after.is_word("crate"))
            });
            if wraps {
                found.push((token.line, what, name.text));
            }
        }
        found
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
                audit.file(&root, "#![allow(unsafe_code)]\n");
            }
        }
        for (path, text) in files {
            audit.file(path, text);
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
                    /// ```\n/// unsafe { f() }\n/// ```\n\
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
        ] {
            assert_eq!(platform(text).len(), 1, "{text}");
        }
    }

    fn exported_wrapper(line: usize, what: &str, name: &str) -> String {
        format!(
            "crates/platform/src/window.rs:{line}: `unsafe` {what} in the exported macro `{name}` \
             wraps tokens its caller passes in (`$...`): they would run as `unsafe` in the \
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
        // Not exported, wrapping only `$crate` paths, a metavariable outside
        // the block, or no `unsafe` at all: nothing to report.
        for text in [
            "macro_rules! local {\n    ($e:expr) => {{\n        // SAFETY: x.\n        unsafe { $e }\n    \
             }};\n}\n",
            "#[macro_export]\nmacro_rules! pid {\n    () => {{\n        // SAFETY: no preconditions.\n        \
             unsafe { $crate::ffi::getpid() }\n    }};\n}\n",
            "#[macro_export]\nmacro_rules! m {\n    ($e:expr) => {{\n        let x: i32 = $e;\n        \
             // SAFETY: `x` is only a number.\n        unsafe { f(x) }\n    }};\n}\n",
            "#[macro_export]\nmacro_rules! id {\n    ($e:expr) => { $e };\n}\n",
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
        tree.write("lib.rs", "mod target;\n");
        tree.write("target/mod.rs", "pub fn f() { unsafe { g() } }\n");
        tree.write("crates/build.rs", "fn main() {}\n");
        tree.write(
            "crates/core/Cargo.toml",
            format!("{package}name = \"core\"\n\n[lib]\npath = \"../core/src/lib.rs\"\n"),
        );
        tree.write("crates/core/src/lib.rs", "");
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

    #[test]
    fn a_missing_crate_root_is_a_violation() {
        let mut audit = Audit::default();
        audit.file("crates/sys/src/lib.rs", "#![allow(unsafe_code)]\n");
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
