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

//! Named commands: a [`Registry`] of every action's id and palette title
//! (M4.6, ADR-0064). Specification: `docs/subsystems/editor.md` →
//! "Commands and keymaps".
//!
//! New in Tidepool. The matcher that ranks titles for the palette came here
//! from `shell::commands`, where M3.4 built it (ADR-0060), unchanged.
//!
//! # Ids
//!
//! Every action Tidepool can take has an id, such as `view.toggleSidePanel`
//! or, from later tasks, `editor.action.copy`: one or more segments joined
//! by `.`, each of ASCII letters and digits and starting with a lowercase
//! letter, at most [`MAX_ID_BYTES`] bytes in all ([`is_valid_id`]). A
//! keymap binds keys to ids, and the palette lists titles, so neither needs
//! to know the code that runs a command.
//!
//! # Why the registry holds no handlers
//!
//! The registry holds each command's id and title only. Its owner runs a
//! command by its id: the shell runs its own, and the editor view and,
//! from M15.6, plugins will run theirs. A table of handlers would have to
//! borrow all of them at once, and a plugin's commands come and go with
//! the plugin.
//!
//! The ids whose first segment is `mode` are the keymap engine's own
//! (`mode.leave` and `mode.enter.<mode>`, [`keymap`](crate::keymap)), so
//! the registry refuses them as taken.
//!
//! # How the palette ranks commands
//!
//! [`Registry::rank`] keeps the commands whose title holds every character
//! of the query, in order, compared without case: a *subsequence* match,
//! so `tsp` finds "View: Toggle Side Panel". Of all the ways a title can
//! match, it takes the one that scores highest:
//!
//! - each character matched scores [`MATCH`];
//! - one at the start of a word (the title's first character, one after a
//!   character that is not a letter or digit, or a capital after a small
//!   letter) scores [`WORD_START`] more;
//! - one right after the last one matched scores [`RUN`] more.
//!
//! The commands are then listed best first, and commands that score the
//! same keep the order they were registered in. An empty query keeps every
//! command, in that order. The characters matched are given as byte ranges
//! of the title, which the palette draws bold (ADR-0052).
//!
//! ```
//! use tidepool_editor::command::Registry;
//!
//! let mut registry = Registry::new();
//! registry.register("view.toggleSidePanel", "View: Toggle Side Panel")?;
//! registry.register("window.close", "Window: Close")?;
//! assert!(registry.register("window.close", "Window: Close Again").is_err());
//! assert!(registry.register("Window.close", "Window: Close").is_err());
//!
//! let ranked = registry.rank("tsp");
//! assert_eq!(ranked.len(), 1);
//! assert_eq!(ranked[0].entry.id(), "view.toggleSidePanel");
//! // "View: Toggle Side Panel": the T, the S and the P.
//! assert_eq!(ranked[0].matched, [6..7, 13..14, 18..19]);
//! # Ok::<(), tidepool_editor::EditorError>(())
//! ```

use std::collections::HashMap;
use std::ops::Range;

use crate::error::EditorError;

/// The longest command id, in bytes.
pub const MAX_ID_BYTES: usize = 128;

/// What one character matched scores.
pub const MATCH: u32 = 1;

/// What a character matched at the start of a word scores more.
pub const WORD_START: u32 = 8;

/// What a character matched right after the last one scores more.
pub const RUN: u32 = 4;

/// The longest query [`Registry::rank`] matches, in characters. A longer
/// query matches nothing, and is not compared at all, so ranking costs at
/// most this many steps for each character of a title. The shell's titles
/// are all shorter.
pub const MAX_QUERY_CHARS: usize = 64;

/// Whether `id` is a command id: one or more segments joined by `.`, each
/// of ASCII letters and digits and starting with a lowercase letter, at
/// most [`MAX_ID_BYTES`] bytes in all.
///
/// ```
/// use tidepool_editor::command::is_valid_id;
///
/// assert!(is_valid_id("editor.action.copy"));
/// assert!(is_valid_id("workspace.openFile"));
/// assert!(is_valid_id("f1"));
/// assert!(!is_valid_id("Editor.copy"), "a segment starts lowercase");
/// assert!(!is_valid_id("editor..copy"), "no empty segment");
/// assert!(!is_valid_id("editor.copy-all"), "letters and digits only");
/// assert!(!is_valid_id(""));
/// ```
pub fn is_valid_id(id: &str) -> bool {
    id.len() <= MAX_ID_BYTES && id.split('.').all(is_segment)
}

