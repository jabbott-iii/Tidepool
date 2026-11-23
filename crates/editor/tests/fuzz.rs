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

//! The fuzz target of `tidepool-editor`: `keymap`, one test named after it
//! (ADR-0027, ADR-0064, `docs/04-quality.md` → "Fuzz smoke").
//!
//! A plain `cargo test` runs its saved crashes (`fuzz/crashes/keymap/`) and
//! its seeds unchanged: the hand-made ones in `fuzz/corpus/keymap/`, and the
//! keymaps in `keymaps/`, read in place. `cargo xtask fuzz-smoke` runs them
//! with mutated inputs too, in an optimized profile with overflow checks.
//!
//! The target reads with small [`Limits`] ([`LIMITS`]), and among its seeds
//! are files that sit exactly at each limit (`edge-*`), so that one
//! mutation takes an input over it and into the check. Input that is not
//! UTF-8 is read as `String::from_utf8_lossy` gives it, three ways: as a
//! keymap file; its `keymap` table, if it has one, parsed with `codec`'s
//! default limits as the binary parses `settings.toml`, as the user's
//! layer over the default keymap; and each of its first lines as keys in
//! the notation. Beyond "no panic", it checks what `editor::keymap`'s
//! documentation promises:
//!
//! - a keymap that reads starts in one of its modes; its modes' names are
//!   mode names, each once, at most the limit's in each layer; each mode's
//!   bindings are at most the limit's in each layer, sorted, none starting
//!   another's, each of one to the limit's steps, naming a command the
//!   registry has or one of the engine's for a mode the keymap has; each
//!   binding's keys, shown, read back the same; every problem is of its
//!   kind, and its message holds nothing unescaped that `xtask` escapes;
//!   each layer gives at most `max_modes + 1` problems about its modes
//!   and `2 × max_bindings + 1` about its bindings, however many entries
//!   its tables have; and reading again gives the same keymap;
//! - an engine over it, given the keys going down that the input's first
//!   bytes choose ([`presses`]: any key with any modifiers, or the steps
//!   of the keymap's own bindings, so that chords finish and modes change),
//!   keeps its stack to one to `MAX_MODE_DEPTH` modes of the keymap, fewer
//!   pending steps than the limit, and resolves only to commands the
//!   registry has, with the repeat flag of the key;
//! - a file that is refused as `EditorError::Keymap` has a message with
//!   nothing unescaped; nothing but that and `EditorError::KeymapSyntax`,
//!   whose checks are `codec`'s `toml` target's, is returned;
//! - keys that read have one to the limit's steps and show in a form that
//!   reads back the same, twice; keys that do not read give an offset at a
//!   character boundary of the keys, and a message with nothing unescaped.

use tidepool_codec::CodecError;
use tidepool_codec::toml::{self, ErrorKind};
use tidepool_core::input::{InputEvent, Key, Modifiers};
use tidepool_core::testing::fuzz::{Outcome, Target};
use tidepool_editor::EditorError;
use tidepool_editor::command::Registry;
use tidepool_editor::keymap::{
    DEFAULT_TOML, Engine, Keymap, Layer, Limits, MAX_MODE_DEPTH, Problem, ProblemKind, Resolution,
    Sequence, Step, is_mode_name,
};
use tidepool_editor::shell::commands;

/// The repository root, where `fuzz/` and `keymaps/` are.
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// The limits the target reads with: 16 KiB of TOML, 8 levels, strings of
/// 128 bytes, numbers of 32 and 128 entries a table; 64 bindings and 8
/// modes a layer, and 8 steps, so that a mutated file is either over a
/// limit or reads in microseconds. Every seed is at or under each, and the
/// `edge-*` seeds exactly at one. Built from the defaults, so that a field
/// added later keeps its default.
const LIMITS: Limits = {
    let mut toml = tidepool_codec::Limits::DEFAULT;
    toml.max_input_bytes = 16 * 1024;
    toml.max_depth = 8;
    toml.max_string_bytes = 128;
    toml.max_number_bytes = 32;
    toml.max_entries = 128;
    let mut limits = Limits::DEFAULT;
    limits.toml = toml;
    limits.max_bindings = 64;
    limits.max_steps = 8;
    limits.max_modes = 8;
    limits
};

