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

//! Keymaps: which keys run which [commands](crate::command) in which
//! mode, read from TOML layers, and the modal [`Engine`] that resolves
//! keys as they go down (M4.6, ADR-0064, ADR-0006). Specification:
//! `docs/subsystems/editor.md` → "Commands and keymaps".
//!
//! New in Tidepool.
//!
//! # Modes
//!
//! A *mode* is a set of bindings that is active at a time, as Vim's Normal
//! and Insert modes are: the same key can run one command in one mode and
//! another in the next. A mode's name is ASCII letters and digits starting
//! with a lowercase letter, at most [`MAX_MODE_BYTES`] bytes
//! ([`is_mode_name`]), such as `insert`, `normal` or `visualLine`.
//! Tidepool's default keymap has one mode, `insert`; vim mode adds the
//! others from M7.2 (`docs/subsystems/vim-mode.md`).
//!
//! # The notation
//!
//! A binding's keys are one or more *steps*, separated by one space, and
//! pressed one after another: `ctrl+k ctrl+s` is Ctrl+K, then Ctrl+S. A
//! step is its modifiers and then one key, joined by `+`:
//!
//! - the modifiers are `ctrl`, `shift`, `alt` (Option on macOS) and
//!   `super` (the Windows key, or Command on macOS), each at most once, in
//!   any order;
//! - the key is named by its label on a US keyboard, unshifted: `a` to
//!   `z`, `0` to `9`, the punctuation by its character (`-`, `=`, `[`, `]`,
//!   `\`, `;`, `'`, `` ` ``, `,`, `.`, `/`), and the other keys by
//!   `core::input::Key`'s names in lowercase (`f1`, `enter`, `escape`,
//!   `space`, `pageup`, `numpad0`). A key is a place on the keyboard, so on
//!   any layout `ctrl+q` is the key right of Tab. A modifier key
//!   (`leftshift`, `rightcontrol`…) ends no step.
//!
//! Names are matched without regard to case, and a [`Sequence`] is shown
//! in one form, `Ctrl+Shift+P`, which reads back the same. A step matches
//! a key going down with exactly its modifiers held: `ctrl+p` is not the P
//! with Ctrl and Shift held. Bindings by typed character, such as `$` on
//! any layout, are vim mode's to add (M7.2).
//!
//! # The file
//!
//! A keymap file is TOML (ADR-0013), version [`VERSION`]: `version = 1`;
//! `start`, the mode the engine starts in, which the file must bind keys
//! in; and `keymap`, a table of modes, each a table of `"keys" = "command
//! id"`. A key the format does not have is refused, as is a file without
//! these three. The default keymap is `keymaps/default.toml`, compiled in
//! ([`DEFAULT_TOML`], [`Keymap::bundled`]):
//!
//! ```toml
//! version = 1
//! start = "insert"
//!
//! [keymap.insert]
//! "ctrl+shift+p" = "palette.open"
//! f1 = "palette.open"
//! "ctrl+b" = "view.toggleSidePanel"
//! "ctrl+j" = "view.toggleBottomPanel"
//! ```
//!
//! A binding names a registered command, or one of the engine's two:
//! `mode.enter.<mode>`, which enters a mode the keymap has, and
//! `mode.leave`, which leaves the mode entered last.
//!
//! # Layers
//!
//! A keymap is read in two [`Layer`]s: the default, from the file, and
//! the user's, a table of modes in the same form, which `settings.toml`
//! gives as its `[keymap.<mode>]` tables (the binary reads it, M4.6). A
//! user binding of keys the default binds in a mode replaces it, and an
//! empty string unbinds those keys in the layer below. The user's layer
//! may add modes, but not change `start`.
//!
//! # Problems
//!
//! Reading never fails for a binding: what is wrong with one is a
//! [`Problem`], kept in [`Keymap::problems`], and the binding is left out
//! or loses, as each says. Each names the layer, the mode and the keys as
//! the layer writes them. There are three kinds ([`ProblemKind`]):
//!
//! - **conflicts**: the same keys bound twice in one mode and layer
//!   (written differently, as `ctrl+b` and `Ctrl+B`, since TOML refuses a
//!   key twice), where the first in the file wins (one left out for an
//!   error does not count); and keys that start the keys of another
//!   binding in one mode, in either layer, where the shorter wins, so the
//!   longer one could never be reached;
//! - **overrides**: a user binding of keys a default binds, listed but not
//!   a conflict;
//! - **errors**: keys that do not read, a value that is not a string, a
//!   command the registry does not have, `mode.enter.<mode>` of a mode the
//!   keymap does not have, a mode whose name or table is not one, and a
//!   layer past its [`Limits`]; each is left out.
//!
//! # Limits
//!
//! Reading takes its [`Limits`]: the TOML parser's, and at most 4,096
//! bindings in a layer, 8 steps in a sequence and 32 modes in a layer, by
//! default. A layer past one is read up to it, with a problem. Every entry
//! of a layer counts toward its limits, one left out for an error too, and
//! is counted before it is looked at, so the problems are bounded by the
//! limits however large the layer's tables are: a layer gives at most
//! `max_modes + 1` problems about its modes and `2 × max_bindings + 1`
//! about its bindings (a user binding can override a default and lose to
//! shorter keys), so a keymap has at most `2 × (max_modes + 1) + 2 × (2 ×
//! max_bindings + 1)` problems, 16,452 at the defaults. Reading never
//! panics, and has a fuzz target, `keymap` (ADR-0027).
//!
//! # The engine
//!
//! The [`Engine`] keeps a stack of modes, starting with the keymap's start
//! mode, and the steps of a sequence it is waiting to finish. When a key
//! goes down, it looks up the pending steps and that key in the top mode's
//! bindings only, never in the modes below, so Insert mode's typed letters
//! cannot reach Normal mode's commands. Then ([`Resolution`]):
//!
//! - a whole binding resolves to its command, which the engine's owner
//!   runs by its id, or the engine runs, for its own two;
//! - the start of a binding waits for the next key, with no timeout;
//! - a key that continues no binding cancels the pending steps and is
//!   consumed, or, with none pending, resolves to nothing;
//! - a modifier key going down alone changes nothing, so Ctrl can be held
//!   down between `ctrl+k` and `ctrl+s`.
//!
//! An auto-repeat resolves like a press, and the result says it was one,
//! so the shell can ignore repeats of its window commands while editing
//! commands repeat (M4.2). A change of mode clears the pending steps.
//! `mode.leave` never leaves the last mode, and the stack holds at most
//! [`MAX_MODE_DEPTH`] modes.
//!
//! ```
//! use tidepool_core::input::{InputEvent, Key, Modifiers};
//! use tidepool_editor::command::Registry;
//! use tidepool_editor::keymap::{Engine, Keymap, Limits, Resolution};
//!
//! let mut registry = Registry::new();
//! registry.register("editor.save", "File: Save")?;
//! let text = r#"
//! version = 1
//! start = "normal"
//!
//! [keymap.normal]
//! i = "mode.enter.insert"
//! "ctrl+k ctrl+s" = "editor.save"
//!
//! [keymap.insert]
//! escape = "mode.leave"
//! "#;
//! let keymap = Keymap::read(text, None, &registry, Limits::DEFAULT)?;
//! assert!(keymap.problems().is_empty());
//! let mut engine = Engine::new(keymap);
//! let key = |key, modifiers| InputEvent::KeyDown { key, modifiers, repeat: false };
//!
//! assert_eq!(engine.handle(&key(Key::K, Modifiers::CTRL)), Resolution::Pending);
//! assert_eq!(
//!     engine.handle(&key(Key::S, Modifiers::CTRL)),
//!     Resolution::Command { id: "editor.save", repeat: false }
//! );
//! engine.handle(&key(Key::I, Modifiers::NONE));
//! assert_eq!(engine.mode(), "insert");
//! assert_eq!(engine.handle(&key(Key::K, Modifiers::CTRL)), Resolution::Unbound);
//! engine.handle(&key(Key::Escape, Modifiers::NONE));
//! assert_eq!(engine.mode(), "normal");
//! # Ok::<(), tidepool_editor::EditorError>(())
//! ```

