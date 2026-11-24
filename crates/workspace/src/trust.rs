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

//! Workspace trust: the folders the user trusts to run their code
//! (ADR-0014).
//!
//! New in Tidepool (M4.7, ADR-0067).
//!
//! A folder opens untrusted. Until the user trusts it, Tidepool runs none
//! of its code: no tasks, no build scripts or proc macros in
//! rust-analyzer, no Gradle, no plugins from it, and its own
//! `.tidepool/settings.toml` is not read ([`settings`](crate::settings)).
//! One prompt (M4.3) trusts it, and the decision persists.
//!
//! # Grants
//!
//! A [`TrustStore`] holds grants: canonical paths of folders the user
//! trusted. A folder is trusted when it is a grant, or is inside one
//! ([`tidepool_core::path::under`]), since trusting a folder already runs
//! the build scripts of the folders in it. So trusting a folder also
//! trusts whatever is cloned or unpacked inside it later, and trusting a
//! home folder trusts everything under it. Only grants are stored: a
//! declined prompt stores nothing, so the folder asks again next time.
//!
//! # The file
//!
//! The grants are in [`FILE_NAME`], `trust.toml` in the user data folder
//! (`platform::paths::user_dirs`, on Linux `$XDG_DATA_HOME/tidepool`):
//!
//! ```toml
//! # The folders the user trusts (ADR-0014). Tidepool writes this file.
//! version = 1
//! folders = ["/home/me/src/tidepool"]
//! ```
//!
//! [`TrustStore::trust`] and [`TrustStore::revoke`] read the file again
//! just before they change it, so a grant another Tidepool made since is
//! kept, then save it with [`tidepool_text::file::save`], through a
//! temporary file and a rename. A file that cannot be used, because it
//! cannot be read, is larger than [`MAX_FILE_BYTES`], is not TOML, has a
//! `version` other than [`VERSION`], or holds anything but at most
//! [`MAX_GRANTS`] absolute paths free of `.` and `..` and spelled as their
//! names join, grants nothing, and
//! is never overwritten: trusting fails with
//! [`WorkspaceError::Unwritable`] until the user fixes or removes it. A
//! folder whose path is not UTF-8 cannot be trusted, since TOML holds only
//! text.
//!
//! Everything that reads or writes the file blocks, so call it on a
//! worker.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use tidepool_codec::Limits;
use tidepool_codec::toml::{Array, Document, Value, ValueKind};
use tidepool_core::path;
use tidepool_text::{Format, Rope};

use crate::WorkspaceError;
use crate::folder::Folder;
use crate::read::{self, Links};

/// The trust file's name in the user data folder.
pub const FILE_NAME: &str = "trust.toml";

/// The trust file's format version, which Tidepool writes and reads.
pub const VERSION: i64 = 1;

/// The largest trust file read: 1 MiB.
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// The most grants the store holds.
pub const MAX_GRANTS: usize = 4096;

/// The comment Tidepool writes at the top of the file.
const HEADER: &str = "# The folders the user trusts (ADR-0014). Tidepool writes this file.\n";

/// The folders the user trusts, and the file that keeps them.
#[derive(Debug)]
pub struct TrustStore {
    file: PathBuf,
    grants: Vec<PathBuf>,
    /// Why the file could not be used when it was last read.
    problem: Option<Arc<WorkspaceError>>,
}

impl TrustStore {
    /// Reads [`FILE_NAME`] in `data_dir`, the user data folder. It never
    /// fails: a missing file grants nothing, and a file that cannot be used
    /// grants nothing and is kept, with its error in
    /// [`TrustStore::problem`].
    ///
    /// It blocks on the file system, so call it on a worker.
    pub fn load(data_dir: &Path) -> TrustStore {
        let file = data_dir.join(FILE_NAME);
        match read_grants(&file) {
            Ok(grants) => TrustStore {
                file,
                grants,
                problem: None,
            },
            Err(problem) => TrustStore {
                file,
                grants: Vec::new(),
                problem: Some(Arc::new(problem)),
            },
        }
    }

    /// The trust file's path.
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// Why the file could not be used when it was last read, if it could
    /// not. The store then grants nothing.
    pub fn problem(&self) -> Option<&WorkspaceError> {
        self.problem.as_deref()
    }

    /// The grants, sorted.
    pub fn grants(&self) -> &[PathBuf] {
        &self.grants
    }

    /// Whether `folder` is trusted: it is a grant or inside one.
    pub fn is_trusted(&self, folder: &Folder) -> bool {
        self.grant_for(folder).is_some()
    }

    /// The grant that trusts `folder`, the one nearest to it, if any:
    /// `folder` itself, or a folder it is inside.
    pub fn grant_for(&self, folder: &Folder) -> Option<&Path> {
        self.grants
            .iter()
            .filter(|grant| path::under(folder.root(), grant).is_some())
            .max_by_key(|grant| grant.components().count())
            .map(PathBuf::as_path)
    }

