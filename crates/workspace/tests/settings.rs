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

//! `workspace::settings`: the layers, invalid values falling back with a
//! logged warning, a workspace's file read only once it is trusted, and
//! values written back with comments and unknown keys kept (M4.7,
//! ADR-0067).

use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use tidepool_core::log;
use tidepool_workspace::WorkspaceError;
use tidepool_workspace::folder::Folder;
use tidepool_workspace::settings::{
    self, Kind, Schema, Scope, Settings, SettingsFile, Source, Value,
};
use tidepool_workspace::trust::TrustStore;

/// A fresh scratch folder for one test, removed when it is dropped:
/// `config/` for the user's settings, `data/` for trust and `work/` for a
/// workspace folder.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir =
            std::env::temp_dir().join(format!("tidepool-settings-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("config")).expect("make the config folder");
        fs::create_dir_all(dir.join("work")).expect("make the work folder");
        Scratch(dir)
    }

    fn config(&self) -> PathBuf {
        self.0.join("config")
    }

    fn user_file(&self) -> PathBuf {
        self.config().join(settings::FILE_NAME)
    }

    fn write_user(&self, text: &str) {
        fs::write(self.user_file(), text).expect("write the user's settings");
    }

    fn folder(&self) -> Folder {
        Folder::open(self.0.join("work")).expect("open the work folder")
    }

    fn write_workspace(&self, text: &str) {
        let file = self.folder().settings_file();
        fs::create_dir_all(file.parent().expect("a file in a folder")).expect("make `.tidepool`");
        fs::write(file, text).expect("write the workspace's settings");
    }

    /// A store that trusts the workspace folder, or one that trusts
    /// nothing.
    fn trust(&self, trusted: bool) -> TrustStore {
        let mut store = TrustStore::load(&self.0.join("data"));
        if trusted {
            store.trust(&self.folder()).expect("trust the work folder");
        }
        store
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A schema with one setting of each kind.
fn schema() -> Schema {
    let mut schema = Schema::new();
    for (key, kind) in [
        ("ui.side_panel", Kind::Bool { default: true }),
        ("ui.bottom_panel", Kind::Bool { default: true }),
        (
            "editor.tab_size",
            Kind::Integer {
                default: 4,
                min: 1,
                max: 16,
            },
        ),
        (
            "editor.keymap",
            Kind::String {
                default: "default",
                choices: &["default", "vim"],
            },
        ),
        ("keymap", Kind::Table),
    ] {
        schema.add(key, kind).expect("a valid declaration");
    }
    schema
}

/// The settings of `user`'s text alone.
fn user(text: &str) -> Settings {
    let file = SettingsFile::from_bytes(Scope::User, "/c/settings.toml", Some(text.as_bytes()));
    Settings::resolve(&schema(), Some(&file), None)
}

/// The warnings' lines.
fn lines(settings: &Settings) -> Vec<String> {
    settings
        .warnings()
        .iter()
        .map(ToString::to_string)
        .collect()
}

#[test]
fn the_defaults_with_no_files() {
    let settings = Settings::resolve(&schema(), None, None);
    assert_eq!(settings.bool("ui.side_panel"), Some(true));
    assert_eq!(settings.integer("editor.tab_size"), Some(4));
    assert_eq!(settings.string("editor.keymap"), Some("default"));
    assert_eq!(settings.table("keymap"), None, "a table has no default");
    assert_eq!(settings.source("ui.side_panel"), Some(Source::Default));
    assert_eq!(settings.get("no.such"), None);
    assert!(settings.warnings().is_empty());
    let missing = SettingsFile::from_bytes(Scope::User, "/c/settings.toml", None);
    assert!(!missing.exists());
    assert_eq!(Settings::resolve(&schema(), Some(&missing), None), settings);
}

#[test]
fn the_user_s_values_and_unknown_keys() {
    let settings = user(
        "version = 1\n[ui]\nside_panel = false\ntheme = \"x\"\n[editor]\ntab_size = 2\nkeymap = \"vim\"\n[keymap.insert]\n\"ctrl+b\" = \"\"\n[plugins.foo]\nx = 1\n",
    );
    assert!(settings.warnings().is_empty(), "{:?}", lines(&settings));
    assert_eq!(settings.bool("ui.side_panel"), Some(false));
    assert_eq!(settings.source("ui.side_panel"), Some(Source::User));
    assert_eq!(settings.bool("ui.bottom_panel"), Some(true));
    assert_eq!(settings.source("ui.bottom_panel"), Some(Source::Default));
    assert_eq!(settings.integer("editor.tab_size"), Some(2));
    assert_eq!(settings.string("editor.keymap"), Some("vim"));
    let keymap = settings.table("keymap").unwrap();
    assert!(keymap.get("insert").is_some());
    // Each spelling of a table is the same.
    for text in [
        "ui.side_panel = false\n",
        "ui = { side_panel = false }\n",
        "[ui]\nside_panel = false\n",
    ] {
        assert_eq!(user(text).bool("ui.side_panel"), Some(false), "{text}");
    }
}

#[test]
fn an_invalid_value_falls_back_to_its_default_with_a_logged_warning() {
    let settings = user(
        "[ui]\nside_panel = \"no\"\nbottom_panel = false\n[editor]\ntab_size = 99\nkeymap = \"emacs\"\nkeymap_extra = 1\n",
    );
    assert_eq!(settings.bool("ui.side_panel"), Some(true), "the default");
    assert_eq!(
        settings.bool("ui.bottom_panel"),
        Some(false),
        "the other key still counts"
    );
    assert_eq!(settings.integer("editor.tab_size"), Some(4));
    assert_eq!(settings.string("editor.keymap"), Some("default"));
    assert_eq!(
        lines(&settings),
        [
            "`/c/settings.toml`: `editor.keymap` is not one of `default`, `vim`; using the default",
            "`/c/settings.toml`: `editor.tab_size` is outside its range, 1 to 16; using the default",
            "`/c/settings.toml`: `ui.side_panel` is not `true` or `false`; using the default",
        ]
    );
    let warning = &settings.warnings()[2];
    assert_eq!(warning.key(), Some("ui.side_panel"));
    assert_eq!(warning.using(), Some(Source::Default));
    assert_eq!(warning.file(), Path::new("/c/settings.toml"));

    log::set_stderr(false);
    settings.log_warnings();
    let logged = log::tail(50).join("\n");
    let line = logged
        .lines()
        .rev()
        .find(|line| line.contains("`ui.side_panel` is not `true` or `false`; using the default"))
        .unwrap_or_else(|| panic!("not logged: {logged}"));
    assert!(line.contains(" WARN "), "{line}");

    let settings = user("[editor]\ntab_size = 2.0\nkeymap = 3\n");
    assert_eq!(
        lines(&settings),
        [
            "`/c/settings.toml`: `editor.keymap` is not a string; using the default",
            "`/c/settings.toml`: `editor.tab_size` is not a whole number; using the default",
        ]
    );
    let settings = user("keymap = \"vim\"\n");
    assert_eq!(
        lines(&settings),
        ["`/c/settings.toml`: `keymap` is not a table; using the default"]
    );
}

#[test]
fn a_value_on_the_way_that_is_not_a_table_warns_once() {
    let settings = user("ui = 3\n[editor]\ntab_size = 8\n");
    assert_eq!(settings.bool("ui.side_panel"), Some(true));
    assert_eq!(settings.bool("ui.bottom_panel"), Some(true));
    assert_eq!(
        settings.integer("editor.tab_size"),
        Some(8),
        "the rest still counts"
    );
    assert_eq!(
        lines(&settings),
        ["`/c/settings.toml`: `ui` is not a table, so the settings in it fall back"]
    );
    assert_eq!(settings.warnings()[0].key(), None);
}

#[test]
fn a_file_that_cannot_be_used_gives_none_of_its_values() {
    let big = format!("[ui]\nside_panel = false\n{}", " ".repeat(1024 * 1024));
    for (text, says) in [
        ("[ui\nside_panel = false\n", "is not valid TOML"),
        (
            "[ui]\nside_panel = false\nside_panel = true\n",
            "is not valid TOML",
        ),
        (
            "version = 2\n[ui]\nside_panel = false\n",
            "does not read; it reads 1",
        ),
        (
            "version = \"1\"\n[ui]\nside_panel = false\n",
            "does not read; it reads 1",
        ),
        (big.as_str(), "larger than the limit of 1048576 bytes"),
    ] {
        let settings = user(text);
        assert_eq!(settings.bool("ui.side_panel"), Some(true), "{says}");
        let lines = lines(&settings);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].contains(says), "{lines:?}");
        assert!(lines[0].contains("/c/settings.toml"), "{lines:?}");
        assert!(
            lines[0].ends_with("; none of its settings are used"),
            "{lines:?}"
        );
        assert_eq!(settings.warnings()[0].using(), None);
    }
    let settings = user("[ui]\nside_panel = false # \u{fffd}\n");
    assert_eq!(settings.bool("ui.side_panel"), Some(false));
    let bytes = SettingsFile::from_bytes(
        Scope::User,
        "/c/s.toml",
        Some(b"[ui]\nside_panel = false # \xff\n"),
    );
    assert!(bytes.problem().is_some(), "not UTF-8");

    // On disk: a folder where the file should be, and a missing file.
    let scratch = Scratch::new("unusable");
    let missing = SettingsFile::user(&scratch.config());
    assert!(!missing.exists() && missing.problem().is_none());
    fs::create_dir_all(scratch.user_file()).unwrap();
    let folder = SettingsFile::user(&scratch.config());
    assert!(matches!(
        folder.problem(),
        Some(WorkspaceError::NotRegularFile { .. })
    ));
}

