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

//! Open buffers: the text of each open file, its undo history, and its
//! loads and saves, which run on workers (M4.11, ADR-0068, ADR-0069).
//!
//! New in Tidepool.
//!
//! # Buffers
//!
//! A [`Buffer`] is one open file: its text, a [`Rope`], and the [`History`]
//! every edit goes through (ADR-0020). [`Buffers`] holds them, each under
//! a [`BufferId`] that is never used again, and finds one by its file's
//! canonical path ([`tidepool_core::path::canonicalize`]), so that a file
//! opened through two spellings, or through a symbolic link and its
//! target, is one buffer.
//!
//! # Work on workers
//!
//! Reading and writing files blocks, and the main thread never waits
//! (`AGENTS.md` rule 6). So [`Buffers`] never touches the file system
//! itself. It hands out jobs, plain values that can move to another
//! thread, and takes back what they did:
//!
//! - [`Buffers::open`] gives a [`LoadJob`]; a worker calls
//!   [`LoadJob::run`]; [`Buffers::finish_load`] makes the buffer from the
//!   [`LoadDone`].
//! - [`Buffers::save`] gives a [`SaveJob`]; a worker calls
//!   [`SaveJob::run`]; [`Buffers::finish_save`] takes the [`SaveDone`] and
//!   says what happened ([`SaveReport`]).
//!
//! Each [`Buffer`] has a [`Version`] that every edit raises. A job carries
//! the buffer and the version it was made for, so a result can be told
//! from a later one; [`Buffers::is_current`] says whether a version is
//! still the buffer's. A save's result for a buffer that was closed, or
//! that is not the save the buffer waits for, is stale and dropped
//! ([`SaveReport::Stale`]).
//!
//! # Large files
//!
//! A file of more than [`LARGE_FILE_BYTES`] (16 MiB) loads whole, like any
//! other, and its buffer opens read-only ([`ReadOnly::Large`]) until
//! [`Buffer::make_editable`] (ADR-0068). A file with bytes that are not
//! text has no format to save in, so its buffer stays read-only
//! ([`ReadOnly::NotText`]).
//!
//! # Saving
//!
//! A save is serialized per buffer: [`Buffers::save`] takes a
//! [`Checkpoint`] of the history, the job writes the checkpoint's text,
//! and a success marks that revision saved
//! ([`History::mark_saved`]), so [`Buffer::is_modified`] is false exactly
//! when the text is the one on disk. A second save waits for the first
//! ([`WorkspaceError::SaveInFlight`]).
//!
//! A save writes a temporary file beside the file and renames it over the
//! file, so a crash leaves the old text or the new, never a mix
//! (ADR-0025). The rename puts a *new* file at the path, which differs
//! from the old one when the old one has other names (hard links), which
//! keep the old text, or when a new file there gets another owner or
//! group than the old one has. So a save in [`SaveMode::Check`], the
//! default, looks before it renames (ADR-0069):
//!
//! 1. [`tidepool_text::file::prepare`] writes and syncs the temporary file.
//! 2. `platform::file::metadata` reads the file the save replaces (after
//!    its symbolic links) and the temporary file: the file's link count,
//!    owner and group, and the owner and group the file system really gave
//!    the new file.
//! 3. When the file has one name and the two agree, the save renames.
//!    Otherwise it removes the temporary file, changes nothing, and asks
//!    ([`SaveReport::Asks`] with a [`SaveQuestion`]). An id that could not
//!    be read, or that is [`OVERFLOW_ID`] (which a user namespace shows for
//!    ids it does not map, and NFS gives squashed users), counts as not
//!    knowing, and the save asks.
//!
//! The user answers by saving again with [`SaveQuestion::write_in_place`]
//! ([`SaveMode::InPlace`]), which writes over the file and keeps its
//! identity but is not atomic, or [`SaveQuestion::save_as_new_file`]
//! ([`SaveMode::NewFile`]), the atomic save of what the question showed;
//! or does nothing, which cancels. A new file is saved without a question, unless
//! a file appeared at its path while the save was writing
//! ([`SaveQuestion::appeared`]).
//!
//! # An answer is for the question it answers
//!
//! The user answers later, and by then much can change: another program
//! renamed its own file over the path, or deleted the file and made a new
//! one; the file gained or lost a name, an owner or a group; or the caller
//! kept the answer to use again. So an answer is bound to one question of
//! one buffer (M4.12, ADR-0070):
//!
//! - **Only the question makes an answer** ([`SaveQuestion::write_in_place`],
//!   [`SaveQuestion::save_as_new_file`]), and the answer names the buffer
//!   and the save that asked ([`Answer`]).
//! - **An answer is used once.** A buffer keeps the question its last save
//!   asked ([`Buffer::asked`]) until its next save starts. That save takes
//!   it: an answer to it does what it says; an answer to an earlier
//!   question, or one used already, runs as [`SaveMode::Check`]; an answer
//!   given to another buffer's save is
//!   [`WorkspaceError::AnswerToAnotherBuffer`].
//! - **"Write in place" writes only into the file asked about.**
//!   [`tidepool_text::file::write_in_place_if`] gives the file it opened to
//!   a check, which compares its identity (`platform::file::FileId`: device,
//!   inode number and birth time) with the question's before a byte is
//!   written. If the path now reaches another file, or none, the save runs
//!   as a checked save, which asks again when that file needs it, or makes
//!   the file anew, as a checked save of a deleted file does.
//! - **"Save as a new file" renames only what the user was shown.** It
//!   prepares its temporary file and asks the question again: if it is the
//!   question answered (the same target, identity, names, owner and new
//!   owner), it renames; if there is nothing to ask now, it renames as a
//!   checked save would; otherwise it asks the new question. As for a
//!   checked save, the look comes before the commit, which still gives the
//!   temporary file its permissions and syncs it before the rename, so a
//!   file another program renames over the path during that sync is not
//!   seen.
//! - **A look that failed never counts as knowing.** Where `platform`
//!   reads no metadata on this system, a question knows nothing of the file
//!   and an answer acts on whatever the path reaches, as the save always
//!   did there. On a system where it does, a question one of whose looks
//!   failed (a missing file, a link loop, an error of the file system) is
//!   answered by "save as a new file" only by asking again. "Write in
//!   place" needs only the identity of the file itself: it writes when the
//!   file it opened is the one whose identity the question read, whatever
//!   the look at the temporary file found, and never when that identity
//!   could not be read.
//!
//! On systems where `platform` cannot read a file's metadata yet (macOS
//! until M11.8, Windows until M12.2), the check cannot know, so every save
//! over an existing file asks.

use std::collections::BTreeMap;
use std::fmt;
use std::fs::File;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tidepool_core::path;
use tidepool_platform::file::{self as platform_file, FileId, Metadata};
use tidepool_text::file::{self as text_file, InPlace, LineEndings, Loaded};
use tidepool_text::{Checkpoint, FileStep, Format, History, LineBreak, Rope, TextError};

use crate::WorkspaceError;

pub use tidepool_platform::file::Owner;
pub use tidepool_text::file::LARGE_FILE_BYTES;

/// The id that stands for an unknown one: what Linux shows inside a user
/// namespace for a user or group it does not map (the default of
/// `/proc/sys/kernel/overflowuid` and `overflowgid`, user_namespaces(7)),
/// and NFS's default anonymous user and group for squashed ones
/// (exports(5)). A save that sees it on either side asks (ADR-0069).
pub const OVERFLOW_ID: u32 = 65534;