    /// Trusts `folder` and every folder in it, and saves the file. If a
    /// grant already trusts it, nothing is written. The file is read again
    /// first, so the store then holds the grants on disk too.
    ///
    /// It blocks on the file system, so call it on a worker.
    ///
    /// # Errors
    ///
    /// - [`WorkspaceError::Path`] when the folder's path is not UTF-8, and
    ///   [`WorkspaceError::NotCanonical`] when it is not spelled as its
    ///   names join, which a grant must be.
    /// - [`WorkspaceError::Unwritable`] when the file on disk cannot be
    ///   used, which it is not replaced.
    /// - [`WorkspaceError::TooManyGrants`] when the store holds
    ///   [`MAX_GRANTS`].
    /// - [`WorkspaceError::CreateFolder`] or [`WorkspaceError::Save`] when
    ///   it cannot be written.
    ///
    /// The store is unchanged by an error, except that it holds what the
    /// file on disk says once that was read.
    pub fn trust(&mut self, folder: &Folder) -> Result<(), WorkspaceError> {
        path::to_utf8(folder.root())?;
        if !is_plain(folder.root()) {
            return Err(WorkspaceError::NotCanonical {
                path: folder.root().to_path_buf(),
            });
        }
        let mut grants = self.reload()?;
        if grants
            .iter()
            .any(|grant| path::under(folder.root(), grant).is_some())
        {
            return Ok(());
        }
        if grants.len() >= MAX_GRANTS {
            return Err(WorkspaceError::TooManyGrants { limit: MAX_GRANTS });
        }
        grants.push(folder.root().to_path_buf());
        grants.sort();
        self.save(&grants)?;
        self.grants = grants;
        Ok(())
    }

    /// Removes the grant `grant`, exactly, and saves the file: the folders
    /// it trusted are no longer trusted, unless another grant trusts them.
    /// Returns whether there was such a grant; if there was none, nothing
    /// is written. The file is read again first, as for
    /// [`TrustStore::trust`].
    ///
    /// It blocks on the file system, so call it on a worker.
    ///
    /// # Errors
    ///
    /// As [`TrustStore::trust`], but for the path's text and the limit.
    pub fn revoke(&mut self, grant: &Path) -> Result<bool, WorkspaceError> {
        let mut grants = self.reload()?;
        let before = grants.len();
        grants.retain(|kept| kept != grant);
        if grants.len() == before {
            return Ok(false);
        }
        self.save(&grants)?;
        self.grants = grants;
        Ok(true)
    }

    /// Reads the file again, keeping what it says in the store, or the
    /// error of a file that cannot be used, which is not to be replaced.
    fn reload(&mut self) -> Result<Vec<PathBuf>, WorkspaceError> {
        match read_grants(&self.file) {
            Ok(grants) => {
                self.grants.clone_from(&grants);
                self.problem = None;
                Ok(grants)
            }
            Err(problem) => {
                let problem = Arc::new(problem);
                self.grants.clear();
                self.problem = Some(Arc::clone(&problem));
                Err(WorkspaceError::Unwritable {
                    path: self.file.clone(),
                    reason: problem,
                })
            }
        }
    }

    /// Writes `grants` to the file, making its folder if needed, once their
    /// text reads back as the same grants, so that a file the next load
    /// would refuse is never written.
    fn save(&self, grants: &[PathBuf]) -> Result<(), WorkspaceError> {
        let text = to_text(grants)?;
        if parse(text.as_bytes(), &self.file)? != grants {
            return Err(WorkspaceError::BadTrustFile {
                path: self.file.clone(),
                reason: "its text would not read back as the same folders",
            });
        }
        if let Some(dir) = self.file.parent() {
            std::fs::create_dir_all(dir).map_err(|source| WorkspaceError::CreateFolder {
                path: dir.to_path_buf(),
                source,
            })?;
        }
        tidepool_text::file::save(&self.file, &Rope::from(text.as_str()), Format::UTF8).map_err(
            |source| WorkspaceError::Save {
                path: self.file.clone(),
                source,
            },
        )
    }
}

