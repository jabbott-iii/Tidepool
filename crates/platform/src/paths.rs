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

//! The folders where Tidepool keeps what it writes for the user: its
//! settings, data, cache and logs ([`user_dirs`]).
//!
//! Ported in part from nadir-platform's `paths`: the folders as a struct of
//! paths, found without making any. Adapted for Tidepool: the name is
//! Tidepool's own, not a caller's; there is a cache folder; and the logs are
//! a folder of their own in the data folder on Linux (ADR-0042). The
//! standard folders of outside tools are in [`dirs`](crate::dirs).

use std::path::PathBuf;

use crate::{Error, backend};

/// Tidepool's folders for one user.
///
/// They are paths only: [`user_dirs`] makes none of them. Make a folder
/// before writing into it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserDirs {
    /// Settings, keymaps and themes the user writes.
    pub config: PathBuf,
    /// What Tidepool keeps between runs: the session, trust, the state of
    /// the managed tools.
    pub data: PathBuf,
    /// What can be made again, and may be deleted at any time.
    pub cache: PathBuf,
    /// Log files.
    pub logs: PathBuf,
}

/// Finds Tidepool's folders for the user who runs it.
///
/// | System | Config | Data | Cache | Logs |
/// | :- | :- | :- | :- | :- |
/// | Linux | `$XDG_CONFIG_HOME/tidepool` | `$XDG_DATA_HOME/tidepool` | `$XDG_CACHE_HOME/tidepool` | `$XDG_DATA_HOME/tidepool/logs` |
///
/// On Linux an `XDG_` variable that is unset, empty or not an absolute path
/// counts as unset (XDG Base Directory 0.8), and its default applies:
/// `~/.config`, `~/.local/share` and `~/.cache`. The home directory is
/// `HOME`, or the user database's when `HOME` is unset or empty, and is
/// looked up only if a default needs it. The user database can be a
/// network service (NSS), so call this once, at startup, or on a worker.
///
/// # Errors
///
/// - [`Error::NoHomeDirectory`] on Linux, if a default needs the home
///   directory and it is not known, or not an absolute path.
/// - [`Error::Unsupported`] on every other system, until macOS (M11.8)
///   and Windows (M12.2).
pub fn user_dirs() -> Result<UserDirs, Error> {
    backend::user_dirs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn found_folders_are_absolute_and_tidepools() {
        // Whatever this machine's environment is, a folder found is
        // absolute and Tidepool's own.
        if let Ok(dirs) = user_dirs() {
            for path in [&dirs.config, &dirs.data, &dirs.cache, &dirs.logs] {
                assert!(path.is_absolute(), "{}", path.display());
            }
            for path in [&dirs.config, &dirs.data, &dirs.cache] {
                assert!(path.ends_with("tidepool"), "{}", path.display());
            }
        }
    }
}
