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

//! Reading a keymap's two layers into a [`Keymap`], with its
//! [`Problem`]s, as the [`keymap`](super) module documentation says.
//!
//! New in Tidepool.
//!
//! It reads in three passes. The first collects each layer's modes, so
//! that `mode.enter.<mode>` can name a mode either layer has. The second
//! reads each layer's bindings, leaving out what does not read and the
//! second of two bindings of the same keys. The third lays the user's
//! layer over the default one, mode by mode, and then leaves out each
//! binding whose keys start with another's, the shorter winning.

use std::collections::{BTreeMap, HashMap};

use tidepool_codec::toml::{Table, Value};

use super::{
    Action, Binding, FileError, Keymap, Layer, Limits, Mode, Place, Problem, Sequence, VERSION,
    is_mode_name,
};
use crate::command::{Registry, is_engine_id};

/// The keys of a keymap file's top level.
const FILE_KEYS: [&str; 3] = ["version", "start", "keymap"];

/// Reads a keymap file's tree, `root`, as the default layer, and `user` as
/// the user's layer over it.
///
/// # Errors
///
/// A [`FileError`] when `root` is not a keymap of [`VERSION`].
pub(super) fn keymap(
    root: &Table,
    user: Option<&Table>,
    registry: &Registry,
    limits: Limits,
) -> Result<Keymap, FileError> {
    if let Some((key, _)) = root
        .entries()
        .iter()
        .find(|(key, _)| !FILE_KEYS.contains(&key.name.as_str()))
    {
        return Err(FileError::UnknownKey(key.name.clone()));
    }
    match root.get("version").map(Value::as_integer) {
        None | Some(None) => return Err(FileError::MissingVersion),
        Some(Some(VERSION)) => {}
        Some(Some(version)) => return Err(FileError::UnsupportedVersion(version)),
    }
    let start = root
        .get("start")
        .and_then(Value::as_str)
        .filter(|start| is_mode_name(start))
        .ok_or(FileError::BadStart)?;
    let default = root
        .get("keymap")
        .and_then(Value::as_table)
        .ok_or(FileError::MissingModes)?;

    let mut reader = Reader {
        registry,
        limits,
        names: Vec::new(),
        problems: [Vec::new(), Vec::new()],
    };
    let default_modes = reader.modes(Layer::Default, default);
    let start = reader
        .names
        .iter()
        .position(|name| name == start)
        .ok_or_else(|| FileError::UnknownStart(start.to_string()))?;
    let user_modes = user
        .map(|user| reader.modes(Layer::User, user))
        .unwrap_or_default();
    let default_bindings = reader.bindings(Layer::Default, &default_modes);
    let user_bindings = reader.bindings(Layer::User, &user_modes);
    Ok(reader.merge(start, default_bindings, user_bindings))
}

/// The state of a read.
struct Reader<'r> {
    registry: &'r Registry,
    limits: Limits,
    /// The modes of both layers, the default layer's first, in the order
    /// the layers give them.
    names: Vec<String>,
    /// The problems found in each layer, the default layer's first.
    problems: [Vec<Problem>; 2],
}

/// A mode's table of bindings in a layer: the mode's index in
/// [`Reader::names`], and the table.
struct ModeTable<'t> {
    mode: usize,
    table: &'t Table,
}

/// A binding of a layer that read: its keys, read and as written, and its
/// command's id and what it does; no command for an empty string, which
/// unbinds the keys.
struct Read {
    keys: Sequence,
    written: String,
    command: Option<(String, Action)>,
}

/// The bindings a layer read for one mode: the mode's index in
/// [`Reader::names`], and the bindings, in the layer's order.
type ModeBindings = (usize, Vec<Read>);

