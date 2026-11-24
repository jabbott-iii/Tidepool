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

//! Settings: typed values in layers, read from TOML files and written back
//! with the rest of each file kept as it was.
//!
//! New in Tidepool (M4.7, ADR-0013, ADR-0067).
//!
//! # Declaring settings
//!
//! Each part of Tidepool that has settings declares them in a [`Schema`]:
//! a key and a [`Kind`] with its default. A key is one to eight segments
//! joined by `.`, each of lowercase ASCII letters, digits and `_`, starting
//! with a letter, at most 64 bytes: `ui.side_panel`. In a file the segments
//! before the last name tables, so `ui.side_panel` is `side_panel` in
//! `[ui]`. `version` is reserved for the file's format, and no key may be
//! a prefix of another (`ui` and `ui.side_panel`).
//!
//! # Layers
//!
//! A setting's value is the top valid one of three layers:
//!
//! 1. its default, from the schema;
//! 2. the user's: [`FILE_NAME`], `settings.toml` in the user config
//!    folder ([`SettingsFile::user`]);
//! 3. the workspace's: `.tidepool/settings.toml` in the folder, read only
//!    once the folder is trusted ([`SettingsFile::workspace`],
//!    ADR-0014). A project's file can bind keys and, later, name tools to
//!    run, so an untrusted one is never read at all, and
//!    [`Settings::resolve`] uses a workspace's file only when one of the
//!    constructors that ask a [`TrustStore`] made it.
//!
//! [`Settings::resolve`] gives each setting its value, and a [`Warning`]
//! for each value it could not use, naming the file and the key and saying
//! which value it used instead:
//!
//! - a value that is not of its setting's kind, is out of its range, or
//!   is not one of its choices falls back to the next layer below that
//!   has a valid one, or to the default;
//! - a table on a key's way that is a value of another kind (`ui = 3`)
//!   gives one warning, and the settings in it fall back the same way;
//! - a layer that cannot be used gives none of its values, and one
//!   warning: a file that is not a regular file, is larger than
//!   [`MAX_FILE_BYTES`], cannot be read, is not TOML, or has a `version`
//!   other than [`VERSION`]. A file without `version` is version 1.
//!
//! A key no setting declares is ignored, and kept in the file. For a
//! table ([`Kind::Table`], such as the keymap), the top layer that has one
//! wins, and [`Settings::table_layers`] gives each layer's for an owner
//! that merges them.
//!
//! # Writing back
//!
//! [`SettingsFile::write`] reads the file again, so an edit the user made
//! since is kept, sets the one value through `codec`'s document model
//! ([`Document::set`]), which keeps every other byte, comments and unknown
//! keys included (ADR-0031), adds `version = 1` to a file that has no
//! `version`, and saves it with [`tidepool_text::file::save`], through a
//! temporary file and a rename that follows a link to its target
//! (ADR-0025). It refuses a file that cannot be used rather than replace
//! text it does not understand, or a newer version's file. A workspace's
//! file is read and written only at `.tidepool/settings.toml` itself:
//! never through a symbolic link at `.tidepool` or at the file, so that a
//! setting never edits another of the project's files, such as its
//! `Cargo.toml`. Only booleans, whole numbers and strings are written; a
//! table is not, in M4.7.
//!
//! Reading and writing block on the file system, so call them on a
//! worker.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tidepool_codec::Limits;
use tidepool_codec::toml::{Document, Step, Table, ValueKind};
use tidepool_core::log::warn;
use tidepool_core::path;
use tidepool_text::{Format, Rope};

use crate::WorkspaceError;
use crate::folder::Folder;
use crate::read::{self, Links};
use crate::trust::TrustStore;

/// The user's settings file's name in the user config folder.
pub const FILE_NAME: &str = "settings.toml";

/// The settings files' format version, which Tidepool reads and writes.
pub const VERSION: i64 = 1;

/// The largest settings file read: 1 MiB. An edit that would make a file
/// larger is refused, so that the file can still be read.
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// The most segments a key has.
pub const MAX_KEY_SEGMENTS: usize = 8;

/// The longest segment of a key, in bytes.
pub const MAX_SEGMENT_BYTES: usize = 64;