mod engine;
mod notation;
mod read;

use std::error::Error;
use std::fmt;

use tidepool_codec::toml::Table;

use crate::command::{Registry, is_segment};
use crate::error::{EditorError, Shown};

pub use engine::{Engine, Resolution};
pub use notation::{NotationError, NotationErrorKind, Sequence, Step};

/// The version of the keymap format this module reads.
pub const VERSION: i64 = 1;

/// The longest mode name, in bytes.
pub const MAX_MODE_BYTES: usize = 32;

/// The most modes the [`Engine`]'s stack holds: entering one more changes
/// nothing.
pub const MAX_MODE_DEPTH: usize = 32;

/// The text of `keymaps/default.toml`, Tidepool's default keymap, compiled
/// in.
pub const DEFAULT_TOML: &str = include_str!("../../../keymaps/default.toml");

/// The TOML limits of [`Limits::DEFAULT`]: 1 MiB of input, the size of a
/// settings file (M3.4), 8 levels, strings of 4 KiB, numbers of 64 bytes,
/// and 4,096 entries a table, as many as a layer's bindings. A keymap is 2
/// levels deep at `keymap.<mode>.<keys>`.
const TOML_LIMITS: tidepool_codec::Limits = tidepool_codec::Limits {
    max_input_bytes: 1024 * 1024,
    max_depth: 8,
    max_string_bytes: 4096,
    max_number_bytes: 64,
    max_entries: 4096,
    ..tidepool_codec::Limits::DEFAULT
};