#[test]
fn an_untrusted_workspace_s_file_is_never_read() {
    let scratch = Scratch::new("untrusted");
    scratch.write_workspace(
        "[ui]\nside_panel = false\n[keymap.insert]\n\"ctrl+q\" = \"window.close\"\n",
    );
    let untrusted = scratch.trust(false);
    assert!(SettingsFile::workspace(&scratch.folder(), &untrusted).is_none());
    // Not even a broken one is looked at.
    scratch.write_workspace("[ui\n");
    assert!(SettingsFile::workspace(&scratch.folder(), &untrusted).is_none());
}

#[test]
fn a_trusted_workspace_s_values_lie_over_the_user_s() {
    let scratch = Scratch::new("trusted");
    scratch.write_user("[ui]\nside_panel = false\nbottom_panel = false\n[editor]\ntab_size = 2\n[keymap.insert]\n\"alt+b\" = \"view.toggleSidePanel\"\n");
    scratch.write_workspace("[ui]\nside_panel = true\nbottom_panel = 7\n[editor]\ntab_size = 0\n[keymap.insert]\n\"ctrl+q\" = \"window.close\"\n");
    let trusted = scratch.trust(true);
    let user = SettingsFile::user(&scratch.config());
    let workspace = SettingsFile::workspace(&scratch.folder(), &trusted).expect("trusted");
    assert_eq!(workspace.scope(), Scope::Workspace);
    let settings = Settings::resolve(&schema(), Some(&user), Some(&workspace));
    assert_eq!(settings.bool("ui.side_panel"), Some(true));
    assert_eq!(settings.source("ui.side_panel"), Some(Source::Workspace));
    // Invalid in the workspace: the user's value, which is valid, is used.
    assert_eq!(settings.bool("ui.bottom_panel"), Some(false));
    assert_eq!(settings.source("ui.bottom_panel"), Some(Source::User));
    assert_eq!(settings.integer("editor.tab_size"), Some(2));
    let lines = lines(&settings);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(
        lines[0]
            .ends_with("`editor.tab_size` is outside its range, 1 to 16; using the user's setting"),
        "{lines:?}"
    );
    assert!(lines[1].contains(".tidepool/settings.toml"), "{lines:?}");
    // Tables: the workspace's wins, and each layer's is there to merge.
    let keymap = settings.table("keymap").unwrap();
    assert!(keymap.get("insert").unwrap().get("ctrl+q").is_some());
    let layers = settings.table_layers("keymap");
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[0].0, Source::User);
    assert_eq!(layers[1].0, Source::Workspace);
    assert!(settings.table_layers("ui.side_panel").is_empty());
}