/// The key reserved for the file's format.
const VERSION_KEY: &str = "version";

/// The limits settings files are read and edited with: `codec`'s
/// defaults, with input up to [`MAX_FILE_BYTES`].
const LIMITS: Limits = {
    let mut limits = Limits::DEFAULT;
    limits.max_input_bytes = MAX_FILE_BYTES as usize;
    limits
};

/// What a setting holds, and its default.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `true` or `false`.
    Bool {
        /// The default.
        default: bool,
    },
    /// A whole number from `min` to `max`, both included.
    Integer {
        /// The default, from `min` to `max`.
        default: i64,
        /// The smallest value.
        min: i64,
        /// The largest value.
        max: i64,
    },
    /// A string: one of `choices`, or any string if there are none.
    String {
        /// The default, one of `choices` if there are any.
        default: &'static str,
        /// The values it may take, or none for any string.
        choices: &'static [&'static str],
    },
    /// A table its owner reads itself, such as the keymap's modes; the
    /// default is none.
    Table,
}

impl Kind {
    /// The default, or `None` for a table.
    fn default_value(&self) -> Option<Value> {
        match *self {
            Kind::Bool { default } => Some(Value::Bool(default)),
            Kind::Integer { default, .. } => Some(Value::Integer(default)),
            Kind::String { default, .. } => Some(Value::String(default.to_string())),
            Kind::Table => None,
        }
    }

    /// `value` as a [`Value`] of this kind, or why it is not one, with
    /// `key` named.
    fn read(&self, key: &str, value: &ValueKind) -> Result<Value, String> {
        let shown = path::display(key);
        match (self, value) {
            (Kind::Bool { .. }, ValueKind::Boolean(value)) => Ok(Value::Bool(*value)),
            (Kind::Bool { .. }, _) => Err(format!("`{shown}` is not `true` or `false`")),
            (Kind::Integer { min, max, .. }, ValueKind::Integer(value)) => {
                if (*min..=*max).contains(value) {
                    Ok(Value::Integer(*value))
                } else {
                    Err(format!("`{shown}` is outside its range, {min} to {max}"))
                }
            }
            (Kind::Integer { .. }, _) => Err(format!("`{shown}` is not a whole number")),
            (Kind::String { choices, .. }, ValueKind::String(value)) => {
                if choices.is_empty() || choices.contains(&value.as_str()) {
                    Ok(Value::String(value.clone()))
                } else {
                    let listed: Vec<String> =
                        choices.iter().map(|choice| format!("`{choice}`")).collect();
                    Err(format!("`{shown}` is not one of {}", listed.join(", ")))
                }
            }
            (Kind::String { .. }, _) => Err(format!("`{shown}` is not a string")),
            (Kind::Table, ValueKind::Table(table)) => Ok(Value::Table(table.clone())),
            (Kind::Table, _) => Err(format!("`{shown}` is not a table")),
        }
    }

    /// Why `value` is not one this kind takes, if it is not.
    fn check(&self, key: &str, value: &Value) -> Result<(), String> {
        let kind = match value {
            Value::Bool(value) => ValueKind::Boolean(*value),
            Value::Integer(value) => ValueKind::Integer(*value),
            Value::String(value) => ValueKind::String(value.clone()),
            Value::Table(table) => ValueKind::Table(table.clone()),
        };
        self.read(key, &kind).map(drop)
    }
}

/// A setting's value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// A [`Kind::Bool`]'s.
    Bool(bool),
    /// A [`Kind::Integer`]'s.
    Integer(i64),
    /// A [`Kind::String`]'s.
    String(String),
    /// A [`Kind::Table`]'s.
    Table(Table),
}

/// The settings a part of Tidepool declares, by key.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Schema {
    settings: BTreeMap<&'static str, Kind>,
}

