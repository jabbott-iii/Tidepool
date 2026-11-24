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

//! Reading the small files this crate reads: ignore files, settings and
//! the trust store, each a regular file of bounded size.
//!
//! New in Tidepool (M4.7, ADR-0067).

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;

use crate::WorkspaceError;

/// Whether a symbolic link at the file's own name is followed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Links {
    /// Followed, as for the user's own settings, which dotfile managers
    /// often link.
    Follow,
    /// Not followed: a link is [`WorkspaceError::NotRegularFile`], as Git
    /// reads a `.gitignore`.
    Refuse,
}

/// The bytes of `path`, or `None` if there is no file there.
///
/// The file must be a regular file, checked before it is opened, so that a
/// pipe or a device is never opened (opening a pipe can wait forever), and
/// again on the opened file. It is read up to one byte past `max_bytes`,
/// whatever its metadata says, so a file that grows is caught too.
///
/// It blocks on the file system, so call it on a worker.
///
/// # Errors
///
/// [`WorkspaceError::NotRegularFile`], [`WorkspaceError::TooLarge`] past
/// `max_bytes`, and [`WorkspaceError::Read`] for any other error but a
/// missing file.
pub(crate) fn bounded(
    path: &Path,
    max_bytes: u64,
    links: Links,
) -> Result<Option<Vec<u8>>, WorkspaceError> {
    let read = |source| WorkspaceError::Read {
        path: path.to_path_buf(),
        source,
    };
    let metadata = match links {
        Links::Follow => fs::metadata(path),
        Links::Refuse => fs::symlink_metadata(path),
    };
    let metadata = match metadata {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(read(error)),
    };
    if !metadata.is_file() {
        return Err(WorkspaceError::NotRegularFile {
            path: path.to_path_buf(),
        });
    }
    let file = match File::open(path) {
        Ok(file) => file,
        // Removed between the two looks: as if it had never been there.
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(read(error)),
    };
    if !file.metadata().map_err(read)?.is_file() {
        return Err(WorkspaceError::NotRegularFile {
            path: path.to_path_buf(),
        });
    }
    let mut bytes = Vec::new();
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(read)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > max_bytes {
        return Err(WorkspaceError::TooLarge {
            path: path.to_path_buf(),
            limit: max_bytes,
        });
    }
    Ok(Some(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regular_files_are_read_within_their_limit() {
        let dir = std::env::temp_dir().join(format!("tidepool-read-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.toml");
        assert!(bounded(&file, 4, Links::Follow).unwrap().is_none());
        fs::write(&file, b"1234").unwrap();
        assert_eq!(
            bounded(&file, 4, Links::Refuse).unwrap().as_deref(),
            Some(&b"1234"[..])
        );
        assert!(matches!(
            bounded(&file, 3, Links::Follow),
            Err(WorkspaceError::TooLarge { limit: 3, .. })
        ));
        assert!(matches!(
            bounded(&dir, 4, Links::Follow),
            Err(WorkspaceError::NotRegularFile { .. })
        ));
        fs::remove_dir_all(&dir).unwrap();
    }
}
