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

//! The shell: the window's frame around the editor, in jabbott.dev's look
//! (M3.4, ADR-0060). Specification: `docs/subsystems/editor.md` → "Shell
//! layout".
//!
//! New in Tidepool.
//!
//! [`Shell::new`] builds it in a `ui` [`Tree`], from the top of the window
//! down:
//!
//! ```text
//! ┌─ ▣ TIDEPOOL  NO FOLDER ─────────────────────────── title strip ─┐
//! │ ▣ │ EXPLORER       ║                                            │
//! │ ⌕ │ No folder is   ║           editor area (the backdrop)       │
//! │ ⑂ │ open.          ║                                            │
//! │ ⚙ │                ╠════════════════════════════════════════════╡
//! │ ⬡ │  side panel    ║ PROBLEMS TERMINAL OUTPUT DEVICES           │
//! │ ✦ │                ║ No problems have been found.  bottom panel │
//! ├───┴────────────────╨────────────────────────────────────────────┤
//! │ ● rust-analyzer  ● git                  Ctrl+Shift+P: commands  │
//! └──────────────────────────────────────────────────── status bar ─┘
//! ```
//!
//! - the **title strip**, [`TITLE_HEIGHT`] high, with the brand and the
//!   workspace's line ([`parts::TitleStrip`]);
//! - the **activity bar**, [`ACTIVITY_WIDTH`] wide, a button for each
//!   [`View`], each with a tooltip naming it ([`parts::ActivityButton`]);
//! - the **side panel**, the active view's title and what it shows, beside
//!   a splitter: it starts at [`SIDE_RATIO`] of the space beside the bar,
//!   and is at least [`SIDE_MIN_WIDTH`] wide, leaving the rest at least
//!   [`MAIN_MIN_WIDTH`];
//! - the **editor area**, where the backdrop shows while no editor is
//!   open, with a welcome line;
//! - the **bottom panel**, under a splitter, a tab strip of [`PanelTab`]s
//!   over what the selected one shows; the editor area starts at
//!   [`EDITOR_RATIO`] of the height, and each keeps at least
//!   [`EDITOR_MIN_HEIGHT`] and [`BOTTOM_MIN_HEIGHT`];
//! - the **status bar**, [`STATUS_HEIGHT`] high, on `bar`, with the
//!   status of rust-analyzer and git (off until M5 and M6 find them) and
//!   the palette's shortcut;
//! - and the **command palette**, a modal overlay over it all while open.
//!
//! Every view and tab is empty until the task that fills it, and says so in
//! a line. The side and bottom panels can be hidden, as [`Panels`] says at
//! the start and the commands toggle them: a hidden panel's split becomes a
//! plain row or column, so the rest fills the space.
//!
//! # Events, commands and timers
//!
//! [`Shell::handle`] gives each input event to the tree first
//! ([`Tree::handle`]), then acts on what the widgets emitted: an activity
//! button shows its view, or hides the side panel if its view is shown
//! already; the tab strip selects a tab; the palette ranks the commands of
//! the shell's [`Registry`] as its query changes ([`Registry::rank`]) and
//! runs the one activated.
//!
//! A key that no widget handled and that no modal blocked (ADR-0055) goes
//! to the shell's keymap [`Engine`], which may resolve it to a command
//! (M4.6, ADR-0064). The shell runs the commands that are its own
//! ([`Command::from_id`]) on a key going down, never on an auto-repeat.
//! The default keymap ([`Keymap::bundled`]) binds four keys: Ctrl+Shift+P
//! or F1 opens the palette, Ctrl+B toggles the side panel and Ctrl+J the
//! bottom panel. Its owner may give the shell another keymap, such as the
//! default with the user's layer over it ([`Shell::set_keymap`]).
//!
//! A tooltip shows [`TOOLTIP_DELAY`] after the pointer comes to rest on its
//! node, and at once for keyboard focus: [`Shell::update`], called after
//! each event and each frame, follows [`Tree::tooltip_target`] and fires
//! the timer, whose deadline [`Shell::next_deadline`] gives the main loop.
//! `ui` has no clock (ADR-0052), so the shell is told the time.

