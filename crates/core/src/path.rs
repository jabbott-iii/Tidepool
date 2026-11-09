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

//! Paths: a display that hides none of a path's bytes, the path as text,
//! one canonical spelling for a file, and containment.
//!
//! New in Tidepool; not ported. The display follows ADR-0032 and
//! ADR-0033, the canonical form and [`resolve`] and [`under`] ADR-0033.
//!
//! - [`display`] shows a path to people and logs. A path is opened,
//!   compared and passed to a child process by its [`Path`], never by its
//!   display.
//! - [`to_utf8`] gives a path as text, for what holds only text: TOML,
//!   JSON, URIs, the UI.
//! - [`canonicalize`] gives the one absolute spelling of a path that
//!   exists, and [`resolve`] the spelling a path will have once its missing
//!   names are made.
//! - [`under`] gives the part of a canonical path inside a canonical root.
//!
//! # The canonical form on each OS
//!
//! - **Linux:** `realpath(3)`'s. Every symbolic link is resolved, `..` is
//!   applied physically (`link/..` is the parent of the link's target),
//!   duplicate and trailing separators are dropped, and names keep the
//!   bytes they were given, in a directory that folds case too.
//! - **macOS:** `realpath(3)`'s, as on Linux, except that each name that
//!   exists comes back as the file system stores it: its case, and on HFS+
//!   its decomposed Unicode form. `/tmp`, `/var` and `/etc` become
//!   `/private/…`. Its `realpath(3)` refuses a path longer than 1,024
//!   bytes, so both [`canonicalize`] and [`resolve`] fail for one with the
//!   OS's error, even where the part that exists is short.
//! - **Windows:** `C:\…`, or `\\server\share\…` for a share; the case is
//!   as stored, 8.3 names are made long, a mapped drive becomes its share
//!   and a SUBST drive its target. `..` is applied to the text before any
//!   link is followed, as Win32 applies it. The OS gives the verbatim form
//!   `\\?\C:\…`, and its `\\?\` is dropped exactly when Win32 reads the
//!   plain form back unchanged, the test `std` makes before it passes a
//!   directory to a child. There is no length rule. So the verbatim form
//!   stays for any path that Win32 would read differently.
//!
//! Win32 rewrites a path's last name (a trailing dot or space, or a device
//! name such as `nul.txt` where this Windows version reserves it), and it
//! drops one final dot from each name inside the path too, but it keeps a
//! space inside the path. So `\\?\C:\w\space ` stays verbatim while
//! `C:\w\space \a.txt` is plain, and `\\?\C:\w\dir.\a.txt` stays verbatim
//! though its last name is ordinary. Forms can therefore alternate down a
//! tree. Take an ancestor of a canonical path by canonicalizing it, never
//! with [`Path::parent`], whose plain answer Win32 may read as another
//! directory. [`under`] treats a plain and a verbatim prefix of one volume
//! as the same; `==` and [`Path::starts_with`] do not, and `starts_with` is
//! lexical besides (`/w/../etc` starts with `/w`).
//!
//! One file can still have two canonical paths: through hard links, bind
//! mounts, a directory that folds case on Linux, or a missing tail typed in
//! another case. Telling those apart needs the file's identity, which
//! belongs to `tidepool-platform`.
//!
//! # Blocking and races
//!
//! [`canonicalize`] and [`resolve`] read the file system, which on NFS, SMB
//! or FUSE can take any time. Call them on a worker, never on the main
//! thread (`AGENTS.md` rule 6). Their answers are checks, not a sandbox: a
//! link changed after the call escapes them. A name made, removed or
//! replaced while [`resolve`] walks is taken as the walk finds it, and one
//! removed or replaced under it can still give an error, such as
//! `NotFound`, or `InvalidInput` for a link that is no longer one. On Windows,
//! canonicalizing through a link to `\\host\share` makes Windows
//! authenticate to that host.
//!
//! This module has no platform `cfg`: Windows' prefixes are told apart by
//! [`Component::Prefix`], which only Windows' path parsing yields.