#[test]
fn saving_keeps_comments_and_unknown_keys() {
    let scratch = Scratch::new("save");
    let original = "# My settings\n[ui]\nside_panel = true # shown\ntheme = \"mine\" # not a setting yet\n\n[future]\nflag = 1\n";
    scratch.write_user(original);
    let mut file = SettingsFile::user(&scratch.config());
    file.write(&schema(), "ui.side_panel", &Value::Bool(false))
        .unwrap();
    let saved = fs::read_to_string(scratch.user_file()).unwrap();
    // `version` goes above the comment that belongs to the first header,
    // as the document model places a key at the root (ADR-0031).
    assert_eq!(
        saved,
        "version = 1\n\n# My settings\n[ui]\nside_panel = false # shown\ntheme = \"mine\" # not a setting yet\n\n[future]\nflag = 1\n"
    );
    // A new key goes in its table; a new table at the end.
    file.write(&schema(), "editor.tab_size", &Value::Integer(8))
        .unwrap();
    file.write(&schema(), "ui.bottom_panel", &Value::Bool(false))
        .unwrap();
    let saved = fs::read_to_string(scratch.user_file()).unwrap();
    assert!(
        saved.starts_with("version = 1\n\n# My settings\n[ui]\n"),
        "{saved}"
    );
    assert!(
        saved.contains("theme = \"mine\" # not a setting yet\nbottom_panel = false\n"),
        "{saved}"
    );
    assert!(saved.ends_with("[editor]\ntab_size = 8\n"), "{saved}");
    let settings = Settings::resolve(
        &schema(),
        Some(&SettingsFile::user(&scratch.config())),
        None,
    );
    assert!(settings.warnings().is_empty());
    assert_eq!(settings.integer("editor.tab_size"), Some(8));
    assert_eq!(settings.bool("ui.bottom_panel"), Some(false));
    // Setting the same value again changes no byte.
    file.write(&schema(), "editor.tab_size", &Value::Integer(8))
        .unwrap();
    assert_eq!(fs::read_to_string(scratch.user_file()).unwrap(), saved);
}

