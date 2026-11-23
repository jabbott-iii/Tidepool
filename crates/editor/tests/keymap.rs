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

//! Keymaps and the modal engine, headless (M4.6, ADR-0064): the default
//! keymap; a keymap of two modes that switches between them; pending
//! sequences, their cancel, auto-repeats and modifier keys; the user's
//! layer over the default; every conflict, override and error reported;
//! the limits at their defaults; the file's errors; and a property test of
//! the engine's bounds, which prints its seed when it fails.

use tidepool_codec::toml::{self, Table};
use tidepool_core::input::{InputEvent, Key, Modifiers};
use tidepool_core::testing::check_sized;
use tidepool_editor::EditorError;
use tidepool_editor::command::Registry;
use tidepool_editor::keymap::{
    DEFAULT_TOML, Engine, FileError, Keymap, Layer, Limits, MAX_MODE_DEPTH, NotationErrorKind,
    Place, Problem, ProblemKind, Resolution, Sequence, Step,
};
use tidepool_editor::shell::commands;

/// A registry of a few commands, for keymaps of their own.
fn registry() -> Registry {
    let mut registry = Registry::new();
    for (id, title) in [
        ("editor.save", "File: Save"),
        ("editor.saveAll", "File: Save All"),
        ("editor.undo", "Edit: Undo"),
        ("editor.redo", "Edit: Redo"),
        ("cursor.down", "Cursor: Down"),
        ("cursor.top", "Cursor: Top"),
    ] {
        registry.register(id, title).expect("valid and new");
    }
    registry
}

/// A keymap file starting in `start` with `body` after its header.
fn file(start: &str, body: &str) -> String {
    format!("version = 1\nstart = \"{start}\"\n{body}")
}

/// The keymap `text` reads into against [`registry`], with no user layer.
fn read(text: &str) -> Keymap {
    Keymap::read(text, None, &registry(), Limits::DEFAULT).expect("a keymap")
}

/// `text`'s `keymap` table, as `settings.toml` gives the user's layer.
fn user(text: &str) -> Table {
    let root = toml::parse(text, tidepool_codec::Limits::DEFAULT).expect("TOML");
    root.get("keymap")
        .and_then(|keymap| keymap.as_table())
        .cloned()
        .expect("a [keymap] table")
}

fn down(key: Key, modifiers: Modifiers) -> InputEvent {
    InputEvent::KeyDown {
        key,
        modifiers,
        repeat: false,
    }
}

fn repeat(key: Key, modifiers: Modifiers) -> InputEvent {
    InputEvent::KeyDown {
        key,
        modifiers,
        repeat: true,
    }
}

fn plain(key: Key) -> InputEvent {
    down(key, Modifiers::NONE)
}

fn ctrl(key: Key) -> InputEvent {
    down(key, Modifiers::CTRL)
}

fn command(id: &str) -> Resolution<'_> {
    Resolution::Command { id, repeat: false }
}

const ENTERED: Resolution<'static> = Resolution::Mode {
    changed: true,
    repeat: false,
};

fn place(layer: Layer, mode: &str, keys: &str) -> Place {
    Place {
        layer,
        mode: mode.to_string(),
        keys: keys.to_string(),
    }
}

/// Each binding of `mode`, as its keys shown and its command.
fn bindings(keymap: &Keymap, mode: &str) -> Vec<(String, String)> {
    keymap
        .bindings(mode)
        .iter()
        .map(|binding| (binding.keys().to_string(), binding.command().to_string()))
        .collect()
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(keys, command)| (keys.to_string(), command.to_string()))
        .collect()
}

#[test]
fn the_default_keymap_binds_the_shell_s_four_keys_to_its_commands_with_no_problems() {
    let registry = commands::registry();
    let keymap = Keymap::bundled(&registry, None);
    assert_eq!(keymap.problems(), [], "the default keymap has no problems");
    assert_eq!(keymap.start(), "insert");
    assert_eq!(keymap.modes().collect::<Vec<_>>(), ["insert"], "one mode");
    for binding in keymap.bindings("insert") {
        assert!(
            registry.contains(binding.command()),
            "{} names {}, which is not registered",
            binding.keys(),
            binding.command()
        );
        assert_eq!(binding.layer(), Layer::Default);
    }
    assert_eq!(
        bindings(&keymap, "insert"),
        // Sorted by their keys: modifiers first, so F1 alone leads.
        pairs(&[
            ("F1", "palette.open"),
            ("Ctrl+B", "view.toggleSidePanel"),
            ("Ctrl+J", "view.toggleBottomPanel"),
            ("Ctrl+Shift+P", "palette.open"),
        ])
    );
    // The same text, read again, against the same registry.
    let again = Keymap::read(DEFAULT_TOML, None, &registry, Limits::DEFAULT).expect("reads");
    assert_eq!(again, keymap);
    // Against a registry without the shell's commands, each binding is an
    // unknown command, left out.
    let bare = Keymap::bundled(&Registry::new(), None);
    assert_eq!(bare.bindings("insert"), []);
    assert_eq!(bare.problems().len(), 4);
    assert!(
        bare.problems()
            .iter()
            .all(|problem| matches!(problem, Problem::UnknownCommand { .. }))
    );
}

