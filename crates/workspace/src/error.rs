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

//! The error type for `tidepool-workspace`.
//!
//! New in Tidepool (M4.7, ADR-0067), with the errors of buffers, the file
//! tree and sessions from M4.11.

use std::error::Error;
use std::fmt;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use tidepool_codec::CodecError;
use tidepool_core::path::{self, PathError};
use tidepool_text::TextError;

/// Errors produced by `tidepool-workspace`: opening a [folder](crate::folder),
/// reading its ignore files, the [trust store](crate::trust), the
/// [settings](crate::settings), [buffers](crate::buffers), the
/// [file tree](crate::files) and [sessions](crate::session).
///
/// A call that returns one changes nothing in memory, and a file it would
/// have written is as it was, except as [`WorkspaceError::Save`] says.
/// Messages name a file by its [`path::display`], which escapes the bytes
/// and characters a path could hide behind, and hold a setting's key the
/// same way; they never quote a file's contents.
///
/// Non-exhaustive: later versions may add variants.
#[derive(Debug)]
#[non_exhaustive]
pub enum WorkspaceError {
    /// A path could not be canonicalized or resolved, or is not UTF-8
    /// ([`tidepool_core::path`]).
    Path(PathError),
    /// The path given as a folder's root is not a directory.
    NotAFolder {
        /// The canonical path.
        path: PathBuf,
    },
    /// A file Tidepool reads is not a regular file: a directory, a device
    /// or a pipe, or a symbolic link where links are not followed (an
    /// ignore file, as Git reads them).
    NotRegularFile {
        /// The file.
        path: PathBuf,
    },
    /// A file could not be opened or read.
    Read {
        /// The file.
        path: PathBuf,
        /// What the operating system reported.
        source: io::Error,
    },
    /// A file is larger than the most that is read of it.
    TooLarge {
        /// The file.
        path: PathBuf,
        /// The limit, in bytes.
        limit: u64,
    },
    /// An ignore file has more patterns than its
    /// [`IgnoreLimits::max_patterns`](crate::folder::IgnoreLimits::max_patterns).
    TooManyPatterns {
        /// The file.
        path: PathBuf,
        /// The limit.
        limit: usize,
    },
    /// An ignore file that would take the ignore files held together past
    /// [`IgnoreLimits::max_total_bytes`](crate::folder::IgnoreLimits::max_total_bytes).
    IgnoreBudget {
        /// The file.
        path: PathBuf,
        /// The limit, in bytes, of all the files together.
        limit: u64,
    },
    /// A pattern of an ignore file is longer than its
    /// [`IgnoreLimits::max_pattern_bytes`](crate::folder::IgnoreLimits::max_pattern_bytes).
    PatternTooLong {
        /// The file.
        path: PathBuf,
        /// The pattern's line, from 1.
        line: usize,
        /// The limit, in bytes.
        limit: usize,
    },
    /// A settings or trust file is not valid TOML, or breaks
    /// `codec`'s limits.
    Toml {
        /// The file.
        path: PathBuf,
        /// The parser's error, which says where and what.
        source: CodecError,
    },
    /// A settings or trust file has a `version` this version of Tidepool
    /// does not read: not the whole number 1.
    Version {
        /// The file.
        path: PathBuf,
    },
    /// A trust file whose contents are not a trust store
    /// ([`trust`](crate::trust)).
    BadTrustFile {
        /// The file.
        path: PathBuf,
        /// What is wrong, such as "`folders` is not an array of strings".
        reason: &'static str,
    },
    /// A folder that a trust grant cannot hold: one that is not absolute,
    /// or holds `.` or `..`. A path that is not UTF-8 is
    /// [`WorkspaceError::Path`].
    NotCanonical {
        /// The path.
        path: PathBuf,
    },
    /// The trust store holds as many grants as it may.
    TooManyGrants {
        /// The limit.
        limit: usize,
    },
    /// A key a [`Schema`](crate::settings::Schema) refuses to declare.
    BadSetting {
        /// The key.
        key: String,
        /// Why, such as "it is a prefix of another setting's key".
        reason: &'static str,
    },
    /// A key the [`Schema`](crate::settings::Schema) does not declare.
    UnknownSetting {
        /// The key.
        key: String,
    },
    /// A value that is not one the setting takes: of another kind, out of
    /// its range, or not one of its choices.
    InvalidValue {
        /// The key.
        key: String,
        /// Why, in the words a warning uses.
        reason: String,
    },
    /// A setting whose value is not written back: a table, in M4.7.
    NotWritable {
        /// The key.
        key: String,
    },
    /// A settings or trust file that cannot be used, so it is not
    /// replaced: Tidepool does not overwrite text it does not understand,
    /// or a newer version's file.
    Unwritable {
        /// The file.
        path: PathBuf,
        /// Why it cannot be used, shared with the file or store that keeps
        /// it.
        reason: Arc<WorkspaceError>,
    },
    /// The settings file's document refused the edit
    /// ([`Document::set`](tidepool_codec::toml::Document::set)), as when a
    /// key's table is a value of another kind.
    Edit {
        /// The file.
        path: PathBuf,
        /// The document model's error, which names the step of the key.
        source: CodecError,
    },
    /// The folder a file goes in could not be made.
    CreateFolder {
        /// The folder.
        path: PathBuf,
        /// What the operating system reported.
        source: io::Error,
    },
    /// A file could not be saved ([`tidepool_text::file::save`], or
    /// [`tidepool_text::file::write_in_place`]). The file is as it was,
    /// unless the step was the directory's sync, after which it holds the
    /// new text, or a step of a write in place, which says what the file
    /// may hold ([`tidepool_text::FileStep`]).
    Save {
        /// The file.
        path: PathBuf,
        /// The save's error, with the step that failed.
        source: TextError,
    },
    /// A workspace's settings file, or its `.tidepool` folder, that is a
    /// symbolic link, or a `.tidepool` that is not a folder: a workspace's
    /// settings are read and written only at `.tidepool/settings.toml`
    /// itself, so that a setting never edits another file.
    Linked {
        /// The link, or what is not a folder.
        path: PathBuf,
    },
    /// A workspace's settings file that no
    /// [`TrustStore`](crate::trust::TrustStore) trusted, which is not
    /// written.
    Untrusted {
        /// The file.
        path: PathBuf,
    },
    /// A file could not be loaded into a buffer
    /// ([`tidepool_text::file::load`]): it cannot be read, or is not a
    /// regular file.
    Load {
        /// The file, by its canonical path.
        path: PathBuf,
        /// The load's error, with the step that failed.
        source: TextError,
    },
    /// No open buffer has this id: it was closed, or never opened.
    NoBuffer {
        /// The id.
        id: crate::buffers::BufferId,
    },
    /// A buffer that is read-only cannot be edited or saved
    /// ([`ReadOnly`](crate::buffers::ReadOnly)).
    ReadOnlyBuffer {
        /// The buffer's file.
        path: PathBuf,
        /// Why it is read-only.
        why: crate::buffers::ReadOnly,
    },
    /// A save of this buffer is already running: saves of one buffer are
    /// serialized ([`tidepool_text::History::checkpoint`]).
    SaveInFlight {
        /// The buffer's file.
        path: PathBuf,
    },
    /// An answer to another buffer's save question was given to this
    /// buffer's save, which would write this buffer's text where the user
    /// answered for another's ([`crate::buffers::Answer`], ADR-0070).
    AnswerToAnotherBuffer {
        /// The file of the buffer saved.
        path: PathBuf,
        /// The buffer whose question the answer answers.
        asked: crate::buffers::BufferId,
    },
    /// A folder the [file tree](crate::files) does not read: one outside
    /// the root, named with `..`, or reached through a symbolic link or
    /// something that is not a folder, as Git does not follow a link.
    NotBrowsable {
        /// The folder, relative to the root.
        path: PathBuf,
    },
    /// A session file whose contents are not a session
    /// ([`session`](crate::session)).
    BadSessionFile {
        /// The file.
        path: PathBuf,
        /// What is wrong, such as "`layout` is missing, or is not a table".
        reason: &'static str,
    },
    /// A session file that holds another folder's session, as when two
    /// folders' names gave it the same name, so it is not used
    /// (ADR-0068).
    OtherFolder {
        /// The file.
        path: PathBuf,
        /// The folder the session was asked for.
        folder: PathBuf,
    },
}

