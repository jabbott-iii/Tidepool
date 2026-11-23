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

//! The Tidepool binary: main loop, wiring, startup and crash handling.
//!
//! Specification: `docs/01-architecture.md` → "The main loop".
//!
//! It opens the GUI's window and runs the event-driven main loop (M3.8,
//! ADR-0012, ADR-0059) until the window is closed. Each turn waits on the
//! window until something happens or the earliest deadline passes (only
//! `gui`'s for now: the most the first present waits for the backdrop),
//! hands every input event to the `ui` tree before anything else, stops at
//! a close request, and then renders, which presents only what changed. So
//! an idle Tidepool waits and draws nothing. The tree shows the first
//! frame's lines (`tidepool_gui::first_frame`) until the shell replaces
//! them (M3.4).
//!
//! `--print-startup-time` prints, on standard output, the time from the
//! start of `main` to the return of the first present (ADR-0045).

use std::fmt;
use std::io::{self, Write};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use tidepool_core::cli::Args;
use tidepool_gui::{Gui, GuiError, first_frame};
use tidepool_platform::events::Event;
use tidepool_platform::window::WindowConfig;
use tidepool_ui::Tree;

/// The exit code for a command line that was not understood.
const EXIT_USAGE: u8 = 2;

/// What the command line accepts.
const USAGE: &str = "usage: tidepool [--print-startup-time]";

fn main() -> ExitCode {
    let start = Instant::now();
    let result = Args::from_env()
        .map_err(|error| error.to_string())
        .and_then(Options::parse)
        .map_err(Error::Usage)
        .and_then(|options| run(&options, start, &mut io::stdout()));
    // If standard error is closed, there is nobody left to tell; the exit
    // status still says what happened.
    let mut err = io::stderr();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error @ Error::Usage(_)) => {
            let _ = writeln!(err, "tidepool: {}\n{USAGE}", printable(&error.to_string()));
            ExitCode::from(EXIT_USAGE)
        }
        Err(error) => {
            let _ = writeln!(err, "tidepool: {}", printable(&error.to_string()));
            ExitCode::FAILURE
        }
    }
}

/// The command line, understood.
#[derive(Debug, PartialEq, Eq)]
struct Options {
    /// `--print-startup-time`.
    print_startup_time: bool,
}

impl Options {
    /// Reads `args`, which take no files or other options yet; an error
    /// says which argument was not understood.
    fn parse(mut args: Args) -> Result<Self, String> {
        let print_startup_time = args.flag("--print-startup-time");
        let positional = args.finish().map_err(|error| error.to_string())?;
        if let Some(argument) = positional.first() {
            return Err(format!(
                "unexpected argument `{argument}`: Tidepool opens no files yet"
            ));
        }
        Ok(Self { print_startup_time })
    }
}

/// Why Tidepool stopped.
#[derive(Debug)]
enum Error {
    /// The command line was not understood: what was wrong with it.
    Usage(String),
    /// The window could not be opened or drawn.
    Gui(GuiError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(error) => write!(f, "{error}"),
            Self::Gui(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Usage(_) => None,
            Self::Gui(error) => Some(error),
        }
    }
}

/// Opens the window and runs the main loop until the window is asked to
/// close, writing the startup time to `out` after the first present if
/// `options` asks.
fn run(options: &Options, start: Instant, out: &mut dyn Write) -> Result<(), Error> {
    let mut gui = Gui::open(&WindowConfig::default()).map_err(Error::Gui)?;
    let mut tree = first_frame::tree();
    let mut events = Vec::with_capacity(64);
    let mut first = true;
    loop {
        events.clear();
        gui.wait_events(gui.next_deadline(), &mut events)
            .map_err(Error::Gui)?;
        if handle(&mut tree, &events) == Flow::Quit {
            return Ok(());
        }
        let presented = gui.render(&mut tree).map_err(Error::Gui)?;
        if presented && first {
            first = false;
            if options.print_startup_time {
                // If standard output is closed, there is nobody to tell,
                // and the window stays open.
                let _ = out
                    .write_all(startup_line(start.elapsed()).as_bytes())
                    .and_then(|()| out.flush());
            }
        }
    }
}

/// Whether the loop goes on after a turn's events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flow {
    /// Wait for the next events.
    Continue,
    /// The user asked to close the window.
    Quit,
}

/// Answers a turn's `events` in order: each input event goes to `tree`,
/// which routes it to the focused node or the one under the pointer
/// (`Tree::handle`), and a close request ends the loop, leaving the events
/// after it unanswered. The rest are `gui`'s, which took them as it waited.
///
/// No widget of the first frame emits actions, and the window has no
/// shortcuts yet, so what comes back from the tree is not used until the
/// shell (M3.4).
fn handle(tree: &mut Tree, events: &[Event]) -> Flow {
    for event in events {
        match event {
            Event::Input(input) => {
                let _response = tree.handle(input);
            }
            Event::CloseRequested => return Flow::Quit,
            Event::Resized(_) | Event::Exposed(_) | Event::Woken | Event::Pasted { .. } => {}
        }
    }
    Flow::Continue
}

