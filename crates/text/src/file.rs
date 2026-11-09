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

//! Text files: [`load`] with the large-file threshold, and the atomic
//! [`save`] of ADR-0025.
//!
//! - **Loading** decodes by BOM ([`encoding`](crate::encoding)) and reports
//!   the format, the line endings, the trailing break and whether the file
//!   is large. Line endings and the trailing newline are reported, never
//!   changed: the rope holds the text as it was (ADR-0019), so an unchanged
//!   file saves byte for byte. A file with bytes that are not text has no
//!   format and is read-only, so no caller writes it back without choosing a
//!   format itself.
//! - **Saving** writes a temporary file in the target's directory, syncs it,
//!   gives it the target's permissions and renames it over the target, so a
//!   crash leaves either the old file or the new one, never a mix. Symbolic
//!   links are followed to the file they name, as the OS follows them, and
//!   stay links.
//! - **A rename makes a new file.** So a save splits a hard link (the other
//!   names keep the old text); gives the file the saver's owner and group on
//!   Unix, even when only the group differs; gives it the access list its
//!   directory passes on (a default POSIX ACL on Linux, the inherited ACL on
//!   Windows); drops extended attributes; and on Unix clears setuid and
//!   setgid, as a write in place would. `workspace::buffers` (M4.3) checks
//!   the link count, owner and group before a save (ADR-0025).
//! - **Callers** run loads and saves on workers, never on the main thread,
//!   and serialize the saves of one file ([`History::checkpoint`]).
//!
//! [`History::checkpoint`]: crate::History::checkpoint

use std::ffi::{OsStr, OsString};
use std::fs::{self, File, Metadata, OpenOptions, Permissions};
use std::io::{self, BufWriter, ErrorKind, Read, Write};
use std::path::{Component, MAIN_SEPARATOR_STR, Path, PathBuf, Prefix};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::encoding::{Decoded, Decoder, Format, encode};
use crate::{LineBreak, Rope, TextError};

/// A file of more than this many bytes, 16 MiB, is large: the editor opens
/// it in large-file mode (`docs/subsystems/editor.md`).
pub const LARGE_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// The start of every temporary file's name. A save that crashes before its
/// rename leaves a file named `.tidepool-save-<pid>-<count>-<time>.tmp` in
/// the target's directory; nothing removes it but the user, or a later
/// cleanup that looks for this prefix.
pub const TEMP_PREFIX: &str = ".tidepool-save-";

/// The most symbolic links a save follows, as Linux's `MAXSYMLINKS`.
const MAX_LINKS: usize = 40;

/// How many names a save tries for its temporary file.
const TEMP_ATTEMPTS: u32 = 16;

/// Bytes read, or buffered for writing, at a time.
const BLOCK: usize = 64 * 1024;

/// Temporary files made by this process, for their names.
static TEMPS: AtomicU32 = AtomicU32::new(0);

/// What a load or save was doing when the operating system failed it, in
/// [`TextError::Io`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FileStep {
    /// Reading the metadata of the path, or of a link on the way.
    Metadata,
    /// Opening the file to read it.
    Open,
    /// Reading the file.
    Read,
    /// Reading where a symbolic link points: reading the link; where
    /// Windows' path rules apply, reading the path to a relative link as
    /// Win32 reads it and finding the canonical path of its directory; or
    /// refusing a target that has a root or a drive but is not absolute
    /// ([`ErrorKind::InvalidInput`]).
    ReadLink,
    /// Opening the existing file for writing, which neither creates nor
    /// truncates it, to check that the user may write it. A refusal is
    /// [`TextError::NotWritable`] instead.
    CheckWritable,
    /// Creating the temporary file.
    CreateTemp,
    /// Writing the text to the temporary file.
    Write,
    /// Giving the temporary file the target's permissions.
    SetPermissions,
    /// Syncing the temporary file to the disk.
    Sync,
    /// Renaming the temporary file over the target.
    Rename,
    /// Syncing the directory after the rename. The target already holds the
    /// new text, but it may not survive a crash.
    SyncDirectory,
}

/// How many line breaks of each kind a text has (ADR-0019).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct LineEndings {
    /// LF breaks.
    pub lf: usize,
    /// CR LF breaks.
    pub crlf: usize,
    /// CR breaks not followed by LF.
    pub cr: usize,
}

impl LineEndings {
    /// The breaks of `text`. O(n).
    pub fn of(text: &Rope) -> Self {
        let (mut lf, mut cr) = (0usize, 0usize);
        for chunk in text.chunks() {
            let bytes = chunk.as_bytes();
            lf += bytes.iter().filter(|&&byte| byte == b'\n').count();
            cr += bytes.iter().filter(|&&byte| byte == b'\r').count();
        }
        // Every LF and CR is in exactly one break, and a CR LF holds one of
        // each, so the CR LFs are the bytes beyond the number of breaks.
        let breaks = text.len_lines().saturating_sub(1);
        let crlf = lf.saturating_add(cr).saturating_sub(breaks);
        Self {
            lf: lf.saturating_sub(crlf),
            crlf,
            cr: cr.saturating_sub(crlf),
        }
    }

