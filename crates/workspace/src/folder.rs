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

//! Workspace folders: a [`Folder`] is one root, its `.tidepool/` folder,
//! and the ignore files that say which of its paths the explorer, search
//! and the watcher leave out.
//!
//! New in Tidepool (M4.7, ADR-0067).
//!
//! # Folders
//!
//! [`Folder::open`] takes a directory by its canonical path
//! ([`tidepool_core::path::canonicalize`]), so that the folder has one
//! spelling for the trust store to look up and for paths to be compared
//! with ([`Folder::relative`]). Tidepool's own files for it live in
//! [`CONFIG_DIR`], `.tidepool/` under the root: its settings in
//! `.tidepool/settings.toml` ([`Folder::settings_file`]). A workspace of
//! several roots holds one `Folder` for each.
//!
//! # Ignore files
//!
//! An ignore file is a `.gitignore` or a `.tidepoolignore`, in any folder
//! of the workspace, or the root's `.git/info/exclude`. All three are read
//! as gitignore(5) says Git reads them ([`IgnoreFile::parse`]):
//!
//! - lines end at a line feed, with a carriage return before it dropped;
//!   a UTF-8 byte-order mark at the start is skipped; a line is cut at a
//!   NUL byte, as Git's strings end there;
//! - an empty line, or one that starts with `#`, is no pattern; trailing
//!   spaces are dropped unless the last is escaped (`\ `);
//! - `!` first makes a pattern include again what an earlier one ignored;
//!   `\!` and `\#` start a pattern with those characters;
//! - a final `/` makes a pattern match folders only, and is dropped;
//! - a pattern with a `/` left in it is anchored: it matches the path from
//!   the ignore file's folder (a leading `/` dropped). One without matches
//!   the last name of a path at any depth below that folder;
//! - the rest is a glob: `*`, `?`, `[...]` and `**` as Git's `wildmatch`
//!   reads them, on bytes, with case kept. A glob with an unclosed `[`, an
//!   unknown `[:class:]` or a final `\` never matches, as in Git.
//!
//! An ignore file comes from the project, so it is untrusted input,
//! whatever the folder's trust. [`IgnoreLimits`] bounds each file and all
//! of them together, since a lookup costs about the path's length times
//! the bytes of every pattern on its way: a project could otherwise give
//! each folder the same large file. [`Folder::read_ignore_files`] reads
//! only a regular file that is not a symbolic link, in a folder reached
//! from the root without one, as Git does since 2.32. Matching is linear
//! in the path's length times the pattern's, with no backtracking.
//!
//! # Which rule wins
//!
//! [`Ignore`] holds the ignore files read so far, by the folder each is
//! in, and answers [`Ignore::is_ignored`] for a path inside the root:
//!
//! 1. A name `.git`, file or folder, is always ignored, and so is
//!    everything inside it.
//! 2. A path inside an ignored folder is ignored: nothing can include it
//!    again, as in Git.
//! 3. Otherwise the `.tidepoolignore` files decide, the deepest folder's
//!    first and the last matching pattern of a file; then the `.gitignore`
//!    files, the same way; then the root's `.git/info/exclude`. So a
//!    `.tidepoolignore` can include again what a `.gitignore` ignores.
//! 4. A path no pattern matches is not ignored.
//!
//! Only files in the root and below it are read: not the `.gitignore` of a
//! folder above it, nor Git's global excludes file, which needs Git's
//! configuration (M8). Paths are compared with their case kept, as Git
//! does with `core.ignoreCase` off, its default on Linux.

mod glob;

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fmt;
use std::path::{Component, Path, PathBuf};

use tidepool_core::path;

use crate::WorkspaceError;
use crate::read::{self, Links};
use glob::{Glob, Scratch};

/// The folder under a root that holds Tidepool's own files for it.
pub const CONFIG_DIR: &str = ".tidepool";

/// The name of a workspace's settings file in [`CONFIG_DIR`].
pub const SETTINGS_FILE: &str = "settings.toml";

/// The name of Tidepool's ignore files.
pub const TIDEPOOL_IGNORE: &str = ".tidepoolignore";

/// The name of Git's ignore files.
pub const GIT_IGNORE: &str = ".gitignore";

/// The name that is always ignored: Git's folder, or the file that points
/// to it in a worktree or a submodule.
pub const GIT_DIR: &str = ".git";

/// A workspace root: a directory, by its canonical path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    root: PathBuf,
}

