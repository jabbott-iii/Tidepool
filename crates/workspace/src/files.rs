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

//! The file tree model: a [`Folder`]'s folders and files, read one folder
//! at a time when the explorer opens it, on workers (M4.11).
//!
//! New in Tidepool.
//!
//! # Reading a folder at a time
//!
//! A project can hold millions of files, so the tree reads only what is
//! shown. [`FileTree::expand`] marks a folder open and, the first time,
//! returns a [`ReadDirJob`] that lists it. A worker calls
//! [`ReadDirJob::run`], and [`FileTree::finish`] keeps the [`Listing`].
//! [`FileTree::refresh`] reads a folder again. A result that a later read
//! of the same folder replaced is stale and dropped, so the tree always
//! shows the latest read.
//!
//! # What a listing holds
//!
//! A [`Listing`] holds at most [`MAX_ENTRIES`] entries of the folder, and
//! says whether there were more ([`Listing::is_truncated`]); which ones are
//! kept then is the order the system lists them in. Each [`Entry`] has its
//! name, its kind, read without following a symbolic link
//! ([`EntryKind::Link`]), and whether the folder's ignore files ignore it
//! ([`Entry::is_ignored`], by the rules of [`crate::folder`]). Ignored
//! entries are kept, marked, so that the explorer can show them, as M4.3
//! decides. Folders come first, then everything else, each sorted by
//! name, ignoring ASCII case first and then by bytes.
//!
//! The tree reads a folder only when it is reached from the root through
//! folders, never through a symbolic link, as Git reads the ignore files
//! ([`WorkspaceError::NotBrowsable`]). A link shows as an entry, and is
//! not opened. Each name on the way is looked at before the folder is
//! listed and again after, since `std` lists a folder by its path and
//! cannot refuse a link there; a process that can write the project and
//! swaps a folder on the way for a link and back between the two looks
//! can still show the names of another folder, though no file is read
//! (M4.11's security review).
//!
//! [`FileTree::rows`] flattens what is open into the rows an explorer
//! shows, depth first.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::WorkspaceError;
use crate::folder::{self, Folder, Ignore, IgnoreLimits};

/// The most entries one folder's [`Listing`] keeps: 10,000.
pub const MAX_ENTRIES: usize = 10_000;

/// What an [`Entry`] is, read without following a symbolic link.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EntryKind {
    /// A regular file.
    File,
    /// A folder, which [`FileTree::expand`] can open.
    Folder,
    /// A symbolic link, to a file or a folder: the tree does not follow it.
    Link,
    /// A device, a pipe, a socket, or anything else.
    Other,
}

/// One name in a folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    name: OsString,
    kind: EntryKind,
    ignored: bool,
}

impl Entry {
    /// The name, as the system gives it, which need not be UTF-8.
    pub fn name(&self) -> &OsStr {
        &self.name
    }

    /// What it is.
    pub fn kind(&self) -> EntryKind {
        self.kind
    }

    /// Whether the folder's ignore files ignore it.
    pub fn is_ignored(&self) -> bool {
        self.ignored
    }
}

/// The entries of one folder, sorted (the [module
/// documentation](self#what-a-listing-holds)).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listing {
    entries: Vec<Entry>,
    truncated: bool,
}

impl Listing {
    /// The entries: folders first, then the rest, each by name.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Whether the folder had more than [`MAX_ENTRIES`] entries, of which
    /// the listing kept [`MAX_ENTRIES`].
    pub fn is_truncated(&self) -> bool {
        self.truncated
    }
}

/// One row an explorer shows ([`FileTree::rows`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// The entry's path, relative to the root.
    pub path: PathBuf,
    /// How many folders it is inside, below the root: 0 for the root's
    /// own entries.
    pub depth: usize,
    /// What it is.
    pub kind: EntryKind,
    /// Whether the ignore files ignore it.
    pub ignored: bool,
    /// Whether it is a folder the tree shows open.
    pub expanded: bool,
    /// Whether it is an open folder whose listing is being read.
    pub loading: bool,
}

