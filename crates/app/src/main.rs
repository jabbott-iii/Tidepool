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
//! It reads the settings (read-only until M4.7, [`settings`]), opens the
//! GUI's window, builds the shell in a `ui` tree (`editor::shell`, M3.4,
//! ADR-0060) with the activity bar's icons registered with the window, and
//! runs the event-driven main loop (M3.8, ADR-0012, ADR-0059) until the
//! window is closed. Each turn waits on the window until something happens
//! or the earliest deadline passes (`gui`'s, the most the first present
//! waits for the backdrop, and the shell's, a tooltip's delay), hands every
//! input event to the shell, which gives it to the tree before anything
//! else, stops at a close request, fires the shell's timer, and then
//! renders, which presents only what changed. So an idle Tidepool waits and
//! draws nothing.
//!
//! `--print-startup-time` prints, on standard output, the time from the
//! start of `main` to the return of the first present (ADR-0045).

mod settings;

use std::fmt;
use std::io::{self, Write};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use tidepool_core::cli::Args;
use tidepool_editor::shell::{Flow, Shell, View};
use tidepool_gui::{Gui, GuiError, Icon, icons};
use tidepool_platform::events::Event;
use tidepool_platform::window::WindowConfig;
use tidepool_ui::{Tree, UiError};

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
    /// The shell's tree refused a change, which would be a bug.
    Ui(UiError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(error) => write!(f, "{error}"),
            Self::Gui(error) => write!(f, "{error}"),
            Self::Ui(error) => write!(f, "the shell failed: {error}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Usage(_) => None,
            Self::Gui(error) => Some(error),
            Self::Ui(error) => Some(error),
        }
    }
}

/// Reads the settings, opens the window, builds the shell and runs the
/// main loop until the window is asked to close, writing the startup time
/// to `out` after the first present if `options` asks.
fn run(options: &Options, start: Instant, out: &mut dyn Write) -> Result<(), Error> {
    let settings = settings::load();
    let mut gui = Gui::open(&WindowConfig::default()).map_err(Error::Gui)?;
    for view in View::ALL {
        gui.set_icon(view.icon(), icon(view));
    }
    let mut tree = Tree::new();
    let mut shell = Shell::new(&mut tree, settings.panels).map_err(Error::Ui)?;
    let mut events = Vec::with_capacity(64);
    let mut first = true;
    loop {
        events.clear();
        let deadline = earliest(gui.next_deadline(), shell.next_deadline());
        gui.wait_events(deadline, &mut events).map_err(Error::Gui)?;
        if handle(&mut tree, &mut shell, &events, Instant::now()).map_err(Error::Ui)? == Flow::Quit
        {
            return Ok(());
        }
        shell.update(&mut tree, Instant::now()).map_err(Error::Ui)?;
        let mut presented = gui.render(&mut tree).map_err(Error::Gui)?;
        // A layout can move a node under the pointer at rest, which opens
        // or closes a tooltip: drawn at once, not at the next event.
        if shell.update(&mut tree, Instant::now()).map_err(Error::Ui)? {
            presented |= gui.render(&mut tree).map_err(Error::Gui)?;
        }
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

/// The outline `view`'s button shows, from `gui`'s icons (ADR-0060).
fn icon(view: View) -> Icon {
    match view {
        View::Explorer => icons::files(),
        View::Search => icons::search(),
        View::SourceControl => icons::branch(),
        View::Toolchains => icons::wrench(),
        View::Devices => icons::device(),
        View::Agents => icons::spark(),
    }
}

/// The earlier of two deadlines, `None` meaning none.
fn earliest(a: Option<Instant>, b: Option<Instant>) -> Option<Instant> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// Answers a turn's `events` in order, at `now`: each input event goes to
/// the shell, which gives it to the tree first, and a close request, or a
/// command that closes the window, ends the loop, leaving the events after
/// it unanswered. The rest are `gui`'s, which took them as it waited.
fn handle(
    tree: &mut Tree,
    shell: &mut Shell,
    events: &[Event],
    now: Instant,
) -> Result<Flow, UiError> {
    for event in events {
        match event {
            Event::Input(input) => {
                if shell.handle(tree, input, now)? == Flow::Quit {
                    return Ok(Flow::Quit);
                }
            }
            Event::CloseRequested => return Ok(Flow::Quit),
            Event::Resized(_) | Event::Exposed(_) | Event::Woken | Event::Pasted { .. } => {}
        }
    }
    Ok(Flow::Continue)
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
    use tidepool_editor::shell::Panels;
    use tidepool_ui::metrics::FixedMetrics;

    use super::*;

    fn parse(args: &[&str]) -> Result<Options, String> {
        Options::parse(Args::new(args.iter().copied()).expect("within the limits"))
    }

    /// The shell in a tree, laid out.
    fn shell() -> (Tree, Shell) {
        let mut tree = Tree::new();
        let shell = Shell::new(&mut tree, Panels::default()).expect("the shell builds");
        let _ = tree.frame(Size::new(1280.0, 800.0), &FixedMetrics::DEFAULT);
        (tree, shell)
    }

    fn ctrl(key: Key) -> Event {
        Event::Input(InputEvent::KeyDown {
            key,
            modifiers: Modifiers::CTRL,
            repeat: false,
        })
    }

    #[test]
    fn input_goes_to_the_shell_in_order() {
        let (mut tree, mut shell) = shell();
        let now = Instant::now();
        let flow = handle(&mut tree, &mut shell, &[ctrl(Key::B)], now).expect("handled");
        assert_eq!(flow, Flow::Continue);
        assert!(!shell.panels().side);
        let flow = handle(
            &mut tree,
            &mut shell,
            &[
                Event::Exposed(DeviceRect::new(0, 0, 8, 8)),
                Event::Resized(DeviceSize::new(640, 400)),
                Event::Woken,
                ctrl(Key::B),
                ctrl(Key::J),
            ],
            now,
        )
        .expect("handled");
        assert_eq!(flow, Flow::Continue, "the events gui takes are passed over");
        assert_eq!(
            shell.panels(),
            Panels {
                side: true,
                bottom: false
            }
        );
    }

    #[test]
    fn a_close_request_ends_the_loop_before_the_events_after_it() {
        let (mut tree, mut shell) = shell();
        let flow = handle(
            &mut tree,
            &mut shell,
            &[Event::CloseRequested, ctrl(Key::B)],
            Instant::now(),
        )
        .expect("handled");
        assert_eq!(flow, Flow::Quit);
        assert!(shell.panels().side, "the Ctrl+B after it went nowhere");
    }

    #[test]
    fn the_earliest_deadline_wins() {
        let now = Instant::now();
        let later = now + Duration::from_millis(5);
        assert_eq!(earliest(None, None), None);
        assert_eq!(earliest(Some(now), None), Some(now));
        assert_eq!(earliest(None, Some(later)), Some(later));
        assert_eq!(earliest(Some(later), Some(now)), Some(now));
    }

    #[test]
    fn every_view_has_its_own_icon() {
        let mut seen: Vec<Icon> = Vec::new();
        for view in View::ALL {
            let icon = icon(view);
            assert!(!seen.contains(&icon), "{view:?}");
            seen.push(icon);
        }
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