/// An open buffer's id, unique among the buffers a [`Buffers`] ever held.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BufferId(u64);

impl BufferId {
    /// The number, as messages show it.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// How many times a buffer was edited: each [`Buffer::edit`] raises it,
/// so a result made for one version can be told from a later one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Version(u64);

impl Version {
    /// The number: 0 when the buffer was loaded.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Why a buffer is read-only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ReadOnly {
    /// The file has more than [`LARGE_FILE_BYTES`]: it opens read-only
    /// until [`Buffer::make_editable`] (ADR-0068).
    Large,
    /// The file has bytes that are not text, which loading replaced with
    /// U+FFFD, so there is no format to save the text in, and saving would
    /// lose the bytes ([`Loaded::read_only`]). It stays read-only.
    NotText,
}

impl fmt::Display for ReadOnly {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Large => "it is larger than 16 MiB, and opens read-only until made editable",
            Self::NotText => "it holds bytes that are not text, which a save would lose",
        })
    }
}

/// How a save writes the file ([`Buffers::save`]).
///
/// Only a [`SaveQuestion`] makes the two answers, each for that question
/// alone, so no save skips the check without a question behind it (the
/// [module documentation](self#an-answer-is-for-the-question-it-answers),
/// ADR-0070).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SaveMode {
    /// The atomic save, which first asks when its rename would split a
    /// hard link or give the file another owner or group (the [module
    /// documentation](self#saving)). What a save does by default.
    Check,
    /// The answer "save as a new file" ([`SaveQuestion::save_as_new_file`]):
    /// the atomic save, which splits the link, or gives the file the
    /// saver's owner and group, as the question said. It renames when the
    /// question asked again just before is the one answered, or there is
    /// none; otherwise it asks the new one.
    NewFile(Answer),
    /// The answer "write in place" ([`SaveQuestion::write_in_place`]):
    /// writing over the file without a rename
    /// ([`tidepool_text::file::write_in_place_if`]), which keeps the file's
    /// links, owner, group, permissions, access list and extended
    /// attributes, but leaves it part-written if Tidepool or the machine
    /// stops during the write. It writes only into the file the question
    /// asked about; if the path reaches another file, or none, the save
    /// runs as [`SaveMode::Check`] instead.
    InPlace(Answer),
}

/// What an answer answers: the buffer and the save whose
/// [`SaveQuestion`] it is (ADR-0070).
///
/// Only [`SaveQuestion::write_in_place`] and
/// [`SaveQuestion::save_as_new_file`] make one. It is taken once, by the
/// next save of its buffer ([`Buffers::save`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Answer {
    buffer: BufferId,
    ticket: u64,
}

impl Answer {
    /// The buffer whose question this answers.
    pub fn buffer(&self) -> BufferId {
        self.buffer
    }
}

/// What a look at a file's metadata found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Look {
    /// The file's metadata.
    Read(Metadata),
    /// `platform` reads no metadata on this system (macOS until M11.8,
    /// Windows until M12.2).
    Unsupported,
    /// The look failed: the file is missing or cannot be reached, a link
    /// leads round in a loop, or the file system erred.
    Failed,
}

impl Look {
    /// What `platform` answered.
    fn of(found: Result<Metadata, tidepool_platform::Error>) -> Self {
        match found {
            Ok(metadata) => Self::Read(metadata),
            Err(tidepool_platform::Error::Unsupported { .. }) => Self::Unsupported,
            Err(_) => Self::Failed,
        }
    }

    /// The metadata, if it was read.
    fn metadata(self) -> Option<Metadata> {
        match self {
            Self::Read(metadata) => Some(metadata),
            Self::Unsupported | Self::Failed => None,
        }
    }
}

/// One open file: its text, its history, and what loading found out
/// about it.
///
/// `Debug` shows sizes and states, never the text.
pub struct Buffer {
    id: BufferId,
    path: PathBuf,
    text: Rope,
    history: History,
    format: Option<Format>,
    line_endings: LineEndings,
    trailing_break: Option<LineBreak>,
    file_len: u64,
    read_only: Option<ReadOnly>,
    version: Version,
    saving: Option<Saving>,
    asked: Option<SaveQuestion>,
}

/// The save a buffer waits for.
struct Saving {
    ticket: u64,
    checkpoint: Checkpoint,
}

impl Buffer {
    /// The id.
    pub fn id(&self) -> BufferId {
        self.id
    }

    /// The file's canonical path, which [`Buffers::find`] looks up.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The text.
    pub fn text(&self) -> &Rope {
        &self.text
    }

    /// The undo history.
    pub fn history(&self) -> &History {
        &self.history
    }

    /// The format a save writes, the one the file was read in; `None`
    /// exactly when the buffer is [`ReadOnly::NotText`].
    pub fn format(&self) -> Option<Format> {
        self.format
    }

    /// The file's line breaks when it was loaded, by kind, for the break
    /// a new line takes ([`LineEndings::dominant`]).
    pub fn line_endings(&self) -> LineEndings {
        self.line_endings
    }

    /// The break the file ended with when it was loaded, if any.
    pub fn trailing_break(&self) -> Option<LineBreak> {
        self.trailing_break
    }

    /// The file's size in bytes when it was loaded.
    pub fn file_len(&self) -> u64 {
        self.file_len
    }

    /// Why the buffer is read-only, or `None` when it may be edited.
    pub fn read_only(&self) -> Option<ReadOnly> {
        self.read_only
    }

    /// The version: how many times [`Buffer::edit`] was called.
    pub fn version(&self) -> Version {
        self.version
    }

    /// Whether the text differs from the one last saved or loaded
    /// ([`History::is_modified`]).
    pub fn is_modified(&self) -> bool {
        self.history.is_modified()
    }

    /// Whether a save is running.
    pub fn is_saving(&self) -> bool {
        self.saving.is_some()
    }

    /// The question the buffer's last save asked, which an answer may still
    /// answer: until the buffer's next save starts.
    pub fn asked(&self) -> Option<&SaveQuestion> {
        self.asked.as_ref()
    }

    /// The text and its history, to edit through the history (ADR-0020).
    /// The version goes up by one, whether or not the caller then changes
    /// the text, so a result made before is no longer current.
    ///
    /// # Errors
    ///
    /// [`WorkspaceError::ReadOnlyBuffer`] when the buffer is read-only.
    pub fn edit(&mut self) -> Result<(&mut Rope, &mut History), WorkspaceError> {
        if let Some(why) = self.read_only {
            return Err(WorkspaceError::ReadOnlyBuffer {
                path: self.path.clone(),
                why,
            });
        }
        self.version = Version(self.version.0.saturating_add(1));
        Ok((&mut self.text, &mut self.history))
    }

    /// Lets a large file's buffer be edited and saved ([`ReadOnly::Large`]).
    /// A buffer that may be edited already stays so.
    ///
    /// # Errors
    ///
    /// [`WorkspaceError::ReadOnlyBuffer`] for [`ReadOnly::NotText`], which
    /// stays read-only.
    pub fn make_editable(&mut self) -> Result<(), WorkspaceError> {
        match self.read_only {
            None | Some(ReadOnly::Large) => {
                self.read_only = None;
                Ok(())
            }
            Some(why) => Err(WorkspaceError::ReadOnlyBuffer {
                path: self.path.clone(),
                why,
            }),
        }
    }
}

