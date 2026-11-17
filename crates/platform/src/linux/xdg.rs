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

//! The user's and the tools' folders on Linux, from the XDG Base Directory
//! variables and the tools' own (`docs/03-platforms.md` → Paths, ADR-0042).
//!
//! Ported from nadir-platform `linux/xdg.rs`: [`resolve`] with an
//! environment and a home directory passed in, so that the rules are tested
//! without the process's own, the rule for a variable ([`absolute`]), and
//! the tests of the defaults, the overrides, the ignored values and the
//! missing home. Adapted for Tidepool: the name is always `tidepool`, there
//! is a cache folder, and the logs are a folder in the data folder; the
//! tools' folders ([`resolve_tool`]) are new.

use std::env;
use std::ffi::OsString;
use std::path::PathBuf;

use crate::Error;
use crate::paths::UserDirs;

/// Tidepool's folder in each base folder.
const NAME: &str = "tidepool";

/// The folder of the logs in the data folder.
const LOGS: &str = "logs";

/// Finds Tidepool's folders from this process's environment.
pub(crate) fn user_dirs() -> Result<UserDirs, Error> {
    // `home_dir` reads `HOME`, and asks the user database if it is unset
    // or empty.
    resolve(|name| env::var_os(name), env::home_dir)
}

/// The Android SDK's folder, from this process's environment.
pub(crate) fn android_sdk() -> Result<PathBuf, Error> {
    resolve_tool(
        "ANDROID_HOME",
        "Android/Sdk",
        |name| env::var_os(name),
        env::home_dir,
    )
}

/// rustup's folder, from this process's environment.
pub(crate) fn rustup_home() -> Result<PathBuf, Error> {
    resolve_tool(
        "RUSTUP_HOME",
        ".rustup",
        |name| env::var_os(name),
        env::home_dir,
    )
}

/// Finds Tidepool's folders. `var` reads an environment variable, and
/// `home` finds the home directory; `home` is called only if a default
/// needs it, and at most once.
fn resolve(
    var: impl Fn(&str) -> Option<OsString>,
    home: impl FnOnce() -> Option<PathBuf>,
) -> Result<UserDirs, Error> {
    let mut home = Home::new(home);
    let mut base = |variable: &str, default: &str| match absolute(var(variable)) {
        Some(path) => Ok(path),
        None => home.get().map(|home| home.join(default)),
    };
    let config = base("XDG_CONFIG_HOME", ".config")?.join(NAME);
    let data = base("XDG_DATA_HOME", ".local/share")?.join(NAME);
    let cache = base("XDG_CACHE_HOME", ".cache")?.join(NAME);
    Ok(UserDirs {
        config,
        logs: data.join(LOGS),
        data,
        cache,
    })
}

/// Finds a tool's folder: the path in `variable`, else `default` in the
/// home directory, which `home` finds only then.
fn resolve_tool(
    variable: &str,
    default: &str,
    var: impl Fn(&str) -> Option<OsString>,
    home: impl FnOnce() -> Option<PathBuf>,
) -> Result<PathBuf, Error> {
    match absolute(var(variable)) {
        Some(path) => Ok(path),
        None => Home::new(home).get().map(|home| home.join(default)),
    }
}

/// The home directory, looked up the first time it is needed.
struct Home<F> {
    lookup: Option<F>,
    found: Option<PathBuf>,
}

impl<F: FnOnce() -> Option<PathBuf>> Home<F> {
    fn new(lookup: F) -> Self {
        Self {
            lookup: Some(lookup),
            found: None,
        }
    }

    /// The home directory, if it is known and absolute.
    fn get(&mut self) -> Result<PathBuf, Error> {
        if let Some(lookup) = self.lookup.take() {
            self.found = lookup().filter(|home| home.is_absolute());
        }
        self.found.clone().ok_or(Error::NoHomeDirectory)
    }
}