use std::ffi::{OsStr, OsString};
use std::fmt::{self, Write as _};
use std::fs;
use std::io;
use std::path::{Component, MAIN_SEPARATOR_STR, Path, PathBuf, Prefix};

/// The most symbolic links [`resolve`]'s walk follows itself for one path:
/// Linux's `MAXSYMLINKS`, and the most `text::file`'s save follows
/// (ADR-0025).
const MAX_LINKS: usize = 40;

/// Shows `path` to people and in logs, hiding none of its bytes.
///
/// - Valid UTF-8 is written as it is, a backslash and quotes included.
/// - Each byte that is not part of valid UTF-8 is written as `\xNN`, with
///   uppercase hex: `/srv/caf\xE9.txt`. On Windows an unpaired surrogate
///   shows as the three bytes `std` holds for it, `\xED\xA0\x80` for
///   U+D800, the same on every OS.
/// - A control character (U+0000 to U+001F, U+007F to U+009F, tab and
///   newline included), a bidirectional control (U+061C, U+200E, U+200F,
///   U+202A to U+202E, U+2066 to U+2069) and the line and paragraph
///   separators (U+2028, U+2029) are written as [`crate::log`] writes an
///   escape, `\u{1b}`. So a display is one line, and a name can neither
///   drive a terminal nor reorder how the rest of the line is drawn.
///
/// A UTF-8 path with none of those characters displays unchanged. The
/// display is for reading, not for parsing back: a name holding the four
/// characters `\xE9` shows the same as one holding the byte 0xE9. `Debug`
/// is the same text in double quotes. Neither allocates, and both ignore
/// width, fill and precision.
///
/// ```
/// use tidepool_core::path;
///
/// assert_eq!(path::display("src/main.rs").to_string(), "src/main.rs");
/// assert_eq!(path::display("a\nb\u{202e}").to_string(), r"a\u{a}b\u{202e}");
/// assert_eq!(format!("{:?}", path::display(r"C:\x")), r#""C:\x""#);
/// ```
#[must_use]
pub fn display<P: AsRef<Path> + ?Sized>(path: &P) -> PathDisplay<'_> {
    PathDisplay {
        bytes: path.as_ref().as_os_str().as_encoded_bytes(),
    }
}

/// A path shown to people and in logs, from [`display`].
#[derive(Clone, Copy)]
pub struct PathDisplay<'a> {
    /// The path's bytes ([`OsStr::as_encoded_bytes`]): on Unix the name's
    /// own, on Windows WTF-8.
    bytes: &'a [u8],
}

impl fmt::Display for PathDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_escaped(self.bytes, f)
    }
}

impl fmt::Debug for PathDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_char('"')?;
        write_escaped(self.bytes, f)?;
        f.write_char('"')
    }
}

/// Writes `bytes` as [`display`] shows them, in runs: no allocation, and
/// no width or padding, which writing in pieces would ignore anyway.
fn write_escaped(bytes: &[u8], out: &mut impl fmt::Write) -> fmt::Result {
    for chunk in bytes.utf8_chunks() {
        // Each piece ends just after a character to escape, except perhaps
        // the last, which may have none.
        for piece in chunk.valid().split_inclusive(is_escaped) {
            let mut chars = piece.chars();
            match chars.next_back() {
                Some(c) if is_escaped(c) => {
                    out.write_str(chars.as_str())?;
                    write!(out, "\\u{{{:x}}}", u32::from(c))?;
                }
                _ => out.write_str(piece)?,
            }
        }
        for byte in chunk.invalid() {
            write!(out, "\\x{byte:02X}")?;
        }
    }
    Ok(())
}

/// Whether [`display`] escapes `c`: a control character
/// ([`char::is_control`]: C0, DEL and C1), a bidirectional control, which
/// reorders what follows it on the line, or the line or paragraph
/// separator, which a view may draw as a line break. The set is `xtask`'s
/// (`is_escaped` in `xtask/src/walk.rs`; ADR-0033).
fn is_escaped(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{061c}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
                | '\u{2028}'
                | '\u{2029}'
        )
}