impl fmt::Debug for Buffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Buffer")
            .field("id", &self.id)
            .field("path", &self.path)
            .field("len", &self.text.len())
            .field("format", &self.format)
            .field("read_only", &self.read_only)
            .field("version", &self.version)
            .field("modified", &self.is_modified())
            .field("saving", &self.is_saving())
            .finish()
    }
}

/// Whether [`Buffers::finish_load`] made a buffer or found one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opened {
    /// The buffer of the file.
    pub id: BufferId,
    /// Whether the file was open already, under another spelling or
    /// through a link perhaps. Then the text just read was dropped, and
    /// the buffer is as it was.
    pub already_open: bool,
}

/// The open buffers.
#[derive(Debug, Default)]
pub struct Buffers {
    buffers: BTreeMap<BufferId, Buffer>,
    by_path: BTreeMap<PathBuf, BufferId>,
    next_id: u64,
}

/// The next save's ticket. One counter for the whole process, so that a
/// save's ticket, and the [`Answer`] to its question, is never another
/// [`Buffers`]' too: an answer that one `Buffers` made, given to another's
/// buffer of the same [`BufferId`], answers no question there (M4.12's
/// second review).
static NEXT_TICKET: AtomicU64 = AtomicU64::new(0);

impl Buffers {
    /// No buffers.
    pub fn new() -> Self {
        Self::default()
    }

    /// A job that loads the file at `path`, for a worker to run. Nothing
    /// happens until [`LoadJob::run`]; then [`Buffers::finish_load`] opens
    /// its buffer.
    pub fn open(&self, path: impl Into<PathBuf>) -> LoadJob {
        LoadJob { path: path.into() }
    }

    /// Opens the buffer of a load that ran, or finds the one already open
    /// for its file. A large file's buffer is read-only
    /// ([`ReadOnly::Large`]), as is one of a file that is not text
    /// ([`ReadOnly::NotText`]).
    ///
    /// # Errors
    ///
    /// The load's error: [`WorkspaceError::Path`] when the path could not
    /// be canonicalized (it is missing, say), and
    /// [`WorkspaceError::Load`] when the file could not be read.
    pub fn finish_load(&mut self, done: LoadDone) -> Result<Opened, WorkspaceError> {
        let (path, loaded) = done.result?;
        if let Some(&id) = self.by_path.get(&path) {
            return Ok(Opened {
                id,
                already_open: true,
            });
        }
        let read_only = if loaded.read_only() {
            Some(ReadOnly::NotText)
        } else if loaded.is_large() {
            Some(ReadOnly::Large)
        } else {
            None
        };
        let format = loaded.format();
        let line_endings = loaded.line_endings();
        let trailing_break = loaded.trailing_break();
        let file_len = loaded.file_len();
        let text = loaded.into_text();
        // Its root revision, the text just loaded, is marked saved.
        let history = History::new(&text);
        let id = BufferId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        self.by_path.insert(path.clone(), id);
        self.buffers.insert(
            id,
            Buffer {
                id,
                path,
                text,
                history,
                format,
                line_endings,
                trailing_break,
                file_len,
                read_only,
                version: Version(0),
                saving: None,
                asked: None,
            },
        );
        Ok(Opened {
            id,
            already_open: false,
        })
    }

    /// The buffer `id`, if it is open.
    pub fn get(&self, id: BufferId) -> Option<&Buffer> {
        self.buffers.get(&id)
    }

    /// The buffer `id`, to edit, if it is open.
    pub fn get_mut(&mut self, id: BufferId) -> Option<&mut Buffer> {
        self.buffers.get_mut(&id)
    }

    /// The buffer of the file whose canonical path is `path`, if one is
    /// open.
    pub fn find(&self, path: &Path) -> Option<BufferId> {
        self.by_path.get(path).copied()
    }

    /// The open buffers, in the order they were opened.
    pub fn iter(&self) -> impl Iterator<Item = &Buffer> {
        self.buffers.values()
    }

    /// How many buffers are open.
    pub fn len(&self) -> usize {
        self.buffers.len()
    }

    /// Whether no buffer is open.
    pub fn is_empty(&self) -> bool {
        self.buffers.is_empty()
    }

    /// Whether `version` is still the version of the open buffer `id`.
    pub fn is_current(&self, id: BufferId, version: Version) -> bool {
        self.get(id).is_some_and(|buffer| buffer.version == version)
    }

    /// Closes the buffer `id` and returns it, if it was open. The result of
    /// a save it was waiting for is then stale. Whether it had unsaved
    /// changes is the caller's to ask about first.
    pub fn close(&mut self, id: BufferId) -> Option<Buffer> {
        let buffer = self.buffers.remove(&id)?;
        self.by_path.remove(&buffer.path);
        Some(buffer)
    }

    /// Starts a save of the buffer `id` in `mode`: takes a checkpoint of its
    /// history and returns the job that writes its text, for a worker to
    /// run. [`Buffers::finish_save`] takes what it did.
    ///
    /// A save that starts takes the buffer's question ([`Buffer::asked`]),
    /// so an answer is used once: an answer to that question does what it
    /// says, and any other answer runs as [`SaveMode::Check`], which
    /// [`SaveJob::mode`] then reports (the [module
    /// documentation](self#an-answer-is-for-the-question-it-answers)).
    ///
    /// # Errors
    ///
    /// [`WorkspaceError::NoBuffer`] when `id` is not open;
    /// [`WorkspaceError::AnswerToAnotherBuffer`] when `mode` answers
    /// another buffer's question; [`WorkspaceError::ReadOnlyBuffer`] when
    /// the buffer is read-only; [`WorkspaceError::SaveInFlight`] when a save
    /// of it is running. None of them takes the buffer's question.
    pub fn save(&mut self, id: BufferId, mode: SaveMode) -> Result<SaveJob, WorkspaceError> {
        let buffer = self
            .buffers
            .get_mut(&id)
            .ok_or(WorkspaceError::NoBuffer { id })?;
        let answer = match mode {
            SaveMode::Check => None,
            SaveMode::NewFile(answer) | SaveMode::InPlace(answer) => Some(answer),
        };
        if let Some(answer) = answer
            && answer.buffer != id
        {
            return Err(WorkspaceError::AnswerToAnotherBuffer {
                path: buffer.path.clone(),
                asked: answer.buffer,
            });
        }
        if let Some(why) = buffer.read_only {
            return Err(WorkspaceError::ReadOnlyBuffer {
                path: buffer.path.clone(),
                why,
            });
        }
        let Some(format) = buffer.format else {
            return Err(WorkspaceError::ReadOnlyBuffer {
                path: buffer.path.clone(),
                why: ReadOnly::NotText,
            });
        };
        if buffer.saving.is_some() {
            return Err(WorkspaceError::SaveInFlight {
                path: buffer.path.clone(),
            });
        }
        // The question, taken whatever this save is, so that no answer to it
        // is used twice; it answers only an answer to it.
        let asked = buffer.asked.take();
        let answered = match (answer, asked) {
            (Some(answer), Some(asked)) if asked.ticket == answer.ticket => Some(asked),
            _ => None,
        };
        let mode = if answered.is_some() {
            mode
        } else {
            SaveMode::Check
        };
        // Unique, not ordered: no other memory depends on it.
        let ticket = NEXT_TICKET.fetch_add(1, Ordering::Relaxed);
        let checkpoint = buffer.history.checkpoint();
        let job = SaveJob {
            id,
            ticket,
            version: buffer.version,
            path: buffer.path.clone(),
            text: checkpoint.text().clone(),
            format,
            mode,
            answered,
        };
        buffer.saving = Some(Saving { ticket, checkpoint });
        Ok(job)
    }