/// The cap on the rise of the written peak, as for the other targets.
const MEMORY_CAP: u64 = 16 * 1024 * 1024;

/// How many of the input's lines are read as keys.
const MAX_LINES: usize = 64;

/// How many presses of the input drive the engine, each of
/// [`PRESS_BYTES`] bytes.
const MAX_PRESSES: usize = 256;

/// The bytes of one press ([`presses`]).
const PRESS_BYTES: usize = 3;

/// The format's keys and values, the notation's names, and the ids of
/// commands and the engine's.
const TOKENS: &[&[u8]] = &[
    b"version = 1\n",
    b"start = \"insert\"\n",
    b"start = \"normal\"\n",
    b"[keymap.insert]\n",
    b"[keymap.normal]\n",
    b"[keymap]\n",
    b"\"mode.enter.normal\"",
    b"\"mode.enter.insert\"",
    b"\"mode.leave\"",
    b"\"palette.open\"",
    b"\"view.toggleSidePanel\"",
    b"\"window.close\"",
    b"\"\"",
    b"ctrl+",
    b"shift+",
    b"alt+",
    b"super+",
    b"Ctrl+Shift+",
    b" ",
    b"+",
    b"leftshift",
    b"escape",
    b"f1",
    b"pageup",
    b"numpad0",
    b"\\\\",
    b"`",
    b"\\u202e",
    b"\\u001b",
];

/// A run's result line for `fuzz-smoke`, then the verdict.
#[expect(
    clippy::print_stdout,
    reason = "`cargo xtask fuzz-smoke` reads the result line from the test's output; libtest \
              shows it only with --nocapture or when the test fails"
)]
fn report(outcome: &Outcome) {
    println!("{}", outcome.summary());
    outcome.assert_clean();
}

/// Whether `c` is what a message must not show unescaped, as
/// `editor::keymap` documents it.
fn is_escaped(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{2028}'
                | '\u{2029}'
                | '\u{061c}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
        )
}

/// Checks that `message` shows nothing unescaped.
fn check_message(message: &str) {
    assert!(
        !message.chars().any(is_escaped),
        "a message shows a character unescaped: {message:?}"
    );
}

/// `keymap`: the input read three ways, with the checks of the module
/// documentation.
fn read_keymap(input: &[u8]) {
    let text = String::from_utf8_lossy(input);
    let registry = commands::registry();
    match Keymap::read(&text, None, &registry, LIMITS) {
        Ok(keymap) => {
            check_keymap(&keymap, &registry);
            assert_eq!(
                Keymap::read(&text, None, &registry, LIMITS).as_ref().ok(),
                Some(&keymap),
                "read again"
            );
            drive(keymap, input, &registry);
        }
        Err(EditorError::Keymap(error)) => check_message(&error.to_string()),
        Err(EditorError::KeymapSyntax(_)) => {}
        Err(other) => panic!("a keymap file gave an error of another kind: {other:?}"),
    }
    // The user's layer, parsed as the binary parses `settings.toml`: within
    // `codec`'s default limits, so that its tables may have far more
    // entries than the keymap's limits read.
    if let Ok(root) = toml::parse(&text, tidepool_codec::Limits::DEFAULT)
        && let Some(user) = root.get("keymap").and_then(|keymap| keymap.as_table())
    {
        let keymap = Keymap::read(DEFAULT_TOML, Some(user), &registry, LIMITS)
            .unwrap_or_else(|error| panic!("the default keymap within the limits: {error}"));
        check_keymap(&keymap, &registry);
        drive(keymap, input, &registry);
    }
    for line in text.lines().take(MAX_LINES) {
        check_keys(line);
    }
}

