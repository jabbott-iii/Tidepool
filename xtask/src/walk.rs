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

//! Finding the files a check reads, and reading them within a size bound.
//!
//! Ported from nadir xtask `unsafe_audit.rs` (`rust_files` and
//! `relative`). Adapted for Tidepool: shared by `deps`, `unsafe-audit` and
//! `headers`, with a predicate on the file name; hidden directories are
//! searched (only the repository's own `.git` is skipped, as
//! `scripts/check-deps.sh` does); the `target` directory of every workspace
//! root is skipped, not only the top-level one (so `agents/target` is
//! skipped too), and no other; a linked worktree of the same repository is
//! skipped and reported; every symbolic link is reported, never resolved,
//! instead of silently ignored; and paths are shown with their control
//! characters escaped ([`escape_controls`]).
//!
//! Nothing the repository holds decides by itself what is skipped: cargo
//! says which directories are workspace roots, git's own records which
//! directories are worktrees, and the build directory the environment names
//! (which a committed `.cargo/config.toml` `[env]` table could set) is
//! searched like any other directory unless it is a workspace root's
//! `target`.

use std::borrow::Cow;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use crate::cargo;
use crate::error::Error;

/// What [`scan`] found under a root, each list sorted by path, so that
/// output does not depend on the order the OS lists directories in.
#[derive(Debug, Default)]
pub(crate) struct Tree {
    /// The regular files whose name the predicate accepts.
    pub(crate) files: Vec<PathBuf>,
    /// Every symbolic link, whatever its name. A link is never followed, so
    /// what it leads to was not checked; nor is it resolved to see what it
    /// leads to, which would touch its target (on Windows, a link to a UNC
    /// path would make the OS contact that host).
    pub(crate) links: Vec<PathBuf>,
    /// The linked worktrees of this repository that were skipped (see
    /// [`scan`]), for a note: each is checked when `xtask` runs in it.
    pub(crate) worktrees: Vec<PathBuf>,
    /// The build directory the environment names ([`target_dir`]), if it
    /// lies under the root and was searched, for a note: whatever cargo
    /// built there is checked as if it were part of the repository.
    pub(crate) searched_build_dir: Option<PathBuf>,
}

/// The largest `.git` file or worktree `gitdir` file read, in bytes. Git
/// writes one line holding a path.
const GIT_FILE_BYTES: u64 = 64 * 1024;

/// Collects the files under `root` whose name `wanted` accepts, and every
/// symbolic link ([`Tree::links`]).
///
/// Skipped: the directory `.git` directly under `root` (the repository's
/// own history), the directory named `target` next to the `Cargo.toml` of
/// a workspace root, as `cargo locate-project --workspace` reports it
/// (cargo's build output, where the tests also generate their fixtures),
/// and a linked worktree of the repository at `root`
/// ([`is_linked_worktree`]), which is listed in [`Tree::worktrees`]. Every
/// other directory is searched, hidden ones included: a `target` directory
/// next to the manifest of a workspace member or of a package cargo cannot
/// read (cargo never builds there), one that is not next to a manifest
/// (such as a module `src/target/`), the build directory the environment
/// names when it is anything else (noted in [`Tree::searched_build_dir`]),
/// and a nested `.git` directory or submodule.
///
/// # Errors
///
/// [`Error::Read`] if a directory cannot be listed.
pub(crate) fn scan(root: &Path, wanted: fn(&str) -> bool) -> Result<Tree, Error> {
    scan_with(root, wanted, false)
}

/// [`scan`], but every regular file is collected, whatever its name.
///
/// # Errors
///
/// [`Error::Read`] if a directory cannot be listed.
pub(crate) fn scan_every_file(root: &Path) -> Result<Tree, Error> {
    scan_with(root, |_| true, true)
}

fn scan_with(root: &Path, wanted: fn(&str) -> bool, every_file: bool) -> Result<Tree, Error> {
    let skip = Skip {
        build_dir: build_dir(),
        worktrees: worktrees_dir(root),
        is_workspace_root: is_workspace_root_by_cargo,
        every_file,
    };
    let mut tree = Tree::default();
    collect(root, root, true, wanted, &skip, &mut tree)?;
    tree.files.sort();
    tree.links.sort();
    tree.worktrees.sort();
    Ok(tree)
}

/// What [`collect`] skips besides the fixed names, and what it collects.
#[derive(Debug)]
struct Skip {
    /// The resolved build directory ([`build_dir`]): never skipped for
    /// being it, only noted when searched.
    build_dir: Option<PathBuf>,
    /// The resolved directory this repository's git keeps its linked
    /// worktrees under ([`worktrees_dir`]).
    worktrees: Option<PathBuf>,
    /// Whether the directory (second argument), which holds a
    /// `Cargo.toml`, is a workspace root, asked from the repository root
    /// (first argument): [`is_workspace_root_by_cargo`] outside tests.
    is_workspace_root: fn(&Path, &Path) -> bool,
    /// Whether every regular file is collected, not only wanted ones.
    every_file: bool,
}

/// Whether cargo, run in `root`, says that `dir/Cargo.toml` is the root
/// manifest of its workspace: `cargo locate-project --workspace` names it.
/// Cargo builds only into a workspace root's `target` directory, never
/// into a member's. If cargo cannot tell (an invalid manifest, a package
/// that belongs to no workspace), the answer is no, so the directory is
/// searched. This reads manifests only: it compiles nothing and does not
/// touch a lockfile.
fn is_workspace_root_by_cargo(root: &Path, dir: &Path) -> bool {
    let manifest = dir.join("Cargo.toml");
    let shown = manifest.to_string_lossy();
    let args = [
        "locate-project",
        "--workspace",
        "--message-format",
        "plain",
        "--manifest-path",
        &shown,
    ];
    let Ok(output) = cargo::capture(root, &args) else {
        return false;
    };
    let output = String::from_utf8_lossy(&output);
    let Some(found) = output.lines().next() else {
        return false;
    };
    match (fs::canonicalize(found), fs::canonicalize(&manifest)) {
        (Ok(found), Ok(manifest)) => found == manifest,
        _ => false,
    }
}