pub mod commands;
pub mod parts;

use std::time::{Duration, Instant};

use tidepool_core::geom::Size;
use tidepool_core::input::InputEvent;
use tidepool_ui::a11y::{Role, Semantics};
use tidepool_ui::input::{Action, ActionKind};
use tidepool_ui::layout::{Align, Axis, Insets, Length, Style};
use tidepool_ui::theme::Token;
use tidepool_ui::widgets::{
    self, CARD_TITLE, Control, Kicker, PATH, Palette, ROW_TEXT, Row, SectionTitle, Splitter,
    Status, StatusDot, Tab, TabKind, TabStrip, Tooltip, TooltipCause, TooltipTarget,
};
use tidepool_ui::{Node, NodeId, Tree, UiError};

use crate::command::Registry;
use crate::keymap::{Engine, Keymap, Resolution};

pub use commands::{Command, PanelTab, View};

use parts::{ActivityButton, Edges, Label, Surface, TitleStrip, set_label};

/// The title strip's height.
pub const TITLE_HEIGHT: f32 = 40.0;

/// The activity bar's width, and each of its buttons' sides.
pub const ACTIVITY_WIDTH: f32 = 48.0;

/// The status bar's height.
pub const STATUS_HEIGHT: f32 = 24.0;

/// The side panel's share of the space beside the activity bar, at the
/// start: about 270 units in a window 1,280 wide.
pub const SIDE_RATIO: f32 = 0.22;

/// The side panel's least width.
pub const SIDE_MIN_WIDTH: f32 = 160.0;

/// The least width of what is beside the side panel: the editor area and
/// the bottom panel.
pub const MAIN_MIN_WIDTH: f32 = 240.0;

/// The editor area's share of the height above the bottom panel, at the
/// start.
pub const EDITOR_RATIO: f32 = 0.7;

/// The editor area's least height.
pub const EDITOR_MIN_HEIGHT: f32 = 120.0;

/// The bottom panel's least height.
pub const BOTTOM_MIN_HEIGHT: f32 = 80.0;

/// The padding of the side panel and of the bottom panel's body.
pub const PANEL_PADDING: f32 = 12.0;

/// The space between the side panel's title and its text.
pub const PANEL_GAP: f32 = 10.0;

/// The status bar's padding at its ends.
pub const STATUS_PADDING: f32 = 12.0;

/// The space between the status bar's items.
pub const STATUS_GAP: f32 = 16.0;

/// The space between a status dot and its name.
pub const STATUS_DOT_GAP: f32 = 6.0;

/// The space between the welcome's lines.
pub const WELCOME_GAP: f32 = 6.0;

/// How long the pointer rests on a node before its tooltip shows.
pub const TOOLTIP_DELAY: Duration = Duration::from_millis(500);

/// What the title strip names while no folder is open (M4.3 opens one).
pub const NO_FOLDER: &str = "No folder";

/// The status bar's reminder of the palette's shortcut.
pub const PALETTE_HINT: &str = "Ctrl+Shift+P: commands";

/// Which panels show: at the start, as the settings say, and as the
/// commands toggle them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Panels {
    /// Whether the side panel shows.
    pub side: bool,
    /// Whether the bottom panel shows.
    pub bottom: bool,
}

impl Default for Panels {
    /// Both show.
    fn default() -> Self {
        Self {
            side: true,
            bottom: true,
        }
    }
}

/// Whether the main loop goes on after an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    /// Go on.
    Continue,
    /// The user asked to close the window: end Tidepool.
    Quit,
}

/// The nodes of the shell's parts.
#[derive(Clone, Copy, Debug)]
struct Ids {
    activity: NodeId,
    buttons: [NodeId; 6],
    side_split: NodeId,
    side: NodeId,
    side_title: NodeId,
    side_text: NodeId,
    bottom_split: NodeId,
    editor: NodeId,
    bottom: NodeId,
    tabs: NodeId,
    bottom_text: NodeId,
    status: NodeId,
}