/// The rules a keymap that reads keeps.
fn check_keymap(keymap: &Keymap, registry: &Registry) {
    let modes: Vec<&str> = keymap.modes().collect();
    assert!(
        modes.contains(&keymap.start()),
        "starts in {}",
        keymap.start()
    );
    assert!(
        modes.len() <= 2 * LIMITS.max_modes,
        "{} modes in two layers",
        modes.len()
    );
    let mut total = 0;
    for (n, mode) in modes.iter().enumerate() {
        assert!(is_mode_name(mode), "{mode:?} is a mode");
        assert!(!modes[..n].contains(mode), "{mode} twice");
        let bindings = keymap.bindings(mode);
        total += bindings.len();
        for pair in bindings.windows(2) {
            assert!(pair[0].keys() < pair[1].keys(), "sorted, each once");
            assert!(
                !pair[1].keys().is_extension_of(pair[0].keys()),
                "{} starts {}",
                pair[0].keys(),
                pair[1].keys()
            );
        }
        for binding in bindings {
            let steps = binding.keys().steps().len();
            assert!((1..=LIMITS.max_steps).contains(&steps), "{steps} steps");
            let command = binding.command();
            let known = registry.contains(command)
                || command == "mode.leave"
                || command
                    .strip_prefix("mode.enter.")
                    .is_some_and(|entered| modes.contains(&entered));
            assert!(known, "{command:?} is not a command");
            let shown = binding.keys().to_string();
            assert_eq!(
                Sequence::parse(&shown, LIMITS).as_ref(),
                Ok(binding.keys()),
                "{shown} reads back"
            );
        }
    }
    assert!(
        total <= 2 * LIMITS.max_bindings,
        "{total} bindings in two layers"
    );
    // [default, user] × [about modes, about bindings].
    let mut counts = [[0usize; 2]; 2];
    for problem in keymap.problems() {
        let kind = match problem {
            Problem::Duplicate { .. } | Problem::Prefix { .. } => ProblemKind::Conflict,
            Problem::Override { .. } => ProblemKind::Override,
            _ => ProblemKind::Error,
        };
        assert_eq!(problem.kind(), kind, "{problem:?}");
        check_message(&problem.to_string());
        let (layer, about_modes) = layer_of(problem);
        let layer = usize::from(layer == Layer::User);
        counts[layer][usize::from(!about_modes)] += 1;
    }
    for [modes, bindings] in counts {
        assert!(
            modes <= LIMITS.max_modes + 1,
            "{modes} problems about a layer's modes"
        );
        assert!(
            bindings <= 2 * LIMITS.max_bindings + 1,
            "{bindings} problems about a layer's bindings"
        );
    }
    let bound = 2 * (LIMITS.max_modes + 1) + 2 * (2 * LIMITS.max_bindings + 1);
    assert!(
        keymap.problems().len() <= bound,
        "past the documented bound"
    );
}

/// The layer a problem counts against, and whether it is about the layer's
/// modes rather than its bindings: a conflict counts against the binding
/// left out, and an override against the user's binding.
fn layer_of(problem: &Problem) -> (Layer, bool) {
    match problem {
        Problem::Duplicate { dropped, .. } | Problem::Prefix { dropped, .. } => {
            (dropped.layer, false)
        }
        Problem::Override { user, .. } => (user.layer, false),
        Problem::NotAString { at }
        | Problem::BadKeys { at, .. }
        | Problem::UnknownCommand { at, .. }
        | Problem::UnknownMode { at, .. } => (at.layer, false),
        Problem::BadModeName { layer, .. }
        | Problem::ModeNotATable { layer, .. }
        | Problem::TooManyModes { layer, .. } => (*layer, true),
        Problem::TooManyBindings { layer, .. } => (*layer, false),
        other => panic!("a problem the target does not count: {other:?}"),
    }
}