/// The Accept line's keymap: Normal enters Insert with `i`, and Insert
/// leaves with Escape.
const TWO_MODES: &str = r#"
[keymap.normal]
i = "mode.enter.insert"
j = "cursor.down"
"ctrl+s" = "editor.save"

[keymap.insert]
escape = "mode.leave"
"ctrl+s" = "editor.saveAll"
"#;

#[test]
fn a_keymap_with_two_modes_switches_between_them() {
    let keymap = read(&file("normal", TWO_MODES));
    assert_eq!(keymap.problems(), []);
    assert_eq!(keymap.modes().collect::<Vec<_>>(), ["normal", "insert"]);
    let mut engine = Engine::new(keymap);
    assert_eq!(engine.mode(), "normal");
    assert_eq!(engine.handle(&plain(Key::J)), command("cursor.down"));
    assert_eq!(engine.handle(&ctrl(Key::S)), command("editor.save"));

    assert_eq!(engine.handle(&plain(Key::I)), ENTERED);
    assert_eq!(engine.mode(), "insert");
    assert_eq!(engine.stack().collect::<Vec<_>>(), ["normal", "insert"]);
    // Only the top mode's bindings: `j` is Normal's, so Insert's typed
    // letter reaches no command.
    assert_eq!(engine.handle(&plain(Key::J)), Resolution::Unbound);
    assert_eq!(engine.handle(&plain(Key::I)), Resolution::Unbound);
    assert_eq!(engine.handle(&ctrl(Key::S)), command("editor.saveAll"));

    assert_eq!(engine.handle(&plain(Key::Escape)), ENTERED);
    assert_eq!(engine.mode(), "normal");
    assert_eq!(engine.handle(&plain(Key::Escape)), Resolution::Unbound);
    assert_eq!(engine.handle(&plain(Key::J)), command("cursor.down"));
}

#[test]
fn leaving_never_leaves_the_last_mode_and_the_stack_has_a_depth_limit() {
    let text = file(
        "normal",
        r#"
[keymap.normal]
i = "mode.enter.normal"
escape = "mode.leave"
"#,
    );
    let mut engine = Engine::new(read(&text));
    assert_eq!(
        engine.handle(&plain(Key::Escape)),
        Resolution::Mode {
            changed: false,
            repeat: false
        },
        "the last mode stays"
    );
    assert!(!engine.leave());
    for _ in 1..MAX_MODE_DEPTH {
        assert_eq!(engine.handle(&plain(Key::I)), ENTERED);
    }
    assert_eq!(engine.stack().count(), MAX_MODE_DEPTH);
    assert_eq!(
        engine.handle(&plain(Key::I)),
        Resolution::Mode {
            changed: false,
            repeat: false
        },
        "no deeper than the limit"
    );
    assert!(!engine.enter("normal"));
    assert_eq!(engine.stack().count(), MAX_MODE_DEPTH);
    assert!(engine.leave());
    assert!(!engine.enter("visual"), "not one of the keymap's modes");
    assert_eq!(engine.stack().count(), MAX_MODE_DEPTH - 1);
}

#[test]
fn a_started_sequence_waits_for_its_next_key_and_a_wrong_one_cancels_it() {
    let text = file(
        "normal",
        r#"
[keymap.normal]
"ctrl+k ctrl+s" = "editor.saveAll"
"ctrl+k u" = "editor.undo"
"g g" = "cursor.top"
x = "editor.redo"
"#,
    );
    let mut engine = Engine::new(read(&text));
    assert_eq!(engine.handle(&ctrl(Key::K)), Resolution::Pending);
    assert_eq!(engine.pending(), [Step::parse("ctrl+k").expect("a step")]);
    // Ctrl held down between the two steps goes down again, alone, as an
    // auto-repeat: nothing changes.
    for modifier in [Key::LeftControl, Key::RightControl] {
        assert_eq!(
            engine.handle(&repeat(modifier, Modifiers::CTRL)),
            Resolution::Ignored
        );
    }
    assert_eq!(engine.pending().len(), 1);
    assert_eq!(engine.handle(&ctrl(Key::S)), command("editor.saveAll"));
    assert_eq!(engine.pending(), []);

    assert_eq!(engine.handle(&ctrl(Key::K)), Resolution::Pending);
    assert_eq!(engine.handle(&plain(Key::U)), command("editor.undo"));

    // A key that continues no binding cancels, and is consumed: `x` alone
    // is bound, but not after Ctrl+K.
    assert_eq!(engine.handle(&ctrl(Key::K)), Resolution::Pending);
    assert_eq!(engine.handle(&plain(Key::X)), Resolution::Canceled);
    assert_eq!(engine.pending(), []);
    assert_eq!(engine.handle(&plain(Key::X)), command("editor.redo"));
    assert_eq!(engine.handle(&plain(Key::G)), Resolution::Pending);
    assert_eq!(
        engine.handle(&ctrl(Key::G)),
        Resolution::Canceled,
        "exact modifiers"
    );
    // Nothing pending: a key no binding starts resolves to nothing.
    assert_eq!(engine.handle(&plain(Key::Q)), Resolution::Unbound);
    // The owner may drop a pending sequence.
    assert_eq!(engine.handle(&plain(Key::G)), Resolution::Pending);
    engine.cancel();
    assert_eq!(engine.handle(&plain(Key::X)), command("editor.redo"));
}