impl fmt::Display for WorkspaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Path(error) => error.fmt(f),
            Self::NotAFolder { path } => {
                write!(f, "`{}` is not a folder", path::display(path))
            }
            Self::NotRegularFile { path } => write!(
                f,
                "`{}` is not a regular file, or is a symbolic link where links are not followed",
                path::display(path)
            ),
            Self::Read { path, source } => {
                write!(f, "cannot read `{}`: {source}", path::display(path))
            }
            Self::TooLarge { path, limit } => write!(
                f,
                "`{}` is larger than the limit of {limit} bytes",
                path::display(path)
            ),
            Self::TooManyPatterns { path, limit } => write!(
                f,
                "`{}` has more than the limit of {limit} patterns",
                path::display(path)
            ),
            Self::IgnoreBudget { path, limit } => write!(
                f,
                "`{}` is not used, as the ignore files read would pass the limit of {limit} \
                 bytes in all",
                path::display(path)
            ),
            Self::PatternTooLong { path, line, limit } => write!(
                f,
                "`{}`, line {line}: a pattern is longer than the limit of {limit} bytes",
                path::display(path)
            ),
            Self::Toml { path, source } => {
                write!(f, "`{}` is not valid TOML: {source}", path::display(path))
            }
            Self::Version { path } => write!(
                f,
                "`{}` has a `version` this Tidepool does not read; it reads 1",
                path::display(path)
            ),
            Self::BadTrustFile { path, reason } => {
                write!(f, "`{}` is not a trust file: {reason}", path::display(path))
            }
            Self::NotCanonical { path } => write!(
                f,
                "`{}` is not an absolute path free of `.` and `..`",
                path::display(path)
            ),
            Self::TooManyGrants { limit } => {
                write!(f, "the trust store holds the limit of {limit} folders")
            }
            Self::BadSetting { key, reason } => write!(
                f,
                "the setting `{}` cannot be declared: {reason}",
                path::display(key.as_str())
            ),
            Self::UnknownSetting { key } => {
                write!(f, "no setting is named `{}`", path::display(key.as_str()))
            }
            Self::InvalidValue { key, reason } => write!(
                f,
                "not a value of `{}`: {reason}",
                path::display(key.as_str())
            ),
            Self::NotWritable { key } => write!(
                f,
                "the setting `{}` is a table, which is not written back",
                path::display(key.as_str())
            ),
            Self::Unwritable { path, reason } => write!(
                f,
                "`{}` is not replaced, as it cannot be used: {reason}",
                path::display(path)
            ),
            Self::Edit { path, source } => {
                write!(f, "cannot edit `{}`: {source}", path::display(path))
            }
            Self::CreateFolder { path, source } => {
                write!(f, "cannot make `{}`: {source}", path::display(path))
            }
            Self::Save { path, source } => {
                write!(f, "cannot save `{}`: {source}", path::display(path))
            }
            Self::Linked { path } => write!(
                f,
                "`{}` is a symbolic link or not a folder; a workspace's settings are read and \
                 written only at `.tidepool/settings.toml` itself",
                path::display(path)
            ),
            Self::Untrusted { path } => write!(
                f,
                "`{}` is not written, as no trusted folder gave it",
                path::display(path)
            ),
            Self::Load { path, source } => {
                write!(f, "cannot open `{}`: {source}", path::display(path))
            }
            Self::NoBuffer { id } => write!(f, "no buffer {} is open", id.get()),
            Self::ReadOnlyBuffer { path, why } => {
                write!(f, "`{}` is read-only: {why}", path::display(path))
            }
            Self::SaveInFlight { path } => write!(
                f,
                "`{}` is being saved already; a save waits for the last one",
                path::display(path)
            ),
            Self::AnswerToAnotherBuffer { path, asked } => write!(
                f,
                "the answer is to buffer {}'s question, not to `{}`'s",
                asked.get(),
                path::display(path)
            ),
            Self::NotBrowsable { path } => write!(
                f,
                "`{}` is not a folder of the workspace reached without a symbolic link",
                path::display(path)
            ),
            Self::BadSessionFile { path, reason } => {
                write!(
                    f,
                    "`{}` is not a session file: {reason}",
                    path::display(path)
                )
            }
            Self::OtherFolder { path, folder } => write!(
                f,
                "`{}` holds another folder's session, not `{}`'s",
                path::display(path),
                path::display(folder)
            ),
        }
    }
}

