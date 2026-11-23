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
//! `workspace::settings` (M3.4, ADR-0060; M4.6, ADR-0064).
//!
//! It reads `settings.toml` in the user's config folder
//! (`platform::paths::user_dirs`, on Linux `$XDG_CONFIG_HOME/tidepool`),
//! once, before the window opens, with `codec::toml` within its default
//! limits and at most [`MAX_FILE_BYTES`]. Of it, two keys and a table:
//!
//! ```toml
//! [ui]
//! side_panel = true    # whether the side panel shows at start
//! bottom_panel = true  # whether the bottom panel shows at start
//!
//! [keymap.insert]      # the user's bindings, over the default keymap's
//! "ctrl+k ctrl+b" = "view.toggleSidePanel"
//! "ctrl+b" = ""        # an empty string unbinds the default's keys
//! ```
//!
//! A missing file, or a missing key, gives the default, `true`. An unknown
//! key is ignored, as M4.7 will keep it. A file that cannot be read, is
//! too large, or is not valid TOML gives every default, and a value of the
//! wrong type gives its own; each logs a warning naming the file and, for a
//! value, its key.
//!
//! `[keymap]`, a table of modes, is the user's layer of the shell's keymap
//! (`editor::keymap`): [`keymap`] reads it over the default keymap, against
//! the shell's commands, and logs what the reading found, each naming the
//! file: a conflict or an error as a warning, and an override at info. A
//! `keymap` that is not a table gives the default keymap alone, with a
//! warning.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use tidepool_codec::Limits;
use tidepool_codec::toml::{self, Table};
use tidepool_core::log::{info, warn};
use tidepool_core::path;
use tidepool_editor::command::Registry;
use tidepool_editor::keymap::{Keymap, ProblemKind};
use tidepool_editor::shell::Panels;

/// The settings file's name in the user's config folder.
pub(crate) const FILE_NAME: &str = "settings.toml";

/// The largest settings file read: 1 MiB. A larger one gives the defaults,
/// with a warning.
pub(crate) const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// The most keymap problems [`keymap`] logs, one line each; one more line
/// says how many it did not. The keymap keeps them all, within its limits
/// (`editor::keymap::Limits`), but a settings file of thousands of bad
/// bindings must not flood the log, whose writer drops lines once its
/// queue is full, nor hold up the start for long.
pub(crate) const MAX_LOGGED_PROBLEMS: usize = 64;

/// What the settings say.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Settings {
    /// Which of the shell's panels show at start.
    pub(crate) panels: Panels,
    /// The user's layer of the keymap: `[keymap]`, a table of modes, if the
    /// file has one.
    pub(crate) keymap: Option<Table>,
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