/// The path as text, borrowed, for what holds only text: TOML, JSON, URIs,
/// the UI.
///
/// ```
/// use tidepool_core::path;
///
/// assert_eq!(path::to_utf8("notes/draft.md").ok(), Some("notes/draft.md"));
/// ```
///
/// # Errors
///
/// [`PathError::NotUtf8`] when the path is not valid UTF-8: on Unix a name
/// with other bytes, on Windows one with an unpaired surrogate. Its message
/// names the path by its [`display`].
pub fn to_utf8<P: AsRef<Path> + ?Sized>(path: &P) -> Result<&str, PathError> {
    let path = path.as_ref();
    path.to_str().ok_or_else(|| PathError::NotUtf8 {
        path: path.to_path_buf(),
    })
}

/// The one absolute spelling of `path`, which must exist: every symbolic
/// link, `.` and `..` resolved as the OS resolves them
/// ([`fs::canonicalize`]), in the form the [module documentation](self)
/// gives for each OS. A relative path starts at the process's current
/// directory.
///
/// It blocks on the file system, so call it on a worker.
///
/// # Errors
///
/// - [`PathError::Empty`] for an empty path.
/// - [`PathError::Io`] when the OS cannot resolve it, naming the path as
///   given, with the OS's error as it is: [`io::ErrorKind::NotFound`] for a
///   missing name, [`io::ErrorKind::NotADirectory`] on Unix for a name below
///   a file, the OS's loop error for a link loop,
///   [`io::ErrorKind::PermissionDenied`] for a directory that cannot be
///   searched. On Windows the file itself must open, so a device, a file
///   link to a directory and some virtual file systems are errors too.
pub fn canonicalize(path: impl AsRef<Path>) -> Result<PathBuf, PathError> {
    let given = path.as_ref();
    if given.as_os_str().is_empty() {
        return Err(PathError::Empty);
    }
    fs::canonicalize(given)
        .map(simplify)
        .map_err(|source| PathError::Io {
            path: given.to_path_buf(),
            source,
        })
}