#[test]
fn a_change_of_mode_clears_the_pending_steps() {
    let text = file(
        "normal",
        r#"
[keymap.normal]
"g g" = "cursor.top"
"g i" = "mode.enter.insert"

[keymap.insert]
"g g" = "editor.save"
escape = "mode.leave"
"#,
    );
    let mut engine = Engine::new(read(&text));
    assert_eq!(engine.handle(&plain(Key::G)), Resolution::Pending);
    assert!(engine.enter("insert"));
    assert_eq!(engine.pending(), [], "entering cleared it");
    assert_eq!(engine.handle(&plain(Key::G)), Resolution::Pending);
    assert!(engine.leave());
    assert_eq!(engine.pending(), [], "leaving cleared it");
    // So this `g` starts afresh, in Normal.
    assert_eq!(engine.handle(&plain(Key::G)), Resolution::Pending);
    assert_eq!(engine.handle(&plain(Key::I)), ENTERED);
    assert_eq!(engine.pending(), []);
    assert_eq!(engine.handle(&plain(Key::G)), Resolution::Pending);
    assert_eq!(engine.handle(&plain(Key::G)), command("editor.save"));
}

#[test]
fn an_auto_repeat_resolves_like_a_press_and_says_it_was_one() {
    let mut engine = Engine::new(read(&file("normal", TWO_MODES)));
    assert_eq!(
        engine.handle(&repeat(Key::J, Modifiers::NONE)),
        Resolution::Command {
            id: "cursor.down",
            repeat: true
        }
    );
    assert_eq!(
        engine.handle(&repeat(Key::I, Modifiers::NONE)),
        Resolution::Mode {
            changed: true,
            repeat: true
        }
    );
    assert_eq!(engine.mode(), "insert");
    assert_eq!(
        engine.handle(&repeat(Key::I, Modifiers::NONE)),
        Resolution::Unbound
    );
}

#[test]
fn modifier_keys_alone_and_other_events_change_nothing() {
    let text = file(
        "normal",
        r#"
[keymap.normal]
"ctrl+k ctrl+s" = "editor.saveAll"
"#,
    );
    let mut engine = Engine::new(read(&text));
    assert_eq!(engine.handle(&ctrl(Key::K)), Resolution::Pending);
    for (key, modifiers) in [
        (Key::LeftShift, Modifiers::SHIFT | Modifiers::CTRL),
        (Key::RightShift, Modifiers::SHIFT),
        (Key::LeftAlt, Modifiers::ALT),
        (Key::RightAlt, Modifiers::ALT),
        (Key::LeftSuper, Modifiers::SUPER),
        (Key::RightSuper, Modifiers::SUPER),
        (Key::LeftControl, Modifiers::CTRL),
    ] {
        assert_eq!(
            engine.handle(&down(key, modifiers)),
            Resolution::Ignored,
            "{key:?}"
        );
    }
    for event in [
        InputEvent::KeyUp {
            key: Key::K,
            modifiers: Modifiers::CTRL,
        },
        InputEvent::Text("s".into()),
        InputEvent::FocusLost,
    ] {
        assert_eq!(engine.handle(&event), Resolution::Ignored, "{event:?}");
    }
    assert_eq!(engine.pending().len(), 1, "still waiting");
    assert_eq!(engine.handle(&ctrl(Key::S)), command("editor.saveAll"));
    // A step matches exactly its modifiers.
    assert_eq!(engine.handle(&ctrl(Key::K)), Resolution::Pending);
    assert_eq!(
        engine.handle(&down(Key::S, Modifiers::CTRL | Modifiers::SHIFT)),
        Resolution::Canceled
    );
}