impl Schema {
    /// No settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Declares the setting `key`, of `kind`.
    ///
    /// # Errors
    ///
    /// [`WorkspaceError::BadSetting`] for a key that is not one (the
    /// [module documentation](self#declaring-settings)), one declared
    /// already, one that is a prefix of another's or has another's as its
    /// prefix, an integer's default outside its range, or a string's
    /// default that is not one of its choices. The schema is unchanged.
    pub fn add(&mut self, key: &'static str, kind: Kind) -> Result<(), WorkspaceError> {
        let bad = |reason| WorkspaceError::BadSetting {
            key: key.to_string(),
            reason,
        };
        if !is_key(key) {
            return Err(bad(
                "a key is 1 to 8 segments of lowercase letters, digits and `_`, each starting \
                 with a letter and at most 64 bytes",
            ));
        }
        if key == VERSION_KEY || key.starts_with("version.") {
            return Err(bad("`version` is the file's format"));
        }
        if self.settings.contains_key(key) {
            return Err(bad("it is declared already"));
        }
        let prefix_of = |longer: &str, shorter: &str| {
            longer.len() > shorter.len()
                && longer.starts_with(shorter)
                && longer.as_bytes()[shorter.len()] == b'.'
        };
        if self
            .settings
            .keys()
            .any(|other| prefix_of(other, key) || prefix_of(key, other))
        {
            return Err(bad(
                "it is a prefix of another setting's key, or has one as its prefix",
            ));
        }
        match &kind {
            Kind::Integer { default, min, max } if !(min..=max).contains(&default) => {
                return Err(bad("its default is outside its range"));
            }
            Kind::String { default, choices }
                if !choices.is_empty() && !choices.contains(default) =>
            {
                return Err(bad("its default is not one of its choices"));
            }
            _ => {}
        }
        self.settings.insert(key, kind);
        Ok(())
    }

    /// The kind of the setting `key`, if it is declared.
    pub fn kind(&self, key: &str) -> Option<&Kind> {
        self.settings.get(key)
    }

    /// The declared keys, sorted.
    pub fn keys(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.settings.keys().copied()
    }
}

/// Whether `key` is one: 1 to [`MAX_KEY_SEGMENTS`] segments joined by `.`,
/// each of lowercase ASCII letters, digits and `_`, starting with a
/// letter, and at most [`MAX_SEGMENT_BYTES`].
pub fn is_key(key: &str) -> bool {
    let mut segments = 0;
    for segment in key.split('.') {
        segments += 1;
        let mut bytes = segment.bytes();
        let starts = bytes.next().is_some_and(|byte| byte.is_ascii_lowercase());
        if !starts
            || segment.len() > MAX_SEGMENT_BYTES
            || !bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return false;
        }
    }
    segments <= MAX_KEY_SEGMENTS
}

/// The layer a value comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Source {
    /// The schema's default.
    Default,
    /// The user's settings file.
    User,
    /// The workspace's settings file.
    Workspace,
}

impl fmt::Display for Source {
    /// "the default", "the user's setting" or "the workspace's setting".
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Source::Default => "the default",
            Source::User => "the user's setting",
            Source::Workspace => "the workspace's setting",
        })
    }
}

/// Which settings file a [`SettingsFile`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scope {
    /// The user's, in the user config folder.
    User,
    /// A workspace folder's, `.tidepool/settings.toml`.
    Workspace,
}

impl Scope {
    /// The layer this file's values come from.
    fn source(self) -> Source {
        match self {
            Scope::User => Source::User,
            Scope::Workspace => Source::Workspace,
        }
    }
}

/// What a settings file holds.
#[derive(Clone, Debug)]
enum State {
    /// There is no file.
    Missing,
    /// A file of [`VERSION`], or none, as a document to edit.
    Read(Document),
    /// A file that cannot be used, and why.
    Unusable(Arc<WorkspaceError>),
}

/// One settings file: its path, which layer it is, and what it holds.
#[derive(Clone, Debug)]
pub struct SettingsFile {
    scope: Scope,
    path: PathBuf,
    /// For a workspace's file, its folder's `.tidepool/`, which must be a
    /// folder and not a symbolic link.
    config_dir: Option<PathBuf>,
    /// Whether a workspace's file came from a folder a [`TrustStore`]
    /// trusts; always true for the user's.
    trusted: bool,
    state: State,
}