/// [`canonicalize`] for a path whose end need not exist yet: the spelling
/// `path` will have once its missing names are made, directories and then
/// the last name as a file.
///
/// - The OS canonicalizes the whole path first; if it exists, that is the
///   answer.
/// - Otherwise the path is walked from its root one name at a time, each
///   name looked at once. A symbolic link is followed as the OS follows it,
///   from the link's own directory, whether it leads anywhere or not, and
///   its target is walked the same way (from its root if it is absolute).
///   On Windows the `..` in a relative target are applied to its text
///   first, as Windows applies them. An empty target, which macOS and the
///   BSDs allow, names nothing there, and so here.
/// - A name below a missing name is missing too, and a `..` after a missing
///   name cancels it, one `..` for one name, without asking the file
///   system again. A `..` after a directory that exists is its parent.
/// - A path that ends in a separator, or on Unix in `/.`, asks for a
///   directory, as it does of the OS: if it names a file, the answer is
///   [`io::ErrorKind::NotADirectory`], and a missing last name is planned
///   as a directory to make. On Windows so does a path that ends in a
///   separator once Win32 has trimmed a final space or dot (`file.txt\ `).
///   The target of a link that is the path's last name asks the same way.
///
/// So `resolve(p) == canonicalize(p)` for a path that exists, and once the
/// missing names of `p` (and a dangling link's target) are made. Names
/// that do not exist keep the case they were given. Its work is one look
/// at each name of the path and of the link targets it follows, besides
/// the OS's canonicalization of the whole path at the start and of the
/// part that exists at the end.
///
/// ```
/// use tidepool_core::path::{self, PathError};
///
/// fn main() -> Result<(), PathError> {
///     let here = path::canonicalize(".")?;
///     let planned = path::resolve("no-such-directory/../no-such-file.txt")?;
///     assert_eq!(planned, here.join("no-such-file.txt"));
///     Ok(())
/// }
/// ```
///
/// It blocks on the file system, so call it on a worker.
///
/// # Errors
///
/// - [`PathError::Empty`] for an empty path.
/// - [`PathError::Io`], naming the path as given:
///   - with the OS's error when a part that exists cannot be resolved, as
///     for [`canonicalize`];
///   - [`io::ErrorKind::NotADirectory`] on every OS when a name or `..`
///     follows a file, or a path that asks for a directory names a file;
///   - [`io::ErrorKind::NotFound`] when not even its root exists (a drive
///     or share that is not there), or for a link whose target is empty;
///   - [`io::ErrorKind::InvalidInput`] for what the walk will not guess at,
///     which only Windows paths hold: a name that Windows reads as a drive
///     (`c:x`, a stream of the file `c`), a `.` that a verbatim path keeps,
///     or a link target that is neither relative nor absolute (`\x`,
///     `c:x`).
/// - [`PathError::TooManyLinks`] when `resolve` itself has followed 40
///   links and meets another. It counts every link it follows across the
///   whole call: when the whole path does not exist, that is every link on
///   the path and in the targets it follows, as the OS counts them. So a
///   chain or a loop through names that do not exist yet
///   (`a -> missing/../b`) ends here on every OS. Once a path is made, the
///   OS's own limit decides: macOS refuses more than 32 links, Windows
///   follows up to 63.
pub fn resolve(path: impl AsRef<Path>) -> Result<PathBuf, PathError> {
    let given = path.as_ref();
    if given.as_os_str().is_empty() {
        return Err(PathError::Empty);
    }
    let fail = |source| PathError::Io {
        path: given.to_path_buf(),
        source,
    };
    // On Unix `absolute` keeps `..`, which the walk applies physically. On
    // Windows it is `GetFullPathNameW`, which applies `..`, a final dot or
    // space and device names as `CreateFileW` will.
    let start = std::path::absolute(given).map_err(fail)?;
    // Under Windows' rules, also what Win32 reads: trimming a final space or
    // dot can leave a final separator (`file.txt\ ` is `file.txt\`).
    let windows = windows_rules(&start);
    let mut wants_directory = asks_for_directory(given.as_os_str(), windows)
        || (windows && asks_for_directory(start.as_os_str(), true));
    // The steps still to take, the next one last.
    let mut queue = Vec::new();
    let mut at = enter(start, true, &mut queue).map_err(fail)?;
    // The names below `at` that do not exist, in order.
    let mut missing: Vec<OsString> = Vec::new();
    let mut hops = 0;
    while let Some(step) = queue.pop() {
        match step {
            Step::Name(name) if !missing.is_empty() => missing.push(name),
            Step::Up if !missing.is_empty() => {
                missing.pop();
            }
            Step::Up => match at.require_directory() {
                // A directory removed since the walk reached it is a
                // missing name now, which a `..` cancels just the same.
                Err(error) if error.kind() != io::ErrorKind::NotFound => {
                    return Err(fail(error));
                }
                _ => at.climb(),
            },
            Step::Name(name) => {
                at.require_directory().map_err(fail)?;
                let below = joined(&at.path, &name);
                let metadata = match fs::symlink_metadata(&below) {
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        missing.push(name);
                        continue;
                    }
                    found => found.map_err(fail)?,
                };
                if !metadata.file_type().is_symlink() {
                    at.descend(below, metadata.is_dir());
                    continue;
                }
                hops += 1;
                if hops > MAX_LINKS {
                    return Err(PathError::TooManyLinks {
                        path: given.to_path_buf(),
                    });
                }
                let target = fs::read_link(&below).map_err(fail)?;
                let windows = windows_rules(Path::new(&at.path));
                if queue.is_empty() {
                    wants_directory |= asks_for_directory(target.as_os_str(), windows);
                }
                if target.is_absolute() {
                    let target = std::path::absolute(&target).map_err(fail)?;
                    at = enter(target, false, &mut queue).map_err(fail)?;
                } else {
                    queue_target(&target, windows, &mut queue).map_err(fail)?;
                }
            }
        }
    }
    if wants_directory && missing.is_empty() {
        at.require_directory().map_err(fail)?;
    }
    let mut planned = at.into_canonical().map_err(fail)?;
    for name in &missing {
        push_name(&mut planned, name);
    }
    Ok(simplify(PathBuf::from(planned)))
}

/// One step of [`resolve`]'s walk, owned, so that a link's target can be
/// queued in front of the steps left.
#[derive(Debug, PartialEq, Eq)]
enum Step {
    /// A name.
    Name(OsString),
    /// `..`.
    Up,
}