#[test]
fn the_same_keys_twice_in_one_mode_and_layer_is_a_conflict_the_first_wins() {
    let text = file(
        "normal",
        r#"
[keymap.normal]
"ctrl+s" = "editor.save"
"Ctrl+S" = "editor.saveAll"
"shift+ctrl+z" = "editor.redo"
"CTRL+SHIFT+Z" = "editor.undo"
"#,
    );
    let keymap = read(&text);
    assert_eq!(
        keymap.problems(),
        [
            Problem::Duplicate {
                kept: place(Layer::Default, "normal", "ctrl+s"),
                dropped: place(Layer::Default, "normal", "Ctrl+S"),
            },
            Problem::Duplicate {
                kept: place(Layer::Default, "normal", "shift+ctrl+z"),
                dropped: place(Layer::Default, "normal", "CTRL+SHIFT+Z"),
            },
        ]
    );
    assert!(
        keymap
            .problems()
            .iter()
            .all(|problem| problem.kind() == ProblemKind::Conflict)
    );
    assert_eq!(
        bindings(&keymap, "normal"),
        pairs(&[("Ctrl+S", "editor.save"), ("Ctrl+Shift+Z", "editor.redo")])
    );
    // A binding left out for an error is not the first of two.
    let keymap = read(&file(
        "normal",
        "[keymap.normal]\n\"ctrl+s\" = \"editor.nothing\"\n\"Ctrl+S\" = \"editor.save\"\n",
    ));
    assert_eq!(
        keymap.problems(),
        [Problem::UnknownCommand {
            at: place(Layer::Default, "normal", "ctrl+s"),
            command: "editor.nothing".into(),
        }]
    );
    assert_eq!(
        bindings(&keymap, "normal"),
        pairs(&[("Ctrl+S", "editor.save")])
    );
    // In the user's layer too.
    let over = user("[keymap.normal]\n\"alt+u\" = \"editor.undo\"\n\"Alt+U\" = \"editor.redo\"\n");
    let keymap = Keymap::read(&text, Some(&over), &registry(), Limits::DEFAULT).expect("reads");
    assert!(keymap.problems().contains(&Problem::Duplicate {
        kept: place(Layer::User, "normal", "alt+u"),
        dropped: place(Layer::User, "normal", "Alt+U"),
    }));
    let message = keymap.problems().last().expect("a problem").to_string();
    assert_eq!(
        message,
        "user layer, [keymap.normal] `Alt+U`: the same keys as `alt+u` before it, which \
         wins; left out"
    );
}

#[test]
fn keys_that_start_another_binding_s_are_a_conflict_the_shorter_wins() {
    let text = file(
        "normal",
        r#"
[keymap.normal]
"ctrl+k ctrl+s" = "editor.saveAll"
"ctrl+k" = "editor.save"
"ctrl+k ctrl+s u" = "editor.undo"
"g" = "cursor.top"
"#,
    );
    let keymap = read(&text);
    assert_eq!(
        keymap.problems(),
        [
            Problem::Prefix {
                kept: place(Layer::Default, "normal", "ctrl+k"),
                dropped: place(Layer::Default, "normal", "ctrl+k ctrl+s"),
            },
            Problem::Prefix {
                kept: place(Layer::Default, "normal", "ctrl+k"),
                dropped: place(Layer::Default, "normal", "ctrl+k ctrl+s u"),
            },
        ]
    );
    assert_eq!(keymap.problems()[0].kind(), ProblemKind::Conflict);
    let mut engine = Engine::new(keymap);
    assert_eq!(engine.handle(&ctrl(Key::K)), command("editor.save"));
    assert_eq!(engine.handle(&ctrl(Key::S)), Resolution::Unbound);

    // Across layers: the user's shorter keys win over the default's longer
    // ones, and the default's shorter over the user's longer.
    let over = user(
        r#"
[keymap.normal]
"g g" = "editor.redo"
"ctrl+k" = ""
"alt+x" = "editor.undo"
"#,
    );
    let defaults = file(
        "normal",
        r#"
[keymap.normal]
g = "cursor.top"
"alt+x alt+y" = "editor.save"
"#,
    );
    let keymap = Keymap::read(&defaults, Some(&over), &registry(), Limits::DEFAULT).expect("reads");
    // In the order of the keys: `g` has no modifiers, so it comes first.
    assert_eq!(
        keymap.problems(),
        [
            Problem::Prefix {
                kept: place(Layer::Default, "normal", "g"),
                dropped: place(Layer::User, "normal", "g g"),
            },
            Problem::Prefix {
                kept: place(Layer::User, "normal", "alt+x"),
                dropped: place(Layer::Default, "normal", "alt+x alt+y"),
            },
        ]
    );
    assert_eq!(
        bindings(&keymap, "normal"),
        pairs(&[("G", "cursor.top"), ("Alt+X", "editor.undo")])
    );
    assert_eq!(
        keymap.problems()[0].to_string(),
        "user layer, [keymap.normal] `g g`: starts with the keys of default layer, \
         [keymap.normal] `g`, which wins, so it could never be reached; left out"
    );
}