    /// Takes what a save did. A success marks the saved revision
    /// ([`History::mark_saved`]); a question or a failure leaves the buffer
    /// free to save again. A failure after which the file may hold neither
    /// text whole, or one whose durability is not known (a write in place
    /// stopped, a sync that failed), forgets which revision is on disk
    /// ([`History::clear_saved`]), so the buffer reads as modified.
    pub fn finish_save(&mut self, done: SaveDone) -> SaveReport {
        let Some(buffer) = self.buffers.get_mut(&done.id) else {
            return SaveReport::Stale;
        };
        if buffer
            .saving
            .as_ref()
            .is_none_or(|saving| saving.ticket != done.ticket)
        {
            return SaveReport::Stale;
        }
        let Some(saving) = buffer.saving.take() else {
            return SaveReport::Stale;
        };
        match done.outcome {
            Outcome::Saved => {
                if let Err(source) = buffer.history.mark_saved(&saving.checkpoint) {
                    // Not reached: the checkpoint came from this history.
                    buffer.history.clear_saved();
                    return SaveReport::Failed {
                        id: done.id,
                        error: WorkspaceError::Save {
                            path: buffer.path.clone(),
                            source,
                        },
                    };
                }
                SaveReport::Saved {
                    id: done.id,
                    version: done.version,
                }
            }
            Outcome::Asks(question) => {
                buffer.asked = Some(question.clone());
                SaveReport::Asks {
                    id: done.id,
                    question,
                }
            }
            Outcome::Failed(error) => {
                if leaves_file_unknown(&error) {
                    buffer.history.clear_saved();
                }
                SaveReport::Failed { id: done.id, error }
            }
        }
    }
}

/// Whether a failed save may have left the file holding neither text
/// whole, or the new one without knowing it survives a crash.
fn leaves_file_unknown(error: &WorkspaceError) -> bool {
    matches!(
        error,
        WorkspaceError::Save {
            source: TextError::Io {
                step: FileStep::WriteInPlace
                    | FileStep::SetLength
                    | FileStep::SyncInPlace
                    | FileStep::SyncDirectory,
                ..
            },
            ..
        }
    )
}

/// A load for a worker to run ([`Buffers::open`]).
#[derive(Clone, Debug)]
pub struct LoadJob {
    path: PathBuf,
}

impl LoadJob {
    /// The path asked for, as given.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Canonicalizes the path and loads the file
    /// ([`tidepool_text::file::load`]). It blocks on the file system, so
    /// run it on a worker, never on the main thread.
    pub fn run(self) -> LoadDone {
        let result = path::canonicalize(&self.path)
            .map_err(WorkspaceError::Path)
            .and_then(|canonical| match text_file::load(&canonical) {
                Ok(loaded) => Ok((canonical, loaded)),
                Err(source) => Err(WorkspaceError::Load {
                    path: canonical,
                    source,
                }),
            });
        LoadDone {
            requested: self.path,
            result,
        }
    }
}

/// What a [`LoadJob`] did, for [`Buffers::finish_load`].
///
/// `Debug` shows the loaded file's sizes, never its text.
#[derive(Debug)]
pub struct LoadDone {
    requested: PathBuf,
    result: Result<(PathBuf, Loaded), WorkspaceError>,
}

impl LoadDone {
    /// The path the load was asked for, as given.
    pub fn requested(&self) -> &Path {
        &self.requested
    }

    /// The file's canonical path, if it could be found.
    pub fn canonical(&self) -> Option<&Path> {
        self.result.as_ref().ok().map(|(path, _)| path.as_path())
    }
}

/// A save for a worker to run ([`Buffers::save`]): a snapshot of the text,
/// shared with the buffer's rope, and where and how to write it.
///
/// `Debug` shows the text's length, never the text.
pub struct SaveJob {
    id: BufferId,
    ticket: u64,
    version: Version,
    path: PathBuf,
    text: Rope,
    format: Format,
    mode: SaveMode,
    /// The question `mode` answers, taken from the buffer: `None` in
    /// [`SaveMode::Check`].
    answered: Option<SaveQuestion>,
}

impl SaveJob {
    /// The buffer saved.
    pub fn id(&self) -> BufferId {
        self.id
    }

    /// The buffer's version whose text is saved.
    pub fn version(&self) -> Version {
        self.version
    }

    /// The file saved, by the canonical path it was opened under.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// How the save writes the file: [`SaveMode::Check`] for an answer that
    /// was not to the buffer's question ([`Buffers::save`]).
    pub fn mode(&self) -> SaveMode {
        self.mode
    }

    /// Writes the text, as the [module documentation](self#saving) says.
    /// It blocks on the file system, so run it on a worker, never on the
    /// main thread.
    pub fn run(self) -> SaveDone {
        self.run_with(
            |path| Look::of(platform_file::metadata(path)),
            |file| Look::of(platform_file::metadata_of(file)),
        )
    }

    /// [`SaveJob::run`], reading the metadata a check compares with
    /// `at_path` for a path and `of_file` for an open file, which a test
    /// replaces to make a file's owner, group or identity differ.
    fn run_with(
        self,
        at_path: impl Fn(&Path) -> Look,
        of_file: impl Fn(&File) -> Look,
    ) -> SaveDone {
        let outcome = match (self.mode, &self.answered) {
            (SaveMode::InPlace(_), Some(question)) => self.in_place(question, &at_path, &of_file),
            (SaveMode::NewFile(_), Some(question)) => self.check(Some(question), &at_path),
            _ => self.check(None, &at_path),
        };
        SaveDone {
            id: self.id,
            ticket: self.ticket,
            version: self.version,
            outcome,
        }
    }

    /// The answer "write in place" to `question`: writes over the file the
    /// path reaches if it is the file asked about, else saves as a checked
    /// save.
    fn in_place(
        &self,
        question: &SaveQuestion,
        at_path: &impl Fn(&Path) -> Look,
        of_file: &impl Fn(&File) -> Look,
    ) -> Outcome {
        let written = text_file::write_in_place_if(&self.path, &self.text, self.format, |file| {
            question.is_file(of_file(file))
        });
        match written {
            Ok(InPlace::Written) => Outcome::Saved,
            // Another file than the one asked about: the answer is not for
            // it (ADR-0070).
            Ok(InPlace::Refused) => self.check(None, at_path),
            // No file there now: a checked save makes it anew.
            Err(TextError::Io {
                step: FileStep::Metadata,
                error,
            }) if error.kind() == ErrorKind::NotFound => self.check(None, at_path),
            Err(source) => self.failed(source),
        }
    }

    /// A checked save: prepares the temporary file, then
    /// [`SaveJob::check_prepared`], with the question `answered`, if this
    /// is the answer "save as a new file" to it.
    fn check(&self, answered: Option<&SaveQuestion>, at_path: &impl Fn(&Path) -> Look) -> Outcome {
        match text_file::prepare(&self.path, &self.text, self.format) {
            Err(source) => self.failed(source),
            Ok(prepared) => self.check_prepared(prepared, answered, at_path),
        }
    }