/// Where [`resolve`]'s walk stands: a path that exists.
struct At {
    /// The path: canonical, but for any names appended since it was, which
    /// exist and are not links.
    path: OsString,
    /// Whether no name has been appended since `path` was canonical.
    canonical: bool,
    /// Whether `path` is a directory, once that is known.
    directory: Option<bool>,
}

impl At {
    /// Fails with [`io::ErrorKind::NotADirectory`] unless `path` is a
    /// directory, which a name or `..` after it needs.
    fn require_directory(&mut self) -> io::Result<()> {
        let directory = match self.directory {
            Some(known) => known,
            None => fs::metadata(&self.path)?.is_dir(),
        };
        self.directory = Some(directory);
        if directory {
            Ok(())
        } else {
            Err(io::Error::from(io::ErrorKind::NotADirectory))
        }
    }

    /// Moves to `below`, a name in `path` that exists and is not a link.
    fn descend(&mut self, below: OsString, directory: bool) {
        self.path = below;
        self.canonical = false;
        self.directory = Some(directory);
    }

    /// Moves to the parent of `path`, a directory. Its text gives the
    /// physical parent, since no name in `path` is a link; the root is its
    /// own parent.
    fn climb(&mut self) {
        if let Some(parent) = Path::new(&self.path).parent() {
            self.path = parent.as_os_str().to_owned();
        }
        self.directory = Some(true);
    }

    /// `path`, canonicalized if a name was appended since it was canonical,
    /// so that a name typed in another case comes back as the file system
    /// gives it.
    fn into_canonical(self) -> io::Result<OsString> {
        if self.canonical {
            Ok(self.path)
        } else {
            fs::canonicalize(&self.path).map(PathBuf::into_os_string)
        }
    }
}

/// Starts [`resolve`]'s walk at `path`, which is absolute.
///
/// With `whole_first`, the OS canonicalizes the whole path first: if it
/// exists, the walk starts at its canonical path with nothing queued.
/// Otherwise, and always for a link's target, the walk starts at the
/// canonical path of its root, with every name after the root queued in
/// front of `queue`. The walk then looks at each name once, and follows
/// and counts each link itself.
///
/// It never canonicalizes a part of the path in the middle. That would
/// repeat the OS's work on every leading part, and on Windows
/// `fs::canonicalize` opens the part it is given, which a directory that
/// may only be passed through refuses, while `fs::symlink_metadata` falls
/// back to the directory's listing.
///
/// # Errors
///
/// `InvalidInput` for a name that does not read back as one name
/// ([`is_one_name`]), or for a `.`, which only a verbatim Windows path
/// keeps; otherwise the OS's error for the whole path, if it is not
/// `NotFound`, or for its root.
fn enter(path: PathBuf, whole_first: bool, queue: &mut Vec<Step>) -> io::Result<At> {
    let path = verbatim(path);
    for part in path.components() {
        match part {
            Component::Normal(name) if !is_one_name(name) => return Err(invalid_input()),
            Component::CurDir => return Err(invalid_input()),
            _ => {}
        }
    }
    // Without a final separator, which `asks_for_directory` has read.
    let whole = path.components().as_path();
    if whole_first {
        match fs::canonicalize(whole) {
            Ok(found) => {
                return Ok(At {
                    path: found.into_os_string(),
                    canonical: true,
                    directory: None,
                });
            }
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            Err(_) => {}
        }
    }
    // `ancestors` ends at the root: the prefix and the root directory.
    let root = whole.ancestors().last().unwrap_or(whole);
    let found = fs::canonicalize(root)?;
    // The last name first, so that the first name is taken first.
    for part in whole.components().rev() {
        let step = match part {
            Component::Normal(name) => Step::Name(name.to_owned()),
            Component::ParentDir => Step::Up,
            Component::Prefix(_) | Component::RootDir => break,
            Component::CurDir => return Err(invalid_input()),
        };
        queue.push(step);
    }
    Ok(At {
        path: found.into_os_string(),
        canonical: true,
        directory: Some(true),
    })
}