impl SettingsFile {
    /// Reads the user's settings: [`FILE_NAME`] in `config_dir`, the user
    /// config folder (`platform::paths::user_dirs`, on Linux
    /// `$XDG_CONFIG_HOME/tidepool`). A link to the file is followed, as
    /// dotfile managers make them. It never fails: a file that cannot be
    /// used is kept with its [`SettingsFile::problem`].
    ///
    /// It blocks on the file system, so call it on a worker.
    pub fn user(config_dir: &Path) -> SettingsFile {
        let path = config_dir.join(FILE_NAME);
        let state = match read::bounded(&path, MAX_FILE_BYTES, Links::Follow) {
            Ok(bytes) => state_of(bytes.as_deref(), &path),
            Err(error) => State::Unusable(Arc::new(error)),
        };
        SettingsFile {
            scope: Scope::User,
            path,
            config_dir: None,
            trusted: true,
            state,
        }
    }

    /// Reads `folder`'s settings, `.tidepool/settings.toml`, if `trust`
    /// trusts the folder; `None`, without reading anything, if it does not.
    /// The file is read only there: if `.tidepool` or the file is a
    /// symbolic link, or `.tidepool` is not a folder, it is not read, and
    /// its problem is [`WorkspaceError::Linked`]. It never fails otherwise:
    /// a file that cannot be used is kept with its
    /// [`SettingsFile::problem`].
    ///
    /// It blocks on the file system, so call it on a worker.
    pub fn workspace(folder: &Folder, trust: &TrustStore) -> Option<SettingsFile> {
        if !trust.is_trusted(folder) {
            return None;
        }
        let config_dir = folder.config_dir();
        let path = folder.settings_file();
        let state = match read_workspace(&config_dir, &path) {
            Ok(bytes) => state_of(bytes.as_deref(), &path),
            Err(error) => State::Unusable(Arc::new(error)),
        };
        Some(SettingsFile {
            scope: Scope::Workspace,
            path,
            config_dir: Some(config_dir),
            trusted: true,
            state,
        })
    }

    /// `folder`'s settings file holding `bytes` (none if `None`), if
    /// `trust` trusts the folder, without reading anything: for a caller
    /// that read the file itself, such as a reload when it changes (M4.5).
    /// `None` if the folder is not trusted. [`SettingsFile::write`] on it
    /// writes the folder's `.tidepool/settings.toml`.
    pub fn workspace_from_bytes(
        folder: &Folder,
        trust: &TrustStore,
        bytes: Option<&[u8]>,
    ) -> Option<SettingsFile> {
        if !trust.is_trusted(folder) {
            return None;
        }
        let path = folder.settings_file();
        let state = state_of(bytes, &path);
        Some(SettingsFile {
            scope: Scope::Workspace,
            path,
            config_dir: Some(folder.config_dir()),
            trusted: true,
            state,
        })
    }

    /// A settings file of `scope` at `path` that holds `bytes`, or none if
    /// `bytes` is `None`, without reading or writing anything. It never
    /// panics: bytes that are not a settings file give one that cannot be
    /// used. A workspace's file made this way is not trusted:
    /// [`Settings::resolve`] does not use it and [`SettingsFile::write`]
    /// refuses it, so a project's settings apply only through
    /// [`SettingsFile::workspace`] or [`SettingsFile::workspace_from_bytes`],
    /// which check its trust.
    pub fn from_bytes(scope: Scope, path: impl Into<PathBuf>, bytes: Option<&[u8]>) -> Self {
        let path = path.into();
        let state = state_of(bytes, &path);
        SettingsFile {
            scope,
            path,
            config_dir: None,
            trusted: scope == Scope::User,
            state,
        }
    }

    /// Whether the file may be used: the user's, or a workspace's that a
    /// [`TrustStore`] trusted when it was made.
    pub fn is_trusted(&self) -> bool {
        self.trusted
    }

    /// Which layer the file is.
    pub fn scope(&self) -> Scope {
        self.scope
    }

    /// The file's path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether there is a file.
    pub fn exists(&self) -> bool {
        !matches!(self.state, State::Missing)
    }

    /// The file's document, if there is a file that can be used.
    pub fn document(&self) -> Option<&Document> {
        match &self.state {
            State::Read(document) => Some(document),
            _ => None,
        }
    }

    /// Why the file cannot be used, if it cannot.
    pub fn problem(&self) -> Option<&WorkspaceError> {
        match &self.state {
            State::Unusable(problem) => Some(problem),
            _ => None,
        }
    }

