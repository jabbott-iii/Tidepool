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

//! The directories the fuzzer reads and writes under the root, reached
//! without following a symbolic link.
//!
//! New for Tidepool. Every directory on the way from the root to a seed or
//! crash directory is looked at with `symlink_metadata`, one component at
//! a time, shallowest first, and a link is refused, naming it. So the
//! fuzzer never reads, writes, renames or deletes through a link planted
//! at `fuzz/`, `fuzz/crashes/` or below. The root itself, and what is above
//! it, are not checked. A missing directory is made one level at a time
//! with `fs::create_dir`, never `create_dir_all`, and looked at again
//! before the walk goes into it. Files are made with `create_new`, which
//! fails on anything already there, a link included.
//!
//! Not covered: races. A directory swapped for a link between the walk and
//! its use is followed; std has no `openat`.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use super::{DIED_PREFIX, FuzzError, Kind, LAST_INPUT, MAX_SEED_BYTES};
use crate::hash::fnv1a64;

/// What the walk to a directory found, when nothing on the way is a link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Walked {
    /// Every component is a directory.
    Present,
    /// One is missing, and so is everything past it.
    Missing,
}

/// The components of `relative`, which must name a place under the root:
/// no `..`, no root or prefix.
fn components(relative: &Path) -> Result<Vec<&Path>, FuzzError> {
    relative
        .components()
        .map(|component| match component {
            Component::Normal(part) => Ok(Path::new(part)),
            _ => Err(FuzzError::NotUnderRoot {
                path: relative.to_path_buf(),
            }),
        })
        .collect()
}

/// Walks `relative` under `root` one component at a time. Each component
/// that exists must be a directory and not a link.
///
/// # Errors
///
/// [`FuzzError::Link`] for a link, [`FuzzError::NotAFile`] for anything
/// else that is not a directory, [`FuzzError::NotUnderRoot`] for a path
/// that leaves the root, [`FuzzError::Read`] if a component cannot be
/// looked at.
pub(super) fn walk(root: &Path, relative: &Path) -> Result<Walked, FuzzError> {
    let mut path = root.to_path_buf();
    for part in components(relative)? {
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(FuzzError::Link { path });
            }
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => return Err(FuzzError::NotAFile { path }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Walked::Missing),
            Err(source) => return Err(FuzzError::Read { path, source }),
        }
    }
    Ok(Walked::Present)
}

/// [`walk`], making each missing component with `fs::create_dir` and
/// looking at it again before going into it. Returns the directories it
/// made, shallowest first, so that a caller can remove them again.
///
/// # Errors
///
/// As for [`walk`], and [`FuzzError::Write`] if a directory cannot be
/// made.
pub(super) fn make_dirs(root: &Path, relative: &Path) -> Result<Vec<PathBuf>, FuzzError> {
    let mut path = root.to_path_buf();
    let mut made = Vec::new();
    for part in components(relative)? {
        path.push(part);
        if let Err(error) = fs::symlink_metadata(&path)
            && error.kind() == io::ErrorKind::NotFound
        {
            match fs::create_dir(&path) {
                Ok(()) => made.push(path.clone()),
                // Made by someone else in between: looked at like any other.
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(source) => return Err(FuzzError::Write { path, source }),
            }
        }
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(FuzzError::Link { path });
            }
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => return Err(FuzzError::NotAFile { path }),
            Err(source) => return Err(FuzzError::Read { path, source }),
        }
    }
    Ok(made)
}

/// `fuzz/crashes/<name>` under the root, as a relative path.
pub(super) fn crash_dir(name: &str) -> PathBuf {
    Path::new("fuzz").join("crashes").join(name)
}

/// `fuzz/corpus/<name>` under the root, as a relative path.
pub(super) fn corpus_dir(name: &str) -> PathBuf {
    Path::new("fuzz").join("corpus").join(name)
}

/// Writes `bytes` to `dir/name` whole or not at all: to a hidden file of
/// its own, made with `create_new`, then renamed into place, so that a
/// reader never sees part of it. `dir` must already have been walked.
///
/// # Errors
///
/// [`FuzzError::Write`] naming `dir/name` if it cannot be written.
pub(super) fn write_whole(dir: &Path, name: &str, bytes: &[u8]) -> Result<PathBuf, FuzzError> {
    write_whole_open(dir, name, bytes).map(|(path, _)| path)
}

/// [`write_whole`], keeping the file open for writing: the handle is that
/// of the file now at `dir/name`. Until the rename, whatever was at
/// `dir/name` stays as it was.
///
/// # Errors
///
/// [`FuzzError::Write`] naming `dir/name` if it cannot be written.
pub(super) fn write_whole_open(
    dir: &Path,
    name: &str,
    bytes: &[u8],
) -> Result<(PathBuf, File), FuzzError> {
    let path = dir.join(name);
    let write_error = |source| FuzzError::Write {
        path: path.clone(),
        source,
    };
    let mut attempt = 0_u32;
    let (temporary, file) = loop {
        let temporary = dir.join(format!(".{name}.{}.{attempt}.tmp", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(mut file) => {
                if let Err(source) = io::Write::write_all(&mut file, bytes) {
                    drop(file);
                    let _ = fs::remove_file(&temporary);
                    return Err(write_error(source));
                }
                break (temporary, file);
            }
            // Left by a run that was killed while writing it, or another
            // process's: try another name.
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists && attempt < 100 => {
                attempt += 1;
            }
            Err(source) => return Err(write_error(source)),
        }
    };
    // A rename replaces what is at `path`, a link included, and never
    // follows it.
    fs::rename(&temporary, &path).map_err(|source| {
        let _ = fs::remove_file(&temporary);
        write_error(source)
    })?;
    Ok((path, file))
}

/// What [`keep_died`] did with a dead replay's input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kept {
    /// It is now `died-<hash>`, which was not there before.
    New(PathBuf),
    /// The replay died on an input that is already a saved crash, which
    /// every run replays first: a `died-<hash>` it replaced, or a saved
    /// crash of another kind (`panic-<hash>` and the like) with the same
    /// bytes, which stays as it is while the dead replay's file goes.
    Existing(PathBuf),
}