/// The shell's keymap: the default one, with the user's layer from
/// `settings` over it, read against `registry`, the shell's commands. Logs
/// the first [`MAX_LOGGED_PROBLEMS`] problems the reading found, naming the
/// settings file, a conflict or an error as a warning and an override at
/// info, and then, if there were more, one warning saying how many it did
/// not log.
pub(crate) fn keymap(registry: &Registry, settings: &Settings) -> Keymap {
    let keymap = Keymap::bundled(registry, settings.keymap.as_ref());
    let problems = keymap.problems();
    for problem in problems.iter().take(MAX_LOGGED_PROBLEMS) {
        match problem.kind() {
            ProblemKind::Override => info!("{FILE_NAME} [keymap]: {problem}"),
            ProblemKind::Conflict | ProblemKind::Error => {
                warn!("{FILE_NAME} [keymap]: {problem}");
            }
        }
    }
    let unlogged = problems.len().saturating_sub(MAX_LOGGED_PROBLEMS);
    if unlogged > 0 {
        warn!(
            "{FILE_NAME} [keymap]: {unlogged} more problems were not logged; the first \
             {MAX_LOGGED_PROBLEMS} were"
        );
    }
    keymap
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
    settings.keymap = match root.get("keymap") {
        None => None,
        Some(value) => match value.as_table() {
            Some(table) => Some(table.clone()),
            None => {
                warnings.push("`keymap` is not a table of modes".to_string());
                None
            }
        },
    };
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
            keymap: None,
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

    #[test]
    fn the_keymap_table_is_the_user_s_layer() {
        let (got, warnings) = settings(
            "[ui]\nside_panel = false\n[keymap.insert]\n\"ctrl+b\" = \"\"\n\"alt+b\" = \"view.toggleSidePanel\"\n",
        );
        assert_eq!(warnings, Vec::<String>::new());
        assert_eq!(
            got.panels,
            Panels {
                side: false,
                bottom: true
            }
        );
        let table = got.keymap.as_ref().expect("a table");
        let insert = table
            .get("insert")
            .and_then(|v| v.as_table())
            .expect("a mode");
        assert_eq!(insert.len(), 2);
        assert_eq!(settings("").0.keymap, None, "none in an empty file");
        let (got, warnings) = settings("keymap = \"vim\"\n[ui]\nbottom_panel = false\n");
        assert_eq!(got, panels(true, false), "the rest still counts");
        assert_eq!(warnings, ["`keymap` is not a table of modes"]);
    }

    #[test]
    fn the_user_s_layer_goes_over_the_default_keymap_and_its_problems_are_logged() {
        log::set_stderr(false);
        let registry = tidepool_editor::shell::commands::registry();
        let none = keymap(&registry, &Settings::default());
        assert_eq!(none.problems(), [], "the default alone");
        assert_eq!(none.bindings("insert").len(), 4);

        let (got, _) = settings(
            "[keymap.insert]\n\"ctrl+b\" = \"window.close\"\n\"alt+q\" = \"window.quit\"\n\"ctrl+j ctrl+j\" = \"panel.showOutput\"\n",
        );
        let keymap = keymap(&registry, &got);
        let kinds: Vec<ProblemKind> = keymap.problems().iter().map(|p| p.kind()).collect();
        assert_eq!(
            kinds,
            [
                ProblemKind::Error,
                ProblemKind::Override,
                ProblemKind::Conflict
            ]
        );
        let logged = log::tail(50).join("\n");
        let line = |needle: &str| {
            logged
                .lines()
                .rev()
                .find(|line| line.contains(needle))
                .unwrap_or_else(|| panic!("no line with {needle:?} in {logged}"))
                .to_string()
        };
        let unknown = line("no command is named `window.quit`");
        assert!(
            unknown.contains(" WARN ") && unknown.contains("settings.toml [keymap]: "),
            "{unknown}"
        );
        let over = line("`ctrl+b`: overrides default layer");
        assert!(over.contains(" INFO "), "{over}");
        let prefix = line("`ctrl+j ctrl+j`: starts with the keys of");
        assert!(prefix.contains(" WARN "), "{prefix}");
        let binding = keymap
            .bindings("insert")
            .iter()
            .find(|binding| binding.keys().to_string() == "Ctrl+B")
            .expect("bound");
        assert_eq!(binding.command(), "window.close");
    }

    #[test]
    fn at_most_the_cap_of_keymap_problems_is_logged_then_how_many_were_not() {
        log::set_stderr(false);
        let registry = tidepool_editor::shell::commands::registry();
        // 100 bindings of commands no registry has, each named so that its
        // line can be told from other tests' lines.
        let mut text = String::from("[keymap.insert]\n");
        for n in 0..100 {
            text.push_str(&format!(
                "\"alt+f{} {}\" = \"logcap.test{n}\"\n",
                n % 12 + 1,
                char::from(b'a' + n % 26)
            ));
        }
        let (got, warnings) = settings(&text);
        assert_eq!(warnings, Vec::<String>::new());
        let keymap = keymap(&registry, &got);
        let problems = keymap.problems().len();
        assert!(problems > MAX_LOGGED_PROBLEMS, "{problems} problems");
        let logged = log::tail(log::RING_CAPACITY);
        let lines = logged
            .iter()
            .filter(|line| line.contains("no command is named `logcap.test"))
            .count();
        assert_eq!(lines, MAX_LOGGED_PROBLEMS);
        let summary = format!(
            "{} more problems were not logged; the first {MAX_LOGGED_PROBLEMS} were",
            problems - MAX_LOGGED_PROBLEMS
        );
        let summary = logged
            .iter()
            .find(|line| line.contains(&summary))
            .unwrap_or_else(|| panic!("no line with {summary:?}"));
        assert!(summary.contains(" WARN "), "{summary}");
    }
}