/// The build directory that the environment names, absolute, whether or
/// not it exists: `CARGO_TARGET_DIR`, or else `CARGO_BUILD_TARGET_DIR`
/// (the environment form of `build.target-dir`), as cargo ranks them.
///
/// Cargo resolves a relative value against the directory it was started
/// in. Cargo starts `xtask` (`cargo xtask`, which is `cargo run`) in that
/// same directory, so this process resolves it the same way, against its
/// current directory. The cargo children `xtask` starts run in the
/// repository root, and the test binaries those start run in their
/// package's directory, where a relative value would name another
/// directory; so `xtask` passes this absolute path on to every cargo child
/// ([`crate::cargo`]).
pub(crate) fn target_dir() -> Option<PathBuf> {
    let cwd = env::current_dir().ok()?;
    absolute_target_dir(
        env::var_os("CARGO_TARGET_DIR"),
        env::var_os("CARGO_BUILD_TARGET_DIR"),
        &cwd,
    )
}

/// [`target_dir`] for the given variables and current directory.
fn absolute_target_dir(
    target_dir: Option<OsString>,
    build_target_dir: Option<OsString>,
    cwd: &Path,
) -> Option<PathBuf> {
    let dir = target_dir
        .filter(|dir| !dir.is_empty())
        .or_else(|| build_target_dir.filter(|dir| !dir.is_empty()))?;
    // `join` keeps an absolute `dir` as it is.
    Some(cwd.join(dir))
}

/// The build directory [`target_dir`] names, resolved, if it exists.
///
/// It is not skipped for being named: a committed `.cargo/config.toml` can
/// set `CARGO_TARGET_DIR` for `cargo xtask` in its `[env]` table, so the
/// name could hide any directory. It is skipped only if it is a workspace
/// root's `target` directory, like any other; otherwise the checks search
/// it, and fail on whatever cargo built or the tests generated there. A
/// build directory set some other way (`build.target-dir` in a cargo
/// configuration file) is treated the same.
pub(crate) fn build_dir() -> Option<PathBuf> {
    fs::canonicalize(target_dir()?).ok()
}

/// The resolved `worktrees` directory of the git repository checked out at
/// `root`, if it has one: under `root/.git` when that is a directory, or,
/// when `root` is itself a linked worktree (`root/.git` is a file), under
/// the common git directory its own git directory's `commondir` names.
fn worktrees_dir(root: &Path) -> Option<PathBuf> {
    let dot_git = root.join(".git");
    let kind = fs::symlink_metadata(&dot_git).ok()?.file_type();
    let common = if kind.is_dir() {
        dot_git
    } else if kind.is_file() {
        let git_dir = git_file_target(&dot_git, root, "gitdir:")?;
        match git_file_target(&git_dir.join("commondir"), &git_dir, "") {
            Some(common) => common,
            None => git_dir,
        }
    } else {
        return None;
    };
    fs::canonicalize(common.join("worktrees")).ok()
}

/// The path in the first line of the small git file `file`, after
/// `prefix`, joined to `base` when it is relative. Git writes these files
/// with `/` on every OS, which `Path` reads on every OS.
fn git_file_target(file: &Path, base: &Path, prefix: &str) -> Option<PathBuf> {
    let kind = fs::symlink_metadata(file).ok()?.file_type();
    if !kind.is_file() {
        return None;
    }
    let text = read_bounded(file, GIT_FILE_BYTES).ok()?;
    let target = text.lines().next()?.strip_prefix(prefix)?.trim();
    (!target.is_empty()).then(|| base.join(target))
}

/// Whether `dir`, below the root, is a linked worktree of the repository
/// whose resolved `worktrees` directory is `worktrees`: `dir/.git` is a
/// regular file whose `gitdir:` names an entry directly in `worktrees`,
/// and that entry's `gitdir` file names `dir/.git` back. That is how
/// `git worktree add` records a worktree on both sides; Claude Code puts
/// its worktrees in `.claude/worktrees/<name>/`, inside the checkout.
/// `dir/.git` itself must be a regular file, not a symbolic link to some
/// worktree's `.git` file.
///
/// A worktree is a whole checkout of the same repository, so its manifests
/// and sources would be checked twice, and against the wrong root: its
/// copy of `crates/sys/src/lib.rs` is not at `crates/sys/`, and another
/// session's half-edited lockfile would fail `deps` here. `xtask` checks a
/// worktree when it runs in it.
///
/// Skipping one does not let its code into this checkout's build
/// unchecked. Cargo builds it as a path package or a workspace member,
/// which `deps`, `layers` and the audit's crate-root check see through
/// `cargo metadata` whatever the walk skipped; or as module files of a
/// crate whose package directory holds the worktree, which the audit
/// reports as a violation; and the audit rejects a `#[path]` or `include!`
/// through a hidden directory, where Claude Code puts its worktrees. Nor
/// can the repository's content fake one: git does not track a path named
/// `.git`, and a fresh clone (CI) has no `worktrees` directory, so there
/// nothing is skipped. A nested `.git` directory, and a submodule's `.git`
/// file (which names `.git/modules/...`), are searched as before.
fn is_linked_worktree(dir: &Path, worktrees: &Path) -> bool {
    let dot_git = dir.join(".git");
    if !fs::symlink_metadata(&dot_git).is_ok_and(|meta| meta.file_type().is_file()) {
        return false;
    }
    let Some(entry) =
        git_file_target(&dot_git, dir, "gitdir:").and_then(|entry| fs::canonicalize(entry).ok())
    else {
        return false;
    };
    if entry.parent() != Some(worktrees) {
        return false;
    }
    let back = git_file_target(&entry.join("gitdir"), &entry, "")
        .and_then(|back| fs::canonicalize(back).ok());
    back.is_some() && back == fs::canonicalize(&dot_git).ok()
}