/// The grants a trust file's `bytes` hold, sorted, the file named `path` in
/// errors. It reads nothing, and never panics.
///
/// # Errors
///
/// [`WorkspaceError::TooLarge`] past [`MAX_FILE_BYTES`];
/// [`WorkspaceError::Toml`] for text that is not TOML;
/// [`WorkspaceError::Version`] for a `version` other than [`VERSION`],
/// or none; [`WorkspaceError::BadTrustFile`] for anything else that is not
/// a trust file: `folders` missing or not an array of strings, an entry
/// that is not an absolute path free of `.` and `..` and spelled as its
/// names join (no `//`, no final separator), or more than [`MAX_GRANTS`].
pub fn parse(bytes: &[u8], path: &Path) -> Result<Vec<PathBuf>, WorkspaceError> {
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_FILE_BYTES {
        return Err(WorkspaceError::TooLarge {
            path: path.to_path_buf(),
            limit: MAX_FILE_BYTES,
        });
    }
    let bad = |reason| WorkspaceError::BadTrustFile {
        path: path.to_path_buf(),
        reason,
    };
    let root = tidepool_codec::toml::parse_bytes(bytes, Limits::DEFAULT).map_err(|source| {
        WorkspaceError::Toml {
            path: path.to_path_buf(),
            source,
        }
    })?;
    if root.get("version").and_then(Value::as_integer) != Some(VERSION) {
        return Err(WorkspaceError::Version {
            path: path.to_path_buf(),
        });
    }
    let folders = root
        .get("folders")
        .and_then(Value::as_array)
        .ok_or_else(|| bad("`folders` is missing, or is not an array"))?;
    if folders.len() > MAX_GRANTS {
        return Err(bad("`folders` holds more than 4,096 folders"));
    }
    let mut grants = Vec::with_capacity(folders.len());
    for entry in folders.items() {
        let text = entry
            .as_str()
            .ok_or_else(|| bad("an entry of `folders` is not a string"))?;
        let grant = PathBuf::from(text);
        if !is_plain(&grant) {
            return Err(bad(
                "an entry of `folders` is not an absolute path free of `.` and `..`",
            ));
        }
        grants.push(grant);
    }
    grants.sort();
    grants.dedup();
    Ok(grants)
}

/// Reads and parses the trust file at `file`; a missing one holds no
/// grants.
fn read_grants(file: &Path) -> Result<Vec<PathBuf>, WorkspaceError> {
    match read::bounded(file, MAX_FILE_BYTES, Links::Follow)? {
        Some(bytes) => parse(&bytes, file),
        None => Ok(Vec::new()),
    }
}

/// Whether `path` is absolute, free of `.` and `..`, and spelled as its
/// names join (no `//`, no final separator), as a canonical path is.
/// `Path::components` drops a `.` inside a path, so the spelling is
/// compared too.
fn is_plain(path: &Path) -> bool {
    path.is_absolute()
        && !path
            .components()
            .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
        && path.components().collect::<PathBuf>().as_os_str() == path.as_os_str()
}

/// The text of a trust file holding `grants`, through `codec`'s writer,
/// which quotes each path as TOML needs.
fn to_text(grants: &[PathBuf]) -> Result<String, WorkspaceError> {
    let mut items = Vec::with_capacity(grants.len());
    for grant in grants {
        items.push(Value::from(ValueKind::String(
            path::to_utf8(grant)?.to_string(),
        )));
    }
    let edit = |source| WorkspaceError::Edit {
        path: PathBuf::from(FILE_NAME),
        source,
    };
    let mut document = Document::parse(HEADER, Limits::DEFAULT).map_err(edit)?;
    document
        .set(&["version".into()], &ValueKind::Integer(VERSION))
        .map_err(edit)?;
    document
        .set(&["folders".into()], &ValueKind::Array(Array::new(items)))
        .map_err(edit)?;
    Ok(document.into_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_reads_back_as_its_grants() {
        let grants = vec![PathBuf::from("/a/b"), PathBuf::from("/c \"q\" \\d")];
        let text = to_text(&grants).unwrap();
        assert!(text.starts_with(HEADER), "{text}");
        assert!(text.contains("version = 1\n"), "{text}");
        let mut sorted = grants.clone();
        sorted.sort();
        assert_eq!(
            parse(text.as_bytes(), Path::new(FILE_NAME)).unwrap(),
            sorted
        );
    }

    #[test]
    fn only_absolute_plain_paths_are_grants() {
        for text in [
            "version = 1\nfolders = [\"relative\"]\n",
            "version = 1\nfolders = [\"/a/../b\"]\n",
            "version = 1\nfolders = [\"/a/./b\"]\n",
            "version = 1\nfolders = [\"/a//b\"]\n",
            "version = 1\nfolders = [\"/a/b/\"]\n",
            "version = 1\nfolders = [1]\n",
            "version = 1\nfolders = \"/a\"\n",
            "version = 1\n",
        ] {
            assert!(
                matches!(
                    parse(text.as_bytes(), Path::new("t")),
                    Err(WorkspaceError::BadTrustFile { .. })
                ),
                "{text}"
            );
        }
        for text in [
            "folders = []\n",
            "version = 2\nfolders = []\n",
            "version = \"1\"\n",
        ] {
            assert!(
                matches!(
                    parse(text.as_bytes(), Path::new("t")),
                    Err(WorkspaceError::Version { .. })
                ),
                "{text}"
            );
        }
    }
}