/// The palette while it is open: its root, and the commands its rows show,
/// in their order.
#[derive(Clone, Debug)]
struct OpenPalette {
    node: NodeId,
    commands: Vec<Command>,
}

/// The shell around the editor (the [module documentation](self)).
#[derive(Debug)]
pub struct Shell {
    ids: Ids,
    view: View,
    tab: PanelTab,
    panels: Panels,
    palette: Option<OpenPalette>,
    /// The tooltip the shell opened, if it is open.
    tooltip: Option<NodeId>,
    /// The tooltip target last seen.
    seen: Option<TooltipTarget>,
    /// When the target's tooltip opens, unless the target changes first.
    due: Option<Instant>,
    /// The shell's commands, which the palette lists.
    registry: Registry,
    /// The keymap engine that keys no widget handled go to.
    keys: Engine,
}

impl Shell {
    /// Builds the shell in `tree`, under its main root, with the Explorer
    /// view and the Problems tab selected, `panels` showing, and the
    /// default keymap ([`Keymap::bundled`]).
    ///
    /// # Errors
    ///
    /// The tree's errors, as when it has too few nodes left
    /// ([`UiError::TooManyNodes`]).
    pub fn new(tree: &mut Tree, panels: Panels) -> Result<Self, UiError> {
        let root = tree.root();
        tree.set_style(root, Style::column().fill())?;
        tree.insert(
            root,
            Node::new(
                Style::default()
                    .width(Length::Fill(1.0))
                    .height(Length::Fixed(TITLE_HEIGHT)),
            )
            .with_semantics(Semantics::new(Role::Heading, "Tidepool"))
            .with_widget(TitleStrip::new(NO_FOLDER)),
        )?;
        let middle = tree.insert(root, Node::new(Style::row().fill()))?;
        let activity = tree.insert(
            middle,
            Node::new(
                Style::column()
                    .width(Length::Fixed(ACTIVITY_WIDTH))
                    .height(Length::Fill(1.0)),
            )
            .with_semantics(Semantics::new(Role::TabList, "Views"))
            .with_widget(Surface {
                fill: Some(Token::Panel),
                edges: Edges::RIGHT,
            }),
        )?;
        let view = View::Explorer;
        let mut buttons = [activity; 6];
        for (slot, button) in buttons.iter_mut().zip(View::ALL) {
            *slot = tree.insert(activity, ActivityButton::new(button, button == view).node())?;
            tree.set_tooltip(*slot, button.name())?;
        }

        let side = Node::new(
            Style::column()
                .fill()
                .min(Size::new(SIDE_MIN_WIDTH, 0.0))
                .padding(Insets::all(PANEL_PADDING))
                .gap(PANEL_GAP)
                .clip(),
        )
        .with_semantics(Semantics::new(Role::Pane, view.name()))
        // No hairline of its own: the splitter's rule divides it from the
        // rest.
        .with_widget(Surface {
            fill: Some(Token::Panel),
            edges: Edges::NONE,
        });
        let main = Node::new(Style::column().fill().min(Size::new(MAIN_MIN_WIDTH, 0.0)));
        let side_split = Splitter::new(Axis::Horizontal, "Side panel", SIDE_RATIO)
            .insert(tree, middle, side, main)?;
        let [side, _, main] = three(tree, side_split)?;
        let side_title = tree.insert(side, SectionTitle::new(view.title()).node())?;
        let side_text = tree.insert(
            side,
            Node::new(Style::default()).with_widget(Label::new(
                view.empty_text(),
                ROW_TEXT,
                Token::Text2,
            )),
        )?;

        let editor = Node::new(
            Style::column()
                .fill()
                .min(Size::new(0.0, EDITOR_MIN_HEIGHT))
                .align(Align::Center, Align::Center)
                .gap(WELCOME_GAP)
                .clip(),
        )
        .with_semantics(Semantics::new(Role::Pane, "Editor"));
        let bottom = Node::new(
            Style::column()
                .fill()
                .min(Size::new(0.0, BOTTOM_MIN_HEIGHT))
                .clip(),
        )
        .with_semantics(Semantics::new(Role::Pane, "Bottom panel"))
        // As the side panel, divided from the editor area by the splitter.
        .with_widget(Surface {
            fill: Some(Token::Panel),
            edges: Edges::NONE,
        });
        let bottom_split = Splitter::new(Axis::Vertical, "Bottom panel", EDITOR_RATIO)
            .insert(tree, main, editor, bottom)?;
        let [editor, _, bottom] = three(tree, bottom_split)?;
        tree.insert(editor, Kicker::new("No folder open").node())?;
        tree.insert(
            editor,
            Node::new(Style::default()).with_widget(Label::new(
                "Tidepool",
                CARD_TITLE,
                Token::Text,
            )),
        )?;
        tree.insert(
            editor,
            Node::new(Style::default()).with_widget(Label::new(
                "Ctrl+Shift+P opens the command palette.",
                ROW_TEXT,
                Token::Text2,
            )),
        )?;
        let tab = PanelTab::Problems;
        let tabs = tree.insert(
            bottom,
            TabStrip::new(
                "Bottom panel",
                TabKind::Chrome,
                PanelTab::ALL.map(|tab| Tab::new(tab.label())),
            )
            .node(),
        )?;
        let body = tree.insert(
            bottom,
            Node::new(Style::column().padding(Insets::all(PANEL_PADDING))),
        )?;
        let bottom_text = tree.insert(
            body,
            Node::new(Style::default()).with_widget(Label::new(
                tab.empty_text(),
                ROW_TEXT,
                Token::Text2,
            )),
        )?;

        let status = tree.insert(
            root,
            Node::new(
                Style::row()
                    .width(Length::Fill(1.0))
                    .height(Length::Fixed(STATUS_HEIGHT))
                    .padding(Insets::symmetric(STATUS_PADDING, 0.0))
                    .gap(STATUS_GAP)
                    .align(Align::Start, Align::Center)
                    .clip(),
            )
            .with_semantics(Semantics::new(Role::StatusBar, "Status"))
            .with_widget(Surface {
                fill: Some(Token::Bar),
                edges: Edges::TOP,
            }),
        )?;
        for name in ["rust-analyzer", "git"] {
            let item = tree.insert(
                status,
                Node::new(
                    Style::row()
                        .gap(STATUS_DOT_GAP)
                        .align(Align::Start, Align::Center),
                ),
            )?;
            tree.insert(item, StatusDot::new(name, Status::Off).node())?;
            tree.insert(
                item,
                Node::new(Style::default()).with_widget(Label::new(name, PATH, Token::Text2)),
            )?;
        }
        tree.insert(status, Node::new(Style::default().width(Length::Fill(1.0))))?;
        tree.insert(
            status,
            Node::new(Style::default()).with_widget(Label::new(PALETTE_HINT, PATH, Token::Text2)),
        )?;

        let registry = commands::registry();
        let keys = Engine::new(Keymap::bundled(&registry, None));
        let mut shell = Self {
            ids: Ids {
                activity,
                buttons,
                side_split,
                side,
                side_title,
                side_text,
                bottom_split,
                editor,
                bottom,
                tabs,
                bottom_text,
                status,
            },
            view,
            tab,
            panels: Panels::default(),
            palette: None,
            tooltip: None,
            seen: None,
            due: None,
            registry,
            keys,
        };
        shell.set_side(tree, panels.side)?;
        shell.set_bottom(tree, panels.bottom)?;
        Ok(shell)
    }