#[test]
fn writing_makes_the_file_and_keeps_an_edit_made_since_it_was_read() {
    let scratch = Scratch::new("fresh");
    fs::remove_dir_all(scratch.config()).unwrap();
    let mut file = SettingsFile::user(&scratch.config());
    assert!(!file.exists());
    file.write(&schema(), "editor.keymap", &Value::String("vim".into()))
        .unwrap();
    assert_eq!(
        fs::read_to_string(scratch.user_file()).unwrap(),
        "version = 1\n\n[editor]\nkeymap = \"vim\"\n"
    );
    // The user edits the file in another editor; the next write keeps it.
    fs::write(
        scratch.user_file(),
        "version = 1\n# mine\n[editor]\nkeymap = \"vim\"\ntab_size = 3\n",
    )
    .unwrap();
    file.write(&schema(), "ui.side_panel", &Value::Bool(false))
        .unwrap();
    let saved = fs::read_to_string(scratch.user_file()).unwrap();
    assert!(
        saved.contains("# mine\n[editor]\nkeymap = \"vim\"\ntab_size = 3\n"),
        "{saved}"
    );
    assert!(saved.contains("[ui]\nside_panel = false\n"), "{saved}");
    assert_eq!(
        file.document().unwrap().as_str(),
        saved,
        "it holds what is on disk"
    );
}