/// What the tree knows of one folder.
#[derive(Debug, Default)]
struct Dir {
    expanded: bool,
    /// The read whose result the tree waits for.
    pending: Option<u64>,
    listing: Option<Listing>,
    error: Option<WorkspaceError>,
    problems: Vec<WorkspaceError>,
}

/// A folder's tree of files, read a folder at a time (the [module
/// documentation](self)). Paths in it are relative to the root, which is
/// `""`.
#[derive(Debug)]
pub struct FileTree {
    folder: Folder,
    limits: IgnoreLimits,
    dirs: BTreeMap<PathBuf, Dir>,
    next_request: u64,
}

impl FileTree {
    /// The tree of `folder`, with nothing read yet, reading ignore files
    /// within [`IgnoreLimits::DEFAULT`].
    pub fn new(folder: Folder) -> Self {
        Self::with_limits(folder, IgnoreLimits::DEFAULT)
    }

    /// The tree of `folder`, reading ignore files within `limits`.
    pub fn with_limits(folder: Folder, limits: IgnoreLimits) -> Self {
        Self {
            folder,
            limits,
            dirs: BTreeMap::new(),
            next_request: 0,
        }
    }

    /// The folder.
    pub fn folder(&self) -> &Folder {
        &self.folder
    }

    /// Opens the folder `dir` (relative to the root; `""` is the root) and
    /// returns the job that reads it, the first time and after a read that
    /// failed; `None` when it was read, or is being read.
    ///
    /// # Errors
    ///
    /// [`WorkspaceError::NotBrowsable`] when `dir` is not a relative path
    /// of plain names: one with `..`, `.` at its start, or a root.
    pub fn expand(&mut self, dir: &Path) -> Result<Option<ReadDirJob>, WorkspaceError> {
        let dir = plain(dir)?;
        let node = self.dirs.entry(dir.clone()).or_default();
        node.expanded = true;
        if node.pending.is_some() || node.listing.is_some() {
            return Ok(None);
        }
        Ok(Some(self.job(dir)))
    }

    /// Reads the folder `dir` again, whether or not it was read, and opens
    /// it. A read already running for it becomes stale.
    ///
    /// # Errors
    ///
    /// As for [`FileTree::expand`].
    pub fn refresh(&mut self, dir: &Path) -> Result<ReadDirJob, WorkspaceError> {
        let dir = plain(dir)?;
        self.dirs.entry(dir.clone()).or_default().expanded = true;
        Ok(self.job(dir))
    }

    /// Shows the folder `dir` closed. Its listing is kept for when it opens
    /// again.
    pub fn collapse(&mut self, dir: &Path) {
        if let Some(node) = self.dirs.get_mut(dir) {
            node.expanded = false;
        }
    }

    /// Keeps what a read found, and returns whether it was used: `false`
    /// for a stale result, which a later read of the folder replaced.
    pub fn finish(&mut self, done: ReadDirDone) -> bool {
        let Some(node) = self.dirs.get_mut(&done.dir) else {
            return false;
        };
        if node.pending != Some(done.request) {
            return false;
        }
        node.pending = None;
        node.problems = done.problems;
        match done.result {
            Ok(listing) => {
                node.listing = Some(listing);
                node.error = None;
            }
            Err(error) => {
                node.listing = None;
                node.error = Some(error);
            }
        }
        true
    }

    /// The listing of the folder `dir`, if it was read.
    pub fn listing(&self, dir: &Path) -> Option<&Listing> {
        self.dirs.get(dir)?.listing.as_ref()
    }

    /// Why the last read of `dir` failed, if it did.
    pub fn error(&self, dir: &Path) -> Option<&WorkspaceError> {
        self.dirs.get(dir)?.error.as_ref()
    }