    /// The view the side panel shows, or shows when it is shown again.
    pub const fn view(&self) -> View {
        self.view
    }

    /// The bottom panel's selected tab.
    pub const fn tab(&self) -> PanelTab {
        self.tab
    }

    /// Which panels show.
    pub const fn panels(&self) -> Panels {
        self.panels
    }

    /// The palette's root while it is open.
    pub fn palette(&self) -> Option<NodeId> {
        self.palette.as_ref().map(|open| open.node)
    }

    /// The tooltip the shell has open, if any.
    pub const fn tooltip(&self) -> Option<NodeId> {
        self.tooltip
    }

    /// The activity bar's button of `view`.
    pub const fn button(&self, view: View) -> NodeId {
        self.ids.buttons[view.index()]
    }

    /// The node of an area: the activity bar, the side panel, the editor
    /// area, the bottom panel or the status bar, for tests and frontends
    /// that ask where they are.
    pub const fn area(&self, area: Area) -> NodeId {
        match area {
            Area::ActivityBar => self.ids.activity,
            Area::SidePanel => self.ids.side,
            Area::Editor => self.ids.editor,
            Area::BottomPanel => self.ids.bottom,
            Area::StatusBar => self.ids.status,
        }
    }

    /// The shell's commands, each with its id and title: what its palette
    /// lists, and what a keymap given to [`set_keymap`](Self::set_keymap)
    /// is read against.
    pub const fn registry(&self) -> &Registry {
        &self.registry
    }