/// Whether `segment` is one segment of an id, or a mode's name: ASCII
/// letters and digits, starting with a lowercase letter.
pub(crate) fn is_segment(segment: &str) -> bool {
    let mut bytes = segment.bytes();
    bytes.next().is_some_and(|first| first.is_ascii_lowercase())
        && bytes.all(|byte| byte.is_ascii_alphanumeric())
}

/// Whether `id` is one of the keymap engine's: its first segment is
/// `mode`.
pub(crate) fn is_engine_id(id: &str) -> bool {
    id == "mode" || id.starts_with("mode.")
}

/// A registered command: its id and its title in the palette.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    id: String,
    title: String,
}

impl Entry {
    /// Its id ([module documentation](self#ids)).
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Its title in the palette, as written: a category, then what it
    /// does, such as "View: Toggle Side Panel".
    pub fn title(&self) -> &str {
        &self.title
    }
}

/// Every command Tidepool knows, by id, in the order they were registered
/// (the [module documentation](self)).
#[derive(Clone, Debug, Default)]
pub struct Registry {
    entries: Vec<Entry>,
    by_id: HashMap<String, usize>,
}

impl Registry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the command `id`, titled `title` in the palette, after those
    /// registered before it.
    ///
    /// # Errors
    ///
    /// [`EditorError::BadCommandId`] if `id` is not an id
    /// ([`is_valid_id`]), and [`EditorError::TakenCommandId`] if the
    /// registry has it already or it is one of the keymap engine's (its
    /// first segment is `mode`). Either way the registry is unchanged.
    pub fn register(&mut self, id: &str, title: &str) -> Result<(), EditorError> {
        if !is_valid_id(id) {
            return Err(EditorError::BadCommandId { id: id.to_string() });
        }
        if is_engine_id(id) || self.by_id.contains_key(id) {
            return Err(EditorError::TakenCommandId { id: id.to_string() });
        }
        self.by_id.insert(id.to_string(), self.entries.len());
        self.entries.push(Entry {
            id: id.to_string(),
            title: title.to_string(),
        });
        Ok(())
    }

    /// The command `id`, if it is registered.
    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.by_id.get(id).and_then(|&at| self.entries.get(at))
    }

    /// Whether the command `id` is registered.
    pub fn contains(&self, id: &str) -> bool {
        self.by_id.contains_key(id)
    }

    /// Every command, in the order they were registered.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// How many commands are registered.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no command is registered.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The commands whose titles match `query`, best first, as the [module
    /// documentation](self#how-the-palette-ranks-commands) says.
    pub fn rank(&self, query: &str) -> Vec<Match<'_>> {
        let query: Vec<char> = query.chars().collect();
        let mut ranked: Vec<Match<'_>> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let (score, matched) = best_match(&query, entry.title())?;
                Some(Match {
                    entry,
                    score,
                    matched,
                })
            })
            .collect();
        // A stable sort: equal scores keep the order of registration.
        ranked.sort_by_key(|ranked| std::cmp::Reverse(ranked.score));
        ranked
    }
}

/// A command [`Registry::rank`] lists for a query, with what it matched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match<'a> {
    /// The command.
    pub entry: &'a Entry,
    /// What its match scored: 0 for an empty query.
    pub score: u32,
    /// The byte ranges of its title the query matched, in order, each
    /// run of characters matched one after another as one range.
    pub matched: Vec<Range<usize>>,
}

