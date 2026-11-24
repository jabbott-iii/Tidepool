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

//! The fuzz targets of `tidepool-workspace`: `ignore`, `settings` and
//! `session`, one test named after each (ADR-0027, ADR-0067, ADR-0068,
//! `docs/04-quality.md` → "Fuzz smoke").
//!
//! A plain `cargo test` runs their saved crashes (`fuzz/crashes/<target>/`)
//! and their seeds unchanged, the hand-made ones in `fuzz/corpus/<target>/`.
//! `cargo xtask fuzz-smoke` runs them with mutated inputs too, in an
//! optimized profile with overflow checks.
//!
//! `ignore` reads any bytes as an ignore file with small [`IgnoreLimits`]
//! ([`LIMITS`]), whose `edge-*` seeds sit exactly at each limit. Beyond "no
//! panic", it checks what `workspace::folder` documents:
//!
//! - a file that parses has at most the limit's patterns; a refusal is one
//!   of the three limits', naming a line the input has, and its message
//!   holds nothing unescaped;
//! - for paths the input's lines give, [`Ignore::is_ignored`] with the file
//!   as the root's `.gitignore` is exactly "a folder above the path, or
//!   the path itself, is matched to be ignored, or a name is `.git`", the
//!   same twice, and the same file as a `.tidepoolignore` over an empty
//!   `.gitignore` gives the same answers;
//! - the glob matcher agrees with a port of Git's `wildmatch` (Git's
//!   `wildmatch.c`, which backtracks), for the input's first line as a
//!   pattern and its other lines as paths, when they are small enough for
//!   backtracking to be quick.
//!
//! `settings` reads any bytes as a user's settings file and as a trusted
//! workspace's over it (a folder in the system's temporary folder, trusted
//! once a run), against a schema of each kind, and as a trust file:
//!
//! - the same bytes as a workspace's file that no trusted folder gave give
//!   only the defaults, and one warning;
//! - every declared setting has a value of its kind, within its range or
//!   among its choices, from the default or a layer given; the warnings
//!   are at most one for the file, one a table on the way, and one a
//!   setting, and hold nothing unescaped;
//! - for a file that can be used, setting each scalar to a value the input
//!   picks either leaves the file unchanged with an edit error, or gives a
//!   file whose settings hold that value, from that layer, with every
//!   other setting as it was;
//! - a trust file that parses gives at most `MAX_GRANTS` grants, sorted,
//!   each once, each absolute and spelled as its names join; a refusal is
//!   one of the documented errors, and holds nothing unescaped.
//!
//! `session` reads any bytes as a session file, with the session's own
//! limits, whose `edge-*` seeds sit exactly at the limits a seed can reach
//! (splits 8 deep, 16 panes, 64 groups, 1,024 cursors):
//!
//! - a session that parses keeps every limit: splits at most
//!   [`MAX_DEPTH`] deep, of 2 to [`MAX_PANES`] panes with finite weights
//!   above 0; at most [`MAX_GROUPS`] groups and [`MAX_EDITORS`] editors,
//!   each group's `active` an editor of it (or 0 when it has none), and
//!   `active_group` a group; each editor's path absolute, with 1 to
//!   [`MAX_CURSORS`] cursors and finite scroll offsets from 0;
//! - it writes back ([`Session::to_text`]) to a text that reads as the
//!   same session, and writing that again gives the same text;
//! - a refusal is one of the documented errors, and holds nothing
//!   unescaped.
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use tidepool_core::testing::fuzz::{Outcome, Target};
use tidepool_workspace::WorkspaceError;
use tidepool_workspace::folder::{Folder, Ignore, IgnoreFile, IgnoreLimits, Match};
use tidepool_workspace::session::{
    self, Group, Layout, MAX_CURSORS, MAX_DEPTH, MAX_EDITORS, MAX_GROUPS, MAX_PANES, Session,
};
use tidepool_workspace::settings::{Kind, Schema, Scope, Settings, SettingsFile, Source, Value};
use tidepool_workspace::trust::{self, TrustStore};

/// The repository root, where `fuzz/` is.
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// The limits `ignore` reads with: 16 KiB, 64 patterns and 256 bytes a
/// pattern, so that a mutated file is either over a limit or matches in
/// microseconds. Built from the defaults, so a field added later keeps
/// its default.
const LIMITS: IgnoreLimits = {
    let mut limits = IgnoreLimits::DEFAULT;
    limits.max_file_bytes = 16 * 1024;
    limits.max_patterns = 64;
    limits.max_pattern_bytes = 256;
    limits
};