    /// The number of breaks.
    pub fn total(&self) -> usize {
        self.lf.saturating_add(self.crlf).saturating_add(self.cr)
    }

    /// Whether the text has breaks of more than one kind.
    pub fn is_mixed(&self) -> bool {
        [self.lf, self.crlf, self.cr]
            .iter()
            .filter(|&&count| count > 0)
            .count()
            > 1
    }

    /// The break for new lines: the most common one. LF wins a tie with
    /// either other kind, and CR LF a tie with CR, so a text without breaks
    /// gives LF; a caller with a setting for new files uses it when
    /// [`total`](LineEndings::total) is 0.
    pub fn dominant(&self) -> LineBreak {
        if self.crlf > self.lf && self.crlf >= self.cr {
            LineBreak::CrLf
        } else if self.cr > self.lf && self.cr > self.crlf {
            LineBreak::Cr
        } else {
            LineBreak::Lf
        }
    }
}

/// A loaded file: its decoded text and what [`load`] found out about it.
///
/// `Debug` shows sizes only, never the text.
#[derive(Clone, Debug)]
pub struct Loaded {
    decoded: Decoded,
    line_endings: LineEndings,
    trailing_break: Option<LineBreak>,
    file_len: u64,
}

impl Loaded {
    /// The text, exactly as the file has it apart from the BOM; every
    /// replaced sequence is one U+FFFD.
    pub fn text(&self) -> &Rope {
        self.decoded.text()
    }

    /// The text, without the rest.
    pub fn into_text(self) -> Rope {
        self.decoded.into_text()
    }

    /// The decoding, with the replaced runs and the original bytes.
    pub fn decoded(&self) -> &Decoded {
        &self.decoded
    }

    /// The decoding, without the rest.
    pub fn into_decoded(self) -> Decoded {
        self.decoded
    }

    /// The format to save the text in, or `None` exactly when bytes were
    /// replaced ([`Decoded::format`]).
    pub fn format(&self) -> Option<Format> {
        self.decoded.format()
    }

    /// The format the bytes were read as ([`Decoded::detected`]).
    pub fn detected(&self) -> Format {
        self.decoded.detected()
    }

    /// Whether bytes were replaced, so that the file must not be written
    /// back ([`Decoded::read_only`]). A large file is not read-only here;
    /// large-file mode is the editor's.
    pub fn read_only(&self) -> bool {
        self.decoded.read_only()
    }

    /// The line breaks, by kind.
    pub fn line_endings(&self) -> LineEndings {
        self.line_endings
    }

    /// The break the text ends with; `None` if it has no trailing newline.
    pub fn trailing_break(&self) -> Option<LineBreak> {
        self.trailing_break
    }

    /// The number of bytes read.
    pub fn file_len(&self) -> u64 {
        self.file_len
    }

    /// Whether the file had more than [`LARGE_FILE_BYTES`] bytes, counted as
    /// read.
    pub fn is_large(&self) -> bool {
        self.file_len > LARGE_FILE_BYTES
    }
}

/// Loads the text file at `path`, following symbolic links.
///
/// Only a regular file loads, checked from its metadata before it is
/// opened, so that a FIFO never blocks. Should the path change into one
/// between the check and the open, the open blocks: `std` has no way to
/// open without blocking. The file is read in 64 KiB blocks into a
/// [`Decoder`], so it is never held twice. Nothing is written.
///
/// # Errors
///
/// [`NotRegularFile`](TextError::NotRegularFile) for a directory or a
/// special file. [`Io`](TextError::Io) at [`FileStep::Metadata`],
/// [`Open`](FileStep::Open) or [`Read`](FileStep::Read).
pub fn load(path: impl AsRef<Path>) -> Result<Loaded, TextError> {
    let path = path.as_ref();
    let metadata = fs::metadata(path).map_err(|error| io_error(FileStep::Metadata, error))?;
    check_regular(&metadata)?;
    let mut file = File::open(path).map_err(|error| io_error(FileStep::Open, error))?;
    let mut decoder = Decoder::new();
    let mut block = vec![0; BLOCK];
    let mut file_len = 0u64;
    loop {
        let read = match file.read(&mut block) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(io_error(FileStep::Read, error)),
        };
        let Some(bytes) = block.get(..read) else {
            // `Read` promises at most the buffer's length.
            return Err(io_error(FileStep::Read, ErrorKind::InvalidData.into()));
        };
        decoder.push(bytes);
        file_len = file_len.saturating_add(read as u64);
    }
    let decoded = decoder.finish();
    Ok(Loaded {
        line_endings: LineEndings::of(decoded.text()),
        trailing_break: trailing_break(decoded.text()),
        decoded,
        file_len,
    })
}