#[test]
fn a_user_binding_of_a_default_s_keys_is_an_override_not_a_conflict() {
    let defaults = file("normal", TWO_MODES);
    let over = user(
        r#"
[keymap.normal]
"Ctrl+S" = "editor.saveAll"
j = ""
k = "cursor.down"
"#,
    );
    let keymap = Keymap::read(&defaults, Some(&over), &registry(), Limits::DEFAULT).expect("reads");
    assert_eq!(
        keymap.problems(),
        [
            Problem::Override {
                default: place(Layer::Default, "normal", "ctrl+s"),
                user: place(Layer::User, "normal", "Ctrl+S"),
                unbinds: false,
            },
            Problem::Override {
                default: place(Layer::Default, "normal", "j"),
                user: place(Layer::User, "normal", "j"),
                unbinds: true,
            },
        ]
    );
    assert!(
        keymap
            .problems()
            .iter()
            .all(|problem| problem.kind() == ProblemKind::Override)
    );
    assert_eq!(
        keymap.problems()[0].to_string(),
        "user layer, [keymap.normal] `Ctrl+S`: overrides default layer, [keymap.normal] \
         `ctrl+s`"
    );
    assert_eq!(
        keymap.problems()[1].to_string(),
        "user layer, [keymap.normal] `j`: unbinds default layer, [keymap.normal] `j`"
    );
    let layers: Vec<(String, Layer)> = keymap
        .bindings("normal")
        .iter()
        .map(|binding| (binding.keys().to_string(), binding.layer()))
        .collect();
    assert_eq!(
        layers,
        [
            ("I".to_string(), Layer::Default),
            ("K".to_string(), Layer::User),
            ("Ctrl+S".to_string(), Layer::User),
        ]
    );
    let mut engine = Engine::new(keymap);
    assert_eq!(engine.handle(&ctrl(Key::S)), command("editor.saveAll"));
    assert_eq!(
        engine.handle(&plain(Key::J)),
        Resolution::Unbound,
        "unbound"
    );
    assert_eq!(engine.handle(&plain(Key::K)), command("cursor.down"));
    // An empty string where no layer below binds the keys unbinds nothing.
    let over = user("[keymap.normal]\n\"alt+z\" = \"\"\n");
    let keymap = Keymap::read(&defaults, Some(&over), &registry(), Limits::DEFAULT).expect("reads");
    assert_eq!(keymap.problems(), []);
}

#[test]
fn the_user_s_layer_may_add_modes_and_enter_them() {
    let over = user(
        r#"
[keymap.insert]
"ctrl+g" = "mode.enter.goto"

[keymap.goto]
t = "cursor.top"
escape = "mode.leave"
"#,
    );
    let keymap = Keymap::read(
        &file("normal", TWO_MODES),
        Some(&over),
        &registry(),
        Limits::DEFAULT,
    )
    .expect("reads");
    assert_eq!(keymap.problems(), []);
    assert_eq!(
        keymap.modes().collect::<Vec<_>>(),
        ["normal", "insert", "goto"]
    );
    let mut engine = Engine::new(keymap);
    for event in [plain(Key::I), ctrl(Key::G)] {
        assert_eq!(engine.handle(&event), ENTERED);
    }
    assert_eq!(
        engine.stack().collect::<Vec<_>>(),
        ["normal", "insert", "goto"]
    );
    assert_eq!(engine.handle(&plain(Key::T)), command("cursor.top"));
}

#[test]
fn every_error_is_reported_with_its_place_and_left_out() {
    let text = file(
        "normal",
        r#"
[keymap.normal]
"ctrl+s" = "editor.save"
"ctrl+q" = "editor.quit"
"ctrl+e" = "mode.enter.visual"
"ctrl+f" = "mode.enter.Normal"
"ctrl+m" = "mode.other"
"ctrl+l" = "mode.leave.now"
"ctrl+n" = 3
"ctrl+t" = { command = "editor.save" }
"ctrl+" = "editor.save"
"ctrl+shift" = "editor.save"
"ctrl+leftshift" = "editor.save"
"a b c d e f g h i" = "editor.save"

[keymap.Normal]
x = "editor.save"

[keymap.other]
j = "cursor.down"

[keymap]
bad = "not a table"
"visual.line" = {}
"#,
    );
    let keymap = read(&text);
    let at = |keys: &str| place(Layer::Default, "normal", keys);
    let problems = keymap.problems();
    assert_eq!(
        problems[..2],
        [
            Problem::BadModeName {
                layer: Layer::Default,
                mode: "Normal".into()
            },
            Problem::ModeNotATable {
                layer: Layer::Default,
                mode: "bad".into()
            },
        ]
    );
    assert_eq!(
        problems[2],
        Problem::BadModeName {
            layer: Layer::Default,
            mode: "visual.line".into()
        }
    );
    assert_eq!(
        problems[3..8],
        [
            Problem::UnknownCommand {
                at: at("ctrl+q"),
                command: "editor.quit".into()
            },
            Problem::UnknownMode {
                at: at("ctrl+e"),
                mode: "visual".into()
            },
            Problem::UnknownMode {
                at: at("ctrl+f"),
                mode: "Normal".into()
            },
            Problem::UnknownCommand {
                at: at("ctrl+m"),
                command: "mode.other".into()
            },
            Problem::UnknownCommand {
                at: at("ctrl+l"),
                command: "mode.leave.now".into()
            },
        ]
    );
    assert_eq!(
        problems[8..10],
        [
            Problem::NotAString { at: at("ctrl+n") },
            Problem::NotAString { at: at("ctrl+t") },
        ]
    );
    let kinds: Vec<NotationErrorKind> = problems[10..]
        .iter()
        .map(|problem| match problem {
            Problem::BadKeys { error, .. } => error.kind.clone(),
            other => panic!("not bad keys: {other:?}"),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            NotationErrorKind::EmptyName,
            NotationErrorKind::MissingKey,
            NotationErrorKind::ModifierKey("leftshift".into()),
            NotationErrorKind::TooManySteps { max: 8 },
        ]
    );
    assert!(
        problems
            .iter()
            .all(|problem| problem.kind() == ProblemKind::Error)
    );
    assert_eq!(problems.len(), 14);
    // What read is kept: one binding in each of the two good modes.
    assert_eq!(
        bindings(&keymap, "normal"),
        pairs(&[("Ctrl+S", "editor.save")])
    );
    assert_eq!(bindings(&keymap, "other"), pairs(&[("J", "cursor.down")]));
    assert_eq!(keymap.bindings("Normal"), []);
    assert_eq!(
        problems[3].to_string(),
        "default layer, [keymap.normal] `ctrl+q`: no command is named `editor.quit`; left out"
    );
    assert_eq!(
        problems[13].to_string(),
        "default layer, [keymap.normal] `a b c d e f g h i`: more than 8 steps, from byte \
         16; left out"
    );
}