impl Reader<'_> {
    /// Adds `problem`, found in `layer`.
    fn problem(&mut self, layer: Layer, problem: Problem) {
        let at = match layer {
            Layer::Default => 0,
            Layer::User => 1,
        };
        if let Some(problems) = self.problems.get_mut(at) {
            problems.push(problem);
        }
    }

    /// The first pass: `layer`'s mode tables, its modes added to
    /// [`Reader::names`]. A mode whose name or table is not one is left
    /// out, and so are the entries past the limit. Every entry counts
    /// toward the limit, checked before the entry is looked at, so a layer
    /// gives at most `max_modes + 1` problems here, however many entries
    /// its table has.
    fn modes<'t>(&mut self, layer: Layer, table: &'t Table) -> Vec<ModeTable<'t>> {
        let mut modes = Vec::new();
        for (count, (key, value)) in table.entries().iter().enumerate() {
            if count >= self.limits.max_modes {
                let max = self.limits.max_modes;
                self.problem(layer, Problem::TooManyModes { layer, max });
                break;
            }
            let name = key.name.as_str();
            if !is_mode_name(name) {
                let mode = name.to_string();
                self.problem(layer, Problem::BadModeName { layer, mode });
                continue;
            }
            let Some(bindings) = value.as_table() else {
                let mode = name.to_string();
                self.problem(layer, Problem::ModeNotATable { layer, mode });
                continue;
            };
            let mode = match self.names.iter().position(|known| known == name) {
                Some(at) => at,
                None => {
                    self.names.push(name.to_string());
                    self.names.len() - 1
                }
            };
            modes.push(ModeTable {
                mode,
                table: bindings,
            });
        }
        modes
    }

    /// The second pass: `layer`'s bindings in each of its `modes`. A binding
    /// that does not read is left out, and so is the second of two of the
    /// same keys, and every binding past the limit. Every entry of a mode's
    /// table counts toward the layer's limit, checked before the entry is
    /// looked at, so a layer gives at most `max_bindings + 1` problems here.
    fn bindings(&mut self, layer: Layer, modes: &[ModeTable<'_>]) -> Vec<ModeBindings> {
        let mut count = 0usize;
        let mut layer_bindings = Vec::with_capacity(modes.len());
        'modes: for mode_table in modes {
            let name = self.names.get(mode_table.mode).cloned().unwrap_or_default();
            let mut read: Vec<Read> = Vec::new();
            let mut seen: HashMap<Sequence, usize> = HashMap::new();
            for (key, value) in mode_table.table.entries() {
                if count >= self.limits.max_bindings {
                    let max = self.limits.max_bindings;
                    self.problem(layer, Problem::TooManyBindings { layer, max });
                    layer_bindings.push((mode_table.mode, read));
                    break 'modes;
                }
                count += 1;
                let at = Place {
                    layer,
                    mode: name.clone(),
                    keys: key.name.clone(),
                };
                let keys = match Sequence::parse(&key.name, self.limits) {
                    Ok(keys) => keys,
                    Err(error) => {
                        self.problem(layer, Problem::BadKeys { at, error });
                        continue;
                    }
                };
                let Some(target) = value.as_str() else {
                    self.problem(layer, Problem::NotAString { at });
                    continue;
                };
                let command = if target.is_empty() {
                    None
                } else {
                    match self.action(target) {
                        Ok(action) => Some((target.to_string(), action)),
                        Err(Unknown::Command) => {
                            let command = target.to_string();
                            self.problem(layer, Problem::UnknownCommand { at, command });
                            continue;
                        }
                        Err(Unknown::Mode(mode)) => {
                            let mode = mode.to_string();
                            self.problem(layer, Problem::UnknownMode { at, mode });
                            continue;
                        }
                    }
                };
                if let Some(first) = seen.get(&keys).and_then(|&first| read.get(first)) {
                    let kept = Place {
                        layer,
                        mode: name.clone(),
                        keys: first.written.clone(),
                    };
                    self.problem(layer, Problem::Duplicate { kept, dropped: at });
                    continue;
                }
                seen.insert(keys.clone(), read.len());
                read.push(Read {
                    keys,
                    written: key.name.clone(),
                    command,
                });
            }
            layer_bindings.push((mode_table.mode, read));
        }
        layer_bindings
    }

    /// What the binding of `command` does, a command's id that is not empty.
    fn action<'c>(&self, command: &'c str) -> Result<Action, Unknown<'c>> {
        if command == "mode.leave" {
            return Ok(Action::Leave);
        }
        if let Some(mode) = command.strip_prefix("mode.enter.") {
            return self
                .names
                .iter()
                .position(|known| known == mode)
                .map(Action::Enter)
                .ok_or(Unknown::Mode(mode));
        }
        if !is_engine_id(command) && self.registry.contains(command) {
            Ok(Action::Command)
        } else {
            Err(Unknown::Command)
        }
    }

    /// The third pass: the user's bindings over the default ones in each
    /// mode, and then the bindings whose keys start with another's left
    /// out.
    fn merge(self, start: usize, default: Vec<ModeBindings>, user: Vec<ModeBindings>) -> Keymap {
        let [mut problems, user_problems] = self.problems;
        problems.extend(user_problems);
        let mut maps: Vec<BTreeMap<Sequence, Binding>> = vec![BTreeMap::new(); self.names.len()];
        for (mode, reads) in default {
            let Some(map) = maps.get_mut(mode) else {
                continue;
            };
            for read in reads {
                // An empty string unbinds what the layers below bind, and
                // the default layer has none below it.
                if let Some((command, action)) = read.command {
                    let binding = Binding {
                        keys: read.keys.clone(),
                        command,
                        layer: Layer::Default,
                        written: read.written,
                        action,
                    };
                    map.insert(read.keys, binding);
                }
            }
        }
        for (mode, reads) in user {
            let (Some(map), Some(name)) = (maps.get_mut(mode), self.names.get(mode)) else {
                continue;
            };
            for read in reads {
                if let Some(default) = map.get(&read.keys) {
                    problems.push(Problem::Override {
                        default: place(default, name),
                        user: Place {
                            layer: Layer::User,
                            mode: name.clone(),
                            keys: read.written.clone(),
                        },
                        unbinds: read.command.is_none(),
                    });
                }
                match read.command {
                    None => {
                        map.remove(&read.keys);
                    }
                    Some((command, action)) => {
                        let binding = Binding {
                            keys: read.keys.clone(),
                            command,
                            layer: Layer::User,
                            written: read.written,
                            action,
                        };
                        map.insert(read.keys, binding);
                    }
                }
            }
        }
        let modes = self
            .names
            .into_iter()
            .zip(maps)
            .map(|(name, map)| {
                let mut bindings: Vec<Binding> = Vec::with_capacity(map.len());
                // In order, a binding comes just before those its keys
                // start, so each is checked against the last one kept.
                for binding in map.into_values() {
                    if let Some(kept) = bindings.last()
                        && binding.keys.is_extension_of(&kept.keys)
                    {
                        problems.push(Problem::Prefix {
                            kept: place(kept, &name),
                            dropped: place(&binding, &name),
                        });
                        continue;
                    }
                    bindings.push(binding);
                }
                Mode { name, bindings }
            })
            .collect();
        Keymap {
            modes,
            start,
            problems,
        }
    }
}

/// Why a binding's command is not one.
enum Unknown<'c> {
    /// No command has this id.
    Command,
    /// `mode.enter.<mode>` of a mode the keymap does not have: the mode.
    Mode(&'c str),
}

/// Where `binding`, of the mode `mode`, is written.
fn place(binding: &Binding, mode: &str) -> Place {
    Place {
        layer: binding.layer,
        mode: mode.to_string(),
        keys: binding.written.clone(),
    }
}
