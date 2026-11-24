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

//! What a save checks about a file before it replaces it: the file's
//! [`metadata`] (its link count, owner, group, permission bits and
//! identity), and the owner and group a new file in its folder would get
//! ([`new_file_owner`]) (M4.10, ADR-0068; the identity, M4.12, ADR-0070).
//!
//! New in Tidepool.
//!
//! # Why a save needs these
//!
//! `tidepool-text` saves a file atomically: it writes a temporary file in
//! the same folder and renames it over the old one, so a crash never
//! leaves half a file (ADR-0025). The rename puts a *new* file at the
//! path, and on Unix a new file differs from the old one in two ways that
//! matter:
//!
//! - **A hard link splits.** A hard link is a second name for the same
//!   file. After the rename, the path names the new file and the other
//!   name still names the old text. [`Metadata::links`] above 1 means the
//!   file has another name.
//! - **The owner and group can change.** A new file belongs to the user
//!   that makes it, and to that user's group or its folder's
//!   ([`new_file_owner`]). Saving another user's file, as root does,
//!   gives it to the saver.
//!
//! So `workspace::buffers` (M4.11) asks the user whether to write in place
//! instead when a save would change either (ADR-0068). It reads the
//! [`metadata`] of the file and of the save's own temporary file, just
//! before the rename, which sees what the file system really gives a new
//! file; [`new_file_owner`] is the rule for it without making one
//! (ADR-0069).
//!
//! When it asks, the user answers later, and by then the path may name
//! another file: another program renamed its own over it, or deleted it
//! and made a new one. So `workspace::buffers` keeps the [`FileId`] of the
//! file it asked about, and before it acts on the answer, reads the file's
//! identity again and compares (ADR-0070).
//!
//! # Systems
//!
//! Linux reads them with `stat(2)` through `std`, and the process's
//! effective user and group with `geteuid(2)` and `getegid(2)`. Every
//! other system fails with [`Error::Unsupported`] until macOS (M11.8) and
//! Windows (M12.2) add theirs.

use std::fs::File;
use std::path::Path;
use std::time::SystemTime;

use crate::{Error, backend};

/// Who owns a file: a user and a group, by the numbers the system gives
/// them (`uid_t` and `gid_t` on Unix).
///
/// Inside a user namespace (a rootless container, toolbox or distrobox, a
/// Flatpak or bubblewrap sandbox), a user or group that the namespace does
/// not map reads as the overflow id, 65534 by default
/// (`/proc/sys/kernel/overflowuid` and `overflowgid`), so two different
/// groups can read as the same number. A caller deciding whether a save
/// changes a file's owner treats that id as unknown (user_namespaces(7),
/// ADR-0068).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Owner {
    /// The user's number: 0 is root.
    pub user: u32,
    /// The group's number.
    pub group: u32,
}

/// Which file a name reaches, so that two looks at a name can tell
/// whether they found the same file (ADR-0070).
///
/// A file system numbers its files: the number is the file's *inode*
/// (index node, the record that holds a file's data and metadata, as
/// opposed to its names), and the file system itself is told apart by a
/// device number. Every name of a file, its hard links included, reaches
/// the same inode, so they share an identity. A rename over the name, or
/// deleting the file and making another at the name, makes the name reach
/// another file.
///
/// That other file can get the same inode number: a file system may give
/// a new file the number of one just deleted. On the reference machine's
/// ext4, a file deleted and made again got the same number in each of
/// 2,000 tries (2026-11-23). So the identity also holds the file's birth
/// time, where the file system reports one, which differed in each try:
/// not because its clock is fine (2,000 files made back to back there had
/// 48 birth times between them), but because a look at the first file
/// and its deletion came between the two, as they do between a save's
/// question and a file made again (ADR-0070).
/// Where it reports none, [`FileId::born`] is `None` and a file made after
/// another was deleted can share its identity; where its clock is coarse,
/// two files made within one tick of it can share a birth time. Equal
/// identities mean the same file as far as can be known, never more.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileId {
    /// The file system's device number (`st_dev` on Unix).
    pub device: u64,
    /// The file's number in that file system (`st_ino` on Unix).
    pub inode: u64,
    /// When the file was made (its birth time, `stx_btime` of statx(2) on
    /// Linux), or `None` where the file system does not say.
    pub born: Option<SystemTime>,
}