    /// Reads the file `prepared` would replace and its temporary file, and
    /// commits when there is nothing to ask, or when the question is the
    /// one `answered`; otherwise drops the prepared save, which removes its
    /// file, and asks.
    fn check_prepared(
        &self,
        prepared: text_file::PreparedSave,
        answered: Option<&SaveQuestion>,
        at_path: &impl Fn(&Path) -> Look,
    ) -> Outcome {
        let file = at_path(prepared.target());
        let new = at_path(prepared.temp_path());
        // A file that appeared since `prepare` found none is asked about
        // too, rather than renamed over.
        let appeared = !prepared.replaces() && file.metadata().is_some();
        let question = if prepared.replaces() || appeared {
            SaveQuestion::build(self.id, self.ticket, prepared.target(), file, new, appeared)
        } else {
            None
        };
        match question {
            None => self.commit(prepared),
            Some(question) if answered.is_some_and(|asked| asked.is_answered_by(&question)) => {
                self.commit(prepared)
            }
            Some(question) => Outcome::Asks(question),
        }
    }

    /// Renames the prepared save over its target.
    fn commit(&self, prepared: text_file::PreparedSave) -> Outcome {
        match prepared.commit() {
            Ok(()) => Outcome::Saved,
            Err(source) => self.failed(source),
        }
    }

    /// The outcome of a save that failed with `source`.
    fn failed(&self, source: TextError) -> Outcome {
        Outcome::Failed(WorkspaceError::Save {
            path: self.path.clone(),
            source,
        })
    }
}

impl fmt::Debug for SaveJob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SaveJob")
            .field("id", &self.id)
            .field("version", &self.version)
            .field("path", &self.path)
            .field("len", &self.text.len())
            .field("format", &self.format)
            .field("mode", &self.mode)
            .finish()
    }
}

/// What a [`SaveJob`] did, for [`Buffers::finish_save`].
#[derive(Debug)]
pub struct SaveDone {
    id: BufferId,
    ticket: u64,
    version: Version,
    outcome: Outcome,
}

impl SaveDone {
    /// The buffer saved.
    pub fn id(&self) -> BufferId {
        self.id
    }
}

/// What a save did, inside a [`SaveDone`].
#[derive(Debug)]
enum Outcome {
    Saved,
    Asks(SaveQuestion),
    Failed(WorkspaceError),
}

/// What [`Buffers::finish_save`] found a save did.
#[derive(Debug)]
#[non_exhaustive]
pub enum SaveReport {
    /// The file holds the text of `version`, and the buffer marks it saved.
    /// The buffer may have been edited since: then it is still modified.
    Saved {
        /// The buffer.
        id: BufferId,
        /// The version whose text was saved.
        version: Version,
    },
    /// Nothing was written: the rename would have split a hard link or
    /// given the file another owner or group, or could not tell. The user
    /// answers by saving with [`SaveQuestion::write_in_place`] or
    /// [`SaveQuestion::save_as_new_file`], or cancels by doing nothing.
    Asks {
        /// The buffer.
        id: BufferId,
        /// What the save found.
        question: SaveQuestion,
    },
    /// The save failed ([`WorkspaceError::Save`] says at which step and
    /// what the file holds).
    Failed {
        /// The buffer.
        id: BufferId,
        /// What failed.
        error: WorkspaceError,
    },
    /// The buffer was closed, or this is not the save it waits for: the
    /// result was dropped.
    Stale,
}

/// Why a save stopped to ask before its rename (ADR-0068, ADR-0069): what
/// it read of the file it would replace and of the new file it wrote.
///
/// Only a save makes one, for its buffer. Its two answers are
/// [`SaveQuestion::write_in_place`] and [`SaveQuestion::save_as_new_file`],
/// each for this question alone (the [module
/// documentation](self#an-answer-is-for-the-question-it-answers),
/// ADR-0070).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveQuestion {
    buffer: BufferId,
    ticket: u64,
    target: PathBuf,
    file: Option<FileId>,
    links: Option<u64>,
    owner: Option<Owner>,
    new_owner: Option<Owner>,
    appeared: bool,
    /// Whether a look failed, for another reason than that `platform`
    /// reads no metadata on this system.
    failed: bool,
}

impl SaveQuestion {
    /// The question that the save `ticket` of `buffer` asks before a
    /// rename over `target`, whose metadata `file` found, of a new file
    /// whose metadata `new` found, or `None` when the rename changes neither
    /// the links nor the owner or group, as far as can be known; always a
    /// question when a file `appeared`.
    fn build(
        buffer: BufferId,
        ticket: u64,
        target: &Path,
        file: Look,
        new: Look,
        appeared: bool,
    ) -> Option<Self> {
        let failed = file == Look::Failed || new == Look::Failed;
        let (file, new) = (file.metadata(), new.metadata());
        let question = Self {
            buffer,
            ticket,
            target: target.to_path_buf(),
            file: file.map(|file| file.id),
            links: file.map(|file| file.links),
            owner: file.map(|file| file.owner),
            new_owner: new.map(|new| new.owner),
            appeared,
            failed,
        };
        let asks = appeared
            || question.splits_link()
            || question.changes_user() != Some(false)
            || question.changes_group() != Some(false);
        asks.then_some(question)
    }

    /// The file the save would replace: the end of its symbolic links.
    pub fn target(&self) -> &Path {
        &self.target
    }

    /// The identity of the file asked about, if it could be read: the one
    /// at [`SaveQuestion::target`] as the save looked, or the one that
    /// appeared there ([`SaveQuestion::appeared`]).
    pub fn file(&self) -> Option<FileId> {
        self.file
    }

    /// The buffer whose save asked.
    pub fn buffer(&self) -> BufferId {
        self.buffer
    }

    /// The answer "write in place", for the next [`Buffers::save`] of
    /// [`SaveQuestion::buffer`]: writes over the file asked about, keeping
    /// its identity, or saves as [`SaveMode::Check`] would if the path
    /// reaches another file by then.
    pub fn write_in_place(&self) -> SaveMode {
        SaveMode::InPlace(self.answer())
    }

    /// The answer "save as a new file", for the next [`Buffers::save`] of
    /// [`SaveQuestion::buffer`]: the atomic save, if this question is still
    /// the one to ask, or there is none; otherwise it asks the new one.
    pub fn save_as_new_file(&self) -> SaveMode {
        SaveMode::NewFile(self.answer())
    }

    /// What this question's answers answer.
    fn answer(&self) -> Answer {
        Answer {
            buffer: self.buffer,
            ticket: self.ticket,
        }
    }

    /// Whether `now`, a look at the file a write in place opened, found the
    /// file asked about: the same identity, or, where `platform` reads no
    /// metadata, nothing known either time. A look that failed never
    /// counts.
    fn is_file(&self, now: Look) -> bool {
        match (self.file, now) {
            (Some(asked), Look::Read(found)) => found.id == asked,
            (None, Look::Unsupported) => !self.failed,
            _ => false,
        }
    }

    /// Whether `now`, the question a save asks again after its answer
    /// prepared the temporary file, is this one, so that the answer is for
    /// what the user was shown: the same target, identity, names, owner,
    /// new owner and appearance, and no look that failed either time.
    fn is_answered_by(&self, now: &SaveQuestion) -> bool {
        !self.failed
            && !now.failed
            && self.target == now.target
            && self.file == now.file
            && self.links == now.links
            && self.owner == now.owner
            && self.new_owner == now.new_owner
            && self.appeared == now.appeared
    }