/// Saves `text` in `format` to `path` atomically (ADR-0025): the file holds
/// either its old bytes or exactly the new ones, even after a crash.
///
/// 1. Follows the chain of symbolic links at `path`, at most 40, to the file
///    they name: the target. A relative link is followed from its own
///    directory as the OS reaches it, so its `..` apply after any directory
///    links before it (ADR-0034). On Unix its target is joined onto the
///    link's parent as written, and the kernel resolves the joined path;
///    where Windows' path rules apply, onto the canonical path of the
///    link's directory, taken from the whole path in its verbatim form. An
///    absolute target is used as it is; one that has a root or a drive but
///    is not absolute (`\x`, `c:x`, which only Windows parses) is refused.
///    The target need not exist (a new file, or a link to nothing); then
///    the save creates it, unless it ends in a separator, which asks for a
///    directory.
/// 2. An existing target must be a regular file that the user may write,
///    checked by opening it for writing without creating or truncating it,
///    even if its directory is writable.
/// 3. Creates a temporary file in the target's directory, never replacing
///    or following anything, named [`TEMP_PREFIX`] and a fixed-length
///    suffix. For an existing target it starts with mode 0600 on Unix, so
///    no other process can open it before it has the target's permissions;
///    for a new file it gets `std`'s default mode, as the file would.
/// 4. Writes the encoded text through a buffer, gives the temporary file
///    the target's permissions (on Unix without setuid and setgid), and
///    syncs it.
/// 5. Renames it over the target, so links to the target stay links.
/// 6. Syncs the directory, so the rename survives a crash. Where the
///    directory cannot be opened (always on Windows, where `std` cannot
///    open one; on Unix, a directory the user may not read), or the file
///    system refuses to sync a directory (`EINVAL`, `ENOTSUP`), this is
///    skipped.
///
/// A crash before step 5 leaves the target as it was and the temporary
/// file behind ([`TEMP_PREFIX`]).
///
/// The rename makes a new file: it splits a hard link, gives the file the
/// saver's owner and group on Unix and the access list its directory
/// passes on (a default POSIX ACL on Linux, the inherited ACL on Windows),
/// and drops extended attributes. `workspace::buffers` (M4.3) checks the
/// link count, owner and group first. Saves of one file must be serialized
/// by the caller ([`History::checkpoint`]).
///
/// [`History::checkpoint`]: crate::History::checkpoint
///
/// # Errors
///
/// [`TooManyLinks`](TextError::TooManyLinks);
/// [`NotRegularFile`](TextError::NotRegularFile);
/// [`NotWritable`](TextError::NotWritable); [`Io`](TextError::Io) with the
/// [`FileStep`] that failed: at [`FileStep::ReadLink`] also when a target
/// has a root or a drive but is not absolute (`InvalidInput`), or, where
/// Windows' path rules apply, when a link's directory cannot be
/// canonicalized (one that may be passed through but not opened); at
/// [`FileStep::Rename`] with `NotADirectory`, before anything is written,
/// when a missing target ends in a separator, as Unix's rename refuses it
/// (NT's would drop the separator and make a file). Before the rename, an
/// error removes the temporary file (if that fails too, it stays behind)
/// and leaves the target untouched. At [`FileStep::SyncDirectory`] the target already holds
/// the new text.
pub fn save(path: impl AsRef<Path>, text: &Rope, format: Format) -> Result<(), TextError> {
    Temp::create(path.as_ref())?
        .write(|file| {
            let mut out = BufWriter::with_capacity(BLOCK, file);
            encode(text, format, &mut out)?;
            out.flush()
        })?
        .commit()
}

/// A save in progress: the temporary file beside the target. Dropping it
/// before [`commit`](Temp::commit) closes and removes the temporary file and
/// leaves the target as it was.
struct Temp {
    /// The temporary file. Declared first, so that it is closed before
    /// `removal` removes it (Windows cannot remove an open file).
    file: File,
    /// Removes the temporary file unless the rename moved it.
    removal: Removal,
    /// The file the save replaces: the end of the chain of links.
    target: PathBuf,
    /// The target's directory, which holds the temporary file.
    dir: PathBuf,
    /// The target's permissions, if it exists.
    permissions: Option<Permissions>,
}

/// Removes a file when dropped, unless disarmed.
struct Removal {
    path: PathBuf,
    armed: bool,
}

impl Drop for Removal {
    fn drop(&mut self) {
        if self.armed {
            // Best effort: a leftover temporary file only takes space.
            let _ = fs::remove_file(&self.path);
        }
    }
}

impl Temp {
    /// Steps 1 to 3 of [`save`]: follows the links, checks the target, and
    /// creates the temporary file.
    fn create(path: &Path) -> Result<Temp, TextError> {
        let (target, existing) = resolve(path)?;
        let permissions = match existing {
            Some(metadata) => {
                check_regular(&metadata)?;
                check_writable(&target)?;
                Some(metadata.permissions())
            }
            // A missing target that ends in a separator asks for a
            // directory, which a save cannot make. Unix's rename refuses
            // it; NT's drops the separator and makes a file, so it is
            // refused here, before anything is written, as Unix refused it.
            None if ends_in_separator(target.as_os_str()) => {
                return Err(io_error(
                    FileStep::Rename,
                    io::Error::from(ErrorKind::NotADirectory),
                ));
            }
            None => None,
        };
        let dir = match target.parent() {
            Some(dir) if !dir.as_os_str().is_empty() => dir.to_path_buf(),
            _ => PathBuf::from("."),
        };
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        // ADR-0025: the temporary file of an existing file starts private,
        // so that no other process can open it, and read what is written
        // later, before it has the target's permissions. Those lose setuid
        // and setgid, as a write in place would: the new file has the
        // saver's owner and group, which the bits must not carry over to.
        // This is the one platform `cfg` in this crate's library.
        #[cfg(unix)]
        let permissions = {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            if permissions.is_some() {
                options.mode(0o600);
            }
            permissions.map(|mut permissions| {
                permissions.set_mode(permissions.mode() & !0o6000);
                permissions
            })
        };
        let mut last = io::Error::from(ErrorKind::AlreadyExists);
        for _ in 0..TEMP_ATTEMPTS {
            let path = dir.join(temp_name());
            match options.open(&path) {
                Ok(file) => {
                    return Ok(Temp {
                        file,
                        removal: Removal { path, armed: true },
                        target,
                        dir,
                        permissions,
                    });
                }
                Err(error) if error.kind() == ErrorKind::AlreadyExists => last = error,
                Err(error) => return Err(io_error(FileStep::CreateTemp, error)),
            }
        }
        Err(io_error(FileStep::CreateTemp, last))
    }