#[test]
fn messages_escape_what_the_layer_writes() {
    let over =
        user("[keymap.insert]\n\"ctrl+\\u001b[2J\" = \"x\\u202ey\"\n\"f2\" = \"x\\u001by\"\n");
    let keymap = Keymap::bundled(&commands::registry(), Some(&over));
    let messages: Vec<String> = keymap.problems().iter().map(ToString::to_string).collect();
    assert_eq!(messages.len(), 2, "{messages:?}");
    for message in &messages {
        assert!(
            !message.contains(['\u{1b}', '\u{202e}']),
            "unescaped: {message:?}"
        );
    }
    assert!(messages[0].contains("`ctrl+\\u{1b}[2J`"), "{}", messages[0]);
    assert!(messages[1].contains("`x\\u{1b}y`"), "{}", messages[1]);
}

/// `count` modes of one binding each, named `m0`, `m1`… as TOML.
fn modes(count: usize) -> String {
    named_modes("m", count)
}

/// `count` modes of one binding each, named `<prefix>0`, `<prefix>1`… as
/// TOML.
fn named_modes(prefix: &str, count: usize) -> String {
    (0..count)
        .map(|n| format!("[keymap.{prefix}{n}]\nescape = \"mode.leave\"\n"))
        .collect()
}

/// `count` bindings spread over modes of at most 4,000, as TOML.
fn many_bindings(count: usize) -> String {
    let mut text = String::new();
    for n in 0..count {
        if n % 4000 == 0 {
            text.push_str(&format!("[keymap.b{}]\n", n / 4000));
        }
        // Keys no two of which start each other: two steps each.
        let first = n % 26;
        let second = n / 26 % 26;
        let third = n / 676;
        text.push_str(&format!(
            "\"{} {} f{}\" = \"cursor.down\"\n",
            char::from(b'a' + u8::try_from(first).unwrap_or(0)),
            char::from(b'a' + u8::try_from(second).unwrap_or(0)),
            third + 1
        ));
    }
    text
}

#[test]
fn a_layer_reads_up_to_its_limits_and_reports_what_is_past_them() {
    let limits = Limits::DEFAULT;
    assert_eq!(
        (limits.max_bindings, limits.max_steps, limits.max_modes),
        (4096, 8, 32)
    );
    // 32 modes: all read; 33: the last left out.
    let keymap = read(&file("m0", &modes(32)));
    assert_eq!((keymap.problems(), keymap.modes().count()), (&[][..], 32));
    let keymap = read(&file("m0", &modes(33)));
    assert_eq!(
        keymap.problems(),
        [Problem::TooManyModes {
            layer: Layer::Default,
            max: 32
        }]
    );
    assert_eq!(keymap.modes().count(), 32);
    assert!(!keymap.has_mode("m32"));
    // 4,096 bindings over two modes: all read; 4,097: the last left out.
    let keymap = read(&file("b0", &many_bindings(4096)));
    assert_eq!(keymap.problems(), []);
    let read_count: usize = ["b0", "b1"].map(|m| keymap.bindings(m).len()).iter().sum();
    assert_eq!(read_count, 4096);
    let keymap = read(&file("b0", &many_bindings(4097)));
    assert_eq!(
        keymap.problems(),
        [Problem::TooManyBindings {
            layer: Layer::Default,
            max: 4096
        }]
    );
    assert_eq!(keymap.bindings("b1").len(), 96);
    // The user's layer has limits of its own, so the keymap may have more
    // modes than one layer.
    let over = user(&named_modes("u", 33));
    let keymap =
        Keymap::read(&file("m0", &modes(32)), Some(&over), &registry(), limits).expect("reads");
    assert_eq!(
        keymap.problems(),
        [Problem::TooManyModes {
            layer: Layer::User,
            max: 32
        }]
    );
    assert!(
        keymap.problems()[0]
            .to_string()
            .starts_with("user layer: more than 32 modes")
    );
    assert_eq!(keymap.modes().count(), 64);
    // Eight steps read; nine are bad keys.
    let keymap = read(&file(
        "normal",
        "[keymap.normal]\n\"a b c d e f g h\" = \"cursor.top\"\n",
    ));
    assert_eq!(keymap.problems(), []);
    assert_eq!(keymap.bindings("normal")[0].keys().steps().len(), 8);
}