impl Folder {
    /// Opens the directory at `path`, by its canonical path. It does not
    /// look at trust, settings or ignore files.
    ///
    /// It blocks on the file system, so call it on a worker.
    ///
    /// # Errors
    ///
    /// [`WorkspaceError::Path`] when `path` cannot be canonicalized: it is
    /// empty, missing, or cannot be reached. [`WorkspaceError::NotAFolder`]
    /// when it is not a directory.
    pub fn open(path: impl AsRef<Path>) -> Result<Folder, WorkspaceError> {
        let root = path::canonicalize(path)?;
        match std::fs::metadata(&root) {
            Ok(metadata) if metadata.is_dir() => Ok(Folder { root }),
            Ok(_) => Err(WorkspaceError::NotAFolder { path: root }),
            Err(source) => Err(WorkspaceError::Read { path: root, source }),
        }
    }

    /// The root's canonical path.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `.tidepool/` under the root ([`CONFIG_DIR`]), which may not exist.
    pub fn config_dir(&self) -> PathBuf {
        self.root.join(CONFIG_DIR)
    }

    /// `.tidepool/settings.toml` under the root, which may not exist.
    pub fn settings_file(&self) -> PathBuf {
        self.config_dir().join(SETTINGS_FILE)
    }

    /// The part of `path` inside the root, or `None` when it is not inside
    /// ([`tidepool_core::path::under`]). `path` must be canonical or
    /// resolved too; `Some("")` is the root itself.
    pub fn relative<'a>(&self, path: &'a Path) -> Option<&'a Path> {
        path::under(path, &self.root)
    }

    /// Reads the ignore files of the folder `dir`, relative to the root
    /// (`""` for the root itself), into `ignore`: its `.tidepoolignore` and
    /// `.gitignore`, and for the root also `.git/info/exclude` when `.git`
    /// is a folder. A missing file is no file. Each file that cannot be
    /// used leaves no rules, and gives its error, with the others still
    /// read: one past [`IgnoreLimits`], or one that would take `ignore`
    /// past [`IgnoreLimits::max_total_bytes`]. A `dir` with a name that is
    /// not a plain one (`..`, a root), or reached through a symbolic link
    /// or something that is not a folder, reads nothing, as Git never goes
    /// through a link.
    ///
    /// It blocks on the file system, so call it on a worker.
    pub fn read_ignore_files(
        &self,
        dir: &Path,
        ignore: &mut Ignore,
        limits: &IgnoreLimits,
    ) -> Vec<WorkspaceError> {
        let mut errors = Vec::new();
        let Some(key) = key_of(dir) else {
            return errors;
        };
        let Some(base) = real_dir(&self.root, dir) else {
            return errors;
        };
        for (name, kind) in [(TIDEPOOL_IGNORE, Kind::Tidepool), (GIT_IGNORE, Kind::Git)] {
            match read_file(&base.join(name), limits, ignore.room(&key, kind, limits)) {
                Ok(Some(file)) => ignore.insert(key.clone(), kind, file),
                Ok(None) => {}
                Err(error) => errors.push(error),
            }
        }
        if key.is_empty() {
            let git = self.root.join(GIT_DIR);
            let is_dir = |path: &Path| {
                std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir())
            };
            if is_dir(&git) && is_dir(&git.join("info")) {
                let room = ignore.room_for_exclude(limits);
                match read_file(&git.join("info").join("exclude"), limits, room) {
                    Ok(Some(file)) => ignore.set_exclude(file),
                    Ok(None) => {}
                    Err(error) => errors.push(error),
                }
            }
        }
        errors
    }

    /// An [`Ignore`] holding the root's ignore files and those of every
    /// folder on the way to `path` (relative to the root), enough to answer
    /// [`Ignore::is_ignored`] for it, and the errors of the files that
    /// could not be used. A walker that reads a tree reads each folder's
    /// files once with [`Folder::read_ignore_files`] instead.
    ///
    /// It blocks on the file system, so call it on a worker.
    pub fn ignore_for(&self, path: &Path, limits: &IgnoreLimits) -> (Ignore, Vec<WorkspaceError>) {
        let mut ignore = Ignore::new();
        let mut errors = self.read_ignore_files(Path::new(""), &mut ignore, limits);
        let mut dir = PathBuf::new();
        let names: Vec<_> = path.components().collect();
        for component in names.iter().take(names.len().saturating_sub(1)) {
            let Component::Normal(name) = component else {
                break;
            };
            dir.push(name);
            errors.extend(self.read_ignore_files(&dir, &mut ignore, limits));
        }
        (ignore, errors)
    }
}