    /// The keymap engine that keys no widget handled go to: its keymap,
    /// mode and pending steps.
    pub const fn keymap_engine(&self) -> &Engine {
        &self.keys
    }

    /// Uses `keymap` for the keys no widget handles, in place of the one
    /// before, in its start mode with nothing pending. A binding of a
    /// command that is not the shell's does nothing here.
    pub fn set_keymap(&mut self, keymap: Keymap) {
        self.keys = Engine::new(keymap);
    }

    /// When the shell's next timer fires: the tooltip's, if one is due.
    /// The main loop waits no longer than this.
    pub const fn next_deadline(&self) -> Option<Instant> {
        self.due
    }

    /// Gives `event` to `tree`, then acts on what came of it, as the
    /// [module documentation](self#events-commands-and-timers) says, at
    /// `now`. Returns [`Flow::Quit`] if the user asked to close the window.
    ///
    /// # Errors
    ///
    /// The tree's errors, which would be a bug in the shell.
    pub fn handle(
        &mut self,
        tree: &mut Tree,
        event: &InputEvent,
        now: Instant,
    ) -> Result<Flow, UiError> {
        let response = tree.handle(event);
        let mut flow = Flow::Continue;
        for &action in &response.actions {
            if self.act(tree, action)? == Flow::Quit {
                flow = Flow::Quit;
            }
        }
        if !response.handled
            && !response.blocked
            && let Some(command) = self.shortcut(event)
            && self.run(tree, command)? == Flow::Quit
        {
            flow = Flow::Quit;
        }
        self.update(tree, now)?;
        Ok(flow)
    }

    /// Follows the tree's tooltip target and fires the tooltip's timer at
    /// `now`: closes the tooltip when the target changes and sets the timer
    /// for the new target, [`TOOLTIP_DELAY`] for the pointer and none for
    /// focus; opens it when the timer has fired. Call it after each event,
    /// which [`handle`](Self::handle) does, after each frame, since a
    /// layout can move nodes under the pointer, and when the deadline
    /// passes. Returns whether it opened or closed a tooltip, after which
    /// the tree has a new frame to draw.
    ///
    /// # Errors
    ///
    /// The tree's errors, which would be a bug in the shell.
    pub fn update(&mut self, tree: &mut Tree, now: Instant) -> Result<bool, UiError> {
        let mut changed = false;
        let target = tree.tooltip_target();
        if target != self.seen {
            if let Some(tip) = self.tooltip.take()
                && tree.contains(tip)
            {
                tree.close_overlay(tip)?;
                changed = true;
            }
            self.seen = target;
            self.due = target.map(|target| match target.cause {
                TooltipCause::Pointer => now + TOOLTIP_DELAY,
                TooltipCause::Focus => now,
            });
        }
        if let (Some(due), Some(target)) = (self.due, self.seen)
            && now >= due
        {
            self.due = None;
            if self.tooltip.is_none() {
                self.tooltip = Some(Tooltip::open(tree, target.node)?);
                changed = true;
            }
        }
        Ok(changed)
    }