/// The bounds reading a keymap keeps to, so that a hostile or broken file
/// cannot take unbounded memory or time.
///
/// They bound the [`Problem`]s too: every entry of a layer counts toward
/// them before it is looked at, so a layer gives at most `max_modes + 1`
/// problems about its modes and `2 × max_bindings + 1` about its bindings,
/// and a keymap at most `2 × (max_modes + 1) + 2 × (2 × max_bindings + 1)`
/// ([module documentation](self#limits)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// What the TOML parser keeps to, for a keymap file's text
    /// ([`Keymap::read`]). A layer given as a table was parsed by its
    /// owner, with its own.
    pub toml: tidepool_codec::Limits,
    /// The most entries one layer reads as bindings, over all its modes,
    /// counting those left out for an error; the entries past it are not
    /// looked at.
    pub max_bindings: usize,
    /// The most steps keys may have.
    pub max_steps: usize,
    /// The most entries one layer reads as modes, counting those left out
    /// for a bad name or a value that is not a table; the entries past it
    /// are not looked at.
    pub max_modes: usize,
}

impl Limits {
    /// The defaults: the TOML limits of a settings file (1 MiB of input),
    /// 4,096 bindings a layer, 8 steps and 32 modes a layer.
    pub const DEFAULT: Self = Self {
        toml: TOML_LIMITS,
        max_bindings: 4096,
        max_steps: 8,
        max_modes: 32,
    };
}

impl Default for Limits {
    /// [`Limits::DEFAULT`].
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Whether `name` is a mode's name: ASCII letters and digits starting with
/// a lowercase letter, at most [`MAX_MODE_BYTES`] bytes.
///
/// ```
/// use tidepool_editor::keymap::is_mode_name;
///
/// assert!(is_mode_name("insert") && is_mode_name("visualLine"));
/// assert!(!is_mode_name("Insert") && !is_mode_name("visual.line"));
/// ```
pub fn is_mode_name(name: &str) -> bool {
    name.len() <= MAX_MODE_BYTES && is_segment(name)
}

/// Which layer of a keymap a binding is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Layer {
    /// The default keymap, from its file.
    Default,
    /// The user's bindings over it, from `settings.toml`.
    User,
}

/// `default` or `user`.
impl fmt::Display for Layer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Default => "default",
            Self::User => "user",
        })
    }
}

/// What a binding does, once read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    /// Runs the registered command its id names.
    Command,
    /// Enters the mode at this index of [`Keymap::modes`].
    Enter(usize),
    /// Leaves the mode entered last.
    Leave,
}

/// A binding of a [`Keymap`]: keys, and the command they run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    keys: Sequence,
    command: String,
    layer: Layer,
    /// The keys as the layer writes them.
    written: String,
    action: Action,
}

impl Binding {
    /// Its keys.
    pub fn keys(&self) -> &Sequence {
        &self.keys
    }

