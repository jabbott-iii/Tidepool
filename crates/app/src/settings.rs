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

//! The settings the shell reads at startup, read-only until M4.7 builds
//! `workspace::settings` (M3.4, ADR-0060).
//!
//! It reads `settings.toml` in the user's config folder
//! (`platform::paths::user_dirs`, on Linux `$XDG_CONFIG_HOME/tidepool`),
//! once, before the window opens, with `codec::toml` within its default
//! limits and at most [`MAX_FILE_BYTES`]. Of it, two keys:
//!
//! ```toml
//! [ui]
//! side_panel = true    # whether the side panel shows at start
//! bottom_panel = true  # whether the bottom panel shows at start
//! ```
//!
//! A missing file, or a missing key, gives the default, `true`. An unknown
//! key is ignored, as M4.7 will keep it. A file that cannot be read, is
//! too large, or is not valid TOML gives every default, and a value of the
//! wrong type gives its own; each logs a warning naming the file and, for a
//! value, its key.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use tidepool_codec::Limits;
use tidepool_codec::toml::{self, Table};
use tidepool_core::log::warn;
use tidepool_core::path;
use tidepool_editor::shell::Panels;

/// The settings file's name in the user's config folder.
pub(crate) const FILE_NAME: &str = "settings.toml";

/// The largest settings file read: 1 MiB. A larger one gives the defaults,
/// with a warning.
pub(crate) const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// What the settings say.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Settings {
    /// Which of the shell's panels show at start.
    pub(crate) panels: Panels,
}

/// Reads the settings from the user's config folder, logging what it
/// could not use; the defaults if the folder is not known.
pub(crate) fn load() -> Settings {
    match tidepool_platform::paths::user_dirs() {
        Ok(dirs) => load_from(&dirs.config.join(FILE_NAME)),
        Err(error) => {
            warn!("the settings were not read, as the config folder is not known: {error}");
            Settings::default()
        }
    }
}

/// Reads the settings from `file`, logging what it could not use.
pub(crate) fn load_from(file: &Path) -> Settings {
    let (settings, warnings) = match read(file) {
        Ok(None) => (Settings::default(), Vec::new()),
        Ok(Some(bytes)) => from_bytes(&bytes),
        Err(error) => (
            Settings::default(),
            vec![format!("it was not read: {error}")],
        ),
    };
    for warning in warnings {
        warn!("{}: {warning}; using the default", path::display(file));
    }
    settings
}

/// The bytes of `file`, or `None` if there is none.
///
/// # Errors
///
/// The error of opening or reading it, or [`io::ErrorKind::FileTooLarge`]
/// past [`MAX_FILE_BYTES`].
fn read(file: &Path) -> io::Result<Option<Vec<u8>>> {
    let opened = match File::open(file) {
        Ok(opened) => opened,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    // One byte more than the most, to tell a file of the most from a
    // larger one, whatever its metadata says.
    opened.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::FileTooLarge,
            format!("it is larger than {MAX_FILE_BYTES} bytes"),
        ));
    }
    Ok(Some(bytes))
}

/// The settings `bytes` give, and a warning for each part of them that
/// gave a default.
fn from_bytes(bytes: &[u8]) -> (Settings, Vec<String>) {
    let root = match toml::parse_bytes(bytes, Limits::DEFAULT) {
        Ok(root) => root,
        Err(error) => {
            return (
                Settings::default(),
                vec![format!("it is not valid TOML: {error}")],
            );
        }
    };
    let mut warnings = Vec::new();
    let mut settings = Settings::default();
    let ui = match root.get("ui") {
        None => None,
        Some(value) => match value.as_table() {
            Some(table) => Some(table),
            None => {
                warnings.push("`ui` is not a table".to_string());
                None
            }
        },
    };
    if let Some(ui) = ui {
        settings.panels.side = boolean(ui, "side_panel", settings.panels.side, &mut warnings);
        settings.panels.bottom = boolean(ui, "bottom_panel", settings.panels.bottom, &mut warnings);
    }
    (settings, warnings)
}