/// Queues the steps of `target`, a link's target that is not absolute, in
/// front of `queue`. Where Windows' rules apply, each `..` in it first
/// cancels the name before it by its text, as Windows reads a link's
/// target, so that only its leading `..` are left to climb from the link's
/// directory. Elsewhere each `..` is taken in turn, as the kernel takes it.
///
/// # Errors
///
/// - `NotFound` for an empty target, which names nothing: macOS (from 13)
///   and the BSDs answer `ENOENT` through such a link, so nothing can be
///   made below it. Linux refuses to make one.
/// - `InvalidInput` for a target with a root or a prefix but not both (on
///   Windows `\x` or `c:x`), or with a name that does not read back as one
///   name ([`is_one_name`]).
fn queue_target(target: &Path, windows: bool, queue: &mut Vec<Step>) -> io::Result<()> {
    if target.as_os_str().is_empty() {
        return Err(io::Error::from(io::ErrorKind::NotFound));
    }
    let mut steps = Vec::new();
    for part in target.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir if windows && matches!(steps.last(), Some(Step::Name(_))) => {
                steps.pop();
            }
            Component::ParentDir => steps.push(Step::Up),
            Component::Normal(name) if is_one_name(name) => {
                steps.push(Step::Name(name.to_owned()));
            }
            Component::Normal(_) | Component::Prefix(_) | Component::RootDir => {
                return Err(invalid_input());
            }
        }
    }
    queue.extend(steps.into_iter().rev());
    Ok(())
}

/// Whether `name`, one component of a path, reads back as that one name
/// when it stands alone. Every name does on Unix. On Windows `c:x` (a
/// stream of the file `c`) reads as a drive and a path, which pushing it
/// onto a path would put in place of that path, and a verbatim path's
/// `a/b` reads as two names.
fn is_one_name(name: &OsStr) -> bool {
    let mut parts = Path::new(name).components();
    matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(only)), None) if only == name
    )
}

/// Whether `text`, a path as given or a link's target, asks for a
/// directory: it ends in a separator, or, where Windows' rules do not
/// apply, in a separator and `.`, which POSIX reads the same way and Win32
/// drops.
fn asks_for_directory(text: &OsStr, windows: bool) -> bool {
    match text.as_encoded_bytes() {
        [.., last] if is_separator_byte(*last) => true,
        [.., before, b'.'] => !windows && is_separator_byte(*before),
        _ => false,
    }
}

/// Whether `path` follows Windows' rules. Only Windows' path parsing gives
/// a path a prefix, and there every absolute path has one.
fn windows_rules(path: &Path) -> bool {
    matches!(path.components().next(), Some(Component::Prefix(_)))
}

/// Whether `byte` is a path separator here: `/`, and on Windows `\` too.
fn is_separator_byte(byte: u8) -> bool {
    std::path::is_separator(char::from(byte))
}

/// Appends `name` to `path` as its last component, by text, so that the
/// name is never read again as a path of its own.
fn push_name(path: &mut OsString, name: &OsStr) {
    if !path
        .as_encoded_bytes()
        .last()
        .is_some_and(|&byte| is_separator_byte(byte))
    {
        path.push(MAIN_SEPARATOR_STR);
    }
    path.push(name);
}

/// `path` with `name` appended as its last component, by text.
fn joined(path: &OsStr, name: &OsStr) -> OsString {
    let mut out = path.to_owned();
    push_name(&mut out, name);
    out
}

/// The [`io::ErrorKind::InvalidInput`] error of what [`resolve`] will not
/// guess at.
fn invalid_input() -> io::Error {
    io::Error::from(io::ErrorKind::InvalidInput)
}

/// `path`, which is absolute, with a plain drive, share or device prefix
/// turned into Windows' verbatim one (`\\?\C:\…`, `\\?\UNC\server\share\…`,
/// `\\?\` for `\\.\`), so that Win32 cannot read a leading part of it again
/// when that part is probed alone: `C:\w\space ` alone would open
/// `C:\w\space`.
/// `std::path::absolute` has already read the whole path as Win32 reads
/// it. Other paths, and every path on Unix, come back as they are.
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
                // `\\.\C:\…`, `\\.\COM1`: Win32 normalizes these too, and
                // both spellings name the same object once it has, as std's
                // `get_long_path` relies on.
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