/// The cap on the rise of the written peak, as for the other targets.
const MEMORY_CAP: u64 = 16 * 1024 * 1024;

/// How many of the input's lines are matched as paths.
const MAX_PATHS: usize = 32;

/// The largest pattern and path compared with the `wildmatch` port, and
/// the most `*` in the pattern, so that its backtracking stays quick.
const MAX_REFERENCE_PATTERN: usize = 40;
const MAX_REFERENCE_PATH: usize = 64;
const MAX_REFERENCE_STARS: usize = 6;

/// The ignore files' syntax.
const IGNORE_TOKENS: &[&[u8]] = &[
    b"**/",
    b"/**",
    b"/**/",
    b"**",
    b"*",
    b"?",
    b"[",
    b"]",
    b"[!",
    b"[^",
    b"-",
    b"\\",
    b"!",
    b"#",
    b"/",
    b"\n",
    b"\r\n",
    b" ",
    b"\\ ",
    b"[:alpha:]",
    b"[:digit:]",
    b"[:space:]",
    b"[:punct:]",
    b"[:nope:]",
    b".git",
    b"target",
    b"\xef\xbb\xbf",
    b"\0",
];

/// The settings and trust files' keys and values.
const SETTINGS_TOKENS: &[&[u8]] = &[
    b"version = 1\n",
    b"version = 2\n",
    b"[ui]\n",
    b"side_panel = ",
    b"ui.side_panel = ",
    b"[editor]\n",
    b"tab_size = ",
    b"keymap = ",
    b"[keymap.insert]\n",
    b"\"vim\"",
    b"\"default\"",
    b"true",
    b"false",
    b"16",
    b"17",
    b"0",
    b"folders = [",
    b"\"/a/b\"",
    b"\"/a/../b\"",
    b"\"rel\"",
    b"]\n",
    b"{ ",
    b" }",
    b"# c\n",
    b"\\u202e",
];