/// The boolean at `ui.<key>`, or `default` if there is none, or, with a
/// warning, if it is not a boolean.
fn boolean(ui: &Table, key: &str, default: bool, warnings: &mut Vec<String>) -> bool {
    match ui.get(key) {
        None => default,
        Some(value) => value.as_bool().unwrap_or_else(|| {
            warnings.push(format!("`ui.{key}` is not `true` or `false`"));
            default
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tidepool_core::log;

    use super::*;

    fn settings(text: &str) -> (Settings, Vec<String>) {
        from_bytes(text.as_bytes())
    }

    fn panels(side: bool, bottom: bool) -> Settings {
        Settings {
            panels: Panels { side, bottom },
        }
    }

    #[test]
    fn both_panels_show_by_default() {
        assert_eq!(Settings::default(), panels(true, true));
        assert_eq!(settings(""), (panels(true, true), vec![]));
        assert_eq!(settings("[ui]\n"), (panels(true, true), vec![]));
    }

    #[test]
    fn the_two_keys_say_which_panels_show() {
        assert_eq!(
            settings("[ui]\nside_panel = false\n"),
            (panels(false, true), vec![])
        );
        assert_eq!(
            settings("ui.bottom_panel = false\nui.side_panel = true\n"),
            (panels(true, false), vec![])
        );
        assert_eq!(
            settings("ui = { side_panel = false, bottom_panel = false }\n"),
            (panels(false, false), vec![])
        );
    }

    #[test]
    fn unknown_keys_are_ignored() {
        assert_eq!(
            settings(
                "[ui]\ntheme = \"tidepool-dark\"\nbottom_panel = false\n[editor]\nkeymap = \"vim\"\n"
            ),
            (panels(true, false), vec![])
        );
    }

    #[test]
    fn a_value_of_the_wrong_type_gives_its_default_with_a_warning() {
        let (got, warnings) = settings("[ui]\nside_panel = \"no\"\nbottom_panel = false\n");
        assert_eq!(got, panels(true, false), "the other key still counts");
        assert_eq!(warnings, ["`ui.side_panel` is not `true` or `false`"]);
        let (got, warnings) = settings("ui = 3\n");
        assert_eq!(got, panels(true, true));
        assert_eq!(warnings, ["`ui` is not a table"]);
    }

    #[test]
    fn a_file_that_is_not_toml_gives_every_default_with_a_warning() {
        for text in [
            "[ui\n",
            "side_panel = = true",
            "[ui]\nside_panel = false\nside_panel = true\n",
        ] {
            let (got, warnings) = settings(text);
            assert_eq!(got, panels(true, true), "{text:?}");
            assert_eq!(warnings.len(), 1, "{text:?}");
            assert!(
                warnings[0].starts_with("it is not valid TOML: "),
                "{warnings:?}"
            );
        }
        let (got, warnings) = from_bytes(b"[ui]\nside_panel = false # \xff\n");
        assert_eq!(got, panels(true, true), "not UTF-8");
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn files_are_read_missing_too_large_and_logged() {
        let dir = std::env::temp_dir().join(format!("tidepool-settings-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        log::set_stderr(false);

        assert_eq!(load_from(&dir.join("missing.toml")), Settings::default());

        let file = dir.join(FILE_NAME);
        fs::write(&file, "[ui]\nbottom_panel = false\n").unwrap();
        assert_eq!(load_from(&file), panels(true, false));

        fs::write(&file, "[ui]\nside_panel = 1\n").unwrap();
        assert_eq!(load_from(&file), panels(true, true));
        let logged = log::tail(50).join("\n");
        assert!(
            logged.contains("`ui.side_panel` is not `true` or `false`; using the default"),
            "{logged}"
        );
        assert!(
            logged.contains(FILE_NAME),
            "the warning names the file: {logged}"
        );

        let mut big = b"[ui]\nside_panel = false\n".to_vec();
        big.resize(usize::try_from(MAX_FILE_BYTES).unwrap() + 1, b' ');
        fs::write(&file, &big).unwrap();
        assert_eq!(load_from(&file), Settings::default(), "too large to read");
        assert!(log::tail(5).join("\n").contains("larger than"));

        // A folder where the file should be cannot be read.
        let folder = dir.join("folder.toml");
        fs::create_dir_all(&folder).unwrap();
        assert_eq!(load_from(&folder), Settings::default());

        fs::remove_dir_all(&dir).unwrap();
    }
}