    /// Sets the setting `key` to `value` in memory, as
    /// [`SettingsFile::write`] does on disk: through the document, keeping
    /// every other byte, and adding `version = 1` to a file without
    /// `version`. A missing file becomes one. It writes nothing.
    ///
    /// # Errors
    ///
    /// - [`WorkspaceError::UnknownSetting`] for a key `schema` does not
    ///   declare; [`WorkspaceError::NotWritable`] for a table;
    ///   [`WorkspaceError::InvalidValue`] for a value the setting does not
    ///   take.
    /// - [`WorkspaceError::Unwritable`] for a file that cannot be used.
    /// - [`WorkspaceError::Edit`] when the document refuses the edit: a
    ///   table on the key's way is a value of another kind, or the file
    ///   would be larger than [`MAX_FILE_BYTES`].
    ///
    /// The file is unchanged by an error.
    pub fn set(&mut self, schema: &Schema, key: &str, value: &Value) -> Result<(), WorkspaceError> {
        let value = writable(schema, key, value)?;
        let mut document = match &self.state {
            State::Missing => Document::parse("", LIMITS).map_err(|source| self.edit(source))?,
            State::Read(document) => document.clone(),
            State::Unusable(problem) => {
                return Err(WorkspaceError::Unwritable {
                    path: self.path.clone(),
                    reason: Arc::clone(problem),
                });
            }
        };
        if document.root().get(VERSION_KEY).is_none() {
            document
                .set(&[VERSION_KEY.into()], &ValueKind::Integer(VERSION))
                .map_err(|source| self.edit(source))?;
        }
        let steps: Vec<Step<'_>> = key.split('.').map(Step::Key).collect();
        document
            .set(&steps, &value)
            .map_err(|source| self.edit(source))?;
        self.state = State::Read(document);
        Ok(())
    }

    /// Sets the setting `key` to `value` in the file on disk: reads it
    /// again, sets the value as [`SettingsFile::set`] does, and saves it,
    /// making its folder if needed. The user's file is written through a
    /// link to it, as dotfile managers make them; a workspace's only at
    /// `.tidepool/settings.toml` itself, so that a setting never edits
    /// another of the project's files. The file then holds what is on disk.
    ///
    /// It blocks on the file system, so call it on a worker.
    ///
    /// # Errors
    ///
    /// As [`SettingsFile::set`], with [`WorkspaceError::Unwritable`] for a
    /// file on disk that cannot be used; [`WorkspaceError::Untrusted`] for
    /// a workspace's file that no [`TrustStore`] trusted;
    /// [`WorkspaceError::Linked`] for a workspace's file, or its
    /// `.tidepool`, that is a symbolic link, or a `.tidepool` that is not a
    /// folder; [`WorkspaceError::CreateFolder`] and
    /// [`WorkspaceError::Save`] when it cannot be written. After an error
    /// the file on disk is as it was (but for a [`WorkspaceError::Save`]
    /// at the directory's sync), and this holds what it says, once it was
    /// read again.
    pub fn write(
        &mut self,
        schema: &Schema,
        key: &str,
        value: &Value,
    ) -> Result<(), WorkspaceError> {
        writable(schema, key, value)?;
        let read = match (self.scope, &self.config_dir) {
            (Scope::User, _) => read::bounded(&self.path, MAX_FILE_BYTES, Links::Follow),
            (Scope::Workspace, Some(config_dir)) if self.trusted => {
                read_workspace(config_dir, &self.path)
            }
            (Scope::Workspace, _) => {
                return Err(WorkspaceError::Untrusted {
                    path: self.path.clone(),
                });
            }
        };
        self.state = match read {
            Ok(bytes) => state_of(bytes.as_deref(), &self.path),
            // A link stays refused, never replaced by a new file.
            Err(error @ WorkspaceError::Linked { .. }) => return Err(error),
            Err(error) => State::Unusable(Arc::new(error)),
        };
        let mut next = self.clone();
        next.set(schema, key, value)?;
        // `set` leaves a document whenever it succeeds.
        let Some(document) = next.document() else {
            return Ok(());
        };
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|source| WorkspaceError::CreateFolder {
                path: dir.to_path_buf(),
                source,
            })?;
        }
        tidepool_text::file::save(&self.path, &Rope::from(document.as_str()), Format::UTF8)
            .map_err(|source| WorkspaceError::Save {
                path: self.path.clone(),
                source,
            })?;
        *self = next;
        Ok(())
    }

    /// The error of an edit the document refused.
    fn edit(&self, source: tidepool_codec::CodecError) -> WorkspaceError {
        WorkspaceError::Edit {
            path: self.path.clone(),
            source,
        }
    }
}