/// The keys going down that `input` chooses for an engine over `keymap`:
/// at most [`MAX_PRESSES`] presses, each [`PRESS_BYTES`] bytes, `[how, a,
/// b]` (a short last one is padded with zeros). The lowest bit of `how`
/// says whether each of the press's keys is an auto-repeat; then, with
/// `n` the number `a` and `b` make, `a` high:
///
/// - `how & 0b110 == 0b010`: the steps of the keymap's binding `n` modulo
///   their number, counted over its modes in order, one after another, so
///   that a chord finishes and `mode.enter` and `mode.leave` run;
/// - `how & 0b110 == 0b100`: one step, number `n` modulo theirs, of all
///   the keymap's bindings' steps in that order;
/// - otherwise, or for a keymap with no bindings: the key `Key::ALL[a]`,
///   `a` modulo their number, with the modifiers of `b`'s four low bits, so
///   that any key, the modifier keys among them, goes down with any
///   modifiers.
fn presses(keymap: &Keymap, input: &[u8]) -> Vec<InputEvent> {
    let bindings: Vec<&[Step]> = keymap
        .modes()
        .flat_map(|mode| keymap.bindings(mode))
        .map(|binding| binding.keys().steps())
        .collect();
    let steps: Vec<Step> = bindings
        .iter()
        .flat_map(|steps| steps.iter().copied())
        .collect();
    let mut events = Vec::new();
    for press in input.chunks(PRESS_BYTES).take(MAX_PRESSES) {
        let byte = |at: usize| press.get(at).copied().unwrap_or(0);
        let (how, a, b) = (byte(0), byte(1), byte(2));
        let repeat = how & 1 == 1;
        let n = usize::from(a) << 8 | usize::from(b);
        let down = |key: Key, modifiers: Modifiers| InputEvent::KeyDown {
            key,
            modifiers,
            repeat,
        };
        match how & 0b110 {
            0b010 if !bindings.is_empty() => {
                let chosen = bindings[n % bindings.len()];
                events.extend(chosen.iter().map(|step| down(step.key(), step.modifiers())));
            }
            0b100 if !steps.is_empty() => {
                let step = steps[n % steps.len()];
                events.push(down(step.key(), step.modifiers()));
            }
            _ => {
                let key = Key::ALL[usize::from(a) % Key::ALL.len()];
                events.push(down(key, Modifiers::from_bits_truncate(b)));
            }
        }
    }
    events
}

/// An engine over `keymap`, given the keys going down that `input` chooses
/// ([`presses`]), keeps its bounds.
fn drive(keymap: Keymap, input: &[u8], registry: &Registry) {
    let events = presses(&keymap, input);
    let mut engine = Engine::new(keymap);
    for event in &events {
        let InputEvent::KeyDown { repeat, .. } = *event else {
            panic!("the driver presses keys only: {event:?}");
        };
        match engine.handle(event) {
            Resolution::Command { id, repeat: said } => {
                assert!(registry.contains(id), "resolved to {id:?}");
                assert_eq!(said, repeat, "the repeat flag");
            }
            Resolution::Mode { repeat: said, .. } => assert_eq!(said, repeat, "the repeat flag"),
            Resolution::Ignored
            | Resolution::Unbound
            | Resolution::Pending
            | Resolution::Canceled => {}
        }
        let depth = engine.stack().count();
        assert!((1..=MAX_MODE_DEPTH).contains(&depth), "a stack of {depth}");
        assert!(
            engine.pending().len() < LIMITS.max_steps,
            "{} steps pending",
            engine.pending().len()
        );
        assert!(engine.keymap().has_mode(engine.mode()), "{}", engine.mode());
    }
}

