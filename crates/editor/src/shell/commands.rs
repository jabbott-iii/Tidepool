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

//! The shell's views, its bottom panel's tabs, its commands, and the
//! matcher that ranks the commands for the palette (M3.4, ADR-0060).
//!
//! New in Tidepool.
//!
//! # How the palette ranks commands
//!
//! [`rank`] keeps the commands whose title holds every character of the
//! query, in order, compared without case: a *subsequence* match, so
//! `tsp` finds "View: Toggle Side Panel". Of all the ways a title can
//! match, it takes the one that scores highest:
//!
//! - each character matched scores [`MATCH`];
//! - one at the start of a word (the title's first character, one after a
//!   character that is not a letter or digit, or a capital after a small
//!   letter) scores [`WORD_START`] more;
//! - one right after the last one matched scores [`RUN`] more.
//!
//! The commands are then listed best first, and commands that score the
//! same keep [`Command::ALL`]'s order. An empty query keeps every command,
//! in that order. The characters matched are given as byte ranges of the
//! title, which the palette draws bold (ADR-0052).

use std::ops::Range;

use tidepool_ui::draw::IconId;

/// What one character matched scores.
pub const MATCH: u32 = 1;

/// What a character matched at the start of a word scores more.
pub const WORD_START: u32 = 8;

/// What a character matched right after the last one scores more.
pub const RUN: u32 = 4;

/// The longest query [`rank`] matches, in characters. No title is longer,
/// so a longer query matches nothing, and is not compared at all.
pub const MAX_QUERY_CHARS: usize = 64;

/// A view of the side panel, which the activity bar switches between.
///
/// Each is empty until the task that fills it, and says in a line what it
/// will show.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum View {
    /// The workspace's files and the open editors (M4.3).
    Explorer,
    /// Project search (M4.4).
    Search,
    /// Git's changes (M8.2).
    SourceControl,
    /// The managed tools (M5.6).
    Toolchains,
    /// Android and iOS devices (M10.6).
    Devices,
    /// Language agents and chat (M9.5).
    Agents,
}

impl View {
    /// Every view, in the activity bar's order.
    pub const ALL: [View; 6] = [
        View::Explorer,
        View::Search,
        View::SourceControl,
        View::Toolchains,
        View::Devices,
        View::Agents,
    ];

    /// Its name, as a tooltip and the accessibility tree say it.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Explorer => "Explorer",
            Self::Search => "Search",
            Self::SourceControl => "Source Control",
            Self::Toolchains => "Toolchains",
            Self::Devices => "Devices",
            Self::Agents => "Agents",
        }
    }

    /// Its title over the side panel: chrome, so uppercase.
    pub const fn title(self) -> &'static str {
        match self {
            Self::Explorer => "EXPLORER",
            Self::Search => "SEARCH",
            Self::SourceControl => "SOURCE CONTROL",
            Self::Toolchains => "TOOLCHAINS",
            Self::Devices => "DEVICES",
            Self::Agents => "AGENTS",
        }
    }

    /// What the side panel says while the view is empty.
    pub const fn empty_text(self) -> &'static str {
        match self {
            Self::Explorer => "No folder is open.",
            Self::Search => "Open a folder to search it.",
            Self::SourceControl => "No repository is open.",
            Self::Toolchains => "No tools have been looked for yet.",
            Self::Devices => "No devices are connected.",
            Self::Agents => "No agents are set up.",
        }
    }

    /// The id of its icon in the activity bar: 1 to 6, in [`View::ALL`]'s
    /// order. The frontend registers what each draws (ADR-0060).
    pub const fn icon(self) -> IconId {
        IconId(match self {
            Self::Explorer => 1,
            Self::Search => 2,
            Self::SourceControl => 3,
            Self::Toolchains => 4,
            Self::Devices => 5,
            Self::Agents => 6,
        })
    }

    /// Its place in [`View::ALL`].
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// A tab of the bottom panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PanelTab {
    /// Diagnostics and cargo's problems (M6.4).
    Problems,
    /// The integrated terminal (M5.4).
    Terminal,
    /// The log's lines and the tools' output.
    Output,
    /// A device's log (M10.6).
    Devices,
}

impl PanelTab {
    /// Every tab, in the strip's order.
    pub const ALL: [PanelTab; 4] = [
        PanelTab::Problems,
        PanelTab::Terminal,
        PanelTab::Output,
        PanelTab::Devices,
    ];