    /// The id of the command it runs: a registered command's, or one of
    /// the engine's (`mode.enter.<mode>`, `mode.leave`).
    pub fn command(&self) -> &str {
        &self.command
    }

    /// The layer it came from.
    pub const fn layer(&self) -> Layer {
        self.layer
    }
}

/// A mode of a keymap: its name, and its bindings, sorted by their keys,
/// none of them starting another's.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Mode {
    name: String,
    bindings: Vec<Binding>,
}

/// What the bindings of a mode hold for some steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lookup {
    /// The binding at this index has exactly those steps.
    Found(usize),
    /// A binding starts with them, and has more.
    Prefix,
    /// No binding starts with them.
    Nothing,
}

/// A keymap: its modes and their bindings, read from its layers, the mode
/// to start in, and the [`Problem`]s found while reading it (the [module
/// documentation](self)).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keymap {
    modes: Vec<Mode>,
    start: usize,
    problems: Vec<Problem>,
}

impl Keymap {
    /// Reads `text`, a keymap file, as the default layer, and `user`, if
    /// given, as the user's layer over it, a table of modes in the file's
    /// `keymap` form; checking each binding's command against `registry`,
    /// within `limits`. What is wrong with a binding is a problem, not an
    /// error ([`Keymap::problems`]).
    ///
    /// # Errors
    ///
    /// [`EditorError::KeymapSyntax`] when `text` is not TOML or breaks
    /// `limits.toml`; [`EditorError::Keymap`] when it is not a keymap of
    /// [`VERSION`]: a key the format does not have, or `version`, `start`
    /// or `keymap` missing or wrong, or a `start` mode the file binds no
    /// keys in.
    pub fn read(
        text: &str,
        user: Option<&Table>,
        registry: &Registry,
        limits: Limits,
    ) -> Result<Self, EditorError> {
        let root =
            tidepool_codec::toml::parse(text, limits.toml).map_err(EditorError::KeymapSyntax)?;
        read::keymap(&root, user, registry, limits).map_err(EditorError::Keymap)
    }

    /// The default keymap ([`DEFAULT_TOML`]), with `user`, if given, as the
    /// user's layer over it, read within [`Limits::DEFAULT`]. The bindings
    /// of commands `registry` does not have are left out, as problems.
    pub fn bundled(registry: &Registry, user: Option<&Table>) -> Self {
        Self::read(DEFAULT_TOML, user, registry, Limits::DEFAULT)
            .expect("the default keymap is a keymap: `editor`'s tests read it")
    }

    /// The mode the engine starts in.
    pub fn start(&self) -> &str {
        self.modes.get(self.start).map_or("", |mode| &mode.name)
    }

    /// Its modes' names: the default layer's in its order, then the ones
    /// the user's layer adds.
    pub fn modes(&self) -> impl Iterator<Item = &str> {
        self.modes.iter().map(|mode| mode.name.as_str())
    }

    /// Whether it has the mode `mode`.
    pub fn has_mode(&self, mode: &str) -> bool {
        self.mode_index(mode).is_some()
    }

    /// The bindings of `mode`, sorted by their keys; none for a mode it
    /// does not have.
    pub fn bindings(&self, mode: &str) -> &[Binding] {
        self.mode_index(mode)
            .and_then(|at| self.modes.get(at))
            .map_or(&[], |mode| mode.bindings.as_slice())
    }

    /// What reading it found wrong, in the order found: at most
    /// `2 × (max_modes + 1) + 2 × (2 × max_bindings + 1)` of the [`Limits`]
    /// it was read with, however large its layers' tables were.
    pub fn problems(&self) -> &[Problem] {
        &self.problems
    }

    /// Where `mode` is in [`Keymap::modes`].
    fn mode_index(&self, mode: &str) -> Option<usize> {
        self.modes.iter().position(|known| known.name == mode)
    }

    /// The name of the mode at `at`.
    fn mode_name(&self, at: usize) -> &str {
        self.modes.get(at).map_or("", |mode| &mode.name)
    }