/// `root` joined with `dir`, if every name of `dir` is a folder reached
/// without a symbolic link.
fn real_dir(root: &Path, dir: &Path) -> Option<PathBuf> {
    let mut path = root.to_path_buf();
    for component in dir.components() {
        path.push(component);
        let metadata = std::fs::symlink_metadata(&path).ok()?;
        if !metadata.is_dir() {
            return None;
        }
    }
    Some(path)
}

/// Reads and parses the ignore file at `path`, not following a link, if
/// it fits in `room`, the bytes the [`Ignore`] it goes into has left.
fn read_file(
    path: &Path,
    limits: &IgnoreLimits,
    room: u64,
) -> Result<Option<IgnoreFile>, WorkspaceError> {
    let Some(bytes) = read::bounded(path, limits.max_file_bytes, Links::Refuse)? else {
        return Ok(None);
    };
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > room {
        return Err(WorkspaceError::IgnoreBudget {
            path: path.to_path_buf(),
            limit: limits.max_total_bytes,
        });
    }
    IgnoreFile::parse(&bytes, path, limits).map(Some)
}

/// The bounds on ignore files, which come from the project.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct IgnoreLimits {
    /// The largest file read, in bytes. A larger one is not used.
    pub max_file_bytes: u64,
    /// The most bytes of ignore files one [`Ignore`] holds, all of its
    /// files together. A file that would take it past this is not used,
    /// so a lookup's cost, and the memory the compiled patterns take
    /// (at most about 15 bytes for each byte of a file, for a file of
    /// one-byte sets such as `[a]`), stay bounded.
    pub max_total_bytes: u64,
    /// The most patterns a file may have. A file with more is not used.
    pub max_patterns: usize,
    /// The longest pattern, in bytes of its line after a carriage return
    /// and trailing spaces are dropped. A file with a longer one is not
    /// used.
    pub max_pattern_bytes: usize,
}

impl IgnoreLimits {
    /// 256 KiB a file and 1 MiB in all, far above any real project's;
    /// 10,000 patterns a file, and 4,096 bytes a pattern, Linux's
    /// `PATH_MAX`.
    pub const DEFAULT: Self = Self {
        max_file_bytes: 256 * 1024,
        max_total_bytes: 1024 * 1024,
        max_patterns: 10_000,
        max_pattern_bytes: 4096,
    };
}

impl Default for IgnoreLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// What the patterns say of a path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Match {
    /// No pattern matches it.
    None,
    /// The last pattern that matches it ignores it.
    Ignore,
    /// The last pattern that matches it starts with `!`, and includes it
    /// again.
    Include,
}

/// One pattern of an ignore file.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Pattern {
    /// The glob, or `None` for one that never matches.
    glob: Option<Glob>,
    /// It starts with `!`.
    negated: bool,
    /// It ended in `/`, so it matches folders only.
    dirs_only: bool,
    /// It has no `/`, so it matches a path's last name at any depth.
    name_only: bool,
}

impl Pattern {
    /// Parses one line, cut at a NUL and with its carriage return dropped,
    /// or `None` for a line that is no pattern.
    fn parse(line: &[u8]) -> Option<Pattern> {
        if line.is_empty() || line[0] == b'#' {
            return None;
        }
        let line = trim_trailing_spaces(line);
        let (negated, rest) = match line.split_first() {
            Some((b'!', rest)) => (true, rest),
            _ => (false, line),
        };
        let (dirs_only, rest) = match rest.split_last() {
            Some((b'/', rest)) => (true, rest),
            _ => (false, rest),
        };
        let name_only = !rest.contains(&b'/');
        let rest = match rest.split_first() {
            Some((b'/', anchored)) => anchored,
            _ => rest,
        };
        Some(Pattern {
            glob: Glob::compile(rest),
            negated,
            dirs_only,
            name_only,
        })
    }

    /// Whether the pattern matches `path`, relative to its file's folder,
    /// whose last name is `name`.
    fn matches(&self, path: &[u8], name: &[u8], is_dir: bool, scratch: &mut Scratch) -> bool {
        if self.dirs_only && !is_dir {
            return false;
        }
        let Some(glob) = &self.glob else {
            return false;
        };
        glob.matches(if self.name_only { name } else { path }, scratch)
    }
}