    /// Runs `command`.
    ///
    /// # Errors
    ///
    /// The tree's errors, which would be a bug in the shell.
    pub fn run(&mut self, tree: &mut Tree, command: Command) -> Result<Flow, UiError> {
        match command {
            Command::Show(view) => self.show(tree, view)?,
            Command::ShowTab(tab) => {
                self.select_tab(tree, tab)?;
                self.set_bottom(tree, true)?;
            }
            Command::ToggleSidePanel => self.set_side(tree, !self.panels.side)?,
            Command::ToggleBottomPanel => self.set_bottom(tree, !self.panels.bottom)?,
            Command::CloseWindow => return Ok(Flow::Quit),
            Command::OpenPalette => self.open_palette(tree)?,
        }
        Ok(Flow::Continue)
    }

    /// Opens the palette, listing every command, unless it is open.
    ///
    /// # Errors
    ///
    /// The tree's errors, which would be a bug in the shell.
    pub fn open_palette(&mut self, tree: &mut Tree) -> Result<(), UiError> {
        if self.palette.is_some() {
            return Ok(());
        }
        let node = Palette::new("Command palette")
            .placeholder("Type a command")
            .empty_text("No commands match.")
            .open(tree)?;
        self.palette = Some(OpenPalette {
            node,
            commands: Vec::new(),
        });
        self.rank(tree)
    }

    /// Acts on one of the widgets' actions.
    fn act(&mut self, tree: &mut Tree, action: Action) -> Result<Flow, UiError> {
        if let Some(view) = View::ALL
            .into_iter()
            .find(|&view| self.button(view) == action.node)
        {
            if action.kind == ActionKind::Activate {
                if view == self.view && self.panels.side {
                    self.set_side(tree, false)?;
                } else {
                    self.show(tree, view)?;
                }
            }
            return Ok(Flow::Continue);
        }
        if action.node == self.ids.tabs && action.kind == ActionKind::Select {
            let selected = tree
                .widget::<TabStrip>(self.ids.tabs)
                .and_then(TabStrip::selected)
                .and_then(|index| PanelTab::ALL.get(index).copied());
            if let Some(tab) = selected {
                self.select_tab(tree, tab)?;
            }
            return Ok(Flow::Continue);
        }
        let Some(open) = &self.palette else {
            return Ok(Flow::Continue);
        };
        let node = open.node;
        if Palette::field(tree, node) == Some(action.node) && action.kind == ActionKind::Change {
            self.rank(tree)?;
        } else if action.node == node {
            match action.kind {
                ActionKind::Activate => {
                    let command = Palette::selected(tree, node)
                        .and_then(|index| open.commands.get(index).copied());
                    self.close_palette(tree)?;
                    if let Some(command) = command {
                        return self.run(tree, command);
                    }
                }
                ActionKind::Dismiss => self.close_palette(tree)?,
                _ => {}
            }
        }
        Ok(Flow::Continue)
    }

    /// The shell's command that `event` resolves to in its keymap, if any:
    /// on a key going down, not on an auto-repeat.
    fn shortcut(&mut self, event: &InputEvent) -> Option<Command> {
        match self.keys.handle(event) {
            Resolution::Command { id, repeat: false } => Command::from_id(id),
            _ => None,
        }
    }

    /// Ranks the registry's commands for the open palette's query, and
    /// gives it them.
    fn rank(&mut self, tree: &mut Tree) -> Result<(), UiError> {
        let Some(open) = &mut self.palette else {
            return Ok(());
        };
        let query = Palette::query(tree, open.node)
            .unwrap_or_default()
            .to_string();
        let (rows, ranked): (Vec<Row>, Vec<Command>) = self
            .registry
            .rank(&query)
            .into_iter()
            .filter_map(|ranked| {
                let command = Command::from_id(ranked.entry.id())?;
                Some((
                    Row::new(ranked.entry.title()).matched(ranked.matched),
                    command,
                ))
            })
            .unzip();
        open.commands = ranked;
        Palette::set_items(tree, open.node, rows)
    }

    /// Closes the palette, if it is open.
    fn close_palette(&mut self, tree: &mut Tree) -> Result<(), UiError> {
        if let Some(open) = self.palette.take()
            && tree.contains(open.node)
        {
            tree.close_overlay(open.node)?;
        }
        Ok(())
    }