    /// The ignore files that could not be used when `dir` was last read:
    /// each left no rules ([`Folder::read_ignore_files`]).
    pub fn problems(&self, dir: &Path) -> &[WorkspaceError] {
        self.dirs.get(dir).map_or(&[], |node| &node.problems)
    }

    /// Whether the tree shows `dir` open.
    pub fn is_expanded(&self, dir: &Path) -> bool {
        self.dirs.get(dir).is_some_and(|node| node.expanded)
    }

    /// Whether a read of `dir` is running.
    pub fn is_loading(&self, dir: &Path) -> bool {
        self.dirs
            .get(dir)
            .is_some_and(|node| node.pending.is_some())
    }

    /// The rows an explorer shows: the root's entries, and inside each open
    /// folder its own, depth first, in each listing's order. Nothing shows
    /// until the root is open and read.
    pub fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        if self.is_expanded(Path::new("")) {
            self.push_rows(Path::new(""), 0, &mut rows);
        }
        rows
    }

    fn push_rows(&self, dir: &Path, depth: usize, rows: &mut Vec<Row>) {
        let Some(listing) = self.listing(dir) else {
            return;
        };
        for entry in listing.entries() {
            let path = dir.join(entry.name());
            let folder = entry.kind() == EntryKind::Folder;
            let expanded = folder && self.is_expanded(&path);
            rows.push(Row {
                path: path.clone(),
                depth,
                kind: entry.kind(),
                ignored: entry.is_ignored(),
                expanded,
                loading: expanded && self.is_loading(&path),
            });
            if expanded {
                self.push_rows(&path, depth.saturating_add(1), rows);
            }
        }
    }

    /// A read of `dir`, which the tree now waits for.
    fn job(&mut self, dir: PathBuf) -> ReadDirJob {
        let request = self.next_request;
        self.next_request = self.next_request.saturating_add(1);
        self.dirs.entry(dir.clone()).or_default().pending = Some(request);
        ReadDirJob {
            folder: self.folder.clone(),
            dir,
            request,
            limits: self.limits,
        }
    }
}

/// `dir` as the tree keys it, if it is a relative path of plain names.
fn plain(dir: &Path) -> Result<PathBuf, WorkspaceError> {
    if dir
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
    {
        Ok(dir.components().collect())
    } else {
        Err(WorkspaceError::NotBrowsable {
            path: dir.to_path_buf(),
        })
    }
}

/// A read of one folder, for a worker to run ([`FileTree::expand`]).
#[derive(Clone, Debug)]
pub struct ReadDirJob {
    folder: Folder,
    dir: PathBuf,
    request: u64,
    limits: IgnoreLimits,
}

impl ReadDirJob {
    /// The folder read, relative to the root.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Reads the ignore files that apply in the folder, then lists it. It
    /// blocks on the file system, so run it on a worker, never on the main
    /// thread.
    pub fn run(self) -> ReadDirDone {
        let (result, problems) = list(&self.folder, &self.dir, &self.limits, MAX_ENTRIES);
        ReadDirDone {
            dir: self.dir,
            request: self.request,
            result,
            problems,
        }
    }
}

/// What a [`ReadDirJob`] found, for [`FileTree::finish`].
#[derive(Debug)]
pub struct ReadDirDone {
    dir: PathBuf,
    request: u64,
    result: Result<Listing, WorkspaceError>,
    problems: Vec<WorkspaceError>,
}