fn collect(
    root: &Path,
    dir: &Path,
    is_root: bool,
    wanted: fn(&str) -> bool,
    skip: &Skip,
    tree: &mut Tree,
) -> Result<(), Error> {
    let read_error = |source| Error::Read {
        path: dir.to_path_buf(),
        source,
    };
    // `symlink_metadata`, not `is_file`, which would follow a linked
    // manifest to its target; such a link is not a manifest here, so a
    // `target` beside it is searched.
    let has_manifest =
        fs::symlink_metadata(dir.join("Cargo.toml")).is_ok_and(|meta| meta.file_type().is_file());
    // Asked only if `dir` has a `target` directory, and then once.
    let mut workspace_root = None;
    for entry in fs::read_dir(dir).map_err(read_error)? {
        let entry = entry.map_err(read_error)?;
        // Does not follow symbolic links: a link is neither a file nor a
        // directory here.
        let file_type = entry.file_type().map_err(read_error)?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let path = entry.path();
        if file_type.is_dir() {
            if is_root && name == ".git" {
                continue;
            }
            if has_manifest
                && name == "target"
                && *workspace_root.get_or_insert_with(|| (skip.is_workspace_root)(root, dir))
            {
                continue;
            }
            if skip
                .worktrees
                .as_deref()
                .is_some_and(|worktrees| is_linked_worktree(&path, worktrees))
            {
                tree.worktrees.push(path);
                continue;
            }
            if is_build_dir(&path, skip.build_dir.as_deref()) {
                tree.searched_build_dir = Some(path.clone());
            }
            collect(root, &path, false, wanted, skip, tree)?;
        } else if file_type.is_symlink() {
            // Not resolved: `fs::metadata` would follow it to its target.
            tree.links.push(path);
        } else if file_type.is_file() && (skip.every_file || wanted(&name)) {
            tree.files.push(path);
        }
    }
    Ok(())
}

/// Whether `dir` is the resolved build directory `build`. Only a directory
/// with the same name is resolved, so the walk does not resolve every
/// directory.
fn is_build_dir(dir: &Path, build: Option<&Path>) -> bool {
    build.is_some_and(|build| {
        build.file_name() == dir.file_name()
            && fs::canonicalize(dir).is_ok_and(|resolved| resolved == build)
    })
}

/// `path` relative to `root`, with `/` between components on every OS, for
/// messages and for matching crate directories. A path outside `root` is
/// shown whole, as the OS writes it. Either way its control characters are
/// escaped ([`escape_controls`]): a file name is the repository's to
/// choose, and printed as it is, a newline in one would add a line of its
/// own to the output.
pub(crate) fn relative(root: &Path, path: &Path) -> String {
    let shown = match path.strip_prefix(root) {
        Ok(inside) => {
            let components: Vec<_> = inside
                .components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect();
            components.join("/")
        }
        Err(_) => path.display().to_string(),
    };
    match escape_controls(&shown, |_| false) {
        Cow::Borrowed(_) => shown,
        Cow::Owned(escaped) => escaped,
    }
}

/// `path` whole, as the OS writes it, with its control characters escaped
/// ([`escape_controls`]): for a path that is not shown relative to a root.
pub(crate) fn shown(path: &Path) -> String {
    let whole = path.display().to_string();
    match escape_controls(&whole, |_| false) {
        Cow::Borrowed(_) => whole,
        Cow::Owned(escaped) => escaped,
    }
}

/// Whether `c` is one [`escape_controls`] escapes: a control character
/// ([`char::is_control`]: C0, DEL and C1), the line or paragraph separator,
/// which a browser or terminal may draw as a line break, or a
/// bidirectional control, which reorders what follows it on the line.
fn is_escaped(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{2028}'
                | '\u{2029}'
                | '\u{061c}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
        )
}

/// `text` with each character [`is_escaped`] accepts and `keep` does not
/// written as its Rust escape (`\n`, `\r`, `\u{1b}`, `\u{202e}`), so that it
/// can neither start a line of output, which GitHub Actions would read as a
/// workflow command such as `::warning::`, nor send an escape sequence to a
/// terminal, nor change how the rest of the line is drawn. Any other
/// character, including a backslash, is kept as it is.
pub(crate) fn escape_controls(text: &str, keep: fn(char) -> bool) -> Cow<'_, str> {
    if !text.chars().any(|c| is_escaped(c) && !keep(c)) {
        return Cow::Borrowed(text);
    }
    let mut escaped = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        if is_escaped(c) && !keep(c) {
            escaped.extend(c.escape_default());
        } else {
            escaped.push(c);
        }
    }
    Cow::Owned(escaped)
}

/// The note that names the linked worktrees a [`scan`] of `root` skipped,
/// if it skipped any.
pub(crate) fn worktrees_note(root: &Path, worktrees: &[PathBuf]) -> Option<String> {
    if worktrees.is_empty() {
        return None;
    }
    let shown: Vec<String> = worktrees
        .iter()
        .map(|worktree| relative(root, worktree))
        .collect();
    Some(format!(
        "skipped linked worktrees of this repository (run xtask in each to check it): {}",
        shown.join(", ")
    ))
}

/// The note that says a [`scan`] of `root` searched the build directory the
/// environment names, if it did.
pub(crate) fn build_dir_note(root: &Path, tree: &Tree) -> Option<String> {
    let dir = tree.searched_build_dir.as_deref()?;
    Some(format!(
        "the build directory CARGO_TARGET_DIR (or CARGO_BUILD_TARGET_DIR) names, {}, is inside \
         the repository and is not a workspace root's `target` directory, so it was searched \
         like the rest of the repository: whatever cargo built there is checked. Use a build \
         directory outside the repository, or `target` at its root",
        relative(root, dir)
    ))
}

/// Why [`read_bounded`] could not give the text of a file.
#[derive(Debug)]
pub(crate) enum ReadError {
    /// The file is larger than the bound.
    TooLarge {
        /// The bound, in bytes.
        limit: u64,
    },
    /// The file is not UTF-8.
    NotUtf8,
    /// The OS could not read it.
    Io(io::Error),
}

/// Reads `path` as UTF-8 text of at most `limit` bytes. A file that grows
/// while it is read is still cut off at the bound.
pub(crate) fn read_bounded(path: &Path, limit: u64) -> Result<String, ReadError> {
    let file = fs::File::open(path).map_err(ReadError::Io)?;
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(ReadError::Io)?;
    if bytes.len() as u64 > limit {
        return Err(ReadError::TooLarge { limit });
    }
    String::from_utf8(bytes).map_err(|_| ReadError::NotUtf8)
}