/// The best way `query` matches `title` as a subsequence: its score and
/// the byte ranges matched; `None` if it does not match. An empty query
/// matches with a score of 0 and no ranges.
fn best_match(query: &[char], title: &str) -> Option<(u32, Vec<Range<usize>>)> {
    if query.is_empty() {
        return Some((0, Vec::new()));
    }
    if query.len() > MAX_QUERY_CHARS {
        return None;
    }
    let chars: Vec<(usize, char)> = title.char_indices().collect();
    let (m, n) = (query.len(), chars.len());
    if m > n {
        return None;
    }
    // `score[i][j]`: the best score of matching `query[..=i]` with
    // `query[i]` at `chars[j]`; `from[i][j]`, where `query[i - 1]` went.
    let mut score = vec![vec![None::<u32>; n]; m];
    let mut from = vec![vec![0usize; n]; m];
    for (i, &wanted) in query.iter().enumerate() {
        // The best of row `i - 1` at any column before the one before `j`,
        // and where it is: a match there is not a run.
        let mut best_before: Option<(u32, usize)> = None;
        for j in 0..n {
            if i > 0
                && j >= 2
                && let Some(s) = score[i - 1][j - 2]
                && best_before.is_none_or(|(b, _)| s > b)
            {
                best_before = Some((s, j - 2));
            }
            if !same(wanted, chars[j].1) {
                continue;
            }
            let own = MATCH + if word_start(&chars, j) { WORD_START } else { 0 };
            if i == 0 {
                score[0][j] = Some(own);
                continue;
            }
            let run = j
                .checked_sub(1)
                .and_then(|k| score[i - 1][k].map(|s| (s + RUN, k)));
            let candidate = match (run, best_before) {
                (Some(r), Some(b)) => Some(if r.0 >= b.0 { r } else { b }),
                (r, b) => r.or(b),
            };
            if let Some((s, k)) = candidate {
                score[i][j] = Some(s + own);
                from[i][j] = k;
            }
        }
    }
    // The best end; the first of equal ends, so matches lean left.
    let (mut j, best) = score[m - 1]
        .iter()
        .enumerate()
        .filter_map(|(j, s)| s.map(|s| (j, s)))
        .fold(None, |acc: Option<(usize, u32)>, (j, s)| match acc {
            Some((_, b)) if b >= s => acc,
            _ => Some((j, s)),
        })?;
    let mut at = vec![0usize; m];
    for i in (0..m).rev() {
        at[i] = j;
        j = from[i][j];
    }
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for &j in &at {
        let (start, c) = chars[j];
        let end = start + c.len_utf8();
        match ranges.last_mut() {
            Some(last) if last.end == start => last.end = end,
            _ => ranges.push(start..end),
        }
    }
    Some((best, ranges))
}

/// Whether two characters are the same without case: their first
/// lowercase characters are equal.
fn same(a: char, b: char) -> bool {
    a == b || a.to_lowercase().next() == b.to_lowercase().next()
}