    /// How many names (hard links) the file has, if that could be read.
    pub fn links(&self) -> Option<u64> {
        self.links
    }

    /// The file's owner and group, if they could be read.
    pub fn owner(&self) -> Option<Owner> {
        self.owner
    }

    /// The owner and group the file system gave the new file, which a
    /// save as a new file gives the file, if they could be read.
    pub fn new_owner(&self) -> Option<Owner> {
        self.new_owner
    }

    /// Whether a file appeared at the path while the save was writing, when
    /// there was none as it began (it was deleted since it was opened, or
    /// is new): a save as a new file, or a write in place, would replace
    /// it.
    pub fn appeared(&self) -> bool {
        self.appeared
    }

    /// Whether the file has more than one name, so that a save as a new
    /// file leaves the other names holding the old text.
    pub fn splits_link(&self) -> bool {
        self.links.is_some_and(|links| links > 1)
    }

    /// Whether a save as a new file gives the file another owner: `None`
    /// when that is not known, because an owner could not be read or is
    /// [`OVERFLOW_ID`].
    pub fn changes_user(&self) -> Option<bool> {
        compare(self.owner?.user, self.new_owner?.user)
    }

    /// Whether a save as a new file gives the file another group: `None`
    /// when that is not known, as for [`SaveQuestion::changes_user`].
    pub fn changes_group(&self) -> Option<bool> {
        compare(self.owner?.group, self.new_owner?.group)
    }
}