impl Error for WorkspaceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Path(error) => Some(error),
            Self::Read { source, .. } | Self::CreateFolder { source, .. } => Some(source),
            Self::Toml { source, .. } | Self::Edit { source, .. } => Some(source),
            Self::Save { source, .. } | Self::Load { source, .. } => Some(source),
            Self::Unwritable { reason, .. } => Some(reason.as_ref()),
            _ => None,
        }
    }
}

impl From<PathError> for WorkspaceError {
    fn from(error: PathError) -> Self {
        Self::Path(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_escape_paths_and_keys() {
        let error = WorkspaceError::NotRegularFile {
            path: PathBuf::from("a\u{1b}[2J/.gitignore"),
        };
        assert!(
            error.to_string().contains(r"a\u{1b}[2J/.gitignore"),
            "{error}"
        );
        let error = WorkspaceError::UnknownSetting {
            key: "ui.\u{202e}x".to_string(),
        };
        assert_eq!(error.to_string(), r"no setting is named `ui.\u{202e}x`");
    }

    #[test]
    fn an_unwritable_file_gives_its_reason_as_its_source() {
        let error = WorkspaceError::Unwritable {
            path: PathBuf::from("/c/settings.toml"),
            reason: Arc::new(WorkspaceError::Version {
                path: PathBuf::from("/c/settings.toml"),
            }),
        };
        assert!(error.source().is_some());
        assert!(error.to_string().contains("does not read"), "{error}");
    }
}