/// The path in a variable, if it is one the XDG specification lets us use:
/// "All paths set in these environment variables must be absolute. If an
/// implementation encounters a relative path in any of these variables it
/// should consider the path invalid and ignore it." An empty value is
/// treated as unset, as the specification says too.
fn absolute(value: Option<OsString>) -> Option<PathBuf> {
    value.map(PathBuf::from).filter(|path| path.is_absolute())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::os::unix::ffi::OsStringExt as _;

    use super::*;

    /// An environment with these variables and nothing else.
    fn env<'a>(vars: &'a [(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |name| {
            vars.iter()
                .find(|(var, _)| *var == name)
                .map(|(_, value)| OsString::from(value))
        }
    }

    fn home(path: &str) -> impl FnOnce() -> Option<PathBuf> {
        let path = PathBuf::from(path);
        move || Some(path)
    }

    fn no_home() -> Option<PathBuf> {
        panic!("the home directory was looked up")
    }

    fn dirs(config: &str, data: &str, cache: &str) -> UserDirs {
        UserDirs {
            config: PathBuf::from(config),
            data: PathBuf::from(data),
            cache: PathBuf::from(cache),
            logs: PathBuf::from(data).join("logs"),
        }
    }

    #[test]
    fn without_xdg_variables_the_defaults_are_in_the_home_directory() {
        assert_eq!(
            resolve(env(&[]), home("/home/ada")).expect("a home"),
            dirs(
                "/home/ada/.config/tidepool",
                "/home/ada/.local/share/tidepool",
                "/home/ada/.cache/tidepool"
            )
        );
    }

    #[test]
    fn the_xdg_variables_override_the_defaults() {
        let vars = [
            ("XDG_CONFIG_HOME", "/cfg"),
            ("XDG_DATA_HOME", "/srv/data"),
            ("XDG_CACHE_HOME", "/var/cache/ada"),
        ];
        assert_eq!(
            resolve(env(&vars), no_home).expect("no home needed"),
            dirs(
                "/cfg/tidepool",
                "/srv/data/tidepool",
                "/var/cache/ada/tidepool"
            )
        );
    }

    #[test]
    fn each_variable_is_used_on_its_own() {
        let found = |vars: &[(&str, &str)]| resolve(env(vars), home("/h")).expect("a home");
        assert_eq!(
            found(&[("XDG_CONFIG_HOME", "/cfg")]),
            dirs(
                "/cfg/tidepool",
                "/h/.local/share/tidepool",
                "/h/.cache/tidepool"
            )
        );
        assert_eq!(
            found(&[("XDG_DATA_HOME", "/data")]),
            dirs(
                "/h/.config/tidepool",
                "/data/tidepool",
                "/h/.cache/tidepool"
            )
        );
        assert_eq!(
            found(&[("XDG_CACHE_HOME", "/cache")]),
            dirs(
                "/h/.config/tidepool",
                "/h/.local/share/tidepool",
                "/cache/tidepool"
            )
        );
    }

    #[test]
    fn empty_and_relative_variables_are_ignored() {
        for value in ["", "relative/path", "./cfg", "~/.config", "tidepool"] {
            let vars = [
                ("XDG_CONFIG_HOME", value),
                ("XDG_DATA_HOME", value),
                ("XDG_CACHE_HOME", value),
            ];
            assert_eq!(
                resolve(env(&vars), home("/home/ada")).expect("a home"),
                dirs(
                    "/home/ada/.config/tidepool",
                    "/home/ada/.local/share/tidepool",
                    "/home/ada/.cache/tidepool"
                ),
                "{value:?}"
            );
        }
    }

    #[test]
    fn the_home_directory_is_looked_up_once_and_only_when_needed() {
        let calls = Cell::new(0);
        let counted = || {
            calls.set(calls.get() + 1);
            Some(PathBuf::from("/home/ada"))
        };
        resolve(env(&[]), counted).expect("a home");
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn an_unknown_or_relative_home_is_an_error_when_a_default_needs_it() {
        let two = [("XDG_CONFIG_HOME", "/cfg"), ("XDG_DATA_HOME", "/data")];
        assert!(matches!(
            resolve(env(&two), || None),
            Err(Error::NoHomeDirectory)
        ));
        for relative in ["", "home/ada", "."] {
            assert!(
                matches!(
                    resolve(env(&[]), home(relative)),
                    Err(Error::NoHomeDirectory)
                ),
                "{relative:?}"
            );
        }
    }

    #[test]
    fn paths_that_are_not_utf_8_are_kept_byte_for_byte() {
        let value = OsString::from_vec(b"/srv/caf\xe9".to_vec());
        let var = |name: &str| (name == "XDG_DATA_HOME").then(|| value.clone());
        let found = resolve(var, home("/h")).expect("a home");
        assert_eq!(
            found.data,
            PathBuf::from(OsString::from_vec(b"/srv/caf\xe9/tidepool".to_vec()))
        );
        assert_eq!(found.logs, found.data.join("logs"));
    }

    #[test]
    fn the_logs_are_a_folder_of_their_own_in_the_data_folder() {
        let found = resolve(env(&[]), home("/home/ada")).expect("a home");
        assert_eq!(found.logs.parent(), Some(found.data.as_path()));
        assert_eq!(found.logs.file_name(), Some("logs".as_ref()));
    }

    #[test]
    fn a_tools_variable_overrides_its_default() {
        let vars = [("ANDROID_HOME", "/opt/android-sdk")];
        assert_eq!(
            resolve_tool("ANDROID_HOME", "Android/Sdk", env(&vars), no_home).expect("set"),
            PathBuf::from("/opt/android-sdk")
        );
        assert_eq!(
            resolve_tool("ANDROID_HOME", "Android/Sdk", env(&[]), home("/home/ada"))
                .expect("a home"),
            PathBuf::from("/home/ada/Android/Sdk")
        );
        for value in ["", "sdk", "./rustup"] {
            let vars = [("RUSTUP_HOME", value)];
            assert_eq!(
                resolve_tool("RUSTUP_HOME", ".rustup", env(&vars), home("/home/ada"))
                    .expect("a home"),
                PathBuf::from("/home/ada/.rustup"),
                "{value:?}"
            );
        }
        assert!(matches!(
            resolve_tool("RUSTUP_HOME", ".rustup", env(&[]), || None),
            Err(Error::NoHomeDirectory)
        ));
    }

    #[test]
    fn the_real_environment_gives_absolute_paths() {
        // Whatever this machine's environment is, a found folder is
        // absolute, and the tools' defaults are where the tools put them.
        if let Ok(found) = user_dirs() {
            for path in [&found.config, &found.data, &found.cache, &found.logs] {
                assert!(path.is_absolute(), "{}", path.display());
            }
        }
        for found in [android_sdk(), rustup_home()].into_iter().flatten() {
            assert!(found.is_absolute(), "{}", found.display());
        }
    }
}