/// Whether two ids differ, or `None` when either stands for an unknown
/// one.
fn compare(old: u32, new: u32) -> Option<bool> {
    (old != OVERFLOW_ID && new != OVERFLOW_ID).then_some(old != new)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The identity a test's fake metadata gives every file.
    const ID: FileId = FileId {
        device: 1,
        inode: 1,
        born: None,
    };

    /// A look that read a file of `links` names, owned by `user` and
    /// `group`, of identity [`ID`].
    fn read(links: u64, user: u32, group: u32) -> Look {
        Look::Read(Metadata {
            links,
            owner: Owner { user, group },
            permissions: 0o644,
            id: ID,
        })
    }

    /// [`read`] with another identity.
    fn read_id(id: FileId, links: u64) -> Look {
        match read(links, 1000, 100) {
            Look::Read(found) => Look::Read(Metadata { id, ..found }),
            other => other,
        }
    }

    /// The question of buffer 1's save 1 about `/w/a.txt`.
    fn question(file: Look, new: Look) -> Option<SaveQuestion> {
        SaveQuestion::build(BufferId(1), 1, Path::new("/w/a.txt"), file, new, false)
    }

    #[test]
    fn a_question_is_asked_only_when_something_changes_or_is_unknown() {
        assert_eq!(question(read(1, 1000, 100), read(1, 1000, 100)), None);
        let cases = [
            // Another name.
            (read(2, 1000, 100), read(1, 1000, 100)),
            // Another owner, as when root saves a user's file.
            (read(1, 1000, 100), read(1, 0, 0)),
            // Another group alone.
            (read(1, 1000, 100), read(1, 1000, 101)),
            // Either side unread, or unreadable here.
            (Look::Failed, read(1, 1000, 100)),
            (read(1, 1000, 100), Look::Failed),
            (Look::Unsupported, Look::Unsupported),
            // The overflow id on either side.
            (read(1, OVERFLOW_ID, 100), read(1, OVERFLOW_ID, 100)),
            (read(1, 1000, OVERFLOW_ID), read(1, 1000, 100)),
        ];
        for (file, new) in cases {
            assert!(question(file, new).is_some(), "{file:?} {new:?}");
        }
        let asked = question(read(3, 1000, 100), read(1, 0, 100)).expect("a question");
        assert!(asked.splits_link());
        assert_eq!(asked.links(), Some(3));
        assert_eq!(asked.changes_user(), Some(true));
        assert_eq!(asked.changes_group(), Some(false));
        assert_eq!(asked.target(), Path::new("/w/a.txt"));
        assert_eq!(asked.file(), Some(ID));
        assert_eq!(asked.buffer(), BufferId(1));
        let unknown = question(Look::Failed, read(1, 0, 0)).expect("a question");
        assert!(!unknown.splits_link());
        assert_eq!(unknown.changes_user(), None);
        assert_eq!(unknown.changes_group(), None);
        assert_eq!(unknown.file(), None);
    }

    /// ADR-0070: an answer names its question; a write in place's check
    /// accepts the file asked about only, and a look that failed never.
    #[test]
    fn an_answer_names_its_question_and_its_file() {
        let asked = question(read(2, 1000, 100), read(1, 1000, 100)).expect("a question");
        let answer = Answer {
            buffer: BufferId(1),
            ticket: 1,
        };
        assert_eq!(asked.write_in_place(), SaveMode::InPlace(answer));
        assert_eq!(asked.save_as_new_file(), SaveMode::NewFile(answer));
        assert_eq!(answer.buffer(), BufferId(1));

        assert!(asked.is_file(read(1, 0, 0)), "the identity alone counts");
        assert!(!asked.is_file(read_id(FileId { inode: 2, ..ID }, 2)));
        assert!(!asked.is_file(read_id(FileId { device: 2, ..ID }, 2)));
        let born = Some(std::time::UNIX_EPOCH);
        assert!(!asked.is_file(read_id(FileId { born, ..ID }, 2)));
        assert!(!asked.is_file(Look::Failed), "known, then failed");
        assert!(!asked.is_file(Look::Unsupported));

        let blind = question(Look::Unsupported, Look::Unsupported).expect("a question");
        assert!(
            blind.is_file(Look::Unsupported),
            "nothing known either time"
        );
        assert!(!blind.is_file(read(2, 1000, 100)));
        let failed = question(Look::Failed, read(1, 1000, 100)).expect("a question");
        assert!(
            !failed.is_file(Look::Unsupported),
            "a failed look never counts"
        );
        assert!(!failed.is_file(Look::Failed));
        assert!(!failed.is_file(read(2, 1000, 100)));
        // A write in place needs only the file's own identity: a failed look
        // at the temporary file does not stop it, but stops "save as a new
        // file" (`a_question_answers_only_the_same_question`).
        let temp_failed = question(read(2, 1000, 100), Look::Failed).expect("a question");
        assert!(temp_failed.is_file(read(2, 1000, 100)));
        assert!(!temp_failed.is_answered_by(&temp_failed.clone()));
    }

    /// M4.12's second review: tickets are unique in the process, so an
    /// answer that one `Buffers` made answers no question of another's,
    /// even for a buffer of the same id; it runs as a checked save there.
    #[test]
    fn an_answer_from_another_buffers_answers_nothing() {
        let scratch = Scratch::new("two-buffers");
        let a = scratch.0.join("a.txt");
        std::fs::write(&a, b"a").expect("write");
        let b = scratch.0.join("b.txt");
        std::fs::write(&b, b"b").expect("write");
        let (mut x, mut y) = (Buffers::new(), Buffers::new());
        let xa = edited(&mut x, &a);
        let yb = edited(&mut y, &b);
        assert_eq!(xa, yb, "each Buffers numbers its own buffers");
        let linked = |_: &Path| read(2, 1000, 100);
        let opened = |_: &File| read(2, 1000, 100);
        let unused = |_: &File| panic!("no file opened");
        let from_x = asks(save_with(&mut x, xa, SaveMode::Check, linked, unused));
        let in_y = asks(save_with(&mut y, yb, SaveMode::Check, linked, unused));
        assert_ne!(from_x.ticket, in_y.ticket);

        let job = y.save(yb, from_x.write_in_place()).expect("a save");
        assert_eq!(job.mode(), SaveMode::Check);
        asks(y.finish_save(job.run_with(linked, opened)));
        assert_eq!(std::fs::read(&b).expect("read"), b"b");
    }

    /// ADR-0070: "save as a new file" renames only when the question asked
    /// again is the one answered.
    #[test]
    fn a_question_answers_only_the_same_question() {
        let asked = question(read(2, 1000, 100), read(1, 1000, 100)).expect("a question");
        assert!(asked.is_answered_by(&asked.clone()));
        let changed = [
            question(read(3, 1000, 100), read(1, 1000, 100)),
            question(read(2, 1000, 101), read(1, 1000, 100)),
            question(read(2, 1000, 100), read(1, 1000, 101)),
            question(read_id(FileId { inode: 2, ..ID }, 2), read(1, 1000, 100)),
            question(read(2, 1000, 100), Look::Failed),
            SaveQuestion::build(
                BufferId(1),
                1,
                Path::new("/w/b.txt"),
                read(2, 1000, 100),
                read(1, 1000, 100),
                false,
            ),
            SaveQuestion::build(
                BufferId(1),
                1,
                Path::new("/w/a.txt"),
                read(2, 1000, 100),
                read(1, 1000, 100),
                true,
            ),
        ];
        for now in changed {
            let now = now.expect("a question");
            assert!(!asked.is_answered_by(&now), "{now:?}");
        }
        let blind = question(Look::Unsupported, Look::Unsupported).expect("a question");
        assert!(
            blind.is_answered_by(&blind.clone()),
            "nothing known either time"
        );
        let failed = question(Look::Failed, read(1, 1000, 100)).expect("a question");
        assert!(
            !failed.is_answered_by(&failed.clone()),
            "a failed look never counts"
        );
    }

    /// A buffer of `path` with `!` typed at its end, through its history.
    fn edited(buffers: &mut Buffers, path: &Path) -> BufferId {
        let id = buffers
            .finish_load(buffers.open(path).run())
            .expect("open")
            .id;
        let buffer = buffers.get_mut(id).expect("open");
        let (text, history) = buffer.edit().expect("editable");
        let selection = tidepool_text::Selection::caret(text.len());
        let transaction = tidepool_text::Transaction::change_each(text, &selection, |cursor| {
            Some((cursor.head..cursor.head, "!", tidepool_text::Place::End))
        })
        .expect("a transaction");
        history
            .commit(
                text,
                transaction,
                &selection,
                std::time::Instant::now(),
                tidepool_text::Group::New,
            )
            .expect("commit");
        id
    }

    /// Runs `buffers`' save of `id` in `mode` with looks `at_path` and
    /// `of_file`, and takes its result.
    fn save_with(
        buffers: &mut Buffers,
        id: BufferId,
        mode: SaveMode,
        at_path: impl Fn(&Path) -> Look,
        of_file: impl Fn(&File) -> Look,
    ) -> SaveReport {
        let job = buffers.save(id, mode).expect("a save");
        buffers.finish_save(job.run_with(at_path, of_file))
    }

    /// The question in `report`, which must ask.
    fn asks(report: SaveReport) -> SaveQuestion {
        match report {
            SaveReport::Asks { question, .. } => question,
            other => panic!("the save must ask: {other:?}"),
        }
    }

    /// ADR-0070, with fake looks: an answer whose file is another by the
    /// time it runs saves as a checked save, which asks again here; where
    /// no metadata can be read, both times, the answer does what it says;
    /// where a look failed, it asks again.
    #[test]
    fn an_answer_runs_only_on_the_file_asked_about() {
        let scratch = Scratch::new("asked");
        let path = scratch.0.join("linked.txt");
        std::fs::write(&path, b"old").expect("write");
        let mut buffers = Buffers::new();
        let id = edited(&mut buffers, &path);
        let first = |_: &Path| read(2, 1000, 100);
        let unused = |_: &File| panic!("no file opened");
        let question = asks(save_with(&mut buffers, id, SaveMode::Check, first, unused));
        assert_eq!(buffers.get(id).expect("open").asked(), Some(&question));

        // By the answer, the path reaches another file, with two names too.
        let other = FileId { inode: 9, ..ID };
        let replaced = |_: &Path| read_id(other, 2);
        let opened = |_: &File| read_id(other, 2);
        let again = asks(save_with(
            &mut buffers,
            id,
            question.write_in_place(),
            replaced,
            opened,
        ));
        assert_eq!(again.file(), Some(other), "about the file there now");
        assert_eq!(std::fs::read(&path).expect("read"), b"old");
        let again = asks(save_with(
            &mut buffers,
            id,
            again.save_as_new_file(),
            first,
            unused,
        ));
        assert_eq!(again.file(), Some(ID), "the question changed back");
        assert_eq!(std::fs::read(&path).expect("read"), b"old");
        assert_eq!(names(&scratch.0), ["linked.txt"]);

        // A look that failed when asked: never answered but by asking again.
        let failed = |_: &Path| Look::Failed;
        let question = asks(save_with(&mut buffers, id, SaveMode::Check, failed, unused));
        let opened_failed = |_: &File| Look::Failed;
        asks(save_with(
            &mut buffers,
            id,
            question.write_in_place(),
            failed,
            opened_failed,
        ));
        assert_eq!(std::fs::read(&path).expect("read"), b"old");

        // Nothing readable here, when asked and now: the answer acts.
        let blind = |_: &Path| Look::Unsupported;
        let question = asks(save_with(&mut buffers, id, SaveMode::Check, blind, unused));
        assert_eq!(question.file(), None);
        let opened_blind = |_: &File| Look::Unsupported;
        let report = save_with(
            &mut buffers,
            id,
            question.write_in_place(),
            blind,
            opened_blind,
        );
        assert!(matches!(report, SaveReport::Saved { .. }), "{report:?}");
        assert_eq!(std::fs::read(&path).expect("read"), b"old!");
    }

    /// ADR-0070: an answer is taken once, by the buffer's next save; reused,
    /// or after another save, it runs as a checked save; given to another
    /// buffer, it is refused.
    #[test]
    fn an_answer_is_used_once_and_only_by_its_buffer() {
        let scratch = Scratch::new("once");
        let path = scratch.0.join("one.txt");
        std::fs::write(&path, b"one").expect("write");
        let other_path = scratch.0.join("two.txt");
        std::fs::write(&other_path, b"two").expect("write");
        let mut buffers = Buffers::new();
        let id = edited(&mut buffers, &path);
        let other = edited(&mut buffers, &other_path);
        let linked = |_: &Path| read(2, 1000, 100);
        let opened = |_: &File| read(2, 1000, 100);
        let unused = |_: &File| panic!("no file opened");
        let question = asks(save_with(&mut buffers, id, SaveMode::Check, linked, unused));

        // Another buffer's save refuses it, and leaves the question.
        let refused = buffers.save(other, question.write_in_place());
        assert!(
            matches!(
                refused,
                Err(WorkspaceError::AnswerToAnotherBuffer { asked, .. }) if asked == id
            ),
            "{refused:?}"
        );
        assert!(buffers.get(id).expect("open").asked().is_some());

        // Used once: it writes in place.
        let job = buffers.save(id, question.write_in_place()).expect("a save");
        assert_eq!(job.mode(), question.write_in_place());
        assert!(buffers.get(id).expect("open").asked().is_none(), "taken");
        let report = buffers.finish_save(job.run_with(linked, opened));
        assert!(matches!(report, SaveReport::Saved { .. }), "{report:?}");
        assert_eq!(std::fs::read(&path).expect("read"), b"one!");

        // Used again: a checked save, which asks, as the file has two names.
        let job = buffers.save(id, question.write_in_place()).expect("a save");
        assert_eq!(job.mode(), SaveMode::Check);
        let second = asks(buffers.finish_save(job.run_with(linked, unused)));
        assert_ne!(second, question, "a question of its own");

        // A question that another save took is answered no more: the
        // answer runs as a checked save, which asks the newest question.
        let third = asks(save_with(&mut buffers, id, SaveMode::Check, linked, unused));
        let job = buffers.save(id, second.write_in_place()).expect("a save");
        assert_eq!(job.mode(), SaveMode::Check);
        let fourth = asks(buffers.finish_save(job.run_with(linked, unused)));
        assert_ne!(fourth, third);
        assert_eq!(std::fs::read(&path).expect("read"), b"one!");
        assert_eq!(std::fs::read(&other_path).expect("read"), b"two");
    }

    /// M4.11's security review: a file made at the path while a save of a
    /// new file was writing is asked about, not renamed over.
    #[test]
    fn a_file_that_appears_during_the_save_is_asked_about() {
        let scratch = Scratch::new("appeared");
        let path = scratch.0.join("gone.txt");
        std::fs::write(&path, b"first").expect("write");
        let mut buffers = Buffers::new();
        let id = buffers
            .finish_load(buffers.open(&path).run())
            .expect("open")
            .id;
        std::fs::remove_file(&path).expect("delete it");
        // The metadata reader stands for the moment after `prepare`: by
        // then another program made the file again.
        let appeared = |at: &Path| {
            if at == path {
                std::fs::write(&path, b"someone else's").expect("write");
            }
            read(1, 1000, 100)
        };
        let unused = |_: &File| panic!("no file opened");
        let question = asks(save_with(
            &mut buffers,
            id,
            SaveMode::Check,
            appeared,
            unused,
        ));
        assert!(question.appeared());
        assert_eq!(std::fs::read(&path).expect("read"), b"someone else's");
        assert_eq!(
            names(&scratch.0),
            ["gone.txt"],
            "the temporary file is gone"
        );
    }

    #[test]
    fn failures_that_may_change_the_file_forget_the_saved_revision() {
        let failed = |step| WorkspaceError::Save {
            path: PathBuf::from("/w/a.txt"),
            source: TextError::Io {
                step,
                error: std::io::ErrorKind::Other.into(),
            },
        };
        for step in [
            FileStep::WriteInPlace,
            FileStep::SetLength,
            FileStep::SyncInPlace,
            FileStep::SyncDirectory,
        ] {
            assert!(leaves_file_unknown(&failed(step)), "{step:?}");
        }
        for step in [FileStep::CreateTemp, FileStep::Write, FileStep::Rename] {
            assert!(!leaves_file_unknown(&failed(step)), "{step:?}");
        }
    }

    /// A scratch folder for one test, removed when the test passes. Unit
    /// tests have no `CARGO_TARGET_TMPDIR`, so it is in the system's
    /// temporary folder, made anew under a name no other run has: one that
    /// exists already, perhaps another user's, fails the test.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let since = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("the clock is after 1970");
            let dir = std::env::temp_dir().join(format!(
                "tidepool-buffers-{name}-{}-{}",
                std::process::id(),
                since.as_nanos()
            ));
            std::fs::create_dir(&dir).expect("make a fresh scratch folder");
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            if !std::thread::panicking() {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    /// The names in `dir`, sorted.
    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .expect("list")
            .map(|entry| {
                entry
                    .expect("an entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }

    /// ADR-0069, without `cfg(unix)` in this crate: the temporary file is
    /// made to read as another group than the file's, as in a folder with
    /// the setgid bit or on a file system mounted with `bsdgroups`, so the
    /// save asks; then each answer does what it says. That a real file's
    /// owner and group are read is `platform::file`'s test (M4.10).
    #[test]
    fn a_group_the_rename_would_change_stops_the_save_for_the_answer() {
        let scratch = Scratch::new("group");
        let path = scratch.0.join("shared.txt");
        std::fs::write(&path, b"old").expect("write");
        let mut buffers = Buffers::new();
        // An edit through the history, so that the text to save is new.
        let id = edited(&mut buffers, &path);
        assert!(buffers.get(id).expect("open").is_modified());

        let other_group = |at: &Path| {
            let group = if at == path { 100 } else { 101 };
            read(1, 1000, group)
        };
        let the_file = |_: &File| read(1, 1000, 100);
        let unused = |_: &File| panic!("no file opened");
        let question = asks(save_with(
            &mut buffers,
            id,
            SaveMode::Check,
            other_group,
            unused,
        ));
        assert_eq!(question.changes_group(), Some(true));
        assert_eq!(question.changes_user(), Some(false));
        assert!(!question.splits_link());
        assert_eq!(std::fs::read(&path).expect("read"), b"old");
        assert_eq!(
            names(&scratch.0),
            ["shared.txt"],
            "the temporary file is gone"
        );
        let buffer = buffers.get(id).expect("open");
        assert!(buffer.is_modified() && !buffer.is_saving());

        // Writing in place.
        let report = save_with(
            &mut buffers,
            id,
            question.write_in_place(),
            other_group,
            the_file,
        );
        assert!(matches!(report, SaveReport::Saved { .. }), "{report:?}");
        assert_eq!(std::fs::read(&path).expect("read"), b"old!");
        assert!(!buffers.get(id).expect("open").is_modified());

        // Saving as a new file, asked again first: the same question, so
        // it renames.
        let question = asks(save_with(
            &mut buffers,
            id,
            SaveMode::Check,
            other_group,
            unused,
        ));
        let report = save_with(
            &mut buffers,
            id,
            question.save_as_new_file(),
            other_group,
            unused,
        );
        assert!(matches!(report, SaveReport::Saved { .. }), "{report:?}");
        assert_eq!(std::fs::read(&path).expect("read"), b"old!");
        assert_eq!(names(&scratch.0), ["shared.txt"]);

        // M4.12's review: a group that changed since the question, so that
        // the rename would give the file another group than the user was
        // shown, asks again, writing nothing.
        let id = edited(&mut buffers, &path);
        let question = asks(save_with(
            &mut buffers,
            id,
            SaveMode::Check,
            other_group,
            unused,
        ));
        let yet_another = |at: &Path| {
            let group = if at == path { 100 } else { 102 };
            read(1, 1000, group)
        };
        let again = asks(save_with(
            &mut buffers,
            id,
            question.save_as_new_file(),
            yet_another,
            unused,
        ));
        assert_eq!(again.new_owner().map(|owner| owner.group), Some(102));
        assert_eq!(std::fs::read(&path).expect("read"), b"old!");
        assert_eq!(names(&scratch.0), ["shared.txt"]);
    }

    #[test]
    fn jobs_and_results_can_go_to_workers() {
        fn send<T: Send + 'static>() {}
        send::<LoadJob>();
        send::<LoadDone>();
        send::<SaveJob>();
        send::<SaveDone>();
    }
}