/// Whether `chars[j]` starts a word: it is the first, follows a character
/// that is not a letter or digit, or is a capital after a small letter.
fn word_start(chars: &[(usize, char)], j: usize) -> bool {
    let Some(previous) = j.checked_sub(1).map(|k| chars[k].1) else {
        return true;
    };
    let here = chars[j].1;
    !previous.is_alphanumeric() || (previous.is_lowercase() && here.is_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry(commands: &[(&str, &str)]) -> Registry {
        let mut registry = Registry::new();
        for (id, title) in commands {
            registry.register(id, title).expect("valid and new");
        }
        registry
    }

    fn ids<'a>(ranked: &[Match<'a>]) -> Vec<&'a str> {
        ranked.iter().map(|ranked| ranked.entry.id()).collect()
    }

    #[test]
    fn ids_are_dotted_segments_starting_lowercase_within_the_limit() {
        for id in [
            "a",
            "view.showExplorer",
            "editor.action.copy",
            "workspace.openFile",
            "git.stage2",
            "a1.b2.c3",
            "x".repeat(MAX_ID_BYTES).as_str(),
        ] {
            assert!(is_valid_id(id), "{id:?}");
        }
        let long = format!("a.{}", "b".repeat(MAX_ID_BYTES - 2));
        assert_eq!(long.len(), MAX_ID_BYTES);
        assert!(is_valid_id(&long));
        for id in [
            String::new(),
            ".".into(),
            "a.".into(),
            ".a".into(),
            "a..b".into(),
            "A".into(),
            "view.ShowExplorer".into(),
            "1view".into(),
            "view.2".into(),
            "view_show".into(),
            "view-show".into(),
            "view show".into(),
            "vïew".into(),
            "view.show\u{0}".into(),
            "x".repeat(MAX_ID_BYTES + 1),
            format!("{long}a"),
        ] {
            assert!(!is_valid_id(&id), "{id:?}");
        }
    }

    #[test]
    fn registering_keeps_the_order_and_refuses_bad_and_taken_ids() {
        let mut registry = Registry::new();
        assert!(registry.is_empty());
        registry.register("b.first", "B: First").expect("new");
        registry.register("a.second", "A: Second").expect("new");
        assert_eq!(registry.len(), 2);
        assert_eq!(
            registry.entries().iter().map(Entry::id).collect::<Vec<_>>(),
            ["b.first", "a.second"],
            "registration order, not sorted"
        );
        assert_eq!(
            registry.get("a.second").map(Entry::title),
            Some("A: Second")
        );
        assert!(registry.contains("b.first") && !registry.contains("b.First"));
        assert_eq!(registry.get("c.third"), None);

        let taken = registry.register("b.first", "B: Again");
        assert!(
            matches!(&taken, Err(EditorError::TakenCommandId { id }) if id == "b.first"),
            "{taken:?}"
        );
        let bad = registry.register("B.first", "B: First");
        assert!(
            matches!(&bad, Err(EditorError::BadCommandId { id }) if id == "B.first"),
            "{bad:?}"
        );
        for engine in ["mode", "mode.leave", "mode.enter.normal", "mode.other"] {
            let taken = registry.register(engine, "Mode");
            assert!(
                matches!(taken, Err(EditorError::TakenCommandId { .. })),
                "{engine}: {taken:?}"
            );
        }
        assert!(registry.register("modes.leave", "Not the engine's").is_ok());
        assert_eq!(registry.len(), 3, "a refused id changes nothing");
        assert_eq!(registry.get("b.first").map(Entry::title), Some("B: First"));
    }

    #[test]
    fn errors_name_the_id_escaped_and_cut() {
        let mut registry = Registry::new();
        let error = registry
            .register("a\u{1b}[31m\u{202e}", "Bad")
            .expect_err("bad");
        let message = error.to_string();
        assert!(message.contains("\\u{1b}[31m\\u{202e}"), "{message}");
        assert!(!message.contains('\u{1b}') && !message.contains('\u{202e}'));
        let error = registry
            .register(&"x".repeat(100_000), "Long")
            .expect_err("too long");
        assert!(error.to_string().len() < 1_000, "cut: {error}");
    }

    #[test]
    fn an_empty_query_lists_every_command_in_order_with_nothing_matched() {
        let registry = registry(&[("b.b", "Bravo"), ("a.a", "Alpha"), ("c.c", "Charlie")]);
        let ranked = registry.rank("");
        assert_eq!(ids(&ranked), ["b.b", "a.a", "c.c"]);
        assert!(ranked.iter().all(|r| r.score == 0 && r.matched.is_empty()));
        assert!(Registry::new().rank("").is_empty());
    }

    #[test]
    fn equal_scores_keep_the_order_of_registration() {
        let registry = registry(&[
            ("panel.devices", "Panel: Devices"),
            ("view.devices", "View: Devices"),
        ]);
        let ranked = registry.rank("dev");
        assert_eq!(ranked[0].score, ranked[1].score);
        assert_eq!(ids(&ranked), ["panel.devices", "view.devices"]);
    }

    #[test]
    fn the_best_alignment_is_taken_and_scored() {
        let registry = registry(&[
            ("view.showSourceControl", "View: Source Control"),
            ("view.showExplorer", "View: Explorer"),
        ]);
        let control = registry.rank("con");
        assert_eq!(control[0].entry.id(), "view.showSourceControl");
        assert_eq!(control[0].matched, vec![13..16], "View: Source Control");
        assert_eq!(control[0].score, 3 * MATCH + WORD_START + 2 * RUN);
        assert!(registry.rank("xyz").is_empty());
        assert!(registry.rank(&"v".repeat(MAX_QUERY_CHARS + 1)).is_empty());
    }

    #[test]
    fn a_title_longer_than_the_longest_query_still_matches_a_short_one() {
        let long = format!("Long: {}", "word ".repeat(40));
        let registry = registry(&[("long.title", long.as_str())]);
        let ranked = registry.rank("lw");
        assert_eq!(ids(&ranked), ["long.title"]);
        assert_eq!(ranked[0].matched, [0..1, 6..7]);
    }
}