    /// Its label: chrome, so uppercase.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Problems => "PROBLEMS",
            Self::Terminal => "TERMINAL",
            Self::Output => "OUTPUT",
            Self::Devices => "DEVICES",
        }
    }

    /// What the panel says while the tab is empty.
    pub const fn empty_text(self) -> &'static str {
        match self {
            Self::Problems => "No problems have been found.",
            Self::Terminal => "No terminal is open.",
            Self::Output => "Nothing has been written yet.",
            Self::Devices => "No device is selected.",
        }
    }

    /// Its place in [`PanelTab::ALL`].
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// Something the shell can do, from the palette or a shortcut.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    /// Shows a view in the side panel, showing the panel if it is hidden.
    Show(View),
    /// Selects a tab of the bottom panel, showing the panel if it is
    /// hidden.
    ShowTab(PanelTab),
    /// Hides the side panel, or shows it (Ctrl+B).
    ToggleSidePanel,
    /// Hides the bottom panel, or shows it (Ctrl+J).
    ToggleBottomPanel,
    /// Closes the window, which ends Tidepool.
    CloseWindow,
}

impl Command {
    /// Every command, in the order the palette lists them for an empty
    /// query, and in which equal scores stay.
    pub const ALL: [Command; 13] = [
        Command::Show(View::Explorer),
        Command::Show(View::Search),
        Command::Show(View::SourceControl),
        Command::Show(View::Toolchains),
        Command::Show(View::Devices),
        Command::Show(View::Agents),
        Command::ShowTab(PanelTab::Problems),
        Command::ShowTab(PanelTab::Terminal),
        Command::ShowTab(PanelTab::Output),
        Command::ShowTab(PanelTab::Devices),
        Command::ToggleSidePanel,
        Command::ToggleBottomPanel,
        Command::CloseWindow,
    ];

    /// Its title in the palette, as written: a category, then what it does.
    pub const fn title(self) -> &'static str {
        match self {
            Self::Show(View::Explorer) => "View: Explorer",
            Self::Show(View::Search) => "View: Search",
            Self::Show(View::SourceControl) => "View: Source Control",
            Self::Show(View::Toolchains) => "View: Toolchains",
            Self::Show(View::Devices) => "View: Devices",
            Self::Show(View::Agents) => "View: Agents",
            Self::ShowTab(PanelTab::Problems) => "Panel: Problems",
            Self::ShowTab(PanelTab::Terminal) => "Panel: Terminal",
            Self::ShowTab(PanelTab::Output) => "Panel: Output",
            Self::ShowTab(PanelTab::Devices) => "Panel: Devices",
            Self::ToggleSidePanel => "View: Toggle Side Panel",
            Self::ToggleBottomPanel => "View: Toggle Bottom Panel",
            Self::CloseWindow => "Window: Close",
        }
    }
}

/// A command the palette lists for a query, with what it matched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ranked {
    /// The command.
    pub command: Command,
    /// What its match scored: 0 for an empty query.
    pub score: u32,
    /// The byte ranges of its title the query matched, in order, each
    /// run of characters matched one after another as one range.
    pub matched: Vec<Range<usize>>,
}