/// What a save checks about a file, read by [`metadata`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Metadata {
    /// How many names (hard links) the file has. A regular file has 1
    /// unless another name was made for it, with `ln` or `link(2)`.
    pub links: u64,
    /// The file's owner and group.
    pub owner: Owner,
    /// The permission bits of the file's mode, `0o7777` at most: setuid
    /// (`0o4000`), setgid (`0o2000`), sticky (`0o1000`), then read, write
    /// and run for the owner, the group and others (`0o755` is `rwxr-xr-x`).
    /// The file's type is not in them. A folder whose write bit for
    /// others (`0o002`) is set lets them make names in it, and rename and
    /// delete any name in it; when it is also sticky, as `/tmp` is, they
    /// can still make names, but only a name's owner, the folder's owner
    /// and a process with `CAP_FOWNER` (root) may rename or delete it.
    pub permissions: u32,
    /// Which file this is (M4.12).
    pub id: FileId,
}

/// Reads the link count, owner, group, permission bits and identity of the
/// file at `path`, following symbolic links to the file they name, as
/// `stat(2)` does (Linux follows at most 40 links in one lookup).
///
/// `text::file::save` follows the links at a path to the file it replaces
/// too (ADR-0025), but reads each link's text and joins it itself, so for
/// a link into `/proc` (`/proc/self/fd/3`, say) the two can reach
/// different files. A check before a save reads the file the save
/// resolved to.
///
/// This blocks on the file system, so call it on a worker, never on the
/// main thread (`AGENTS.md` rule 6).
///
/// # Errors
///
/// - [`Error::Io`] at `stat` on Linux when the file cannot be read: it
///   does not exist, a folder on the way cannot be searched, or a link
///   leads nowhere or round in a loop.
/// - [`Error::Unsupported`] on every other system, until macOS (M11.8)
///   and Windows (M12.2).
pub fn metadata(path: &Path) -> Result<Metadata, Error> {
    backend::file_metadata(path)
}

/// Reads the link count, owner, group, permission bits and identity of
/// the open `file`, as [`metadata`] does for a path (`fstat(2)` on the
/// open file, through `std`).
///
/// An open file is the file itself, whatever happens to the names that
/// reached it, so this tells exactly which file a write through `file`
/// changes (`text::file::write_in_place_if`, M4.12, ADR-0070).
///
/// This blocks on the file system, so call it on a worker, never on the
/// main thread (`AGENTS.md` rule 6).
///
/// # Errors
///
/// - [`Error::Io`] at `fstat` on Linux when the file's metadata cannot be
///   read.
/// - [`Error::Unsupported`] on every other system, until macOS (M11.8)
///   and Windows (M12.2).
pub fn metadata_of(file: &File) -> Result<Metadata, Error> {
    backend::open_file_metadata(file)
}

/// The owner and group that a file this process makes in `folder` gets,
/// following symbolic links to the folder.
///
/// For a save, `folder` is the folder that holds the file the save
/// replaces once its links are followed, such as the parent of that
/// file's canonical path: not the folder of a link to it, which can have
/// another group or setgid bit. A bare file name's parent is the empty
/// path, which names no folder.
///
/// On Linux the owner is the process's effective user, and the group is
/// `folder`'s own when `folder` has the setgid bit (`0o2000`), else the
/// process's effective group (open(2)). This is a rule, not a look at a
/// new file, and these cases differ from it without its seeing them:
///
/// - a file system mounted with `bsdgroups` (also called `grpid`,
///   ext4(5)) gives every new file its folder's group;
/// - one without Unix owners (FAT, or a network share mapped to one user)
///   gives every file the owner its mount options name;
/// - NFS with root squashing, an export's default, gives root's new files
///   the anonymous user and group (65534 by default), and `all_squash`
///   gives everyone's (exports(5));
/// - inside a user namespace, unmapped ids read as the overflow id
///   ([`Owner`], user_namespaces(7)), so the folder's group and a file's
///   can look the same when they are not.
///
/// On those a check against this can ask the user about a change that
/// does not happen, or miss one that does; a caller treats an error, and
/// an overflow id, as not knowing, and asks (ADR-0068).
///
/// This blocks on the file system, so call it on a worker, never on the
/// main thread (`AGENTS.md` rule 6).
///
/// # Errors
///
/// - [`Error::Io`] at `stat` on Linux when `folder` cannot be read, or is
///   not a folder ([`std::io::ErrorKind::NotADirectory`]).
/// - [`Error::Unsupported`] on every other system, until macOS (M11.8)
///   and Windows (M12.2).
pub fn new_file_owner(folder: &Path) -> Result<Owner, Error> {
    backend::new_file_owner(folder)
}