    /// Step 4 of [`save`]: writes the temporary file with `write`, gives it
    /// the target's permissions (less setuid and setgid on Unix), and syncs
    /// it.
    fn write(mut self, write: impl FnOnce(&mut File) -> io::Result<()>) -> Result<Temp, TextError> {
        write(&mut self.file).map_err(|error| io_error(FileStep::Write, error))?;
        if let Some(permissions) = &self.permissions {
            self.file
                .set_permissions(permissions.clone())
                .map_err(|error| io_error(FileStep::SetPermissions, error))?;
        }
        self.file
            .sync_all()
            .map_err(|error| io_error(FileStep::Sync, error))?;
        Ok(self)
    }

    /// Steps 5 and 6 of [`save`]: closes the temporary file, renames it over
    /// the target, and syncs the directory.
    fn commit(self) -> Result<(), TextError> {
        let Temp {
            file,
            mut removal,
            target,
            dir,
            ..
        } = self;
        // Closed before the rename, which Windows needs.
        drop(file);
        fs::rename(&removal.path, &target).map_err(|error| io_error(FileStep::Rename, error))?;
        removal.armed = false;
        sync_dir(&dir)
    }

    /// A crash after [`write`](Temp::write): closes the temporary file and
    /// leaves it behind, and the target as it was.
    #[cfg(test)]
    fn abandon(self) -> PathBuf {
        let Temp { mut removal, .. } = self;
        removal.armed = false;
        std::mem::take(&mut removal.path)
    }
}

/// Follows the symbolic links at `path` to the path of the file they name,
/// with its metadata if it exists. Each link is followed by [`follow`]: on
/// Unix from the link's parent as written, and where Windows' path rules
/// apply from the canonical path of its directory (ADR-0034).
fn resolve(path: &Path) -> Result<(PathBuf, Option<Metadata>), TextError> {
    let mut current = path.to_path_buf();
    let mut followed = 0;
    loop {
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok((current, None)),
            Err(error) => return Err(io_error(FileStep::Metadata, error)),
        };
        if !metadata.file_type().is_symlink() {
            return Ok((current, Some(metadata)));
        }
        if followed == MAX_LINKS {
            return Err(TextError::TooManyLinks { max: MAX_LINKS });
        }
        followed += 1;
        let link = fs::read_link(&current).map_err(|error| io_error(FileStep::ReadLink, error))?;
        current = follow(&current, link)?;
    }
}

/// The path that `link`, read from the symbolic link at `at`, names
/// (ADR-0034).
///
/// - An absolute target is used as it is. A target with a root or a drive
///   but not the rest of an absolute path (`\x`, `c:x`, `c:`, which only
///   Windows parses) is refused, as `core::path::resolve` refuses it.
/// - Where Unix path rules apply, a relative target is joined onto the
///   link's directory as given, and the kernel resolves the joined path
///   when the save uses it, after any directory links before it and
///   through `/proc` magic links and mounts as it opens them.
///   Canonicalizing the directory instead would read a magic link's target
///   as text (`realpath(3)`) and could name another directory: a save
///   through `/proc/<pid>/root/…` or `/proc/self/fd/N/…`, or below a
///   directory a mount covers, wrote another file that way. (That shows
///   only with a mount namespace, as under `unshare -Urm`, so no test here
///   can.)
/// - Where Windows' path rules apply, it is joined onto the link's own
///   directory as Windows reaches it: the whole path to the link read as
///   Win32 reads it (`std::path::absolute`), in its verbatim form, so that
///   its parent is not read again on its own (`C:\w\space ` alone would
///   open `C:\w\space`), then canonicalized. `std`'s `push` onto that
///   verbatim directory applies the target's `.` and `..`, as NT applies
///   them after the directory links before it. Joining onto the path as
///   given would let Win32 apply `..` to its text, and write outside the
///   directory the link is in. A final separator on the target is kept,
///   as the join keeps it on Unix, so that [`save`] refuses a missing
///   target that asks for a directory.
fn follow(at: &Path, link: PathBuf) -> Result<PathBuf, TextError> {
    if link.is_absolute() {
        return Ok(link);
    }
    // A root or a drive without the rest of an absolute path means
    // something that depends on the current drive, so it is refused. The
    // message does not name the target.
    if link.has_root() || matches!(link.components().next(), Some(Component::Prefix(_))) {
        return Err(io_error(
            FileStep::ReadLink,
            io::Error::new(
                ErrorKind::InvalidInput,
                "the link's target has a root or a drive but is not absolute",
            ),
        ));
    }
    if !windows_rules() {
        // Unix path rules: the lexical join, exactly as before ADR-0034.
        return Ok(match at.parent() {
            Some(dir) => dir.join(link),
            None => link,
        });
    }
    let whole = std::path::absolute(at).map_err(|error| io_error(FileStep::ReadLink, error))?;
    let whole = verbatim(whole);
    let dir = whole.parent().unwrap_or(&whole);
    let dir = fs::canonicalize(dir).map_err(|error| io_error(FileStep::ReadLink, error))?;
    let mut target = dir.join(&link).into_os_string();
    if ends_in_separator(link.as_os_str()) {
        target.push(MAIN_SEPARATOR_STR);
    }
    Ok(PathBuf::from(target))
}