impl Kept {
    /// Where the input is kept.
    pub fn path(&self) -> &Path {
        match self {
            Self::New(path) | Self::Existing(path) => path,
        }
    }
}

/// Keeps the input a replay that died left in `fuzz/crashes/<name>/` (its
/// hidden [`LAST_INPUT`]): renames it to `died-<hash>`, the FNV-1a hash of
/// its bytes in 16 hex digits, where it runs with the saved crashes from
/// then on. A file already of that name is replaced, so it holds these
/// bytes whatever it held. When a saved crash of another kind of the same
/// hash (`<kind>-<hash>`, [`Kind::file_prefix`]) is a file that holds
/// these bytes, the replay died on that crash: it is left as it is, and
/// the dead replay's file is removed rather than kept twice. Returns where
/// it is kept, or `None` if there was nothing to keep.
///
/// Call it only once the process running the replay has ended without a
/// result: while a replay runs, the file is its own. `cargo xtask
/// fuzz-smoke` calls it after a death or an exit without a result, when
/// the replay wrote the file, but not after an interrupt.
///
/// Every directory on the way is walked without following a link, and so
/// is the file itself.
///
/// # Errors
///
/// [`FuzzError`] for a link or anything else in the way, and if the file
/// cannot be read or renamed.
pub fn keep_died(root: &Path, name: &str) -> Result<Option<Kept>, FuzzError> {
    let Some((last, bytes)) = last_input(root, name)? else {
        return Ok(None);
    };
    let dir = root.join(crash_dir(name));
    let hash = fnv1a64(&bytes);
    for kind in [Kind::Panic, Kind::Hang, Kind::Memory, Kind::Reserved] {
        let saved = dir.join(format!("{}-{hash:016x}", kind.file_prefix()));
        if holds(&saved, &bytes) {
            fs::remove_file(&last).map_err(|source| FuzzError::Write { path: last, source })?;
            return Ok(Some(Kept::Existing(saved)));
        }
    }
    let kept = dir.join(format!("{DIED_PREFIX}-{hash:016x}"));
    let existed = fs::symlink_metadata(&kept).is_ok();
    // A rename replaces what is at `kept`, a link included, and never
    // follows it.
    fs::rename(&last, &kept).map_err(|source| FuzzError::Write {
        path: kept.clone(),
        source,
    })?;
    Ok(Some(if existed {
        Kept::Existing(kept)
    } else {
        Kept::New(kept)
    }))
}

/// Reads what an earlier replay left in `fuzz/crashes/<name>/` (its hidden
/// [`LAST_INPUT`]), for a replay that is about to run it first. It stays
/// where it is until that replay's own file replaces it, just before its
/// first input, which is this one: a replay that fails before then leaves
/// it. It is not kept as `died-<hash>`: whether the earlier replay died on
/// it or was interrupted is not known here.
///
/// # Errors
///
/// As for [`keep_died`].
pub(super) fn leftover(root: &Path, name: &str) -> Result<Option<Vec<u8>>, FuzzError> {
    Ok(last_input(root, name)?.map(|(_, bytes)| bytes))
}

/// Whether `path` is a file, not a link, that holds `bytes`. Anything that
/// cannot be read does not.
fn holds(path: &Path, bytes: &[u8]) -> bool {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && metadata.len() == bytes.len() as u64 => {}
        _ => return false,
    }
    let mut held = Vec::new();
    let read = fs::File::open(path)
        .and_then(|file| file.take(bytes.len() as u64 + 1).read_to_end(&mut held));
    read.is_ok() && held == bytes
}

/// The path and bytes of `fuzz/crashes/<name>/.last-input`, if there is
/// one, walked without following a link.
fn last_input(root: &Path, name: &str) -> Result<Option<(PathBuf, Vec<u8>)>, FuzzError> {
    let dir = crash_dir(name);
    if walk(root, &dir)? == Walked::Missing {
        return Ok(None);
    }
    let last = root.join(dir).join(LAST_INPUT);
    match fs::symlink_metadata(&last) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(FuzzError::Link { path: last });
        }
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Err(FuzzError::NotAFile { path: last }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(FuzzError::Read { path: last, source }),
    }
    let mut bytes = Vec::new();
    fs::File::open(&last)
        .and_then(|file| file.take(MAX_SEED_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|source| FuzzError::Read {
            path: last.clone(),
            source,
        })?;
    if bytes.len() as u64 > MAX_SEED_BYTES {
        return Err(FuzzError::TooLarge { path: last });
    }
    Ok(Some((last, bytes)))
}