/// Git's `trim_trailing_spaces`: drops the run of spaces at the end of
/// `line`, unless a backslash escapes the first of them.
fn trim_trailing_spaces(line: &[u8]) -> &[u8] {
    let mut last_space = None;
    let mut at = 0;
    while let Some(&byte) = line.get(at) {
        match byte {
            b' ' => {
                last_space.get_or_insert(at);
            }
            b'\\' => {
                at += 1;
                if at >= line.len() {
                    return line;
                }
                last_space = None;
            }
            _ => last_space = None,
        }
        at += 1;
    }
    match last_space {
        Some(end) => &line[..end],
        None => line,
    }
}

/// The patterns of one ignore file, in order.
#[derive(Clone, PartialEq, Eq)]
pub struct IgnoreFile {
    patterns: Vec<Pattern>,
    /// The file's length in bytes, which counts toward an [`Ignore`]'s
    /// [`IgnoreLimits::max_total_bytes`].
    bytes: u64,
}

impl fmt::Debug for IgnoreFile {
    /// Shows how many patterns there are, not what they say.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IgnoreFile")
            .field("patterns", &self.patterns.len())
            .field("bytes", &self.bytes)
            .finish()
    }
}

impl IgnoreFile {
    /// Parses the bytes of an ignore file, as the
    /// [module documentation](self#ignore-files) says. `path` is the file's,
    /// for errors. It never panics, and reads nothing.
    ///
    /// # Errors
    ///
    /// [`WorkspaceError::TooLarge`], [`WorkspaceError::TooManyPatterns`] or
    /// [`WorkspaceError::PatternTooLong`] for a file past one of `limits`.
    pub fn parse(
        bytes: &[u8],
        path: &Path,
        limits: &IgnoreLimits,
    ) -> Result<IgnoreFile, WorkspaceError> {
        let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if size > limits.max_file_bytes {
            return Err(WorkspaceError::TooLarge {
                path: path.to_path_buf(),
                limit: limits.max_file_bytes,
            });
        }
        let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
        let mut patterns = Vec::new();
        for (index, line) in bytes.split(|&byte| byte == b'\n').enumerate() {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            let line = match line.iter().position(|&byte| byte == 0) {
                Some(nul) => &line[..nul],
                None => line,
            };
            if line.is_empty() || line[0] == b'#' {
                continue;
            }
            // Checked before the glob is compiled, which takes up to 12
            // bytes for each byte of it.
            if trim_trailing_spaces(line).len() > limits.max_pattern_bytes {
                return Err(WorkspaceError::PatternTooLong {
                    path: path.to_path_buf(),
                    line: index + 1,
                    limit: limits.max_pattern_bytes,
                });
            }
            let Some(pattern) = Pattern::parse(line) else {
                continue;
            };
            if patterns.len() == limits.max_patterns {
                return Err(WorkspaceError::TooManyPatterns {
                    path: path.to_path_buf(),
                    limit: limits.max_patterns,
                });
            }
            patterns.push(pattern);
        }
        patterns.shrink_to_fit();
        Ok(IgnoreFile {
            patterns,
            bytes: size,
        })
    }

    /// How many patterns the file has.
    pub fn len(&self) -> usize {
        self.patterns.len()
    }

    /// The file's length in bytes.
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Whether the file has no patterns.
    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    /// What this file alone says of `path`, relative to the file's folder,
    /// with `/` between its names (a folder if `is_dir`): the last pattern
    /// that matches it decides. It does not look at the folders above the
    /// path, as [`Ignore::is_ignored`] does.
    pub fn matched(&self, path: &[u8], is_dir: bool) -> Match {
        self.matched_with(path, is_dir, &mut Scratch::default())
    }

    /// [`IgnoreFile::matched`], with room for the match.
    fn matched_with(&self, path: &[u8], is_dir: bool, scratch: &mut Scratch) -> Match {
        let name = match path.iter().rposition(|&byte| byte == b'/') {
            Some(slash) => &path[slash + 1..],
            None => path,
        };
        for pattern in self.patterns.iter().rev() {
            if pattern.matches(path, name, is_dir, scratch) {
                return if pattern.negated {
                    Match::Include
                } else {
                    Match::Ignore
                };
            }
        }
        Match::None
    }
}

/// Which kind of ignore file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Tidepool,
    Git,
}