/// `value` as the document's value for `key`, if the schema lets it be
/// written.
fn writable(schema: &Schema, key: &str, value: &Value) -> Result<ValueKind, WorkspaceError> {
    let kind = schema
        .kind(key)
        .ok_or_else(|| WorkspaceError::UnknownSetting {
            key: key.to_string(),
        })?;
    if matches!(kind, Kind::Table) {
        return Err(WorkspaceError::NotWritable {
            key: key.to_string(),
        });
    }
    kind.check(key, value)
        .map_err(|reason| WorkspaceError::InvalidValue {
            key: key.to_string(),
            reason,
        })?;
    Ok(match value {
        Value::Bool(value) => ValueKind::Boolean(*value),
        Value::Integer(value) => ValueKind::Integer(*value),
        Value::String(value) => ValueKind::String(value.clone()),
        Value::Table(table) => ValueKind::Table(table.clone()),
    })
}

/// What a file holding `bytes` (none if `None`) is, named `path` in errors.
fn state_of(bytes: Option<&[u8]>, path: &Path) -> State {
    let Some(bytes) = bytes else {
        return State::Missing;
    };
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_FILE_BYTES {
        return State::Unusable(Arc::new(WorkspaceError::TooLarge {
            path: path.to_path_buf(),
            limit: MAX_FILE_BYTES,
        }));
    }
    let document = match Document::parse_bytes(bytes, LIMITS) {
        Ok(document) => document,
        Err(source) => {
            return State::Unusable(Arc::new(WorkspaceError::Toml {
                path: path.to_path_buf(),
                source,
            }));
        }
    };
    match document.root().get(VERSION_KEY) {
        None => State::Read(document),
        Some(version) if version.as_integer() == Some(VERSION) => State::Read(document),
        Some(_) => State::Unusable(Arc::new(WorkspaceError::Version {
            path: path.to_path_buf(),
        })),
    }
}

/// The bytes of a workspace's settings file at `path`, in `config_dir`,
/// its folder's `.tidepool/`: `None` if either is missing, and
/// [`WorkspaceError::Linked`] if either is a symbolic link or `config_dir`
/// is not a folder. The folder's root is canonical, so the file read is
/// the folder's own.
fn read_workspace(config_dir: &Path, path: &Path) -> Result<Option<Vec<u8>>, WorkspaceError> {
    for (at, folder) in [(config_dir, true), (path, false)] {
        match std::fs::symlink_metadata(at) {
            Ok(metadata) if metadata.file_type().is_symlink() || (folder && !metadata.is_dir()) => {
                return Err(WorkspaceError::Linked {
                    path: at.to_path_buf(),
                });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(WorkspaceError::Read {
                    path: at.to_path_buf(),
                    source,
                });
            }
        }
    }
    read::bounded(path, MAX_FILE_BYTES, Links::Refuse)
}

/// A value [`Settings::resolve`] could not use, a table on keys' way that
/// is a value of another kind, or a file it could not use at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    file: PathBuf,
    about: About,
    message: String,
}

/// What a [`Warning`] is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum About {
    /// One setting's value, and the layer used instead.
    Value { key: &'static str, using: Source },
    /// A table on the way to settings, which fall back.
    Table,
    /// The whole file, none of whose values are used.
    File,
}

impl Warning {
    /// The file the value is in.
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// The setting whose value it is, or `None` for a warning about the
    /// whole file, or about a table on the way to several settings.
    pub fn key(&self) -> Option<&'static str> {
        match self.about {
            About::Value { key, .. } => Some(key),
            About::Table | About::File => None,
        }
    }

    /// What is wrong, such as "`ui.side_panel` is not `true` or `false`".
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The layer whose value is used instead, for a warning about one
    /// setting's value.
    pub fn using(&self) -> Option<Source> {
        match self.about {
            About::Value { using, .. } => Some(using),
            About::Table | About::File => None,
        }
    }
}