#[test]
fn writes_that_are_refused_change_nothing() {
    let scratch = Scratch::new("refused");
    let broken = "[ui\nside_panel = true\n";
    scratch.write_user(broken);
    let mut file = SettingsFile::user(&scratch.config());
    let error = file
        .write(&schema(), "ui.side_panel", &Value::Bool(false))
        .unwrap_err();
    assert!(
        matches!(error, WorkspaceError::Unwritable { .. }),
        "{error}"
    );
    assert_eq!(fs::read_to_string(scratch.user_file()).unwrap(), broken);

    scratch.write_user("version = 2\n");
    let error = file
        .write(&schema(), "ui.side_panel", &Value::Bool(false))
        .unwrap_err();
    assert!(error.to_string().contains("does not read"), "{error}");
    assert_eq!(
        fs::read_to_string(scratch.user_file()).unwrap(),
        "version = 2\n"
    );

    let kept = "ui = 3\n";
    scratch.write_user(kept);
    for (key, value) in [
        ("no.such", Value::Bool(true)),
        ("keymap", Value::Bool(true)),
        ("editor.tab_size", Value::Integer(17)),
        ("editor.keymap", Value::String("emacs".into())),
        ("ui.side_panel", Value::Integer(1)),
        ("ui.side_panel", Value::Bool(false)),
    ] {
        let error = file.write(&schema(), key, &value).unwrap_err();
        let expected = match key {
            "no.such" => matches!(error, WorkspaceError::UnknownSetting { .. }),
            "keymap" => matches!(error, WorkspaceError::NotWritable { .. }),
            _ if matches!(value, Value::Bool(false)) => {
                matches!(error, WorkspaceError::Edit { .. })
            }
            _ => matches!(error, WorkspaceError::InvalidValue { .. }),
        };
        assert!(expected, "{key}: {error}");
        assert_eq!(
            fs::read_to_string(scratch.user_file()).unwrap(),
            kept,
            "{key}"
        );
    }
}

#[test]
fn setting_in_memory_writes_nothing() {
    let mut file = SettingsFile::from_bytes(Scope::User, "/nowhere/settings.toml", Some(b"# hi\n"));
    file.set(&schema(), "editor.tab_size", &Value::Integer(2))
        .unwrap();
    assert_eq!(
        file.document().unwrap().as_str(),
        "# hi\nversion = 1\n\n[editor]\ntab_size = 2\n"
    );
    let settings = Settings::resolve(&schema(), Some(&file), None);
    assert_eq!(settings.integer("editor.tab_size"), Some(2));
    assert!(!Path::new("/nowhere/settings.toml").exists());
}

/// Makes a symbolic link at `at` to `target`, or says why it cannot.
fn link(target: &Path, at: &Path) -> Result<(), String> {
    // `fs::soft_link` is deprecated in favour of one function per OS, but
    // those would need a platform `cfg`, which `workspace` must not have
    // (`AGENTS.md` rule 5); this one works wherever links do.
    #[allow(deprecated)]
    fs::soft_link(target, at).map_err(|error| error.to_string())
}

/// Makes `at` a link to `target`, or prints `SKIPPED:` for `test` and
/// says no.
fn linked(test: &str, target: &Path, at: &Path) -> bool {
    match link(target, at) {
        Ok(()) => true,
        Err(reason) => {
            let _ = writeln!(io::stderr(), "SKIPPED: {test}: no link: {reason}");
            false
        }
    }
}

#[test]
fn a_workspace_s_linked_settings_file_is_neither_read_nor_written() {
    let test = "a_workspace_s_linked_settings_file_is_neither_read_nor_written";
    let scratch = Scratch::new("linkfile");
    let folder = scratch.folder();
    fs::create_dir_all(folder.config_dir()).expect("make `.tidepool`");
    let trusted = scratch.trust(true);
    // A link out of the folder, and one to a file of the project itself.
    let elsewhere = scratch.0.join("elsewhere.toml");
    let cargo = folder.root().join("Cargo.toml");
    for target in [&elsewhere, &cargo] {
        let kept = "[ui]\nside_panel = false\n";
        fs::write(target, kept).expect("write the target");
        let _ = fs::remove_file(folder.settings_file());
        if !linked(test, target, &folder.settings_file()) {
            return;
        }
        let mut file = SettingsFile::workspace(&folder, &trusted).expect("trusted");
        assert!(
            matches!(file.problem(), Some(WorkspaceError::Linked { .. })),
            "{:?}",
            file.problem()
        );
        let settings = Settings::resolve(&schema(), None, Some(&file));
        assert_eq!(settings.bool("ui.side_panel"), Some(true), "not read");
        assert!(matches!(
            file.write(&schema(), "ui.side_panel", &Value::Bool(true)),
            Err(WorkspaceError::Linked { .. })
        ));
        assert_eq!(fs::read_to_string(target).unwrap(), kept, "not written");
    }
}