/// `path` without Windows' `\\?\` (giving `C:\…`) or `\\?\UNC\` (giving
/// `\\server\share\…`), exactly when Win32 reads the plain form back
/// unchanged: on Windows `std::path::absolute` of a plain path is
/// `GetFullPathNameW`, the reading `CreateFileW` applies, and the same test
/// `std` makes (`is_absolute_exact`) before it passes a directory to a
/// child. The plain form is built from the prefix and the rest of the path
/// as it is, so a name that is not valid UTF-16 is tested like any other.
/// Otherwise, and on Unix, where no path has a prefix, `path` is returned
/// as it is.
fn simplify(path: PathBuf) -> PathBuf {
    let plain = {
        let mut parts = path.components();
        match parts.next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::VerbatimDisk(drive) => {
                    let mut plain = OsString::from(format!("{}:", char::from(drive)));
                    plain.push(parts.as_path());
                    Some(plain)
                }
                Prefix::VerbatimUNC(server, share) => {
                    let mut plain = OsString::from(r"\\");
                    plain.push(server);
                    plain.push(r"\");
                    plain.push(share);
                    plain.push(parts.as_path());
                    Some(plain)
                }
                // `\\?\Volume{…}`, `\\?\GLOBALROOT`, `\\.\` devices and plain
                // prefixes stay as they are.
                Prefix::Verbatim(_) | Prefix::DeviceNS(_) | Prefix::UNC(..) | Prefix::Disk(_) => {
                    None
                }
            },
            _ => None,
        }
    };
    match plain {
        Some(plain)
            if std::path::absolute(&plain)
                .is_ok_and(|back| back.as_os_str() == plain.as_os_str()) =>
        {
            PathBuf::from(plain)
        }
        _ => path,
    }
}

/// The part of `path` inside `root`, compared component by component:
/// `Some("")` when they are equal, `None` when `path` is not inside.
///
/// Both must be absolute and hold no `.` or `..`, or the answer is `None`.
/// A plain and a verbatim Windows prefix of one volume count as equal
/// (`\\?\C:\w\x` is inside `C:\w`), but a root that is a bare verbatim
/// prefix (`\\?\C:`, or `\\?\UNC\server\share` with no `\` after it) holds
/// nothing. Every part of the answer is one name that `std` reads back as
/// itself, so a name that Windows reads as a drive (`c:x`, a stream of the
/// file `c`) gives `None`, and `root.join(part)` has `path`'s names. It
/// names the same file only when `root` has `path`'s own form, or `path`
/// is plain: a verbatim `path` under a plain `root` is verbatim because
/// Win32 reads its plain form as something else (a device such as `nul`,
/// or a name without its trailing space or dot), and that plain form is
/// what the join gives. So keep `path` itself, or canonicalize or resolve
/// the join again, rather than open `root.join(part)`. It does no I/O and
/// folds no case, so both sides must come from [`canonicalize`] or
/// [`resolve`]. Unlike [`Path::starts_with`], it is not fooled by `..`.
///
/// ```
/// use std::path::Path;
/// use tidepool_core::path;
///
/// let root = Path::new(env!("CARGO_MANIFEST_DIR"));
/// assert_eq!(path::under(&root.join("src/lib.rs"), root), Some(Path::new("src/lib.rs")));
/// assert_eq!(path::under(&root.join("../etc"), root), None);
/// assert!(root.join("../etc").starts_with(root));
/// ```
#[must_use]
pub fn under<'a>(path: &'a Path, root: &Path) -> Option<&'a Path> {
    if !is_plain(path) || !is_plain(root) {
        return None;
    }
    let mut inside = path.components();
    let mut wanted = root.components();
    match (inside.next(), wanted.next()) {
        (Some(Component::Prefix(a)), Some(Component::Prefix(b))) => {
            if !same_volume(a.kind(), b.kind()) {
                return None;
            }
        }
        // The root directory, on Unix.
        (a, b) if a == b => {}
        _ => return None,
    }
    for part in wanted {
        if inside.next() != Some(part) {
            return None;
        }
    }
    // Each part left must be a name that reads back as itself: not the root
    // directory that a bare verbatim `root` stops before, and not a name
    // such as `c:x`, which on Windows reads as a drive. Fail closed.
    if !inside
        .clone()
        .all(|part| matches!(part, Component::Normal(name) if is_one_name(name)))
    {
        return None;
    }
    Some(inside.as_path())
}