impl fmt::Display for Warning {
    /// One line: the file and what is wrong, then what is used instead.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let file = path::display(&self.file);
        match self.about {
            About::Value { using, .. } => {
                write!(f, "`{file}`: {}; using {using}", self.message)
            }
            About::Table => write!(
                f,
                "`{file}`: {}, so the settings in it fall back",
                self.message
            ),
            // The file's error names it.
            About::File => write!(f, "{}; none of its settings are used", self.message),
        }
    }
}

/// Every declared setting's value, from the layers.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    values: BTreeMap<&'static str, (Value, Source)>,
    tables: BTreeMap<&'static str, Vec<(Source, Table)>>,
    warnings: Vec<Warning>,
}

impl Settings {
    /// Gives every setting `schema` declares its value from the defaults,
    /// then `user`, then `workspace`, as the
    /// [module documentation](self#layers) says, with a [`Warning`] for
    /// each value or file that could not be used. It reads nothing, and
    /// never panics.
    ///
    /// `user` must be the user's file, and `workspace` a workspace's from
    /// [`SettingsFile::workspace`] or [`SettingsFile::workspace_from_bytes`],
    /// which give one only for a trusted folder. A file given in the wrong
    /// place, or a workspace's that is not trusted, gives none of its
    /// values, with a warning.
    pub fn resolve(
        schema: &Schema,
        user: Option<&SettingsFile>,
        workspace: Option<&SettingsFile>,
    ) -> Settings {
        let mut settings = Settings {
            values: BTreeMap::new(),
            tables: BTreeMap::new(),
            warnings: Vec::new(),
        };
        for (&key, kind) in &schema.settings {
            if let Some(value) = kind.default_value() {
                settings.values.insert(key, (value, Source::Default));
            }
        }
        for (file, scope) in [(user, Scope::User), (workspace, Scope::Workspace)] {
            let Some(file) = file else {
                continue;
            };
            let refused = if file.scope != scope {
                Some("is not the layer it was given as")
            } else if !file.trusted {
                Some("is not used, as no trusted folder gave it")
            } else {
                None
            };
            match refused {
                Some(why) => settings.warnings.push(Warning {
                    file: file.path.clone(),
                    about: About::File,
                    message: format!("`{}` {why}", path::display(&file.path)),
                }),
                None => settings.layer(schema, file),
            }
        }
        settings
    }

    /// Lays `file`'s values over the ones so far.
    fn layer(&mut self, schema: &Schema, file: &SettingsFile) {
        let document = match &file.state {
            State::Missing => return,
            State::Unusable(problem) => {
                self.warnings.push(Warning {
                    file: file.path.clone(),
                    about: About::File,
                    message: problem.to_string(),
                });
                return;
            }
            State::Read(document) => document,
        };
        let source = file.scope.source();
        // The tables on keys' ways that are values of another kind, each
        // warned about once.
        let mut not_tables: Vec<String> = Vec::new();
        for (&key, kind) in &schema.settings {
            let Some(found) = lookup(document.root(), key, &mut not_tables, &mut |prefix| {
                self.warnings.push(Warning {
                    file: file.path.clone(),
                    about: About::Table,
                    message: format!("`{}` is not a table", path::display(prefix)),
                });
            }) else {
                continue;
            };
            match kind.read(key, found) {
                Ok(Value::Table(table)) => {
                    self.tables
                        .entry(key)
                        .or_default()
                        .push((source, table.clone()));
                    self.values.insert(key, (Value::Table(table), source));
                }
                Ok(value) => {
                    self.values.insert(key, (value, source));
                }
                Err(message) => {
                    let using = self
                        .values
                        .get(key)
                        .map_or(Source::Default, |(_, below)| *below);
                    self.warnings.push(Warning {
                        file: file.path.clone(),
                        about: About::Value { key, using },
                        message,
                    });
                }
            }
        }
    }