#[test]
fn a_file_that_is_not_a_keymap_is_an_error() {
    let registry = registry();
    let error = |text: &str| match Keymap::read(text, None, &registry, Limits::DEFAULT) {
        Err(EditorError::Keymap(error)) => error,
        other => panic!("{text:?}: {other:?}"),
    };
    let body = "[keymap.normal]\nx = \"editor.save\"\n";
    assert_eq!(
        error(&format!("start = \"normal\"\n{body}")),
        FileError::MissingVersion
    );
    assert_eq!(
        error(&format!("version = \"1\"\nstart = \"normal\"\n{body}")),
        FileError::MissingVersion
    );
    assert_eq!(
        error(&format!("version = 2\nstart = \"normal\"\n{body}")),
        FileError::UnsupportedVersion(2)
    );
    assert_eq!(error(&format!("version = 1\n{body}")), FileError::BadStart);
    assert_eq!(
        error(&format!("version = 1\nstart = \"Normal\"\n{body}")),
        FileError::BadStart
    );
    assert_eq!(
        error(&format!("version = 1\nstart = \"insert\"\n{body}")),
        FileError::UnknownStart("insert".into())
    );
    assert_eq!(
        error("version = 1\nstart = \"normal\"\n"),
        FileError::MissingModes
    );
    assert_eq!(
        error("version = 1\nstart = \"normal\"\nkeymap = 3\n"),
        FileError::MissingModes
    );
    assert_eq!(
        error(&format!(
            "version = 1\nstart = \"normal\"\nname = \"mine\"\n{body}"
        )),
        FileError::UnknownKey("name".into())
    );
    let message = error("version = 7\nstart = \"normal\"\n[keymap.normal]\n").to_string();
    assert!(message.contains("version 1"), "{message}");
    // A start mode whose table did not read is unknown too.
    assert_eq!(
        error("version = 1\nstart = \"normal\"\nkeymap.normal = 3\n"),
        FileError::UnknownStart("normal".into())
    );
    // Not TOML, or over its TOML limits.
    for text in ["version = = 1", "[keymap\n", "version = 1\nversion = 1\n"] {
        assert!(
            matches!(
                Keymap::read(text, None, &registry, Limits::DEFAULT),
                Err(EditorError::KeymapSyntax(_))
            ),
            "{text:?}"
        );
    }
    let small = Limits {
        toml: tidepool_codec::Limits {
            max_input_bytes: 16,
            ..Limits::DEFAULT.toml
        },
        ..Limits::DEFAULT
    };
    assert!(matches!(
        Keymap::read(DEFAULT_TOML, None, &registry, small),
        Err(EditorError::KeymapSyntax(_))
    ));
}

/// A keymap of three modes with chords and mode changes, for the property
/// test.
const BUSY: &str = r#"
[keymap.normal]
i = "mode.enter.insert"
v = "mode.enter.visual"
"g g" = "cursor.top"
"g u u" = "editor.undo"
"ctrl+k ctrl+s" = "editor.saveAll"
"ctrl+w ctrl+w ctrl+w ctrl+w ctrl+w ctrl+w ctrl+w ctrl+w" = "editor.redo"
escape = "mode.leave"

[keymap.insert]
escape = "mode.leave"
"ctrl+o" = "mode.enter.normal"

[keymap.visual]
escape = "mode.leave"
v = "mode.enter.visual"
"g g" = "cursor.top"
"#;

/// The keys the property test presses, with and without Ctrl.
const KEYS: [Key; 10] = [
    Key::I,
    Key::V,
    Key::G,
    Key::U,
    Key::K,
    Key::S,
    Key::W,
    Key::O,
    Key::Escape,
    Key::LeftControl,
];

#[test]
fn the_engine_keeps_its_bounds_whatever_keys_go_down() {
    let keymap = read(&file("normal", BUSY));
    assert_eq!(keymap.problems(), []);
    let longest = ["normal", "insert", "visual"]
        .iter()
        .flat_map(|mode| keymap.bindings(mode))
        .map(|binding| binding.keys().steps().len())
        .max()
        .unwrap_or(0);
    check_sized(
        "the engine's stack and pending steps stay in bounds",
        200,
        |rng, size| {
            (0..size * 4)
                .map(|_| {
                    let key = rng.choose(&KEYS).copied().unwrap_or(Key::G);
                    let ctrl = rng.range(0..3_u32) == 0;
                    let repeat = rng.range(0..5_u32) == 0;
                    (key, ctrl, repeat)
                })
                .collect::<Vec<_>>()
        },
        |presses| {
            let mut engine = Engine::new(keymap.clone());
            for &(key, ctrl, repeat) in presses {
                let modifiers = if ctrl {
                    Modifiers::CTRL
                } else {
                    Modifiers::NONE
                };
                let pending_before = engine.pending().len();
                let resolution = engine.handle(&InputEvent::KeyDown {
                    key,
                    modifiers,
                    repeat,
                });
                match resolution {
                    Resolution::Ignored => {
                        if key != Key::LeftControl {
                            return Err(format!("{key:?} was ignored"));
                        }
                    }
                    Resolution::Command { repeat: said, .. }
                    | Resolution::Mode { repeat: said, .. } => {
                        if said != repeat {
                            return Err("the repeat flag is wrong".into());
                        }
                    }
                    Resolution::Canceled if pending_before == 0 => {
                        return Err("canceled with nothing pending".into());
                    }
                    _ => {}
                }
                let depth = engine.stack().count();
                if depth == 0 || depth > MAX_MODE_DEPTH {
                    return Err(format!("a stack of {depth}"));
                }
                if engine.pending().len() >= longest {
                    return Err(format!("{} steps pending", engine.pending().len()));
                }
                if !engine.keymap().has_mode(engine.mode()) {
                    return Err(format!("in a mode it does not have: {}", engine.mode()));
                }
            }
            Ok(())
        },
    );
}