    /// What the bindings of the mode at `mode` hold for `steps`.
    fn lookup(&self, mode: usize, steps: &[Step]) -> Lookup {
        let Some(mode) = self.modes.get(mode) else {
            return Lookup::Nothing;
        };
        match mode
            .bindings
            .binary_search_by(|binding| binding.keys.steps().cmp(steps))
        {
            Ok(at) => Lookup::Found(at),
            // Sorted, so the first binding after `steps` is the first that
            // could start with them.
            Err(at) => {
                if mode
                    .bindings
                    .get(at)
                    .is_some_and(|binding| binding.keys.steps().starts_with(steps))
                {
                    Lookup::Prefix
                } else {
                    Lookup::Nothing
                }
            }
        }
    }
}

/// Where a binding is written: its layer, its mode and its keys, as the
/// layer writes them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    /// Its layer.
    pub layer: Layer,
    /// Its mode's name.
    pub mode: String,
    /// Its keys, as written.
    pub keys: String,
}

/// `user layer, [keymap.insert] `ctrl+b``, with the mode and keys escaped
/// and cut as [`EditorError`]'s messages are.
impl fmt::Display for Place {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} layer, [keymap.{}] `{}`",
            self.layer,
            Shown(&self.mode),
            Shown(&self.keys)
        )
    }
}

/// Which kind a [`Problem`] is, which says how loudly to report it: the
/// binary logs a conflict or an error as a warning, and an override at
/// info.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProblemKind {
    /// Two bindings that cannot both work; one of them wins.
    Conflict,
    /// A user binding in place of a default one, as asked.
    Override,
    /// A binding, a mode or a part of a layer left out.
    Error,
}

/// Something wrong, or worth listing, in a keymap's layers (the [module
/// documentation](self#problems)).
///
/// Its `Display` names the layer, the mode and the keys, escaped and cut
/// as [`EditorError`]'s messages are.
///
/// Non-exhaustive: later versions may add variants.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Problem {
    /// A conflict: two bindings of the same keys in one mode and layer,
    /// written differently. The first in the layer is kept, and the second
    /// left out.
    Duplicate {
        /// The binding kept.
        kept: Place,
        /// The binding left out.
        dropped: Place,
    },
    /// A conflict: `kept`'s keys start `dropped`'s, in one mode. The
    /// shorter wins, so the longer could never be reached, and is left
    /// out.
    Prefix {
        /// The shorter binding, kept.
        kept: Place,
        /// The longer one, left out.
        dropped: Place,
    },
    /// An override: a user binding of keys a default binding binds, which
    /// it replaces, or unbinds.
    Override {
        /// The default binding, replaced.
        default: Place,
        /// The user's binding.
        user: Place,
        /// Whether the user's binding is an empty string, which unbinds the
        /// keys.
        unbinds: bool,
    },
    /// An error: a value that is not a string. Left out.
    NotAString {
        /// The binding.
        at: Place,
    },
    /// An error: keys that do not read in the notation. Left out.
    BadKeys {
        /// The binding.
        at: Place,
        /// Why its keys do not read.
        error: NotationError,
    },
    /// An error: a command the registry does not have, or an engine
    /// command that is not `mode.leave` or `mode.enter.<mode>`. Left out.
    UnknownCommand {
        /// The binding.
        at: Place,
        /// The command, as written.
        command: String,
    },
    /// An error: `mode.enter.<mode>` of a mode the keymap does not have.
    /// Left out.
    UnknownMode {
        /// The binding.
        at: Place,
        /// The mode, as written.
        mode: String,
    },
    /// An error: a mode's name that is not one ([`is_mode_name`]). Its
    /// table is left out.
    BadModeName {
        /// The layer.
        layer: Layer,
        /// The name, as written.
        mode: String,
    },
    /// An error: a mode whose value is not a table of bindings. Left out.
    ModeNotATable {
        /// The layer.
        layer: Layer,
        /// The mode.
        mode: String,
    },
    /// An error: a layer whose table of modes has more entries than
    /// [`Limits::max_modes`]. The entries past the limit are left out, and
    /// not looked at.
    TooManyModes {
        /// The layer.
        layer: Layer,
        /// The limit.
        max: usize,
    },
    /// An error: a layer whose modes' tables have more entries than
    /// [`Limits::max_bindings`]. The entries past the limit, and the modes
    /// after them, are left out, and not looked at.
    TooManyBindings {
        /// The layer.
        layer: Layer,
        /// The limit.
        max: usize,
    },
}

