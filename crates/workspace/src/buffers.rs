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
//! The user answers by saving again in [`SaveMode::InPlace`], which writes
//! over the file and keeps its identity but is not atomic, or
//! [`SaveMode::NewFile`], the atomic save without the check; or does
//! nothing, which cancels. A new file is saved without a question, unless
//! a file appeared at its path while the save was writing
//! ([`SaveQuestion::appeared`]).
//!
//! On systems where `platform` cannot read a file's metadata yet (macOS
//! until M11.8, Windows until M12.2), the check cannot know, so every save
//! over an existing file asks.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use tidepool_core::path;
use tidepool_platform::file::{self as platform_file, Metadata};
use tidepool_text::file::{self as text_file, LineEndings, Loaded};
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SaveMode {
    /// The atomic save, which first asks when its rename would split a
    /// hard link or give the file another owner or group (the [module
    /// documentation](self#saving)). What a save does by default.
    Check,
    /// The atomic save without the check: the answer "save as a new
    /// file", which splits the link, or gives the file the saver's owner
    /// and group, as the question said.
    NewFile,
    /// Writing over the file without a rename
    /// ([`tidepool_text::file::write_in_place`]): the answer "write in
    /// place", which keeps the file's links, owner, group, permissions,
    /// access list and extended attributes, but leaves it part-written if
    /// Tidepool or the machine stops during the write.
    InPlace,
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
    next_ticket: u64,
}

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
    /// # Errors
    ///
    /// [`WorkspaceError::NoBuffer`] when `id` is not open;
    /// [`WorkspaceError::ReadOnlyBuffer`] when the buffer is read-only;
    /// [`WorkspaceError::SaveInFlight`] when a save of it is running.
    pub fn save(&mut self, id: BufferId, mode: SaveMode) -> Result<SaveJob, WorkspaceError> {
        let ticket = self.next_ticket;
        let buffer = self
            .buffers
            .get_mut(&id)
            .ok_or(WorkspaceError::NoBuffer { id })?;
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
        let checkpoint = buffer.history.checkpoint();
        let job = SaveJob {
            id,
            ticket,
            version: buffer.version,
            path: buffer.path.clone(),
            text: checkpoint.text().clone(),
            format,
            mode,
        };
        buffer.saving = Some(Saving { ticket, checkpoint });
        self.next_ticket = self.next_ticket.saturating_add(1);
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
            Outcome::Asks(question) => SaveReport::Asks {
                id: done.id,
                question,
            },
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

    /// How the save writes the file.
    pub fn mode(&self) -> SaveMode {
        self.mode
    }

    /// Writes the text, as the [module documentation](self#saving) says.
    /// It blocks on the file system, so run it on a worker, never on the
    /// main thread.
    pub fn run(self) -> SaveDone {
        self.run_with(|path| platform_file::metadata(path).ok())
    }

    /// [`SaveJob::run`], reading the metadata a check compares with
    /// `metadata`, which a test replaces to make a file's owner or group
    /// differ.
    fn run_with(self, metadata: impl Fn(&Path) -> Option<Metadata>) -> SaveDone {
        let save_error = |source| {
            Outcome::Failed(WorkspaceError::Save {
                path: self.path.clone(),
                source,
            })
        };
        let outcome = match self.mode {
            SaveMode::NewFile => match text_file::save(&self.path, &self.text, self.format) {
                Ok(()) => Outcome::Saved,
                Err(source) => save_error(source),
            },
            SaveMode::InPlace => {
                match text_file::write_in_place(&self.path, &self.text, self.format) {
                    Ok(()) => Outcome::Saved,
                    Err(source) => save_error(source),
                }
            }
            SaveMode::Check => match text_file::prepare(&self.path, &self.text, self.format) {
                Err(source) => save_error(source),
                Ok(prepared) => {
                    let file = metadata(prepared.target());
                    let new = metadata(prepared.temp_path());
                    // A file that appeared since `prepare` found none is
                    // asked about too, rather than renamed over.
                    let appeared = !prepared.replaces() && file.is_some();
                    let question = if prepared.replaces() || appeared {
                        SaveQuestion::build(prepared.target(), file, new, appeared)
                    } else {
                        None
                    };
                    match question {
                        // Dropping the prepared save removes its file.
                        Some(question) => Outcome::Asks(question),
                        None => match prepared.commit() {
                            Ok(()) => Outcome::Saved,
                            Err(source) => save_error(source),
                        },
                    }
                }
            },
        };
        SaveDone {
            id: self.id,
            ticket: self.ticket,
            version: self.version,
            outcome,
        }
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
    /// answers by saving in [`SaveMode::InPlace`] or [`SaveMode::NewFile`],
    /// or cancels by doing nothing.
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveQuestion {
    target: PathBuf,
    links: Option<u64>,
    owner: Option<Owner>,
    new_owner: Option<Owner>,
    appeared: bool,
}

impl SaveQuestion {
    /// The question for a rename over `target`, whose metadata is `file`,
    /// of a new file whose metadata is `new` (`None` where they could not
    /// be read), or `None` when the rename changes neither the links nor
    /// the owner or group, as far as can be known.
    pub fn of(target: &Path, file: Option<Metadata>, new: Option<Metadata>) -> Option<Self> {
        Self::build(target, file, new, false)
    }

    /// [`SaveQuestion::of`], always asking when `appeared`.
    fn build(
        target: &Path,
        file: Option<Metadata>,
        new: Option<Metadata>,
        appeared: bool,
    ) -> Option<Self> {
        let question = Self {
            target: target.to_path_buf(),
            links: file.map(|file| file.links),
            owner: file.map(|file| file.owner),
            new_owner: new.map(|new| new.owner),
            appeared,
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

    fn metadata(links: u64, user: u32, group: u32) -> Option<Metadata> {
        Some(Metadata {
            links,
            owner: Owner { user, group },
            permissions: 0o644,
        })
    }

    #[test]
    fn a_question_is_asked_only_when_something_changes_or_is_unknown() {
        let target = Path::new("/w/a.txt");
        assert_eq!(
            SaveQuestion::of(target, metadata(1, 1000, 100), metadata(1, 1000, 100)),
            None
        );
        let cases = [
            // Another name.
            (metadata(2, 1000, 100), metadata(1, 1000, 100)),
            // Another owner, as when root saves a user's file.
            (metadata(1, 1000, 100), metadata(1, 0, 0)),
            // Another group alone.
            (metadata(1, 1000, 100), metadata(1, 1000, 101)),
            // Either side unread.
            (None, metadata(1, 1000, 100)),
            (metadata(1, 1000, 100), None),
            // The overflow id on either side.
            (metadata(1, OVERFLOW_ID, 100), metadata(1, OVERFLOW_ID, 100)),
            (metadata(1, 1000, OVERFLOW_ID), metadata(1, 1000, 100)),
        ];
        for (file, new) in cases {
            let question = SaveQuestion::of(target, file, new);
            assert!(question.is_some(), "{file:?} {new:?}");
        }
        let question = SaveQuestion::of(target, metadata(3, 1000, 100), metadata(1, 0, 100))
            .expect("a question");
        assert!(question.splits_link());
        assert_eq!(question.links(), Some(3));
        assert_eq!(question.changes_user(), Some(true));
        assert_eq!(question.changes_group(), Some(false));
        assert_eq!(question.target(), target);
        let unknown = SaveQuestion::of(target, None, metadata(1, 0, 0)).expect("a question");
        assert!(!unknown.splits_link());
        assert_eq!(unknown.changes_user(), None);
        assert_eq!(unknown.changes_group(), None);
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
        let job = buffers.save(id, SaveMode::Check).expect("a save");
        // The metadata reader stands for the moment after `prepare`: by
        // then another program made the file again.
        let appeared = |at: &Path| {
            if at == path {
                std::fs::write(&path, b"someone else's").expect("write");
            }
            metadata(1, 1000, 100)
        };
        let report = buffers.finish_save(job.run_with(appeared));
        let SaveReport::Asks { question, .. } = report else {
            panic!("the save must ask: {report:?}");
        };
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
        let id = buffers
            .finish_load(buffers.open(&path).run())
            .expect("open")
            .id;
        // An edit through the history, so that the text to save is new.
        let buffer = buffers.get_mut(id).expect("open");
        let (text, history) = buffer.edit().expect("editable");
        let selection = tidepool_text::Selection::caret(3);
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
        assert!(buffers.get(id).expect("open").is_modified());

        let other_group = |at: &Path| {
            let group = if at == path { 100 } else { 101 };
            metadata(1, 1000, group)
        };
        let job = buffers.save(id, SaveMode::Check).expect("a save");
        let SaveReport::Asks { question, .. } = buffers.finish_save(job.run_with(other_group))
        else {
            panic!("the save must ask");
        };
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
        let job = buffers.save(id, SaveMode::InPlace).expect("a save");
        assert!(matches!(
            buffers.finish_save(job.run_with(other_group)),
            SaveReport::Saved { .. }
        ));
        assert_eq!(std::fs::read(&path).expect("read"), b"old!");
        assert!(!buffers.get(id).expect("open").is_modified());

        // Saving as a new file asks nothing.
        let job = buffers.save(id, SaveMode::NewFile).expect("a save");
        assert!(matches!(
            buffers.finish_save(job.run_with(other_group)),
            SaveReport::Saved { .. }
        ));
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