    /// Shows `view` in the side panel, and the panel if it is hidden.
    fn show(&mut self, tree: &mut Tree, view: View) -> Result<(), UiError> {
        if view != self.view {
            for (button, id) in View::ALL.into_iter().zip(self.ids.buttons) {
                widgets::update::<ActivityButton, _>(tree, id, |b| b.set_active(button == view))?;
            }
            // A section title's text is set when it is made.
            tree.remove(self.ids.side_title)?;
            self.ids.side_title =
                tree.insert_at(self.ids.side, 0, SectionTitle::new(view.title()).node())?;
            set_label(tree, self.ids.side_text, view.empty_text());
            tree.set_semantics(self.ids.side, Semantics::new(Role::Pane, view.name()))?;
            self.view = view;
        }
        self.set_side(tree, true)
    }

    /// Selects `tab` in the bottom panel.
    fn select_tab(&mut self, tree: &mut Tree, tab: PanelTab) -> Result<(), UiError> {
        if tree
            .widget::<TabStrip>(self.ids.tabs)
            .and_then(TabStrip::selected)
            != Some(tab.index())
        {
            widgets::update::<TabStrip, _>(tree, self.ids.tabs, |strip| strip.select(tab.index()))?;
        }
        set_label(tree, self.ids.bottom_text, tab.empty_text());
        self.tab = tab;
        Ok(())
    }

    /// Shows the side panel, or hides it.
    fn set_side(&mut self, tree: &mut Tree, shown: bool) -> Result<(), UiError> {
        self.panels.side = shown;
        let split = self.ids.side_split;
        show_pane(
            tree,
            split,
            self.ids.side,
            shown,
            Axis::Horizontal,
            SIDE_RATIO,
        )
    }

    /// Shows the bottom panel, or hides it.
    fn set_bottom(&mut self, tree: &mut Tree, shown: bool) -> Result<(), UiError> {
        self.panels.bottom = shown;
        let split = self.ids.bottom_split;
        show_pane(
            tree,
            split,
            self.ids.bottom,
            shown,
            Axis::Vertical,
            EDITOR_RATIO,
        )
    }
}

/// An area of the shell ([`Shell::area`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Area {
    /// The activity bar.
    ActivityBar,
    /// The side panel.
    SidePanel,
    /// The editor area.
    Editor,
    /// The bottom panel.
    BottomPanel,
    /// The status bar.
    StatusBar,
}

/// The three children of a split made by [`Splitter::insert`]: the first
/// pane, the divider and the second pane.
fn three(tree: &Tree, split: NodeId) -> Result<[NodeId; 3], UiError> {
    match tree.children(split) {
        Some(&[first, divider, second]) => Ok([first, divider, second]),
        _ => Err(UiError::UnknownNode { id: split }),
    }
}

/// Shows or hides `pane` and its split's divider. A split whose pane is
/// hidden becomes a plain row or column, so the other pane fills it; shown
/// again, it is a split with its divider's ratio (or `ratio`).
fn show_pane(
    tree: &mut Tree,
    split: NodeId,
    pane: NodeId,
    shown: bool,
    axis: Axis,
    ratio: f32,
) -> Result<(), UiError> {
    let divider = Splitter::divider(tree, split).ok_or(UiError::UnknownNode { id: split })?;
    for id in [pane, divider] {
        let style = *tree.style(id).ok_or(UiError::UnknownNode { id })?;
        if style.hidden == shown {
            tree.set_style(
                id,
                Style {
                    hidden: !shown,
                    ..style
                },
            )?;
        }
    }
    let ratio = tree
        .widget::<Splitter>(divider)
        .map_or(ratio, Splitter::ratio);
    let layout = if shown {
        Style::split(axis, ratio).fill()
    } else {
        match axis {
            Axis::Horizontal => Style::row().fill(),
            Axis::Vertical => Style::column().fill(),
        }
    };
    let current = *tree
        .style(split)
        .ok_or(UiError::UnknownNode { id: split })?;
    if current.layout != layout.layout {
        tree.set_style(
            split,
            Style {
                layout: layout.layout,
                ..current
            },
        )?;
    }
    Ok(())
}
