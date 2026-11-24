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

//! The shell's views, its bottom panel's tabs, and its commands, with
//! their ids, which the shell registers in a [`Registry`] (M3.4,
//! ADR-0060; M4.6, ADR-0064).
//!
//! New in Tidepool.
//!
//! # How the palette ranks commands
//!
//! The palette ranks the registry's titles with the subsequence matcher of
//! [`Registry::rank`], whose documentation (in [`command`](crate::command))
//! gives its scores; [`rank`] ranks the shell's own commands the same way.
//! Commands that score the same keep [`Command::ALL`]'s order, which is
//! the order the shell registers them in.

use std::ops::Range;

use tidepool_ui::draw::IconId;

use crate::command::Registry;
use crate::edit;

pub use crate::command::{MATCH, MAX_QUERY_CHARS, RUN, WORD_START};

/// A view of the side panel, which the activity bar switches between.
///
/// Each is empty until the task that fills it, and says in a line what it
/// will show.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum View {
    /// The workspace's files and the open editors (M4.13).
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
    /// Opens the command palette (Ctrl+Shift+P or F1), unless it is open.
    OpenPalette,
}

impl Command {
    /// Every command, in the order the shell registers them, so the order
    /// the palette lists them in for an empty query, and in which equal
    /// scores stay.
    pub const ALL: [Command; 14] = [
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
        Command::OpenPalette,
    ];

    /// Its id, which keymaps bind keys to (ADR-0064).
    pub const fn id(self) -> &'static str {
        match self {
            Self::Show(View::Explorer) => "view.showExplorer",
            Self::Show(View::Search) => "view.showSearch",
            Self::Show(View::SourceControl) => "view.showSourceControl",
            Self::Show(View::Toolchains) => "view.showToolchains",
            Self::Show(View::Devices) => "view.showDevices",
            Self::Show(View::Agents) => "view.showAgents",
            Self::ShowTab(PanelTab::Problems) => "panel.showProblems",
            Self::ShowTab(PanelTab::Terminal) => "panel.showTerminal",
            Self::ShowTab(PanelTab::Output) => "panel.showOutput",
            Self::ShowTab(PanelTab::Devices) => "panel.showDevices",
            Self::ToggleSidePanel => "view.toggleSidePanel",
            Self::ToggleBottomPanel => "view.toggleBottomPanel",
            Self::CloseWindow => "window.close",
            Self::OpenPalette => "palette.open",
        }
    }

    /// The command whose id is `id`, if it is one of the shell's.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|command| command.id() == id)
    }

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
            Self::OpenPalette => "Command Palette: Show",
        }
    }
}

/// A registry of the shell's commands, each with its id and title, in
/// [`Command::ALL`]'s order, then the editing commands, in
/// [`edit::Command::ALL`]'s (M4.2, ADR-0065): the registry the default
/// keymap and the user's bindings are read against. The palette lists the
/// commands of it that the shell runs.
///
/// ```
/// use tidepool_editor::edit;
/// use tidepool_editor::shell::commands::{Command, registry};
///
/// let registry = registry();
/// assert_eq!(registry.len(), Command::ALL.len() + edit::Command::ALL.len());
/// assert_eq!(
///     registry.get("palette.open").map(|entry| entry.title()),
///     Some(Command::OpenPalette.title())
/// );
/// assert_eq!(
///     registry.get("editor.undo").map(|entry| entry.title()),
///     Some("Edit: Undo")
/// );
/// ```
pub fn registry() -> Registry {
    let mut registry = Registry::new();
    for command in Command::ALL {
        registry
            .register(command.id(), command.title())
            .expect("the shell's ids are valid and distinct: its tests check them");
    }
    edit::Command::register(&mut registry).expect(
        "the editing ids are valid, distinct and the shell's are not among them: tests check them",
    );
    registry
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

/// The shell's commands whose titles match `query`, best first, as the
/// shell's palette ranks them ([`Registry::rank`] over [`registry`]).
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
    registry()
        .rank(query)
        .into_iter()
        .filter_map(|ranked| {
            Some(Ranked {
                command: Command::from_id(ranked.entry.id())?,
                score: ranked.score,
                matched: ranked.matched,
            })
        })
        .collect()
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

    #[test]
    fn the_commands_have_the_ids_of_adr_0064_and_read_back_from_them() {
        let ids: Vec<&str> = Command::ALL.iter().map(|c| c.id()).collect();
        assert_eq!(
            ids,
            [
                "view.showExplorer",
                "view.showSearch",
                "view.showSourceControl",
                "view.showToolchains",
                "view.showDevices",
                "view.showAgents",
                "panel.showProblems",
                "panel.showTerminal",
                "panel.showOutput",
                "panel.showDevices",
                "view.toggleSidePanel",
                "view.toggleBottomPanel",
                "window.close",
                "palette.open",
            ]
        );
        for command in Command::ALL {
            assert!(crate::command::is_valid_id(command.id()), "{command:?}");
            assert_eq!(Command::from_id(command.id()), Some(command));
        }
        assert_eq!(Command::from_id("palette.close"), None);
        assert_eq!(Command::from_id("View.showExplorer"), None);
    }

    #[test]
    fn the_registry_holds_every_command_in_order_with_its_title() {
        let registry = registry();
        assert_eq!(registry.len(), 14 + 40, "the shell's, then the editor's");
        let (shell, editing) = registry.entries().split_at(Command::ALL.len());
        for (entry, command) in shell.iter().zip(Command::ALL) {
            assert_eq!((entry.id(), entry.title()), (command.id(), command.title()));
        }
        for (entry, command) in editing.iter().zip(edit::Command::ALL) {
            assert_eq!((entry.id(), entry.title()), (command.id(), command.title()));
        }
    }

    #[test]
    fn opening_the_palette_has_its_own_entry_that_tog_does_not_match() {
        // The palette golden scene types "tog": the palette's own entry
        // must not join its rows.
        assert_eq!(
            commands("tog"),
            [Command::ToggleSidePanel, Command::ToggleBottomPanel]
        );
        assert_eq!(commands("palette")[0], Command::OpenPalette);
        assert_eq!(commands("cps")[0], Command::OpenPalette);
    }
}