/// Whether [`under`] compares `path`: absolute, with no `.` or `..`.
fn is_plain(path: &Path) -> bool {
    path.is_absolute()
        && !path
            .components()
            .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
}

/// Whether two Windows prefixes name one volume: a drive letter with or
/// without `\\?\` (`std` has already uppercased it), or a server and share
/// with or without `\\?\UNC\`, compared byte for byte. Any other prefix
/// must be the same.
fn same_volume(a: Prefix<'_>, b: Prefix<'_>) -> bool {
    match (a, b) {
        (
            Prefix::Disk(drive) | Prefix::VerbatimDisk(drive),
            Prefix::Disk(other) | Prefix::VerbatimDisk(other),
        ) => drive == other,
        (
            Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share),
            Prefix::UNC(other_server, other_share) | Prefix::VerbatimUNC(other_server, other_share),
        ) => server == other_server && share == other_share,
        _ => a == b,
    }
}

/// Why a path could not be given as text, canonicalized or resolved.
///
/// Every message names the path by its [`display`], as in
/// ``cannot resolve `/srv/caf\xE9`: No such file or directory (os error 2)``.
#[derive(Debug)]
pub enum PathError {
    /// The path is empty, which names no file.
    Empty,
    /// The path is not valid UTF-8 ([`to_utf8`]).
    NotUtf8 {
        /// The path.
        path: PathBuf,
    },
    /// The OS could not resolve the path.
    Io {
        /// The path, as the caller gave it.
        path: PathBuf,
        /// What the OS reported, or for [`resolve`] what its walk found:
        /// [`io::ErrorKind::NotADirectory`] for a name, a `..` or a final
        /// separator after a file, and [`io::ErrorKind::InvalidInput`] for
        /// what it will not guess at in a Windows path: a name Windows reads
        /// as a drive (`c:x`), a `.` a verbatim path keeps, or a link target
        /// that is neither relative nor absolute.
        source: io::Error,
    },
    /// [`resolve`] itself followed 40 symbolic links and met another: in a
    /// chain or a loop through names that do not exist yet, on any OS, or
    /// in a chain longer than the OS checks.
    TooManyLinks {
        /// The path, as the caller gave it.
        path: PathBuf,
    },
}

impl PathError {
    /// The kind of I/O error this is closest to: [`io::ErrorKind::InvalidInput`]
    /// for [`PathError::Empty`], [`io::ErrorKind::InvalidData`] for
    /// [`PathError::NotUtf8`], the source's for [`PathError::Io`], and
    /// [`io::ErrorKind::Other`] for [`PathError::TooManyLinks`], since the
    /// kind for a link loop cannot be named on stable Rust and
    /// [`io::ErrorKind::TooManyLinks`] means too many hard links.
    pub fn kind(&self) -> io::ErrorKind {
        match self {
            Self::Empty => io::ErrorKind::InvalidInput,
            Self::NotUtf8 { .. } => io::ErrorKind::InvalidData,
            Self::Io { source, .. } => source.kind(),
            Self::TooManyLinks { .. } => io::ErrorKind::Other,
        }
    }
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("the path is empty"),
            Self::NotUtf8 { path } => write!(f, "`{}` is not UTF-8", display(path)),
            Self::Io { path, source } => {
                write!(f, "cannot resolve `{}`: {source}", display(path))
            }
            Self::TooManyLinks { path } => write!(
                f,
                "cannot resolve `{}`: it goes through more than {MAX_LINKS} symbolic links",
                display(path)
            ),
        }
    }
}

impl std::error::Error for PathError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Empty | Self::NotUtf8 { .. } | Self::TooManyLinks { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests;