/// A folder's two ignore files.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Files {
    tidepool: Option<IgnoreFile>,
    git: Option<IgnoreFile>,
}

/// The ignore files of a folder's tree read so far, by the folder each is
/// in, and the rules of the [module documentation](self#which-rule-wins).
///
/// It does no I/O: [`Folder::read_ignore_files`] fills it, folder by folder,
/// within [`IgnoreLimits::max_total_bytes`], and a folder whose files were
/// not read counts as having none. The `set_*` methods add a file without
/// that check, for a caller that bounds what it adds itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ignore {
    /// By the folder's path from the root, its names joined by `/`; the
    /// root's is empty.
    files: BTreeMap<Vec<u8>, Files>,
    /// The root's `.git/info/exclude`.
    exclude: Option<IgnoreFile>,
    /// The bytes of every file held.
    total: u64,
}

impl Ignore {
    /// No ignore files.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the `.tidepoolignore` of the folder `dir` (relative to the
    /// root), replacing any it had. A `dir` with a name that is not a plain
    /// one is ignored.
    pub fn set_tidepool_ignore(&mut self, dir: &Path, file: IgnoreFile) {
        if let Some(key) = key_of(dir) {
            self.insert(key, Kind::Tidepool, file);
        }
    }

    /// Sets the `.gitignore` of the folder `dir`, as
    /// [`Ignore::set_tidepool_ignore`] does.
    pub fn set_git_ignore(&mut self, dir: &Path, file: IgnoreFile) {
        if let Some(key) = key_of(dir) {
            self.insert(key, Kind::Git, file);
        }
    }

    /// Sets the root's `.git/info/exclude`.
    pub fn set_exclude(&mut self, file: IgnoreFile) {
        self.total = self.total.saturating_add(file.bytes);
        if let Some(old) = self.exclude.replace(file) {
            self.total = self.total.saturating_sub(old.bytes);
        }
    }

    /// The bytes of every ignore file held.
    pub fn total_bytes(&self) -> u64 {
        self.total
    }

    /// Stores `file` as the `kind` file of the folder `key`.
    fn insert(&mut self, key: Vec<u8>, kind: Kind, file: IgnoreFile) {
        self.total = self.total.saturating_add(file.bytes);
        let files = self.files.entry(key).or_default();
        let slot = match kind {
            Kind::Tidepool => &mut files.tidepool,
            Kind::Git => &mut files.git,
        };
        if let Some(old) = slot.replace(file) {
            self.total = self.total.saturating_sub(old.bytes);
        }
    }

    /// The bytes a new `kind` file of the folder `key` may have within
    /// `limits`, counting the one it would replace as free.
    fn room(&self, key: &[u8], kind: Kind, limits: &IgnoreLimits) -> u64 {
        let replaced = self.files.get(key).and_then(|files| match kind {
            Kind::Tidepool => files.tidepool.as_ref(),
            Kind::Git => files.git.as_ref(),
        });
        let held = self
            .total
            .saturating_sub(replaced.map_or(0, |file| file.bytes));
        limits.max_total_bytes.saturating_sub(held)
    }

    /// [`Ignore::room`] for the exclude file.
    fn room_for_exclude(&self, limits: &IgnoreLimits) -> u64 {
        let held = self
            .total
            .saturating_sub(self.exclude.as_ref().map_or(0, |file| file.bytes));
        limits.max_total_bytes.saturating_sub(held)
    }

    /// Whether `path`, relative to the root, is ignored (a folder if
    /// `is_dir`), by the rules of the
    /// [module documentation](self#which-rule-wins). The root itself
    /// (`""`), and a path with a name that is not a plain one (`..`, a
    /// root, a prefix), are not.
    pub fn is_ignored(&self, path: &Path, is_dir: bool) -> bool {
        let mut names = Vec::new();
        for component in path.components() {
            match component {
                Component::Normal(name) => names.push(name),
                _ => return false,
            }
        }
        let mut scratch = Scratch::default();
        let mut joined = Vec::new();
        for (at, name) in names.iter().enumerate() {
            if *name == OsStr::new(GIT_DIR) {
                return true;
            }
            if at > 0 {
                joined.push(b'/');
            }
            joined.extend_from_slice(name.as_encoded_bytes());
            let last = at + 1 == names.len();
            // Every name but the last is a folder.
            if self.decide(&joined, !last || is_dir, &mut scratch) == Match::Ignore {
                return true;
            }
        }
        false
    }