/// Whether Windows' path rules apply: whether `std` parses a drive prefix,
/// which only its Windows path parsing does. There every absolute path
/// (`std::path::absolute`'s answer) has a prefix, and on Unix none does.
/// Asked of a constant, so that on Unix a path is never made absolute,
/// which needs the current directory's name.
fn windows_rules() -> bool {
    matches!(
        Path::new(r"C:\").components().next(),
        Some(Component::Prefix(_))
    )
}

/// Whether `text` ends in a path separator here: `/`, and on Windows `\`
/// too.
fn ends_in_separator(text: &OsStr) -> bool {
    text.as_encoded_bytes()
        .last()
        .is_some_and(|&byte| std::path::is_separator(char::from(byte)))
}

/// `path`, which is absolute, with a plain drive, share or device prefix
/// turned into Windows' verbatim one (`\\?\C:\…`, `\\?\UNC\server\share\…`,
/// `\\?\` for `\\.\`), as `core::path` probes a path: then no part of it is
/// read again by Win32's rules on its own. Built from the prefix's parts and
/// the rest of the path as it is. Any other path comes back unchanged.
fn verbatim(path: PathBuf) -> PathBuf {
    let spelled = {
        let mut parts = path.components();
        match parts.next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::Disk(_) => {
                    let mut spelled = OsString::from(r"\\?\");
                    spelled.push(path.as_os_str());
                    Some(spelled)
                }
                Prefix::UNC(server, share) => {
                    let mut spelled = OsString::from(r"\\?\UNC\");
                    spelled.push(server);
                    spelled.push(r"\");
                    spelled.push(share);
                    spelled.push(parts.as_path());
                    Some(spelled)
                }
                Prefix::DeviceNS(device) => {
                    let mut spelled = OsString::from(r"\\?\");
                    spelled.push(device);
                    spelled.push(parts.as_path());
                    Some(spelled)
                }
                Prefix::Verbatim(_) | Prefix::VerbatimUNC(..) | Prefix::VerbatimDisk(_) => None,
            },
            _ => None,
        }
    };
    spelled.map_or(path, PathBuf::from)
}

/// Refuses anything but a regular file.
fn check_regular(metadata: &Metadata) -> Result<(), TextError> {
    if metadata.is_file() {
        Ok(())
    } else {
        Err(TextError::NotRegularFile {
            directory: metadata.is_dir(),
        })
    }
}

/// Refuses a regular file the user may not write, by opening it for
/// writing without creating or truncating it.
fn check_writable(target: &Path) -> Result<(), TextError> {
    match OpenOptions::new().write(true).open(target) {
        Ok(_) => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::PermissionDenied | ErrorKind::ReadOnlyFilesystem
            ) =>
        {
            Err(TextError::NotWritable { error })
        }
        Err(error) => Err(io_error(FileStep::CheckWritable, error)),
    }
}

/// Syncs `dir`, skipping a directory that cannot be opened or a file system
/// that cannot sync one.
fn sync_dir(dir: &Path) -> Result<(), TextError> {
    match File::open(dir) {
        Ok(handle) => dir_synced(handle.sync_all()),
        Err(_) => Ok(()),
    }
}

/// The result of syncing a directory. Some file systems (VirtualBox shared
/// folders, some network ones) refuse to sync a directory with `EINVAL` or
/// `ENOTSUP`; the rename is then as durable as they make it, and the save
/// succeeds. Any other error is the save's.
fn dir_synced(result: io::Result<()>) -> Result<(), TextError> {
    match result {
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::InvalidInput | ErrorKind::Unsupported
            ) =>
        {
            Ok(())
        }
        result => result.map_err(|error| io_error(FileStep::SyncDirectory, error)),
    }
}

/// A fresh name for a temporary file, of fixed length whatever the target's
/// name: the prefix, the process id, a count and the time, in hex.
fn temp_name() -> String {
    let count = TEMPS.fetch_add(1, Ordering::Relaxed);
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.subsec_nanos() ^ since.as_secs() as u32);
    format!(
        "{TEMP_PREFIX}{:08x}-{count:08x}-{time:08x}.tmp",
        std::process::id()
    )
}

