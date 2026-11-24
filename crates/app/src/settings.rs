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

//! The settings the shell reads at startup, through `workspace::settings`
//! (M4.7, ADR-0067; M3.4, ADR-0060; M4.6, ADR-0064).
//!
//! [`schema`] declares the shell's settings, and [`load`] reads the user's
//! `settings.toml` in the user's config folder
//! (`platform::paths::user_dirs`, on Linux `$XDG_CONFIG_HOME/tidepool`),
//! once, before the window opens, as `workspace::settings` reads it (at
//! most 1 MiB, `version = 1` or none). Of it, two keys and a table:
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
//! key is ignored, and kept should the file be written back. A file that
//! cannot be read, is too large, is not valid TOML or has another
//! `version` gives every default, and a value of the wrong type gives its
//! own; each logs a warning naming the file and, for a value, its key.
//!
//! The binary opens no folder until M4.3, so no workspace's settings are
//! read yet, and it writes nothing back yet. Settings changed while it runs
//! take effect at its next start, until M4.5 watches the file.
//!
//! `[keymap]`, a table of modes, is the user's layer of the shell's keymap
//! (`editor::keymap`): [`keymap`] reads it over the default keymap, against
//! the shell's commands, and logs what the reading found, each naming the
//! file: a conflict or an error as a warning, and an override at info. A
//! `keymap` that is not a table gives the default keymap alone, with a
//! warning.

use std::path::Path;

use tidepool_codec::toml::Table;
use tidepool_core::log::{info, warn};
use tidepool_editor::command::Registry;
use tidepool_editor::keymap::{Keymap, ProblemKind};
use tidepool_editor::shell::Panels;
use tidepool_workspace::settings::{self as layered, Kind, Schema, SettingsFile};

/// Whether the side panel shows at start.
pub(crate) const SIDE_PANEL: &str = "ui.side_panel";

/// Whether the bottom panel shows at start.
pub(crate) const BOTTOM_PANEL: &str = "ui.bottom_panel";

/// The user's layer of the keymap, a table of modes.
pub(crate) const KEYMAP: &str = "keymap";

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

/// The shell's settings: the two panels, `true` by default, and the
/// keymap's table.
pub(crate) fn schema() -> Schema {
    let mut schema = Schema::new();
    for (key, kind) in [
        (SIDE_PANEL, Kind::Bool { default: true }),
        (BOTTOM_PANEL, Kind::Bool { default: true }),
        (KEYMAP, Kind::Table),
    ] {
        schema
            .add(key, kind)
            .expect("the shell's settings are valid keys, declared once");
    }
    schema
}

/// Reads the settings from the user's config folder, logging what it
/// could not use; the defaults if the folder is not known.
pub(crate) fn load() -> Settings {
    match tidepool_platform::paths::user_dirs() {
        Ok(dirs) => load_from(&dirs.config),
        Err(error) => {
            warn!("the settings were not read, as the config folder is not known: {error}");
            Settings::default()
        }
    }
}

/// Reads the settings from `settings.toml` in `config_dir`, logging what
/// it could not use.
pub(crate) fn load_from(config_dir: &Path) -> Settings {
    let resolved = resolve(&SettingsFile::user(config_dir));
    resolved.log_warnings();
    from_resolved(&resolved)
}

/// The settings of the user's `file` alone.
fn resolve(file: &SettingsFile) -> layered::Settings {
    layered::Settings::resolve(&schema(), Some(file), None)
}

/// What the shell takes from the resolved settings.
fn from_resolved(resolved: &layered::Settings) -> Settings {
    let defaults = Panels::default();
    Settings {
        panels: Panels {
            side: resolved.bool(SIDE_PANEL).unwrap_or(defaults.side),
            bottom: resolved.bool(BOTTOM_PANEL).unwrap_or(defaults.bottom),
        },
        keymap: resolved.table(KEYMAP).cloned(),
    }
}