/// [`read_required`] for the file at `path`, relative to `root` with `/`
/// between components, which a check reads by its fixed name (the
/// allowlist, the dependency matrix): each component on the way, the file
/// included, must not be a symbolic link. The walk reports links without
/// opening them; this keeps a linked input from being read, and quoted,
/// before that.
///
/// # Errors
///
/// [`Error::Read`] if a component is a link, or the file cannot be read.
pub(crate) fn read_unlinked(root: &Path, path: &str, limit: u64) -> Result<String, Error> {
    let mut at = root.to_path_buf();
    for component in path.split('/') {
        at.push(component);
        if fs::symlink_metadata(&at).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err(Error::Read {
                path: root.join(path),
                source: io::Error::other(format!(
                    "{} is a symbolic link, which xtask does not follow",
                    relative(root, &at)
                )),
            });
        }
    }
    read_required(&root.join(path), limit)
}

/// [`read_bounded`] for a file a check cannot do without: every way it can
/// fail is an [`Error::Read`] that names the file.
pub(crate) fn read_required(path: &Path, limit: u64) -> Result<String, Error> {
    read_bounded(path, limit).map_err(|error| Error::Read {
        path: path.to_path_buf(),
        source: match error {
            ReadError::Io(source) => source,
            ReadError::TooLarge { limit } => {
                io::Error::other(format!("the file is larger than the {limit}-byte limit"))
            }
            ReadError::NotUtf8 => io::Error::other("the file is not UTF-8"),
        },
    })
}

#[cfg(test)]
pub(crate) mod testing {
    //! A scratch directory per test, removed when the test ends.

    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Distinguishes the directories one process makes.
    static NEXT: AtomicU32 = AtomicU32::new(0);

    /// A new directory, removed when dropped.
    ///
    /// It lives under the repository's own `target/xtask-unit-tmp/`, not
    /// the shared system temporary directory, so no other user can plant
    /// anything where it will be. Its name has the process id, a counter
    /// and the clock in it, and it is made with `fs::create_dir`, which
    /// fails if anything (a symbolic link included) is already there.
    pub(crate) struct TempTree {
        root: PathBuf,
    }