#[test]
fn sequences_show_in_one_form_that_reads_back() {
    for (written, shown) in [
        ("ctrl+shift+p", "Ctrl+Shift+P"),
        ("SHIFT+CTRL+P", "Ctrl+Shift+P"),
        ("ctrl+k ctrl+s", "Ctrl+K Ctrl+S"),
        ("f1", "F1"),
        ("alt+super+pagedown", "Alt+Super+PageDown"),
        ("ctrl+`", "Ctrl+`"),
        ("shift+'", "Shift+'"),
        ("g g", "G G"),
        ("numpadenter", "NumpadEnter"),
    ] {
        let keys = Sequence::parse(written, Limits::DEFAULT).expect(written);
        assert_eq!(keys.to_string(), shown);
        assert_eq!(Sequence::parse(shown, Limits::DEFAULT), Ok(keys));
    }
}

/// The most problems a layer may give about its modes, and about its
/// bindings, as `keymap::Limits` documents them.
fn layer_bounds(limits: Limits) -> (usize, usize) {
    (limits.max_modes + 1, 2 * limits.max_bindings + 1)
}

#[test]
fn a_user_table_of_100000_entries_that_are_not_modes_gives_problems_within_the_bound() {
    let mut text = String::from("[keymap]\n");
    for n in 0..100_000 {
        text.push_str(&format!("A{n} = 1\n"));
    }
    let over = user(&text);
    assert_eq!(over.len(), 100_000);
    let keymap = Keymap::bundled(&commands::registry(), Some(&over));
    let (modes, _) = layer_bounds(Limits::DEFAULT);
    let problems = keymap.problems();
    assert_eq!(problems.len(), modes, "every entry counts toward the limit");
    assert!(
        problems[..modes - 1]
            .iter()
            .all(|problem| matches!(problem, Problem::BadModeName { .. }))
    );
    assert_eq!(
        problems.last(),
        Some(&Problem::TooManyModes {
            layer: Layer::User,
            max: 32
        })
    );
    // Values that are not tables count the same.
    let mut text = String::from("[keymap]\n");
    for n in 0..100_000 {
        text.push_str(&format!("m{n} = 1\n"));
    }
    let keymap = Keymap::bundled(&commands::registry(), Some(&user(&text)));
    assert_eq!(keymap.problems().len(), modes);
    assert_eq!(keymap.bindings("insert").len(), 4, "the default is kept");
}

#[test]
fn a_mode_of_100000_bad_bindings_gives_problems_within_the_bound() {
    let mut text = String::from("[keymap.insert]\n");
    for n in 0..100_000 {
        text.push_str(&format!("\"bad{n}\" = \"palette.open\"\n"));
    }
    let over = user(&text);
    let keymap = Keymap::bundled(&commands::registry(), Some(&over));
    let (_, bindings) = layer_bounds(Limits::DEFAULT);
    let problems = keymap.problems();
    assert!(problems.len() <= bindings, "{} problems", problems.len());
    assert_eq!(problems.len(), Limits::DEFAULT.max_bindings + 1);
    assert!(
        problems[..Limits::DEFAULT.max_bindings]
            .iter()
            .all(|problem| matches!(problem, Problem::BadKeys { .. }))
    );
    assert_eq!(
        problems.last(),
        Some(&Problem::TooManyBindings {
            layer: Layer::User,
            max: 4096
        })
    );
    // Keys that read, with a value that is not a string or names no
    // command, count the same: four letters each, all different.
    let mut text = String::from("[keymap.insert]\n");
    let letter = |n: usize| char::from(b'a' + u8::try_from(n % 26).unwrap_or(0));
    for n in 0..100_000 {
        let keys = format!(
            "{} {} {} {}",
            letter(n),
            letter(n / 26),
            letter(n / 676),
            letter(n / 17_576)
        );
        let value = if n % 2 == 0 { "1" } else { "\"no.such\"" };
        text.push_str(&format!("\"{keys}\" = {value}\n"));
    }
    let keymap = Keymap::bundled(&commands::registry(), Some(&user(&text)));
    let problems = keymap.problems();
    assert_eq!(problems.len(), Limits::DEFAULT.max_bindings + 1);
    assert!(
        problems[..Limits::DEFAULT.max_bindings]
            .iter()
            .all(|problem| matches!(
                problem,
                Problem::NotAString { .. } | Problem::UnknownCommand { .. }
            ))
    );
}