impl ReadDirDone {
    /// The folder read, relative to the root.
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

/// Lists the folder `dir` of `folder`, keeping at most `max` entries, with
/// the errors of the ignore files that could not be used.
fn list(
    folder: &Folder,
    dir: &Path,
    limits: &IgnoreLimits,
    max: usize,
) -> (Result<Listing, WorkspaceError>, Vec<WorkspaceError>) {
    let Some(base) = folder::real_dir(folder.root(), dir) else {
        let error = WorkspaceError::NotBrowsable {
            path: dir.to_path_buf(),
        };
        return (Err(error), Vec::new());
    };
    let (ignore, problems) = ignore_in(folder, dir, limits);
    let listing = read_entries(&base, dir, &ignore, max);
    // Looked at again: a folder on the way that became a link while it was
    // listed may have shown another folder's names.
    if folder::real_dir(folder.root(), dir).is_none() {
        let error = WorkspaceError::NotBrowsable {
            path: dir.to_path_buf(),
        };
        return (Err(error), problems);
    }
    (listing, problems)
}

/// The ignore files that apply to the entries of `dir`: the root's, and
/// those of every folder down to `dir` itself.
fn ignore_in(folder: &Folder, dir: &Path, limits: &IgnoreLimits) -> (Ignore, Vec<WorkspaceError>) {
    let (mut ignore, mut problems) = folder.ignore_for(dir, limits);
    if dir.components().next().is_some() {
        problems.extend(folder.read_ignore_files(dir, &mut ignore, limits));
    }
    (ignore, problems)
}

/// The sorted entries of the folder at `base`, which is `dir` under the
/// root, at most `max` of them.
fn read_entries(
    base: &Path,
    dir: &Path,
    ignore: &Ignore,
    max: usize,
) -> Result<Listing, WorkspaceError> {
    let read = |source| WorkspaceError::Read {
        path: base.to_path_buf(),
        source,
    };
    let mut entries = Vec::new();
    let mut truncated = false;
    for entry in fs::read_dir(base).map_err(read)? {
        let entry = entry.map_err(read)?;
        if entries.len() == max {
            truncated = true;
            break;
        }
        let file_type = entry.file_type().map_err(read)?;
        let kind = if file_type.is_symlink() {
            EntryKind::Link
        } else if file_type.is_dir() {
            EntryKind::Folder
        } else if file_type.is_file() {
            EntryKind::File
        } else {
            EntryKind::Other
        };
        let name = entry.file_name();
        let ignored = ignore.is_ignored(&dir.join(&name), kind == EntryKind::Folder);
        entries.push(Entry {
            name,
            kind,
            ignored,
        });
    }
    entries.sort_by(order);
    Ok(Listing { entries, truncated })
}

/// Folders first, then by name ignoring ASCII case, then by bytes.
fn order(a: &Entry, b: &Entry) -> Ordering {
    let folder = |entry: &Entry| entry.kind != EntryKind::Folder;
    let (left, right) = (a.name.as_encoded_bytes(), b.name.as_encoded_bytes());
    folder(a)
        .cmp(&folder(b))
        .then_with(|| {
            left.iter()
                .map(u8::to_ascii_lowercase)
                .cmp(right.iter().map(u8::to_ascii_lowercase))
        })
        .then_with(|| left.cmp(right))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, kind: EntryKind) -> Entry {
        Entry {
            name: name.into(),
            kind,
            ignored: false,
        }
    }

    #[test]
    fn folders_come_first_then_names_without_case() {
        let mut entries = [
            entry("b.rs", EntryKind::File),
            entry("Zed", EntryKind::Folder),
            entry("README.md", EntryKind::File),
            entry("a", EntryKind::Folder),
            entry("B.rs", EntryKind::File),
            entry("link", EntryKind::Link),
        ];
        entries.sort_by(order);
        let names: Vec<_> = entries
            .iter()
            .map(|entry| entry.name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["a", "Zed", "B.rs", "b.rs", "link", "README.md"]);
    }

    #[test]
    fn only_plain_relative_paths_are_folders_of_the_tree() {
        assert_eq!(plain(Path::new("")).unwrap(), PathBuf::new());
        assert_eq!(plain(Path::new("a/b/")).unwrap(), PathBuf::from("a/b"));
        for bad in ["../a", "a/../b", "/a", "./a"] {
            assert!(
                matches!(
                    plain(Path::new(bad)),
                    Err(WorkspaceError::NotBrowsable { .. })
                ),
                "{bad}"
            );
        }
    }
}