    impl TempTree {
        /// Creates an empty directory for the test `name`.
        pub(crate) fn new(name: &str) -> Self {
            let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/xtask-unit-tmp");
            fs::create_dir_all(&parent).expect("create the directory for unit-test trees");
            let parent_type = fs::symlink_metadata(&parent)
                .expect("the directory for unit-test trees exists")
                .file_type();
            assert!(
                parent_type.is_dir() && !parent_type.is_symlink(),
                "{} must be a directory, not a link",
                parent.display()
            );
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.subsec_nanos());
            let root = parent.join(format!(
                "{}-{}-{nanos:09}-{name}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).expect("create a new temporary directory");
            Self { root }
        }

        /// The directory.
        pub(crate) fn path(&self) -> &Path {
            &self.root
        }

        /// Writes `contents` to `path`, relative to the directory, creating
        /// directories on the way.
        pub(crate) fn write(&self, path: &str, contents: impl AsRef<[u8]>) {
            let path = self.root.join(path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create the parent directory");
            }
            fs::write(path, contents).expect("write the file");
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    /// Makes `link` (relative to `tree`) a symbolic link to `target`.
    /// Returns the reason when this OS or account cannot make links; the
    /// caller then reports the test as skipped.
    pub(crate) fn symlink(tree: &TempTree, link: &str, target: &Path) -> Result<(), String> {
        let link = tree.path().join(link);
        if let Some(parent) = link.parent() {
            fs::create_dir_all(parent).expect("create the parent directory");
        }
        // `fs::soft_link` is deprecated in favour of one function per OS,
        // but those would need a platform `cfg`, which xtask's checks must
        // not have (`AGENTS.md` rule 5). This one works everywhere links do.
        // On Windows it always makes a file symbolic link, even to a
        // directory, so these tests never make a directory link or a
        // junction there: those are not covered on Windows.
        #[allow(deprecated)]
        fs::soft_link(target, &link).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::testing::{TempTree, symlink};
    use super::*;

    fn is_rust(name: &str) -> bool {
        name.ends_with(".rs")
    }

    fn found(tree: &TempTree) -> Vec<String> {
        scan(tree.path(), is_rust)
            .expect("the tree can be listed")
            .files
            .iter()
            .map(|path| relative(tree.path(), path))
            .collect()
    }

    #[test]
    fn finds_wanted_files_in_path_order() {
        let tree = TempTree::new("walk-order");
        for path in [
            "b/z.rs",
            "a/y.rs",
            "a/b/x.rs",
            "top.rs",
            "notes.md",
            "a/lib.rs.orig",
        ] {
            tree.write(path, "");
        }
        assert_eq!(found(&tree), ["a/b/x.rs", "a/y.rs", "b/z.rs", "top.rs"]);
    }

    #[test]
    fn skips_workspace_build_output_and_the_repository_s_git_directory_only() {
        // Real manifests, since cargo says which is a workspace root.
        let tree = TempTree::new("walk-skip");
        tree.write(
            "Cargo.toml",
            "[workspace]\nresolver = \"3\"\nmembers = [\"crates/m\"]\n",
        );
        tree.write("target/debug/build/out.rs", "");
        tree.write(".git/hooks/sample.rs", "");
        tree.write("agents/Cargo.toml", "[workspace]\n");
        tree.write("agents/target/debug/out.rs", "");
        // Cargo never builds into a member's `target`: a nested workspace
        // there is searched.
        tree.write(
            "crates/m/Cargo.toml",
            "[package]\nname = \"m\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        );
        tree.write("crates/m/src/lib.rs", "");
        tree.write("crates/m/target/w/src/lib.rs", "");
        // So is the `target` beside a manifest cargo cannot read.
        tree.write("broken/Cargo.toml", "not toml");
        tree.write("broken/target/x.rs", "");
        // Hidden directories are searched, and so is a nested `.git`.
        tree.write(".cargo-home/registry/src/x.rs", "");
        tree.write(".devtools/gen/build.rs", "");
        tree.write("vendor/sub/.git/x.rs", "");
        // A `target` directory that is not next to a manifest is source.
        tree.write("crates/render/src/target/mod.rs", "");
        tree.write("crates/target/src/lib.rs", "");
        assert_eq!(
            found(&tree),
            [
                ".cargo-home/registry/src/x.rs",
                ".devtools/gen/build.rs",
                "broken/target/x.rs",
                "crates/m/src/lib.rs",
                "crates/m/target/w/src/lib.rs",
                "crates/render/src/target/mod.rs",
                "crates/target/src/lib.rs",
                "vendor/sub/.git/x.rs",
            ]
        );
        // Only `target` itself is skipped, in that case: cargo never builds
        // into `Target`. (A tree of its own, since on macOS and Windows the
        // two names are one directory.)
        let tree = TempTree::new("walk-skip-case");
        tree.write("Cargo.toml", "[workspace]\nresolver = \"3\"\n");
        tree.write("Target/x.rs", "");
        assert_eq!(found(&tree), ["Target/x.rs"]);
    }

    fn always(_: &Path, _: &Path) -> bool {
        true
    }

    fn never(_: &Path, _: &Path) -> bool {
        false
    }

    /// A workspace-root question that must not be asked.
    fn not_asked(_: &Path, dir: &Path) -> bool {
        panic!("asked whether {} is a workspace root", dir.display())
    }

    #[test]
    fn a_linked_manifest_is_not_resolved_and_the_target_beside_it_is_searched() {
        let tree = TempTree::new("walk-linked-manifest");
        let outside = TempTree::new("walk-linked-manifest-outside");
        outside.write("Cargo.toml", "[workspace]\n");
        tree.write("sub/target/debug/out.rs", "");
        let target = outside.path().join("Cargo.toml");
        if let Err(reason) = symlink(&tree, "sub/Cargo.toml", &target) {
            let _ = writeln!(
                io::stderr(),
                "SKIPPED: a_linked_manifest_is_not_resolved_and_the_target_beside_it_is_searched: \
                 cannot make a link here: {reason}"
            );
            return;
        }
        // Followed, the link would be a manifest, and cargo would be asked
        // about the directory: `not_asked` panics if it is.
        let skip = Skip {
            build_dir: None,
            worktrees: None,
            is_workspace_root: not_asked,
            every_file: false,
        };
        let mut found = Tree::default();
        collect(tree.path(), tree.path(), true, is_rust, &skip, &mut found).unwrap();
        let shown = |paths: &[PathBuf]| -> Vec<String> {
            paths
                .iter()
                .map(|path| relative(tree.path(), path))
                .collect()
        };
        assert_eq!(shown(&found.files), ["sub/target/debug/out.rs"]);
        assert_eq!(shown(&found.links), ["sub/Cargo.toml"]);
    }

    #[test]
    fn a_file_read_by_its_fixed_name_is_refused_through_a_link() {
        let tree = TempTree::new("walk-unlinked");
        let outside = TempTree::new("walk-unlinked-outside");
        tree.write("xtask/list.txt", "real\n");
        outside.write("list.txt", "linked\n");
        outside.write("dir/list.txt", "linked\n");
        assert_eq!(
            read_unlinked(tree.path(), "xtask/list.txt", 64).unwrap(),
            "real\n"
        );
        let file = outside.path().join("list.txt");
        if let Err(reason) = symlink(&tree, "docs/list.txt", &file) {
            let _ = writeln!(
                io::stderr(),
                "SKIPPED: a_file_read_by_its_fixed_name_is_refused_through_a_link: cannot make a \
                 link here: {reason}"
            );
            return;
        }
        symlink(&tree, "linked", &outside.path().join("dir")).expect("a second link");
        for (path, link) in [
            ("docs/list.txt", "docs/list.txt"),
            ("linked/list.txt", "linked"),
        ] {
            let error = read_unlinked(tree.path(), path, 64).expect_err("a link on the way");
            let message = error.to_string();
            assert!(
                message.ends_with(&format!(
                    ": {link} is a symbolic link, which xtask does not follow"
                )),
                "{message}"
            );
            assert!(matches!(error, Error::Read { .. }), "{error:?}");
        }
    }

    /// The files [`collect`] finds under `tree` with `skip`, and the build
    /// directory it noted as searched.
    fn collected(tree: &TempTree, skip: &Skip) -> (Vec<String>, Option<String>) {
        let mut found = Tree::default();
        collect(tree.path(), tree.path(), true, is_rust, skip, &mut found).unwrap();
        found.files.sort();
        let files = found
            .files
            .iter()
            .map(|path| relative(tree.path(), path))
            .collect();
        let noted = found
            .searched_build_dir
            .map(|dir| relative(tree.path(), &dir));
        (files, noted)
    }

    #[test]
    fn the_build_directory_the_environment_names_is_searched_and_noted() {
        // `[env] CARGO_TARGET_DIR = { value = "build", relative = true }` in
        // a committed `.cargo/config.toml` names it for `cargo xtask`: being
        // named must not hide it.
        let tree = TempTree::new("walk-build-dir");
        tree.write("Cargo.toml", "");
        tree.write("build/tmp/fixture/src/lib.rs", "");
        tree.write("crates/a/build/x.rs", "");
        tree.write("target/debug/out.rs", "");
        // Named like it, inside it: always found after it, whatever order
        // the directories are listed in, so a note that went by the name
        // alone would name this one.
        tree.write("build/tmp/build/y.rs", "");
        let skip = Skip {
            build_dir: Some(fs::canonicalize(tree.path().join("build")).unwrap()),
            worktrees: None,
            is_workspace_root: always,
            every_file: false,
        };
        assert_eq!(
            collected(&tree, &skip),
            (
                vec![
                    "build/tmp/build/y.rs".to_string(),
                    "build/tmp/fixture/src/lib.rs".to_string(),
                    "crates/a/build/x.rs".to_string()
                ],
                Some("build".to_string())
            )
        );
        // A workspace root's `target` is skipped whether or not it is named,
        // and so it is not noted.
        let skip = Skip {
            build_dir: Some(fs::canonicalize(tree.path().join("target")).unwrap()),
            ..skip
        };
        assert_eq!(collected(&tree, &skip).1, None);
        // Beside a manifest that is not a workspace root's, it is searched.
        let skip = Skip {
            is_workspace_root: never,
            ..skip
        };
        let (files, noted) = collected(&tree, &skip);
        assert!(
            files.contains(&"target/debug/out.rs".to_string()),
            "{files:?}"
        );
        assert_eq!(noted.as_deref(), Some("target"));
    }

    #[test]
    fn every_file_can_be_collected_whatever_its_name() {
        let tree = TempTree::new("walk-every-file");
        tree.write("a.rs", "");
        tree.write("third_party/libpad.rlib", "");
        tree.write("notes.md", "");
        let scanned = scan_every_file(tree.path()).unwrap();
        let files: Vec<String> = scanned
            .files
            .iter()
            .map(|path| relative(tree.path(), path))
            .collect();
        assert_eq!(files, ["a.rs", "notes.md", "third_party/libpad.rlib"]);
        assert_eq!(found(&tree), ["a.rs"]);
    }

    #[test]
    fn symbolic_links_are_reported_and_not_followed() {
        let tree = TempTree::new("walk-links");
        let outside = TempTree::new("walk-links-outside");
        outside.write("evil.rs", "unsafe { }");
        outside.write("dir/evil.rs", "unsafe { }");
        if let Err(reason) = symlink(&tree, "src/s.rs", &outside.path().join("evil.rs")) {
            let _ = writeln!(
                io::stderr(),
                "SKIPPED: symbolic_links_are_reported_and_not_followed: cannot make a link here: \
                 {reason}"
            );
            return;
        }
        symlink(&tree, "src/m", &outside.path().join("dir")).expect("a second link");
        symlink(&tree, "src/notes.md", &outside.path().join("evil.rs")).expect("a third link");
        symlink(&tree, "src/dangling", &tree.path().join("missing")).expect("a fourth link");
        tree.write("src/lib.rs", "");
        let scanned = scan(tree.path(), is_rust).unwrap();
        let shown = |paths: &[PathBuf]| -> Vec<String> {
            paths
                .iter()
                .map(|path| relative(tree.path(), path))
                .collect()
        };
        assert_eq!(shown(&scanned.files), ["src/lib.rs"]);
        // Every link is reported, whatever its name and wherever it leads:
        // telling a link to a directory from one to a file would mean
        // resolving it, which touches its target.
        assert_eq!(
            shown(&scanned.links),
            ["src/dangling", "src/m", "src/notes.md", "src/s.rs"]
        );
        let every = scan_every_file(tree.path()).unwrap();
        assert_eq!(shown(&every.links), shown(&scanned.links));
    }

    #[test]
    fn paths_are_shown_with_their_control_characters_escaped() {
        let root = Path::new("repo");
        // A newline, a carriage return, ESC (an escape sequence), DEL and
        // C1's CSI each become an escape; other text stays.
        let name = "a\n::warning::x\r\u{1b}[2K\u{7f}\u{9b}é.rs";
        assert_eq!(
            relative(root, &root.join("src").join(name)),
            "src/a\\n::warning::x\\r\\u{1b}[2K\\u{7f}\\u{9b}é.rs"
        );
        assert!(
            !relative(root, &root.join(name))
                .chars()
                .any(char::is_control)
        );
        // Outside the root too.
        assert_eq!(
            relative(root, Path::new("elsewhere\ttab")),
            "elsewhere\\ttab"
        );
        // An ordinary path is unchanged, and not copied.
        assert_eq!(relative(root, &root.join("src/lib.rs")), "src/lib.rs");
        assert!(matches!(
            escape_controls("src/lib.rs", |_| false),
            Cow::Borrowed("src/lib.rs")
        ));
        // A backslash is not a control character (nor, on Windows, a part
        // of a name), so it is kept.
        assert!(matches!(
            escape_controls("a\\b", |_| false),
            Cow::Borrowed("a\\b")
        ));
        // The line and paragraph separators and the bidirectional controls
        // too, which a browser or terminal may draw as a break or use to
        // reorder the line; other format characters stay.
        assert_eq!(
            escape_controls(
                "a\u{2028}b\u{2029}c\u{202e}d\u{2066}e\u{200f}f\u{200b}g",
                |_| false
            ),
            "a\\u{2028}b\\u{2029}c\\u{202e}d\\u{2066}e\\u{200f}f\u{200b}g"
        );
        assert_eq!(shown(Path::new("x\ny")), "x\\ny");
        // `keep` spares what it accepts.
        assert_eq!(
            escape_controls("a\nb\tc\u{1b}", |c| c == '\n'),
            "a\nb\\tc\\u{1b}"
        );
    }

    #[test]
    fn a_file_named_with_control_characters_is_found_and_shown_escaped() {
        let tree = TempTree::new("walk-control-names");
        let name = "a\nb\u{1b}c.rs";
        if let Err(error) = fs::write(tree.path().join(name), "") {
            // Windows allows no control character in a file name.
            let _ = writeln!(
                io::stderr(),
                "SKIPPED: a_file_named_with_control_characters_is_found_and_shown_escaped: \
                 cannot name a file so here: {error}"
            );
            return;
        }
        let scanned = scan(tree.path(), is_rust).unwrap();
        let files: Vec<String> = scanned
            .files
            .iter()
            .map(|path| relative(tree.path(), path))
            .collect();
        assert_eq!(files, ["a\\nb\\u{1b}c.rs"]);
    }

    #[test]
    fn a_directory_that_cannot_be_listed_is_an_error() {
        let tree = TempTree::new("walk-missing");
        let missing = tree.path().join("missing");
        let error = scan(&missing, is_rust).expect_err("the directory does not exist");
        assert!(
            matches!(&error, Error::Read { path, .. } if *path == missing),
            "{error:?}"
        );
    }

    #[test]
    fn temporary_trees_are_new_and_distinct() {
        let a = TempTree::new("walk-temp");
        let b = TempTree::new("walk-temp");
        assert_ne!(a.path(), b.path());
        assert!(!fs::symlink_metadata(a.path()).unwrap().is_symlink());
        let path = a.path().to_path_buf();
        drop(a);
        assert!(!path.exists());
    }

    #[test]
    fn relative_paths_use_forward_slashes() {
        let root = Path::new("repo");
        assert_eq!(
            relative(
                root,
                &root.join("crates").join("sys").join("src").join("lib.rs")
            ),
            "crates/sys/src/lib.rs"
        );
        // A path outside the root is shown whole, as the OS writes it,
        // with no doubled separator after its root.
        assert_eq!(
            relative(root, Path::new("elsewhere/x.rs")),
            "elsewhere/x.rs"
        );
        let outside = env::temp_dir().join("elsewhere").join("x.rs");
        assert_eq!(
            relative(Path::new("repo"), &outside),
            outside.display().to_string()
        );
        assert!(!relative(root, &outside).contains("//"));
    }

    #[test]
    fn the_build_directory_is_made_absolute_as_cargo_resolves_it() {
        let cwd = env::temp_dir().join("invocation");
        let var = |value: &str| Some(OsString::from(value));
        // Relative to the directory cargo (and so `xtask`) started in.
        assert_eq!(
            absolute_target_dir(var("reltarget"), None, &cwd),
            Some(cwd.join("reltarget"))
        );
        let elsewhere = env::temp_dir().join("t");
        assert_eq!(
            absolute_target_dir(var(&elsewhere.to_string_lossy()), None, &cwd),
            Some(elsewhere.clone())
        );
        // `CARGO_TARGET_DIR` wins over `CARGO_BUILD_TARGET_DIR`, and an
        // empty value counts as unset.
        assert_eq!(
            absolute_target_dir(var("a"), var("b"), &cwd),
            Some(cwd.join("a"))
        );
        assert_eq!(
            absolute_target_dir(var(""), var("b"), &cwd),
            Some(cwd.join("b"))
        );
        assert_eq!(absolute_target_dir(var(""), var(""), &cwd), None);
        assert_eq!(absolute_target_dir(None, None, &cwd), None);
    }

    /// Registers `<tree>/<at>` as the linked worktree `name` of the
    /// repository at `<tree>`, as `git worktree add` does: a `.git` file in
    /// the worktree naming `.git/worktrees/<name>`, and that entry's
    /// `gitdir` file naming the worktree's `.git` back. `relative` writes
    /// both paths relative, as `worktree.useRelativePaths` does.
    fn add_worktree(tree: &TempTree, at: &str, name: &str, relative: bool) {
        let root = fs::canonicalize(tree.path()).unwrap();
        let depth = at.split('/').count();
        let up = "../".repeat(depth);
        let (forward, back) = if relative {
            (
                format!("{up}.git/worktrees/{name}"),
                format!("../../../{at}/.git"),
            )
        } else {
            (
                root.join(".git/worktrees").join(name).display().to_string(),
                root.join(at).join(".git").display().to_string(),
            )
        };
        tree.write(&format!("{at}/.git"), format!("gitdir: {forward}\n"));
        tree.write(
            &format!(".git/worktrees/{name}/gitdir"),
            format!("{back}\n"),
        );
        tree.write(
            &format!(".git/worktrees/{name}/HEAD"),
            "ref: refs/heads/x\n",
        );
    }

    #[test]
    fn a_linked_worktree_of_this_repository_is_skipped_and_listed() {
        let tree = TempTree::new("walk-worktree");
        tree.write("crates/sys/src/lib.rs", "");
        // Claude Code's place for worktrees, and one made by hand.
        add_worktree(&tree, ".claude/worktrees/x", "x", false);
        tree.write(".claude/worktrees/x/crates/sys/src/lib.rs", "");
        add_worktree(&tree, "wt/y", "y", true);
        tree.write("wt/y/crates/sys/src/lib.rs", "");
        let scanned = scan(tree.path(), is_rust).unwrap();
        let shown = |paths: &[PathBuf]| -> Vec<String> {
            paths
                .iter()
                .map(|path| relative(tree.path(), path))
                .collect()
        };
        assert_eq!(shown(&scanned.files), ["crates/sys/src/lib.rs"]);
        assert_eq!(shown(&scanned.worktrees), [".claude/worktrees/x", "wt/y"]);
    }

    #[test]
    fn anything_short_of_a_registered_worktree_is_searched() {
        let tree = TempTree::new("walk-not-worktree");
        let other = TempTree::new("walk-not-worktree-other");
        add_worktree(&tree, "real", "real", false);
        tree.write("real/a.rs", "");
        // A `.git` file whose entry does not name it back.
        tree.write("one/.git", "gitdir: ../.git/worktrees/real\n");
        tree.write("one/a.rs", "");
        // An entry that does not exist.
        tree.write("two/.git", "gitdir: ../.git/worktrees/gone\n");
        tree.write("two/a.rs", "");
        // A submodule: its git directory is under `.git/modules`.
        tree.write("three/.git", "gitdir: ../.git/modules/three\n");
        tree.write(".git/modules/three/gitdir", "../../../three/.git\n");
        tree.write("three/a.rs", "");
        // A worktree of another repository.
        add_worktree(&other, "four", "four", false);
        let foreign = fs::canonicalize(other.path().join(".git/worktrees/four")).unwrap();
        tree.write("four/.git", format!("gitdir: {}\n", foreign.display()));
        tree.write("four/a.rs", "");
        // A nested repository with a `.git` directory, and a `.git` file
        // with no `gitdir:` line.
        tree.write("five/.git/HEAD", "ref: refs/heads/main\n");
        tree.write("five/a.rs", "");
        tree.write("six/.git", "not a git file\n");
        tree.write("six/a.rs", "");
        let scanned = scan(tree.path(), is_rust).unwrap();
        let found: Vec<String> = scanned
            .files
            .iter()
            .map(|path| relative(tree.path(), path))
            .collect();
        assert_eq!(
            found,
            [
                "five/a.rs",
                "four/a.rs",
                "one/a.rs",
                "six/a.rs",
                "three/a.rs",
                "two/a.rs"
            ]
        );
        assert_eq!(scanned.worktrees.len(), 1);
        // Without a `.git` directory at the root, nothing is a worktree of
        // it: the worktree's own `.git` file is not enough.
        let bare = TempTree::new("walk-not-worktree-bare");
        bare.write("wt/.git", format!("gitdir: {}\n", foreign.display()));
        bare.write("wt/a.rs", "");
        assert_eq!(scan(bare.path(), is_rust).unwrap().files.len(), 1);
    }

    #[test]
    fn a_dot_git_link_to_a_worktree_s_git_file_does_not_make_a_worktree() {
        // `crates/text/src/evil/.git` is a link to the `.git` file of the
        // registered worktree `real`: resolving both sides would see the
        // record match, so the link must not be followed.
        let tree = TempTree::new("walk-git-link");
        add_worktree(&tree, "real", "real", false);
        tree.write("real/a.rs", "");
        tree.write("crates/text/src/evil/mod.rs", "");
        if let Err(reason) = symlink(
            &tree,
            "crates/text/src/evil/.git",
            &tree.path().join("real/.git"),
        ) {
            let _ = writeln!(
                io::stderr(),
                "SKIPPED: a_dot_git_link_to_a_worktree_s_git_file_does_not_make_a_worktree: \
                 cannot make a link here: {reason}"
            );
            return;
        }
        let scanned = scan(tree.path(), is_rust).unwrap();
        let shown = |paths: &[PathBuf]| -> Vec<String> {
            paths
                .iter()
                .map(|path| relative(tree.path(), path))
                .collect()
        };
        assert_eq!(shown(&scanned.files), ["crates/text/src/evil/mod.rs"]);
        assert_eq!(shown(&scanned.worktrees), ["real"]);
    }

    #[test]
    fn a_root_git_link_finds_no_worktrees() {
        // The root's `.git` is a link to the real git directory, or to a
        // `gitdir:` file naming it, whose `worktrees/x` records `wt` both
        // ways. A link is not followed, so `wt` is searched; and like every
        // link, it is reported.
        for to_directory in [true, false] {
            let tree = TempTree::new("walk-root-git-link");
            let git = TempTree::new("walk-root-git-link-gitdir");
            tree.write("wt/a.rs", "");
            let wt = fs::canonicalize(tree.path().join("wt")).unwrap();
            let gitdir = fs::canonicalize(git.path()).unwrap().join("gitdir");
            git.write(
                "gitdir/worktrees/x/gitdir",
                format!("{}\n", wt.join(".git").display()),
            );
            git.write("gitdir/HEAD", "ref: refs/heads/main\n");
            git.write("dotgit", format!("gitdir: {}\n", gitdir.display()));
            tree.write(
                "wt/.git",
                format!("gitdir: {}\n", gitdir.join("worktrees/x").display()),
            );
            let target = if to_directory {
                gitdir.clone()
            } else {
                git.path().join("dotgit")
            };
            if let Err(reason) = symlink(&tree, ".git", &target) {
                let _ = writeln!(
                    io::stderr(),
                    "SKIPPED: a_root_git_link_finds_no_worktrees: cannot make a link here: \
                     {reason}"
                );
                return;
            }
            let scanned = scan(tree.path(), is_rust).unwrap();
            assert_eq!(
                scanned.files,
                [tree.path().join("wt/a.rs")],
                "{to_directory}"
            );
            assert!(scanned.worktrees.is_empty(), "{to_directory}");
            assert_eq!(scanned.links, [tree.path().join(".git")], "{to_directory}");
        }
    }

    #[test]
    fn a_worktree_s_own_nested_worktrees_are_found_through_its_common_git_directory() {
        // `xtask` running in the worktree `.claude/worktrees/x` of the
        // repository at `tree`, which holds another worktree of the same
        // repository.
        let tree = TempTree::new("walk-worktree-common");
        add_worktree(&tree, ".claude/worktrees/x", "x", false);
        tree.write(".git/worktrees/x/commondir", "../..\n");
        add_worktree(&tree, ".claude/worktrees/x/.claude/worktrees/y", "y", false);
        tree.write(".claude/worktrees/x/.claude/worktrees/y/a.rs", "");
        tree.write(".claude/worktrees/x/b.rs", "");
        let root = tree.path().join(".claude/worktrees/x");
        let scanned = scan(&root, is_rust).unwrap();
        assert_eq!(scanned.files, [root.join("b.rs")]);
        assert_eq!(scanned.worktrees, [root.join(".claude/worktrees/y")]);
    }

    #[test]
    fn a_commondir_that_is_a_link_is_not_followed() {
        // As in the test above, but the worktree's `commondir` is a link to
        // a file that names the common git directory: like every git file
        // the walk reads, it is not followed, so the common directory is
        // not found and the nested worktree is searched.
        let tree = TempTree::new("walk-commondir-link");
        add_worktree(&tree, ".claude/worktrees/x", "x", false);
        tree.write("elsewhere/commondir", "../..\n");
        add_worktree(&tree, ".claude/worktrees/x/.claude/worktrees/y", "y", false);
        tree.write(".claude/worktrees/x/.claude/worktrees/y/a.rs", "");
        tree.write(".claude/worktrees/x/b.rs", "");
        if let Err(reason) = symlink(
            &tree,
            ".git/worktrees/x/commondir",
            &tree.path().join("elsewhere/commondir"),
        ) {
            let _ = writeln!(
                io::stderr(),
                "SKIPPED: a_commondir_that_is_a_link_is_not_followed: cannot make a link here: \
                 {reason}"
            );
            return;
        }
        let root = tree.path().join(".claude/worktrees/x");
        let scanned = scan(&root, is_rust).unwrap();
        assert_eq!(
            scanned.files,
            [root.join(".claude/worktrees/y/a.rs"), root.join("b.rs")]
        );
        assert!(scanned.worktrees.is_empty(), "{:?}", scanned.worktrees);
    }

    #[test]
    fn reading_is_bounded_and_needs_utf8() {
        let tree = TempTree::new("walk-read");
        tree.write("small.rs", "fn f() {}\n");
        tree.write("big.rs", "x".repeat(65));
        tree.write("binary.rs", [0xff, 0xfe, 0x00]);
        let path = |name: &str| tree.path().join(name);
        assert_eq!(
            read_bounded(&path("small.rs"), 64).expect("small enough"),
            "fn f() {}\n"
        );
        assert!(read_bounded(&path("big.rs"), 65).is_ok());
        assert!(matches!(
            read_bounded(&path("big.rs"), 64),
            Err(ReadError::TooLarge { limit: 64 })
        ));
        assert!(matches!(
            read_bounded(&path("binary.rs"), 64),
            Err(ReadError::NotUtf8)
        ));
        assert!(matches!(
            read_bounded(&path("missing.rs"), 64),
            Err(ReadError::Io(_))
        ));
        let message = read_required(&path("big.rs"), 64)
            .expect_err("too large")
            .to_string();
        assert!(
            message.ends_with("the file is larger than the 64-byte limit"),
            "{message}"
        );
    }
}