impl Problem {
    /// Which kind it is.
    pub const fn kind(&self) -> ProblemKind {
        match self {
            Self::Duplicate { .. } | Self::Prefix { .. } => ProblemKind::Conflict,
            Self::Override { .. } => ProblemKind::Override,
            Self::NotAString { .. }
            | Self::BadKeys { .. }
            | Self::UnknownCommand { .. }
            | Self::UnknownMode { .. }
            | Self::BadModeName { .. }
            | Self::ModeNotATable { .. }
            | Self::TooManyModes { .. }
            | Self::TooManyBindings { .. } => ProblemKind::Error,
        }
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Duplicate { kept, dropped } => write!(
                f,
                "{dropped}: the same keys as `{}` before it, which wins; left out",
                Shown(&kept.keys)
            ),
            Self::Prefix { kept, dropped } => write!(
                f,
                "{dropped}: starts with the keys of {kept}, which wins, so it could never be \
                 reached; left out"
            ),
            Self::Override {
                default,
                user,
                unbinds: true,
            } => write!(f, "{user}: unbinds {default}"),
            Self::Override { default, user, .. } => write!(f, "{user}: overrides {default}"),
            Self::NotAString { at } => write!(
                f,
                "{at}: the value is not a string naming a command; left out"
            ),
            Self::BadKeys { at, error } => write!(f, "{at}: {error}; left out"),
            Self::UnknownCommand { at, command } => write!(
                f,
                "{at}: no command is named `{}`; left out",
                Shown(command)
            ),
            Self::UnknownMode { at, mode } => write!(
                f,
                "{at}: the keymap has no mode `{}` to enter; left out",
                Shown(mode)
            ),
            Self::BadModeName { layer, mode } => write!(
                f,
                "{layer} layer, [keymap] `{}`: not a mode's name (ASCII letters and digits, \
                 starting with a lowercase letter, at most {MAX_MODE_BYTES} bytes); left out",
                Shown(mode)
            ),
            Self::ModeNotATable { layer, mode } => write!(
                f,
                "{layer} layer, [keymap] `{}`: not a table of bindings; left out",
                Shown(mode)
            ),
            Self::TooManyModes { layer, max } => write!(
                f,
                "{layer} layer: more than {max} modes; the rest are left out"
            ),
            Self::TooManyBindings { layer, max } => write!(
                f,
                "{layer} layer: more than {max} bindings; the rest are left out"
            ),
        }
    }
}

/// A keymap file that is TOML but not a keymap of [`VERSION`]
/// ([`EditorError::Keymap`]).
///
/// Non-exhaustive: later versions may add variants.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FileError {
    /// `version` is missing, or is not an integer.
    MissingVersion,
    /// `version` is a version this module does not read.
    UnsupportedVersion(i64),
    /// `start` is missing, not a string, or not a mode's name.
    BadStart,
    /// `start` names a mode the file has no table of bindings for that
    /// reads.
    UnknownStart(String),
    /// `keymap` is missing, or is not a table.
    MissingModes,
    /// A key at the file's top level that the format does not have.
    UnknownKey(String),
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingVersion => write!(
                f,
                "the keymap has no `version`: it needs `version = {VERSION}`"
            ),
            Self::UnsupportedVersion(version) => write!(
                f,
                "the keymap's `version` is {version}, which is not supported: this Tidepool \
                 reads version {VERSION}"
            ),
            Self::BadStart => f.write_str(
                "the keymap's `start` is missing or not a mode's name (ASCII letters and \
                 digits, starting with a lowercase letter)",
            ),
            Self::UnknownStart(mode) => write!(
                f,
                "the keymap starts in `{}`, which it has no `[keymap.{}]` table for",
                Shown(mode),
                Shown(mode)
            ),
            Self::MissingModes => f.write_str("the keymap has no `keymap` table of modes"),
            Self::UnknownKey(key) => write!(
                f,
                "the keymap has `{}`, which is not a key of the format (`version`, `start` \
                 and `keymap`)",
                Shown(key)
            ),
        }
    }
}

impl Error for FileError {}