/// `keys` read in the notation keeps its rules.
fn check_keys(keys: &str) {
    match Sequence::parse(keys, LIMITS) {
        Ok(sequence) => {
            let steps = sequence.steps().len();
            assert!((1..=LIMITS.max_steps).contains(&steps), "{steps} steps");
            let shown = sequence.to_string();
            let again = Sequence::parse(&shown, LIMITS);
            assert_eq!(again.as_ref(), Ok(&sequence), "{shown} reads back");
            assert_eq!(
                again.map(|again| again.to_string()).as_deref(),
                Ok(shown.as_str()),
                "shown twice the same"
            );
        }
        Err(error) => {
            assert!(
                keys.is_char_boundary(error.offset),
                "offset {} of {keys:?}",
                error.offset
            );
            check_message(&error.to_string());
        }
    }
}

#[test]
fn keymap() {
    // Tidepool's own keymaps, read where they are.
    let outcome = Target::new(ROOT, "keymap", read_keymap)
        .seeds_in("keymaps", &["toml"])
        .dictionary(TOKENS)
        .memory_cap(MEMORY_CAP)
        .run();
    report(&outcome);
}

/// Every keymap in `keymaps/`, which the target reads in place as seeds,
/// reads within [`LIMITS`] with no problems: a seed over a limit would be
/// refused or cut, and the fuzzer would mutate a file it does not read
/// whole.
#[test]
fn every_keymap_seed_reads_within_the_limits() {
    let registry = commands::registry();
    let dir = format!("{ROOT}/keymaps");
    let mut read = 0;
    for entry in std::fs::read_dir(&dir).unwrap_or_else(|error| panic!("{dir}: {error}")) {
        let path = entry
            .unwrap_or_else(|error| panic!("{dir}: {error}"))
            .path();
        if path.extension().is_none_or(|extension| extension != "toml") {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let keymap = Keymap::read(&text, None, &registry, LIMITS)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert_eq!(keymap.problems(), [], "{}", path.display());
        read += 1;
    }
    assert!(read > 0, "no keymap in {dir}");
}

/// The `edge-*` seeds sit exactly at each limit: each reads as a keymap
/// with [`LIMITS`] and no problem, and with that limit one lower gives the
/// limit's problem or error; and the other seeds read as keymaps.
#[test]
fn the_edge_seeds_sit_exactly_at_each_limit() {
    let registry = commands::registry();
    let seed = |name: &str| {
        let path = format!("{ROOT}/fuzz/corpus/keymap/{name}");
        std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
    };
    let read = |text: &str, limits: Limits| Keymap::read(text, None, &registry, limits);
    for name in ["minimal.toml", "problems.toml"] {
        let keymap = read(&seed(name), LIMITS);
        assert!(keymap.is_ok(), "{name}: {keymap:?}");
    }
    assert_eq!(
        read(&seed("minimal.toml"), LIMITS)
            .map(|keymap| keymap.problems().len())
            .ok(),
        Some(0)
    );
    assert!(
        read(&seed("problems.toml"), LIMITS)
            .map(|keymap| keymap.problems().len())
            .is_ok_and(|count| count >= 8),
        "problems.toml has a problem of each kind"
    );
    for field in ["bindings", "steps", "modes", "input"] {
        let file = format!("edge-{field}.toml");
        let text = seed(&file);
        assert!(text.len() <= LIMITS.toml.max_input_bytes, "{file}");
        let keymap = read(&text, LIMITS);
        assert_eq!(
            keymap.as_ref().map(|keymap| keymap.problems()).ok(),
            Some(&[][..]),
            "{file}: {keymap:?}"
        );
        let mut lower = LIMITS;
        match field {
            "bindings" => lower.max_bindings -= 1,
            "steps" => lower.max_steps -= 1,
            "modes" => lower.max_modes -= 1,
            _ => lower.toml.max_input_bytes -= 1,
        }
        let lowered = read(&text, lower);
        let expected = match (&lowered, field) {
            (Ok(keymap), "bindings") => {
                keymap.problems()
                    == [Problem::TooManyBindings {
                        layer: tidepool_editor::keymap::Layer::Default,
                        max: lower.max_bindings,
                    }]
            }
            (Ok(keymap), "steps") => {
                matches!(keymap.problems(), [Problem::BadKeys { .. }])
            }
            (Ok(keymap), "modes") => {
                keymap.problems()
                    == [Problem::TooManyModes {
                        layer: tidepool_editor::keymap::Layer::Default,
                        max: lower.max_modes,
                    }]
            }
            (Err(EditorError::KeymapSyntax(CodecError::Toml { kind, .. })), "input") => {
                matches!(kind, ErrorKind::InputTooLarge { .. })
            }
            _ => false,
        };
        assert!(expected, "{file}: {lowered:?}");
    }
}

/// The driver can press every binding of the default keymap, each step as
/// any key with any modifiers and the whole binding as one press, and the
/// engine resolves it to its command; and its binding presses run the
/// engine's mode commands.
#[test]
fn the_driver_presses_every_default_binding_and_changes_modes() {
    let registry = commands::registry();
    let keymap = Keymap::bundled(&registry, None);
    let bindings = keymap.bindings("insert");
    assert_eq!(bindings.len(), 4);
    for (at, binding) in bindings.iter().enumerate() {
        let expected: Vec<InputEvent> = binding
            .keys()
            .steps()
            .iter()
            .map(|step| InputEvent::KeyDown {
                key: step.key(),
                modifiers: step.modifiers(),
                repeat: false,
            })
            .collect();
        let mut by_key = Vec::new();
        for step in binding.keys().steps() {
            let key = Key::ALL
                .iter()
                .position(|&key| key == step.key())
                .and_then(|key| u8::try_from(key).ok())
                .expect("a key of Key::ALL");
            by_key.extend([0, key, step.modifiers().bits()]);
        }
        let by_binding = [0b010, 0, u8::try_from(at).expect("few bindings")];
        for input in [by_key.as_slice(), &by_binding] {
            let events = presses(&keymap, input);
            assert_eq!(events, expected, "{} from {input:?}", binding.keys());
            let mut engine = Engine::new(keymap.clone());
            let (last, before) = events.split_last().expect("a step");
            for event in before {
                assert_eq!(engine.handle(event), Resolution::Pending);
            }
            assert_eq!(
                engine.handle(last),
                Resolution::Command {
                    id: binding.command(),
                    repeat: false
                },
                "{}",
                binding.keys()
            );
        }
    }
    // Any key goes down with any modifiers, the modifier keys too.
    for (at, &key) in Key::ALL.iter().enumerate() {
        let key_byte = u8::try_from(at).expect("118 keys");
        for bits in 0..16u8 {
            let events = presses(&keymap, &[0, key_byte, bits]);
            assert_eq!(
                events,
                [InputEvent::KeyDown {
                    key,
                    modifiers: Modifiers::from_bits_truncate(bits),
                    repeat: false
                }]
            );
        }
    }
    // A binding press runs `mode.enter` and `mode.leave`, a step press
    // presses one step of a chord, and the low bit makes a repeat.
    let text = "version = 1\nstart = \"normal\"\n[keymap.normal]\n\"g i\" = \"mode.enter.insert\"\n[keymap.insert]\nescape = \"mode.leave\"\n";
    let keymap = Keymap::read(text, None, &registry, LIMITS).expect("a keymap");
    let mut engine = Engine::new(keymap.clone());
    // The bindings in order: Normal's `g i`, then Insert's Escape.
    for (input, mode) in [([0b010, 0, 0], "insert"), ([0b010, 0, 1], "normal")] {
        for event in presses(&keymap, &input) {
            engine.handle(&event);
        }
        assert_eq!(engine.mode(), mode);
    }
    let steps = presses(&keymap, &[0b100, 0, 0, 0b101, 0, 1]);
    assert_eq!(steps.len(), 2);
    assert!(matches!(
        steps[1],
        InputEvent::KeyDown {
            key: Key::I,
            repeat: true,
            ..
        }
    ));
}
