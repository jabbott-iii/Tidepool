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

//! The rotating log file.
//!
//! Ported from nadir-core `log.rs` (`FileConfig`, `RotatingFile` and the
//! rotation helpers). Adapted for Tidepool: the file is opened on the
//! caller's thread but written only by the log writer thread
//! ([`super::writer`]), so writes and rotations never run on the thread that
//! logs.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

/// How the file sink rotates. See [`open_file`](super::open_file).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileConfig {
    /// Rotate before a record would grow the file beyond this many bytes.
    /// A single record larger than this still gets written, alone in its
    /// file.
    pub max_bytes: u64,
    /// How many rotated files to keep besides the current one.
    pub keep: u32,
    /// Whether opening rotates an existing, non-empty file first, so that
    /// every run starts a fresh file. If `false`, opening appends.
    pub rotate_on_open: bool,
}

impl Default for FileConfig {
    /// 4 MiB per file, 3 rotated files (`tidepool.log.1` to `.3`),
    /// appending on open.
    fn default() -> Self {
        Self {
            max_bytes: 4 * 1024 * 1024,
            keep: 3,
            rotate_on_open: false,
        }
    }
}

pub(super) struct RotatingFile {
    path: PathBuf,
    config: FileConfig,
    file: File,
    /// Bytes in the current file.
    size: u64,
    /// Rotate before the file would grow past this size. It is
    /// `config.max_bytes`, pushed further out after a failed rotation.
    rotate_at: u64,
}

impl RotatingFile {
    pub(super) fn open(path: &Path, config: FileConfig) -> io::Result<Self> {
        if config.rotate_on_open && fs::metadata(path).is_ok_and(|existing| existing.len() > 0) {
            shift_files(path, config.keep)?;
        }
        let (file, size) = open_for_append(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            config,
            file,
            size,
            rotate_at: config.max_bytes,
        })
    }

    /// Appends `line`, which ends with a newline, rotating first if needed.
    /// Returns `false` if the write failed.
    pub(super) fn write_line(&mut self, line: &str) -> bool {
        let len = line.len() as u64;
        if self.size > 0 && self.size.saturating_add(len) > self.rotate_at && self.rotate().is_err()
        {
            // Keep logging to the current file, and do not retry on every
            // line: wait until another `max_bytes` have been written.
            self.rotate_at = self.size.saturating_add(self.config.max_bytes);
        }
        // A failed write (disk full, file removed) cannot be reported
        // through the log itself; the caller counts it.
        if self.file.write_all(line.as_bytes()).is_ok() {
            self.size = self.size.saturating_add(len);
            true
        } else {
            false
        }
    }

    /// Hands buffered data to the OS. `File` does not buffer, so this only
    /// reports errors the OS may defer.
    pub(super) fn flush(&mut self) -> bool {
        self.file.flush().is_ok()
    }

    /// A sink over a read-only handle to the running test binary, for queue
    /// tests that never write to it.
    #[cfg(test)]
    pub(super) fn unwritable_for_tests() -> Self {
        let path = std::env::current_exe().expect("the test binary has a path");
        let file = File::open(&path).expect("the test binary is readable");
        let config = FileConfig::default();
        Self {
            path,
            config,
            file,
            size: 0,
            rotate_at: config.max_bytes,
        }
    }

    fn rotate(&mut self) -> io::Result<()> {
        shift_files(&self.path, self.config.keep)?;
        let (file, size) = open_for_append(&self.path)?;
        // Assigning drops the old handle, which by now refers to the file
        // that was renamed to `<path>.1`.
        self.file = file;
        self.size = size;
        self.rotate_at = self.config.max_bytes;
        Ok(())
    }
}

fn open_for_append(path: &Path) -> io::Result<(File, u64)> {
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    let size = file.metadata()?.len();
    Ok((file, size))
}

/// Moves `path` out of the way: `path` becomes `path.1`, `path.1` becomes
/// `path.2`, and so on up to `path.<keep>`, whose old contents are lost.
/// With `keep == 0`, `path` is simply deleted.
fn shift_files(path: &Path, keep: u32) -> io::Result<()> {
    if keep == 0 {
        return ignore_not_found(fs::remove_file(path));
    }
    for index in (1..keep).rev() {
        ignore_not_found(fs::rename(numbered(path, index), numbered(path, index + 1)))?;
    }
    ignore_not_found(fs::rename(path, numbered(path, 1)))
}

/// Returns `path` with `.<index>` appended to the file name.
fn numbered(path: &Path, index: u32) -> PathBuf {
    let mut name = OsString::from(path);
    name.push(format!(".{index}"));
    PathBuf::from(name)
}

fn ignore_not_found(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_matches_the_documented_policy() {
        let config = FileConfig::default();
        assert_eq!(config.keep, 3);
        assert_eq!(config.max_bytes, 4 * 1024 * 1024);
        assert!(!config.rotate_on_open);
    }

    #[test]
    fn rotated_files_are_numbered_after_the_full_file_name() {
        assert_eq!(
            numbered(Path::new("logs/tidepool.log"), 1),
            Path::new("logs/tidepool.log.1")
        );
        assert_eq!(
            numbered(Path::new("tidepool.log"), 12),
            Path::new("tidepool.log.12")
        );
    }

    #[test]
    fn only_not_found_is_ignored() {
        assert!(ignore_not_found(Ok(())).is_ok());
        assert!(ignore_not_found(Err(io::ErrorKind::NotFound.into())).is_ok());
        let denied = ignore_not_found(Err(io::ErrorKind::PermissionDenied.into()));
        assert_eq!(
            denied.map_err(|error| error.kind()),
            Err(io::ErrorKind::PermissionDenied)
        );
    }
}