/// The commands whose titles match `query`, best first, as the [module
/// documentation](self#how-the-palette-ranks-commands) says.
///
/// ```
/// use tidepool_editor::shell::commands::{Command, rank};
///
/// let ranked = rank("tsp");
/// assert_eq!(ranked[0].command, Command::ToggleSidePanel);
/// // "View: Toggle Side Panel": the T, the S and the P.
/// assert_eq!(ranked[0].matched, [6..7, 13..14, 18..19]);
/// assert_eq!(rank("").len(), Command::ALL.len());
/// assert!(rank("zzz").is_empty());
/// ```
pub fn rank(query: &str) -> Vec<Ranked> {
    let query: Vec<char> = query.chars().collect();
    let mut ranked: Vec<Ranked> = Command::ALL
        .iter()
        .filter_map(|&command| {
            let (score, matched) = best_match(&query, command.title())?;
            Some(Ranked {
                command,
                score,
                matched,
            })
        })
        .collect();
    // A stable sort: equal scores keep `Command::ALL`'s order.
    ranked.sort_by_key(|ranked| std::cmp::Reverse(ranked.score));
    ranked
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

    fn commands(query: &str) -> Vec<Command> {
        rank(query).into_iter().map(|r| r.command).collect()
    }

    #[test]
    fn an_empty_query_lists_every_command_in_order_with_nothing_matched() {
        let ranked = rank("");
        assert_eq!(
            ranked.iter().map(|r| r.command).collect::<Vec<_>>(),
            Command::ALL
        );
        assert!(ranked.iter().all(|r| r.score == 0 && r.matched.is_empty()));
    }

    #[test]
    fn every_character_must_appear_in_order_without_case() {
        assert_eq!(commands("TOGGLE"), commands("toggle"));
        assert_eq!(
            commands("toggle"),
            [Command::ToggleSidePanel, Command::ToggleBottomPanel]
        );
        assert!(
            !commands("pst").contains(&Command::ToggleSidePanel),
            "the order of the characters counts"
        );
        assert!(commands("xyz").is_empty());
        assert!(
            commands("View: Explorer!").is_empty(),
            "a character no title has"
        );
    }

    #[test]
    fn initials_find_the_command_they_stand_for() {
        assert_eq!(commands("tsp")[0], Command::ToggleSidePanel);
        assert_eq!(commands("tbp")[0], Command::ToggleBottomPanel);
        assert_eq!(commands("vsc")[0], Command::Show(View::SourceControl));
        assert_eq!(commands("wc")[0], Command::CloseWindow);
    }

    #[test]
    fn word_starts_and_runs_beat_scattered_letters() {
        // "term" runs at the start of "Terminal", and is scattered in no
        // other title, so the terminal comes first.
        assert_eq!(commands("term")[0], Command::ShowTab(PanelTab::Terminal));
        // "dev" starts a word in both "View: Devices" and "Panel: Devices":
        // equal scores keep the list's order, the view first.
        assert_eq!(
            commands("dev")[..2],
            [
                Command::Show(View::Devices),
                Command::ShowTab(PanelTab::Devices)
            ]
        );
        let explorer = rank("exp");
        assert_eq!(explorer[0].command, Command::Show(View::Explorer));
        assert_eq!(explorer[0].score, 3 * MATCH + WORD_START + 2 * RUN);
    }

    #[test]
    fn the_best_alignment_is_taken_not_the_first() {
        // Taking the first match of each character would give "con" the
        // "c" of "Source", then "on" of "Control": one run, no word start.
        // The best match is "Con" itself.
        let control = rank("con");
        assert_eq!(control[0].command, Command::Show(View::SourceControl));
        assert_eq!(control[0].matched, vec![13..16], "View: Source Control");
        assert_eq!(control[0].score, 3 * MATCH + WORD_START + 2 * RUN);
    }

    #[test]
    fn matched_ranges_are_byte_ranges_of_the_title_merged_into_runs() {
        for query in ["tsp", "view", "panel: ou", "e", "close"] {
            for ranked in rank(query) {
                let title = ranked.command.title();
                let mut last_end = 0;
                let mut chars = 0;
                for range in &ranked.matched {
                    assert!(
                        range.start >= last_end && range.start < range.end,
                        "{query}: {range:?}"
                    );
                    assert!(
                        title.is_char_boundary(range.start) && title.is_char_boundary(range.end)
                    );
                    if last_end != 0 {
                        assert!(range.start > last_end, "{query}: runs are merged");
                    }
                    chars += title[range.clone()].chars().count();
                    last_end = range.end;
                }
                assert_eq!(chars, query.chars().count(), "{query} in {title}");
            }
        }
    }

    #[test]
    fn a_query_longer_than_any_title_matches_nothing() {
        let long = "v".repeat(MAX_QUERY_CHARS + 1);
        assert!(rank(&long).is_empty());
        assert!(rank(&"View: Toggle Bottom Panel!".repeat(3)).is_empty());
    }

    #[test]
    fn non_ascii_queries_never_panic_and_match_only_what_they_hold() {
        assert!(rank("é").is_empty());
        assert!(rank("\u{0}").is_empty());
        // The Kelvin sign's lowercase is a k; no title has one.
        assert!(rank("\u{212a}").is_empty());
        // A capital I with a dot folds to an i, as its lowercase starts.
        assert_eq!(commands("\u{130}"), commands("i"));
    }

    #[test]
    fn views_and_tabs_are_in_order_with_their_icons() {
        for (i, view) in View::ALL.into_iter().enumerate() {
            assert_eq!(view.index(), i);
            assert_eq!(usize::from(view.icon().0), i + 1);
            assert_eq!(view.title(), view.name().to_uppercase());
        }
        for (i, tab) in PanelTab::ALL.into_iter().enumerate() {
            assert_eq!(tab.index(), i);
            assert_eq!(tab.label(), tab.label().to_uppercase());
        }
        let titles: std::collections::HashSet<_> = Command::ALL.iter().map(|c| c.title()).collect();
        assert_eq!(
            titles.len(),
            Command::ALL.len(),
            "no two commands share a title"
        );
        assert!(
            Command::ALL
                .iter()
                .all(|c| c.title().chars().count() <= MAX_QUERY_CHARS)
        );
    }
}