/// The break `text` ends with, if any.
fn trailing_break(text: &Rope) -> Option<LineBreak> {
    // Chunks never split a CR LF, so the last one holds the whole break.
    match text.chunks().next_back()?.as_bytes() {
        [.., b'\r', b'\n'] => Some(LineBreak::CrLf),
        [.., b'\n'] => Some(LineBreak::Lf),
        [.., b'\r'] => Some(LineBreak::Cr),
        _ => None,
    }
}

fn io_error(step: FileStep, error: io::Error) -> TextError {
    TextError::Io { step, error }
}

#[cfg(test)]
mod tests {
    use std::ops::Deref;

    use tidepool_core::rng::Pcg32;
    use tidepool_core::testing::check;

    use super::*;

    /// A test's scratch directory, removed when the test passes; a failing
    /// test leaves it behind. Unit tests have no `CARGO_TARGET_TMPDIR`, so
    /// it is under the system's temporary directory, created there anew
    /// with a name no other run has: one that already exists, perhaps
    /// another user's, fails the test.
    struct Scratch(PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            if !std::thread::panicking() {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
    }

    impl Deref for Scratch {
        type Target = Path;

        fn deref(&self) -> &Path {
            &self.0
        }
    }

    fn scratch_dir(test: &str) -> Scratch {
        let since = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the clock is after 1970");
        let dir = std::env::temp_dir().join(format!(
            "tidepool-text-file-{test}-{}-{}",
            std::process::id(),
            since.as_nanos()
        ));
        fs::create_dir(&dir).expect("create a fresh test directory");
        Scratch(dir)
    }

    /// The names in `dir`, sorted.
    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .expect("list the directory")
            .map(|entry| {
                let entry = entry.expect("read an entry");
                entry.file_name().to_string_lossy().into_owned()
            })
            .collect();
        names.sort();
        names
    }

    /// Writes like the inner writer for `left` bytes, then fails.
    struct FailAfter<W> {
        inner: W,
        left: usize,
    }