/// The line `--print-startup-time` prints: the time to the first frame, in
/// milliseconds to a tenth.
fn startup_line(elapsed: Duration) -> String {
    format!("first frame: {:.1} ms\n", elapsed.as_secs_f64() * 1000.0)
}

/// `text` with every control character, bidirectional control and line
/// or paragraph separator escaped (`\u{1b}`), as `xtask` escapes them, so
/// that an argument shown in a message cannot move the terminal's cursor,
/// change its colors, reorder the line or start another.
fn printable(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if is_escaped(c) {
            out.extend(c.escape_unicode());
        } else {
            out.push(c);
        }
    }
    out
}

/// Whether [`printable`] escapes `c`.
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

#[cfg(test)]
mod tests {
    use tidepool_core::geom::{DeviceRect, DeviceSize, Size};
    use tidepool_core::input::{InputEvent, Key, Modifiers};
    use tidepool_ui::metrics::FixedMetrics;
    use tidepool_ui::widgets::{Button, ButtonKind, Control};

    use super::*;

    /// A tree of two buttons, laid out, and their ids.
    fn buttons() -> (Tree, [tidepool_ui::NodeId; 2]) {
        let mut tree = Tree::new();
        let root = tree.root();
        let ids = ["OPEN", "CLOSE"].map(|label| {
            tree.insert(root, Button::new(label, ButtonKind::Ghost).node())
                .expect("a node")
        });
        let _ = tree.frame(Size::new(320.0, 200.0), &FixedMetrics::DEFAULT);
        (tree, ids)
    }

    fn tab() -> Event {
        Event::Input(InputEvent::KeyDown {
            key: Key::Tab,
            modifiers: Modifiers::NONE,
            repeat: false,
        })
    }

    fn focused(tree: &Tree, id: tidepool_ui::NodeId) -> bool {
        tree.state(id).expect("in the tree").focused
    }

    #[test]
    fn input_goes_to_the_tree_in_order() {
        let (mut tree, [open, close]) = buttons();
        assert_eq!(handle(&mut tree, &[tab()]), Flow::Continue);
        assert!(focused(&tree, open) && !focused(&tree, close));
        assert_eq!(
            handle(
                &mut tree,
                &[
                    Event::Exposed(DeviceRect::new(0, 0, 8, 8)),
                    Event::Resized(DeviceSize::new(640, 400)),
                    Event::Woken,
                    tab(),
                ]
            ),
            Flow::Continue,
            "the events gui takes are passed over"
        );
        assert!(focused(&tree, close) && !focused(&tree, open));
    }

    #[test]
    fn a_close_request_ends_the_loop_before_the_events_after_it() {
        let (mut tree, [open, _]) = buttons();
        assert_eq!(
            handle(&mut tree, &[Event::CloseRequested, tab()]),
            Flow::Quit
        );
        assert!(!focused(&tree, open), "the Tab after it went nowhere");
    }

    fn parse(args: &[&str]) -> Result<Options, String> {
        Options::parse(Args::new(args.iter().copied()).expect("within the limits"))
    }

    #[test]
    fn the_command_line_takes_only_the_startup_flag() {
        assert_eq!(
            parse(&[]),
            Ok(Options {
                print_startup_time: false
            })
        );
        assert_eq!(
            parse(&["--print-startup-time"]),
            Ok(Options {
                print_startup_time: true
            })
        );
        assert_eq!(
            parse(&["--tui"]),
            Err("unexpected argument `--tui` (argument 1)".to_string())
        );
        assert_eq!(
            parse(&["--print-startup-time", "notes.md"]),
            Err("unexpected argument `notes.md`: Tidepool opens no files yet".to_string())
        );
        assert!(parse(&["--", "--print-startup-time"]).is_err());
    }

    #[test]
    fn the_startup_line_is_in_milliseconds_to_a_tenth() {
        assert_eq!(
            startup_line(Duration::from_micros(123_456)),
            "first frame: 123.5 ms\n"
        );
        assert_eq!(startup_line(Duration::ZERO), "first frame: 0.0 ms\n");
    }

    #[test]
    fn control_characters_are_shown_escaped() {
        assert_eq!(printable("a\u{1b}[31mb\n"), "a\\u{1b}[31mb\\u{a}");
        assert_eq!(printable("plain `--tui`"), "plain `--tui`");
    }

    #[test]
    fn bidirectional_controls_and_separators_are_shown_escaped() {
        assert_eq!(printable("abc\u{202e}fed"), "abc\\u{202e}fed");
        assert_eq!(printable("\u{2066}x\u{2069}"), "\\u{2066}x\\u{2069}");
        assert_eq!(printable("a\u{2028}b"), "a\\u{2028}b");
        assert_eq!(printable("مرحبا é"), "مرحبا é", "other text is left alone");
    }
}