    /// The value of the setting `key`, if it is declared (and, for a
    /// table, set in some layer).
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key).map(|(value, _)| value)
    }

    /// The layer the value of the setting `key` comes from.
    pub fn source(&self, key: &str) -> Option<Source> {
        self.values.get(key).map(|(_, source)| *source)
    }

    /// The value of the boolean setting `key`.
    pub fn bool(&self, key: &str) -> Option<bool> {
        match self.get(key) {
            Some(Value::Bool(value)) => Some(*value),
            _ => None,
        }
    }

    /// The value of the whole-number setting `key`.
    pub fn integer(&self, key: &str) -> Option<i64> {
        match self.get(key) {
            Some(Value::Integer(value)) => Some(*value),
            _ => None,
        }
    }

    /// The value of the string setting `key`.
    pub fn string(&self, key: &str) -> Option<&str> {
        match self.get(key) {
            Some(Value::String(value)) => Some(value),
            _ => None,
        }
    }

    /// The table of the setting `key` from the top layer that has a valid
    /// one.
    pub fn table(&self, key: &str) -> Option<&Table> {
        match self.get(key) {
            Some(Value::Table(table)) => Some(table),
            _ => None,
        }
    }

    /// Each layer's valid table for the setting `key`, the user's first,
    /// for an owner that merges them, as the keymap lays the workspace's
    /// bindings over the user's.
    pub fn table_layers(&self, key: &str) -> &[(Source, Table)] {
        self.tables.get(key).map_or(&[], Vec::as_slice)
    }

    /// The warnings, in the order of the layers and then of the keys.
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }

    /// Logs each warning at the warning level.
    pub fn log_warnings(&self) {
        for warning in &self.warnings {
            warn!("{warning}");
        }
    }
}

/// The value at `key` in `root`, following its segments through tables.
/// A segment's value that is not a table makes `warn` say so of its
/// prefix, once (`not_tables` keeps those warned of), and gives `None`.
fn lookup<'t>(
    root: &'t Table,
    key: &str,
    not_tables: &mut Vec<String>,
    warn: &mut dyn FnMut(&str),
) -> Option<&'t ValueKind> {
    let mut table = root;
    let mut segments = key.split('.').peekable();
    let mut end = 0;
    while let Some(segment) = segments.next() {
        end += segment.len();
        let value = &table.get(segment)?.kind;
        if segments.peek().is_none() {
            return Some(value);
        }
        match value {
            ValueKind::Table(inner) => table = inner,
            _ => {
                let prefix = &key[..end];
                if !not_tables.iter().any(|seen| seen == prefix) {
                    not_tables.push(prefix.to_string());
                    warn(prefix);
                }
                return None;
            }
        }
        end += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_lowercase_segments() {
        for key in ["ui", "ui.side_panel", "a1.b_2", "a.b.c.d.e.f.g.h"] {
            assert!(is_key(key), "{key}");
        }
        for key in [
            "",
            "UI",
            "ui.",
            ".ui",
            "ui..x",
            "1ui",
            "_ui",
            "ui.side-panel",
            "a.b.c.d.e.f.g.h.i",
            "ui.\u{e9}",
        ] {
            assert!(!is_key(key), "{key}");
        }
        assert!(is_key(&"a".repeat(64)));
        assert!(!is_key(&"a".repeat(65)));
    }

    #[test]
    fn declarations_that_are_refused() {
        let mut schema = Schema::new();
        schema
            .add("ui.side_panel", Kind::Bool { default: true })
            .unwrap();
        for (key, kind) in [
            ("ui.side_panel", Kind::Bool { default: true }),
            ("ui", Kind::Table),
            ("ui.side_panel.x", Kind::Table),
            ("version", Kind::Table),
            ("version.x", Kind::Table),
            ("Bad", Kind::Table),
            (
                "editor.tab_size",
                Kind::Integer {
                    default: 0,
                    min: 1,
                    max: 16,
                },
            ),
            (
                "editor.keymap",
                Kind::String {
                    default: "emacs",
                    choices: &["default", "vim"],
                },
            ),
        ] {
            assert!(
                matches!(
                    schema.add(key, kind),
                    Err(WorkspaceError::BadSetting { .. })
                ),
                "{key}"
            );
        }
        assert_eq!(schema.keys().collect::<Vec<_>>(), ["ui.side_panel"]);
        schema.add("ui.side", Kind::Bool { default: true }).unwrap();
    }
}