    /// What the ignore files say of `path` itself, with `/` between its
    /// names, without looking at the folders above it: the deepest
    /// `.tidepoolignore` that matches, then the deepest `.gitignore`, then
    /// the exclude file.
    fn decide(&self, path: &[u8], is_dir: bool, scratch: &mut Scratch) -> Match {
        for kind in [Kind::Tidepool, Kind::Git] {
            // Each folder above `path`, the deepest first: the part after
            // each `/`, and last the root.
            let mut end = path.len();
            loop {
                let cut = path[..end].iter().rposition(|&byte| byte == b'/');
                let (dir, rest) = match cut {
                    Some(slash) => (&path[..slash], &path[slash + 1..]),
                    None => (&path[..0], path),
                };
                if let Some(files) = self.files.get(dir) {
                    let file = match kind {
                        Kind::Tidepool => files.tidepool.as_ref(),
                        Kind::Git => files.git.as_ref(),
                    };
                    if let Some(file) = file {
                        let matched = file.matched_with(rest, is_dir, scratch);
                        if matched != Match::None {
                            return matched;
                        }
                    }
                }
                match cut {
                    Some(slash) => end = slash,
                    None => break,
                }
            }
        }
        match &self.exclude {
            Some(file) => file.matched_with(path, is_dir, scratch),
            None => Match::None,
        }
    }
}

/// The key of a folder relative to the root: its names joined by `/`, or
/// `None` for a path with a name that is not a plain one.
fn key_of(dir: &Path) -> Option<Vec<u8>> {
    let mut key = Vec::new();
    for component in dir.components() {
        let Component::Normal(name) = component else {
            return None;
        };
        if !key.is_empty() {
            key.push(b'/');
        }
        key.extend_from_slice(name.as_encoded_bytes());
    }
    Some(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(text: &str) -> IgnoreFile {
        IgnoreFile::parse(
            text.as_bytes(),
            Path::new(".gitignore"),
            &IgnoreLimits::DEFAULT,
        )
        .unwrap()
    }

    #[test]
    fn trailing_spaces_go_unless_escaped() {
        assert_eq!(trim_trailing_spaces(b"a  "), b"a");
        assert_eq!(trim_trailing_spaces(br"a\ "), br"a\ ");
        assert_eq!(trim_trailing_spaces(br"a\  "), br"a\ ");
        assert_eq!(trim_trailing_spaces(b"a b"), b"a b");
        assert_eq!(trim_trailing_spaces(b"a\\"), b"a\\");
        assert_eq!(trim_trailing_spaces(b"   "), b"");
    }

    #[test]
    fn lines_that_are_no_pattern() {
        let parsed = file("\n# comment\n\r\n   \nreal\n");
        // The line of spaces is a pattern that matches nothing, as in Git.
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed.matched(b"real", false), Match::Ignore);
        assert_eq!(parsed.matched(b"# comment", false), Match::None);
    }

    #[test]
    fn the_last_matching_pattern_decides() {
        let parsed = file("*.log\n!keep.log\n");
        assert_eq!(parsed.matched(b"a.log", false), Match::Ignore);
        assert_eq!(parsed.matched(b"keep.log", false), Match::Include);
        assert_eq!(parsed.matched(b"sub/keep.log", false), Match::Include);
        assert_eq!(parsed.matched(b"a.txt", false), Match::None);
    }

    #[test]
    fn a_crlf_file_with_a_bom_reads_as_lf() {
        let parsed = file("\u{feff}cr\r\nother\r\n");
        assert_eq!(parsed.matched(b"cr", false), Match::Ignore);
        assert_eq!(parsed.matched(b"other", false), Match::Ignore);
        assert_eq!(parsed.matched(b"cr\r", false), Match::None);
    }

    #[test]
    fn a_nul_cuts_its_line() {
        let parsed =
            IgnoreFile::parse(b"ab\0cd\n", Path::new("x"), &IgnoreLimits::DEFAULT).unwrap();
        assert_eq!(parsed.matched(b"ab", false), Match::Ignore);
    }

    #[test]
    fn keys_are_plain_names_only() {
        assert_eq!(key_of(Path::new("")), Some(Vec::new()));
        assert_eq!(key_of(Path::new("a/b")), Some(b"a/b".to_vec()));
        assert_eq!(key_of(Path::new("a/../b")), None);
        assert_eq!(key_of(Path::new("/a")), None);
    }
}