/// The session file's keys and values.
const SESSION_TOKENS: &[&[u8]] = &[
    b"version = 1\n",
    b"folder = \"/w\"\n",
    b"active_group = ",
    b"layout = ",
    b"{ split = \"row\", panes = [",
    b"{ split = \"column\", panes = [",
    b"{ weight = 1.0, layout = ",
    b"{ active = 0, editors = [",
    b"{ path = \"/w/a.rs\", primary = 0, cursors = [[0, 0]], ",
    b"scroll = { line = 0, row = 0, y = 0.0, x = 0.0 } }",
    b"[0, 0], ",
    b"] }",
    b" }, ",
    b"0",
    b"1",
    b"-1",
    b"1e39",
    b"nan",
    b"inf",
    b"\"/w/../x\"",
    b"\"rel\"",
    b"\\u202e",
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

/// Checks that `message` shows nothing unescaped that a message must not:
/// a control character, a line or paragraph separator, or a bidirectional
/// control.
fn check_message(message: &str) {
    let hidden = |c: char| {
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
    };
    assert!(
        !message.chars().any(hidden),
        "a message shows a character unescaped: {message:?}"
    );
}

#[test]
fn ignore() {
    let outcome = Target::new(ROOT, "ignore", read_ignore)
        .dictionary(IGNORE_TOKENS)
        .memory_cap(MEMORY_CAP)
        .run();
    report(&outcome);
}

#[test]
fn settings() {
    let outcome = Target::new(ROOT, "settings", read_settings)
        .dictionary(SETTINGS_TOKENS)
        .memory_cap(MEMORY_CAP)
        .run();
    report(&outcome);
}

#[test]
fn session() {
    let outcome = Target::new(ROOT, "session", read_session)
        .dictionary(SESSION_TOKENS)
        .memory_cap(MEMORY_CAP)
        .run();
    report(&outcome);
}

/// The input's lines, each cut at a NUL and its carriage return dropped,
/// as relative paths: empty names and `.` and `..` dropped. A line with a
/// `\\` or a `:` is left out, since Windows reads those as separators and
/// drives, and the seeds run on every system.
fn paths(input: &[u8]) -> Vec<Vec<u8>> {
    input
        .split(|&byte| byte == b'\n')
        .take(MAX_PATHS)
        .filter_map(|line| {
            let line = line.split(|&byte| byte == 0).next().unwrap_or_default();
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            if line.contains(&b'\\') || line.contains(&b':') {
                return None;
            }
            let names: Vec<&[u8]> = line
                .split(|&byte| byte == b'/')
                .filter(|name| !name.is_empty() && *name != b"." && *name != b"..")
                .collect();
            (!names.is_empty()).then(|| names.join(&b'/'))
        })
        .collect()
}

/// `bytes` as a relative path, `/` between its names. The input's bytes
/// are kept only when they are UTF-8, since a path of any bytes needs a
/// platform `cfg` to build; the rest are replaced.
fn as_path(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

/// `ignore`: the input as an ignore file, its lines as paths, and its first
/// line against the `wildmatch` port.
fn read_ignore(input: &[u8]) {
    let origin = Path::new("fuzz/.gitignore");
    let file = match IgnoreFile::parse(input, origin, &LIMITS) {
        Ok(file) => file,
        Err(error) => {
            let lines = input.split(|&byte| byte == b'\n').count();
            match &error {
                WorkspaceError::TooLarge { limit, .. } => {
                    assert_eq!(*limit, LIMITS.max_file_bytes);
                    assert!(input.len() as u64 > LIMITS.max_file_bytes);
                }
                WorkspaceError::TooManyPatterns { limit, .. } => {
                    assert_eq!(*limit, LIMITS.max_patterns);
                }
                WorkspaceError::PatternTooLong { line, limit, .. } => {
                    assert_eq!(*limit, LIMITS.max_pattern_bytes);
                    assert!((1..=lines).contains(line), "line {line} of {lines}");
                }
                other => panic!("an ignore file gave an error of another kind: {other:?}"),
            }
            check_message(&error.to_string());
            return;
        }
    };
    assert!(file.len() <= LIMITS.max_patterns);

    let mut as_git = Ignore::new();
    as_git.set_git_ignore(Path::new(""), file.clone());
    let mut as_tidepool = Ignore::new();
    as_tidepool.set_tidepool_ignore(Path::new(""), file.clone());
    let empty = IgnoreFile::parse(b"", origin, &LIMITS).expect("an empty file");
    as_tidepool.set_git_ignore(Path::new(""), empty);
    for path in paths(input) {
        for is_dir in [false, true] {
            let expected = expected_ignored(&file, &path, is_dir);
            let shown = as_path(&path);
            // A replaced byte changes the path, so compare it as `Ignore`
            // sees it.
            let seen = shown.to_string_lossy().into_owned().into_bytes();
            let expected = if seen == path {
                expected
            } else {
                expected_ignored(&file, &seen, is_dir)
            };
            let got = as_git.is_ignored(&shown, is_dir);
            assert_eq!(got, expected, "{path:?} dir={is_dir}");
            assert_eq!(as_git.is_ignored(&shown, is_dir), got, "the same twice");
            assert_eq!(
                as_tidepool.is_ignored(&shown, is_dir),
                got,
                "as a `.tidepoolignore`"
            );
        }
    }
    compare_with_wildmatch(input);
}

#[test]
fn the_differential_seeds_are_compared_with_wildmatch() {
    for (name, paths) in [("differential.ignore", 6), ("differential-name.ignore", 4)] {
        let path = format!("{ROOT}/fuzz/corpus/ignore/{name}");
        let seed = std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
        assert_eq!(compare_with_wildmatch(&seed), paths, "{name}");
    }
}

/// What the rules say of `path` with `file` as the root's only ignore file:
/// a name `.git`, or a folder above it, or it, matched to be ignored.
fn expected_ignored(file: &IgnoreFile, path: &[u8], is_dir: bool) -> bool {
    let names: Vec<&[u8]> = path.split(|&byte| byte == b'/').collect();
    for end in 1..=names.len() {
        if names[end - 1] == b".git" {
            return true;
        }
        let prefix = names[..end].join(&b'/');
        let folder = end < names.len() || is_dir;
        if file.matched(&prefix, folder) == Match::Ignore {
            return true;
        }
    }
    false
}

/// The input's first line as a pattern, its others as paths, compared
/// with the `wildmatch` port where both are small. The pattern is used
/// only when its line is itself (no `!`, `#`, final `/` or space, or line
/// break), so that only the glob decides. Returns how many paths it
/// compared.
fn compare_with_wildmatch(input: &[u8]) -> usize {
    let mut lines = input.split(|&byte| byte == b'\n');
    let Some(pattern) = lines.next() else {
        return 0;
    };
    let plain = !pattern.is_empty()
        && pattern.len() <= MAX_REFERENCE_PATTERN
        && pattern.iter().filter(|&&byte| byte == b'*').count() <= MAX_REFERENCE_STARS
        && !matches!(pattern[0], b'!' | b'#')
        && !matches!(pattern[pattern.len() - 1], b'/' | b' ' | b'\r')
        && !pattern.contains(&0)
        && !pattern.starts_with(b"\xef\xbb\xbf");
    if !plain {
        return 0;
    }
    let mut text = pattern.to_vec();
    text.push(b'\n');
    let Ok(file) = IgnoreFile::parse(&text, Path::new("fuzz"), &LIMITS) else {
        return 0;
    };
    let mut compared = 0;
    let anchored = pattern.contains(&b'/');
    let glob = pattern.strip_prefix(b"/").unwrap_or(pattern);
    for line in lines.take(MAX_PATHS) {
        let path = line;
        if path.is_empty() || path.len() > MAX_REFERENCE_PATH || path.contains(&0) {
            continue;
        }
        let subject = if anchored {
            path
        } else {
            match path.iter().rposition(|&byte| byte == b'/') {
                Some(slash) => &path[slash + 1..],
                None => path,
            }
        };
        let reference = wildmatch(glob, subject) == MATCH;
        let got = file.matched(path, false) == Match::Ignore;
        assert_eq!(got, reference, "pattern {pattern:?} on {path:?}");
        compared += 1;
    }
    compared
}

/// `wildmatch`'s answers.
const MATCH: i8 = 0;
const NO_MATCH: i8 = 1;
const ABORT_ALL: i8 = -1;
const ABORT_TO_STARSTAR: i8 = -2;

/// A port of Git's `dowild` (`wildmatch.c`) with `WM_PATHNAME` and case
/// kept, without its speed-up for a literal after `*`, which changes no
/// answer. Backtracking, so only for small inputs.
fn wildmatch(pattern: &[u8], text: &[u8]) -> i8 {
    let mut p = 0;
    let mut t = 0;
    while p < pattern.len() {
        let p_ch = pattern[p];
        let t_ch = text.get(t).copied();
        if t_ch.is_none() && p_ch != b'*' {
            return ABORT_ALL;
        }
        match p_ch {
            b'\\' => {
                p += 1;
                // A final backslash compares with NUL, which no text has.
                if pattern.get(p).copied() != t_ch {
                    return NO_MATCH;
                }
            }
            b'?' => {
                if t_ch == Some(b'/') {
                    return NO_MATCH;
                }
            }
            b'*' => {
                p += 1;
                let match_slash = if pattern.get(p) == Some(&b'*') {
                    let before_first = p.checked_sub(2).map(|at| pattern[at]);
                    while pattern.get(p) == Some(&b'*') {
                        p += 1;
                    }
                    let next = pattern.get(p).copied();
                    if (before_first.is_none() || before_first == Some(b'/'))
                        && (next.is_none()
                            || next == Some(b'/')
                            || (next == Some(b'\\') && pattern.get(p + 1) == Some(&b'/')))
                    {
                        if next == Some(b'/') && wildmatch(&pattern[p + 1..], &text[t..]) == MATCH {
                            return MATCH;
                        }
                        true
                    } else {
                        false
                    }
                } else {
                    false
                };
                if p == pattern.len() {
                    if !match_slash && text[t..].contains(&b'/') {
                        return ABORT_TO_STARSTAR;
                    }
                    return MATCH;
                }
                if !match_slash && pattern[p] == b'/' {
                    match text[t..].iter().position(|&byte| byte == b'/') {
                        None => return ABORT_ALL,
                        Some(at) => {
                            t += at;
                            // The slash is matched by the loop's next step.
                            continue;
                        }
                    }
                }
                loop {
                    let Some(&here) = text.get(t) else {
                        return ABORT_ALL;
                    };
                    let matched = wildmatch(&pattern[p..], &text[t..]);
                    if matched != NO_MATCH {
                        if !match_slash || matched != ABORT_TO_STARSTAR {
                            return matched;
                        }
                    } else if !match_slash && here == b'/' {
                        return ABORT_TO_STARSTAR;
                    }
                    t += 1;
                }
            }
            b'[' => match bracket(pattern, p, t_ch.unwrap_or(0)) {
                Some((matched, close)) => {
                    if !matched || t_ch == Some(b'/') {
                        return NO_MATCH;
                    }
                    p = close;
                }
                None => return ABORT_ALL,
            },
            _ => {
                if t_ch != Some(p_ch) {
                    return NO_MATCH;
                }
            }
        }
        p += 1;
        t += 1;
    }
    if t < text.len() { NO_MATCH } else { MATCH }
}

/// `dowild`'s `[` case: whether `t_ch` is in the set at `open`, and the
/// index of its `]`; `None` where `dowild` aborts.
fn bracket(pattern: &[u8], open: usize, t_ch: u8) -> Option<(bool, usize)> {
    let mut p = open + 1;
    let mut p_ch = *pattern.get(p)?;
    if p_ch == b'!' {
        p_ch = b'^';
    }
    let negated = p_ch == b'^';
    if negated {
        p += 1;
        p_ch = *pattern.get(p)?;
    }
    let mut prev_ch = 0u8;
    let mut matched = false;
    loop {
        if p_ch == b'\\' {
            p += 1;
            p_ch = *pattern.get(p)?;
            if t_ch == p_ch {
                matched = true;
            }
        } else if p_ch == b'-'
            && prev_ch != 0
            && pattern.get(p + 1).is_some_and(|&next| next != b']')
        {
            p += 1;
            p_ch = pattern[p];
            if p_ch == b'\\' {
                p += 1;
                p_ch = *pattern.get(p)?;
            }
            if t_ch <= p_ch && t_ch >= prev_ch {
                matched = true;
            }
            p_ch = 0;
        } else if p_ch == b'[' && pattern.get(p + 1) == Some(&b':') {
            let start = p + 2;
            let mut end = start;
            while end < pattern.len() && pattern[end] != b']' {
                end += 1;
            }
            if end >= pattern.len() {
                return None;
            }
            if end == start || pattern[end - 1] != b':' {
                // No `:]`: the `[` is a member, and the `:` comes next.
                p_ch = b'[';
                if t_ch == p_ch {
                    matched = true;
                }
            } else {
                let space = matches!(t_ch, b'\t' | b'\n' | b'\r' | b' ');
                let is = match &pattern[start..end - 1] {
                    b"alnum" => t_ch.is_ascii_alphanumeric(),
                    b"alpha" => t_ch.is_ascii_alphabetic(),
                    b"blank" => matches!(t_ch, b' ' | b'\t'),
                    b"cntrl" => t_ch.is_ascii_control(),
                    b"digit" => t_ch.is_ascii_digit(),
                    b"graph" => (0x20..0x7f).contains(&t_ch) && !space,
                    b"lower" => t_ch.is_ascii_lowercase(),
                    b"print" => (0x20..0x7f).contains(&t_ch),
                    b"punct" => t_ch.is_ascii_punctuation(),
                    b"space" => space,
                    b"upper" => t_ch.is_ascii_uppercase(),
                    b"xdigit" => t_ch.is_ascii_hexdigit(),
                    _ => return None,
                };
                if is {
                    matched = true;
                }
                p = end;
                p_ch = 0;
            }
        } else if t_ch == p_ch {
            matched = true;
        }
        prev_ch = p_ch;
        p += 1;
        p_ch = *pattern.get(p)?;
        if p_ch == b']' {
            break;
        }
    }
    Some((matched != negated, p))
}

/// The schema `settings` resolves against: one setting of each kind.
fn schema() -> Schema {
    let mut schema = Schema::new();
    for (key, kind) in [
        ("ui.side_panel", Kind::Bool { default: true }),
        ("ui.bottom_panel", Kind::Bool { default: false }),
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
        (
            "editor.font",
            Kind::String {
                default: "JetBrains Mono",
                choices: &[],
            },
        ),
        ("keymap", Kind::Table),
    ] {
        schema.add(key, kind).expect("a valid declaration");
    }
    schema
}

/// Checks every setting's value against its kind.
fn check_values(schema: &Schema, settings: &Settings, layers: &[Source]) {
    for key in schema.keys() {
        let kind = schema.kind(key).expect("declared");
        let source = settings.source(key);
        match (kind, settings.get(key)) {
            (Kind::Bool { .. }, Some(Value::Bool(_))) => {}
            (Kind::Integer { min, max, .. }, Some(Value::Integer(value))) => {
                assert!((*min..=*max).contains(value), "{key} = {value}");
            }
            (Kind::String { choices, .. }, Some(Value::String(value))) => {
                assert!(
                    choices.is_empty() || choices.contains(&value.as_str()),
                    "{key}"
                );
            }
            (Kind::Table, Some(Value::Table(_)) | None) => {}
            (kind, value) => panic!("{key}: {value:?} is not a {kind:?}"),
        }
        if let Some(source) = source {
            assert!(
                source == Source::Default || layers.contains(&source),
                "{key} from {source:?}"
            );
        }
    }
    let keys = schema.keys().count();
    assert!(
        settings.warnings().len() <= layers.len() * (2 * keys + 1),
        "{} warnings",
        settings.warnings().len()
    );
    for warning in settings.warnings() {
        check_message(&warning.to_string());
    }
}

/// A folder that a trust store trusts, `tidepool-fuzz-settings` in the
/// system's temporary folder, for the workspace's layer.
fn trusted_folder() -> &'static (Folder, TrustStore) {
    static FOLDER: OnceLock<(Folder, TrustStore)> = OnceLock::new();
    FOLDER.get_or_init(|| {
        // One fixed folder, shared by runs, so that none are left behind;
        // trusting it again changes nothing.
        let dir = std::env::temp_dir().join("tidepool-fuzz-settings");
        std::fs::create_dir_all(dir.join("work")).expect("make the scratch folder");
        let folder = Folder::open(dir.join("work")).expect("open the scratch folder");
        let mut store = TrustStore::load(&dir.join("data"));
        store.trust(&folder).expect("trust the scratch folder");
        (folder, store)
    })
}

/// `settings`: the input as a user's settings, a workspace's over them, an
/// edit of each scalar, and a trust file.
fn read_settings(input: &[u8]) {
    let schema = schema();
    let user = SettingsFile::from_bytes(Scope::User, "fuzz/settings.toml", Some(input));
    let alone = Settings::resolve(&schema, Some(&user), None);
    check_values(&schema, &alone, &[Source::User]);
    // Compared without the keymap's table, since a NaN in it is not equal
    // to itself.
    let again = Settings::resolve(&schema, Some(&user), None);
    assert_eq!(again.warnings(), alone.warnings(), "resolved again");
    for key in schema.keys().filter(|key| *key != "keymap") {
        assert_eq!(again.get(key), alone.get(key), "{key} resolved again");
    }
    // A workspace's file not from a trusted folder gives nothing.
    let untrusted = SettingsFile::from_bytes(
        Scope::Workspace,
        "fuzz/.tidepool/settings.toml",
        Some(input),
    );
    let refused = Settings::resolve(&schema, None, Some(&untrusted));
    assert_eq!(refused.warnings().len(), 1, "one warning for the file");
    for key in schema.keys() {
        assert_eq!(
            refused.source(key).unwrap_or(Source::Default),
            Source::Default,
            "{key}"
        );
    }
    let (folder, store) = trusted_folder();
    let workspace = SettingsFile::workspace_from_bytes(folder, store, Some(input))
        .expect("the folder is trusted");
    let both = Settings::resolve(&schema, Some(&user), Some(&workspace));
    check_values(&schema, &both, &[Source::User, Source::Workspace]);
    if let Some(problem) = user.problem() {
        check_message(&problem.to_string());
        assert!(matches!(
            problem,
            WorkspaceError::Toml { .. }
                | WorkspaceError::Version { .. }
                | WorkspaceError::TooLarge { .. }
        ));
    }

    // Each scalar set to a value the input picks.
    let pick = input.iter().fold(0usize, |hash, &byte| {
        hash.wrapping_mul(31).wrapping_add(usize::from(byte))
    });
    let values = [
        ("ui.side_panel", Value::Bool(pick % 2 == 0)),
        ("ui.bottom_panel", Value::Bool(pick % 3 == 0)),
        (
            "editor.tab_size",
            Value::Integer(i64::try_from(pick % 16).unwrap_or(0) + 1),
        ),
        (
            "editor.keymap",
            Value::String(if pick % 5 == 0 { "vim" } else { "default" }.to_string()),
        ),
        (
            "editor.font",
            Value::String(format!("F\u{202e}{}\"\\", pick % 7)),
        ),
    ];
    for (key, value) in values {
        let mut edited = user.clone();
        match edited.set(&schema, key, &value) {
            Ok(()) => {
                let after = Settings::resolve(&schema, Some(&edited), None);
                assert_eq!(after.get(key), Some(&value), "{key}");
                assert_eq!(after.source(key), Some(Source::User), "{key}");
                for other in schema
                    .keys()
                    .filter(|other| *other != key && *other != "keymap")
                {
                    assert_eq!(after.get(other), alone.get(other), "{other} after {key}");
                }
            }
            Err(WorkspaceError::Unwritable { .. }) => assert!(user.problem().is_some()),
            Err(error @ WorkspaceError::Edit { .. }) => {
                check_message(&error.to_string());
                assert_eq!(
                    edited.document().map(|document| document.as_str()),
                    user.document().map(|document| document.as_str()),
                    "unchanged"
                );
            }
            Err(other) => panic!("setting {key} gave {other:?}"),
        }
    }

    match trust::parse(input, Path::new("fuzz/trust.toml")) {
        Ok(grants) => {
            assert!(grants.len() <= trust::MAX_GRANTS);
            assert!(
                grants.windows(2).all(|pair| pair[0] < pair[1]),
                "sorted, each once"
            );
            for grant in &grants {
                assert!(grant.is_absolute(), "{grant:?}");
                assert_eq!(
                    grant.components().collect::<PathBuf>(),
                    *grant,
                    "spelled as it joins"
                );
            }
        }
        Err(error) => {
            assert!(matches!(
                error,
                WorkspaceError::TooLarge { .. }
                    | WorkspaceError::Toml { .. }
                    | WorkspaceError::Version { .. }
                    | WorkspaceError::BadTrustFile { .. }
            ));
            check_message(&error.to_string());
        }
    }
}

#[test]
fn the_edge_seeds_sit_exactly_at_each_limit() {
    let seed = |name: &str| {
        let path = format!("{ROOT}/fuzz/corpus/ignore/{name}");
        std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
    };
    let parse =
        |bytes: &[u8], limits: &IgnoreLimits| IgnoreFile::parse(bytes, Path::new("x"), limits);
    let file = seed("edge-file.ignore");
    assert_eq!(file.len() as u64, LIMITS.max_file_bytes);
    assert!(parse(&file, &LIMITS).is_ok());
    let patterns = seed("edge-patterns.ignore");
    assert_eq!(
        parse(&patterns, &LIMITS).expect("at the limit").len(),
        LIMITS.max_patterns
    );
    let long = seed("edge-pattern-bytes.ignore");
    assert!(parse(&long, &LIMITS).is_ok());
    let mut one_less = LIMITS;
    one_less.max_pattern_bytes -= 1;
    assert!(matches!(
        parse(&long, &one_less),
        Err(WorkspaceError::PatternTooLong { .. })
    ));
    one_less = LIMITS;
    one_less.max_patterns -= 1;
    assert!(matches!(
        parse(&patterns, &one_less),
        Err(WorkspaceError::TooManyPatterns { .. })
    ));
    one_less = LIMITS;
    one_less.max_file_bytes -= 1;
    assert!(matches!(
        parse(&file, &one_less),
        Err(WorkspaceError::TooLarge { .. })
    ));
}

#[test]
fn the_wildmatch_port_answers_as_git_did() {
    // A few of `tests/folder.rs`'s cases, which Git 2.43.0 answered.
    for (pattern, text, matched) in [
        ("x**y", "xzy", true),
        ("x**y", "x/y", false),
        ("**/deep", "a/b/deep", true),
        ("mid/**/end", "mid/end", true),
        ("a/**b", "a/x/b", false),
        ("[]]z", "]z", true),
        ("[a-]w", "-w", true),
        ("[[:a]", ":", true),
        ("[[:space:]]s", "\u{b}s", false),
        ("un[closed", "un[closed", false),
        ("bs\\", "bs\\", false),
        ("a/*", "a/b/c", false),
    ] {
        assert_eq!(
            wildmatch(pattern.as_bytes(), text.as_bytes()) == MATCH,
            matched,
            "{pattern:?} {text:?}"
        );
    }
}

/// Reads `input` as a session file and checks what `workspace::session`
/// promises of it.
fn read_session(input: &[u8]) {
    let file = Path::new("session.toml");
    match session::parse(input, file) {
        Ok(parsed) => {
            check_session(&parsed);
            let text = parsed.to_text().expect("a session read writes back");
            let again = session::parse(text.as_bytes(), file).expect("its text reads");
            assert_eq!(again, parsed, "the text reads as the same session");
            assert_eq!(
                again.to_text().expect("writes"),
                text,
                "and writes the same"
            );
        }
        Err(error) => {
            assert!(
                matches!(
                    error,
                    WorkspaceError::TooLarge { .. }
                        | WorkspaceError::Toml { .. }
                        | WorkspaceError::Version { .. }
                        | WorkspaceError::BadSessionFile { .. }
                ),
                "an undocumented error: {error:?}"
            );
            check_message(&error.to_string());
        }
    }
}

/// Checks that a session that parsed keeps every limit.
fn check_session(parsed: &Session) {
    assert!(parsed.folder.is_absolute());
    let groups = parsed.groups();
    assert!(!groups.is_empty() && groups.len() <= MAX_GROUPS);
    assert!(parsed.active_group < groups.len());
    let editors: usize = groups.iter().map(|group| group.editors.len()).sum();
    assert!(editors <= MAX_EDITORS);
    for group in &groups {
        check_group(group);
    }
    check_depth(&parsed.layout, 0);
}

fn check_group(group: &Group) {
    assert!(group.active < group.editors.len().max(1));
    for editor in &group.editors {
        assert!(editor.path.is_absolute());
        let cursors = editor.selection.cursors().len();
        assert!((1..=MAX_CURSORS).contains(&cursors));
        let scroll = editor.scroll;
        for units in [scroll.y, scroll.x] {
            assert!(units.is_finite() && units >= 0.0, "{units}");
        }
    }
}

fn check_depth(layout: &Layout, depth: usize) {
    if let Layout::Split(split) = layout {
        assert!(depth < MAX_DEPTH, "splits nest past the limit");
        assert!((2..=MAX_PANES).contains(&split.panes.len()));
        for pane in &split.panes {
            assert!(pane.weight.is_finite() && pane.weight > 0.0);
            check_depth(&pane.layout, depth + 1);
        }
    }
}

#[test]
fn the_session_edge_seeds_sit_exactly_at_each_limit() {
    let seed = |name: &str| {
        let path = format!("{ROOT}/fuzz/corpus/session/{name}");
        std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
    };
    let parse = |bytes: &[u8]| session::parse(bytes, Path::new("x"));
    let depth = |layout: &Layout| {
        let mut deepest = 0;
        let mut stack = vec![(layout, 0)];
        while let Some((layout, at)) = stack.pop() {
            if let Layout::Split(split) = layout {
                deepest = deepest.max(at + 1);
                stack.extend(split.panes.iter().map(|pane| (&pane.layout, at + 1)));
            }
        }
        deepest
    };
    let deep = parse(&seed("edge-depth.toml")).expect("at the limit");
    assert_eq!(depth(&deep.layout), MAX_DEPTH);
    let panes = parse(&seed("edge-panes.toml")).expect("at the limit");
    let Layout::Split(split) = &panes.layout else {
        panic!("a split");
    };
    assert_eq!(split.panes.len(), MAX_PANES);
    let groups = parse(&seed("edge-groups.toml")).expect("at the limit");
    assert_eq!(groups.groups().len(), MAX_GROUPS);
    let cursors = parse(&seed("edge-cursors.toml")).expect("at the limit");
    let selection = &cursors.groups()[0].editors[0].selection;
    assert_eq!(selection.cursors().len(), MAX_CURSORS);

    // One step past each is refused: by the session's check, or for an
    // array past 1,024 entries by `codec`'s limit first.
    let past = |name: &str, from: &str, to: &str| {
        let text = String::from_utf8(seed(name)).expect("UTF-8");
        assert!(text.contains(from), "{name}");
        let changed = text.replacen(from, to, 1);
        assert!(
            matches!(
                parse(changed.as_bytes()),
                Err(WorkspaceError::BadSessionFile { .. } | WorkspaceError::Toml { .. })
            ),
            "{name}"
        );
    };
    // One more split around the innermost group: both of its ends change.
    let deeper = String::from_utf8(seed("edge-depth.toml"))
        .expect("UTF-8")
        .replacen(
            "{ active = 0, editors = [{",
            "{ split = \"row\", panes = [{ weight = 1.0, layout = { active = 0, editors = [] } \
             }, { weight = 1.0, layout = { active = 0, editors = [{",
            1,
        )
        .replacen("x = 0.0 } }] }", "x = 0.0 } }] } }] }", 1);
    assert!(
        matches!(
            parse(deeper.as_bytes()),
            Err(WorkspaceError::BadSessionFile { .. })
        ),
        "one more split"
    );
    past(
        "edge-panes.toml",
        "panes = [",
        "panes = [{ weight = 1.0, layout = { active = 0, editors = [] } }, ",
    );
    past(
        "edge-groups.toml",
        "panes = [{ weight = 1.0, layout = { active = 0, editors = [] } }, ",
        "panes = [{ weight = 1.0, layout = { active = 0, editors = [] } }, \
         { weight = 1.0, layout = { active = 0, editors = [] } }, ",
    );
    past("edge-cursors.toml", "cursors = [", "cursors = [[0, 0], ");
}