#[test]
fn a_linked_tidepool_folder_is_neither_read_nor_written() {
    let test = "a_linked_tidepool_folder_is_neither_read_nor_written";
    let scratch = Scratch::new("linkdir");
    let folder = scratch.folder();
    let elsewhere = scratch.0.join("elsewhere");
    fs::create_dir_all(&elsewhere).expect("make the target");
    fs::write(
        elsewhere.join("settings.toml"),
        "[ui]\nside_panel = false\n",
    )
    .unwrap();
    if !linked(test, &elsewhere, &folder.config_dir()) {
        return;
    }
    let trusted = scratch.trust(true);
    let mut file = SettingsFile::workspace(&folder, &trusted).expect("trusted");
    assert!(matches!(
        file.problem(),
        Some(WorkspaceError::Linked { .. })
    ));
    assert!(matches!(
        file.write(&schema(), "editor.tab_size", &Value::Integer(2)),
        Err(WorkspaceError::Linked { .. })
    ));
    assert_eq!(
        fs::read_to_string(elsewhere.join("settings.toml")).unwrap(),
        "[ui]\nside_panel = false\n"
    );
    // A `.tidepool` that is a file, not a folder, is refused the same way.
    fs::remove_file(folder.config_dir()).expect("remove the link");
    fs::write(folder.config_dir(), "").unwrap();
    let file = SettingsFile::workspace(&folder, &trusted).expect("trusted");
    assert!(matches!(
        file.problem(),
        Some(WorkspaceError::Linked { .. })
    ));
}

#[test]
fn a_workspace_s_file_not_from_a_trusted_folder_is_not_used() {
    let text = b"[ui]\nside_panel = false\n";
    let untrusted =
        SettingsFile::from_bytes(Scope::Workspace, "/w/.tidepool/settings.toml", Some(text));
    assert!(!untrusted.is_trusted());
    let settings = Settings::resolve(&schema(), None, Some(&untrusted));
    assert_eq!(settings.bool("ui.side_panel"), Some(true));
    assert_eq!(
        lines(&settings),
        [
            "`/w/.tidepool/settings.toml` is not used, as no trusted folder gave it; none of its settings are used"
        ]
    );
    let mut untrusted = untrusted;
    assert!(matches!(
        untrusted.write(&schema(), "ui.side_panel", &Value::Bool(false)),
        Err(WorkspaceError::Untrusted { .. })
    ));
    // A file in the wrong place gives nothing either.
    let user = SettingsFile::from_bytes(Scope::User, "/c/settings.toml", Some(text));
    let settings = Settings::resolve(&schema(), Some(&untrusted), Some(&user));
    assert_eq!(settings.bool("ui.side_panel"), Some(true));
    assert_eq!(settings.warnings().len(), 2, "{:?}", lines(&settings));

    // From bytes, through the trust store.
    let scratch = Scratch::new("frombytes");
    let folder = scratch.folder();
    assert!(
        SettingsFile::workspace_from_bytes(&folder, &scratch.trust(false), Some(text)).is_none()
    );
    let trusted = scratch.trust(true);
    let file = SettingsFile::workspace_from_bytes(&folder, &trusted, Some(text)).expect("trusted");
    assert!(file.is_trusted());
    let settings = Settings::resolve(&schema(), None, Some(&file));
    assert_eq!(settings.bool("ui.side_panel"), Some(false));
    assert_eq!(settings.source("ui.side_panel"), Some(Source::Workspace));
}

#[test]
fn a_trusted_workspace_s_file_is_written_in_its_folder() {
    let scratch = Scratch::new("wswrite");
    let trusted = scratch.trust(true);
    let folder = scratch.folder();
    let mut file = SettingsFile::workspace(&folder, &trusted).unwrap();
    assert!(!file.exists());
    file.write(&schema(), "editor.tab_size", &Value::Integer(2))
        .unwrap();
    assert_eq!(
        fs::read_to_string(folder.settings_file()).unwrap(),
        "version = 1\n\n[editor]\ntab_size = 2\n"
    );
}