    impl<W: Write> Write for FailAfter<W> {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.left == 0 {
                return Err(io::Error::other("injected write failure"));
            }
            let n = buf.len().min(self.left);
            let written = self.inner.write(&buf[..n])?;
            self.left -= written;
            Ok(written)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.inner.flush()
        }
    }

    fn write_text(text: &Rope, format: Format) -> impl FnOnce(&mut File) -> io::Result<()> {
        move |file| encode(text, format, file)
    }

    #[test]
    fn a_crash_before_the_rename_leaves_the_original() {
        let dir = scratch_dir("crash");
        let path = dir.join("notes.txt");
        fs::write(&path, b"old\r\ntext").expect("write the original");
        let new = Rope::from("new text\n");

        let temp = Temp::create(&path)
            .expect("create")
            .write(write_text(&new, Format::UTF8))
            .expect("write and sync");
        let left = temp.abandon();

        assert_eq!(fs::read(&path).expect("read"), b"old\r\ntext");
        assert_eq!(fs::read(&left).expect("read the temp file"), b"new text\n");
        let name = left.file_name().expect("a name").to_string_lossy();
        assert!(name.starts_with(TEMP_PREFIX), "{name}");
        assert_eq!(names(&dir).len(), 2);

        // The next save still works, and leaves the crash's file alone.
        save(&path, &Rope::from("newer"), Format::UTF8).expect("save");
        assert_eq!(fs::read(&path).expect("read"), b"newer");
        assert!(left.exists());
        assert_eq!(names(&dir).len(), 2);
    }

    #[test]
    fn a_failed_write_removes_the_temp_file_and_keeps_the_original() {
        let dir = scratch_dir("failed-write");
        let path = dir.join("notes.txt");
        fs::write(&path, b"original").expect("write the original");
        let text = Rope::from("x".repeat(100_000).as_str());

        for (left, format) in [
            (0, Format::UTF8),
            (1, Format::new(crate::Encoding::Utf16Le, true)),
            (70_000, Format::UTF8),
        ] {
            let result = Temp::create(&path).expect("create").write(|file| {
                let mut out = FailAfter { inner: file, left };
                encode(&text, format, &mut out)
            });
            let Err(error) = result else {
                panic!("the write must fail after {left} bytes");
            };
            assert!(
                matches!(
                    error,
                    TextError::Io {
                        step: FileStep::Write,
                        ..
                    }
                ),
                "{error:?}"
            );
            assert_eq!(fs::read(&path).expect("read"), b"original");
            assert_eq!(names(&dir), ["notes.txt"]);
        }
    }

    #[test]
    fn a_failed_rename_removes_the_temp_file() {
        let dir = scratch_dir("failed-rename");
        let path = dir.join("notes.txt");
        fs::write(&path, b"original").expect("write the original");
        let mut temp = Temp::create(&path)
            .expect("create")
            .write(write_text(&Rope::from("new"), Format::UTF8))
            .expect("write");
        // Renaming over a directory that is not empty fails everywhere.
        let blocker = dir.join("blocker");
        fs::create_dir_all(blocker.join("inside")).expect("create the blocker");
        temp.target = blocker;
        let error = temp.commit().expect_err("the rename must fail");
        assert!(
            matches!(
                error,
                TextError::Io {
                    step: FileStep::Rename,
                    ..
                }
            ),
            "{error:?}"
        );
        assert_eq!(names(&dir), ["blocker", "notes.txt"]);
        assert_eq!(fs::read(&path).expect("read"), b"original");
    }

    #[test]
    fn a_directory_sync_the_file_system_refuses_is_skipped() {
        assert!(dir_synced(Ok(())).is_ok());
        for kind in [ErrorKind::InvalidInput, ErrorKind::Unsupported] {
            assert!(dir_synced(Err(kind.into())).is_ok(), "{kind:?}");
        }
        let error = dir_synced(Err(io::Error::other("EIO"))).expect_err("a real error");
        assert!(matches!(
            error,
            TextError::Io {
                step: FileStep::SyncDirectory,
                ..
            }
        ));
    }

    #[test]
    fn temp_names_are_fixed_length_and_fresh() {
        let a = temp_name();
        let b = temp_name();
        assert_ne!(a, b);
        assert_eq!(a.len(), b.len());
        assert_eq!(
            a.len(),
            TEMP_PREFIX.len() + 8 + 1 + 8 + 1 + 8 + ".tmp".len()
        );
        assert!(a.starts_with(TEMP_PREFIX) && a.ends_with(".tmp"), "{a}");
    }

    /// ADR-0025: the temporary file of an existing file is 0600 until it
    /// has the target's mode.
    #[cfg(unix)]
    #[test]
    fn the_temp_file_starts_private() {
        use std::os::unix::fs::PermissionsExt;

        let dir = scratch_dir("private");
        let path = dir.join("shared.txt");
        fs::write(&path, b"old").expect("write the original");
        fs::set_permissions(&path, Permissions::from_mode(0o664)).expect("chmod");
        let mode = |path: &Path| fs::metadata(path).expect("stat").permissions().mode() & 0o7777;

        let temp = Temp::create(&path).expect("create");
        assert_eq!(mode(&temp.removal.path), 0o600);
        let temp = temp
            .write(write_text(&Rope::from("new"), Format::UTF8))
            .expect("write");
        assert_eq!(mode(&temp.removal.path), 0o664);
        temp.commit().expect("commit");
        assert_eq!(mode(&path), 0o664);
        assert_eq!(fs::read(&path).expect("read"), b"new");
    }

    /// A text; its LF, CR LF and CR counts; whether they are mixed; the
    /// dominant break; the trailing break.
    type EndingsCase = (&'static str, [usize; 3], bool, LineBreak, Option<LineBreak>);

    #[test]
    fn line_endings_count_each_kind() {
        let cases: [EndingsCase; 9] = [
            ("", [0, 0, 0], false, LineBreak::Lf, None),
            ("one line", [0, 0, 0], false, LineBreak::Lf, None),
            (
                "a\nb\n",
                [2, 0, 0],
                false,
                LineBreak::Lf,
                Some(LineBreak::Lf),
            ),
            (
                "a\r\nb\r\n",
                [0, 2, 0],
                false,
                LineBreak::CrLf,
                Some(LineBreak::CrLf),
            ),
            (
                "a\rb\r",
                [0, 0, 2],
                false,
                LineBreak::Cr,
                Some(LineBreak::Cr),
            ),
            ("a\nb\r\nc\rd", [1, 1, 1], true, LineBreak::Lf, None),
            (
                "a\r\nb\r\nc\n",
                [1, 2, 0],
                true,
                LineBreak::CrLf,
                Some(LineBreak::Lf),
            ),
            (
                "\r\r\n\r\n\r",
                [0, 2, 2],
                true,
                LineBreak::CrLf,
                Some(LineBreak::Cr),
            ),
            (
                "\r\r\r\n\n",
                [1, 1, 2],
                true,
                LineBreak::Cr,
                Some(LineBreak::Lf),
            ),
        ];
        for (text, [lf, crlf, cr], mixed, dominant, trailing) in cases {
            let rope = Rope::from(text);
            let endings = LineEndings::of(&rope);
            assert_eq!(endings, LineEndings { lf, crlf, cr }, "{text:?}");
            assert_eq!(endings.total(), rope.len_lines() - 1, "{text:?}");
            assert_eq!(endings.is_mixed(), mixed, "{text:?}");
            assert_eq!(endings.dominant(), dominant, "{text:?}");
            assert_eq!(trailing_break(&rope), trailing, "{text:?}");
        }
    }

    #[test]
    fn ties_go_to_lf_then_crlf() {
        let dominant = |lf, crlf, cr| LineEndings { lf, crlf, cr }.dominant();
        assert_eq!(dominant(0, 0, 0), LineBreak::Lf);
        assert_eq!(dominant(2, 2, 0), LineBreak::Lf);
        assert_eq!(dominant(2, 0, 2), LineBreak::Lf);
        assert_eq!(dominant(2, 2, 2), LineBreak::Lf);
        assert_eq!(dominant(0, 2, 2), LineBreak::CrLf);
        assert_eq!(dominant(1, 3, 2), LineBreak::CrLf);
        assert_eq!(dominant(1, 2, 3), LineBreak::Cr);
    }

    /// A text of `len` units, mostly breaks, so that CR LFs meet chunk seams.
    fn breaky_text(rng: &mut Pcg32, len: u32) -> String {
        const UNITS: [&str; 6] = ["\n", "\r", "\r\n", "a", "é", "\u{2028}"];
        (0..len)
            .map(|_| *rng.choose(&UNITS).unwrap_or(&"a"))
            .collect()
    }

    #[test]
    fn line_endings_match_a_byte_by_byte_count() {
        check(
            "line endings match a byte-by-byte count",
            2_000,
            |rng| {
                let len = rng.range(0..3_000u32);
                breaky_text(rng, len)
            },
            |text| {
                let (mut lf, mut crlf, mut cr) = (0, 0, 0);
                let bytes = text.as_bytes();
                for (i, &byte) in bytes.iter().enumerate() {
                    match byte {
                        b'\n' if i > 0 && bytes[i - 1] == b'\r' => crlf += 1,
                        b'\n' => lf += 1,
                        b'\r' if bytes.get(i + 1) == Some(&b'\n') => {}
                        b'\r' => cr += 1,
                        _ => {}
                    }
                }
                let rope = Rope::from(text.as_str());
                let got = LineEndings::of(&rope);
                let want = LineEndings { lf, crlf, cr };
                if got != want {
                    return Err(format!("got {got:?}, want {want:?}"));
                }
                let trailing = [LineBreak::CrLf, LineBreak::Lf, LineBreak::Cr]
                    .into_iter()
                    .find(|kind| text.ends_with(kind.as_str()));
                if trailing_break(&rope) != trailing {
                    return Err(format!("trailing break {:?}", trailing_break(&rope)));
                }
                Ok(())
            },
        );
    }

    /// ADR-0034: a target with a root or a drive but not the rest of an
    /// absolute path is refused where Windows' path rules apply. Tested on
    /// `follow` itself, since no link need hold one: Windows stores a
    /// drive-relative target as an absolute path when it makes the link.
    /// Elsewhere such a target is an ordinary name, joined onto the link's
    /// parent as written.
    #[test]
    fn a_target_with_a_root_or_a_drive_alone_is_refused_where_windows_reads_it() {
        let at = Path::new("dir").join("link");
        for target in [r"\x", "c:x", "c:"] {
            let followed = follow(&at, PathBuf::from(target));
            if windows_rules() {
                let error = followed.expect_err("refused");
                assert!(
                    matches!(&error, TextError::Io { step: FileStep::ReadLink, error }
                        if error.kind() == ErrorKind::InvalidInput),
                    "{target}: {error:?}"
                );
                assert!(!error.to_string().contains(target), "{error}");
            } else {
                let joined = followed.expect("an ordinary name here");
                assert_eq!(joined, Path::new("dir").join(target), "{target}");
            }
        }
    }

    /// ADR-0034: where Unix path rules apply, a relative target is joined
    /// onto the link's parent as written, with no file system call, and the
    /// kernel resolves the result when the save uses it.
    #[test]
    fn unix_rules_keep_the_lexical_join() {
        if windows_rules() {
            return;
        }
        for (at, target, joined) in [
            ("a/b/link", "../x", "a/b/../x"),
            ("/no/such/dir/link", "x", "/no/such/dir/x"),
            ("link", "x", "x"),
            ("a/link", "x/", "a/x/"),
        ] {
            let followed = follow(Path::new(at), PathBuf::from(target)).expect("joined");
            assert_eq!(followed.as_os_str(), joined, "{at} -> {target}");
        }
    }

    /// A target that asks for a directory keeps its final separator, as
    /// the plain join keeps it on Unix; `std`'s `push` onto the verbatim
    /// directory Windows' rules give would drop it.
    #[test]
    fn a_final_separator_on_a_target_is_kept() {
        let dir = scratch_dir("final-separator");
        let target = PathBuf::from(format!("x{MAIN_SEPARATOR_STR}"));
        let followed = follow(&dir.join("link"), target).expect("joined");
        assert!(ends_in_separator(followed.as_os_str()), "{followed:?}");
        assert_eq!(followed.file_name(), Some(OsStr::new("x")));
    }

    /// Where Windows' path rules apply, the path to a link is probed in its
    /// verbatim form, so that Win32 does not read its parent again on its
    /// own (`C:\w\space ` alone opens `C:\w\space`). Elsewhere it is kept.
    #[test]
    fn a_plain_windows_prefix_becomes_verbatim() {
        for (plain, spelled) in [
            (r"C:\w\space \l", r"\\?\C:\w\space \l"),
            (r"\\srv\sh\d\l", r"\\?\UNC\srv\sh\d\l"),
            (r"\\.\C:\w\l", r"\\?\C:\w\l"),
        ] {
            let want = if windows_rules() { spelled } else { plain };
            let got = verbatim(PathBuf::from(plain));
            assert_eq!(got.as_os_str(), want, "{plain}");
        }
        for kept in [r"\\?\C:\w\l", r"\\?\UNC\srv\sh\l", "/home/me/l"] {
            assert_eq!(verbatim(PathBuf::from(kept)).as_os_str(), kept);
        }
    }
}