/// The shell's keymap: the default one, with the user's layer from
/// `settings` over it, read against `registry`, the shell's commands. Logs
/// the first [`MAX_LOGGED_PROBLEMS`] problems the reading found, naming the
/// settings file, a conflict or an error as a warning and an override at
/// info, and then, if there were more, one warning saying how many it did
/// not log.
pub(crate) fn keymap(registry: &Registry, settings: &Settings) -> Keymap {
    let file = layered::FILE_NAME;
    let keymap = Keymap::bundled(registry, settings.keymap.as_ref());
    let problems = keymap.problems();
    for problem in problems.iter().take(MAX_LOGGED_PROBLEMS) {
        match problem.kind() {
            ProblemKind::Override => info!("{file} [keymap]: {problem}"),
            ProblemKind::Conflict | ProblemKind::Error => {
                warn!("{file} [keymap]: {problem}");
            }
        }
    }
    let unlogged = problems.len().saturating_sub(MAX_LOGGED_PROBLEMS);
    if unlogged > 0 {
        warn!(
            "{file} [keymap]: {unlogged} more problems were not logged; the first \
             {MAX_LOGGED_PROBLEMS} were"
        );
    }
    keymap
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tidepool_core::log;
    use tidepool_workspace::settings::Scope;

    use super::*;

    /// The settings `bytes` give, and the message of each warning.
    fn from_bytes(bytes: &[u8]) -> (Settings, Vec<String>) {
        let file = SettingsFile::from_bytes(Scope::User, layered::FILE_NAME, Some(bytes));
        let resolved = resolve(&file);
        let warnings = resolved
            .warnings()
            .iter()
            .map(|warning| warning.message().to_string())
            .collect();
        (from_resolved(&resolved), warnings)
    }

    fn settings(text: &str) -> (Settings, Vec<String>) {
        from_bytes(text.as_bytes())
    }

    #[test]
    fn the_schema_declares_the_shell_s_settings() {
        let schema = schema();
        assert_eq!(
            schema.keys().collect::<Vec<_>>(),
            [KEYMAP, BOTTOM_PANEL, SIDE_PANEL]
        );
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
            assert!(warnings[0].contains("is not valid TOML: "), "{warnings:?}");
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

        assert_eq!(load_from(&dir.join("missing")), Settings::default());

        let file = dir.join(layered::FILE_NAME);
        fs::write(&file, "[ui]\nbottom_panel = false\n").unwrap();
        assert_eq!(load_from(&dir), panels(true, false));

        fs::write(&file, "[ui]\nside_panel = 1\n").unwrap();
        assert_eq!(load_from(&dir), panels(true, true));
        let logged = log::tail(50).join("\n");
        assert!(
            logged.contains("`ui.side_panel` is not `true` or `false`; using the default"),
            "{logged}"
        );
        assert!(
            logged.contains(layered::FILE_NAME),
            "the warning names the file: {logged}"
        );

        let mut big = b"[ui]\nside_panel = false\n".to_vec();
        big.resize(usize::try_from(layered::MAX_FILE_BYTES).unwrap() + 1, b' ');
        fs::write(&file, &big).unwrap();
        assert_eq!(load_from(&dir), Settings::default(), "too large to read");
        assert!(log::tail(5).join("\n").contains("larger than"));

        // A folder where the file should be cannot be read.
        fs::remove_file(&file).unwrap();
        fs::create_dir_all(&file).unwrap();
        assert_eq!(load_from(&dir), Settings::default());
        assert!(log::tail(5).join("\n").contains("is not a regular file"));

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
        assert_eq!(warnings, ["`keymap` is not a table"]);
    }

    #[test]
    fn the_user_s_layer_goes_over_the_default_keymap_and_its_problems_are_logged() {
        log::set_stderr(false);
        let registry = tidepool_editor::shell::commands::registry();
        let none = keymap(&registry, &Settings::default());
        assert_eq!(none.problems(), [], "the default alone");
        // The shell's 4, and the editing commands' 41 (M4.2).
        assert_eq!(none.bindings("insert").len(), 45);

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
